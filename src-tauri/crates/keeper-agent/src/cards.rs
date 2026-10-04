//! Cards on the board, as an agent's host writes them (story 92.2, AD-386).
//!
//! Two writers, both through the journaled executor:
//!
//! - **The host's run writer** ([`write_run`]): `run:` and `last_run:`, by
//!   the session's claim holder, on a transition only, as a guarded write
//!   on the bytes it read — re-read and tried once more when the card's
//!   length changed under it (a person's move, a pull), then reported.
//! - **An agent's session tools** (R50): `card_update` sets a card's
//!   `status`, `order`, `assignee` and `host`, and `session_write` writes a
//!   file of the agent's own session. Both are served from the agent's own
//!   host through [`ToolHost::run_named`] — never a ⌘9 bot's (R38) — and both
//!   pass what they write through [`card::stamp_agent_write`], the one door
//!   every agent write of a card goes through (R52). They are the only door:
//!   an agent host's `drive_write` and `drive_edit` never reach the sessions
//!   zone (R51).
//!
//! An agent writes cards of the session its turn runs in, whose claim its
//! host holds (NFR-120): a card in another session or another drive is out of
//! reach by construction.
//!
//! [`ToolHost::run_named`]: keeper_core::bots::tools::ToolHost::run_named

use std::path::Path;

use keeper_core::agents::card::{self, Run};
use keeper_core::agents::label::{check_sink, Readers, Sink, SinkVerdict};
use keeper_core::agents::log::HostSlug;
use keeper_core::bots::chat::{ToolCall as WireToolCall, ToolSpec};
use keeper_core::bots::tools::ToolOutcome;
use keeper_core::notes::frontmatter::{FieldValue, Frontmatter};
use keeper_core::notes::order::set_order_in;
use keeper_core::sessions::files;
use keeper_core::sessions::plan::{Plan, PlanStep};
use keeper_core::sessions::pool::{read_one, PoolFile};
use keeper_core::sessions::shape::{KindTag, TaskStatus};
use keeper_core::sessions::tasks::TASK_STATUS_KEY;
use keeper_sync::browse;
use keeper_sync::tasks::TaskSchedule;
use serde_json::{json, Map, Value};

use crate::delegate::{Delegator, TurnView};
use crate::host::UNATTENDED_REFUSAL;
use crate::sessions::exec::{self, ExecError};
use crate::sessions::lock::ZoneLock;
use crate::sessions::verbs::VerbError;
use crate::sessions::write::{landing, session_write_with, NO_CLAIM};

/// The tool that edits a card's keys.
pub const CARD_UPDATE: &str = "card_update";
/// The tool that writes a file of the agent's own session.
pub const SESSION_WRITE: &str = "session_write";
/// Both tools' tier: a write into the agent's own session, undone by a
/// person's edit (AD-392).
pub const TIER: u8 = 1;

/// What refuses `run` and `last_run`.
pub const HOST_WRITES: &str = "is written by the host that runs the card, never by a tool.";
/// What refuses `scheduled_by` and `integrity`.
pub const KEEPER_WRITES: &str =
    "is written by keeper: it marks what an agent wrote, and only a person removes it.";

/// Whether `name` is `card_update` or `session_write`.
pub fn is_card_tool(name: &str) -> bool {
    name == CARD_UPDATE || name == SESSION_WRITE
}

/// The specs a turn is offered: each tool `[tools].allow` names.
pub fn specs(allow: &[String]) -> Vec<ToolSpec> {
    let allowed = |name: &str| allow.iter().any(|allowed| allowed == name);
    let mut specs = Vec::new();
    if allowed(CARD_UPDATE) {
        specs.push(ToolSpec {
            name: CARD_UPDATE.to_owned(),
            description: "Change a card of this session (a file tagged task): its status (in-preparation, todo, done or deferred), order, assignee or host. A schedule or a workflow needs a person. run, last_run, scheduled_by and integrity are keeper's.".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "card": {"type": "string", "description": "The card's path in this session, e.g. brief.md."},
                    "fields": {
                        "type": "object",
                        "description": "The keys to set.",
                        "properties": {
                            "status": {"type": "string", "enum": keeper_core::sessions::shape::STATUSES.map(TaskStatus::as_str)},
                            "order": {"type": "number"},
                            "assignee": {"type": "string"},
                            "host": {"type": "string"},
                            "schedule": {"type": "string"},
                            "workflow": {"type": "string"}
                        }
                    }
                },
                "required": ["card", "fields"],
                "additionalProperties": false
            }),
        });
    }
    if allowed(SESSION_WRITE) {
        specs.push(ToolSpec {
            name: SESSION_WRITE.to_owned(),
            description: "Write a file into this session, replacing it if it is there: markdown, csv or json anywhere in the session (a card is markdown tagged task), finished output under artifacts/, anything under workspace/. keeper's own files (log/, approvals/, agent.toml, README.md, AGENTS.md) are not yours to write.".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "The file's path in this session."},
                    "content": {"type": "string"}
                },
                "required": ["path", "content"],
                "additionalProperties": false
            }),
        });
    }
    specs
}

