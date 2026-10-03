//! The agents' Matrix events: room types, event types, the contents 90.4–90.6
//! send, and a session room's power levels (AD-370, AD-372; *Matrix events*).
//!
//! Every content is a closed struct: it serialises exactly its schema's keys,
//! and reading one refuses a key it does not know, so a state event — which is
//! not encrypted — can never be made to carry a title, a path or text by a
//! field nobody meant to add (the architecture's Ambiguity 7).

use std::time::Duration;

use matrix_sdk::ruma::{OwnedEventId, OwnedUserId, UserId};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use ts_rs::TS;

use crate::agents::session::SessionKind;

/// `m.room.create` `type` of every session room: a proxy's DM, a proxy
/// conversation, a delegated, scheduled, workflow or gate session.
pub const SESSION_ROOM_TYPE: &str = "dev.keeper.agent.session";
/// `m.room.create` `type` of a principal's control room.
pub const CONTROL_ROOM_TYPE: &str = "dev.keeper.agent.control";

/// The session's status anchor and its edits.
pub const STATUS: &str = "dev.keeper.agent.status";
/// The key inside a streamed answer's anchor content naming its log line.
pub const TURN: &str = "dev.keeper.agent.turn";
/// The drives in scope and the label (people may send it in a proxy's rooms).
pub const SCOPE: &str = "dev.keeper.agent.scope";
/// A person's decision on an approval.
pub const APPROVAL_DECISION: &str = "dev.keeper.agent.approval.decision";
/// Where a spoken answer stopped.
pub const HEARD: &str = "dev.keeper.agent.heard";
/// A device's answer to a surface call.
pub const SURFACE_RESULT: &str = "dev.keeper.agent.surface.result";
/// The claim on a session (state, key `""`).
pub const CLAIM: &str = "dev.keeper.agent.claim";
/// A host's manifest (state, key = the host slug).
pub const HOST: &str = "dev.keeper.agent.host";
/// A person's ask, in their proxy's `main` DM, for a new conversation
/// (ruling R36): only the main session's claim holder acts on it.
pub const CONVERSATION_REQUEST: &str = "dev.keeper.agent.conversation.request";
/// A host's ask to the one device of its person that is in front: open a
/// note, highlight, point, scroll, or propose an edit (AD-383).
pub const SURFACE_REQUEST: &str = "dev.keeper.agent.surface.request";
/// One of a person's keeper clients, and whether it is in front (state in
/// the principal's control room, key = its Matrix device id; AD-383).
pub const PRESENCE: &str = "dev.keeper.agent.presence";

/// The contents' schema version, `"v": 1`.
pub const CONTENT_VERSION: u32 = 1;

/// The longest final message of a streamed answer, in bytes (R23). Longer
/// answers are cut on a `char` boundary and point to an artifact holding
/// the whole text; a device draws an answer up to this and that sentence
/// (R42).
///
/// Measured, not chosen (90.5 acceptance 14): the largest text whose
/// encrypted final edit the Synapse test homeserver accepted was 47 061
/// bytes (2026-10-03; the server's 64 KiB event cap after Megolm and base64),
/// so R23's 60 KiB does not fit. This is that, less room for the artifact
/// sentence, rounded down to 1 KiB (`docs/agents.md` § Measured).
pub const FINAL_CUT_BYTES: usize = 45 * 1024;

/// `dev.keeper.agent.status` (timeline, encrypted).
///
/// `kind` tells a device a proxy conversation (`main`, `conversation`) from a
/// session it only watches (R25, F7). `detail` carries counts, never paths or
/// titles (S-16).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusContent {
    pub v: u32,
    /// The session's drive-relative path.
    pub session: String,
    pub kind: SessionKind,
    pub title: String,
    pub agent: OwnedUserId,
    pub host: String,
    pub epoch: u64,
    pub run: RunState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// The host the session waits for, with `run: waiting`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting: Option<String>,
    /// The status anchor this edit replaces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<OwnedEventId>,
}

