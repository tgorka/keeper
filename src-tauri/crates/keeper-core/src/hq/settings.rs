//! Per-session model and effort choices (109.17). No provider logic lives here.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use ts_rs::TS;

use super::sessions::AgentSessionRef;
use crate::bots::chat::Effort;
use crate::hq::port::{RowOp, WindowBase};
use crate::hq::sessions::{authored, int, names, object, opt_text, Source};
use crate::hq::{Hq, HqError};

/// One published definition's defaults, kept per instance rather than guessing
/// a common default when instances disagree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentSettingsDefaultVm {
    pub client_id: String,
    pub model: Option<String>,
    pub effort: Option<Effort>,
}

/// Picker inputs and the saved selection. Null means definition defaults;
/// changing either selection affects future turns, never a running turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
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

fn view(source: &Source, session: &str) -> Result<AgentSessionSettingsVm, HqError> {
    let agent = source.replica.query(
        "SELECT s.agent_actor FROM sessions s JOIN session_members m ON m.session_id = s.id WHERE s.id = ?1 AND m.actor_id = ?2 AND m.role IN ('owner', 'person')",
        &[session.into(), source.actor.as_str().into()],
    ).map_err(sentence)?.first().and_then(|r| opt_text(r, "agent_actor"))
        .ok_or_else(|| sentence("Only a person in this conversation can change its model and effort."))?;
    let profiles = source.replica.query(
        "SELECT id, models, efforts, default_model, default_effort FROM agent_profiles WHERE actor_id = ?1 ORDER BY id",
        &[agent.into()],
    ).map_err(sentence)?;
    let mut models = BTreeSet::new();
    let mut efforts = BTreeSet::new();
    let mut defaults = Vec::new();
    for profile in profiles {
        models.extend(names(&profile, "models"));
        efforts.extend(
            names(&profile, "efforts")
                .iter()
                .filter_map(|name| Effort::parse(name)),
        );
        defaults.push(AgentSettingsDefaultVm {
            client_id: opt_text(&profile, "id").unwrap_or_default(),
            model: opt_text(&profile, "default_model"),
            effort: opt_text(&profile, "default_effort").and_then(|s| Effort::parse(&s)),
        });
    }
    let saved = row(source, session)?;
    Ok(AgentSessionSettingsVm {
        model: saved.as_ref().and_then(|r| opt_text(r, "model")),
        effort: saved
            .as_ref()
            .and_then(|r| opt_text(r, "effort"))
            .and_then(|s| Effort::parse(&s)),
        models: models.into_iter().collect(),
        efforts: efforts.into_iter().collect(),
        defaults,
    })
}

impl Hq {
    /// Read the saved choices and the session agent's published picker inputs.
    pub fn session_settings(
        &self,
        key: &AgentSessionRef,
    ) -> Result<AgentSessionSettingsVm, HqError> {
        view(&self.source(&key.database)?, &key.session_id)
    }

    /// Replace both choices together. None resets that choice to the definition
    /// default. The server remains the authority for membership and offers.
    pub fn set_session_settings(
        &self,
        key: &AgentSessionRef,
        model: Option<&str>,
        effort: Option<Effort>,
    ) -> Result<(), HqError> {
        let source = self.source(&key.database)?;
        set(
            &source,
            &key.session_id,
            model,
            effort,
            super::sessions::now_ms(),
        )?;
        self.touch();
        Ok(())
    }
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
    source
        .replica
        .mutate(vec![RowOp::Upsert {
            table: "session_settings".to_owned(),
            values: authored(
                source,
                object(serde_json::json!({
                    "session_id": session, "model": model, "effort": effort, "updated_at_ms": now,
                })),
            ),
            base_version: version.filter(|v| *v > 0),
        }])
        .map_err(sentence)?;
    source.replica.sync_now();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hq::port::ReplicaSpec;
    use crate::hq::syncular::SyncularReplica;
    use serde_json::json;
    use std::sync::Arc;

    fn fixture(complete: bool) -> (tempfile::TempDir, Source) {
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
        for (id, actor, models, efforts, default) in [
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
}
