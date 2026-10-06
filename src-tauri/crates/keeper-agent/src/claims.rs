//! Claims: which host writes a session (AD-378, S-05; story 90.6).
//!
//! A host acquires a session's claim by sending `dev.keeper.agent.claim`
//! (retry off, D1), reading it back from the server — never the cache, which
//! can be a sync behind (C9) — waiting [`claim::settle`], and reading it
//! again: it proceeds only if the claim is still its own event. Then it holds
//! a [`Lease`]: its [`SessionWriter`](crate::writer::SessionWriter) stamps
//! every line with the lease's epoch and claim event and writes nothing once
//! the lease is lost or unconfirmed for [`claim::STOP_WITHOUT_RENEWAL`] — by
//! the monotonic clock or by the wall clock, since only the second runs on
//! while the machine sleeps.
//!
//! The log records transitions only — `acquired`, `released`, `lost` — each
//! with its claim event and the server's time; renewals are not logged.

use std::collections::VecDeque;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use keeper_core::agents::claim::{self, Claimant, ServerClaim, RENEW_EVERY};
use keeper_core::agents::events::{RunState, StatusContent, CLAIM};
use keeper_core::agents::log::reader::{read_session, ClaimConflict};
use keeper_core::agents::log::{ClaimAction, ClaimBody};
use keeper_core::agents::matrix::{AgentClient, AgentMatrixError, ServerState};
use keeper_core::agents::placement::Placement;
use matrix_sdk::ruma::{OwnedEventId, OwnedRoomId};
use serde_json::Value;
use tokio::sync::watch;
use tokio::time::Instant;

/// The longest a taker waits for one `/sync` round while it settles.
pub const SYNC_ROUND_MAX: Duration = Duration::from_secs(30);

/// How long one claim request may take before it counts as failed.
pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// How far the wall clock may run ahead of the monotonic one since a lease
/// was confirmed before the lease counts as unconfirmed: more means the
/// machine slept (or its VM was paused) while the monotonic clock stood.
const CLOCK_JUMP: Duration = Duration::from_secs(5);

/// How many round trips [`Rtt`] remembers.
const RTT_KEPT: usize = 32;

/// A boxed claim future.
pub type ClaimFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// One session room's claim on the homeserver.
pub trait ClaimPort: Send + Sync {
    /// Send a claim content, once.
    fn send(&self, content: Value) -> ClaimFuture<'_, Result<OwnedEventId, AgentMatrixError>>;
    /// The claim as the server holds it now.
    fn read(&self) -> ClaimFuture<'_, Result<Option<ServerState>, AgentMatrixError>>;
    /// Wait for the next completed `/sync` round; how long it took.
    fn next_sync(&self) -> ClaimFuture<'_, Duration>;
}

/// The real port: one copy's client in one room, with its sync loop's round
/// counter. A session's claim has the state key `""`; a steward duty's
/// creation claim, in the control room, its session's id (R165).
pub struct RoomClaims {
    client: AgentClient,
    room: OwnedRoomId,
    key: String,
    syncs: watch::Receiver<u64>,
}

impl RoomClaims {
    pub fn new(client: AgentClient, room: OwnedRoomId, syncs: watch::Receiver<u64>) -> RoomClaims {
        RoomClaims::keyed(client, room, "", syncs)
    }

    /// The claim under the state key `key` of `room`.
    pub fn keyed(
        client: AgentClient,
        room: OwnedRoomId,
        key: &str,
        syncs: watch::Receiver<u64>,
    ) -> RoomClaims {
        RoomClaims {
            client,
            room,
            key: key.to_owned(),
            syncs,
        }
    }
}

/// `request`, failed when it takes longer than [`REQUEST_TIMEOUT`].
pub(crate) async fn bounded<T>(
    request: impl Future<Output = Result<T, AgentMatrixError>>,
) -> Result<T, AgentMatrixError> {
    tokio::time::timeout(REQUEST_TIMEOUT, request)
        .await
        .unwrap_or_else(|_| {
            Err(AgentMatrixError::Network(format!(
                "no answer within {} s",
                REQUEST_TIMEOUT.as_secs()
            )))
        })
}

impl ClaimPort for RoomClaims {
    fn send(&self, content: Value) -> ClaimFuture<'_, Result<OwnedEventId, AgentMatrixError>> {
        Box::pin(async move {
            bounded(
                self.client
                    .send_state(&self.room, CLAIM, &self.key, &content),
            )
            .await
        })
    }

    fn read(&self) -> ClaimFuture<'_, Result<Option<ServerState>, AgentMatrixError>> {
        Box::pin(
            async move { bounded(self.client.server_state(&self.room, CLAIM, &self.key)).await },
        )
    }

    fn next_sync(&self) -> ClaimFuture<'_, Duration> {
        Box::pin(async move {
            let mut syncs = self.syncs.clone();
            syncs.borrow_and_update();
            let started = Instant::now();
            let _ = syncs.changed().await;
            started.elapsed()
        })
    }
}

