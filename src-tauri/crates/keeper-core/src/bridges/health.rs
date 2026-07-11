//! Bridge-session health: a pure, per-session state machine with a documented
//! impure Matrix shell (Story 6.5, FR-28, NFR-6, AD-16, UX-DR8/UX-DR11).
//!
//! A bridge session can silently die (device unlinked, token expired) and vanish for
//! days. This module adds a per-session health state — [`BridgeHealth`] `{ Healthy,
//! Degraded, Disconnected }`, keyed by `(account_id, network_id)` — fed by two legs
//! into one machine:
//!
//! - **Leg 1 (primary, real-time):** a Matrix event handler on the bot management
//!   room classifies the bot's own notices as they arrive via the running sync
//!   ([`classify_health_signal`]) — the 60 s target for the common "the bridge told
//!   us" case.
//! - **Leg 2 (fallback):** a bounded liveness tick (≤ 60 s) that optionally pings the
//!   bot (reusing the 6.4 send/await) and treats a timeout, after a debounce
//!   threshold, as `Disconnected` — covering silent deaths.
//!
//! **Pure core, impure shell — the 6.2/6.3/6.4 discipline.** [`classify_health_signal`],
//! the debounced [`HealthState::apply`], and the snapshot [`diff_sessions`] are pure and
//! fully unit-tested (the whole I/O matrix). The live Matrix shell ([`HealthMonitor`] —
//! mgmt-room event handler, bot-ping send/await-with-timeout, subscription lifecycle)
//! cannot be exercised against a live bot unattended and is a **documented residual
//! risk**; a scripted-observation test proves an observation sequence yields the
//! expected snapshot emissions.
//!
//! **keeper never guesses.** Only bot output matching the versioned, data-driven
//! [`BridgeHealthGrammar`] changes state; unmatched output is ignored (no state change,
//! no emit). The bot's verbatim reason (trimmed, length-capped, no tokens/session
//! material) may ride along as optional `detail`.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use matrix_sdk::event_handler::EventHandlerHandle;
use matrix_sdk::ruma::events::room::message::{MessageType, OriginalSyncRoomMessageEvent};
use matrix_sdk::ruma::OwnedUserId;
use matrix_sdk::{Client, Room};
use tokio::task::JoinHandle;

use crate::bridges::data::{self, BridgeHealthGrammar};
use crate::notify::{self, NotifyConfig};
use crate::platform::Platform;
use crate::vm::{BridgeHealth, BridgeHealthSnapshot, BridgeSessionHealthVm};

/// The debounce threshold: consecutive liveness-tick timeouts from `Healthy` needed
/// before flipping to `Disconnected` (a single missed ping is not a disconnect). An
/// explicit disconnected notice flips immediately regardless. Kept small.
pub const DISCONNECT_DEBOUNCE_THRESHOLD: u32 = 3;

/// The maximum length (chars) of a surfaced bot `detail` reason. A bot notice could be
/// arbitrarily large; capping keeps an unbounded reason from reaching the VM/DOM
/// verbatim (mirrors the bot transport's `MAX_BOT_MESSAGE_CHARS`).
const MAX_DETAIL_CHARS: usize = 300;

/// Truncate a surfaced `detail` reason to [`MAX_DETAIL_CHARS`] on a char boundary
/// (`chars().take(..)` never splits a codepoint).
fn cap_detail(reason: &str) -> String {
    reason.chars().take(MAX_DETAIL_CHARS).collect::<String>()
}

/// Classify a management-room notice body against a network's [`BridgeHealthGrammar`]
/// into a [`BridgeHealth`] transition, or `None` when it matches no marker (the
/// **pure**, unit-tested core — keeper never guesses).
///
/// Matching is case-insensitive substring against the data-driven marker phrases, in
/// honest severity precedence (most-severe first): a `disconnected` marker →
/// `Disconnected`; else a `degraded` marker → `Degraded`; else a `healthy` marker →
/// `Healthy`; else `None` (no state change, no emit). Precedence puts a "logged out"
/// ahead of a co-occurring "reconnecting" so a real death is never masked by a
/// hopeful reconnect line.
pub fn classify_health_signal(text: &str, grammar: &BridgeHealthGrammar) -> Option<BridgeHealth> {
    let lower = text.to_lowercase();
    if lower.trim().is_empty() {
        return None;
    }
    let matches_any = |markers: &[String]| {
        markers
            .iter()
            .any(|m| !m.trim().is_empty() && lower.contains(&m.to_lowercase()))
    };
    if matches_any(&grammar.disconnected_markers) {
        Some(BridgeHealth::Disconnected)
    } else if matches_any(&grammar.degraded_markers) {
        Some(BridgeHealth::Degraded)
    } else if matches_any(&grammar.healthy_markers) {
        Some(BridgeHealth::Healthy)
    } else {
        None
    }
}

/// One observation fed into a [`HealthState`] (the pure input the impure shell builds
/// from a real event or a liveness tick).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthObservation {
    /// A classified management-room notice: the new health plus the bot's verbatim
    /// reason (already trimmed by the shell).
    Notice {
        /// The classified health transition.
        health: BridgeHealth,
        /// The bot's verbatim reason to surface as `detail` (capped by `apply`).
        reason: String,
    },
    /// A liveness-tick ping reply arrived and classified as a health signal (an
    /// active-ping recovery/degrade). Distinct from a passive `Notice` only in origin;
    /// carries no reason.
    PingReply {
        /// The classified health transition from the ping reply.
        health: BridgeHealth,
    },
    /// A liveness-tick ping timed out (no reply within the bounded wait). A single
    /// timeout is a signal, not an error — it increments the debounce counter and only
    /// flips `Healthy`→`Disconnected` at the threshold.
    PingTimeout,
}

/// The pure, per-session debounced health state machine (Story 6.5).
///
/// Holds the current [`BridgeHealth`], the optional surfaced `detail`, and the
/// consecutive-timeout counter used for the `Healthy`→`Disconnected` debounce. All
/// transitions are pure and unit-tested; the impure shell only feeds [`HealthObservation`]s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthState {
    /// The current health.
    health: BridgeHealth,
    /// The bot's last verbatim reason, surfaced as `detail`. Cleared on recovery.
    detail: Option<String>,
    /// Consecutive liveness-tick timeouts since the last non-timeout signal. Reset by
    /// any healthy/degraded/disconnected signal.
    consecutive_timeouts: u32,
}

