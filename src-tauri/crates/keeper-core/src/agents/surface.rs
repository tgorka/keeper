//! A surface call on the person's device (AD-383): which requests this
//! device acts on, and what it hands the notes view.
//!
//! An agent's host sends `dev.keeper.agent.surface.request` into the
//! session room naming one device. Only that device acts, only in one of
//! the person's own proxy conversations and only on a request from that
//! room's proxy ([`crate::agents::proxy::room_proxy`]), only over a drive
//! the proxy declares where this device knows them, once per event — a sync
//! can deliver one twice — and
//! only within [`SURFACE_WAIT`] of the server receiving it; one already past
//! that is answered `expired` and not shown ([`SurfaceInbox`]). The notes
//! view executes it ([`SurfaceRequestVm`]) and answers through
//! `agent_surface_result` ([`SurfaceAnswerReq`]), which this device sends only
//! for a request it was handed.

use std::collections::HashMap;

use chrono::DateTime;
use matrix_sdk::ruma::{OwnedRoomId, RoomId, UserId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::agents::events::{
    LineSpan, SurfaceOutcome, SurfaceRequestContent, SurfaceTool, CONTENT_VERSION, SURFACE_WAIT,
};
use crate::agents::proxy::RoomProxy;
use crate::panels::PanelTargetVm;

/// A request from this account's own user: a person never asks.
pub const OWN_REQUEST: &str = "a surface request from this account's own user";
/// A request from someone below an agent's power in the room.
pub const NOT_AN_AGENT: &str = "a surface request from someone who is not an agent of the room";
/// A request in a room that is not one of this account's proxy
/// conversations, or from an agent that is not that room's proxy.
pub const NOT_MY_PROXY: &str =
    "a surface request from an agent that is not this person's proxy in its own conversation";
/// A content this version does not read.
pub const UNREADABLE: &str = "a surface request this version cannot read";
/// A request naming another device.
pub const ANOTHER_DEVICE: &str = "a surface request for another device";
/// A request this device already acted on or answered.
pub const HANDLED: &str = "a surface request already handled";
/// What the room is told whenever this device will not show what a request
/// names: one sentence for every reason, so an answer never says which
/// drives, folders or paths this device keeps. The reason is logged.
pub const CANNOT_SHOW: &str = "This device cannot show that.";

/// [`SURFACE_WAIT`] in ms.
const WAIT_MS: u64 = SURFACE_WAIT.as_secs() * 1_000;
/// How long past its deadline a request is remembered: a replay is not
/// answered twice, and the notes view's own `expired` is still sent.
const KEPT_MS: u64 = 60_000;

/// What this device does with one surface request event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    /// Hand it to the notes view, which shows it until `deadline_ms` (this
    /// device's clock).
    Forward {
        request: SurfaceRequestContent,
        deadline_ms: u64,
    },
    /// Answer `expired` without showing it.
    Expired(SurfaceRequestContent),
    /// Answer `unavailable` ([`CANNOT_SHOW`]) without showing it: it names a
    /// drive this device knows the room's proxy not to have.
    Unavailable(SurfaceRequestContent),
    /// Do nothing; why, for the log.
    Ignored(&'static str),
}

/// One surface request event as the account's handler read it.
#[derive(Debug, Clone, Copy)]
pub struct RequestEvent<'a> {
    pub event_id: &'a str,
    pub room: &'a RoomId,
    pub content: &'a Value,
    pub sender: &'a UserId,
    /// The sender is this account's own user.
    pub from_own_user: bool,
    /// The sender holds an agent's power (≥ 50) in the room.
    pub from_agent: bool,
    /// The room's proxy, when the room is one of this account's proxy
    /// conversations.
    pub proxy: Option<&'a RoomProxy>,
    /// This client's Matrix device id.
    pub this_device: &'a str,
    /// `origin_server_ts`: when the server received it, the server's clock.
    pub sent_ms: u64,
    /// `unsigned.age`: how long ago the server received it, as the server
    /// delivered it.
    pub age_ms: Option<u64>,
    pub now_ms: u64,
}

impl RequestEvent<'_> {
    /// When this device stops showing the request, on its own clock: the
    /// host's wait, counted from when the server received it. The age the
    /// server reports needs no clock of this device's; without it,
    /// `origin_server_ts` is the server's clock read against this one.
    fn deadline_ms(&self) -> u64 {
        let age = self
            .age_ms
            .unwrap_or_else(|| self.now_ms.saturating_sub(self.sent_ms));
        self.now_ms.saturating_add(WAIT_MS.saturating_sub(age))
    }
}