/// A status's `run`: the five states a device shows; any other word is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunState {
    Idle,
    Running,
    Blocked,
    /// Waiting for the host named in `waiting`.
    Waiting,
    Done,
}

/// `dev.keeper.agent.turn` inside a streamed answer's anchor: which session
/// and which log line the answer belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnRef {
    pub session: String,
    /// The `user` line's id.
    pub line: String,
}

/// `dev.keeper.agent.scope` (timeline, encrypted).
///
/// The person's device sends it in its proxy's own rooms: `drives` asks for
/// a scope (absent: the scope stays), `focus` says what the docked notes
/// view shows (absent: nothing). The owning host echoes each accepted scope
/// with the drives' titles and the session's label, which is what a room's
/// scope and label chips read (R30: only the agent's own scope is shown).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeContent {
    pub v: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drives: Option<Vec<ScopeDrive>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<crate::agents::label::Label>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<Focus>,
    /// Who chose the scope: the person, also in the host's echo.
    pub set_by: OwnedUserId,
}

/// A drive in a scope event, in the scope's order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeDrive {
    pub id: String,
    pub title: String,
}

/// The note the person is looking at in the docked notes view: a drive, a
/// drive-relative path and the heading above the caret (AD-382).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Focus {
    pub drive: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading: Option<String>,
}

/// `dev.keeper.agent.conversation.request` (timeline, encrypted).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationRequestContent {
    pub v: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// `dev.keeper.agent.claim` (state, key `""`, unencrypted): who writes the
/// session (AD-378). Times are RFC 3339 UTC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimContent {
    pub v: u32,
    pub host: String,
    pub device: String,
    pub agent: OwnedUserId,
    pub epoch: u64,
    pub acquired_at: String,
    pub renewed_at: String,
    pub expires_at: String,
    pub released: bool,
    /// The start of the scheduled-card window the holder runs (S-25).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<String>,
}

/// `dev.keeper.agent.presence` (state, key = the device id, unencrypted):
/// metadata only — which device, on which platform, whether it is in front
/// and which primary view it shows. No path, no title, no drive (AD-383).
/// Times are RFC 3339 UTC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresenceContent {
    pub v: u32,
    pub user: OwnedUserId,
    pub device: String,
    pub platform: PresencePlatform,
    pub focused: bool,
    /// A primary view's id (`notes`, `chats`), never a note.
    pub view: String,
    pub renewed_at: String,
    pub expires_at: String,
}

/// The platform a presence names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresencePlatform {
    Macos,
    Ios,
    Android,
}

/// How long a host waits for a device's answer to a surface call (AD-383).
/// The device counts it from when the server received the request — the one
/// clock both sides read — never from `expires_at`, the host's own clock.
pub const SURFACE_WAIT: Duration = Duration::from_secs(60);

/// `dev.keeper.agent.surface.request` (timeline, encrypted): one surface
/// call, for the device `device` only; `expires_at` is when the host says it
/// stops waiting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceRequestContent {
    pub v: u32,
    pub id: String,
    pub device: String,
    pub tool: SurfaceTool,
    pub args: SurfaceArgs,
    pub expires_at: String,
}

/// What a surface call asks the device to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SurfaceTool {
    Open,
    Highlight,
    Point,
    Scroll,
    ProposeEdit,
}

impl SurfaceTool {
    /// The tool's name as the model calls it.
    pub fn wire(self) -> &'static str {
        match self {
            Self::Open => "surface_open",
            Self::Highlight => "surface_highlight",
            Self::Point => "surface_point",
            Self::Scroll => "surface_scroll",
            Self::ProposeEdit => "surface_propose_edit",
        }
    }

    /// The five, in the order a turn offers them.
    pub const ALL: [Self; 5] = [
        Self::Open,
        Self::Highlight,
        Self::Point,
        Self::Scroll,
        Self::ProposeEdit,
    ];

    /// The tool the model called `name`, if it is one of the five.
    pub fn from_wire(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tool| tool.wire() == name)
    }
}

