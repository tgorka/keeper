//! A proxy's host answers its person's ask for their `main` DM (story
//! 101.1, R416, R450; [`keeper_core::agents::main_dm`]).
//!
//! The ask arrives in the person's control room and is queued on the copy of
//! the agent it names ([`Asks`]); the host's tick serves the queue with its
//! own calibrated server clock ([`serve_all`]). [`main_dm::decide`] reads
//! each ask — its age from the server's `unsigned.age`, else from its
//! `origin_server_ts` against that clock, else it is not answered. A refusal
//! is answered with its sentence, at most once per asker per
//! [`REFUSE_AGAIN`]: the request row is open to every member at power 0,
//! and one stranger's stream of asks is not one encrypted answer each (the
//! queue keeps one ask per asker, [`ASKS_KEPT`] askers at most). An ask it
//! takes is served by one host only — the one that wins the request's claim
//! in that room ([`claim::main_dm_key`]), a minute lease judged by the host's
//! calibrated clock, so a crashed holder's claim is taken over once it
//! lapses — with the same [`seed::main_dm_synced`] `keeper-agentd agents
//! init` uses, fenced by that claim: no room is made, no folder written and
//! no anchor sent once it no longer holds. The DM is found (its session
//! folder, or a room the copy is already in) or made, the person invited
//! again if they left it, and the answer names the room.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use keeper_core::agents::claim::{self, Claimant};
use keeper_core::agents::events::{
    MainDmAnswerContent, CONTENT_VERSION, CONTROL_ROOM_TYPE, MAIN_DM_ANSWER,
};
use keeper_core::agents::main_dm::{self, Proxy, Request, Verdict};
use keeper_core::agents::matrix::AgentMatrixError;
use matrix_sdk::ruma::{OwnedEventId, OwnedRoomId, OwnedUserId, UserId};
use matrix_sdk::{Room, RoomMemberships};
use serde_json::Value;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::claims::{acquire, release, Acquired, ClaimPort, Lease, RoomClaims, Rtt, ServerClock};
use crate::runtime::Copy;
use crate::seed;

/// How many askers' asks one copy keeps waiting at most.
pub(crate) const ASKS_KEPT: usize = 16;

/// How long after refusing an asker a copy refuses them again: asks in
/// between are dropped without an answer.
pub(crate) const REFUSE_AGAIN: Duration = Duration::from_secs(10 * 60);

/// One copy's asks waiting for the host's tick: the newest per room and
/// asker, the askers it refused lately, and the ask it is serving.
pub(crate) struct Asks<T> {
    waiting: BTreeMap<(OwnedRoomId, OwnedUserId), (T, Instant)>,
    refused: HashMap<OwnedUserId, Instant>,
    serving: Option<JoinHandle<()>>,
}

impl<T> Default for Asks<T> {
    fn default() -> Self {
        Asks {
            waiting: BTreeMap::new(),
            refused: HashMap::new(),
            serving: None,
        }
    }
}

impl<T> Asks<T> {
    /// Keep `ask` from `sender` in `room`, heard at `now`: it replaces that
    /// asker's earlier one; a new asker past [`ASKS_KEPT`] is dropped.
    /// Whether it was kept.
    pub(crate) fn push(
        &mut self,
        room: OwnedRoomId,
        sender: OwnedUserId,
        ask: T,
        now: Instant,
    ) -> bool {
        let key = (room, sender);
        if !self.waiting.contains_key(&key) && self.waiting.len() >= ASKS_KEPT {
            return false;
        }
        self.waiting.insert(key, (ask, now));
        true
    }

    /// Every ask waiting, with when it was heard; the queue is left empty.
    pub(crate) fn take(&mut self) -> Vec<(OwnedRoomId, OwnedUserId, T, Instant)> {
        std::mem::take(&mut self.waiting)
            .into_iter()
            .map(|((room, sender), (ask, heard))| (room, sender, ask, heard))
            .collect()
    }

    /// Whether `sender` may be answered with a refusal at `now`: not refused
    /// within [`REFUSE_AGAIN`]. One that may is counted as refused now.
    pub(crate) fn may_refuse(&mut self, sender: &UserId, now: Instant) -> bool {
        self.refused
            .retain(|_, at| now.saturating_duration_since(*at) < REFUSE_AGAIN);
        if self.refused.contains_key(sender) {
            return false;
        }
        if self.refused.len() >= ASKS_KEPT * 4 {
            return false;
        }
        self.refused.insert(sender.to_owned(), now);
        true
    }

    /// Whether an ask is being served now.
    fn busy(&mut self) -> bool {
        if self.serving.as_ref().is_some_and(JoinHandle::is_finished) {
            self.serving = None;
        }
        self.serving.is_some()
    }
}