/// Set the host's keys of the card at `rel` in the session at `session`
/// (zone-relative): `run:`, and `last_run:` when given. Whether it wrote:
/// a card that already says both is left as it is. `may_write` is the
/// session's claim, asked right before the write (R120).
pub fn write_run(
    zone: &Path,
    session: &str,
    rel: &str,
    run: Run,
    last_run: Option<&str>,
    may_write: &dyn Fn() -> bool,
) -> Result<bool, VerbError> {
    let held = exec::hold(zone)?;
    rewrite(
        &held,
        session,
        rel,
        "card-run",
        |text| Ok(card::set_host_keys(text, run, last_run)),
        may_write,
        &mut |_| {},
    )
}

/// Rewrite one file of a session under the zone's hold, where it lands
/// through the session fence ([`landing`]): `compose` makes the new bytes
/// from the current ones (`None`: nothing to write), and the write is
/// guarded on the exact bytes read (R120). A file that changed by the time
/// the write ran is read and composed once more; a second change is
/// reported. `may_write` is asked right before each write: without the
/// session's claim nothing is written. `between` runs after each read —
/// where another writer's change lands.
fn rewrite(
    held: &ZoneLock,
    session: &str,
    rel: &str,
    verb: &str,
    compose: impl Fn(&str) -> Result<Option<String>, String>,
    may_write: &dyn Fn() -> bool,
    between: &mut dyn FnMut(&Path),
) -> Result<bool, VerbError> {
    let landed = landing(held.zone(), session, rel)?;
    files::check_rewritable(&landed).map_err(|refusal| VerbError::Refused(refusal.to_string()))?;
    let file = browse::lexical_join(held.zone(), &format!("{session}/{landed}"))
        .map_err(|refusal| VerbError::Refused(refusal.to_string()))?;
    let mut attempts = 0;
    loop {
        attempts += 1;
        let text = std::fs::read_to_string(&file)
            .map_err(|error| VerbError::Refused(format!("{rel} could not be read: {error}")))?;
        let Some(content) = compose(&text).map_err(VerbError::Refused)? else {
            return Ok(false);
        };
        between(&file);
        if !may_write() {
            return Err(VerbError::Refused(NO_CLAIM.to_owned()));
        }
        let plan = Plan {
            verb: verb.to_owned(),
            session: session.to_owned(),
            steps: vec![PlanStep::guarded(
                format!("{session}/{landed}"),
                &text,
                content,
            )],
        };
        match exec::run_held(plan, held) {
            Ok(()) => return Ok(true),
            Err(ExecError::Refused(_))
                if attempts == 1 && std::fs::read_to_string(&file).is_ok_and(|now| now != text) => {
            }
            Err(error) => return Err(error.into()),
        }
    }
}

/// Whether a session file is markdown, which the stamp keeps: a card is
/// one, and so is a note an agent may retag as one.
fn is_markdown(landed: &str) -> bool {
    let name = landed
        .rsplit('/')
        .next()
        .unwrap_or(landed)
        .to_ascii_lowercase();
    name.ends_with(".md") || name.ends_with(".markdown")
}

/// One turn's `card_update` and `session_write`.
pub struct CardTools<'t> {
    pub from: Delegator,
    /// The readers of the home drive the session is in: the audience of
    /// every write these tools make.
    pub drive_readers: Readers,
    pub view: &'t dyn TurnView,
    pub allow: &'t [String],
}

fn refused(reason: impl Into<String>) -> Option<ToolOutcome> {
    Some(ToolOutcome::Refused {
        reason: reason.into(),
    })
}