/// A surface call's arguments. `range` counts lines of the note's body —
/// the editor's buffer, without the frontmatter — 1-based and inclusive;
/// `expected` is the text the agent read in that range, so the device
/// applies a proposal only while its buffer still holds it (R40).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceArgs {
    pub drive: String,
    /// The note's path from the drive's root, `/`-joined.
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<LineSpan>,
    /// `propose_edit`: the lines that replace `range`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
}

/// Lines `from` through `to` of a note's body, 1-based and inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct LineSpan {
    pub from: u32,
    pub to: u32,
}

/// `dev.keeper.agent.surface.result` (timeline, encrypted): the named
/// device's answer to the request `request`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceResultContent {
    pub v: u32,
    pub request: String,
    pub device: String,
    pub outcome: SurfaceOutcome,
    /// `propose_edit`: whether the person applied it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// How a surface call ended, in the word the model is told.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum SurfaceOutcome {
    Done,
    Declined,
    Expired,
    Unavailable,
}

impl SurfaceOutcome {
    pub fn word(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Declined => "declined",
            Self::Expired => "expired",
            Self::Unavailable => "unavailable",
        }
    }
}

/// A streamed answer's edit: `m.replace` of `target`, the whole text in
/// `m.new_content`, and a fallback `body` of at most [`FALLBACK_BODY_MAX`]
/// bytes, so an edit never carries its text twice (AD-373).
pub fn edit_content(target: &OwnedEventId, text: &str) -> Value {
    json!({
        "msgtype": "m.text",
        "body": fallback(text),
        "m.new_content": { "msgtype": "m.text", "body": text },
        "m.relates_to": { "rel_type": "m.replace", "event_id": target },
    })
}

/// The most bytes an edit's fallback `body` carries.
pub const FALLBACK_BODY_MAX: usize = 1024;

/// `* ` and `text`, cut on a `char` boundary and marked `…` when it would
/// pass [`FALLBACK_BODY_MAX`] bytes.
fn fallback(text: &str) -> String {
    if "* ".len() + text.len() <= FALLBACK_BODY_MAX {
        return format!("* {text}");
    }
    let mut end = FALLBACK_BODY_MAX - "* ".len() - "…".len();
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("* {}…", &text[..end])
}

/// A session room's power levels (F1 as ruling R30 amends it).
///
/// Every session room: the creating agent 100, the other agents 50, people 0;
/// `events_default` and `state_default` 50, so a person writes no state.
/// Session rooms are encrypted, so the server sees every timeline event a
/// person sends as `m.room.encrypted` and cannot tell a decision from free
/// text: `m.room.encrypted` is allowed at 0 in every session room, and the
/// host — which decrypts — is what keeps an observer's free text out of a
/// non-proxy session's turns (AD-380). The per-type rows below stand for a
/// client that sends in clear: decisions, `heard` and surface results
/// everywhere, and in a proxy's own rooms (`main`, `conversation`) also
/// `m.room.message` and the scope.
///
/// Because `m.room.encrypted` is at 0, a person can send *any* agent type
/// encrypted — a status, a scope, a turn, an `m.replace` of the anchor. The
/// server cannot refuse it, so the host checks the sender of every decrypted
/// agent-typed event against these levels (power ≥ 50, or one of the room's
/// agents) before acting on it; the per-type rows bind cleartext senders only.
pub fn power_levels(kind: SessionKind, creator: &UserId, agents: &[OwnedUserId]) -> Value {
    let mut users = Map::new();
    for agent in agents {
        users.insert(agent.to_string(), json!(50));
    }
    users.insert(creator.to_string(), json!(100));
    let mut events = Map::new();
    for event in ["m.room.encrypted", APPROVAL_DECISION, HEARD, SURFACE_RESULT] {
        events.insert(event.to_owned(), json!(0));
    }
    if matches!(kind, SessionKind::Main | SessionKind::Conversation) {
        events.insert("m.room.message".to_owned(), json!(0));
        events.insert(SCOPE.to_owned(), json!(0));
    }
    json!({
        "users": users,
        "users_default": 0,
        "events": events,
        "events_default": 50,
        "state_default": 50,
        "ban": 50,
        "kick": 50,
        "redact": 50,
        "invite": 50,
    })
}

