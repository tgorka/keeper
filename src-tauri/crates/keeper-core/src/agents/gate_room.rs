//! A gate's room as a person's keeper draws it (UX-DR147, UX-DR160; story
//! 99.1).
//!
//! The outside system posts its requests at power 0; the gate — the room's
//! only member at an agent's power (codemap Q3) — says one message per step
//! about each of them, `answer`, `ticket`, `result` or `refused`, carrying
//! [`events::GATE`] and an `m.reference` to the request's event
//! ([`crate::agents::gate::content`]). A device takes a step only from a
//! sender at an agent's power who is not the own user, as it takes an
//! answer ([`super::room::is_trusted_turn`]): the peer's own
//! `dev.keeper.agent.gate` (its request's name and deadline) never says a
//! state. A message is drawn as a request only once the gate has spoken of
//! it — the event a trusted step references, from a sender below an agent's
//! power who is not the own user, and not edited since (R114's precedent:
//! the gate never reads an edit, so its word does not stand over one) —
//! its chip the newest step the gate said of it; its text is the outside
//! system's, drawn as data, under its sender's name quoted and cut. Before
//! that it is an ordinary message. The outside system's account is the
//! sender of the newest request the gate spoke of: the room holds the peer
//! and the approvers alike at 0, and only the gate's own words tell them
//! apart.
//!
//! The hour's card (UX-DR160) is a coalesced approval request in a gate
//! room: each row names the request it serves, quoted, the ticket it
//! decides and the agent the work would go to, beside the card's own whole
//! payload; the card names the system and the hour it covers, and which
//! rows *Deny all* would deny — the ones still waiting that this device may
//! decide. A decided row keeps its verdict.

use std::collections::HashMap;

use matrix_sdk::ruma::{EventId, OwnedUserId, UserId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;
use ulid::Ulid;

use crate::agents::approval_card::{ApprovalCardVm, ApprovalStateVm};
use crate::agents::events::{self, ApprovalRequestContent};
use crate::agents::gate::{shown_request, CARD_WINDOW_MS, GATE_VERSION};
use crate::agents::log::GateReason;
use crate::agents::tier::AgentTool;

/// Where a request stands, as the gate said it in the room.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum GateStepVm {
    /// Answered within its deadline.
    Answered,
    /// Ticketed: the work goes on, and its result comes later.
    Ticket,
    /// The ticket's work, done.
    Result,
    /// Not taken, or given up: `reason` says why.
    Refused,
}

impl GateStepVm {
    fn of(state: &str) -> Option<GateStepVm> {
        match state {
            "answer" => Some(Self::Answered),
            "ticket" => Some(Self::Ticket),
            "result" => Some(Self::Result),
            "refused" => Some(Self::Refused),
            // `request` and `held` are the log's alone: never said in a room.
            _ => None,
        }
    }
}

/// Why a gate refused a request ([`GateReason`]), as the room says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum GateReasonVm {
    Busy,
    Mode,
    NoAnswer,
    Interrupted,
    Denied,
    Expired,
    Failed,
    Withheld,
    Unreleased,
}

impl From<GateReason> for GateReasonVm {
    fn from(reason: GateReason) -> GateReasonVm {
        match reason {
            GateReason::Busy => Self::Busy,
            GateReason::Mode => Self::Mode,
            GateReason::NoAnswer => Self::NoAnswer,
            GateReason::Interrupted => Self::Interrupted,
            GateReason::Denied => Self::Denied,
            GateReason::Expired => Self::Expired,
            GateReason::Failed => Self::Failed,
            GateReason::Withheld => Self::Withheld,
            GateReason::Unreleased => Self::Unreleased,
        }
    }
}

/// Whose line it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum GateRoleVm {
    /// The outside system's request: its text is data, never keeper's or
    /// the agent's words.
    Request,
    /// The gate's own word on a request.
    Gate,
}