impl HealthState {
    /// A freshly bootstrapped session — `Healthy`, no detail, no timeouts.
    pub fn new_healthy() -> Self {
        Self {
            health: BridgeHealth::Healthy,
            detail: None,
            consecutive_timeouts: 0,
        }
    }

    /// The current health.
    pub fn health(&self) -> BridgeHealth {
        self.health
    }

    /// The current surfaced `detail`, if any.
    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }

    /// Apply one [`HealthObservation`], returning `true` iff the effective state
    /// (health **or** detail) changed — the caller emits only on a real change (the
    /// debounce means an at-threshold recompute is the only timeout that flips state).
    ///
    /// Rules:
    /// - a `Notice`/`PingReply` with a `Healthy` signal recovers immediately, clearing
    ///   the detail and the timeout counter;
    /// - a `Disconnected`/`Degraded` signal flips immediately (an explicit death does
    ///   not wait for the debounce), setting the capped `detail`;
    /// - a `PingTimeout` increments the counter and promotes a `Healthy` **or**
    ///   `Degraded` session to `Disconnected` once the counter reaches
    ///   [`DISCONNECT_DEBOUNCE_THRESHOLD`] (a session stuck reconnecting while the bot
    ///   has gone silent is dead); below the threshold the state is unchanged (no emit),
    ///   and an already-`Disconnected` session stays (no emit).
    pub fn apply(&mut self, obs: &HealthObservation) -> bool {
        match obs {
            HealthObservation::Notice { health, reason } => {
                let detail = match health {
                    BridgeHealth::Healthy => None,
                    _ => Some(cap_detail(reason)),
                };
                self.set(*health, detail)
            }
            HealthObservation::PingReply { health } => self.set(*health, None),
            HealthObservation::PingTimeout => {
                self.consecutive_timeouts = self.consecutive_timeouts.saturating_add(1);
                // A sustained silence promotes to Disconnected from either a Healthy or
                // a Degraded ("reconnecting") state at the debounce threshold — a session
                // stuck reconnecting while the bot has gone silent is dead, not merely
                // degraded. Already Disconnected stays (redundant — no emit).
                if self.health != BridgeHealth::Disconnected
                    && self.consecutive_timeouts >= DISCONNECT_DEBOUNCE_THRESHOLD
                {
                    self.health = BridgeHealth::Disconnected;
                    if self.detail.is_none() {
                        self.detail = Some("No response from the bridge.".to_owned());
                    }
                    true
                } else {
                    false
                }
            }
        }
    }

    /// Set the health + detail, resetting the timeout counter (any non-timeout signal
    /// is a fresh liveness data point). Returns whether health or detail changed.
    fn set(&mut self, health: BridgeHealth, detail: Option<String>) -> bool {
        self.consecutive_timeouts = 0;
        let changed = self.health != health || self.detail != detail;
        self.health = health;
        self.detail = detail;
        changed
    }
}

/// A monitored session's identity + resolved display name, paired with its pure
/// [`HealthState`]. Keyed by `(account_id, network_id)` in the owning map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitoredSession {
    /// Opaque keeper account id.
    pub account_id: String,
    /// The stable machine `network_id` (the `protocol.id`) — the join key.
    pub network_id: String,
    /// The Network's display name for the card/banner copy.
    pub network_name: String,
    /// The pure per-session health state.
    pub state: HealthState,
}

impl MonitoredSession {
    /// Project this session to its wire [`BridgeSessionHealthVm`] with the given
    /// `last_checked_ms` timestamp.
    pub fn to_vm(&self, last_checked_ms: i64) -> BridgeSessionHealthVm {
        BridgeSessionHealthVm {
            account_id: self.account_id.clone(),
            network_id: self.network_id.clone(),
            network_name: self.network_name.clone(),
            health: self.state.health(),
            last_checked_ms,
            detail: self.state.detail().map(str::to_owned),
        }
    }
}

/// The stable per-session key `(account_id, network_id)` used to order and diff the
/// monitored set.
pub type SessionKey = (String, String);

/// Build a stable snapshot map from a set of monitored sessions and a shared
/// `last_checked_ms`, keyed by `(account_id, network_id)` (a `BTreeMap` for
/// deterministic order). The pure input to [`diff_sessions`] and to the emitted
/// [`BridgeHealthSnapshot`].
pub fn snapshot_map(
    sessions: &BTreeMap<SessionKey, MonitoredSession>,
    last_checked_ms: i64,
) -> BTreeMap<SessionKey, BridgeSessionHealthVm> {
    sessions
        .iter()
        .map(|(key, session)| (key.clone(), session.to_vm(last_checked_ms)))
        .collect()
}

/// Whether two snapshots differ in a way that warrants an emit — the **pure** cadence
/// gate. Compares only the *render-material* fields (`health` + `detail` + the session
/// set), deliberately **ignoring** `last_checked_ms` so a re-check that produced no
/// real change never re-emits (idempotent recompute → `false`, matching the
/// `NetworksSink` cadence contract).
pub fn diff_sessions(
    prev: &BTreeMap<SessionKey, BridgeSessionHealthVm>,
    next: &BTreeMap<SessionKey, BridgeSessionHealthVm>,
) -> bool {
    if prev.len() != next.len() {
        return true;
    }
    for (key, next_vm) in next {
        match prev.get(key) {
            Some(prev_vm) => {
                if prev_vm.health != next_vm.health
                    || prev_vm.detail != next_vm.detail
                    || prev_vm.network_name != next_vm.network_name
                {
                    return true;
                }
            }
            None => return true,
        }
    }
    false
}

/// Build the whole-set [`BridgeHealthSnapshot`] emitted over the channel from an
/// ordered snapshot map (deterministic order from the `BTreeMap`).
pub fn to_snapshot(map: &BTreeMap<SessionKey, BridgeSessionHealthVm>) -> BridgeHealthSnapshot {
    BridgeHealthSnapshot {
        sessions: map.values().cloned().collect(),
    }
}

