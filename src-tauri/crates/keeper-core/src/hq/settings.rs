//! Per-session model and effort choices (109.17) and the composer's pickers
//! over them (109.18). No provider logic lives here.

use std::collections::{BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use ts_rs::TS;

use super::sessions::AgentSessionRef;
use crate::bots::chat::Effort;
use crate::hq::live;
use crate::hq::port::{Rejection, ReplicaError, RowOp, WindowBase, CONFLICT};
use crate::hq::sessions::{authored, int, json, names, object, opt_text, read, Source};
use crate::hq::{Hq, HqError};

const TABLE: &str = "session_settings";

/// One published definition's defaults, kept per instance rather than guessing
/// a common default when instances disagree. Internal: the pickers cross IPC
/// as [`AgentSessionPickerVm`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSettingsDefaultVm {
    pub client_id: String,
    pub model: Option<String>,
    pub effort: Option<Effort>,
}

/// Picker inputs and the saved selection. Null means definition defaults;
/// changing either selection affects future turns, never a running turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSessionSettingsVm {
    pub model: Option<String>,
    pub effort: Option<Effort>,
    pub models: Vec<String>,
    pub efforts: Vec<Effort>,
    pub defaults: Vec<AgentSettingsDefaultVm>,
}

fn sentence(error: impl std::fmt::Display) -> HqError {
    HqError::Sentence(error.to_string())
}

fn row(source: &Source, session: &str) -> Result<Option<Map<String, Value>>, HqError> {
    let (rows, pending) = source.replica.query_covered(
        "SELECT model, effort, _sync_version AS version FROM session_settings WHERE session_id = ?1",
        &[session.into()],
        &WindowBase { table: "session_settings".to_owned(), variable: "session".to_owned(), fixed_scopes: Vec::new() },
        &[session.to_owned()],
    ).map_err(sentence)?;
    if !pending.is_empty() {
        return Err(HqError::Arriving(
            "The session's settings are still arriving.".to_owned(),
        ));
    }
    Ok(rows.into_iter().next())
}

/// What `agent` offers in `source`'s database: the union of its published
/// profiles, and each instance's defaults.
struct Offered {
    models: BTreeSet<String>,
    efforts: BTreeSet<Effort>,
    defaults: Vec<AgentSettingsDefaultVm>,
}

fn offered(source: &Source, agent: &str) -> Result<Offered, ReplicaError> {
    let profiles = source.replica.query(
        "SELECT id, models, efforts, default_model, default_effort FROM agent_profiles WHERE actor_id = ?1 ORDER BY id",
        &[agent.into()],
    )?;
    let mut offered = Offered {
        models: BTreeSet::new(),
        efforts: BTreeSet::new(),
        defaults: Vec::new(),
    };
    for profile in profiles {
        offered.models.extend(names(&profile, "models"));
        offered.efforts.extend(
            names(&profile, "efforts")
                .iter()
                .filter_map(|name| Effort::parse(name)),
        );
        offered.defaults.push(AgentSettingsDefaultVm {
            client_id: opt_text(&profile, "id").unwrap_or_default(),
            model: opt_text(&profile, "default_model"),
            effort: opt_text(&profile, "default_effort").and_then(|s| Effort::parse(&s)),
        });
    }
    Ok(offered)
}

/// The agents of `source`'s database that offer more than one model — the
/// union of their profiles, as [`offered`] reads it: there is a model to
/// choose, so the Agents list's right-click menu offers "Model…" (A109-05).
/// One model, or none, is nothing to choose.
pub(crate) fn agents_choosing_model(source: &Source) -> HashSet<String> {
    let mut models: HashMap<String, BTreeSet<String>> = HashMap::new();
    for profile in read(source, "SELECT actor_id, models FROM agent_profiles", &[]) {
        if let Some(agent) = opt_text(&profile, "actor_id") {
            models
                .entry(agent)
                .or_default()
                .extend(names(&profile, "models"));
        }
    }
    models
        .into_iter()
        .filter(|(_, offered)| offered.len() > 1)
        .map(|(agent, _)| agent)
        .collect()
}

fn view(source: &Source, session: &str) -> Result<AgentSessionSettingsVm, HqError> {
    let agent = source.replica.query(
        "SELECT s.agent_actor FROM sessions s JOIN session_members m ON m.session_id = s.id WHERE s.id = ?1 AND m.actor_id = ?2 AND m.role IN ('owner', 'person')",
        &[session.into(), source.actor.as_str().into()],
    ).map_err(sentence)?.first().and_then(|r| opt_text(r, "agent_actor"))
        .ok_or_else(|| sentence("Only a person in this conversation can change its model and effort."))?;
    let offered = offered(source, &agent).map_err(sentence)?;
    let saved = row(source, session)?;
    Ok(AgentSessionSettingsVm {
        model: saved.as_ref().and_then(|r| opt_text(r, "model")),
        effort: saved
            .as_ref()
            .and_then(|r| opt_text(r, "effort"))
            .and_then(|s| Effort::parse(&s)),
        models: offered.models.into_iter().collect(),
        efforts: offered.efforts.into_iter().collect(),
        defaults: offered.defaults,
    })
}

impl Hq {
    /// Change one choice, or reset both to the definition defaults. The
    /// server remains the authority for membership and offers; its later
    /// refusal reaches the picker through [`AgentSessionPickerVm::write`].
    pub fn change_session_settings(
        &self,
        key: &AgentSessionRef,
        what: AgentSettingsChange,
    ) -> Result<AgentSettingsChangedVm, HqError> {
        let source = self.source(&key.database)?;
        let changed = change(&source, &key.session_id, what, super::sessions::now_ms())?;
        self.touch();
        Ok(changed)
    }
}

/// A person's choice of model and effort for an agent, as a session holds it:
/// each `None` for the agent's default. This device keeps the last one per
/// agent (a cookie lens, like the favourites) and hands it to the next
/// session it opens with that agent (Q109-16 b), which writes only what the
/// agent still offers ([`initial`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentSettingsPick {
    pub model: Option<String>,
    pub effort: Option<Effort>,
}

/// What a change wrote, and the sentence it owes the person now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentSettingsChangedVm {
    /// While a turn runs: the change applies from the next answer.
    pub note: Option<String>,
    /// The session's choice after the change: the device's last pick for
    /// this agent from now on. Reset writes both `None`.
    pub pick: AgentSettingsPick,
}

/// One change a person makes in the pickers or with `/model` and `/effort`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum AgentSettingsChange {
    /// Choose one of the offered models; the effort stays as chosen.
    Model { model: String },
    /// Choose one of the offered efforts; the model stays as chosen.
    Effort { effort: Effort },
    /// Back to the agent's defaults: both choices written as null.
    Reset,
}

/// What the next answer uses while a turn is still being written.
pub const RUNNING: &str =
    "This answer keeps its model and effort; your change applies from the next answer.";