/// A line of a gate room (UX-DR147): the outside system's request, or the
/// gate's word on one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GateLineVm {
    pub role: GateRoleVm,
    /// On the gate's line, what it says; on a request, the newest step the
    /// gate said of it.
    pub state: GateStepVm,
    /// The request's name as the outside system gave it, quoted and cut as
    /// the gate shows it (`gate::shown_request`); `None` for a request named
    /// by its event (an async one).
    pub request: Option<String>,
    /// The ticket: the id of the session that works it (AD-368).
    pub ticket: Option<String>,
    /// Why it was refused.
    pub reason: Option<GateReasonVm>,
    /// A sync request's deadline as the gate clamped it, ms.
    #[ts(type = "number | null")]
    pub deadline_ms: Option<u32>,
    /// On a request, its sender's display name in the room (else its user
    /// id) — chosen by the outside system, so quoted and cut as a request's
    /// name is; `None` on the gate's own line.
    pub sender_name: Option<String>,
}

/// A gate room's header line (UX-DR147): the outside system's name and,
/// once the gate has answered it, its account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GateHeaderVm {
    /// The `system` the gate is the door for, as its status titles the
    /// session.
    pub system: String,
    /// The outside system's Matrix user id; `None` until the gate has
    /// spoken of one of its requests in what the room has loaded.
    pub peer: Option<String>,
    /// Its display name in the room, else its user id — chosen by the
    /// outside system, so quoted and cut as a request's name is.
    pub peer_name: Option<String>,
}

/// One step the gate said: of which request (its name and event), and what.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Step {
    request: String,
    event: String,
    state: GateStepVm,
    ticket: Option<String>,
    reason: Option<GateReasonVm>,
    deadline_ms: Option<u32>,
}

#[derive(Deserialize)]
struct Probe {
    #[serde(rename = "type")]
    kind: String,
    content: Value,
}

/// The step a message's original event `json` says, read as the gate's
/// own: `None` for anything but an `m.room.message` referencing one event
/// whose gate key is this version's, names its request and says a room
/// state.
fn step_of(json: &str) -> Option<Step> {
    if !json.contains(events::GATE) {
        return None;
    }
    let probe: Probe = serde_json::from_str(json).ok()?;
    let content = &probe.content;
    let relation = &content["m.relates_to"];
    if probe.kind != "m.room.message" || relation["rel_type"] != "m.reference" {
        return None;
    }
    let key = &content[events::GATE];
    if key["v"].as_u64() != Some(u64::from(GATE_VERSION)) {
        return None;
    }
    let state = GateStepVm::of(key["state"].as_str()?)?;
    let request = key["request"].as_str().filter(|name| !name.is_empty())?;
    let event = relation["event_id"].as_str()?;
    Some(Step {
        request: request.to_owned(),
        event: event.to_owned(),
        state,
        ticket: key["ticket"]
            .as_str()
            .filter(|ticket| Ulid::from_string(ticket).is_ok())
            .map(str::to_owned),
        reason: serde_json::from_value::<GateReason>(key["reason"].clone())
            .ok()
            .filter(|_| state == GateStepVm::Refused)
            .map(GateReasonVm::from),
        deadline_ms: key["deadline_ms"]
            .as_u64()
            .and_then(|ms| u32::try_from(ms).ok()),
    })
}

/// A request's name as a line shows it: none when it is the event's own id
/// (an async request is named by its event), else quoted and cut.
fn shown_name(request: &str, event: &str) -> Option<String> {
    (request != event).then(|| shown_request(request))
}

/// One loaded message as a gate room reads it.
#[derive(Debug, Clone, Copy)]
pub struct GateInput<'a> {
    /// Its event id; `None` for a local echo.
    pub event: Option<&'a str>,
    pub sender: &'a UserId,
    /// Its original event.
    pub json: Option<&'a str>,
    /// Whether it was edited since.
    pub edited: bool,
    /// Its sender's display name in the room, when read.
    pub name: Option<&'a str>,
}

/// What a gate room's loaded timeline says of each request: the newest
/// step the gate said of it, by the request's event id, and the outside
/// system's account.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GateView {
    steps: HashMap<String, Step>,
    peer: Option<OwnedUserId>,
}