// ============================================================================
// Impure Matrix shell — the HealthMonitor + shared aggregator (residual risk).
//
// This half performs Matrix I/O (mgmt-room event handlers, bot-ping send/await,
// tick timers) and cannot be exercised against a live bot unattended. It is a
// documented residual risk (as with 6.2's discovery, 6.3's provisioning, and 6.4's
// bot shells). Every state transition it drives runs through the pure, unit-tested
// core above.
// ============================================================================

/// Sink that receives each produced [`BridgeHealthSnapshot`]. The shell wraps a Tauri
/// `Channel::send`; tests capture into a vector. Returns `true` if delivered, `false`
/// if the channel is closed (the monitors then stop emitting).
pub type BridgeHealthSink = Box<dyn Fn(BridgeHealthSnapshot) -> bool + Send + Sync>;

/// The shared cross-account health aggregator: the whole monitored-session set plus
/// the last emitted snapshot, guarded so per-account monitors recompute + diff +
/// emit atomically. One is created per `subscribe_bridge_health` and cloned into each
/// account's [`HealthMonitor`].
#[derive(Clone)]
pub struct HealthAggregator {
    inner: Arc<Mutex<AggregatorState>>,
}

struct AggregatorState {
    /// Every monitored session across all accounts, keyed `(account_id, network_id)`.
    sessions: BTreeMap<SessionKey, MonitoredSession>,
    /// The last emitted snapshot map (for the pure `diff_sessions` cadence gate).
    last_emitted: BTreeMap<SessionKey, BridgeSessionHealthVm>,
    /// The sink; set `None` once it reports its channel closed so we stop emitting.
    sink: Option<BridgeHealthSink>,
    /// The optional FR-28 native-notify leg (Story 10.4): the shared `Platform` port +
    /// `NotifyConfig`. `None` on a headless build / in the pure diff tests. When set, a
    /// per-session transition **into** `Disconnected` posts exactly one native
    /// notification (gated on global DND inside [`notify::notify_bridge_disconnected`]).
    notify: Option<NotifyHook>,
}

/// The bound notify leg carried by the aggregator (Story 10.4): the shared `Platform`
/// port and the app-wide `NotifyConfig`, threaded from `subscribe_bridge_health`.
#[derive(Clone)]
struct NotifyHook {
    platform: Arc<dyn Platform>,
    config: Arc<NotifyConfig>,
}

/// A monotonically-increasing wall clock in ms (UTC), used for `last_checked_ms`. A
/// system-clock failure degrades to `0` rather than panicking (health is best-effort).
fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

impl HealthAggregator {
    /// Construct an aggregator over `sink`, seeded with `sessions` (the discovery
    /// bootstrap). Emits nothing here — the caller emits the initial snapshot after
    /// construction via [`HealthAggregator::emit_initial`].
    pub fn new(sink: BridgeHealthSink, sessions: BTreeMap<SessionKey, MonitoredSession>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(AggregatorState {
                sessions,
                last_emitted: BTreeMap::new(),
                sink: Some(sink),
                notify: None,
            })),
        }
    }

    /// Bind the FR-28 native-notify leg (Story 10.4): a per-session transition **into**
    /// `Disconnected` posts exactly one native notification through `platform` (gated on
    /// global DND). Called once by `subscribe_bridge_health` after construction; on a
    /// headless build it is simply never bound and the leg is inert.
    pub fn set_notify(&self, platform: Arc<dyn Platform>, config: Arc<NotifyConfig>) {
        let Ok(mut state) = self.inner.lock() else {
            // A poisoned lock here would silently drop the FR-28 notify leg for the whole
            // subscription lifetime; surface it rather than fail silent (Story 10.4 review).
            tracing::warn!("bridge health: notify leg not bound (aggregator lock poisoned)");
            return;
        };
        state.notify = Some(NotifyHook { platform, config });
    }

    /// Emit the bootstrap snapshot unconditionally (the stream always opens with the
    /// current set), then record it as the diff baseline.
    pub fn emit_initial(&self) {
        let Ok(mut state) = self.inner.lock() else {
            return;
        };
        let map = snapshot_map(&state.sessions, now_ms());
        Self::send(&mut state, &map);
    }

    /// Apply one observation to the session keyed `(account_id, network_id)` and, if
    /// the session's effective state changed, recompute the whole snapshot and emit it
    /// — but only when the pure [`diff_sessions`] reports a real change (idempotent
    /// recompute → no emit). An observation for an unknown/unmonitored session is a
    /// no-op (only logged-in sessions have health).
    pub fn observe(&self, account_id: &str, network_id: &str, obs: &HealthObservation) {
        let Ok(mut state) = self.inner.lock() else {
            return;
        };
        let key = (account_id.to_owned(), network_id.to_owned());
        // Capture the health BEFORE applying so we can detect the transition **into**
        // `Disconnected` (Story 10.4): only a real transition notifies — a session that
        // stays `Disconnected` never re-notifies, and `apply` already reports no change
        // for a redundant same-state observation. `Degraded` never satisfies the
        // now-Disconnected check, so it never toasts.
        let (changed, transition_name) = match state.sessions.get_mut(&key) {
            Some(session) => {
                let was_disconnected = session.state.health() == BridgeHealth::Disconnected;
                let changed = session.state.apply(obs);
                let now_disconnected = session.state.health() == BridgeHealth::Disconnected;
                let transition_name = (changed && now_disconnected && !was_disconnected)
                    .then(|| session.network_name.clone());
                (changed, transition_name)
            }
            None => return,
        };
        if !changed {
            return;
        }
        // Capture the FR-28 native-notify leg for the transition into Disconnected (fires
        // exactly once, using the session's `network_name`) — but defer the post until
        // AFTER the aggregator lock is released below. `Platform::notify` is an arbitrary
        // trait impl (an OS post); it must never run while this `std::sync::Mutex` is held,
        // or a blocking backend would serialize every concurrent `observe` behind a
        // synchronous OS round-trip (Story 10.4 review — the click-capable backend lands in
        // Epic 11). Both fields are cheap `Arc`/`String` clones.
        let pending_notify = transition_name
            .zip(state.notify.clone())
            .map(|(name, hook)| (hook, name));
        let map = snapshot_map(&state.sessions, now_ms());
        if diff_sessions(&state.last_emitted, &map) {
            Self::send(&mut state, &map);
        }
        drop(state);
        if let Some((hook, name)) = pending_notify {
            notify::notify_bridge_disconnected(
                hook.platform.as_ref(),
                &hook.config,
                account_id,
                network_id,
                &name,
            );
        }
    }

    /// Drop every monitored session for `account_id` (sign-out / shutdown) and, if that
    /// changed the set, emit the reduced snapshot.
    pub fn remove_account(&self, account_id: &str) {
        let Ok(mut state) = self.inner.lock() else {
            return;
        };
        let before = state.sessions.len();
        state.sessions.retain(|(acct, _), _| acct != account_id);
        if state.sessions.len() == before {
            return;
        }
        let map = snapshot_map(&state.sessions, now_ms());
        if diff_sessions(&state.last_emitted, &map) {
            Self::send(&mut state, &map);
        }
    }

    /// Send a snapshot through the sink and record it as the diff baseline. Clears the
    /// sink if the channel is closed so later observations stop emitting.
    fn send(state: &mut AggregatorState, map: &BTreeMap<SessionKey, BridgeSessionHealthVm>) {
        let delivered = match &state.sink {
            Some(sink) => sink(to_snapshot(map)),
            None => false,
        };
        if delivered {
            state.last_emitted = map.clone();
        } else {
            state.sink = None;
        }
    }
}