/// An ask as the copy's handler heard it.
pub(crate) struct Heard {
    room: Room,
    value: Value,
}

/// Queue `value`, a `dev.keeper.agent.main_dm.request` (a control-room state
/// event keyed by its asker) that arrived in `room`, on `copy` for the
/// host's next tick.
pub(crate) fn hear(copy: &Copy, room: &Room, value: Value) {
    let Some(sender) = value["sender"]
        .as_str()
        .and_then(|sender| UserId::parse(sender).ok())
    else {
        return;
    };
    let kept = copy.dm_asks.lock().unwrap_or_else(|p| p.into_inner()).push(
        room.room_id().to_owned(),
        sender.clone(),
        Heard {
            room: room.clone(),
            value,
        },
        Instant::now(),
    );
    if !kept {
        tracing::debug!(room = %room.room_id(), %sender, "agentd: too many DM requests wait; this one is dropped");
    }
}

/// What a host serves its copies' asks with: its name and the claims'
/// calibrated server clock.
#[derive(Clone)]
pub(crate) struct DmRound {
    pub(crate) host: String,
    pub(crate) clock: Arc<ServerClock>,
    pub(crate) rtt: Arc<Rtt>,
}

/// Serve every copy's waiting asks on `round` (the host's tick). An ask the
/// copy takes while it is still serving one waits for the next tick.
pub(crate) fn serve_all(copies: &[Arc<Copy>], round: &DmRound) {
    for copy in copies {
        let asks = copy
            .dm_asks
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        for (room, sender, heard, at) in asks {
            serve(copy, round, room, sender, heard, at);
        }
    }
}

fn serve(
    copy: &Arc<Copy>,
    round: &DmRound,
    room_id: OwnedRoomId,
    sender: OwnedUserId,
    heard: Heard,
    at: Instant,
) {
    let config = &copy.deps.home.config;
    let Some(request) = heard.value["event_id"]
        .as_str()
        .and_then(|id| OwnedEventId::try_from(id).ok())
    else {
        return;
    };
    let in_control_room = heard
        .room
        .room_type()
        .is_some_and(|kind| kind.to_string() == CONTROL_ROOM_TYPE);
    let age = main_dm::request_age(
        heard.value["unsigned"]["age"].as_u64(),
        at.elapsed().as_millis() as u64,
        heard.value["origin_server_ts"].as_u64(),
        round.clock.calibrated().then(|| round.clock.now()),
    );
    let verdict = main_dm::decide(
        &Request {
            in_control_room,
            sender: &sender,
            content: &heard.value["content"],
            age_ms: age,
        },
        &Proxy {
            agent: &config.matrix_user,
            name: &config.name,
            human: config.human.as_deref(),
        },
    );
    match verdict {
        Verdict::Ignore(why) => {
            tracing::debug!(room = %room_id, why, "agentd: a DM request is not this copy's");
        }
        Verdict::Refuse(sentence) => {
            let mut asks = copy.dm_asks.lock().unwrap_or_else(|p| p.into_inner());
            if !asks.may_refuse(&sender, Instant::now()) {
                tracing::debug!(room = %room_id, %sender, "agentd: a DM request refused lately is not answered again");
                return;
            }
            drop(asks);
            tracing::warn!(room = %room_id, %sender, %sentence, "agentd: a DM request was refused");
            let copy = Arc::clone(copy);
            tokio::spawn(async move {
                send_answer(&copy, &heard.room, request, None, Some(sentence)).await;
            });
        }
        Verdict::Open => {
            let mut asks = copy.dm_asks.lock().unwrap_or_else(|p| p.into_inner());
            if asks.busy() {
                // One at a time: this one waits for the next tick.
                asks.push(room_id, sender, heard, at);
                return;
            }
            let (task_copy, round) = (Arc::clone(copy), round.clone());
            asks.serving = Some(tokio::spawn(async move {
                open(&task_copy, &round, &heard.room, request, &sender).await;
            }));
        }
    }
}

/// Take the request's claim through `port` as `me`, on the minute lease,
/// judged by `clock` — the host's, calibrated by its manifest's read-back —
/// so another device's claim that lapsed unreleased is taken over and a live
/// one, or one an uncalibrated clock cannot judge, is left alone.
pub(crate) async fn take_claim(
    port: &dyn ClaimPort,
    me: &Claimant,
    clock: &ServerClock,
    rtt: &Rtt,
) -> Result<Option<Arc<Lease>>, AgentMatrixError> {
    Ok(match acquire(port, me, clock, rtt, None).await? {
        Acquired::Won { lease, .. } => Some(lease),
        Acquired::HeldElsewhere | Acquired::Yielded => None,
    })
}