impl GateView {
    /// Read `items`, in the timeline's order, as `own` reads them, where
    /// `agent` says who holds an agent's power: every step from the gate,
    /// the later one about a request standing, and the sender of the
    /// request the newest step spoke of.
    pub fn of<'a>(
        items: impl IntoIterator<Item = GateInput<'a>> + Clone,
        own: &UserId,
        agent: &dyn Fn(&UserId) -> bool,
    ) -> GateView {
        let mut steps: HashMap<String, (usize, Step)> = HashMap::new();
        for (at, input) in items.clone().into_iter().enumerate() {
            if input.sender == own || !agent(input.sender) {
                continue;
            }
            if let Some(step) = input.json.and_then(step_of) {
                steps.insert(step.event.clone(), (at, step));
            }
        }
        let peer = items
            .into_iter()
            .filter_map(|input| {
                let (at, _) = steps.get(input.event?)?;
                (input.sender != own && !agent(input.sender)).then_some((*at, input.sender))
            })
            .max_by_key(|(at, _)| *at)
            .map(|(_, sender)| sender.to_owned());
        GateView {
            steps: steps
                .into_iter()
                .map(|(event, (_, step))| (event, step))
                .collect(),
            peer,
        }
    }

    /// The outside system's account, once the gate has spoken of one of
    /// its requests.
    pub fn peer(&self) -> Option<&UserId> {
        self.peer.as_deref()
    }

    /// How `message`, with its original event, is drawn as `own` reads it:
    /// the gate's word on a request, a request the gate spoke of and that
    /// was not edited since, or `None`.
    pub fn line(
        &self,
        own: &UserId,
        message: GateInput<'_>,
        agent: &dyn Fn(&UserId) -> bool,
    ) -> Option<GateLineVm> {
        if message.sender == own {
            return None;
        }
        if agent(message.sender) {
            let step = message.json.and_then(step_of)?;
            return Some(GateLineVm {
                role: GateRoleVm::Gate,
                state: step.state,
                request: shown_name(&step.request, &step.event),
                ticket: step.ticket,
                reason: step.reason,
                deadline_ms: step.deadline_ms,
                sender_name: None,
            });
        }
        if message.edited {
            return None;
        }
        let step = self.steps.get(message.event?)?;
        Some(GateLineVm {
            role: GateRoleVm::Request,
            state: step.state,
            request: shown_name(&step.request, &step.event),
            ticket: step.ticket.clone(),
            reason: step.reason,
            deadline_ms: step.deadline_ms,
            sender_name: Some(shown_request(
                message.name.unwrap_or(message.sender.as_str()),
            )),
        })
    }
}

/// The hour's card as a gate room draws it (UX-DR160).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GateCardVm {
    /// The `system` the gate is the door for.
    pub system: String,
    /// The hour the card gathers tickets in: from the card's own event, by
    /// the homeserver's clock, ms since the Unix epoch.
    #[ts(type = "number")]
    pub from: i64,
    #[ts(type = "number")]
    pub until: i64,
    /// The rows *Deny all* denies: each still waiting that this device may
    /// decide. A decided row keeps its verdict.
    pub deny_all: Vec<String>,
}

impl GateCardVm {
    /// The card of `system` whose event the homeserver stamped `at` ms,
    /// over its rows `cards`.
    pub fn of(system: &str, at: u64, cards: &[ApprovalCardVm]) -> GateCardVm {
        let from = i64::try_from(at).unwrap_or(i64::MAX);
        GateCardVm {
            system: system.to_owned(),
            from,
            until: from.saturating_add(i64::try_from(CARD_WINDOW_MS).unwrap_or(i64::MAX)),
            deny_all: cards
                .iter()
                .filter(|card| card.can_decide && card.state == ApprovalStateVm::Pending)
                .map(|card| card.id.clone())
                .collect(),
        }
    }
}

/// One row of the hour's card: which request it serves and where its work
/// would go. What it hands on is the card's own payload, whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GateRowVm {
    /// The request the ticket serves, quoted and cut as the gate shows it;
    /// `None` for one named by its event (an async request).
    pub request: Option<String>,
    /// The ticket the row decides, as the room's ticket line names it: the
    /// id of the session that would work it.
    pub ticket: Option<String>,
    /// The agent the work would go to, as the call named it — one of the
    /// gate's delegates, or the host would have blocked it unasked —
    /// quoted as data; `None` for a journal entry, which stays in the
    /// gate's own journal.
    pub to: Option<String>,
}

/// The most characters of a row's agent name shown; the rest is cut.
const ROW_NAME_CHARS: usize = 64;