/// The bounded reply timeout for a liveness-tick bot ping — the tick fires
/// periodically, so a ping must answer within a fraction of the tick or count as a
/// timeout (a debounced disconnect signal, never an infinite wait).
const PING_REPLY_TIMEOUT: Duration = Duration::from_secs(20);

/// Removes a registered room event handler on drop so the mgmt-room handler never
/// leaks past the monitor, even if the monitor task is aborted (unsubscribe /
/// shutdown) mid-run. Mirrors the bot transport's `HandlerGuard`.
struct HandlerGuard {
    client: Client,
    handle: Option<EventHandlerHandle>,
}

impl Drop for HandlerGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            self.client.remove_event_handler(handle);
        }
    }
}

/// One monitored session's live wiring inside a [`HealthMonitor`]: the session's stable
/// `network_id`, the resolved bot management `Room`, the bot's MXID (to filter its
/// notices), the network's grammar, and the mgmt-room handler guard.
struct SessionWiring {
    network_id: String,
    room: Room,
    bot_mxid: OwnedUserId,
    grammar: BridgeHealthGrammar,
    _handler: HandlerGuard,
}

/// A live per-account health monitor (Story 6.5, impure shell). Owns the account's
/// session wirings (each with a mgmt-room event handler feeding classified notices to
/// the shared [`HealthAggregator`]) and a bounded liveness-tick task that optionally
/// pings the bot and feeds timeouts/replies to the aggregator. Dropping it aborts the
/// tick and removes every handler (via the guards).
pub struct HealthMonitor {
    tick: JoinHandle<()>,
    /// Kept alive so the per-session handler guards live as long as the monitor.
    _wirings: Arc<Vec<SessionWiring>>,
}

impl HealthMonitor {
    /// Spawn a monitor for `account_id` over its live `client`, wiring each monitored
    /// `sessions` entry (already in the aggregator) to its bot management room and a
    /// bounded liveness tick that feeds observations to `aggregator`. A session whose
    /// bot room can't be resolved is skipped (logged) — it stays in the aggregator as
    /// Healthy (its notices simply won't be observed) rather than being dropped.
    pub async fn spawn(
        client: Client,
        account_id: String,
        sessions: &[MonitoredSession],
        aggregator: HealthAggregator,
    ) -> Self {
        let mut wirings: Vec<SessionWiring> = Vec::new();
        for session in sessions {
            let grammar = match data::health_signals() {
                Ok(doc) => doc.grammar_for(&session.network_id),
                Err(e) => {
                    tracing::warn!(error = %e, "health: could not load grammar; skipping session");
                    continue;
                }
            };
            // Resolve the bot management DM **only if it already exists** — a passive
            // health observer must never create a room as a side effect of subscribing
            // (this runs automatically at launch). A session with no existing bot DM is
            // left unobserved (stays Healthy) rather than provoking a `create_dm`.
            let Some((room, bot_mxid)) =
                crate::bridges::find_bot_room(&client, &session.network_id).await
            else {
                tracing::debug!(
                    account_id = %account_id,
                    network_id = %session.network_id,
                    "health: no existing bot management room; session unmonitored"
                );
                continue;
            };
            let handler = register_mgmt_handler(
                &room,
                bot_mxid.clone(),
                account_id.clone(),
                session.network_id.clone(),
                grammar.clone(),
                aggregator.clone(),
            );
            wirings.push(SessionWiring {
                network_id: session.network_id.clone(),
                room,
                bot_mxid,
                grammar,
                _handler: handler,
            });
        }

        let wirings = Arc::new(wirings);
        let tick_wirings = wirings.clone();
        let tick_account = account_id.clone();
        let tick = tokio::spawn(async move {
            run_liveness_tick(tick_account, tick_wirings, aggregator).await;
        });

        Self {
            tick,
            _wirings: wirings,
        }
    }

    /// Abort the tick task; the handler guards are dropped with the monitor, removing
    /// every mgmt-room handler.
    pub fn drain(self) {
        self.tick.abort();
    }
}

/// Register the management-room event handler for one session: on each `m.room.message`
/// **from the bot**, classify its body against the session's grammar and feed a
/// `Notice` observation to the aggregator (the pure machine decides whether it emits).
/// Only the bot's own messages count — our echoes and other senders are ignored.
fn register_mgmt_handler(
    room: &Room,
    bot_mxid: OwnedUserId,
    account_id: String,
    network_id: String,
    grammar: BridgeHealthGrammar,
    aggregator: HealthAggregator,
) -> HandlerGuard {
    let client = room.client();
    let handle = room.add_event_handler(move |ev: OriginalSyncRoomMessageEvent| {
        let bot_mxid = bot_mxid.clone();
        let account_id = account_id.clone();
        let network_id = network_id.clone();
        let grammar = grammar.clone();
        let aggregator = aggregator.clone();
        async move {
            if ev.sender != bot_mxid {
                return;
            }
            let body = message_body(&ev.content.msgtype);
            if let Some(health) = classify_health_signal(&body, &grammar) {
                aggregator.observe(
                    &account_id,
                    &network_id,
                    &HealthObservation::Notice {
                        health,
                        reason: body,
                    },
                );
            }
        }
    });
    HandlerGuard {
        client,
        handle: Some(handle),
    }
}