/// The round trips this host measured to the homeserver, the last few.
#[derive(Debug, Default)]
pub struct Rtt {
    recent: Mutex<VecDeque<Duration>>,
}

impl Rtt {
    pub fn record(&self, rtt: Duration) {
        let mut recent = self.recent.lock().unwrap_or_else(|p| p.into_inner());
        if recent.len() == RTT_KEPT {
            recent.pop_front();
        }
        recent.push_back(rtt);
    }

    pub fn longest(&self) -> Duration {
        self.recent
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .copied()
            .max()
            .unwrap_or_default()
    }
}

/// This host's estimate of the homeserver's clock: its own wall clock plus
/// the offset its last read-back of its own event showed. Until the first
/// read-back it is only this host's wall clock: uncalibrated.
#[derive(Debug, Default)]
pub struct ServerClock {
    offset_ms: AtomicI64,
    calibrated: AtomicBool,
    /// The wall clock it adds the offset to, when not this host's own: a
    /// value its holder sets, so a test drives both clocks by hand.
    wall: Option<Arc<AtomicU64>>,
}

/// This host's wall clock, ms since the Unix epoch.
pub fn wall_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

impl ServerClock {
    /// A clock whose wall clock reads `wall` (ms since the Unix epoch)
    /// instead of this host's.
    pub fn on_wall(wall: Arc<AtomicU64>) -> ServerClock {
        ServerClock {
            wall: Some(wall),
            ..ServerClock::default()
        }
    }

    /// The server's time now, ms.
    pub fn now(&self) -> u64 {
        let wall = self
            .wall
            .as_ref()
            .map_or_else(wall_ms, |wall| wall.load(Ordering::Relaxed));
        wall.saturating_add_signed(self.offset_ms.load(Ordering::Relaxed))
    }

    /// The server stamped an event `server_ts` that this host sent between
    /// `sent` and `answered` (its wall clock, ms).
    pub fn observe(&self, server_ts: u64, sent: u64, answered: u64) {
        let midpoint = sent / 2 + answered / 2;
        self.offset_ms
            .store(server_ts as i64 - midpoint as i64, Ordering::Relaxed);
        self.calibrated.store(true, Ordering::Relaxed);
    }

    /// Whether a read-back has set the offset: before it, a claim's age by
    /// [`Self::now`] is its age by this host's own clock.
    pub fn calibrated(&self) -> bool {
        self.calibrated.load(Ordering::Relaxed)
    }
}

/// A moment on both of this host's clocks: the monotonic one, which stands
/// still while the machine sleeps or its VM is paused, and the wall clock,
/// which does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Moment {
    pub at: Instant,
    /// ms since the Unix epoch.
    pub wall_ms: u64,
}

impl Moment {
    pub fn now() -> Moment {
        Moment {
            at: Instant::now(),
            wall_ms: wall_ms(),
        }
    }

    /// How long since `earlier` by each clock: `(monotonic, wall)`. A wall
    /// clock set back counts as no time.
    fn since(&self, earlier: Moment) -> (Duration, Duration) {
        (
            self.at.saturating_duration_since(earlier.at),
            Duration::from_millis(self.wall_ms.saturating_sub(earlier.wall_ms)),
        )
    }
}

/// A confirmed claim this host holds.
#[derive(Debug)]
pub struct Lease {
    pub epoch: u64,
    /// The event that acquired it: with `epoch`, the fence key every line carries.
    pub claim_event: OwnedEventId,
    /// The server's time of that event, ms.
    pub server_ts: u64,
    /// The scheduled window the claim names (R56): set before a scheduled
    /// run, and sent with every renewal and the release after it.
    window: Mutex<Option<String>>,
    confirmed: Mutex<Moment>,
    lost: AtomicBool,
}

impl Lease {
    /// A lease acquired by `claim_event`, its claim last confirmed at `at`.
    pub fn new(
        epoch: u64,
        claim_event: OwnedEventId,
        server_ts: u64,
        window: Option<String>,
        at: Moment,
    ) -> Lease {
        Lease {
            epoch,
            claim_event,
            server_ts,
            window: Mutex::new(window),
            confirmed: Mutex::new(at),
            lost: AtomicBool::new(false),
        }
    }