/// The sentence a change owes the person now: only while a turn runs.
fn running(source: &Source, session: &str) -> Option<String> {
    let turns = live::running_turns(source, session).unwrap_or_else(|error| {
        tracing::debug!(database = %source.database, %error, "the session's running turns could not be read");
        Vec::new()
    });
    (!turns.is_empty()).then(|| RUNNING.to_owned())
}

/// `words` as a person reads a list: `a`, `a or b`, `a, b or c`.
fn or_list(words: &[&str]) -> String {
    match words {
        [] => String::new(),
        [only] => (*only).to_owned(),
        [first @ .., last] => format!("{} or {last}", first.join(", ")),
    }
}

/// Apply `change` over what is saved. A saved half the agent no longer
/// offers is written back as null rather than refused: the runtime already
/// answers with the default for it.
pub(crate) fn change(
    source: &Source,
    session: &str,
    change: AgentSettingsChange,
    now: i64,
) -> Result<AgentSettingsChangedVm, HqError> {
    let offered = view(source, session)?;
    let kept_model = offered.model.clone().filter(|m| offered.models.contains(m));
    let kept_effort = offered.effort.filter(|e| offered.efforts.contains(e));
    let (model, effort) = match change {
        AgentSettingsChange::Model { model } => {
            if !offered.models.contains(&model) {
                let names: Vec<&str> = offered.models.iter().map(String::as_str).collect();
                return Err(sentence(if names.is_empty() {
                    format!(
                        "{model} is not offered: this conversation's agent offers no model choice."
                    )
                } else {
                    format!("{model} is not offered here; choose {}.", or_list(&names))
                }));
            }
            (Some(model), kept_effort)
        }
        AgentSettingsChange::Effort { effort } => {
            if !offered.efforts.contains(&effort) {
                return Err(effort_refusal(effort_word(effort), &offered.efforts));
            }
            (kept_model, Some(effort))
        }
        AgentSettingsChange::Reset => (None, None),
    };
    set(source, session, model.as_deref(), effort, now)?;
    Ok(AgentSettingsChangedVm {
        note: running(source, session),
        pick: AgentSettingsPick { model, effort },
    })
}

/// The word a person types for `effort`: the menu's label, in lower case.
pub(crate) fn effort_word(effort: Effort) -> String {
    effort_label(effort).to_lowercase()
}

/// Why `word` is not an effort this agent takes, naming the ones it does.
fn effort_refusal(word: impl std::fmt::Display, efforts: &[Effort]) -> HqError {
    let words: Vec<String> = efforts.iter().map(|&e| effort_word(e)).collect();
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    sentence(if words.is_empty() {
        "This conversation's agent offers no effort choice.".to_owned()
    } else {
        format!("{word} is not offered here; choose {}.", or_list(&words))
    })
}

/// The offered effort `word` names — the menu's label ("extra high") or the
/// contract's word (`xhigh`), in any case — or a refusal naming the offers.
pub(crate) fn effort_named(source: &Source, session: &str, word: &str) -> Result<Effort, HqError> {
    let offered = view(source, session)?;
    let said = word
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    offered
        .efforts
        .iter()
        .copied()
        .find(|&e| said == e.as_str() || said == effort_word(e))
        .ok_or_else(|| effort_refusal(word.trim(), &offered.efforts))
}

/// The offered efforts as `/effort`'s hint names them.
pub(crate) fn effort_hint(efforts: &[Effort]) -> String {
    let words: Vec<String> = efforts.iter().map(|&e| effort_word(e)).collect();
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    or_list(&words)
}

/// The settings row a session `session` with `agent`, being created on this
/// device, starts with: `pick`'s halves the agent still offers here, or no
/// row at all when it offers neither. Written as its own commit between the
/// session's and its first message's (`sessions::create_with_first`): a
/// refusal of it loses neither, and the first turn still runs with it.
pub(crate) fn initial(
    source: &Source,
    agent: &str,
    session: &str,
    pick: &AgentSettingsPick,
    now: i64,
) -> Result<Option<RowOp>, ReplicaError> {
    let offered = offered(source, agent)?;
    let model = pick.model.as_ref().filter(|m| offered.models.contains(*m));
    let effort = pick.effort.filter(|e| offered.efforts.contains(e));
    if model.is_none() && effort.is_none() {
        return Ok(None);
    }
    Ok(Some(RowOp::Upsert {
        table: TABLE.to_owned(),
        values: authored(
            source,
            object(serde_json::json!({
                "session_id": session, "model": model, "effort": effort, "updated_at_ms": now,
            })),
        ),
        base_version: None,
    }))
}

fn set(
    source: &Source,
    session: &str,
    model: Option<&str>,
    effort: Option<Effort>,
    now: i64,
) -> Result<(), HqError> {
    let offered = view(source, session)?;
    if model.is_some_and(|m| !offered.models.iter().any(|s| s == m)) {
        return Err(sentence(
            "This model is not offered by this conversation's agent.",
        ));
    }
    if effort.is_some_and(|e| !offered.efforts.contains(&e)) {
        return Err(sentence(
            "This effort is not offered by this conversation's agent.",
        ));
    }
    let version = row(source, session)?
        .as_ref()
        .and_then(|r| int(r, "version"));
    // The newest write decides, as a rename's does: every earlier write of
    // the row — refused, or still waiting and so not refused yet — is set
    // aside in the same step that writes this one.
    let earlier: Vec<String> = source
        .replica
        .row_writes(TABLE, session, None)
        .map(|writes| {
            writes
                .refused
                .into_iter()
                .map(|r| r.commit_id)
                .chain(writes.pending)
                .collect()
        })
        .map_err(sentence)?;
    source
        .replica
        .mutate_superseding(
            vec![RowOp::Upsert {
                table: TABLE.to_owned(),
                values: authored(
                    source,
                    object(serde_json::json!({
                        "session_id": session, "model": model, "effort": effort, "updated_at_ms": now,
                    })),
                ),
                base_version: version.filter(|v| *v > 0),
            }],
            earlier,
        )
        .map_err(sentence)?;
    source.replica.sync_now();
    Ok(())
}

/// One offered model as the picker lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentModelOptionVm {
    pub id: String,
    /// The next answer uses it: the saved choice, or a default while none
    /// that is still offered is saved.
    pub current: bool,
    /// An instance of the agent names it as its default.
    pub default: bool,
}

/// One offered effort as the picker lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentEffortOptionVm {
    pub effort: Effort,
    pub label: String,
    pub current: bool,
    pub default: bool,
}