/// The keys `card_update` sets, checked and turned into frontmatter values,
/// or the sentence refusing the first one it may not.
fn checked_fields(fields: &Map<String, Value>) -> Result<Vec<(&str, FieldValue)>, String> {
    let text = |key: &str, value: &Value| {
        value
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| format!("{key} is a non-empty string."))
    };
    let mut out = Vec::new();
    let mut needs_person = false;
    for (key, value) in fields {
        let set = match key.as_str() {
            "status" => {
                let word = text(key, value)?;
                let status = TaskStatus::parse(&word).ok_or_else(|| {
                    format!("{word} is not one of the board's columns: in-preparation, todo, done or deferred. An agent's run is not a column.")
                })?;
                FieldValue::Str(status.as_str().to_owned())
            }
            "order" => match value.as_f64().filter(|n| n.is_finite()) {
                Some(n) => FieldValue::Num(n),
                None => return Err("order is a number.".to_owned()),
            },
            card::ASSIGNEE => {
                let id = text(key, value)?;
                if !keeper_core::agents::home::is_agent_id(&id) {
                    return Err(format!("{id} is not an agent id: lower-case letters, digits and dashes."));
                }
                FieldValue::Str(id)
            }
            card::HOST => {
                let slug = text(key, value)?;
                HostSlug::new(&slug).map_err(|_| format!("{slug} is not a host's name: lower-case letters, digits and dashes."))?;
                FieldValue::Str(slug)
            }
            card::SCHEDULE => {
                let schedule = text(key, value)?;
                TaskSchedule::parse(&schedule).map_err(|refusal| refusal.to_string())?;
                needs_person = true;
                FieldValue::Str(schedule)
            }
            card::WORKFLOW => {
                text(key, value)?;
                needs_person = true;
                continue;
            }
            card::RUN | card::LAST_RUN => return Err(format!("{key} {HOST_WRITES}")),
            card::SCHEDULED_BY | card::INTEGRITY => return Err(format!("{key} {KEEPER_WRITES}")),
            card::ALLOWED_BY => {
                return Err(format!("{key} is written when a person allows a schedule."))
            }
            card::REQUESTED_BY => {
                return Err("requested_by is set when a card is made; card_update changes a card that exists.".to_owned())
            }
            other => {
                return Err(format!(
                    "card_update sets status, order, assignee, host, schedule and workflow; {other} is none of them."
                ))
            }
        };
        out.push((key.as_str(), set));
    }
    // A schedule or a workflow is a person's to give (Q16, T3).
    if needs_person {
        return Err(UNATTENDED_REFUSAL.to_owned());
    }
    Ok(out)
}