impl GateRowVm {
    /// The row `record` draws on the hour's card.
    pub fn of(record: &ApprovalRequestContent) -> GateRowVm {
        let binding = &record.action.exec_binding;
        GateRowVm {
            request: binding["request"]
                .as_str()
                .filter(|name| EventId::parse(*name).is_err())
                .map(shown_request),
            ticket: binding["ticket"]
                .as_str()
                .filter(|ticket| Ulid::from_string(ticket).is_ok())
                .map(str::to_owned),
            to: (record.action.tool == AgentTool::Delegate.as_wire())
                .then(|| record.action.args["agent"].as_str())
                .flatten()
                .map(|agent| {
                    let mut cut: String = agent.chars().take(ROW_NAME_CHARS).collect();
                    if cut.len() < agent.len() {
                        cut.push('…');
                    }
                    format!("{cut:?}")
                }),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const GATE: &str = "@forge:example.org";
    const PEER: &str = "@github:example.org";
    const PERSON: &str = "@tgorka:example.org";
    const MARTA: &str = "@marta:example.org";
    const TICKET: &str = "01JGATE0000000000000000001";

    fn user(id: &str) -> OwnedUserId {
        OwnedUserId::try_from(id).expect("user")
    }

    fn agent(user: &UserId) -> bool {
        user.as_str() == GATE
    }

    /// The gate's message `state` about the request `name` of event
    /// `event`, as `gate::content` writes it.
    fn said(event: &str, name: &str, state: &str, extra: Value) -> String {
        let mut key = json!({"v": 1, "request": name, "state": state});
        if let Value::Object(extra) = extra {
            key.as_object_mut().expect("object").extend(extra);
        }
        json!({
            "type": "m.room.message", "event_id": "$said:example.org",
            "content": {
                "msgtype": "m.text", "body": "…", (events::GATE): key,
                "m.relates_to": {"rel_type": "m.reference", "event_id": event},
            },
        })
        .to_string()
    }

    fn request(name: Option<&str>) -> String {
        let mut content = json!({"msgtype": "m.text", "body": "status of PR 12?"});
        if let Some(name) = name {
            content[events::GATE] = json!({"request": name, "deadline_ms": 5000});
        }
        json!({"type": "m.room.message", "content": content}).to_string()
    }

    /// A loaded, unedited message whose sender's name is not read.
    fn msg<'a>(event: Option<&'a str>, sender: &'a UserId, json: Option<&'a str>) -> GateInput<'a> {
        GateInput {
            event,
            sender,
            json,
            edited: false,
            name: None,
        }
    }