/// A request handed to the notes view and not answered yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waiting {
    room: OwnedRoomId,
    /// Forgotten after this, ms on this device's clock.
    until_ms: u64,
}

/// The surface requests one account's device has seen: the events handled,
/// until they could no longer be live, and the requests handed to the notes
/// view and not yet answered, until they could no longer be.
#[derive(Debug, Default)]
pub struct SurfaceInbox {
    /// Event id → forgotten after this, ms.
    handled: HashMap<String, u64>,
    /// Request id → where it came from.
    waiting: HashMap<String, Waiting>,
}

impl SurfaceInbox {
    /// Decide one request event.
    pub fn admit(&mut self, event: RequestEvent<'_>) -> Admission {
        if event.from_own_user {
            return Admission::Ignored(OWN_REQUEST);
        }
        if !event.from_agent {
            return Admission::Ignored(NOT_AN_AGENT);
        }
        let Some(proxy) = event.proxy.filter(|proxy| proxy.agent == event.sender) else {
            return Admission::Ignored(NOT_MY_PROXY);
        };
        let Ok(request) = serde_json::from_value::<SurfaceRequestContent>(event.content.clone())
        else {
            return Admission::Ignored(UNREADABLE);
        };
        // The host's statement of when it stops waiting: read, so a content
        // this version cannot read is refused, but never compared with this
        // device's clock (see `deadline_ms`).
        if ms_of(&request.expires_at).is_none() || request.v != CONTENT_VERSION {
            return Admission::Ignored(UNREADABLE);
        }
        if request.device != event.this_device {
            return Admission::Ignored(ANOTHER_DEVICE);
        }
        let now = event.now_ms;
        self.handled.retain(|_, until| *until > now);
        self.waiting.retain(|_, waiting| waiting.until_ms > now);
        if self.handled.contains_key(event.event_id) {
            return Admission::Ignored(HANDLED);
        }
        let deadline_ms = event.deadline_ms();
        self.handled
            .insert(event.event_id.to_owned(), deadline_ms.max(now) + KEPT_MS);
        if deadline_ms <= now {
            return Admission::Expired(request);
        }
        if proxy
            .drives
            .as_ref()
            .is_some_and(|drives| !drives.contains(&request.args.drive))
        {
            return Admission::Unavailable(request);
        }
        self.waiting.insert(
            request.id.clone(),
            Waiting {
                room: event.room.to_owned(),
                until_ms: deadline_ms + KEPT_MS,
            },
        );
        Admission::Forward {
            request,
            deadline_ms,
        }
    }

    /// The notes view answers `request` in `room`: take it, when this device
    /// was handed it and has not answered it yet. A second answer meanwhile
    /// finds nothing; [`SurfaceInbox::restore`] puts it back.
    pub fn take(&mut self, room: &RoomId, request: &str) -> Option<Waiting> {
        match self.waiting.get(request) {
            Some(waiting) if waiting.room == room => self.waiting.remove(request),
            _ => None,
        }
    }

    /// The answer taken for `request` was not sent: it waits again.
    pub fn restore(&mut self, request: String, waiting: Waiting) {
        self.waiting.insert(request, waiting);
    }
}

/// An RFC 3339 time as ms since the epoch.
pub fn ms_of(at: &str) -> Option<u64> {
    DateTime::parse_from_rfc3339(at)
        .ok()
        .and_then(|at| u64::try_from(at.timestamp_millis()).ok())
}

/// A request the device admitted, as the account's handler hands it on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceRequestArrived {
    pub account_id: String,
    pub room_id: OwnedRoomId,
    pub request: SurfaceRequestContent,
    /// When this device stops showing it, ms on this device's clock.
    pub deadline_ms: u64,
    /// The drives the room's proxy declares, where this device knows them:
    /// the only ones the shell names a target within.
    pub drives: Option<Vec<String>>,
}

/// A surface call for the notes view to execute (UX-DR131).
///
/// `target` is the note or file the request names, resolved on this device
/// by Rust (AD-65: the webview joins no paths). `range` counts the editor
/// buffer's lines (the body, without frontmatter), 1-based and inclusive:
/// for `open` and `scroll` it is the heading's section when one was found,
/// absent otherwise (open at the top); for `highlight`, `point` and
/// `propose_edit` it is always present. `propose_edit` carries `text`, the
/// lines that replace the range, and `expected`, the range's text as the
/// agent read it (lines joined by `\n`): the proposal applies only while the
/// buffer's lines `range` are exactly `expected`, else it is answered
/// `unavailable`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SurfaceRequestVm {
    pub account_id: String,
    pub room_id: String,
    pub request_id: String,
    pub tool: SurfaceTool,
    pub target: PanelTargetVm,
    pub heading: Option<String>,
    pub range: Option<LineSpan>,
    pub text: Option<String>,
    pub expected: Option<String>,
    /// When the host stops waiting, ms since the epoch on this device's
    /// clock (counted from when the server received the request): an answer
    /// after it is not read.
    #[ts(type = "number")]
    pub expires_at_ms: u64,
}