/// What became of the settings written on this device, read from the
/// replica's outbox and outcome journal (`Replica::row_writes`), never
/// inferred from the write returning: that only says the outbox took it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "state", rename_all = "camelCase")]
#[ts(export)]
pub enum AgentSettingsWriteVm {
    /// Nothing written here waits or stands refused.
    Confirmed,
    /// A change waits in the outbox for the server.
    Pending,
    /// The server refused the newest change written here, as the person reads it.
    Refused { sentence: String },
    /// The replica could not say.
    Unknown,
}

/// The composer's model picker and effort menu, decided here (109.18). An
/// effort menu with nothing offered is absent, never disabled: `efforts`
/// is empty and `effort_label` is `None`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentSessionPickerVm {
    /// Only the models the session's agent offers, in order.
    pub models: Vec<AgentModelOptionVm>,
    /// Only the efforts the agent offers, lowest first.
    pub efforts: Vec<AgentEffortOptionVm>,
    /// What the model button says.
    pub model_label: String,
    /// What the effort button says; `None` when no effort is offered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub effort_label: Option<String>,
    /// A choice is saved, so Reset has something to write back to null.
    pub resettable: bool,
    /// While a turn is being written: a change waits for the next answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub running: Option<String>,
    /// The newest answer's `metadata.model_fallback`: the chosen model was
    /// no longer offered, and which one answered instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub fallback: Option<String>,
    pub write: AgentSettingsWriteVm,
}

fn effort_label(effort: Effort) -> &'static str {
    match effort {
        Effort::Low => "Low",
        Effort::Medium => "Medium",
        Effort::High => "High",
        Effort::Xhigh => "Extra high",
    }
}

/// A refused settings write, by the server's V16 code, matched exactly.
fn refusal(rejection: &Rejection) -> String {
    let why = match rejection.code.as_str() {
        "hq.v16.model" => "the agent does not offer that model".to_owned(),
        "hq.v16.effort" => "the agent does not offer that effort".to_owned(),
        "hq.v16.person_member" => {
            "only a person in this conversation can change its model and effort".to_owned()
        }
        "hq.v16.session_changed" => "a choice belongs to one conversation".to_owned(),
        CONFLICT => "another device changed it first".to_owned(),
        _ => rejection.to_string(),
    };
    format!("The server refused your model and effort change: {why}.")
}

fn write_state(source: &Source, session: &str) -> AgentSettingsWriteVm {
    match source.replica.row_writes(TABLE, session, None) {
        Ok(writes) => match writes.refused.first() {
            Some(refused) => AgentSettingsWriteVm::Refused {
                sentence: refusal(&refused.rejection),
            },
            None if !writes.pending.is_empty() => AgentSettingsWriteVm::Pending,
            None => AgentSettingsWriteVm::Confirmed,
        },
        Err(error) => {
            tracing::debug!(database = %source.database, %error, "the settings' delivery could not be read");
            AgentSettingsWriteVm::Unknown
        }
    }
}

/// The newest answer's fallback sentence, if it carries one.
fn fallback(source: &Source, session: &str) -> Option<String> {
    let rows = read(
        source,
        "SELECT metadata FROM messages WHERE session_id = ?1 AND role = 'agent' \
         ORDER BY created_at_ms DESC, id DESC LIMIT 1",
        &[Value::from(session)],
    );
    let metadata = json(rows.first()?, "metadata");
    metadata
        .get("model_fallback")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// One label for a set of per-instance defaults: the one they agree on, or
/// "Default" when they disagree or name none.
fn default_label<'a>(defaults: impl Iterator<Item = &'a str>) -> String {
    let distinct: BTreeSet<&str> = defaults.collect();
    match distinct.into_iter().collect::<Vec<_>>().as_slice() {
        [only] => (*only).to_owned(),
        _ => "Default".to_owned(),
    }
}

/// The pickers over `settings`, given what else the session says now.
pub(crate) fn picker_of(
    settings: &AgentSessionSettingsVm,
    running: Option<String>,
    fallback: Option<String>,
    write: AgentSettingsWriteVm,
) -> AgentSessionPickerVm {
    let default_models: BTreeSet<&str> = settings
        .defaults
        .iter()
        .filter_map(|d| d.model.as_deref())
        .collect();
    let default_efforts: BTreeSet<Effort> =
        settings.defaults.iter().filter_map(|d| d.effort).collect();
    // A saved choice the agent no longer offers is not the one used.
    let model = settings
        .model
        .as_deref()
        .filter(|m| settings.models.iter().any(|o| o == m));
    let effort = settings.effort.filter(|e| settings.efforts.contains(e));
    let models = settings
        .models
        .iter()
        .map(|id| AgentModelOptionVm {
            id: id.clone(),
            current: model.map_or_else(|| default_models.contains(id.as_str()), |m| m == id),
            default: default_models.contains(id.as_str()),
        })
        .collect();
    let efforts: Vec<AgentEffortOptionVm> = settings
        .efforts
        .iter()
        .map(|&e| AgentEffortOptionVm {
            effort: e,
            label: effort_label(e).to_owned(),
            current: effort.map_or_else(|| default_efforts.contains(&e), |c| c == e),
            default: default_efforts.contains(&e),
        })
        .collect();
    AgentSessionPickerVm {
        model_label: model.map_or_else(
            || default_label(default_models.iter().copied()),
            str::to_owned,
        ),
        effort_label: (!efforts.is_empty()).then(|| {
            effort.map_or_else(
                || default_label(default_efforts.iter().map(|&e| effort_label(e))),
                |e| effort_label(e).to_owned(),
            )
        }),
        models,
        efforts,
        resettable: settings.model.is_some() || settings.effort.is_some(),
        running,
        fallback,
        write,
    }
}