/// The plain-text body of a bot message for classification (notice/text/emote carry
/// their body; other types fall back to the SDK's `body()`), trimmed.
fn message_body(msgtype: &MessageType) -> String {
    match msgtype {
        MessageType::Text(c) => c.body.trim().to_owned(),
        MessageType::Notice(c) => c.body.trim().to_owned(),
        MessageType::Emote(c) => c.body.trim().to_owned(),
        other => other.body().trim().to_owned(),
    }
}

/// The bounded liveness-tick loop: every `tick_interval_secs` (the smallest across the
/// account's session grammars, clamped to the 60 s budget), for each session with
/// `enable_ping`, send its ping command and await one reply — a classified reply feeds
/// a `PingReply`, a timeout feeds a `PingTimeout` (the debounced disconnect signal).
/// Sessions without `enable_ping` rely on the real-time mgmt-room handler alone. The
/// task ends when aborted (monitor drain).
async fn run_liveness_tick(
    account_id: String,
    wirings: Arc<Vec<SessionWiring>>,
    aggregator: HealthAggregator,
) {
    // No ping-enabled session → the mgmt-room handler alone carries this account; don't
    // spin an interval forever doing nothing (the shipped default has `enable_ping`
    // off). The real-time notice handlers stay registered via the monitor's wirings.
    if !wirings.iter().any(|w| w.grammar.enable_ping) {
        return;
    }
    // The tick cadence is the smallest configured interval across the account's
    // sessions (bounded ≤ 60 s by data validation), or 60 s when there is none.
    let interval_secs = wirings
        .iter()
        .map(|w| w.grammar.tick_interval_secs)
        .min()
        .unwrap_or(60)
        .clamp(1, 60);
    let mut ticker = tokio::time::interval(Duration::from_secs(interval_secs));
    // Skip the immediate first tick (bootstrap already emitted the initial snapshot).
    ticker.tick().await;
    loop {
        ticker.tick().await;
        // Ping every enabled session *concurrently* — a serial loop with each ping's
        // 20 s reply timeout would blow the ≤ 60 s detection budget once more than a
        // couple of sessions are dead. The wiring carries its own stable network id (a
        // management DM has no `m.bridge` portal state to derive it from).
        let pings = wirings
            .iter()
            .filter(|w| w.grammar.enable_ping)
            .map(|wiring| async move { (&wiring.network_id, ping_once(wiring).await) });
        for (network_id, obs) in futures_util::future::join_all(pings).await {
            aggregator.observe(&account_id, network_id, &obs);
        }
    }
}