    /// The window the claim names now.
    pub fn window(&self) -> Option<String> {
        self.window
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Name `window` in the claim from its next write on.
    pub fn set_window(&self, window: Option<String>) {
        *self.window.lock().unwrap_or_else(|p| p.into_inner()) = window;
    }

    /// Whether a line may be written under this lease now (NFR-120).
    pub fn may_write(&self) -> bool {
        self.may_write_at(Moment::now())
    }

    /// Whether a line may be written at `now`: not lost, confirmed within
    /// [`claim::STOP_WITHOUT_RENEWAL`] by both clocks, and no sleep since
    /// the confirmation — after one, the next renewal's read decides first.
    fn may_write_at(&self, now: Moment) -> bool {
        !self.must_stop(now) && !self.slept(now)
    }

    /// Whether the holder must stop at `now`: the lease is lost, or no
    /// renewal was confirmed for [`claim::STOP_WITHOUT_RENEWAL`] by either
    /// clock.
    pub fn must_stop(&self, now: Moment) -> bool {
        let confirmed = self.confirmed();
        let (_, wall) = now.since(confirmed);
        self.is_lost()
            || claim::holder_must_stop(confirmed.at.into_std(), now.at.into_std())
            || wall >= claim::STOP_WITHOUT_RENEWAL
    }

    /// Whether a renewal is due at `now`: [`RENEW_EVERY`] by either clock,
    /// or the machine slept since the last confirmation.
    pub fn renewal_due(&self, now: Moment) -> bool {
        let (monotonic, wall) = now.since(self.confirmed());
        monotonic >= RENEW_EVERY || wall >= RENEW_EVERY || self.slept(now)
    }

    /// Whether the wall clock ran ahead of the monotonic one since the last
    /// confirmation: the machine slept, and another host may have taken the
    /// session meanwhile.
    fn slept(&self, now: Moment) -> bool {
        let (monotonic, wall) = now.since(self.confirmed());
        wall.saturating_sub(monotonic) >= CLOCK_JUMP
    }

    fn confirmed(&self) -> Moment {
        *self.confirmed.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// A renewal sent at `at` was accepted.
    pub fn confirm(&self, at: Moment) {
        *self.confirmed.lock().unwrap_or_else(|p| p.into_inner()) = at;
    }

    pub fn lose(&self) {
        self.lost.store(true, Ordering::Relaxed);
    }

    pub fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Relaxed)
    }

    /// The `claim` line of a transition of this lease.
    pub fn line(&self, action: ClaimAction, from_host: Option<String>) -> ClaimBody {
        ClaimBody {
            epoch: self.epoch,
            action,
            from_host,
            claim_event: self.claim_event.to_string(),
            server_ts: claim::rfc3339(self.server_ts),
        }
    }
}

/// What an attempt to acquire ended in.
#[derive(Debug)]
pub enum Acquired {
    /// The claim is this host's; `from_host` held it before.
    Won {
        lease: std::sync::Arc<Lease>,
        from_host: Option<String>,
        /// The window the claim named before this host took it.
        from_window: Option<String>,
        /// Whether that claim was released: its window then ran to the end.
        from_released: bool,
    },
    /// Another host's claim is live.
    HeldElsewhere,
    /// Another host's write won the race: on the read-back, or after the
    /// settle. Nothing is written.
    Yielded,
}

/// A claim the server holds, read; an unreadable one counts as live until
/// it lapses by its server time, at its raw epoch.
struct Current {
    epoch: u64,
    host: Option<String>,
    window: Option<String>,
    released: bool,
    acquirable: bool,
    /// Whether `acquirable` was decided by the server's clock: a claim that
    /// is neither absent, released, nor `me`'s own.
    clocked: bool,
}

fn current(state: Option<&ServerState>, me: &Claimant, server_now: u64) -> Current {
    let Some(state) = state else {
        return Current {
            epoch: 0,
            host: None,
            window: None,
            released: false,
            acquirable: true,
            clocked: false,
        };
    };
    match ServerClaim::read(state) {
        Ok(claim) => Current {
            epoch: claim.content.epoch,
            acquirable: claim::may_acquire(Some(&claim), server_now),
            clocked: !claim.content.released && !claim.is_held_by(me, claim.content.epoch),
            host: Some(claim.content.host),
            window: claim.content.window,
            released: claim.content.released,
        },
        Err(refusal) => {
            tracing::warn!(event = %state.event_id, %refusal, "agents: a session's claim does not read");
            Current {
                epoch: state.content["epoch"].as_u64().unwrap_or(0),
                host: None,
                window: None,
                released: false,
                acquirable: server_now
                    >= u64::from(state.origin_server_ts.get())
                        .saturating_add(claim::TTL.as_millis() as u64),
                clocked: true,
            }
        }
    }
}

/// Time `request` into `rtt`.
async fn timed<T>(rtt: &Rtt, request: impl Future<Output = T>) -> T {
    let started = Instant::now();
    let answer = request.await;
    rtt.record(started.elapsed());
    answer
}

/// Acquire the claim through `port` as `me` (C9, S-05): read the server's
/// claim, write the next epoch when it may be taken, read it back, settle,
/// read it again, and win only if both reads name this host's event.
///
/// Another host's live-looking claim is judged by the server's clock only:
/// until a read-back has calibrated `clock`, it is left alone. A free,
/// released or own claim needs no clock, and taking it calibrates it.
pub async fn acquire(
    port: &dyn ClaimPort,
    me: &Claimant,
    clock: &ServerClock,
    rtt: &Rtt,
    window: Option<String>,
) -> Result<Acquired, AgentMatrixError> {
    acquire_naming(port, me, clock, rtt, |_| window).await
}