/// The pickers of `session` as its replica holds them now; an error while
/// the settings are still arriving or the reader may not change them.
pub(crate) fn picker(source: &Source, session: &str) -> Result<AgentSessionPickerVm, HqError> {
    let settings = view(source, session)?;
    Ok(picker_of(
        &settings,
        running(source, session),
        fallback(source, session),
        write_state(source, session),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hq::port::ReplicaSpec;
    use crate::hq::syncular::SyncularReplica;
    use serde_json::json;
    use std::sync::Arc;

    /// A slash command as the composer runs it, from under an empty banner.
    fn run(
        source: &Source,
        sid: &str,
        draft: &str,
        pick: Option<&AgentSettingsPick>,
    ) -> Result<crate::hq::commands::AgentCommandResultVm, String> {
        let decisions = crate::hq::approvals::Decisions::load(
            std::env::temp_dir().join(format!("keeper-settings-{}.json", ulid::Ulid::new())),
        );
        crate::hq::commands::run(source, &[], &decisions, sid, draft, pick, &[])
    }

    fn fixture(complete: bool) -> (tempfile::TempDir, Source) {
        fixture_with(complete, true)
    }

    /// `with_efforts: false` is Nixi as shipped: her effort is withheld, so no
    /// instance offers one.
    fn fixture_with(complete: bool, with_efforts: bool) -> (tempfile::TempDir, Source) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.sqlite");
        let replica = SyncularReplica::open(ReplicaSpec {
            database: "alpha".to_owned(),
            path: path.clone(),
            client_id: Some("dev1".to_owned()),
            link: None,
            subscriptions: Vec::new(),
            watch_presence: None,
        })
        .expect("replica");
        let source = Source {
            database: "alpha".to_owned(),
            replica: Arc::new(replica),
            actor: "ana".to_owned(),
            client: "dev1".to_owned(),
        };
        let mut rows = vec![
            (
                "sessions",
                json!({"id":"dev1.1", "title":"choices", "status":"open", "agent_actor":"nixi", "created_by_actor":"ana", "created_by_client":"dev1", "invite":"[]", "archived":false, "created_at_ms":1, "last_activity_ms":1}),
            ),
            (
                "session_members",
                json!({"id":"dev1.1/ana", "session_id":"dev1.1", "actor_id":"ana", "role":"person", "reads_trace":false, "added_by_actor":"ana", "added_at_ms":1}),
            ),
        ];
        for (id, actor, models, efforts_of, default) in [
            (
                "nixi.one",
                "nixi",
                vec!["primary", "alternate"],
                vec!["high"],
                "primary",
            ),
            (
                "nixi.two",
                "nixi",
                vec!["alternate", "second"],
                vec!["medium"],
                "second",
            ),
            (
                "foreign.one",
                "foreign",
                vec!["foreign"],
                vec!["low"],
                "foreign",
            ),
        ] {
            let efforts: Vec<&str> = if with_efforts || actor != "nixi" {
                efforts_of
            } else {
                Vec::new()
            };
            rows.push(("agent_profiles", json!({"id":id, "actor_id":actor, "name":actor, "description":"choices", "definition_hash":"h", "image_digest":"d", "started_at_ms":1, "dir":"alpha", "models":serde_json::to_string(&models).expect("models"), "efforts":serde_json::to_string(&efforts).expect("efforts"), "default_model":default, "default_effort":null})));
        }
        source
            .replica
            .mutate(
                rows.into_iter()
                    .map(|(table, values)| RowOp::Upsert {
                        table: table.to_owned(),
                        values: authored(&source, object(values)),
                        base_version: None,
                    })
                    .collect(),
            )
            .expect("seed");
        source
            .replica
            .set_window(
                &WindowBase {
                    table: "session_settings".to_owned(),
                    variable: "session".to_owned(),
                    fixed_scopes: Vec::new(),
                },
                &["dev1.1".to_owned()],
            )
            .expect("window");
        if complete {
            // The real adapter reads the same persisted completion that a server round writes.
            // Offline fixture only: no network or fake query answers.
            rusqlite::Connection::open(path).expect("db").execute(
                "UPDATE _syncular_subscriptions SET state_json = ?1 WHERE tbl = 'session_settings'",
                [json!({"requested":{"session":["dev1.1"]}, "effectiveScopes":{"session":["dev1.1"]}, "status":"active", "cursor":0}).to_string()],
            ).expect("complete settings window");
        }
        (dir, source)
    }

    /// A109-05: the list's "Model…" is a person member's of a live session
    /// whose agent offers more than one model, its profiles' union.
    #[test]
    fn the_list_offers_model_only_where_there_is_more_than_one_to_choose() {
        let (_dir, source) = fixture(true);
        let session = |id: &str, agent: &str, role: &str, archived: bool| {
            vec![
                (
                    "sessions",
                    json!({"id":id, "title":id, "status":"open", "agent_actor":agent, "created_by_actor":"ana", "created_by_client":"dev1", "invite":"[]", "archived":archived, "created_at_ms":1, "last_activity_ms":1}),
                ),
                (
                    "session_members",
                    json!({"id":format!("{id}/ana"), "session_id":id, "actor_id":"ana", "role":role, "reads_trace":false, "added_by_actor":"ana", "added_at_ms":1}),
                ),
            ]
        };
        let profile = |id: &str, actor: &str, model: &str| {
            (
                "agent_profiles",
                json!({"id":id, "actor_id":actor, "name":actor, "description":"", "definition_hash":"h", "image_digest":"d", "started_at_ms":1, "dir":"alpha", "models":serde_json::to_string(&[model]).expect("models"), "efforts":"[]", "default_model":model, "default_effort":null}),
            )
        };
        let mut rows = Vec::new();
        rows.extend(session("dev1.2", "foreign", "owner", false));
        rows.extend(session("dev1.3", "duo", "owner", false));
        rows.extend(session("dev1.4", "nixi", "agent", false));
        rows.extend(session("dev1.5", "nixi", "owner", true));
        rows.extend(session("dev1.6", "silent", "owner", false));
        // foreign's second instance offers the same one model; duo's two
        // instances one model each, two together.
        rows.push(profile("foreign.two", "foreign", "foreign"));
        rows.push(profile("duo.one", "duo", "left"));
        rows.push(profile("duo.two", "duo", "right"));
        source
            .replica
            .mutate(
                rows.into_iter()
                    .map(|(table, values)| RowOp::Upsert {
                        table: table.to_owned(),
                        values: authored(&source, object(values)),
                        base_version: None,
                    })
                    .collect(),
            )
            .expect("seed");
        let mut menu: Vec<(String, bool)> = crate::hq::sessions::rows(&source, 2)
            .into_iter()
            .map(|row| (row.session_id, row.chooses_model))
            .collect();
        menu.sort();
        assert_eq!(
            menu,
            [
                ("dev1.1".to_owned(), true),
                ("dev1.2".to_owned(), false),
                ("dev1.3".to_owned(), true),
                ("dev1.4".to_owned(), false),
                ("dev1.5".to_owned(), false),
                ("dev1.6".to_owned(), false),
            ],
            "three models; one model twice; two from two instances; not a person member; \
             archived; no profile"
        );
    }

    #[test]
    fn choices_union_only_matching_profiles_and_preserve_each_default() {
        let (_dir, source) = fixture(true);
        let choices = view(&source, "dev1.1").expect("choices");
        assert_eq!(choices.models, ["alternate", "primary", "second"]);
        assert_eq!(choices.efforts, [Effort::Medium, Effort::High]);
        assert_eq!(
            choices
                .defaults
                .iter()
                .map(|d| (d.client_id.as_str(), d.model.as_deref(), d.effort))
                .collect::<Vec<_>>(),
            vec![
                ("nixi.one", Some("primary"), None),
                ("nixi.two", Some("second"), None)
            ]
        );
        set(&source, "dev1.1", Some("alternate"), Some(Effort::High), 10).expect("save");
        let saved = view(&source, "dev1.1").expect("saved");
        assert_eq!(
            (saved.model.as_deref(), saved.effort),
            (Some("alternate"), Some(Effort::High))
        );
        set(&source, "dev1.1", None, None, 11).expect("reset");
        let reset = view(&source, "dev1.1").expect("reset view");
        assert_eq!((reset.model, reset.effort), (None, None));
    }

    #[test]
    fn refused_choices_and_nonmembers_never_overwrite_the_saved_selection() {
        let (_dir, mut source) = fixture(true);
        set(&source, "dev1.1", Some("primary"), Some(Effort::Medium), 10).expect("save");
        assert!(matches!(
            set(&source, "dev1.1", Some("foreign"), None, 11),
            Err(HqError::Sentence(_))
        ));
        assert!(matches!(
            set(&source, "dev1.1", None, Some(Effort::Low), 11),
            Err(HqError::Sentence(_))
        ));
        source.actor = "outsider".to_owned();
        assert!(matches!(
            set(&source, "dev1.1", None, None, 11),
            Err(HqError::Sentence(_))
        ));
        source.actor = "ana".to_owned();
        let saved = view(&source, "dev1.1").expect("still saved");
        assert_eq!(
            (saved.model.as_deref(), saved.effort),
            (Some("primary"), Some(Effort::Medium))
        );
    }

    #[test]
    fn incomplete_settings_are_not_mistaken_for_default_choices() {
        let (_dir, source) = fixture(false);
        assert!(matches!(view(&source, "dev1.1"), Err(HqError::Arriving(_))));
        assert!(matches!(
            set(&source, "dev1.1", None, None, 11),
            Err(HqError::Arriving(_))
        ));
        assert!(source
            .replica
            .query("SELECT * FROM session_settings", &[])
            .expect("settings")
            .is_empty());
    }

    /// Write `values` into `table` as the replica holds rows, bypassing every
    /// check: a row a server round would have brought.
    fn put(source: &Source, table: &str, values: serde_json::Value) {
        source
            .replica
            .mutate(vec![RowOp::Upsert {
                table: table.to_owned(),
                values: authored(source, object(values)),
                base_version: None,
            }])
            .expect("row");
    }

    /// What the settings row holds, raw: what the runtime reads at turn start.
    fn stored(source: &Source) -> (Value, Value) {
        let rows = source
            .replica
            .query(
                "SELECT model, effort FROM session_settings WHERE session_id = 'dev1.1'",
                &[],
            )
            .expect("row");
        let row = rows.first().expect("a settings row");
        (row["model"].clone(), row["effort"].clone())
    }

    fn answer(source: &Source, id: &str, turn: &str, at: i64, metadata: serde_json::Value) {
        put(
            source,
            "messages",
            json!({"id": id, "session_id": "dev1.1",
                   "page_key": crate::hq::sessions::page_key("dev1.1", &crate::hq::sessions::month(at)),
                   "turn_id": turn, "role": "agent", "body": "an answer", "body_blob": null,
                   "metadata": metadata.to_string(), "created_at_ms": at}),
        );
    }

    /// Record, as a round would, the server refusing commit `commit` that
    /// wrote this session's settings row.
    fn journal_refusal(dir: &tempfile::TempDir, commit: &str, code: &str) {
        use syncular_client::api::{CommitOperation, CommitOperationOutcome, RejectionRecord};
        let operations = vec![CommitOperation {
            table: TABLE.to_owned(),
            row_id: "dev1.1".to_owned(),
            op: "upsert".to_owned(),
            base_version: None,
            values: json!({"session_id": "dev1.1", "model": "alternate"})
                .as_object()
                .cloned(),
        }];
        let results = vec![CommitOperationOutcome::Error {
            rejection: RejectionRecord {
                client_commit_id: commit.to_owned(),
                op_index: 0,
                code: code.to_owned(),
                message: code.to_owned(),
                retryable: false,
                details: None,
                operation: None,
            },
        }];
        let conn = rusqlite::Connection::open(dir.path().join("settings.sqlite")).expect("opens");
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .expect("timeout");
        conn.execute(
            "INSERT INTO _syncular_commit_outcomes(client_commit_id, status, recorded_at_ms, \
             results_json, operations_json) VALUES (?1, 'rejected', 1, ?2, ?3)",
            rusqlite::params![
                commit,
                serde_json::to_string(&results).expect("results"),
                serde_json::to_string(&operations).expect("operations")
            ],
        )
        .expect("records");
    }

    fn settings(
        model: Option<&str>,
        effort: Option<Effort>,
        defaults: &[(Option<&str>, Option<Effort>)],
    ) -> AgentSessionSettingsVm {
        AgentSessionSettingsVm {
            model: model.map(str::to_owned),
            effort,
            models: vec!["alternate".to_owned(), "primary".to_owned()],
            efforts: vec![Effort::Medium, Effort::High],
            defaults: defaults
                .iter()
                .enumerate()
                .map(|(i, (model, effort))| AgentSettingsDefaultVm {
                    client_id: format!("nixi.{i}"),
                    model: model.map(str::to_owned),
                    effort: *effort,
                })
                .collect(),
        }
    }

    #[test]
    fn the_picker_lists_only_offers_and_marks_the_choice_used_and_each_default() {
        let mark = |vm: &AgentSessionPickerVm| {
            (
                vm.models
                    .iter()
                    .map(|m| (m.id.clone(), m.current, m.default))
                    .collect::<Vec<_>>(),
                vm.efforts
                    .iter()
                    .map(|e| (e.effort, e.current, e.default))
                    .collect::<Vec<_>>(),
            )
        };
        // Nothing saved but a model the agent dropped: never listed, and the
        // default it falls back to is the one used.
        let dropped = picker_of(
            &settings(Some("gone"), None, &[(Some("primary"), Some(Effort::High))]),
            None,
            None,
            AgentSettingsWriteVm::Confirmed,
        );
        assert_eq!(
            mark(&dropped),
            (
                vec![
                    ("alternate".to_owned(), false, false),
                    ("primary".to_owned(), true, true)
                ],
                vec![(Effort::Medium, false, false), (Effort::High, true, true)]
            )
        );
        assert_eq!(dropped.model_label, "primary");
        assert_eq!(dropped.effort_label.as_deref(), Some("High"));
        assert!(
            dropped.resettable,
            "a saved choice, even a dropped one, can be reset"
        );

        let chosen = picker_of(
            &settings(
                Some("alternate"),
                Some(Effort::Medium),
                &[(Some("primary"), Some(Effort::High))],
            ),
            None,
            None,
            AgentSettingsWriteVm::Confirmed,
        );
        assert_eq!(
            mark(&chosen),
            (
                vec![
                    ("alternate".to_owned(), true, false),
                    ("primary".to_owned(), false, true)
                ],
                vec![(Effort::Medium, true, false), (Effort::High, false, true)]
            )
        );
        assert_eq!(chosen.model_label, "alternate");
        assert_eq!(chosen.effort_label.as_deref(), Some("Medium"));

        // Instances that disagree are never given one invented default.
        let split = picker_of(
            &settings(
                None,
                None,
                &[(Some("primary"), None), (Some("alternate"), None)],
            ),
            None,
            None,
            AgentSettingsWriteVm::Confirmed,
        );
        assert_eq!(split.model_label, "Default");
        assert_eq!(split.effort_label.as_deref(), Some("Default"));
        assert!(!split.resettable);
    }

    #[test]
    fn nixi_offers_no_effort_so_her_effort_menu_and_command_are_absent() {
        let (_dir, source) = fixture_with(true, false);
        let vm = picker(&source, "dev1.1").expect("picker");
        assert_eq!(vm.models.len(), 3, "her models are still offered");
        assert!(vm.efforts.is_empty());
        assert_eq!(vm.effort_label, None, "absent, not a disabled menu");
        let offers = crate::hq::commands::Offers::of(
            &source,
            "dev1.1",
            &crate::hq::timeline::session_row(&source, "dev1.1").expect("session"),
        );
        assert!(offers.models && offers.efforts.is_empty());
        assert!(run(&source, "dev1.1", "/effort high", None).is_err());
        assert!(matches!(
            change(
                &source,
                "dev1.1",
                AgentSettingsChange::Effort {
                    effort: Effort::High
                },
                2
            ),
            Err(HqError::Sentence(_))
        ));
    }

    #[test]
    fn reset_writes_null_for_both_choices() {
        let (_dir, source) = fixture(true);
        change(
            &source,
            "dev1.1",
            AgentSettingsChange::Model {
                model: "alternate".to_owned(),
            },
            2,
        )
        .expect("model");
        change(
            &source,
            "dev1.1",
            AgentSettingsChange::Effort {
                effort: Effort::High,
            },
            3,
        )
        .expect("effort");
        assert_eq!(stored(&source), (json!("alternate"), json!("high")));
        assert!(picker(&source, "dev1.1").expect("picker").resettable);
        change(&source, "dev1.1", AgentSettingsChange::Reset, 4).expect("reset");
        assert_eq!(stored(&source), (Value::Null, Value::Null));
        assert!(!picker(&source, "dev1.1").expect("picker").resettable);
    }

    #[test]
    fn one_change_keeps_the_other_choice_and_clears_a_dropped_one() {
        let (_dir, source) = fixture(true);
        change(
            &source,
            "dev1.1",
            AgentSettingsChange::Model {
                model: "second".to_owned(),
            },
            2,
        )
        .expect("model");
        change(
            &source,
            "dev1.1",
            AgentSettingsChange::Effort {
                effort: Effort::Medium,
            },
            3,
        )
        .expect("effort");
        assert_eq!(stored(&source), (json!("second"), json!("medium")));
        // The agent stopped offering the saved model: the runtime already
        // uses the default for it, so an effort change does not fail on it.
        put(
            &source,
            TABLE,
            json!({"session_id": "dev1.1", "model": "gone", "effort": "medium", "updated_at_ms": 4}),
        );
        change(
            &source,
            "dev1.1",
            AgentSettingsChange::Effort {
                effort: Effort::High,
            },
            5,
        )
        .expect("effort over a dropped model");
        assert_eq!(stored(&source), (Value::Null, json!("high")));
        let refused = change(
            &source,
            "dev1.1",
            AgentSettingsChange::Model {
                model: "foreign".to_owned(),
            },
            6,
        );
        assert!(
            matches!(&refused, Err(HqError::Sentence(s)) if s.contains("alternate, primary or second")),
            "{refused:?}"
        );
        assert_eq!(stored(&source), (Value::Null, json!("high")));
    }

    #[test]
    fn a_change_while_a_turn_runs_says_it_applies_from_the_next_answer() {
        let (_dir, source) = fixture(true);
        let model = || AgentSettingsChange::Model {
            model: "alternate".to_owned(),
        };
        assert_eq!(
            change(&source, "dev1.1", model(), 2).expect("idle").note,
            None
        );
        put(
            &source,
            "turn_chunks",
            json!({"id": "chunk-t1", "session_id": "dev1.1", "turn_id": "t1", "seq": 0,
                   "text": "", "created_at_ms": 3}),
        );
        assert_eq!(
            change(&source, "dev1.1", model(), 4)
                .expect("running")
                .note
                .as_deref(),
            Some(RUNNING)
        );
        assert_eq!(
            picker(&source, "dev1.1")
                .expect("picker")
                .running
                .as_deref(),
            Some(RUNNING)
        );
        answer(&source, "m1", "t1", 5, json!({"model": "primary"}));
        assert_eq!(
            change(&source, "dev1.1", model(), 6).expect("ended").note,
            None
        );
        assert_eq!(picker(&source, "dev1.1").expect("picker").running, None);
    }

    #[test]
    fn the_newest_answers_fallback_is_said_by_the_picker() {
        let (_dir, source) = fixture(true);
        let said = "gone is no longer offered; using primary instead.";
        answer(
            &source,
            "m1",
            "t1",
            5,
            json!({"model": "primary", "model_fallback": said}),
        );
        assert_eq!(
            picker(&source, "dev1.1")
                .expect("picker")
                .fallback
                .as_deref(),
            Some(said)
        );
        answer(
            &source,
            "m2",
            "t2",
            6,
            json!({"model": "primary", "model_fallback": null}),
        );
        assert_eq!(
            picker(&source, "dev1.1").expect("picker").fallback,
            None,
            "an older answer's fallback is not the newest answer's"
        );
    }

    #[test]
    fn a_refusal_of_a_change_a_newer_one_replaced_never_reaches_the_picker() {
        let (dir, source) = fixture(true);
        change(
            &source,
            "dev1.1",
            AgentSettingsChange::Model {
                model: "alternate".to_owned(),
            },
            2,
        )
        .expect("first");
        let first = source
            .replica
            .row_writes(TABLE, "dev1.1", None)
            .expect("writes")
            .pending
            .first()
            .cloned()
            .expect("the first change waits");
        // A newer change while the first still waits; the server then refuses
        // the first. The person's latest choice is not shown as refused.
        change(&source, "dev1.1", AgentSettingsChange::Reset, 3).expect("newer");
        refuse_waiting(&dir, &first, "hq.v16.model");
        assert_eq!(
            picker(&source, "dev1.1").expect("picker").write,
            AgentSettingsWriteVm::Pending
        );
    }

    #[test]
    fn a_later_server_refusal_reaches_the_picker_until_a_new_change() {
        let (dir, source) = fixture(true);
        assert_eq!(
            picker(&source, "dev1.1").expect("picker").write,
            AgentSettingsWriteVm::Confirmed
        );
        change(
            &source,
            "dev1.1",
            AgentSettingsChange::Model {
                model: "alternate".to_owned(),
            },
            2,
        )
        .expect("model");
        assert_eq!(
            picker(&source, "dev1.1").expect("picker").write,
            AgentSettingsWriteVm::Pending
        );
        journal_refusal(&dir, "c-refused", "hq.v16.model");
        assert_eq!(
            picker(&source, "dev1.1").expect("picker").write,
            AgentSettingsWriteVm::Refused {
                sentence: "The server refused your model and effort change: the agent does not offer that model.".to_owned()
            }
        );
        change(&source, "dev1.1", AgentSettingsChange::Reset, 3).expect("reset");
        assert_eq!(
            picker(&source, "dev1.1").expect("picker").write,
            AgentSettingsWriteVm::Pending,
            "a new change sets the old refusal aside"
        );
    }

    #[test]
    fn model_and_effort_commands_write_what_the_pickers_write() {
        let (_dir, source) = fixture(true);
        assert_eq!(
            run(&source, "dev1.1", "/model alternate", None)
                .expect("model")
                .message,
            "The model is now alternate."
        );
        assert_eq!(
            run(&source, "dev1.1", "/effort High", None)
                .expect("effort")
                .message,
            "The effort is now high."
        );
        assert_eq!(stored(&source), (json!("alternate"), json!("high")));
        assert!(run(&source, "dev1.1", "/model foreign", None)
            .is_err_and(|e| e.contains("choose alternate, primary or second")));
        assert!(run(&source, "dev1.1", "/effort low", None)
            .is_err_and(|e| e.contains("choose medium or high")));
        assert!(run(&source, "dev1.1", "/effort loud", None).is_err());
        assert_eq!(stored(&source), (json!("alternate"), json!("high")));
    }

    #[test]
    fn every_v16_refusal_reaches_the_picker_in_its_own_words() {
        for (code, why) in [
            ("hq.v16.model", "the agent does not offer that model"),
            ("hq.v16.effort", "the agent does not offer that effort"),
            (
                "hq.v16.person_member",
                "only a person in this conversation can change its model and effort",
            ),
            (
                "hq.v16.session_changed",
                "a choice belongs to one conversation",
            ),
        ] {
            let (dir, source) = fixture(true);
            journal_refusal(&dir, "c-refused", code);
            assert_eq!(
                picker(&source, "dev1.1").expect("picker").write,
                AgentSettingsWriteVm::Refused {
                    sentence: format!("The server refused your model and effort change: {why}.")
                },
                "{code}"
            );
        }
        // keeper's own code for a conflict outcome, which carries none of its own.
        assert_eq!(
            refusal(&Rejection::new(CONFLICT, "someone changed it")),
            "The server refused your model and effort change: another device changed it first."
        );
    }

    #[test]
    fn effort_takes_only_offered_levels_by_the_menus_word_or_the_contracts() {
        let (_dir, source) = fixture(true);
        // Offered: medium and high. Neither low nor xhigh is listed or taken.
        for word in ["low", "xhigh", "extra high", "loud"] {
            let refused = run(&source, "dev1.1", &format!("/effort {word}"), None);
            assert!(
                refused
                    .as_ref()
                    .is_err_and(|e| e.ends_with("is not offered here; choose medium or high.")),
                "{word}: {refused:?}"
            );
        }
        // An agent that offers extra high takes the menu's label for it.
        put(
            &source,
            "agent_profiles",
            json!({"id": "nixi.three", "actor_id": "nixi", "name": "nixi", "description": "choices",
                   "definition_hash": "h", "image_digest": "d", "started_at_ms": 1, "dir": "alpha",
                   "models": "[\"primary\"]", "efforts": "[\"xhigh\"]",
                   "default_model": "primary", "default_effort": null}),
        );
        for (word, effort) in [
            ("Extra  High", "xhigh"),
            ("xhigh", "xhigh"),
            ("medium", "medium"),
        ] {
            let result = run(&source, "dev1.1", &format!("/effort {word}"), None).expect(word);
            assert_eq!(stored(&source).1, json!(effort), "{word}");
            assert_eq!(
                result.pick.and_then(|p| p.effort).map(Effort::as_str),
                Some(effort)
            );
        }
        assert_eq!(
            run(&source, "dev1.1", "/effort extra high", None)
                .expect("label")
                .message,
            "The effort is now extra high."
        );
    }

    /// The settings row a creation wrote, raw.
    fn seeded(source: &Source, session: &str) -> Option<(Value, Value)> {
        source
            .replica
            .query(
                "SELECT model, effort FROM session_settings WHERE session_id = ?1",
                &[session.into()],
            )
            .expect("rows")
            .first()
            .map(|row| (row["model"].clone(), row["effort"].clone()))
    }

    #[test]
    fn a_new_session_starts_with_the_devices_last_pick_where_still_offered() {
        use crate::hq::sessions::create;
        let (_dir, source) = fixture(true);
        let pick = |model: Option<&str>, effort| AgentSettingsPick {
            model: model.map(str::to_owned),
            effort,
        };
        // The last pick, all of it offered: written with the creation.
        let id = create(
            &source,
            "nixi",
            "hello",
            Some(&pick(Some("second"), Some(Effort::High))),
        )
        .expect("created");
        assert_eq!(seeded(&source, &id), Some((json!("second"), json!("high"))));
        // Half of it dropped since: only the offered half.
        let id = create(
            &source,
            "nixi",
            "hello",
            Some(&pick(Some("gone"), Some(Effort::Medium))),
        )
        .expect("created");
        assert_eq!(seeded(&source, &id), Some((Value::Null, json!("medium"))));
        // All of it dropped: nothing written, the agent's defaults answer.
        let id = create(
            &source,
            "nixi",
            "hello",
            Some(&pick(Some("foreign"), Some(Effort::Low))),
        )
        .expect("created");
        assert_eq!(seeded(&source, &id), None);
        // No pick kept for the agent (or a reset one): nothing written.
        for none in [None, Some(&AgentSettingsPick::default())] {
            let id = create(&source, "nixi", "hello", none).expect("created");
            assert_eq!(seeded(&source, &id), None);
        }
    }

    /// The tables each commit waiting in the outbox writes, oldest first.
    fn outbox(dir: &tempfile::TempDir) -> Vec<(String, Vec<String>)> {
        let conn = rusqlite::Connection::open(dir.path().join("settings.sqlite")).expect("opens");
        let mut statement = conn
            .prepare("SELECT commit_id, ops_json FROM _syncular_outbox ORDER BY seq")
            .expect("outbox");
        statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .expect("commits")
            .map(|row| {
                let (commit, ops) = row.expect("commit");
                let ops: Vec<Value> = serde_json::from_str(&ops).expect("ops");
                let tables = ops
                    .iter()
                    .map(|op| op["table"].as_str().expect("table").to_owned())
                    .collect();
                (commit, tables)
            })
            .collect()
    }

    /// The server refuses waiting commit `commit` as a round records it: the
    /// outcome with the commit's own operations, and the commit gone from the
    /// outbox.
    fn refuse_waiting(dir: &tempfile::TempDir, commit: &str, code: &str) {
        use syncular_client::api::{CommitOperation, CommitOperationOutcome, RejectionRecord};
        let mut conn =
            rusqlite::Connection::open(dir.path().join("settings.sqlite")).expect("opens");
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .expect("timeout");
        let ops: String = conn
            .query_row(
                "SELECT ops_json FROM _syncular_outbox WHERE commit_id = ?1",
                [commit],
                |row| row.get(0),
            )
            .expect("a waiting commit");
        let operations: Vec<CommitOperation> = serde_json::from_str(&ops).expect("wire operations");
        let results: Vec<CommitOperationOutcome> = (0..operations.len())
            .map(|index| CommitOperationOutcome::Error {
                rejection: RejectionRecord {
                    client_commit_id: commit.to_owned(),
                    op_index: i32::try_from(index).expect("small"),
                    code: code.to_owned(),
                    message: code.to_owned(),
                    retryable: false,
                    details: None,
                    operation: None,
                },
            })
            .collect();
        let tx = conn.transaction().expect("transaction");
        tx.execute(
            "INSERT INTO _syncular_commit_outcomes(client_commit_id, status, recorded_at_ms, \
             results_json, operations_json) VALUES (?1, 'rejected', 1, ?2, ?3)",
            rusqlite::params![
                commit,
                serde_json::to_string(&results).expect("results"),
                serde_json::to_string(&operations).expect("operations")
            ],
        )
        .expect("records");
        tx.execute(
            "DELETE FROM _syncular_outbox WHERE commit_id = ?1",
            [commit],
        )
        .expect("settles outbox");
        tx.commit().expect("durable");
    }

    /// Hold the settings of `sessions` as a completed server round would.
    fn cover(dir: &tempfile::TempDir, source: &Source, sessions: &[&str]) {
        let ids: Vec<String> = sessions.iter().map(|s| (*s).to_owned()).collect();
        source
            .replica
            .set_window(
                &WindowBase {
                    table: TABLE.to_owned(),
                    variable: "session".to_owned(),
                    fixed_scopes: Vec::new(),
                },
                &ids,
            )
            .expect("window");
        rusqlite::Connection::open(dir.path().join("settings.sqlite"))
            .expect("db")
            .execute(
                "UPDATE _syncular_subscriptions SET state_json = ?1 WHERE tbl = 'session_settings'",
                [json!({"requested":{"session":ids}, "effectiveScopes":{"session":ids}, "status":"active", "cursor":0}).to_string()],
            )
            .expect("complete settings window");
    }

    #[test]
    fn a_new_sessions_pick_is_its_own_commit_between_the_session_and_its_message() {
        let (dir, source) = fixture(true);
        let before = outbox(&dir).len();
        let pick = AgentSettingsPick {
            model: Some("second".to_owned()),
            effort: Some(Effort::High),
        };
        crate::hq::sessions::create(&source, "nixi", "hello", Some(&pick)).expect("created");
        let created: Vec<Vec<String>> = outbox(&dir)
            .into_iter()
            .skip(before)
            .map(|(_, tables)| tables)
            .collect();
        assert_eq!(
            created,
            vec![
                vec!["sessions".to_owned(), "session_members".to_owned()],
                vec![TABLE.to_owned()],
                vec!["messages".to_owned()],
            ],
            "the session, then the pick, then the first message"
        );
        // Nothing of a pick to write (none kept, or none still offered): the
        // creation stays the one commit it always was.
        for pick in [
            None,
            Some(AgentSettingsPick {
                model: Some("foreign".to_owned()),
                effort: Some(Effort::Low),
            }),
        ] {
            let before = outbox(&dir).len();
            crate::hq::sessions::create(&source, "nixi", "hello", pick.as_ref()).expect("created");
            let created: Vec<Vec<String>> = outbox(&dir)
                .into_iter()
                .skip(before)
                .map(|(_, tables)| tables)
                .collect();
            assert_eq!(
                created,
                vec![vec![
                    "sessions".to_owned(),
                    "session_members".to_owned(),
                    "messages".to_owned()
                ]],
                "{pick:?}"
            );
        }
    }

    #[test]
    fn a_refused_pick_leaves_the_new_session_and_its_first_message_and_says_so() {
        let (dir, source) = fixture(true);
        let before = outbox(&dir).len();
        // The device remembers a model it still sees offered; the server no
        // longer does, and V16 refuses the row.
        let pick = AgentSettingsPick {
            model: Some("second".to_owned()),
            effort: None,
        };
        let id = crate::hq::sessions::create(&source, "nixi", "hello there", Some(&pick))
            .expect("created");
        let commits = outbox(&dir);
        let (settings_commit, _) = commits
            .iter()
            .skip(before)
            .find(|(_, tables)| tables == &[TABLE.to_owned()])
            .expect("the pick's own commit")
            .clone();
        refuse_waiting(&dir, &settings_commit, "hq.v16.model");
        // The conversation and the person's words are still here, waiting to
        // be sent in their own commits.
        assert!(crate::hq::timeline::session_row(&source, &id).is_some());
        let messages = source
            .replica
            .query(
                "SELECT body FROM messages WHERE session_id = ?1",
                &[id.as_str().into()],
            )
            .expect("messages");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["body"], json!("hello there"));
        assert!(outbox(&dir)
            .iter()
            .any(|(_, tables)| tables == &["messages".to_owned()]));
        // And the new session's picker says the pick did not hold.
        cover(&dir, &source, &["dev1.1", &id]);
        assert_eq!(
            picker(&source, &id).expect("picker").write,
            AgentSettingsWriteVm::Refused {
                sentence: "The server refused your model and effort change: the agent does not offer that model.".to_owned()
            }
        );
    }

    #[test]
    fn every_change_answers_the_pick_the_device_keeps() {
        let (_dir, source) = fixture(true);
        let model = change(
            &source,
            "dev1.1",
            AgentSettingsChange::Model {
                model: "second".to_owned(),
            },
            2,
        )
        .expect("model");
        assert_eq!(model.pick.model.as_deref(), Some("second"));
        let effort = change(
            &source,
            "dev1.1",
            AgentSettingsChange::Effort {
                effort: Effort::High,
            },
            3,
        )
        .expect("effort");
        assert_eq!(
            effort.pick,
            AgentSettingsPick {
                model: Some("second".to_owned()),
                effort: Some(Effort::High)
            }
        );
        let reset = change(&source, "dev1.1", AgentSettingsChange::Reset, 4).expect("reset");
        assert_eq!(reset.pick, AgentSettingsPick::default());
    }
}