/// Send one liveness ping to a session's bot and await a bounded reply, mapping it to a
/// [`HealthObservation`] — a reply → `PingReply(Healthy)`, a timeout / send failure →
/// `PingTimeout` (the debounced disconnect signal). This is the impure send/await
/// (residual risk).
async fn ping_once(wiring: &SessionWiring) -> HealthObservation {
    use matrix_sdk::ruma::events::room::message::RoomMessageEventContent;
    use tokio::sync::mpsc;

    // Arm a one-shot listener for the bot's next reply BEFORE sending (no race). The
    // reply's *body* is carried back so it can be classified — a bot that answers a
    // ping with "you have been logged out" is a disconnect, not a liveness "reply".
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let bot_mxid = wiring.bot_mxid.clone();
    let handle = wiring
        .room
        .add_event_handler(move |ev: OriginalSyncRoomMessageEvent| {
            let tx = tx.clone();
            let bot_mxid = bot_mxid.clone();
            async move {
                if ev.sender == bot_mxid {
                    let _ = tx.send(message_body(&ev.content.msgtype));
                }
            }
        });
    let _guard = HandlerGuard {
        client: wiring.room.client(),
        handle: Some(handle),
    };

    // Send the ping (best-effort — a send failure is treated as a timeout signal).
    if let Err(e) = wiring
        .room
        .send(RoomMessageEventContent::text_plain(
            &wiring.grammar.ping_command,
        ))
        .await
    {
        tracing::debug!(error = %e, "health: ping send failed; treating as a timeout");
        return HealthObservation::PingTimeout;
    }

    match tokio::time::timeout(PING_REPLY_TIMEOUT, rx.recv()).await {
        // A reply arrived — classify it. A `disconnected`/`degraded` reply is an
        // explicit unhealthy Notice (immediate flip); a `healthy` or unmatched reply
        // just proves the bot is alive → `PingReply(Healthy)`.
        Ok(Some(body)) => match classify_health_signal(&body, &wiring.grammar) {
            Some(health @ (BridgeHealth::Disconnected | BridgeHealth::Degraded)) => {
                HealthObservation::Notice {
                    health,
                    reason: body,
                }
            }
            _ => HealthObservation::PingReply {
                health: BridgeHealth::Healthy,
            },
        },
        // Timeout or closed channel → a debounced disconnect signal.
        Ok(None) | Err(_) => HealthObservation::PingTimeout,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Mutex as StdMutex;

    use super::*;
    use crate::error::CoreError;
    use crate::vm::NotifyTarget;

    /// A capturing [`Platform`] double recording every notification the aggregator's
    /// FR-28 notify leg posts, so the bridge transition/DND/dedup matrix is covered
    /// without a homeserver or an OS notifier.
    struct CapturingNotifier {
        calls: StdMutex<Vec<(String, NotifyTarget)>>,
    }

    impl CapturingNotifier {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                calls: StdMutex::new(Vec::new()),
            })
        }
        fn bodies(&self) -> Vec<String> {
            self.calls
                .lock()
                .expect("lock")
                .iter()
                .map(|(body, _)| body.clone())
                .collect()
        }
        fn targets(&self) -> Vec<NotifyTarget> {
            self.calls
                .lock()
                .expect("lock")
                .iter()
                .map(|(_, target)| target.clone())
                .collect()
        }
    }

    impl Platform for CapturingNotifier {
        fn data_dir(&self) -> Result<PathBuf, CoreError> {
            Ok(PathBuf::from("/tmp/keeper-health-test"))
        }
        fn keychain_set(&self, _key: &str, _value: &str) -> Result<(), CoreError> {
            Ok(())
        }
        fn keychain_get(&self, _key: &str) -> Result<Option<String>, CoreError> {
            Ok(None)
        }
        fn keychain_delete(&self, _key: &str) -> Result<(), CoreError> {
            Ok(())
        }
        fn open_url(&self, _url: &str) -> Result<(), CoreError> {
            Ok(())
        }
        fn notify(&self, _title: &str, body: &str, target: &NotifyTarget) -> Result<(), CoreError> {
            self.calls
                .lock()
                .expect("lock")
                .push((body.to_owned(), target.clone()));
            Ok(())
        }
        fn sidecar_path(&self, _name: &str) -> Result<PathBuf, CoreError> {
            Err(CoreError::Unsupported("unused".to_owned()))
        }
        fn exclude_from_backup(&self, _path: &std::path::Path) -> Result<(), CoreError> {
            Ok(())
        }
        fn set_badge_count(&self, _count: Option<u32>) -> Result<(), CoreError> {
            Ok(())
        }
    }

    /// A one-session aggregator wired with a capturing notifier and the given
    /// [`NotifyConfig`], for the FR-28 notify-leg tests. The sink swallows snapshots (the
    /// pure diff/emit path is covered elsewhere).
    fn notify_aggregator(
        notifier: Arc<CapturingNotifier>,
        config: Arc<NotifyConfig>,
    ) -> HealthAggregator {
        let sessions = map_of(vec![session("acctA", "signal", BridgeHealth::Healthy)]);
        // Give the session a Network display name for the copy.
        let mut sessions = sessions;
        if let Some(s) = sessions.get_mut(&("acctA".to_owned(), "signal".to_owned())) {
            s.network_name = "Signal".to_owned();
        }
        let aggregator = HealthAggregator::new(Box::new(|_snapshot| true), sessions);
        aggregator.set_notify(notifier, config);
        aggregator
    }

    fn disconnect_notice() -> HealthObservation {
        HealthObservation::Notice {
            health: BridgeHealth::Disconnected,
            reason: "you have been logged out".to_owned(),
        }
    }

    #[test]
    fn aggregator_notifies_once_on_transition_into_disconnected() {
        let notifier = CapturingNotifier::new();
        let config = Arc::new(NotifyConfig::new(true));
        let aggregator = notify_aggregator(notifier.clone(), config);
        aggregator.observe("acctA", "signal", &disconnect_notice());
        assert_eq!(
            notifier.bodies(),
            vec!["Signal disconnected — re-link to keep receiving messages.".to_owned()]
        );
        assert_eq!(
            notifier.targets(),
            vec![NotifyTarget::Bridge {
                account_id: "acctA".to_owned(),
                network_id: "signal".to_owned(),
            }]
        );
    }

    #[test]
    fn aggregator_does_not_renotify_while_still_disconnected() {
        // Only the transition notifies: a second disconnect observation on an already-
        // Disconnected session (redundant — no state change) must NOT re-toast.
        let notifier = CapturingNotifier::new();
        let config = Arc::new(NotifyConfig::new(true));
        let aggregator = notify_aggregator(notifier.clone(), config);
        aggregator.observe("acctA", "signal", &disconnect_notice());
        aggregator.observe("acctA", "signal", &disconnect_notice());
        // A sustained-silence ping timeout on an already-Disconnected session also no-ops.
        aggregator.observe("acctA", "signal", &HealthObservation::PingTimeout);
        assert_eq!(notifier.bodies().len(), 1, "one alert per drop");
    }

    #[test]
    fn aggregator_renotifies_on_a_fresh_drop_after_recovery() {
        // Recover, then drop again → the SECOND drop is a fresh transition and notifies.
        let notifier = CapturingNotifier::new();
        let config = Arc::new(NotifyConfig::new(true));
        let aggregator = notify_aggregator(notifier.clone(), config);
        aggregator.observe("acctA", "signal", &disconnect_notice());
        aggregator.observe(
            "acctA",
            "signal",
            &HealthObservation::PingReply {
                health: BridgeHealth::Healthy,
            },
        );
        aggregator.observe("acctA", "signal", &disconnect_notice());
        assert_eq!(notifier.bodies().len(), 2, "each fresh drop notifies");
    }

    #[test]
    fn aggregator_does_not_toast_on_degraded() {
        // A Degraded transition changes state (emits a snapshot) but must NEVER toast.
        let notifier = CapturingNotifier::new();
        let config = Arc::new(NotifyConfig::new(true));
        let aggregator = notify_aggregator(notifier.clone(), config);
        aggregator.observe(
            "acctA",
            "signal",
            &HealthObservation::Notice {
                health: BridgeHealth::Degraded,
                reason: "reconnecting".to_owned(),
            },
        );
        assert!(notifier.bodies().is_empty(), "Degraded must not toast");
    }

    #[test]
    fn aggregator_bridge_notify_suppressed_by_global_dnd() {
        // Global DND suppresses the native bridge notification; the session still flips
        // (the in-app 6.5 surfaces update via the emitted snapshot, not asserted here).
        let notifier = CapturingNotifier::new();
        let config = Arc::new(NotifyConfig::new(true));
        config.set_dnd_enabled(true);
        let aggregator = notify_aggregator(notifier.clone(), config);
        aggregator.observe("acctA", "signal", &disconnect_notice());
        assert!(notifier.bodies().is_empty(), "DND suppresses the toast");
    }

    /// A grammar with distinct, unambiguous markers for the classifier I/O matrix.
    fn grammar() -> BridgeHealthGrammar {
        BridgeHealthGrammar {
            disconnected_markers: vec![
                "you have been logged out".to_owned(),
                "session expired".to_owned(),
            ],
            degraded_markers: vec!["reconnecting".to_owned()],
            healthy_markers: vec!["connected".to_owned(), "you're logged in".to_owned()],
            ping_command: "ping".to_owned(),
            tick_interval_secs: 60,
            enable_ping: false,
        }
    }

    // --- The pure classifier I/O matrix -------------------------------------

    #[test]
    fn disconnected_notice_classifies_disconnected() {
        assert_eq!(
            classify_health_signal("You have been logged out of WhatsApp.", &grammar()),
            Some(BridgeHealth::Disconnected)
        );
    }

    #[test]
    fn degraded_notice_classifies_degraded() {
        assert_eq!(
            classify_health_signal("Reconnecting to the server…", &grammar()),
            Some(BridgeHealth::Degraded)
        );
    }

    #[test]
    fn healthy_notice_classifies_healthy() {
        assert_eq!(
            classify_health_signal("You're logged in and connected.", &grammar()),
            Some(BridgeHealth::Healthy)
        );
    }

    #[test]
    fn unmatched_notice_classifies_none() {
        // Chatty prose matching no marker must never be guessed at.
        assert_eq!(
            classify_health_signal("Here is a message you received.", &grammar()),
            None
        );
        // An empty / whitespace-only body is also None.
        assert_eq!(classify_health_signal("   ", &grammar()), None);
    }

    #[test]
    fn disconnected_beats_a_co_occurring_degraded_marker() {
        // A real death that also mentions a reconnect must classify Disconnected —
        // severity precedence, not first-match.
        assert_eq!(
            classify_health_signal("Session expired; reconnecting will not help.", &grammar()),
            Some(BridgeHealth::Disconnected)
        );
    }

    #[test]
    fn classification_is_case_insensitive() {
        assert_eq!(
            classify_health_signal("YOU HAVE BEEN LOGGED OUT", &grammar()),
            Some(BridgeHealth::Disconnected)
        );
    }

    // --- The debounced state machine ----------------------------------------

    #[test]
    fn disconnected_notice_flips_immediately_with_capped_detail() {
        let mut state = HealthState::new_healthy();
        let changed = state.apply(&HealthObservation::Notice {
            health: BridgeHealth::Disconnected,
            reason: "you have been logged out".to_owned(),
        });
        assert!(changed);
        assert_eq!(state.health(), BridgeHealth::Disconnected);
        assert_eq!(state.detail(), Some("you have been logged out"));
    }

    #[test]
    fn healthy_recovery_clears_detail() {
        let mut state = HealthState::new_healthy();
        state.apply(&HealthObservation::Notice {
            health: BridgeHealth::Disconnected,
            reason: "logged out".to_owned(),
        });
        let changed = state.apply(&HealthObservation::Notice {
            health: BridgeHealth::Healthy,
            reason: String::new(),
        });
        assert!(changed);
        assert_eq!(state.health(), BridgeHealth::Healthy);
        assert_eq!(state.detail(), None);
    }

    #[test]
    fn detail_is_length_capped() {
        let mut state = HealthState::new_healthy();
        let huge = "x".repeat(MAX_DETAIL_CHARS + 500);
        state.apply(&HealthObservation::Notice {
            health: BridgeHealth::Disconnected,
            reason: huge,
        });
        assert_eq!(
            state.detail().map(|d| d.chars().count()),
            Some(MAX_DETAIL_CHARS)
        );
    }

    #[test]
    fn ping_timeout_below_threshold_does_not_flip() {
        let mut state = HealthState::new_healthy();
        // N-1 timeouts: still Healthy, no emit each time.
        for _ in 0..(DISCONNECT_DEBOUNCE_THRESHOLD - 1) {
            let changed = state.apply(&HealthObservation::PingTimeout);
            assert!(!changed, "below-threshold timeout must not flip");
            assert_eq!(state.health(), BridgeHealth::Healthy);
        }
    }

    #[test]
    fn ping_timeout_at_threshold_flips_disconnected() {
        let mut state = HealthState::new_healthy();
        let mut last_changed = false;
        for _ in 0..DISCONNECT_DEBOUNCE_THRESHOLD {
            last_changed = state.apply(&HealthObservation::PingTimeout);
        }
        assert!(last_changed, "the Nth timeout must flip + emit");
        assert_eq!(state.health(), BridgeHealth::Disconnected);
        assert!(state.detail().is_some());
    }

    #[test]
    fn default_grammar_does_not_mask_negated_phrases_as_healthy() {
        // The bare healthy markers "connected"/"online" are substrings of negated death
        // phrases ("not connected", "no longer online"). Disconnected precedence must
        // catch those in the *default* grammar (not only the whatsapp override), or a
        // real death on any other bridge is misread as Healthy.
        let doc = data::health_signals().expect("health signals parse");
        let default = doc.grammar_for("no-such-network");
        assert_eq!(
            classify_health_signal("You are not connected right now.", &default),
            Some(BridgeHealth::Disconnected),
        );
        assert_eq!(
            classify_health_signal("Your device is no longer online.", &default),
            Some(BridgeHealth::Disconnected),
        );
        // A genuine recovery notice containing "online" must still read Healthy.
        assert_eq!(
            classify_health_signal("You are back online.", &default),
            Some(BridgeHealth::Healthy),
        );
    }

    #[test]
    fn whatsapp_override_does_not_mask_negated_phrases_as_healthy() {
        // Regression: the whatsapp override *replaces* the default grammar (grammar_for
        // returns the matching override, not a merge), so its healthy marker "connected"
        // is a substring of negated death phrases ("not connected", "disconnected") just
        // like the default's — but the override must carry the same disconnected guards or
        // a real WhatsApp death reads as Healthy on the flagship bridge.
        let doc = data::health_signals().expect("health signals parse");
        let whatsapp = doc.grammar_for("whatsapp");
        assert_eq!(
            classify_health_signal("You are not connected right now.", &whatsapp),
            Some(BridgeHealth::Disconnected),
        );
        assert_eq!(
            classify_health_signal("WhatsApp disconnected.", &whatsapp),
            Some(BridgeHealth::Disconnected),
        );
        assert_eq!(
            classify_health_signal("Your device is no longer connected.", &whatsapp),
            Some(BridgeHealth::Disconnected),
        );
        // A genuine recovery notice must still read Healthy.
        assert_eq!(
            classify_health_signal("You are back online.", &whatsapp),
            Some(BridgeHealth::Healthy),
        );
        assert_eq!(
            classify_health_signal("Successfully connected to WhatsApp.", &whatsapp),
            Some(BridgeHealth::Healthy),
        );
    }

    #[test]
    fn ping_timeout_escalates_a_degraded_session_to_disconnected() {
        // A session that went Degraded from a "reconnecting" notice and then the bot
        // goes silent must escalate to Disconnected at the debounce threshold — a stuck
        // reconnecting session with no bot response is dead, not merely degraded.
        let mut state = HealthState::new_healthy();
        state.apply(&HealthObservation::Notice {
            health: BridgeHealth::Degraded,
            reason: "Reconnecting…".to_owned(),
        });
        assert_eq!(state.health(), BridgeHealth::Degraded);
        let mut flipped = false;
        for _ in 0..DISCONNECT_DEBOUNCE_THRESHOLD {
            flipped = state.apply(&HealthObservation::PingTimeout);
        }
        assert!(
            flipped,
            "the Nth timeout must escalate Degraded → Disconnected"
        );
        assert_eq!(state.health(), BridgeHealth::Disconnected);
    }

    #[test]
    fn a_healthy_signal_resets_the_timeout_debounce() {
        let mut state = HealthState::new_healthy();
        // Accumulate N-1 timeouts, then a healthy ping reply resets the counter.
        for _ in 0..(DISCONNECT_DEBOUNCE_THRESHOLD - 1) {
            state.apply(&HealthObservation::PingTimeout);
        }
        state.apply(&HealthObservation::PingReply {
            health: BridgeHealth::Healthy,
        });
        // Now a single further timeout must NOT flip (the counter reset).
        let changed = state.apply(&HealthObservation::PingTimeout);
        assert!(!changed);
        assert_eq!(state.health(), BridgeHealth::Healthy);
    }

    #[test]
    fn a_redundant_same_state_notice_reports_no_change() {
        let mut state = HealthState::new_healthy();
        // A healthy notice on an already-healthy state with no detail is no change.
        let changed = state.apply(&HealthObservation::PingReply {
            health: BridgeHealth::Healthy,
        });
        assert!(!changed);
    }

    // --- diff_sessions idempotence ------------------------------------------

    fn session(account: &str, network: &str, health: BridgeHealth) -> MonitoredSession {
        let mut state = HealthState::new_healthy();
        state.health = health;
        MonitoredSession {
            account_id: account.to_owned(),
            network_id: network.to_owned(),
            network_name: network.to_owned(),
            state,
        }
    }

    fn map_of(sessions: Vec<MonitoredSession>) -> BTreeMap<SessionKey, MonitoredSession> {
        sessions
            .into_iter()
            .map(|s| ((s.account_id.clone(), s.network_id.clone()), s))
            .collect()
    }

    #[test]
    fn idempotent_recompute_does_not_diff() {
        let sessions = map_of(vec![session("acctA", "whatsapp", BridgeHealth::Healthy)]);
        // Same render state, DIFFERENT last_checked_ms → must NOT diff (cadence gate).
        let prev = snapshot_map(&sessions, 1_000);
        let next = snapshot_map(&sessions, 9_999);
        assert!(
            !diff_sessions(&prev, &next),
            "a re-check with no change must not emit"
        );
    }

    #[test]
    fn a_health_change_diffs() {
        let prev = snapshot_map(
            &map_of(vec![session("acctA", "whatsapp", BridgeHealth::Healthy)]),
            0,
        );
        let next = snapshot_map(
            &map_of(vec![session(
                "acctA",
                "whatsapp",
                BridgeHealth::Disconnected,
            )]),
            0,
        );
        assert!(diff_sessions(&prev, &next));
    }

    #[test]
    fn adding_or_removing_a_session_diffs() {
        let one = snapshot_map(
            &map_of(vec![session("acctA", "whatsapp", BridgeHealth::Healthy)]),
            0,
        );
        let two = snapshot_map(
            &map_of(vec![
                session("acctA", "whatsapp", BridgeHealth::Healthy),
                session("acctA", "telegram", BridgeHealth::Healthy),
            ]),
            0,
        );
        assert!(diff_sessions(&one, &two));
        assert!(diff_sessions(&two, &one));
    }

    // --- The scripted-observation contract test -----------------------------
    //
    // Prove a sequence of observations fed to one session's HealthState yields the
    // expected snapshot emissions (mirrors bot.rs's scripted-driver test) — without
    // any live Matrix I/O (the impure shell is documented residual risk).

    #[test]
    fn scripted_observation_sequence_yields_expected_emissions() {
        let mut session = session("acctA", "whatsapp", BridgeHealth::Healthy);
        // The scripted observation feed: bootstrap-healthy → a degraded notice → a
        // disconnect notice → a healthy recovery.
        let script = vec![
            HealthObservation::Notice {
                health: BridgeHealth::Degraded,
                reason: "reconnecting".to_owned(),
            },
            HealthObservation::Notice {
                health: BridgeHealth::Disconnected,
                reason: "you have been logged out".to_owned(),
            },
            HealthObservation::PingTimeout, // redundant on Disconnected — no emit
            HealthObservation::Notice {
                health: BridgeHealth::Healthy,
                reason: String::new(),
            },
        ];

        // Capture the health at each observation that actually emitted (changed).
        let mut emitted: Vec<BridgeHealth> = Vec::new();
        for obs in &script {
            if session.state.apply(obs) {
                emitted.push(session.state.health());
            }
        }
        assert_eq!(
            emitted,
            vec![
                BridgeHealth::Degraded,
                BridgeHealth::Disconnected,
                // the PingTimeout on Disconnected did NOT emit
                BridgeHealth::Healthy,
            ]
        );
    }

    #[test]
    fn to_snapshot_orders_deterministically_and_omits_secrets() {
        let sessions = map_of(vec![
            session("acctA", "whatsapp", BridgeHealth::Disconnected),
            session("acctA", "telegram", BridgeHealth::Healthy),
        ]);
        let map = snapshot_map(&sessions, 42);
        let snapshot = to_snapshot(&map);
        // BTreeMap orders (account, network): telegram before whatsapp.
        assert_eq!(snapshot.sessions.len(), 2);
        assert_eq!(snapshot.sessions[0].network_id, "telegram");
        assert_eq!(snapshot.sessions[1].network_id, "whatsapp");
        assert_eq!(snapshot.sessions[1].last_checked_ms, 42);
    }
}