/// A principal's control room's power levels (AD-374): the creating agent
/// 100, every other agent user of the principal 50 — so any of them writes
/// its host's manifest, `dev.keeper.agent.host` at 50 — and people 0. A
/// person writes one state there: each of their devices' presence,
/// `dev.keeper.agent.presence` at 0 (R37). `events_default` and
/// `state_default` are 50.
pub fn control_power_levels(creator: &UserId, agents: &[OwnedUserId]) -> Value {
    let mut users = Map::new();
    for agent in agents {
        users.insert(agent.to_string(), json!(50));
    }
    users.insert(creator.to_string(), json!(100));
    json!({
        "users": users,
        "users_default": 0,
        "events": { HOST: 50, PRESENCE: 0 },
        "events_default": 50,
        "state_default": 50,
        "ban": 50,
        "kick": 50,
        "redact": 50,
        "invite": 50,
    })
}

/// What a control room made before R37 needs so its people can publish
/// their presence.
#[derive(Debug, Clone, PartialEq)]
pub enum PresenceLevels {
    /// `dev.keeper.agent.presence` is already at 0.
    UpToDate,
    /// These levels: the room's, with the presence row added.
    Update(Value),
    /// `me` may not change the room's power levels.
    NoPower,
}

/// Whether the control room whose `m.room.power_levels` content is
/// `levels` needs the presence row, and whether `me` may write it.
pub fn presence_levels(levels: &Value, me: &UserId) -> PresenceLevels {
    if levels["events"][PRESENCE] == json!(0) {
        return PresenceLevels::UpToDate;
    }
    let level = |value: &Value, default: i64| value.as_i64().unwrap_or(default);
    let mine = level(
        &levels["users"][me.as_str()],
        level(&levels["users_default"], 0),
    );
    let needed = level(
        &levels["events"]["m.room.power_levels"],
        level(&levels["state_default"], 50),
    );
    if mine < needed {
        return PresenceLevels::NoPower;
    }
    let mut updated = levels.clone();
    match updated["events"].as_object_mut() {
        Some(events) => {
            events.insert(PRESENCE.to_owned(), json!(0));
        }
        None => updated["events"] = json!({ PRESENCE: 0 }),
    }
    PresenceLevels::Update(updated)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn keys(value: &Value) -> BTreeSet<String> {
        value
            .as_object()
            .expect("an object")
            .keys()
            .cloned()
            .collect()
    }

    fn user(id: &str) -> OwnedUserId {
        OwnedUserId::try_from(id).expect("user id")
    }

    #[test]
    fn a_status_round_trips_with_exactly_its_keys() {
        let status = StatusContent {
            v: CONTENT_VERSION,
            session: "60-sessions/active/2026-10-02-plan".to_owned(),
            kind: SessionKind::Conversation,
            title: "plan".to_owned(),
            agent: user("@nixi:example.org"),
            host: "electra".to_owned(),
            epoch: 0,
            run: RunState::Running,
            detail: Some("2 tool calls".to_owned()),
            waiting: None,
            anchor: None,
        };
        let value = serde_json::to_value(&status).expect("serialise");
        assert_eq!(value["kind"], "conversation");
        assert_eq!(value["run"], "running");
        assert_eq!(
            keys(&value),
            ["agent", "detail", "epoch", "host", "kind", "run", "session", "title", "v"]
                .map(str::to_owned)
                .into()
        );
        assert_eq!(
            serde_json::from_value::<StatusContent>(value.clone()).expect("read back"),
            status
        );
        let mut unknown = value;
        unknown["run"] = json!("sleeping");
        assert!(
            serde_json::from_value::<StatusContent>(unknown).is_err(),
            "a run state outside the five is refused"
        );
    }

    #[test]
    fn a_claim_round_trips_and_refuses_a_foreign_key() {
        let claim = ClaimContent {
            v: CONTENT_VERSION,
            host: "electra".to_owned(),
            device: "ELECTRA1".to_owned(),
            agent: user("@nixi:example.org"),
            epoch: 3,
            acquired_at: "2026-10-02T12:00:00Z".to_owned(),
            renewed_at: "2026-10-02T12:01:00Z".to_owned(),
            expires_at: "2026-10-02T12:04:00Z".to_owned(),
            released: false,
            window: None,
        };
        let mut value = serde_json::to_value(&claim).expect("serialise");
        assert_eq!(
            keys(&value),
            [
                "acquired_at",
                "agent",
                "device",
                "epoch",
                "expires_at",
                "host",
                "released",
                "renewed_at",
                "v"
            ]
            .map(str::to_owned)
            .into()
        );
        assert_eq!(
            serde_json::from_value::<ClaimContent>(value.clone()).expect("read back"),
            claim
        );
        value["title"] = json!("a secret plan");
        assert!(serde_json::from_value::<ClaimContent>(value).is_err());
    }

    #[test]
    fn every_session_room_takes_a_persons_encrypted_event_and_no_state_from_them() {
        let creator = user("@nixi:example.org");
        let other = user("@tola:example.org");
        for kind in SessionKind::ALL {
            let levels = power_levels(kind, &creator, std::slice::from_ref(&other));
            assert_eq!(levels["users"]["@nixi:example.org"], 100);
            assert_eq!(levels["users"]["@tola:example.org"], 50);
            assert_eq!(levels["users_default"], 0);
            assert_eq!(levels["events_default"], 50);
            for allowed in [APPROVAL_DECISION, HEARD, SURFACE_RESULT] {
                assert_eq!(levels["events"][allowed], 0, "{kind}: {allowed}");
            }
            let proxy = matches!(kind, SessionKind::Main | SessionKind::Conversation);
            assert_eq!(
                levels["events"].get("m.room.message") == Some(&json!(0)),
                proxy,
                "{kind}: m.room.message"
            );
            // R30: the server sees a person's every event as encrypted.
            assert_eq!(levels["events"]["m.room.encrypted"], 0, "{kind}");
            assert_eq!(levels["state_default"], 50, "{kind}");
            assert_eq!(
                levels["events"].get(SCOPE) == Some(&json!(0)),
                proxy,
                "{kind}: scope"
            );
        }
    }

    #[test]
    fn the_fallback_body_is_at_most_1_kib_and_the_new_content_is_whole() {
        let target = OwnedEventId::try_from("$anchor:example.org").expect("event id");
        let text = "ż".repeat(3000);
        let edit = edit_content(&target, &text);
        let body = edit["body"].as_str().expect("body");
        assert!(body.len() <= FALLBACK_BODY_MAX, "{}", body.len());
        assert!(body.starts_with("* ż"));
        assert_eq!(edit["m.new_content"]["body"], text.as_str());
        assert_eq!(edit["m.relates_to"]["rel_type"], "m.replace");
        assert_eq!(edit["m.relates_to"]["event_id"], "$anchor:example.org");

        let short = edit_content(&target, "hi");
        assert_eq!(short["body"], "* hi");
    }

    #[test]
    fn a_control_room_lets_every_agent_of_the_principal_write_its_manifest() {
        let creator = user("@nixi:example.org");
        let amelia = user("@amelia:example.org");
        let levels = control_power_levels(&creator, std::slice::from_ref(&amelia));
        assert_eq!(levels["users"]["@nixi:example.org"], 100);
        assert_eq!(levels["users"]["@amelia:example.org"], 50);
        assert_eq!(levels["users_default"], 0);
        assert_eq!(levels["events"][HOST], 50);
        assert_eq!(levels["state_default"], 50);
        // Each agent reaches the manifest's level; a person (users_default)
        // reaches no state but their devices' presence (R37).
        let level = |id: &str| {
            levels["users"]
                .get(id)
                .cloned()
                .unwrap_or(levels["users_default"].clone())
        };
        assert!(level("@amelia:example.org").as_i64() >= levels["events"][HOST].as_i64());
        assert!(level("@tgorka:example.org").as_i64() < levels["state_default"].as_i64());
        assert!(level("@tgorka:example.org").as_i64() < levels["events"][HOST].as_i64());
        assert_eq!(levels["events"][PRESENCE], 0);
        assert!(level("@tgorka:example.org").as_i64() >= levels["events"][PRESENCE].as_i64());
    }

    #[test]
    fn a_control_room_made_before_presence_is_brought_up_to_date_by_its_creator() {
        let creator = user("@nixi:example.org");
        let amelia = user("@amelia:example.org");
        let mut old = control_power_levels(&creator, std::slice::from_ref(&amelia));
        old["events"]
            .as_object_mut()
            .expect("events")
            .remove(PRESENCE);
        let PresenceLevels::Update(updated) = presence_levels(&old, &creator) else {
            panic!("the creator updates the room");
        };
        assert_eq!(updated["events"][PRESENCE], 0);
        // Everything else is the room's own.
        let mut back = updated.clone();
        back["events"]
            .as_object_mut()
            .expect("events")
            .remove(PRESENCE);
        assert_eq!(back, old);
        // An agent at 50 may not change the power levels (state_default 50
        // is reached, but a room naming the levels' own row at 100 is not).
        old["events"]["m.room.power_levels"] = json!(100);
        assert_eq!(presence_levels(&old, &amelia), PresenceLevels::NoPower);
        assert!(matches!(
            presence_levels(&old, &creator),
            PresenceLevels::Update(_)
        ));
        assert_eq!(presence_levels(&updated, &amelia), PresenceLevels::UpToDate);
    }

    #[test]
    fn surface_events_round_trip_with_exactly_their_keys() {
        let request = SurfaceRequestContent {
            v: CONTENT_VERSION,
            id: "01J".to_owned(),
            device: "KALYPSO".to_owned(),
            tool: SurfaceTool::ProposeEdit,
            args: SurfaceArgs {
                drive: "tgdrive".to_owned(),
                path: "notes/plan.md".to_owned(),
                heading: None,
                range: Some(LineSpan { from: 3, to: 5 }),
                text: Some("new".to_owned()),
                expected: Some("old".to_owned()),
            },
            expires_at: "2026-10-03T12:01:00.000Z".to_owned(),
        };
        let value = serde_json::to_value(&request).expect("serialise");
        assert_eq!(value["tool"], "propose_edit");
        assert_eq!(
            keys(&value["args"]),
            ["drive", "expected", "path", "range", "text"]
                .map(str::to_owned)
                .into()
        );
        assert_eq!(
            serde_json::from_value::<SurfaceRequestContent>(value).expect("read back"),
            request
        );
        let result = json!({"v": 1, "request": "01J", "device": "KALYPSO", "outcome": "done", "applied": true});
        let read: SurfaceResultContent = serde_json::from_value(result).expect("a result");
        assert_eq!(read.outcome, SurfaceOutcome::Done);
        assert!(serde_json::from_value::<SurfaceResultContent>(
            json!({"v": 1, "request": "01J", "device": "K", "outcome": "maybe"})
        )
        .is_err());
        for tool in SurfaceTool::ALL {
            assert_eq!(SurfaceTool::from_wire(tool.wire()), Some(tool));
        }
        assert_eq!(SurfaceTool::from_wire("drive_read"), None);
    }
}