impl CardTools<'_> {
    /// Run `wire` when it is `card_update` or `session_write`; `None` for any
    /// other name.
    pub fn run(&self, wire: &WireToolCall) -> Option<ToolOutcome> {
        if !is_card_tool(&wire.name) {
            return None;
        }
        if !self.allow.contains(&wire.name) {
            return refused(format!("{} is not one of this agent's tools.", wire.name));
        }
        let args = wire.arguments.as_ref();
        let text = |key: &str| args.and_then(|args| args[key].as_str());
        if let SinkVerdict::Block { reason, .. } = check_sink(
            &self.view.label(),
            &Sink::DriveWrite {
                drive_readers: self.drive_readers.clone(),
            },
        ) {
            return refused(reason);
        }
        match wire.name.as_str() {
            CARD_UPDATE => {
                let (Some(rel), Some(fields)) = (
                    text("card"),
                    args.and_then(|args| args["fields"].as_object()),
                ) else {
                    return refused("card_update needs \"card\" and \"fields\" arguments.");
                };
                self.update(rel, fields)
            }
            _ => {
                let (Some(rel), Some(content)) = (text("path"), text("content")) else {
                    return refused("session_write needs \"path\" and \"content\" arguments.");
                };
                self.write(rel, content)
            }
        }
    }

    fn update(&self, rel: &str, fields: &Map<String, Value>) -> Option<ToolOutcome> {
        let fields = match checked_fields(fields) {
            Ok(fields) if fields.is_empty() => {
                return refused("card_update was given no key to set.")
            }
            Ok(fields) => fields,
            Err(sentence) => return refused(sentence),
        };
        let integrity = self.view.label().integrity;
        let compose = |text: &str| {
            let read = read_one(PoolFile { rel, text });
            if read.kind != Some(KindTag::Task) {
                return Err(format!(
                    "{rel} is not a card: a card is a file tagged task."
                ));
            }
            let mut out = text.to_owned();
            for (key, value) in &fields {
                out = match (*key, value) {
                    ("order", FieldValue::Num(n)) => set_order_in(&out, *n),
                    ("status", value) => Frontmatter::set_in(&out, TASK_STATUS_KEY, value.clone()),
                    (key, value) => {
                        Frontmatter::set_after_in(&out, &card::KEYS, key, value.clone())
                    }
                };
            }
            let out = card::stamp_agent_write(Some(text), &out, &self.from.user, integrity);
            Ok((out != text).then_some(out))
        };
        let wrote = exec::hold(&self.from.zone)
            .map_err(VerbError::from)
            .and_then(|held| {
                rewrite(
                    &held,
                    &self.from.session,
                    rel,
                    "card-update",
                    compose,
                    &|| self.view.may_write(),
                    &mut |_| {},
                )
            });
        match wrote {
            Ok(true) => Some(ToolOutcome::Answered {
                text: format!("Updated {rel}."),
            }),
            Ok(false) => Some(ToolOutcome::Answered {
                text: format!("{rel} already says that; nothing was written."),
            }),
            Err(VerbError::Refused(sentence)) => refused(sentence),
            Err(error) => refused(format!("{rel} could not be updated: {error}")),
        }
    }

    fn write(&self, rel: &str, content: &str) -> Option<ToolOutcome> {
        let integrity = self.view.label().integrity;
        // Every markdown file, card or not: a note retagged later carries
        // what the stamp put on it (R119).
        let written = session_write_with(
            &self.from.zone,
            &self.from.session,
            rel,
            &|| self.view.may_write(),
            |landed, old| {
                if is_markdown(landed) {
                    card::stamp_agent_write(old, content, &self.from.user, integrity)
                } else {
                    content.to_owned()
                }
            },
        );
        match written {
            Ok(()) => Some(ToolOutcome::Answered {
                text: format!("Wrote {rel}."),
            }),
            Err(error) => refused(error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use keeper_core::agents::delegation::CARD_FILE;
    use keeper_core::agents::home;
    use keeper_core::agents::label::{Integrity, Label};
    use keeper_core::agents::session::SessionKind;
    use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId};

    use super::*;
    use crate::delegate::Delegation;

    const SESSION: &str = "active/2026-10-04-tola";
    const CARD: &str = "---\ntags: [task]\ntitle: Sort the inbox\nstatus: todo\norder: 1\nassignee: tola\nrequested_by: \"@nixi:h\"\nrun: queued\n---\n\nSort what came in.\n";

    struct View(Label, std::sync::atomic::AtomicBool);

    impl TurnView for View {
        fn label(&self) -> Label {
            self.0.clone()
        }
        fn delegation(&self, _: &str) -> Option<Delegation> {
            None
        }
        fn may_write(&self) -> bool {
            self.1.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    fn view(integrity: Integrity) -> View {
        View(label(integrity), std::sync::atomic::AtomicBool::new(true))
    }

    fn tgorka() -> OwnedUserId {
        OwnedUserId::try_from("@tgorka:h").expect("user")
    }

    fn label(integrity: Integrity) -> Label {
        Label {
            readers: Readers::Only([tgorka()].into_iter().collect()),
            integrity,
            local_only: false,
        }
    }

    /// A zone holding one delegated session with its card.
    fn zone() -> tempfile::TempDir {
        let zone = tempfile::tempdir().expect("zone");
        let session = zone.path().join(SESSION);
        std::fs::create_dir_all(&session).expect("session");
        std::fs::write(session.join(CARD_FILE), CARD).expect("card");
        zone
    }

    fn card_text(zone: &Path) -> String {
        std::fs::read_to_string(zone.join(SESSION).join(CARD_FILE)).expect("card")
    }

    fn tools<'t>(zone: &Path, view: &'t View, allow: &'t [String]) -> CardTools<'t> {
        CardTools {
            from: Delegator {
                user: OwnedUserId::try_from("@tola:h").expect("user"),
                drive: "tgdrive".to_owned(),
                id: "01J5AAAAAAAAAAAAAAAAAAAAAA".to_owned(),
                session: SESSION.to_owned(),
                room: OwnedRoomId::try_from("!r:h").expect("room"),
                kind: SessionKind::Delegated,
                requester: tgorka(),
                hop: 1,
                limits: home::Limits {
                    rounds_per_turn: 8,
                    tokens_per_turn: 0,
                    tokens_per_delegation: 200_000,
                    hop_limit: 3,
                    rounds_per_exchange: 3,
                    max_concurrent_sessions: 1,
                },
                zone: zone.to_owned(),
                subfolder: "60-sessions".to_owned(),
                chain: Vec::new(),
            },
            drive_readers: Readers::Only([tgorka()].into_iter().collect()),
            view,
            allow,
        }
    }

    fn call(name: &str, args: Value) -> WireToolCall {
        WireToolCall {
            id: "c1".to_owned(),
            name: name.to_owned(),
            arguments_raw: args.to_string(),
            arguments: Some(args),
        }
    }

    fn allow() -> Vec<String> {
        vec![CARD_UPDATE.to_owned(), SESSION_WRITE.to_owned()]
    }

    fn refusal(outcome: Option<ToolOutcome>) -> String {
        match outcome {
            Some(ToolOutcome::Refused { reason }) => reason,
            other => panic!("not refused: {other:?}"),
        }
    }

    /// AC4: `run`, `last_run`, `scheduled_by` and `integrity` are each
    /// refused with their sentence, and the card's bytes stay as they were;
    /// a column is set as one key.
    #[test]
    fn card_update_refuses_the_keys_keeper_writes() {
        let zone = zone();
        let view = view(Integrity::Agent);
        let allow = allow();
        let tools = tools(zone.path(), &view, &allow);
        let update = |fields: Value| {
            tools.run(&call(
                CARD_UPDATE,
                json!({"card": CARD_FILE, "fields": fields}),
            ))
        };
        for (key, sentence) in [
            ("run", HOST_WRITES),
            ("last_run", HOST_WRITES),
            ("scheduled_by", KEEPER_WRITES),
            ("integrity", KEEPER_WRITES),
        ] {
            assert_eq!(
                refusal(update(json!({ key: "x" }))),
                format!("{key} {sentence}")
            );
        }
        assert!(refusal(update(json!({"requested_by": "@marta:h"}))).contains("requested_by"));
        assert!(refusal(update(json!({"status": "blocked"})))
            .contains("not one of the board's columns"));
        assert_eq!(card_text(zone.path()), CARD, "nothing was written");

        assert!(matches!(
            update(json!({"status": "done", "order": 2.5})),
            Some(ToolOutcome::Answered { .. })
        ));
        assert_eq!(
            card_text(zone.path()),
            CARD.replace("status: todo", "status: done")
                .replace("order: 1\n", "order: 2.5\n")
        );
    }

    /// R52: `card_update` passes the stamp too — a session at `untrusted`
    /// integrity marks the card it moves.
    #[test]
    fn a_card_update_from_outside_content_marks_the_card() {
        let zone = zone();
        let view = view(Integrity::Untrusted);
        let allow = allow();
        let tools = tools(zone.path(), &view, &allow);
        tools.run(&call(
            CARD_UPDATE,
            json!({"card": CARD_FILE, "fields": {"status": "done"}}),
        ));
        let text = card_text(zone.path());
        assert!(text.contains("status: done"), "{text}");
        assert_eq!(
            Frontmatter::parse(&text).0.as_string(card::INTEGRITY),
            Some("untrusted")
        );
    }

    /// AC5: a schedule is checked where it is written, and a readable one —
    /// or any workflow — needs a person, so before Epic 93 nothing is written.
    #[test]
    fn the_schedule_is_checked_where_it_is_written() {
        let zone = zone();
        let view = view(Integrity::Agent);
        let allow = allow();
        let tools = tools(zone.path(), &view, &allow);
        let update = |fields: Value| {
            refusal(tools.run(&call(
                CARD_UPDATE,
                json!({"card": CARD_FILE, "fields": fields}),
            )))
        };
        assert!(update(json!({"schedule": "every 30s"})).contains("more often than once a minute"));
        assert!(update(json!({"schedule": "0 0 30 2 *"})).contains("matches no instant"));
        assert_eq!(update(json!({"schedule": "@daily"})), UNATTENDED_REFUSAL);
        assert_eq!(update(json!({"workflow": "triage"})), UNATTENDED_REFUSAL);
        assert_eq!(card_text(zone.path()), CARD);
    }

    /// AC4, R120: the host writes `run:` only on a transition, one key, every
    /// other byte kept; a card changed under the write — even by an edit of
    /// the same length — is read again and written once more, and a second
    /// change is reported.
    #[test]
    fn the_host_writes_run_only_on_a_transition() {
        let zone = zone();
        assert!(
            !write_run(zone.path(), SESSION, CARD_FILE, Run::Queued, None, &|| true)
                .expect("queued")
        );
        assert_eq!(
            card_text(zone.path()),
            CARD,
            "running → running writes nothing"
        );
        assert!(
            write_run(zone.path(), SESSION, CARD_FILE, Run::Running, None, &|| {
                true
            })
            .expect("running")
        );
        assert_eq!(
            card_text(zone.path()),
            CARD.replace("run: queued", "run: running")
        );

        // A person moves the card between the read and the write.
        let held = exec::hold(zone.path()).expect("hold");
        let mut moves = 0;
        let wrote = rewrite(
            &held,
            SESSION,
            CARD_FILE,
            "card-run",
            |text| Ok(card::set_host_keys(text, Run::Review, None)),
            &|| true,
            &mut |file| {
                moves += 1;
                if moves == 1 {
                    let text = std::fs::read_to_string(file).expect("read");
                    std::fs::write(file, text.replace("status: todo", "status: done"))
                        .expect("move");
                }
            },
        )
        .expect("retried once");
        assert!(wrote);
        assert_eq!(moves, 2, "read again after the move");
        let text =
            std::fs::read_to_string(zone.path().join(SESSION).join(CARD_FILE)).expect("card");
        assert!(
            text.contains("status: done") && text.contains("run: review"),
            "{text}"
        );

        // A card that keeps changing is reported after the second try.
        let mut grows = 0;
        let error = rewrite(
            &held,
            SESSION,
            CARD_FILE,
            "card-run",
            |text| Ok(card::set_host_keys(text, Run::Failed, None)),
            &|| true,
            &mut |file| {
                grows += 1;
                let text = std::fs::read_to_string(file).expect("read");
                std::fs::write(file, format!("{text}more\n")).expect("grow");
            },
        )
        .expect_err("reported");
        assert_eq!(grows, 2);
        assert!(
            matches!(error, VerbError::Exec(ExecError::Refused(_))),
            "{error:?}"
        );
        drop(held);
        assert!(!card_text(zone.path()).contains("run: failed"));
    }

    /// AC10 (S-21): a `session_write` of a card that sets `schedule:` stores
    /// `scheduled_by: <the agent>` whatever it wrote there; a later rewrite
    /// dropping `scheduled_by` or `integrity` stores them again; a write that
    /// changes neither adds no mark; a session at `untrusted` integrity marks
    /// the card it writes.
    #[test]
    fn an_agents_schedule_is_stamped_and_never_unstamped_by_an_agent() {
        let zone = zone();
        let allow = allow();
        let agent = view(Integrity::Agent);
        let mine = tools(zone.path(), &agent, &allow);
        let write = |tools: &CardTools<'_>, rel: &str, content: &str| {
            tools.run(&call(
                SESSION_WRITE,
                json!({"path": rel, "content": content}),
            ))
        };
        let field = |rel: &str, key: &str| {
            let text = std::fs::read_to_string(zone.path().join(SESSION).join(rel)).expect("file");
            Frontmatter::parse(&text)
                .0
                .as_string(key)
                .map(str::to_owned)
        };
        let scheduled = CARD.replace(
            "run: queued\n",
            "schedule: \"@daily\"\nscheduled_by: \"@tgorka:h\"\n",
        );
        assert!(matches!(
            write(&mine, "cards/daily.md", &scheduled),
            Some(ToolOutcome::Answered { .. })
        ));
        assert_eq!(
            field("cards/daily.md", card::SCHEDULED_BY).as_deref(),
            Some("@tola:h")
        );

        let dropped = scheduled.replace("scheduled_by: \"@tgorka:h\"\n", "");
        write(&mine, "cards/daily.md", &dropped);
        assert_eq!(
            field("cards/daily.md", card::SCHEDULED_BY).as_deref(),
            Some("@tola:h")
        );

        write(&mine, "plain.md", CARD);
        assert_eq!(
            field("plain.md", card::SCHEDULED_BY),
            None,
            "no schedule, no mark"
        );

        let outside = view(Integrity::Untrusted);
        let untrusted = tools(zone.path(), &outside, &allow);
        write(&untrusted, "from-inbox.md", CARD);
        assert_eq!(
            field("from-inbox.md", card::INTEGRITY).as_deref(),
            Some("untrusted")
        );
        write(&mine, "from-inbox.md", CARD);
        assert_eq!(
            field("from-inbox.md", card::INTEGRITY).as_deref(),
            Some("untrusted"),
            "an agent's rewrite keeps the mark"
        );

        assert!(refusal(write(&mine, "log/x.md", CARD)).contains("keeper's own record"));
    }

    /// A tool `[tools].allow` does not name is refused, and nothing is
    /// written; a label beyond the drive's readers writes nothing.
    #[test]
    fn a_card_tool_needs_the_allow_and_the_drives_readers() {
        let zone = zone();
        let view = view(Integrity::Agent);
        let none: Vec<String> = Vec::new();
        let unlisted = tools(zone.path(), &view, &none);
        assert!(refusal(unlisted.run(&call(
            SESSION_WRITE,
            json!({"path": "x.md", "content": "x"})
        )))
        .contains("not one of this agent's tools"));

        let allow = allow();
        let mut narrow_drive = tools(zone.path(), &view, &allow);
        narrow_drive.drive_readers = Readers::Only(
            [tgorka(), OwnedUserId::try_from("@marta:h").expect("user")]
                .into_iter()
                .collect(),
        );
        refusal(narrow_drive.run(&call(
            SESSION_WRITE,
            json!({"path": "x.md", "content": "x"}),
        )));
        assert!(!zone.path().join(SESSION).join("x.md").exists());
    }

    /// R119 (R4-01): a folder link is followed to where it lands, and the
    /// fence is asked there — a link in `workspace/` back into the session
    /// cannot reach `agent.toml` or the log, and a link to another session's
    /// cards cannot carry a `card_update` into it.
    #[test]
    fn a_folder_link_never_carries_a_write_out_of_the_fence() {
        let zone = zone();
        let session = zone.path().join(SESSION);
        std::fs::write(session.join("agent.toml"), "kept\n").expect("agent.toml");
        std::fs::create_dir_all(session.join("workspace")).expect("workspace");
        std::os::unix::fs::symlink("..", session.join("workspace/back")).expect("link");
        let other = zone.path().join("active/2026-10-04-other/cards");
        std::fs::create_dir_all(&other).expect("other");
        std::fs::write(other.join("task.md"), CARD).expect("their card");
        std::os::unix::fs::symlink("../2026-10-04-other/cards", session.join("cards"))
            .expect("link");
        let view = view(Integrity::Agent);
        let allow = allow();
        let tools = tools(zone.path(), &view, &allow);
        for path in ["workspace/back/agent.toml", "workspace/back/log/x.md"] {
            refusal(tools.run(&call(
                SESSION_WRITE,
                json!({"path": path, "content": "taken"}),
            )));
        }
        assert_eq!(
            std::fs::read_to_string(session.join("agent.toml")).expect("read"),
            "kept\n"
        );
        assert!(!session.join("log").exists());
        refusal(tools.run(&call(
            CARD_UPDATE,
            json!({"card": "cards/task.md", "fields": {"status": "done"}}),
        )));
        refusal(tools.run(&call(
            SESSION_WRITE,
            json!({"path": "cards/task.md", "content": "taken"}),
        )));
        assert_eq!(
            std::fs::read_to_string(other.join("task.md")).expect("read"),
            CARD
        );
    }

    /// R119 (R4-02): keeper's own names are refused however they are
    /// spelled, as the Mac's volume reads them alike.
    #[test]
    fn keepers_names_are_refused_in_any_case() {
        let zone = zone();
        let view = view(Integrity::Agent);
        let allow = allow();
        let tools = tools(zone.path(), &view, &allow);
        for path in [
            "readme.md",
            "Agents.md",
            "Approvals/x.json",
            "Log/blobs/x.json",
            "LOG/x.md",
        ] {
            assert!(
                refusal(tools.run(&call(SESSION_WRITE, json!({"path": path, "content": "{}"}),)))
                    .contains("keeper's own record"),
                "{path}"
            );
        }
        let entries: Vec<_> = std::fs::read_dir(zone.path().join(SESSION))
            .expect("session")
            .flatten()
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(entries, [std::ffi::OsString::from(CARD_FILE)]);
    }

    /// R119 (R4-03, R4-04): through the tool, a rewrite cannot change the
    /// host's `run:`, and a note staged with a schedule and a forged
    /// `allowed_by:`, then retagged as a card, reaches the board stamped as
    /// the agent's and with no person's tick.
    #[test]
    fn the_tools_never_launder_a_card() {
        let zone = zone();
        let view = view(Integrity::Agent);
        let allow = allow();
        let tools = tools(zone.path(), &view, &allow);
        let write = |rel: &str, content: &str| {
            tools.run(&call(
                SESSION_WRITE,
                json!({"path": rel, "content": content}),
            ))
        };
        write(CARD_FILE, &CARD.replace("run: queued", "run: review"));
        assert_eq!(card_text(zone.path()), CARD, "run is the host's");

        let note = "---\ntags: [note]\ntitle: Daily\nschedule: \"@daily\"\nallowed_by: \"@tgorka:h\"\n---\n\nRun daily.\n";
        write("daily.md", note);
        let staged =
            std::fs::read_to_string(zone.path().join(SESSION).join("daily.md")).expect("note");
        write("daily.md", &staged.replace("[note]", "[task]"));
        let card =
            std::fs::read_to_string(zone.path().join(SESSION).join("daily.md")).expect("card");
        let (fm, _) = Frontmatter::parse(&card);
        assert_eq!(fm.as_string(card::SCHEDULED_BY), Some("@tola:h"), "{card}");
        assert_eq!(fm.count(card::ALLOWED_BY), 0, "{card}");

        // A card's mark survives being retagged a note, stripped there, and
        // retagged a card again.
        let marked = CARD.replace("run: queued\n", "integrity: untrusted\n");
        std::fs::write(zone.path().join(SESSION).join("marked.md"), &marked).expect("card");
        let note = marked.replace("[task]", "[note]");
        write("marked.md", &note);
        write("marked.md", &note.replace("integrity: untrusted\n", ""));
        write(
            "marked.md",
            &note
                .replace("integrity: untrusted\n", "")
                .replace("[note]", "[task]"),
        );
        let back =
            std::fs::read_to_string(zone.path().join(SESSION).join("marked.md")).expect("card");
        assert!(card::marked_untrusted(&back), "{back}");
    }

    /// R120 (R4-10): a write that waited for the zone while its host lost
    /// the session's claim writes nothing — the agent's tool and the host's
    /// run writer alike.
    #[test]
    fn a_write_without_the_claim_has_no_effect() {
        let zone = zone();
        let view = view(Integrity::Agent);
        let allow = allow();
        let held = exec::hold(zone.path()).expect("hold");
        std::thread::scope(|scope| {
            let waiting = scope.spawn(|| {
                tools(zone.path(), &view, &allow).run(&call(
                    CARD_UPDATE,
                    json!({"card": CARD_FILE, "fields": {"status": "done"}}),
                ))
            });
            std::thread::sleep(std::time::Duration::from_millis(200));
            view.1.store(false, std::sync::atomic::Ordering::SeqCst);
            drop(held);
            assert_eq!(refusal(waiting.join().expect("tool")), NO_CLAIM);
        });
        assert_eq!(card_text(zone.path()), CARD);
        let lost = write_run(zone.path(), SESSION, CARD_FILE, Run::Review, None, &|| {
            false
        });
        assert!(matches!(lost, Err(VerbError::Refused(sentence)) if sentence == NO_CLAIM));
        assert_eq!(card_text(zone.path()), CARD);
        let written =
            crate::sessions::write::session_write(zone.path(), SESSION, "x.md", "x\n", &|| false);
        assert!(matches!(written, Err(VerbError::Refused(sentence)) if sentence == NO_CLAIM));
        assert!(!zone.path().join(SESSION).join("x.md").exists());
    }

    /// R122 (R4-14): a person's move on one copy and the host's first
    /// `run:` on another merge line by line, through the `git merge -X
    /// theirs` keeper-sync's engine converges with, keeping both edits — on
    /// a card whose agent keys sit beside `status:`/`order:`, and on one
    /// with none. Had the two edits touched, `-X theirs` would have dropped
    /// one side's.
    #[test]
    fn a_move_and_a_run_merge_through_gits_own_merge() {
        use keeper_core::sessions::tasks::{compile_move, TaskFile};
        use keeper_sync::git::cli::MergeOutcome;
        use keeper_sync::git::GitCli;

        let git = |repo: &Path, args: &[&str]| {
            let status = std::process::Command::new("git")
                .current_dir(repo)
                .args([
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@h",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .output()
                .expect("git");
            assert!(status.status.success(), "git {args:?}: {status:?}");
        };
        for base in [
            "---\ntags: [task]\nassignee: tola-grey\nstatus: todo\norder: 1\n---\n\nBody.\n",
            "---\ntags: [task]\ntitle: Sort\nstatus: todo\norder: 1\n---\n\nBody.\n",
        ] {
            let repo = tempfile::tempdir().expect("repo");
            let repo = repo.path();
            git(repo, &["init", "-q", "-b", "main"]);
            std::fs::write(repo.join("card.md"), base).expect("card");
            git(repo, &["add", "card.md"]);
            git(repo, &["commit", "-q", "-m", "base"]);
            git(repo, &["checkout", "-q", "-b", "host"]);
            let ran = card::set_host_keys(base, Run::Running, Some("2026-10-04T09:00:00+02:00"))
                .expect("a transition");
            std::fs::write(repo.join("card.md"), &ran).expect("run");
            git(repo, &["commit", "-q", "-am", "run"]);
            git(repo, &["checkout", "-q", "main"]);
            let other = "---\ntags: [task]\nstatus: done\norder: 1\n---\n";
            let plan = compile_move(
                "active/s",
                "card.md",
                base,
                TaskStatus::Done,
                &[TaskFile {
                    rel: "other.md",
                    text: other,
                    order: 1.0,
                }],
                1,
            )
            .expect("a move");
            let Some(PlanStep::GuardedWrite { content: moved, .. }) = plan.steps.last() else {
                panic!("the moved card is written last");
            };
            std::fs::write(repo.join("card.md"), moved).expect("move");
            git(repo, &["commit", "-q", "-am", "move"]);

            let merged = GitCli::new("git".into())
                .merge_theirs(repo, "host", "merge")
                .expect("merge");
            assert_eq!(merged, MergeOutcome::Clean);
            let text = std::fs::read_to_string(repo.join("card.md")).expect("merged");
            let (fm, _) = Frontmatter::parse(&text);
            assert_eq!(fm.as_string(TASK_STATUS_KEY), Some("done"), "{text}");
            assert_eq!(fm.get("order"), Some(&FieldValue::Num(2.0)), "{text}");
            assert_eq!(fm.as_string(card::RUN), Some("running"), "{text}");
            assert!(fm.as_string(card::LAST_RUN).is_some(), "{text}");
        }
    }
}