    /// UX-DR147: a step counts only from the gate — at an agent's power,
    /// not the own user — and only as the gate writes it: this version, a
    /// room state, a reference to the request. The peer's own gate key, a
    /// person's look-alike, the log-only states, a newer version and an
    /// edit are ordinary messages.
    #[test]
    fn a_gate_step_counts_only_from_the_gate() {
        let own = user(PERSON);
        let view = GateView::default();
        let line = |sender: &str, json: &str| {
            let sender = user(sender);
            view.line(
                &own,
                msg(Some("$x:example.org"), &sender, Some(json)),
                &agent,
            )
        };
        let ticket = said(
            "$req:example.org",
            "r1",
            "ticket",
            json!({"ticket": TICKET, "deadline_ms": 3000}),
        );
        assert_eq!(
            line(GATE, &ticket),
            Some(GateLineVm {
                role: GateRoleVm::Gate,
                state: GateStepVm::Ticket,
                request: Some("\"r1\"".to_owned()),
                ticket: Some(TICKET.to_owned()),
                reason: None,
                deadline_ms: Some(3000),
                sender_name: None,
            })
        );
        for (state, step) in [
            ("answer", GateStepVm::Answered),
            ("result", GateStepVm::Result),
            ("refused", GateStepVm::Refused),
        ] {
            let drawn = line(GATE, &said("$req:example.org", "r1", state, json!({})));
            assert_eq!(drawn.map(|line| line.state), Some(step), "{state}");
        }
        let refused = said(
            "$req:example.org",
            "r1",
            "refused",
            json!({"reason": "busy"}),
        );
        assert_eq!(
            line(GATE, &refused).and_then(|line| line.reason),
            Some(GateReasonVm::Busy)
        );
        // Anyone below an agent's power writing the gate's words, and the
        // own user even at an agent's power: not a step.
        for sender in [PEER, MARTA] {
            assert_eq!(line(sender, &ticket), None, "{sender}");
        }
        assert_eq!(
            view.line(&own, msg(None, &own, Some(&ticket)), &|_| true),
            None
        );
        // The log's own states, a newer version, an edit.
        for (json, why) in [
            (said("$req:example.org", "r1", "held", json!({})), "held"),
            (
                said("$req:example.org", "r1", "request", json!({})),
                "request",
            ),
            (
                said("$req:example.org", "r1", "ticket", json!({"v": 2})),
                "v2",
            ),
        ] {
            assert_eq!(line(GATE, &json), None, "{why}");
        }
        let mut edit: Value = serde_json::from_str(&ticket).expect("json");
        edit["content"]["m.relates_to"] =
            json!({"rel_type": "m.replace", "event_id": "$req:example.org"});
        assert_eq!(line(GATE, &edit.to_string()), None, "an edit");
        // A ticket that names no session id names none.
        let forged = said(
            "$req:example.org",
            "r1",
            "ticket",
            json!({"ticket": "child"}),
        );
        assert_eq!(line(GATE, &forged).expect("a ticket").ticket, None);
        // An async request is named by its event: no name is shown.
        let unnamed = said("$req:example.org", "$req:example.org", "ticket", json!({}));
        assert_eq!(line(GATE, &unnamed).expect("a ticket").request, None);
    }