/// Open the DM with `human` under the request's claim in `room`, and say
/// which room it is — or why it is not.
async fn open(
    copy: &Arc<Copy>,
    round: &DmRound,
    room: &Room,
    request: OwnedEventId,
    human: &UserId,
) {
    let deps = &copy.deps;
    let config = &deps.home.config;
    let claims = RoomClaims::keyed(
        copy.client.clone(),
        room.room_id().to_owned(),
        &claim::main_dm_key(&config.matrix_user),
        copy.syncs.clone(),
    );
    let me = Claimant {
        host: round.host.clone(),
        device: copy.client.device_id().unwrap_or_default(),
        agent: config.matrix_user.clone(),
    };
    let lease = match take_claim(&claims, &me, &round.clock, &round.rtt).await {
        Ok(Some(lease)) => lease,
        Ok(None) => {
            tracing::info!(room = %room.room_id(), "agentd: another host answers this DM request");
            return;
        }
        Err(error) => {
            tracing::warn!(room = %room.room_id(), %error, "agentd: a DM request's claim could not be taken");
            let sentence = main_dm::not_opened_sentence(
                &config.name,
                &format!("its host could not take the request: {error}"),
            );
            send_answer(copy, room, request, None, Some(sentence)).await;
            return;
        }
    };
    let now = chrono::Local::now();
    let fence: seed::Fence = {
        let lease = Arc::clone(&lease);
        Arc::new(move || lease.may_write())
    };
    let made = seed::main_dm_synced(
        &copy.client,
        config,
        &deps.home.drive,
        &deps.sessions_zone,
        &deps.sessions_subfolder,
        &round.host,
        now,
        Some(fence),
    )
    .await;
    let (room_id, refused) = match made {
        Ok(dm) => {
            invite_again(copy, &dm.room, human, now).await;
            tracing::info!(room = %dm.room, made = ?dm.made, "agentd: answered a DM request");
            (Some(dm.room), None)
        }
        Err(error) => {
            tracing::warn!(%error, "agentd: a DM request could not be served");
            (
                None,
                Some(main_dm::not_opened_sentence(&config.name, &error)),
            )
        }
    };
    send_answer(copy, room, request, room_id, refused).await;
    if let Err(error) = release(&claims, &me, &lease, &round.clock, &round.rtt).await {
        tracing::warn!(%error, "agentd: a DM request's claim could not be handed back; it lapses");
    }
}

/// Invite `human` into the DM `room` again when they are neither in it nor
/// invited — a DM they left, adopted rather than made — under the `main`
/// session's label, which reaches them alone. A failure is a WARN: the
/// answer still names the room.
async fn invite_again(
    copy: &Arc<Copy>,
    room: &OwnedRoomId,
    human: &UserId,
    now: chrono::DateTime<chrono::Local>,
) {
    let Some(joined) = copy.client.client().get_room(room) else {
        return;
    };
    let present = joined
        .members(RoomMemberships::JOIN | RoomMemberships::INVITE)
        .await
        .is_ok_and(|members| members.iter().any(|member| member.user_id() == human));
    if present {
        return;
    }
    let deps = &copy.deps;
    let label = seed::main_session(&deps.home.config, &deps.home.drive, human, room, now).label;
    if let Err(error) = copy.client.invite(room, human, &label, None).await {
        tracing::warn!(%room, %error, "agentd: the person could not be invited to their DM again");
    }
}

/// Send the answer to `request` into `room`. A failure is a WARN; the
/// person's keeper says it waited in vain.
async fn send_answer(
    copy: &Arc<Copy>,
    room: &Room,
    request: OwnedEventId,
    room_id: Option<OwnedRoomId>,
    refused: Option<String>,
) {
    let content = MainDmAnswerContent {
        v: CONTENT_VERSION,
        request,
        agent: copy.deps.home.config.matrix_user.clone(),
        room: room_id,
        refused,
    };
    let Ok(content) = serde_json::to_value(content) else {
        return;
    };
    if let Err(error) = copy
        .client
        .send(room.room_id(), MAIN_DM_ANSWER, content, None)
        .await
    {
        tracing::warn!(room = %room.room_id(), %error, "agentd: a DM request's answer could not be sent");
    }
}