impl SurfaceRequestVm {
    /// The view model of `arrived`, which names `target` on this device.
    pub fn new(arrived: &SurfaceRequestArrived, target: PanelTargetVm) -> SurfaceRequestVm {
        let request = &arrived.request;
        SurfaceRequestVm {
            account_id: arrived.account_id.clone(),
            room_id: arrived.room_id.to_string(),
            request_id: request.id.clone(),
            tool: request.tool,
            target,
            heading: request.args.heading.clone(),
            range: request.args.range,
            text: request.args.text.clone(),
            expected: request.args.expected.clone(),
            expires_at_ms: arrived.deadline_ms,
        }
    }
}

/// The notes view's answer to a [`SurfaceRequestVm`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SurfaceAnswerReq {
    pub request_id: String,
    pub outcome: SurfaceOutcome,
    /// `propose_edit`: whether the person applied it.
    pub applied: Option<bool>,
    /// A short sentence the agent is told with the outcome; never note text.
    pub detail: Option<String>,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use matrix_sdk::ruma::OwnedUserId;
    use serde_json::json;

    use super::*;
    use crate::agents::claim::rfc3339;
    use crate::agents::proxy::{room_proxy, ProxyFacts, ProxyRoomRow};
    use crate::agents::session::SessionKind;

    const NOW: u64 = 1_790_000_000_000;

    fn room() -> OwnedRoomId {
        OwnedRoomId::try_from("!dm:example.org").expect("room")
    }

    fn user(id: &str) -> OwnedUserId {
        OwnedUserId::try_from(id).expect("user")
    }

    /// Nixi, tgorka's proxy, over tgdrive.
    fn nixi() -> RoomProxy {
        RoomProxy {
            agent: user("@nixi:example.org"),
            drives: Some(vec!["tgdrive".to_owned()]),
        }
    }

    fn request(device: &str) -> Value {
        request_over(device, "tgdrive")
    }

    fn request_over(device: &str, drive: &str) -> Value {
        json!({
            "v": 1,
            "id": "01JREQ",
            "device": device,
            "tool": "highlight",
            "args": {"drive": drive, "path": "notes/plan.md", "range": {"from": 2, "to": 3}},
            "expires_at": rfc3339(NOW + 60_000),
        })
    }

    /// `content` from `proxy`'s agent in its own room, just received.
    fn event<'a>(
        id: &'a str,
        room: &'a RoomId,
        content: &'a Value,
        proxy: &'a RoomProxy,
        now_ms: u64,
    ) -> RequestEvent<'a> {
        RequestEvent {
            event_id: id,
            room,
            content,
            sender: &proxy.agent,
            from_own_user: false,
            from_agent: true,
            proxy: Some(proxy),
            this_device: "KALYPSO",
            sent_ms: now_ms,
            age_ms: Some(0),
            now_ms,
        }
    }

    #[test]
    fn a_surface_request_for_another_device_is_ignored() {
        let room = room();
        let nixi = nixi();
        let mut inbox = SurfaceInbox::default();
        let live = request("KALYPSO");

        // Naming this device, from the room's proxy: handed on.
        let Admission::Forward {
            request: forwarded, ..
        } = inbox.admit(event("$a", &room, &live, &nixi, NOW))
        else {
            panic!("forwarded");
        };
        assert_eq!(forwarded.args.range, Some(LineSpan { from: 2, to: 3 }));

        // A sync delivering the same event again: ignored.
        assert_eq!(
            inbox.admit(event("$a", &room, &live, &nixi, NOW + 1_000)),
            Admission::Ignored(HANDLED)
        );

        // Naming another device: ignored.
        let other = request("HESPERIA");
        assert_eq!(
            inbox.admit(event("$b", &room, &other, &nixi, NOW)),
            Admission::Ignored(ANOTHER_DEVICE)
        );

        // From a person (power 0), or from this account's own user: ignored.
        let person = RequestEvent {
            from_agent: false,
            ..event("$c", &room, &live, &nixi, NOW)
        };
        assert_eq!(inbox.admit(person), Admission::Ignored(NOT_AN_AGENT));
        let own = RequestEvent {
            from_own_user: true,
            ..event("$d", &room, &live, &nixi, NOW)
        };
        assert_eq!(inbox.admit(own), Admission::Ignored(OWN_REQUEST));

        // The server received it the whole wait ago: answered `expired`,
        // once.
        let late = RequestEvent {
            age_ms: Some(60_000),
            ..event("$e", &room, &live, &nixi, NOW)
        };
        assert!(matches!(inbox.admit(late), Admission::Expired(_)));
        assert_eq!(
            inbox.admit(RequestEvent {
                age_ms: Some(65_000),
                ..event("$e", &room, &live, &nixi, NOW + 5_000)
            }),
            Admission::Ignored(HANDLED)
        );

        // Unreadable: another version, or a key the schema lacks.
        let mut v2 = live.clone();
        v2["v"] = json!(2);
        assert_eq!(
            inbox.admit(event("$f", &room, &v2, &nixi, NOW)),
            Admission::Ignored(UNREADABLE)
        );
        let mut extra = live;
        extra["note_text"] = json!("x");
        assert_eq!(
            inbox.admit(event("$g", &room, &extra, &nixi, NOW)),
            Admission::Ignored(UNREADABLE)
        );
    }

    /// Session rooms give every agent of their principal power 50, and the
    /// person may be in session rooms another principal's agent made (Dr
    /// Lucyna Novak, audience {tgorka, Marta}). Only the person's own proxy,
    /// in its own conversation, over the drives it declares, is acted on.
    #[test]
    fn a_surface_request_from_another_principals_agent_is_ignored() {
        let me = user("@tgorka:example.org");
        let marta = user("@marta:example.org");
        let lucyna = user("@lucyna:example.org");
        let room = room();
        let mut inbox = SurfaceInbox::default();
        let live = request("KALYPSO");
        let none = BTreeMap::new();
        let nixis_dm = ProxyRoomRow {
            room_id: room.to_string(),
            name: "Nixi".to_owned(),
            kind: Some(SessionKind::Main),
            agent: Some(nixi().agent),
            title: None,
            recency: 1,
            creators: vec![nixi().agent],
            encrypted: true,
            direct_to: vec![nixi().agent],
            scope: Some(vec!["tgdrive".to_owned()]),
        };
        let nixi = room_proxy(&nixis_dm, &me, &none).expect("Nixi's DM is a proxy room");

        // Another agent at 50 in Nixi's own DM.
        let from_lucyna = RequestEvent {
            sender: &lucyna,
            ..event("$a", &room, &live, &nixi, NOW)
        };
        assert_eq!(inbox.admit(from_lucyna), Admission::Ignored(NOT_MY_PROXY));

        // Lucyna's own rooms with tgorka: her status calling one `main` is
        // not tgorka's DM; where her zone is on the device, a conversation
        // is Marta's proxy's. Neither is a room of tgorka's proxy.
        let lucynas = ProxyRoomRow {
            agent: Some(lucyna.clone()),
            creators: vec![lucyna.clone()],
            direct_to: Vec::new(),
            ..nixis_dm.clone()
        };
        let conversation = ProxyRoomRow {
            kind: Some(SessionKind::Conversation),
            ..lucynas.clone()
        };
        let zone = BTreeMap::from([(
            lucyna.clone(),
            ProxyFacts {
                human: marta,
                allowed: Vec::new(),
            },
        )]);
        for refused in [
            room_proxy(&lucynas, &me, &none),
            room_proxy(&conversation, &me, &zone),
        ] {
            assert_eq!(refused, None);
            let in_hers = RequestEvent {
                sender: &lucyna,
                proxy: refused.as_ref(),
                ..event("$b", &room, &live, &nixi, NOW)
            };
            assert_eq!(inbox.admit(in_hers), Admission::Ignored(NOT_MY_PROXY));
        }

        // Nixi naming a drive it does not declare: answered `unavailable`,
        // never shown, though this device may sync that folder.
        let diary = request_over("KALYPSO", "marta-diary");
        assert!(matches!(
            inbox.admit(event("$c", &room, &diary, &nixi, NOW)),
            Admission::Unavailable(_)
        ));
        assert!(
            inbox.take(&room, "01JREQ").is_none(),
            "nothing handed on to answer"
        );

        // Nixi, in its DM, over its drive: handed on.
        assert!(matches!(
            inbox.admit(event("$d", &room, &live, &nixi, NOW)),
            Admission::Forward { .. }
        ));

        // A phone before Nixi's host echoed a scope knows none of its
        // drives, so it cannot refuse one: Nixi's own request is handed on.
        let unechoed = ProxyRoomRow {
            scope: None,
            ..nixis_dm
        };
        let unknown = room_proxy(&unechoed, &me, &none).expect("still Nixi's DM");
        assert_eq!(unknown.drives, None);
        assert!(matches!(
            inbox.admit(event("$e", &room, &diary, &unknown, NOW)),
            Admission::Forward { .. }
        ));
    }

    /// `expires_at` is the host's clock; a phone a minute ahead of it would
    /// answer every request `expired` on arrival. The device counts the
    /// wait from when the server received the request.
    #[test]
    fn the_wait_is_counted_from_the_server_not_the_hosts_clock() {
        let room = room();
        let nixi = nixi();
        let mut inbox = SurfaceInbox::default();
        let live = request("KALYPSO");
        // This device's clock is 90 s past the host's `expires_at`, but the
        // server received the request half a second ago.
        let ahead = NOW + 150_000;
        let fresh = RequestEvent {
            age_ms: Some(500),
            ..event("$a", &room, &live, &nixi, ahead)
        };
        assert_eq!(
            inbox.admit(fresh),
            Admission::Forward {
                request: serde_json::from_value(live.clone()).expect("request"),
                deadline_ms: ahead + 59_500,
            }
        );
        // Without an age, `origin_server_ts` read against this clock.
        let no_age = RequestEvent {
            age_ms: None,
            sent_ms: ahead - 2_000,
            ..event("$b", &room, &live, &nixi, ahead)
        };
        assert!(matches!(
            inbox.admit(no_age),
            Admission::Forward { deadline_ms, .. } if deadline_ms == ahead + 58_000
        ));
        // However far out the host puts `expires_at`, the wait is the wait.
        let mut far = live;
        far["expires_at"] = json!(rfc3339(NOW + 365 * 86_400_000));
        assert!(matches!(
            inbox.admit(event("$c", &room, &far, &nixi, NOW)),
            Admission::Forward { deadline_ms, .. } if deadline_ms == NOW + 60_000
        ));
    }

    #[test]
    fn only_a_request_handed_on_is_answered_and_only_once() {
        let room = room();
        let nixi = nixi();
        let elsewhere = OwnedRoomId::try_from("!other:example.org").expect("room");
        let mut inbox = SurfaceInbox::default();
        let live = request("KALYPSO");
        assert!(
            inbox.take(&room, "01JREQ").is_none(),
            "nothing handed on yet"
        );
        assert!(matches!(
            inbox.admit(event("$a", &room, &live, &nixi, NOW)),
            Admission::Forward { .. }
        ));
        assert!(
            inbox.take(&elsewhere, "01JREQ").is_none(),
            "not from that room"
        );
        let taken = inbox.take(&room, "01JREQ").expect("handed on");
        assert!(inbox.take(&room, "01JREQ").is_none(), "answered once");
        // The answer could not be sent: it can be given again, once.
        inbox.restore("01JREQ".to_owned(), taken);
        assert!(inbox.take(&room, "01JREQ").is_some());
        assert!(inbox.take(&room, "01JREQ").is_none());
    }

    /// A request nobody answers — no notes view yet, a panel never opened —
    /// is not kept for the account's life.
    #[test]
    fn an_unanswered_request_is_forgotten_once_it_could_no_longer_be_answered() {
        let room = room();
        let nixi = nixi();
        let mut inbox = SurfaceInbox::default();
        let live = request("KALYPSO");
        let mut next = request("KALYPSO");
        next["id"] = json!("01JNEXT");
        assert!(matches!(
            inbox.admit(event("$a", &room, &live, &nixi, NOW)),
            Admission::Forward { .. }
        ));
        // Within a minute past its deadline the notes view may still answer.
        assert!(matches!(
            inbox.admit(event("$b", &room, &next, &nixi, NOW + 119_000)),
            Admission::Forward { .. }
        ));
        let taken = inbox.take(&room, "01JREQ").expect("still waiting");
        inbox.restore("01JREQ".to_owned(), taken);
        // Past that, the next admission forgets it.
        assert!(matches!(
            inbox.admit(event("$c", &room, &next, &nixi, NOW + 120_001)),
            Admission::Forward { .. }
        ));
        assert!(inbox.take(&room, "01JREQ").is_none());
        assert!(inbox.take(&room, "01JNEXT").is_some());
    }
}