    /// UX-DR147: a message is a request once the gate has spoken of it, and
    /// its chip is the newest step said of it; the outside system is the
    /// sender of the request the newest step spoke of. A message the gate
    /// never referenced, the own user's, and one only a forged step
    /// references are ordinary messages.
    #[test]
    fn a_request_is_drawn_by_the_gates_newest_word_on_it() {
        let own = user(PERSON);
        let (gate, peer, marta) = (user(GATE), user(PEER), user(MARTA));
        let r1 = request(Some("r1"));
        let r2 = request(None);
        let ticket = said("$r1:x", "r1", "ticket", json!({"ticket": TICKET}));
        let result = said("$r1:x", "r1", "result", json!({"ticket": TICKET}));
        let answer_marta = said("$m:x", "m", "answer", json!({}));
        let forged = said("$r2:x", "$r2:x", "refused", json!({"reason": "busy"}));
        let items: Vec<GateInput<'_>> = vec![
            msg(Some("$r1:x"), &peer, Some(&r1)),
            msg(Some("$r2:x"), &peer, Some(&r2)),
            msg(Some("$m:x"), &marta, Some("{}")),
            msg(Some("$t:x"), &gate, Some(&ticket)),
            msg(Some("$f:x"), &peer, Some(&forged)),
            msg(Some("$res:x"), &gate, Some(&result)),
            msg(Some("$am:x"), &gate, Some(&answer_marta)),
        ];
        let view = GateView::of(items.iter().copied(), &own, &agent);
        let drawn = |at: usize| view.line(&own, items[at], &agent);
        let first = drawn(0).expect("r1 is a request");
        assert_eq!(first.role, GateRoleVm::Request);
        assert_eq!(first.state, GateStepVm::Result, "the newest step stands");
        assert_eq!(first.ticket.as_deref(), Some(TICKET));
        assert_eq!(first.request.as_deref(), Some("\"r1\""));
        assert_eq!(drawn(1), None, "only the peer's forged step names it");
        // The gate answered Marta's message: she is below an agent's power,
        // so it is drawn as a request, and the newest step spoke of hers.
        assert_eq!(drawn(2).map(|line| line.state), Some(GateStepVm::Answered));
        assert_eq!(view.peer(), Some(marta.as_ref()));
        let without_marta = GateView::of(items[..6].iter().copied(), &own, &agent);
        assert_eq!(without_marta.peer(), Some(peer.as_ref()));
        // The own user's message the gate spoke of is never a request.
        let own_item = msg(Some("$r1:x"), &own, Some(&r1));
        assert_eq!(view.line(&own, own_item, &agent), None);
        // Before the gate has said anything, nothing is a request and no
        // one is the peer.
        let before = GateView::of(items[..3].iter().copied(), &own, &agent);
        assert_eq!(before.line(&own, items[0], &agent), None);
        assert_eq!(before.peer(), None);
    }

    /// UX-DR147, R114's precedent: the gate never reads an edit, so its
    /// word does not stand over an edited request — that is an ordinary
    /// message, while the gate's own line on it stands and its sender is
    /// still the peer. A request names its sender as the outside system
    /// chose the name, quoted; unread, by its user id.
    #[test]
    fn an_edited_request_is_an_ordinary_message_and_its_sender_is_quoted() {
        let own = user(PERSON);
        let (gate, peer) = (user(GATE), user(PEER));
        let r1 = request(Some("r1"));
        let ticket = said("$r1:x", "r1", "ticket", json!({"ticket": TICKET}));
        let asked = GateInput {
            name: Some("pr-12 · answered"),
            ..msg(Some("$r1:x"), &peer, Some(&r1))
        };
        let word = msg(Some("$t:x"), &gate, Some(&ticket));
        let view = GateView::of([asked, word], &own, &agent);
        let drawn = view.line(&own, asked, &agent).expect("a request");
        assert_eq!(drawn.sender_name.as_deref(), Some("\"pr-12 · answered\""));
        let unnamed = view.line(&own, msg(Some("$r1:x"), &peer, Some(&r1)), &agent);
        assert_eq!(
            unnamed.and_then(|line| line.sender_name),
            Some(format!("{PEER:?}"))
        );
        let edited = GateInput {
            edited: true,
            ..asked
        };
        assert_eq!(view.line(&own, edited, &agent), None);
        let stands = view.line(&own, word, &agent).expect("the gate's word");
        assert_eq!(
            (stands.state, stands.sender_name),
            (GateStepVm::Ticket, None)
        );
        let after_edit = GateView::of([edited, word], &own, &agent);
        assert_eq!(after_edit.peer(), Some(peer.as_ref()));
    }

    fn record(tool: &str, args: Value, exec_binding: Value) -> ApprovalRequestContent {
        serde_json::from_value(json!({
            "v": 1, "id": TICKET, "session": "sessions/active/x", "room": "!gate:example.org",
            "agent": "forge", "tier": 2, "summary": "s",
            "action": {"tool": tool, "args": args, "exec_binding": exec_binding},
            "checkpoint_sha256": "00",
            "preconditions": {"files": [], "workspace": null, "screen": null, "max_staleness_s": null},
            "binding_digest": "d",
            "scopes": ["once"], "expires_at": "2026-10-14T00:00:00Z",
            "approvers": [PERSON], "dispatch_chain": [PERSON, GATE],
        }))
        .expect("a record")
    }

    /// UX-DR160: a row names the request it serves, quoted as the gate
    /// shows it, the ticket it decides and the agent the work would go to,
    /// quoted as data; a journal entry goes to no agent; an async request
    /// shows no name; a ticket that names no session id names none.
    #[test]
    fn a_card_row_names_its_request_ticket_and_agent() {
        let row = GateRowVm::of(&record(
            "delegate",
            json!({"agent": "tola\u{202e}grey", "brief": "Check PR 12"}),
            json!({"ticket": TICKET, "request": "r1"}),
        ));
        assert_eq!(
            row,
            GateRowVm {
                request: Some("\"r1\"".to_owned()),
                ticket: Some(TICKET.to_owned()),
                to: Some("\"tola\\u{202e}grey\"".to_owned()),
            }
        );
        let journal = GateRowVm::of(&record(
            "journal_append",
            json!({"text": "line one"}),
            json!({"ticket": "child", "request": "$req:example.org"}),
        ));
        assert_eq!(
            journal,
            GateRowVm {
                request: None,
                ticket: None,
                to: None
            }
        );
        let long = "a".repeat(ROW_NAME_CHARS + 1);
        let cut = GateRowVm::of(&record("delegate", json!({"agent": long}), json!({})));
        assert_eq!(cut.to, Some(format!("\"{}…\"", "a".repeat(ROW_NAME_CHARS))));
    }
}