/// The asks a copy keeps, shared with its handler.
pub(crate) type SharedAsks = Arc<Mutex<Asks<Heard>>>;

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use keeper_core::agents::matrix::ServerState;
    use matrix_sdk::ruma::{MilliSecondsSinceUnixEpoch, UInt};

    use super::*;
    use crate::claims::{wall_ms, ClaimFuture};

    fn user(id: &str) -> OwnedUserId {
        OwnedUserId::try_from(id).expect("user")
    }

    fn control() -> OwnedRoomId {
        OwnedRoomId::try_from("!control:example.org").expect("room")
    }

    /// DW (review-101rooms-1): a member at power 0 who keeps asking is
    /// refused once per [`REFUSE_AGAIN`], not once per ask; another asker is
    /// refused on their own; the queue keeps one ask per asker and no more
    /// than [`ASKS_KEPT`] askers.
    #[tokio::test(start_paused = true)]
    async fn a_stranger_asking_again_and_again_is_refused_once_a_while() {
        let mut asks = Asks::<u32>::default();
        let (stranger, other) = (user("@stranger:example.org"), user("@other:example.org"));
        let now = Instant::now();
        assert!(asks.may_refuse(&stranger, now));
        for n in 1..100 {
            assert!(
                !asks.may_refuse(&stranger, now + Duration::from_secs(n)),
                "ask {n}"
            );
        }
        assert!(asks.may_refuse(&other, now), "someone else's first ask");
        assert!(
            asks.may_refuse(&stranger, now + REFUSE_AGAIN),
            "a while later"
        );

        for n in 0..10 {
            assert!(asks.push(control(), stranger.clone(), n, now));
        }
        assert_eq!(asks.take().len(), 1, "one ask per asker, the newest");
        for n in 0..ASKS_KEPT {
            assert!(asks.push(control(), user(&format!("@u{n}:example.org")), 0, now));
        }
        assert!(
            !asks.push(control(), stranger.clone(), 0, now),
            "one asker too many"
        );
        assert!(
            asks.push(control(), user("@u0:example.org"), 1, now),
            "a kept asker's newer ask replaces theirs"
        );
        assert_eq!(asks.take().len(), ASKS_KEPT);
    }

    /// A homeserver holding one claim, stamping what it is sent with `now`.
    struct OneClaim {
        now: AtomicU64,
        current: Mutex<Option<ServerState>>,
    }

    impl ClaimPort for OneClaim {
        fn send(&self, content: Value) -> ClaimFuture<'_, Result<OwnedEventId, AgentMatrixError>> {
            Box::pin(async move {
                let event = OwnedEventId::try_from("$mine:example.org").expect("event");
                *self.current.lock().expect("lock") = Some(ServerState {
                    event_id: event.clone(),
                    sender: user("@nixi:example.org"),
                    origin_server_ts: MilliSecondsSinceUnixEpoch(
                        UInt::new(self.now.load(Ordering::Relaxed)).expect("ts"),
                    ),
                    content,
                });
                Ok(event)
            })
        }

        fn read(&self) -> ClaimFuture<'_, Result<Option<ServerState>, AgentMatrixError>> {
            Box::pin(async move { Ok(self.current.lock().expect("lock").clone()) })
        }

        fn next_sync(&self) -> ClaimFuture<'_, Duration> {
            Box::pin(async move { Duration::from_millis(30) })
        }
    }

    fn claimant(host: &str, device: &str) -> Claimant {
        Claimant {
            host: host.to_owned(),
            device: device.to_owned(),
            agent: user("@nixi:example.org"),
        }
    }

    /// R450 (review-101rooms-1 07): a request's claim another device left
    /// unreleased — it crashed while answering — is taken over by the host's
    /// calibrated clock once its minute lease lapsed; never while it is
    /// live, and never by a clock no read-back calibrated.
    #[tokio::test(start_paused = true)]
    async fn a_crashed_devices_request_claim_is_taken_once_it_lapses() {
        let ttl = claim::TTL.as_millis() as u64;
        // Lapsed by this host's own wall clock: only a calibrated clock may
        // say so.
        let at = wall_ms() - ttl - 5_000;
        let crashed = claimant("argo", "ARGO1").content(3, at, at, false, None);
        let port = OneClaim {
            now: AtomicU64::new(at),
            current: Mutex::new(Some(ServerState {
                event_id: OwnedEventId::try_from("$crashed:example.org").expect("event"),
                sender: user("@nixi:example.org"),
                origin_server_ts: MilliSecondsSinceUnixEpoch(UInt::new(at).expect("ts")),
                content: serde_json::to_value(&crashed).expect("content"),
            })),
        };
        let me = claimant("electra", "ELECTRA1");
        let host_clock = |server_now: u64| {
            let clock = ServerClock::default();
            clock.observe(server_now, wall_ms(), wall_ms());
            clock
        };
        let live = take_claim(&port, &me, &host_clock(at + ttl - 1_000), &Rtt::default())
            .await
            .expect("read");
        assert!(live.is_none(), "live");
        let uncalibrated = take_claim(&port, &me, &ServerClock::default(), &Rtt::default())
            .await
            .expect("read");
        assert!(
            uncalibrated.is_none(),
            "an uncalibrated clock judges no other device"
        );
        port.now.store(at + ttl, Ordering::Relaxed);
        let taken = take_claim(&port, &me, &host_clock(at + ttl), &Rtt::default())
            .await
            .expect("read")
            .expect("taken over");
        assert_eq!(taken.epoch, 4);
    }
}
