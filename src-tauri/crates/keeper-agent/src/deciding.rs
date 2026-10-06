//! Who decided, and whether it counts (93.3; AD-395, R30, R87).
//!
//! A decision reaches a session's worker only sealed by its sender's own
//! device and from a reader of the session ([`crate::rooms::classify`]).
//! Here the worker gathers the rest of keeper-core's [`TrustFacts`] — the
//! record's approvers and requester, what the sender's homeserver publishes
//! of the device and identity, and the host's trusted master key — and
//! [`decide_trust`] answers. Whatever does not count is logged as an ignored
//! decision with its reason (R80) and changes nothing.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use keeper_core::agents::approval::{ApprovalRecord, DecidedBy};
use keeper_core::agents::matrix::AgentClient;
use keeper_core::agents::trust::{
    decide_trust, Anchor, Published, Trust, TrustFacts, KEYS_UNKNOWN,
};
use matrix_sdk::ruma::{DeviceId, OwnedEventId, OwnedRoomId, RoomId, UserId};

use crate::agent::{AgentDeps, Arrived, ServedSession};
use crate::approvals::{DecisionSource, RoomFuture};
use crate::rooms::reads;

/// Where an arrival for a session goes: its worker's channel, through the
/// host's router.
pub type Inbox = Arc<dyn Fn(Arrived) + Send + Sync>;

/// The decisions this host waits for in its proxies' DMs (R89): a request
/// its session's room could not carry went to each approver's proxy DM
/// (R85), so the decision on it arrives there, to the proxy's `main`
/// worker, which hands it to the session that asked — R55's way home. The
/// record stays in the requesting session; only its worker writes the
/// decision and consumes. A decision goes only to another session's room,
/// at most once per event, and a forwarded one is never forwarded again
/// (R184): nothing circles.
#[derive(Default)]
pub struct Forwards {
    routes: Mutex<HashMap<(OwnedRoomId, String), (OwnedRoomId, Inbox)>>,
    forwarded: Mutex<HashSet<OwnedEventId>>,
}

impl Forwards {
    /// A decision on approval `id` in the DM `dm` goes to `to`, the inbox of
    /// the session whose room is `home`; a DM that is that room itself
    /// routes nothing.
    pub fn expect(&self, dm: &RoomId, id: &str, home: &RoomId, to: Inbox) {
        if dm == home {
            return;
        }
        self.routes
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert((dm.to_owned(), id.to_owned()), (home.to_owned(), to));
    }

    /// Hand `arrived`, a decision in the DM `dm`, to the session whose
    /// request it answers; whether it is that session's (handed now, or
    /// already). Only the approval's own id routes, so a decision on
    /// anything else stays in the DM, as does one forwarded here.
    pub fn forward(&self, dm: &RoomId, arrived: &Arrived) -> bool {
        if arrived.via.is_some() {
            return false;
        }
        let Some(id) = arrived.content["id"].as_str() else {
            return false;
        };
        let route = self
            .routes
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&(dm.to_owned(), id.to_owned()))
            .cloned();
        let Some((home, to)) = route else {
            return false;
        };
        if home == dm {
            return false;
        }
        let first = self
            .forwarded
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(arrived.event_id.clone());
        if first {
            to(Arrived {
                via: Some(dm.to_owned()),
                ..arrived.clone()
            });
        }
        true
    }
}

/// The decision source over an agent's own Matrix client: the trust
/// adapter both production hosts install (R92) — agentd under
/// [`Anchor::Pinned`], the desktop under [`Anchor::Desktop`].
pub struct ClientDecisions {
    pub client: AgentClient,
    pub anchor: Anchor,
}

impl DecisionSource for ClientDecisions {
    fn anchor(&self) -> &Anchor {
        &self.anchor
    }

    fn published<'a>(
        &'a self,
        user: &'a UserId,
        device: &'a DeviceId,
    ) -> RoomFuture<'a, Published> {
        Box::pin(self.client.published(user, device))
    }
}

