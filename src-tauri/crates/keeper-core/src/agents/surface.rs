//! A surface call on the person's device (AD-383): which requests this
//! device acts on, and what it hands the notes view.
//!
//! An agent's host sends `dev.keeper.agent.surface.request` into the
//! session room naming one device. Only that device acts, only on a request
//! from an agent of the room (power ≥ 50, never the person's own user), once
//! per event — a sync can deliver one twice — and only before it expires;
//! one already expired is answered `expired` and not shown ([`SurfaceInbox`]).
//! The notes view executes it ([`SurfaceRequestVm`]) and answers through
//! `agent_surface_result` ([`SurfaceAnswerReq`]), which this device sends only
//! for a request it was handed.

use std::collections::HashMap;

use chrono::DateTime;
use matrix_sdk::ruma::{OwnedRoomId, RoomId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::agents::events::{
    LineSpan, SurfaceOutcome, SurfaceRequestContent, SurfaceTool, CONTENT_VERSION,
};
use crate::panels::PanelTargetVm;

/// A request from this account's own user: a person never asks.
pub const OWN_REQUEST: &str = "a surface request from this account's own user";
/// A request from someone below an agent's power in the room.
pub const NOT_AN_AGENT: &str = "a surface request from someone who is not an agent of the room";
/// A content this version does not read.
pub const UNREADABLE: &str = "a surface request this version cannot read";
/// A request naming another device.
pub const ANOTHER_DEVICE: &str = "a surface request for another device";
/// A request this device already acted on or answered.
pub const HANDLED: &str = "a surface request already handled";

/// What this device does with one surface request event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    /// Hand it to the notes view.
    Forward(SurfaceRequestContent),
    /// Answer `expired` without showing it.
    Expired(SurfaceRequestContent),
    /// Do nothing; why, for the log.
    Ignored(&'static str),
}

/// One surface request event as the account's handler read it.
#[derive(Debug, Clone, Copy)]
pub struct RequestEvent<'a> {
    pub event_id: &'a str,
    pub room: &'a RoomId,
    pub content: &'a Value,
    /// The sender is this account's own user.
    pub from_own_user: bool,
    /// The sender holds an agent's power (≥ 50) in the room.
    pub from_agent: bool,
    /// This client's Matrix device id.
    pub this_device: &'a str,
    pub now_ms: u64,
}