/// [`acquire`] for a scheduled session: the claim it writes names the
/// window the claim it takes named (R163). A window in flight stays named
/// from its first write on, through every takeover, until a holder that
/// settled it names its own.
pub async fn acquire_carrying(
    port: &dyn ClaimPort,
    me: &Claimant,
    clock: &ServerClock,
    rtt: &Rtt,
) -> Result<Acquired, AgentMatrixError> {
    acquire_naming(port, me, clock, rtt, |found| found).await
}

async fn acquire_naming(
    port: &dyn ClaimPort,
    me: &Claimant,
    clock: &ServerClock,
    rtt: &Rtt,
    window: impl FnOnce(Option<String>) -> Option<String>,
) -> Result<Acquired, AgentMatrixError> {
    let before = timed(rtt, port.read()).await?;
    let server_now = clock.now();
    let found = current(before.as_ref(), me, server_now);
    if !found.acquirable || (found.clocked && !clock.calibrated()) {
        return Ok(Acquired::HeldElsewhere);
    }
    let epoch = found.epoch.saturating_add(1).max(1);
    let window = window(found.window.clone());
    let content = me.content(epoch, server_now, server_now, false, window.clone());
    let sent_at = Moment::now();
    let sent_wall = sent_at.wall_ms;
    let event = timed(
        rtt,
        port.send(serde_json::to_value(&content).unwrap_or(Value::Null)),
    )
    .await?;
    let answered_wall = wall_ms();
    let first = timed(rtt, port.read()).await?;
    let Some(first) = first.filter(|state| state.event_id == event) else {
        return Ok(Acquired::Yielded);
    };
    let server_ts = u64::from(first.origin_server_ts.get());
    clock.observe(server_ts, sent_wall, answered_wall);

    let settling = Instant::now();
    let round = tokio::time::timeout(SYNC_ROUND_MAX, port.next_sync())
        .await
        .unwrap_or(SYNC_ROUND_MAX);
    tokio::time::sleep_until(settling + claim::settle(rtt.longest(), round)).await;
    let second = timed(rtt, port.read()).await?;
    if second.is_none_or(|state| state.event_id != event) {
        return Ok(Acquired::Yielded);
    }
    Ok(Acquired::Won {
        lease: std::sync::Arc::new(Lease::new(epoch, event, server_ts, window, sent_at)),
        from_host: found.host.filter(|host| *host != me.host),
        from_window: found.window,
        from_released: found.released,
    })
}

/// Renew `lease`: read the server's claim, and when it is still this host's,
/// write it again with a fresh expiry; the lease is confirmed at the send.
/// `Ok(false)` when another host holds it now: the lease is lost.
pub async fn renew(
    port: &dyn ClaimPort,
    me: &Claimant,
    lease: &Lease,
    clock: &ServerClock,
    rtt: &Rtt,
) -> Result<bool, AgentMatrixError> {
    if !holds(port, me, lease, rtt).await? {
        lease.lose();
        return Ok(false);
    }
    let sent_at = Moment::now();
    let server_now = clock.now();
    let content = me.content(
        lease.epoch,
        lease.server_ts,
        server_now,
        false,
        lease.window(),
    );
    timed(
        rtt,
        port.send(serde_json::to_value(&content).unwrap_or(Value::Null)),
    )
    .await?;
    lease.confirm(sent_at);
    Ok(true)
}

/// Hand the claim back with `released: true` when the server still holds
/// it as this host's. The lease is lost either way: nothing more is written.
pub async fn release(
    port: &dyn ClaimPort,
    me: &Claimant,
    lease: &Lease,
    clock: &ServerClock,
    rtt: &Rtt,
) -> Result<bool, AgentMatrixError> {
    lease.lose();
    if !holds(port, me, lease, rtt).await? {
        return Ok(false);
    }
    let content = me.content(
        lease.epoch,
        lease.server_ts,
        clock.now(),
        true,
        lease.window(),
    );
    timed(
        rtt,
        port.send(serde_json::to_value(&content).unwrap_or(Value::Null)),
    )
    .await?;
    Ok(true)
}

async fn holds(
    port: &dyn ClaimPort,
    me: &Claimant,
    lease: &Lease,
    rtt: &Rtt,
) -> Result<bool, AgentMatrixError> {
    let state = timed(rtt, port.read()).await?;
    Ok(state
        .as_ref()
        .and_then(|state| ServerClaim::read(state).ok())
        .is_some_and(|claim| claim.is_held_by(me, lease.epoch)))
}

/// What one tick does about one session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Nothing,
    /// Placement names this host and the claim may be taken.
    Acquire,
    /// The renewal is due.
    Renew,
    /// The lease is lost or unconfirmed for too long: stop writing.
    Stop,
    /// Placement prefers another live host, or the pin names another host,
    /// and no turn runs (C10).
    HandBack,
}