impl ServedSession {
    /// A decision in this session's room that answers a request another
    /// session sent here, handed to that session (R89); whether it was. An
    /// approval of this session's own is decided here, never forwarded.
    pub(crate) fn forward(&self, arrived: &Arrived) -> bool {
        let own = arrived.content["id"]
            .as_str()
            .is_some_and(|id| self.context.parked.contains_key(id));
        !own && self
            .doors
            .as_ref()
            .is_some_and(|doors| doors.forwards().forward(&self.context.agent.room, arrived))
    }

    /// Wait, in each proxy DM a pending approval's request went to, for its
    /// decision (R85, R89): at serve start, from the DMs kept beside each
    /// record — never inferred from the room's read cursor — so a restart
    /// hears every one still waiting there again.
    pub(crate) fn expect_decisions(&self, deps: &AgentDeps) {
        let (Some(doors), Some(inbox)) = (&self.doors, &self.inbox) else {
            return;
        };
        for pending in self.context.parked.values() {
            if pending.ended.is_some() {
                continue;
            }
            for (_, dm) in self.asked(deps, &pending.id) {
                doors.forwards().expect(
                    &dm,
                    &pending.id,
                    &self.context.agent.room,
                    Arc::clone(inbox),
                );
            }
        }
    }

    /// Who decided `arrived` on `record`, when it counts at the record's
    /// tier; the reason it is ignored when it does not.
    pub(crate) async fn decided_by(
        &mut self,
        deps: &AgentDeps,
        source: &dyn DecisionSource,
        record: &ApprovalRecord,
        arrived: &Arrived,
    ) -> Result<DecidedBy, String> {
        let sender = &arrived.sender;
        let known = self.delegations.as_ref().map(|rooms| rooms.known());
        let seat = Seat {
            sender_is_agent: *sender == deps.home.config.matrix_user
                || known.as_ref().is_some_and(|known| {
                    known
                        .agents
                        .iter()
                        .any(|agent| agent.matrix_user == *sender)
                }),
            sender_in_label: reads(&self.context.label.readers, sender),
            sender_in_approvers: reads(&record.label.readers, sender),
            requester: record.dispatch_chain.first().cloned(),
        };
        judge(source, arrived, seat, record.risk.tier).await
    }
}

/// What the session knows of a decision's sender.
#[derive(Debug, Clone)]
pub struct Seat {
    pub sender_is_agent: bool,
    /// Whether they read the session as its label is now.
    pub sender_in_label: bool,
    /// Whether they read the label the record was parked under.
    pub sender_in_approvers: bool,
    /// The head of the record's `dispatch_chain`.
    pub requester: Option<String>,
}

/// Whether `arrived`, a decision on a record of `tier`, counts: what its
/// sender's homeserver publishes of the device and identity, asked for
/// this decision alone through `source` (R182), under `source`'s anchor,
/// with what the session knows of the sender (`seat`). Who decided when it
/// does; why not when it does not — a closed reason, never the text of an
/// error the network returned.
pub async fn judge(
    source: &dyn DecisionSource,
    arrived: &Arrived,
    seat: Seat,
    tier: u8,
) -> Result<DecidedBy, String> {
    let sender = &arrived.sender;
    let (published, this_process) = match &arrived.device {
        Some(device) => (
            source
                .published(sender, device)
                .await
                .map_err(|_| KEYS_UNKNOWN.to_owned())?,
            source.anchor().this_process(sender, device.as_str()),
        ),
        None => (Published::default(), false),
    };
    let facts = TrustFacts {
        sender: sender.to_string(),
        device: arrived.device.as_ref().map(ToString::to_string),
        event_encrypted: matches!(
            arrived.arrival,
            crate::rooms::Arrival::Decision { sealed: true }
        ),
        sender_is_agent: seat.sender_is_agent,
        sender_in_label: seat.sender_in_label,
        sender_in_approvers: seat.sender_in_approvers,
        requester: seat.requester,
        device_is_this_process: this_process,
        device_cross_signed_by_owner: published.cross_signed_by_owner,
        owner_master_key: published.master_key,
        pinned_master_key: source.anchor().pinned(sender).map(str::to_owned),
    };
    match decide_trust(&facts, tier) {
        Trust::Verified => Ok(DecidedBy {
            user: facts.sender,
            device: facts.device.unwrap_or_default(),
            verified: true,
        }),
        Trust::Unverified { reason } => Err(reason),
    }
}
