//! The agents' Matrix events: room types, event types, the contents 90.4–90.6
//! send, and a session room's power levels (AD-370, AD-372; *Matrix events*).
//!
//! Every content is a closed struct: it serialises exactly its schema's keys,
//! and reading one refuses a key it does not know, so a state event — which is
//! not encrypted — can never be made to carry a title, a path or text by a
//! field nobody meant to add (the architecture's Ambiguity 7).

use matrix_sdk::ruma::{OwnedEventId, OwnedUserId, UserId};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

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

/// The contents' schema version, `"v": 1`.
pub const CONTENT_VERSION: u32 = 1;

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
}