/// The step for one session at `now`: `held` is this host's lease on it,
/// `placement` where it should run, `may_acquire` what the claim the host
/// last saw allows.
pub fn step(
    held: Option<&Lease>,
    placement: &Placement,
    me: &str,
    turn_running: bool,
    may_acquire: bool,
    now: Moment,
) -> Step {
    let placed_here = matches!(placement, Placement::Host(host) if host == me);
    // A pin is explicit (AD-379): a session pinned elsewhere is handed back
    // even when the pinned host cannot serve it yet. A session that waits
    // for no pin keeps its holder.
    let belongs_elsewhere = match placement {
        Placement::Host(host) => host != me,
        Placement::Waiting {
            host: Some(pin), ..
        } => pin != me,
        Placement::Waiting { host: None, .. } => false,
    };
    match held {
        Some(lease) if lease.must_stop(now) => Step::Stop,
        Some(_) if !turn_running && belongs_elsewhere => Step::HandBack,
        Some(lease) if lease.renewal_due(now) => Step::Renew,
        Some(_) => Step::Nothing,
        None if placed_here && may_acquire => Step::Acquire,
        None => Step::Nothing,
    }
}

/// What a conflicted session's status says (S-05).
pub fn conflict_sentence(epoch: u64) -> String {
    format!("Two hosts wrote this session at once (epoch {epoch}). It waits for you.")
}

/// What `status` and `agents list` say of a conflicted session: both events.
pub fn conflict_line(conflict: &ClaimConflict) -> String {
    format!(
        "conflicted at epoch {}: claim events {} and {}",
        conflict.epoch, conflict.events[0], conflict.events[1]
    )
}

/// The first conflict in the session log at `dir`, when there is one.
pub fn conflict_of(dir: &Path) -> Option<ClaimConflict> {
    read_session(dir).conflicts.into_iter().next()
}