/// The surface requests one account's device has seen: the events handled,
/// until they could no longer be live, and the requests handed to the notes
/// view and not yet answered.
#[derive(Debug, Default)]
pub struct SurfaceInbox {
    /// Event id → when the request expires, ms.
    handled: HashMap<String, u64>,
    /// Request id → the room it came from.
    waiting: HashMap<String, OwnedRoomId>,
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
        let Ok(request) = serde_json::from_value::<SurfaceRequestContent>(event.content.clone())
        else {
            return Admission::Ignored(UNREADABLE);
        };
        let Some(expires) = ms_of(&request.expires_at) else {
            return Admission::Ignored(UNREADABLE);
        };
        if request.v != CONTENT_VERSION {
            return Admission::Ignored(UNREADABLE);
        }
        if request.device != event.this_device {
            return Admission::Ignored(ANOTHER_DEVICE);
        }
        self.handled.retain(|_, until| *until > event.now_ms);
        if self.handled.contains_key(event.event_id) {
            return Admission::Ignored(HANDLED);
        }
        // Kept a minute past its expiry, so a replay of a request that
        // expired is not answered twice.
        self.handled.insert(
            event.event_id.to_owned(),
            expires.max(event.now_ms) + 60_000,
        );
        if expires <= event.now_ms {
            return Admission::Expired(request);
        }
        self.waiting
            .insert(request.id.clone(), event.room.to_owned());
        Admission::Forward(request)
    }

    /// The notes view answers `request` in `room`: whether this device was
    /// handed it and has not answered it yet. Answering takes it.
    pub fn answer(&mut self, room: &RoomId, request: &str) -> bool {
        match self.waiting.get(request) {
            Some(from) if from == room => {
                self.waiting.remove(request);
                true
            }
            _ => false,
        }
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
    /// When the host stops waiting, ms since the epoch: an answer after it
    /// is not read.
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
            expires_at_ms: ms_of(&request.expires_at).unwrap_or(0),
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
    use serde_json::json;

    use super::*;
    use crate::agents::claim::rfc3339;

    const NOW: u64 = 1_790_000_000_000;

    fn room() -> OwnedRoomId {
        OwnedRoomId::try_from("!dm:example.org").expect("room")
    }

    fn request(device: &str, expires_ms: u64) -> Value {
        json!({
            "v": 1,
            "id": "01JREQ",
            "device": device,
            "tool": "highlight",
            "args": {"drive": "tgdrive", "path": "notes/plan.md", "range": {"from": 2, "to": 3}},
            "expires_at": rfc3339(expires_ms),
        })
    }

    fn event<'a>(
        id: &'a str,
        room: &'a RoomId,
        content: &'a Value,
        now_ms: u64,
    ) -> RequestEvent<'a> {
        RequestEvent {
            event_id: id,
            room,
            content,
            from_own_user: false,
            from_agent: true,
            this_device: "KALYPSO",
            now_ms,
        }
    }

    #[test]
    fn a_surface_request_for_another_device_is_ignored() {
        let room = room();
        let mut inbox = SurfaceInbox::default();
        let live = request("KALYPSO", NOW + 60_000);

        // Naming this device, from an agent of the room: handed on.
        let Admission::Forward(forwarded) = inbox.admit(event("$a", &room, &live, NOW)) else {
            panic!("forwarded");
        };
        assert_eq!(forwarded.args.range, Some(LineSpan { from: 2, to: 3 }));

        // A sync delivering the same event again: ignored.
        assert_eq!(
            inbox.admit(event("$a", &room, &live, NOW + 1_000)),
            Admission::Ignored(HANDLED)
        );

        // Naming another device: ignored.
        let other = request("HESPERIA", NOW + 60_000);
        assert_eq!(
            inbox.admit(event("$b", &room, &other, NOW)),
            Admission::Ignored(ANOTHER_DEVICE)
        );

        // From a person (power 0), or from this account's own user: ignored.
        let person = RequestEvent {
            from_agent: false,
            ..event("$c", &room, &live, NOW)
        };
        assert_eq!(inbox.admit(person), Admission::Ignored(NOT_AN_AGENT));
        let own = RequestEvent {
            from_own_user: true,
            ..event("$d", &room, &live, NOW)
        };
        assert_eq!(inbox.admit(own), Admission::Ignored(OWN_REQUEST));

        // Past `expires_at`: answered `expired`, once.
        let late = request("KALYPSO", NOW - 1);
        assert!(matches!(
            inbox.admit(event("$e", &room, &late, NOW)),
            Admission::Expired(_)
        ));
        assert_eq!(
            inbox.admit(event("$e", &room, &late, NOW + 5_000)),
            Admission::Ignored(HANDLED)
        );

        // Unreadable: another version, or a key the schema lacks.
        let mut v2 = live.clone();
        v2["v"] = json!(2);
        assert_eq!(
            inbox.admit(event("$f", &room, &v2, NOW)),
            Admission::Ignored(UNREADABLE)
        );
        let mut extra = live;
        extra["note_text"] = json!("x");
        assert_eq!(
            inbox.admit(event("$g", &room, &extra, NOW)),
            Admission::Ignored(UNREADABLE)
        );
    }

    #[test]
    fn only_a_request_handed_on_is_answered_and_only_once() {
        let room = room();
        let elsewhere = OwnedRoomId::try_from("!other:example.org").expect("room");
        let mut inbox = SurfaceInbox::default();
        let live = request("KALYPSO", NOW + 60_000);
        assert!(!inbox.answer(&room, "01JREQ"), "nothing handed on yet");
        assert!(matches!(
            inbox.admit(event("$a", &room, &live, NOW)),
            Admission::Forward(_)
        ));
        assert!(!inbox.answer(&elsewhere, "01JREQ"), "not from that room");
        assert!(inbox.answer(&room, "01JREQ"));
        assert!(!inbox.answer(&room, "01JREQ"), "answered once");
    }
}