/// The status a conflicted session is parked with, over `base`.
pub fn blocked_status(base: StatusContent, conflict: &ClaimConflict) -> StatusContent {
    StatusContent {
        run: RunState::Blocked,
        epoch: conflict.epoch,
        detail: Some(conflict_sentence(conflict.epoch)),
        waiting: None,
        ..base
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use keeper_core::agents::placement::Need;
    use matrix_sdk::ruma::{MilliSecondsSinceUnixEpoch, OwnedUserId, UInt};
    use tokio::sync::Barrier;

    use super::*;

    const T0: u64 = 1_790_000_000_000;

    fn claimant(host: &str) -> Claimant {
        Claimant {
            host: host.to_owned(),
            device: host.to_uppercase(),
            agent: OwnedUserId::try_from("@nixi:example.org").expect("user"),
        }
    }

    /// A homeserver's claim state shared by every port, with the race S-05
    /// names: until both takers have waited a sync round, each one's
    /// read-back answers its own write.
    #[derive(Default)]
    struct Server {
        current: Mutex<Option<ServerState>>,
        sent: Mutex<u32>,
        racing: AtomicBool,
        fail_sends: AtomicBool,
    }

    impl Server {
        fn hold(&self, content: &keeper_core::agents::events::ClaimContent, at: u64) {
            *self.current.lock().expect("lock") = Some(ServerState {
                event_id: OwnedEventId::try_from("$old:example.org").expect("id"),
                sender: content.agent.clone(),
                origin_server_ts: MilliSecondsSinceUnixEpoch(UInt::new(at).expect("ts")),
                content: serde_json::to_value(content).expect("content"),
            });
        }
    }

    struct Port {
        server: Arc<Server>,
        own: Mutex<Option<ServerState>>,
        barrier: Option<Arc<Barrier>>,
        /// Both takers read the old claim before either writes.
        start: Option<Arc<Barrier>>,
        started: AtomicBool,
        /// Sends wait for this many earlier sends, so the race's order is fixed.
        turn: u32,
    }

    impl Port {
        fn new(server: &Arc<Server>, barrier: Option<Arc<Barrier>>, turn: u32) -> Port {
            Port {
                server: Arc::clone(server),
                own: Mutex::new(None),
                start: barrier.as_ref().map(|_| Arc::new(Barrier::new(1))),
                started: AtomicBool::new(false),
                barrier,
                turn,
            }
        }
    }

    impl ClaimPort for Port {
        fn send(&self, content: Value) -> ClaimFuture<'_, Result<OwnedEventId, AgentMatrixError>> {
            Box::pin(async move {
                if self.server.fail_sends.load(Ordering::Relaxed) {
                    return Err(AgentMatrixError::Network("unreachable".to_owned()));
                }
                while *self.server.sent.lock().expect("lock") < self.turn {
                    tokio::task::yield_now().await;
                }
                let mut sent = self.server.sent.lock().expect("lock");
                *sent += 1;
                let event =
                    OwnedEventId::try_from(format!("$claim{sent}:example.org")).expect("event");
                let state = ServerState {
                    event_id: event.clone(),
                    sender: OwnedUserId::try_from("@nixi:example.org").expect("user"),
                    origin_server_ts: MilliSecondsSinceUnixEpoch(
                        UInt::new(T0 + u64::from(*sent)).expect("ts"),
                    ),
                    content,
                };
                *self.own.lock().expect("lock") = Some(state.clone());
                *self.server.current.lock().expect("lock") = Some(state);
                Ok(event)
            })
        }

        fn read(&self) -> ClaimFuture<'_, Result<Option<ServerState>, AgentMatrixError>> {
            Box::pin(async move {
                let current = self.server.current.lock().expect("lock").clone();
                if let Some(start) = &self.start {
                    if !self.started.swap(true, Ordering::Relaxed) {
                        start.wait().await;
                        return Ok(current);
                    }
                }
                let own = self.own.lock().expect("lock").clone();
                if self.server.racing.load(Ordering::Relaxed) && own.is_some() {
                    return Ok(own);
                }
                Ok(self.server.current.lock().expect("lock").clone())
            })
        }

        fn next_sync(&self) -> ClaimFuture<'_, Duration> {
            Box::pin(async move {
                if let Some(barrier) = &self.barrier {
                    if barrier.wait().await.is_leader() {
                        self.server.racing.store(false, Ordering::Relaxed);
                    }
                    barrier.wait().await;
                }
                Duration::from_millis(30)
            })
        }
    }

    fn clock_at(server_ms: u64) -> ServerClock {
        let clock = ServerClock::default();
        clock.observe(server_ms, wall_ms(), wall_ms());
        clock
    }

    #[tokio::test(start_paused = true)]
    async fn a_taker_that_loses_after_the_settle_yields() {
        let server = Arc::new(Server::default());
        server.hold(&claimant("argo").content(4, T0, T0, true, None), T0);
        server.racing.store(true, Ordering::Relaxed);
        let barrier = Arc::new(Barrier::new(2));
        let start = Arc::new(Barrier::new(2));
        let mut a = Port::new(&server, Some(Arc::clone(&barrier)), 0);
        let mut b = Port::new(&server, Some(barrier), 1);
        a.start = Some(Arc::clone(&start));
        b.start = Some(start);
        let clock = clock_at(T0 + 1_000);
        let (rtt_a, rtt_b) = (Rtt::default(), Rtt::default());
        let (me_a, me_b) = (claimant("electra"), claimant("hesperia"));
        let (won_a, won_b) = tokio::join!(
            acquire(&a, &me_a, &clock, &rtt_a, None),
            acquire(&b, &me_b, &clock, &rtt_b, None),
        );
        // Both wrote epoch 5 and both read their own event back.
        for port in [&a, &b] {
            let own = port.own.lock().expect("lock").clone().expect("sent");
            assert_eq!(own.content["epoch"], 5);
        }
        assert!(matches!(won_a.expect("a"), Acquired::Yielded), "A yields");
        match won_b.expect("b") {
            Acquired::Won {
                lease, from_host, ..
            } => {
                assert_eq!(lease.epoch, 5);
                assert_eq!(lease.claim_event.as_str(), "$claim2:example.org");
                assert_eq!(from_host.as_deref(), Some("argo"));
            }
            other => panic!("B proceeds: {other:?}"),
        }
    }

    /// A live claim is left alone; once it lapses the taker wins it, and
    /// learns the window the claim named (R56): a scheduled run it may have
    /// begun.
    #[tokio::test(start_paused = true)]
    async fn a_live_claim_is_left_alone_and_a_read_back_of_another_event_yields() {
        let server = Arc::new(Server::default());
        let electra = claimant("electra");
        let window = "2026-10-05T09:00:00Z".to_owned();
        server.hold(&electra.content(2, T0, T0, false, Some(window.clone())), T0);
        let port = Port::new(&server, None, 0);
        let rtt = Rtt::default();
        let clock = clock_at(T0 + 179_000);
        let held = acquire(&port, &claimant("hesperia"), &clock, &rtt, None).await;
        assert!(matches!(held.expect("read"), Acquired::HeldElsewhere));
        assert_eq!(*server.sent.lock().expect("lock"), 0, "nothing was written");

        let clock = clock_at(T0 + 180_000);
        match acquire(&port, &claimant("hesperia"), &clock, &rtt, None)
            .await
            .expect("acquire")
        {
            Acquired::Won {
                lease,
                from_host,
                from_window,
                ..
            } => {
                assert_eq!(lease.epoch, 3, "epoch + 1");
                assert_eq!(from_host.as_deref(), Some("electra"));
                assert_eq!(from_window, Some(window));
                assert_eq!(lease.window(), None, "the taker names no window of its own");
                assert_eq!(lease.server_ts, T0 + 1);
            }
            other => panic!("{other:?}"),
        }
    }

    /// R163: a scheduled session's taker names the window the lapsed claim
    /// named from its very first write, so a taker dying before it settles
    /// the window still leaves it named for the next.
    #[tokio::test(start_paused = true)]
    async fn a_carrying_taker_names_the_window_from_its_first_write() {
        let server = Arc::new(Server::default());
        let window = "2026-10-05T09:00:00Z".to_owned();
        server.hold(
            &claimant("electra").content(2, T0, T0, false, Some(window.clone())),
            T0,
        );
        let port = Port::new(&server, None, 0);
        let (rtt, clock) = (Rtt::default(), clock_at(T0 + 180_000));
        let Acquired::Won {
            lease, from_window, ..
        } = acquire_carrying(&port, &claimant("hesperia"), &clock, &rtt)
            .await
            .expect("acquire")
        else {
            panic!("won");
        };
        let named =
            ServerClaim::read(&server.current.lock().expect("lock").clone().expect("claim"))
                .expect("claim");
        assert_eq!(named.content.host, "hesperia");
        assert_eq!(named.content.window, Some(window.clone()));
        assert_eq!(lease.window(), Some(window.clone()));
        assert_eq!(from_window, Some(window));
    }

    /// NFR-120: renewals fail from T; the lease stops writing at T + 120 s,
    /// a taker may acquire from T + 180 s by the server's clock, and the
    /// 60 s between them is the margin.
    #[tokio::test(start_paused = true)]
    async fn a_holder_that_cannot_renew_stops_writing_before_another_may_take_over() {
        let server = Arc::new(Server::default());
        let port = Port::new(&server, None, 0);
        let me = claimant("electra");
        let rtt = Rtt::default();
        let clock = clock_at(T0);
        let Acquired::Won { lease, .. } = acquire(&port, &me, &clock, &rtt, None)
            .await
            .expect("acquire")
        else {
            panic!("won");
        };
        // The last confirmed renewal, at T.
        assert!(renew(&port, &me, &lease, &clock, &rtt)
            .await
            .expect("renew"));
        let t = Instant::now();
        let renewed =
            ServerClaim::read(&server.current.lock().expect("lock").clone().expect("claim"))
                .expect("claim");
        server.fail_sends.store(true, Ordering::Relaxed);

        tokio::time::advance(RENEW_EVERY).await;
        assert!(renew(&port, &me, &lease, &clock, &rtt).await.is_err());
        tokio::time::advance(Duration::from_secs(59)).await;
        assert_eq!(t.elapsed(), Duration::from_secs(119));
        assert!(lease.may_write(), "at T + 119 s");
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(!lease.may_write(), "from T + 120 s the writer refuses");
        assert_eq!(
            step(
                Some(&lease),
                &Placement::Host("electra".to_owned()),
                "electra",
                false,
                false,
                Moment::now()
            ),
            Step::Stop
        );

        let stop_at = renewed.origin_server_ts + claim::STOP_WITHOUT_RENEWAL.as_millis() as u64;
        let takeover = renewed.lapses_at();
        assert!(!claim::may_acquire(Some(&renewed), takeover - 1));
        assert!(claim::may_acquire(Some(&renewed), takeover));
        assert_eq!(takeover - stop_at, 60_000, "the margin");
    }

    /// R56: a window set on a held lease is named by the next renewal and
    /// kept by the release, so a taker finds it; the release hands the claim
    /// back and stops the writer.
    #[tokio::test(start_paused = true)]
    async fn a_release_hands_the_claim_back_and_stops_the_writer() {
        let server = Arc::new(Server::default());
        let port = Port::new(&server, None, 0);
        let me = claimant("electra");
        let (rtt, clock) = (Rtt::default(), clock_at(T0));
        let Acquired::Won { lease, .. } = acquire(&port, &me, &clock, &rtt, None).await.expect("a")
        else {
            panic!("won");
        };
        let claim_now = || {
            ServerClaim::read(&server.current.lock().expect("lock").clone().expect("claim"))
                .expect("claim")
        };
        let window = "2026-10-05T09:00:00Z".to_owned();
        lease.set_window(Some(window.clone()));
        assert!(renew(&port, &me, &lease, &clock, &rtt)
            .await
            .expect("renew"));
        assert_eq!(claim_now().content.window, Some(window.clone()));
        assert!(release(&port, &me, &lease, &clock, &rtt)
            .await
            .expect("release"));
        assert!(!lease.may_write());
        let released = claim_now();
        assert!(released.content.released);
        assert_eq!(released.content.window, Some(window));
        assert!(claim::may_acquire(Some(&released), T0));
    }

    /// `now` advanced by `by` on both clocks, as a machine that stays awake.
    fn awake(now: Moment, by: Duration) -> Moment {
        Moment {
            at: now.at + by,
            wall_ms: now.wall_ms + by.as_millis() as u64,
        }
    }

    fn lease_at(now: Moment) -> Lease {
        Lease::new(
            3,
            OwnedEventId::try_from("$c:example.org").expect("id"),
            T0,
            None,
            now,
        )
    }

    #[tokio::test(start_paused = true)]
    async fn the_holder_hands_back_when_idle_and_placement_prefers_another_live_host() {
        let now = Moment::now();
        let lease = lease_at(now);
        let electra = Placement::Host("electra".to_owned());
        let hesperia = Placement::Host("hesperia".to_owned());
        let waiting = Placement::Waiting {
            host: None,
            missing: Vec::new(),
        };
        // electra came back: hesperia hands the session back at its next
        // idle moment, never during a turn.
        assert_eq!(
            step(Some(&lease), &electra, "hesperia", false, false, now),
            Step::HandBack
        );
        assert_eq!(
            step(Some(&lease), &electra, "hesperia", true, false, now),
            Step::Nothing
        );
        assert_eq!(
            step(Some(&lease), &hesperia, "hesperia", false, false, now),
            Step::Nothing
        );
        assert_eq!(
            step(Some(&lease), &waiting, "hesperia", false, false, now),
            Step::Nothing
        );
        // A turn running does not hold off the renewal.
        let later = awake(now, RENEW_EVERY);
        assert_eq!(
            step(Some(&lease), &electra, "hesperia", true, false, later),
            Step::Renew
        );
        // And electra takes what it wins once the claim is released.
        assert_eq!(
            step(None, &electra, "electra", false, true, now),
            Step::Acquire
        );
        assert_eq!(
            step(None, &electra, "electra", false, false, now),
            Step::Nothing
        );
        assert_eq!(
            step(None, &electra, "hesperia", false, true, now),
            Step::Nothing
        );
    }

    /// AD-379: a pin added while another host holds the session hands it
    /// back, even while the pinned host cannot serve it; a session that
    /// waits with no pin, or for a pin to this host, keeps its holder.
    #[tokio::test(start_paused = true)]
    async fn a_holder_hands_back_a_session_pinned_to_another_host() {
        let now = Moment::now();
        let lease = lease_at(now);
        let pinned = |host: &str| Placement::Waiting {
            host: Some(host.to_owned()),
            missing: vec![Need::LiveHost],
        };
        assert_eq!(
            step(
                Some(&lease),
                &pinned("hesperia"),
                "electra",
                false,
                false,
                now
            ),
            Step::HandBack
        );
        assert_eq!(
            step(
                Some(&lease),
                &pinned("hesperia"),
                "electra",
                true,
                false,
                now
            ),
            Step::Nothing,
            "never during a turn"
        );
        assert_eq!(
            step(
                Some(&lease),
                &pinned("electra"),
                "electra",
                false,
                false,
                now
            ),
            Step::Nothing
        );
    }

    /// NFR-120 across a sleep: the monotonic clock stands while the lid is
    /// closed, the wall clock does not. Ten minutes asleep stop the writer;
    /// a shorter sleep leaves the lease unconfirmed until a renewal's read
    /// decides.
    #[tokio::test(start_paused = true)]
    async fn a_lease_asleep_for_ten_minutes_writes_nothing() {
        let now = Moment::now();
        let lease = lease_at(now);
        let here = Placement::Host("hesperia".to_owned());
        assert!(lease.may_write_at(awake(now, Duration::from_secs(119))));

        let woke = Moment {
            at: now.at + Duration::from_secs(10),
            wall_ms: now.wall_ms + 600_000,
        };
        assert!(!lease.may_write_at(woke), "the writer refuses");
        assert_eq!(
            step(Some(&lease), &here, "hesperia", false, false, woke),
            Step::Stop
        );

        let dozed = Moment {
            at: now.at + Duration::from_secs(10),
            wall_ms: now.wall_ms + 40_000,
        };
        assert!(!lease.may_write_at(dozed), "unconfirmed after a sleep");
        assert_eq!(
            step(Some(&lease), &here, "hesperia", false, false, dozed),
            Step::Renew,
            "the renewal reads the claim first"
        );
        lease.confirm(dozed);
        assert!(lease.may_write_at(awake(dozed, Duration::from_secs(1))));
    }

    /// Before a read-back calibrates the clock, another host's claim
    /// that looks lapsed by this host's own clock is left alone; a claim of
    /// this host's own copy is not, and taking it calibrates the clock.
    #[tokio::test(start_paused = true)]
    async fn an_uncalibrated_clock_never_takes_another_hosts_claim() {
        let lapsed = wall_ms() - 200_000;
        let server = Arc::new(Server::default());
        server.hold(
            &claimant("argo").content(4, lapsed, lapsed, false, None),
            lapsed,
        );
        let port = Port::new(&server, None, 0);
        let (rtt, me) = (Rtt::default(), claimant("electra"));
        let clock = ServerClock::default();
        let held = acquire(&port, &me, &clock, &rtt, None).await.expect("a");
        assert!(matches!(held, Acquired::HeldElsewhere), "{held:?}");
        assert_eq!(*server.sent.lock().expect("lock"), 0, "nothing written");

        server.hold(&me.content(4, lapsed, lapsed, false, None), lapsed);
        let own = acquire(&port, &me, &clock, &rtt, None).await.expect("a");
        assert!(matches!(own, Acquired::Won { .. }), "{own:?}");
        assert!(clock.calibrated());

        let lapsed = clock.now() - 200_000;
        server.hold(
            &claimant("argo").content(6, lapsed, lapsed, false, None),
            lapsed,
        );
        let calibrated = acquire(&port, &me, &clock, &rtt, None).await.expect("a");
        assert!(matches!(calibrated, Acquired::Won { .. }), "{calibrated:?}");
    }
}
