//! The docked note's focus on its way to the proxy (AD-382, R41).
//!
//! While the notes view's dock is open, every change of the note in front
//! of the person reaches [`FocusLane::change`]; closing the dock reaches
//! [`FocusLane::close`]. One lane per (account, proxy room) decides what
//! the host is told, and the promise it keeps is R41's: nothing while the
//! dock is closed.
//!
//! - **Order.** Every call carries the webview's sequence number and is
//!   registered before anything is awaited, so a focus that arrives after a
//!   later close is dropped, however the two calls were scheduled. The note
//!   is named only once it has been still for [`FOCUS_STILLNESS`], and a
//!   close or a newer change while it is being named makes that naming
//!   stale. Sends are serialized per lane: a clear waits for the focus
//!   already on the wire, so the host receives them in that order.
//! - **Loss.** The host forgets a focus it has not heard for [`FOCUS_TTL`];
//!   the open dock says its note again every [`FOCUS_HEARTBEAT`], so a quit,
//!   a reload or a crash that never sent the clear stops being stated within
//!   that bound. A clear that could not be sent is owed and tried again
//!   until it lands, a newer focus replaces it, or [`FOCUS_TTL`] made it
//!   moot.

use std::collections::HashMap;
use std::future::Future;
use std::hash::Hash;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use crate::agents::events::Focus;

/// How long the docked note must stay still before its focus is sent: one
/// scope event per second of stillness (AD-382).
pub const FOCUS_STILLNESS: Duration = Duration::from_secs(1);

/// How often the open dock says the note in front of the person again while
/// it stays on it (the webview's `FOCUS_HEARTBEAT_MS`).
pub const FOCUS_HEARTBEAT: Duration = Duration::from_secs(5 * 60);

/// A focus said again at least this long after it was last sent goes out
/// again; shorter than [`FOCUS_HEARTBEAT`], so every heartbeat does.
pub const FOCUS_RESEND: Duration = Duration::from_secs(4 * 60);

/// How long a host states a focus it has not heard again (R41): three
/// heartbeats.
pub const FOCUS_TTL: Duration = Duration::from_secs(15 * 60);

/// The first wait before an owed clear is tried again; it doubles up to
/// [`CLEAR_RETRY_MOST`].
const CLEAR_RETRY_FIRST: Duration = Duration::from_secs(5);
const CLEAR_RETRY_MOST: Duration = Duration::from_secs(60);

/// A send the lane decided on, valid while no later send or close was
/// decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token(u64);

/// What [`FocusDebounce::close`] asks the caller to send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnClose {
    /// The host was told of a focus (or a clear is owed): one event saying
    /// there is none now.
    Clear(Token),
    Nothing,
}

/// A change that has been still long enough: name it, then
/// [`FocusDebounce::commit`] what it names under `generation`.
pub struct Due<R> {
    pub generation: u64,
    pub request: R,
}

/// What goes on the wire: a focus, or that there is none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    pub focus: Option<Focus>,
    pub token: Token,
}

/// When the docked note's focus goes out: after [`FOCUS_STILLNESS`] with no
/// change, only when it differs from what the host was told (or the same
/// one after [`FOCUS_RESEND`]), and never after the dock closed. `R` is the
/// change as the webview reported it, named only once it is due.
pub struct FocusDebounce<R> {
    /// The newest sequence number seen; an older call is stale.
    seq: u64,
    pending: Option<(R, Instant)>,
    /// Moves on every change and close: a naming begun before is stale.
    generation: u64,
    /// What the host was last told and not told otherwise since, and when.
    told: Option<(Focus, Instant)>,
    /// Moves on every send decided and every close.
    sends: u64,
    owed_clear: bool,
    driving: bool,
}

impl<R> Default for FocusDebounce<R> {
    fn default() -> Self {
        FocusDebounce {
            seq: 0,
            pending: None,
            generation: 0,
            told: None,
            sends: 0,
            owed_clear: false,
            driving: false,
        }
    }
}

impl<R> FocusDebounce<R> {
    /// The docked note or its caret changed at `now` (call `seq`); `false`
    /// when a later call was already seen.
    pub fn change(&mut self, seq: u64, request: R, now: Instant) -> bool {
        if seq <= self.seq {
            return false;
        }
        self.seq = seq;
        self.generation += 1;
        self.pending = Some((request, now));
        true
    }

    /// The dock closed (call `seq`; `None` on quit, which outranks every
    /// call): what waits is dropped, and the host is told there is no focus
    /// when it was told of one or a clear is owed. `None` when a later call
    /// was already seen.
    pub fn close(&mut self, seq: Option<u64>) -> Option<OnClose> {
        if let Some(seq) = seq {
            if seq <= self.seq {
                return None;
            }
            self.seq = seq;
        }
        self.pending = None;
        self.generation += 1;
        let told = self.told.take().is_some();
        if told || self.owed_clear {
            self.owed_clear = false;
            self.sends += 1;
            return Some(OnClose::Clear(Token(self.sends)));
        }
        Some(OnClose::Nothing)
    }

    /// Drop a change that cannot be sent (its room was refused).
    pub fn forget_pending(&mut self) {
        self.pending = None;
    }

    /// When the waiting change could be due, if one waits.
    pub fn next_due(&self) -> Option<Instant> {
        self.pending.as_ref().map(|(_, at)| *at + FOCUS_STILLNESS)
    }

    /// The waiting change, once it has been still for [`FOCUS_STILLNESS`]
    /// at `now`.
    pub fn due(&mut self, now: Instant) -> Option<Due<R>> {
        let (_, at) = self.pending.as_ref()?;
        if now.saturating_duration_since(*at) < FOCUS_STILLNESS {
            return None;
        }
        let (request, _) = self.pending.take()?;
        Some(Due {
            generation: self.generation,
            request,
        })
    }

    /// What `generation`'s change named: what to send at `now`, if anything.
    /// Nothing when a change or a close came since, or when the host was
    /// told the same less than [`FOCUS_RESEND`] ago.
    pub fn commit(
        &mut self,
        generation: u64,
        named: Option<Focus>,
        now: Instant,
    ) -> Option<Outgoing> {
        if generation != self.generation {
            return None;
        }
        let same = self.told.as_ref().map(|(focus, _)| focus) == named.as_ref();
        let fresh = self
            .told
            .as_ref()
            .is_none_or(|(_, at)| now.saturating_duration_since(*at) < FOCUS_RESEND);
        if same && fresh {
            return None;
        }
        self.told = named.clone().map(|focus| (focus, now));
        self.owed_clear = false;
        self.sends += 1;
        Some(Outgoing {
            focus: named,
            token: Token(self.sends),
        })
    }

    /// Whether `token`'s send is still the newest decided.
    pub fn current(&self, token: Token) -> bool {
        self.sends == token.0
    }

    /// `token`'s clear failed: it is owed while it is the newest send.
    pub fn owe_clear(&mut self, token: Token) -> bool {
        if self.current(token) {
            self.owed_clear = true;
        }
        self.owed_clear && self.current(token)
    }

    /// Whether `token`'s clear is still owed.
    pub fn owes_clear(&self, token: Token) -> bool {
        self.owed_clear && self.current(token)
    }

    /// `token`'s clear landed.
    pub fn cleared(&mut self, token: Token) {
        if self.current(token) {
            self.owed_clear = false;
        }
    }

    /// What the host was last told: a scope the person sets carries it.
    pub fn last(&self) -> Option<&Focus> {
        self.told.as_ref().map(|(focus, _)| focus)
    }
}

/// The note a due change names, worked out once it is due.
pub type Named = Pin<Box<dyn Future<Output = Option<Focus>> + Send>>;
/// A change as the webview reported it, named only once it is due.
pub type Namer = Box<dyn FnOnce() -> Named + Send>;
/// A send's answer: the error as a sentence.
pub type SendFuture<'a> = Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

/// Where a lane's focus goes: the proxy room, as a scope event.
pub trait FocusPort: Send + Sync {
    /// Tell the host the note in front of the person, or that there is none.
    fn send(&self, focus: Option<Focus>) -> SendFuture<'_>;
}

/// One (account, proxy room)'s focus: its [`FocusDebounce`], its room once
/// admitted, and the lock its sends take in turn.
pub struct FocusLane {
    state: Mutex<FocusDebounce<Namer>>,
    port: OnceLock<Arc<dyn FocusPort>>,
    wire: tokio::sync::Mutex<()>,
}

impl Default for FocusLane {
    fn default() -> Self {
        FocusLane {
            state: Mutex::new(FocusDebounce::default()),
            port: OnceLock::new(),
            wire: tokio::sync::Mutex::new(()),
        }
    }
}

/// The lanes' clock: tokio's, so a paused test clock moves it.
fn now() -> Instant {
    tokio::time::Instant::now().into_std()
}

impl FocusLane {
    fn state(&self) -> MutexGuard<'_, FocusDebounce<Namer>> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The docked note changed (call `seq`): registered now, named once due.
    /// `false` when a later call was already seen.
    pub fn change(&self, seq: u64, namer: Namer) -> bool {
        self.state().change(seq, namer, now())
    }

    /// Whether the lane's room was admitted.
    pub fn connected(&self) -> bool {
        self.port.get().is_some()
    }

    /// The lane's room, once admitted.
    pub fn connect(&self, port: Arc<dyn FocusPort>) {
        let _ = self.port.set(port);
    }

    /// Drop what waits: its room was refused.
    pub fn forget_pending(&self) {
        self.state().forget_pending();
    }

    /// Hold the lane's sends: anything sent meanwhile goes out after.
    pub async fn hold(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.wire.lock().await
    }

    /// What the host was last told.
    pub fn last(&self) -> Option<Focus> {
        self.state().last().cloned()
    }

    /// Send what waits once it is due, unless a driver already does.
    pub fn drive(self: &Arc<Self>) {
        {
            let mut state = self.state();
            if state.driving || state.next_due().is_none() {
                return;
            }
            state.driving = true;
        }
        tokio::spawn(Arc::clone(self).run());
    }

    async fn run(self: Arc<Self>) {
        loop {
            let at = {
                let mut state = self.state();
                match state.next_due() {
                    Some(at) => at,
                    None => {
                        state.driving = false;
                        return;
                    }
                }
            };
            tokio::time::sleep_until(tokio::time::Instant::from_std(at)).await;
            let Some(due) = self.state().due(now()) else {
                continue;
            };
            let named = (due.request)().await;
            let Some(port) = self.port.get().cloned() else {
                continue;
            };
            let _wire = self.wire.lock().await;
            let Some(out) = self.state().commit(due.generation, named, now()) else {
                continue;
            };
            let clear = out.focus.is_none();
            if let Err(error) = port.send(out.focus).await {
                tracing::warn!(%error, "agents: the docked note's focus could not be sent");
                if clear {
                    self.owe(out.token);
                }
            }
        }
    }

    /// The dock closed (call `seq`; `None` on quit). Waits for a focus on
    /// the wire, then clears it; a clear that fails is owed and retried.
    pub async fn close(self: &Arc<Self>, seq: Option<u64>) -> Result<(), String> {
        let Some(OnClose::Clear(token)) = self.state().close(seq) else {
            return Ok(());
        };
        let Some(port) = self.port.get().cloned() else {
            return Ok(());
        };
        let _wire = self.wire.lock().await;
        if !self.state().current(token) {
            return Ok(());
        }
        let sent = port.send(None).await;
        if sent.is_err() {
            self.owe(token);
        }
        sent
    }

    fn owe(self: &Arc<Self>, token: Token) {
        if !self.state().owe_clear(token) {
            return;
        }
        let lane = Arc::clone(self);
        tokio::spawn(async move {
            let until = now() + FOCUS_TTL;
            let mut wait = CLEAR_RETRY_FIRST;
            loop {
                tokio::time::sleep(wait).await;
                if now() >= until {
                    return;
                }
                let Some(port) = lane.port.get().cloned() else {
                    return;
                };
                let _wire = lane.wire.lock().await;
                if !lane.state().owes_clear(token) {
                    return;
                }
                if port.send(None).await.is_ok() {
                    lane.state().cleared(token);
                    return;
                }
                wait = (wait * 2).min(CLEAR_RETRY_MOST);
            }
        });
    }

    /// The lane is gone (its account signed out): an owed clear is not
    /// retried any more.
    fn retire(&self) {
        let mut state = self.state();
        state.owed_clear = false;
        state.sends += 1;
    }
}

/// Every lane, by key.
pub struct FocusLanes<K> {
    lanes: Mutex<HashMap<K, Arc<FocusLane>>>,
}

impl<K> Default for FocusLanes<K> {
    fn default() -> Self {
        FocusLanes {
            lanes: Mutex::new(HashMap::new()),
        }
    }
}

impl<K: Eq + Hash + Clone> FocusLanes<K> {
    fn lanes(&self) -> MutexGuard<'_, HashMap<K, Arc<FocusLane>>> {
        self.lanes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// `key`'s lane, made on first use.
    pub fn lane(&self, key: &K) -> Arc<FocusLane> {
        Arc::clone(self.lanes().entry(key.clone()).or_default())
    }

    /// `key`'s lane, if it has one.
    pub fn get(&self, key: &K) -> Option<Arc<FocusLane>> {
        self.lanes().get(key).cloned()
    }

    /// Clear and drop every lane whose key `matches`, each clear bounded by
    /// `within`: the account is going (quit, sign-out).
    pub async fn close_where(&self, matches: impl Fn(&K) -> bool, within: Duration) {
        let gone: Vec<Arc<FocusLane>> = {
            let mut lanes = self.lanes();
            let keys: Vec<K> = lanes.keys().filter(|key| matches(key)).cloned().collect();
            keys.iter().filter_map(|key| lanes.remove(key)).collect()
        };
        futures_util::future::join_all(gone.iter().map(|lane| async move {
            let _ = tokio::time::timeout(within, lane.close(None)).await;
            lane.retire();
        }))
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::oneshot;

    fn focus(path: &str, heading: Option<&str>) -> Focus {
        Focus {
            drive: "tgdrive".to_owned(),
            path: path.to_owned(),
            heading: heading.map(str::to_owned),
        }
    }

    #[test]
    fn focus_is_sent_after_a_second_of_stillness() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let mut debounce = FocusDebounce::<&str>::default();
        // Five changes inside 900 ms: nothing before a second of stillness
        // after the last, then the last one.
        for (i, ms) in [0, 200, 400, 600, 800].into_iter().enumerate() {
            assert!(debounce.change(i as u64 + 1, "notes", at(ms)));
            assert!(debounce.due(at(ms + 100)).is_none());
        }
        assert!(debounce.due(at(1_799)).is_none());
        assert_eq!(debounce.next_due(), Some(at(1_800)));
        let due = debounce.due(at(1_800)).expect("due");
        assert_eq!(due.request, "notes");
        let first = debounce.commit(due.generation, Some(focus("notes/4.md", None)), at(1_800));
        assert_eq!(
            first.map(|out| out.focus),
            Some(Some(focus("notes/4.md", None)))
        );
        assert!(debounce.due(at(5_000)).is_none(), "sent once");
        // The same note again sends nothing until it is due a resend; a
        // heading change alone does.
        debounce.change(6, "same", at(6_000));
        let due = debounce.due(at(7_000)).expect("due");
        assert_eq!(
            debounce.commit(due.generation, Some(focus("notes/4.md", None)), at(7_000)),
            None
        );
        debounce.change(7, "heading", at(8_000));
        let due = debounce.due(at(9_000)).expect("due");
        let heading = Some(focus("notes/4.md", Some("Plans")));
        assert!(debounce
            .commit(due.generation, heading.clone(), at(9_000))
            .is_some());
        assert_eq!(debounce.last(), heading.as_ref());
        // Said again after FOCUS_RESEND, the same note goes out again: the
        // heartbeat that keeps the host's copy alive.
        let later = at(9_000) + FOCUS_RESEND;
        debounce.change(8, "heartbeat", later);
        let due = debounce.due(later + FOCUS_STILLNESS).expect("due");
        assert!(debounce
            .commit(due.generation, heading.clone(), later + FOCUS_STILLNESS)
            .is_some());
        // Closing drops what waits and clears what was sent, once; a call
        // older than one already seen changes nothing.
        debounce.change(9, "five", at(10_000));
        assert!(matches!(debounce.close(Some(10)), Some(OnClose::Clear(_))));
        assert!(
            !debounce.change(9, "late", at(10_001)),
            "older than the close"
        );
        assert!(debounce.due(at(20_000)).is_none());
        assert_eq!(debounce.close(Some(11)), Some(OnClose::Nothing));
        assert_eq!(debounce.close(Some(11)), None, "stale");
        assert_eq!(debounce.last(), None);
    }

    /// Records every send in order; a send waits for its gate when one is set.
    #[derive(Default)]
    struct Recorder {
        sent: Mutex<Vec<Option<Focus>>>,
        gates: Mutex<Vec<oneshot::Receiver<()>>>,
        failures: Mutex<usize>,
    }

    impl Recorder {
        fn sent(&self) -> Vec<Option<Focus>> {
            self.sent.lock().expect("lock").clone()
        }
    }

    impl FocusPort for Recorder {
        fn send(&self, focus: Option<Focus>) -> SendFuture<'_> {
            Box::pin(async move {
                let gate = self.gates.lock().expect("lock").pop();
                if let Some(gate) = gate {
                    let _ = gate.await;
                }
                self.sent.lock().expect("lock").push(focus);
                let mut failures = self.failures.lock().expect("lock");
                if *failures > 0 {
                    *failures -= 1;
                    return Err("offline".to_owned());
                }
                Ok(())
            })
        }
    }

    fn lane(port: &Arc<Recorder>) -> Arc<FocusLane> {
        let lane = Arc::new(FocusLane::default());
        lane.connect(Arc::clone(port) as Arc<dyn FocusPort>);
        lane
    }

    fn names(focus: Focus) -> Namer {
        Box::new(move || Box::pin(async move { Some(focus) }))
    }

    /// Let every task that can run, run.
    async fn settle() {
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
    }

    /// F2: the person moves the caret and folds the dock while the note is
    /// still being named. The fold wins: nothing reaches the host, and a
    /// focus call that arrives after the fold is dropped.
    #[tokio::test(start_paused = true)]
    async fn a_note_still_being_named_never_overtakes_the_fold() {
        let port = Arc::new(Recorder::default());
        let lane = lane(&port);
        let (named_tx, named_rx) = oneshot::channel::<Option<Focus>>();
        let slow: Namer = Box::new(move || Box::pin(async move { named_rx.await.ok().flatten() }));
        assert!(lane.change(1, slow));
        lane.drive();
        tokio::time::advance(FOCUS_STILLNESS).await;
        settle().await;
        // Naming is under way; the dock folds.
        lane.close(Some(2)).await.expect("nothing to clear");
        named_tx
            .send(Some(focus("notes/plans.md", None)))
            .expect("named");
        settle().await;
        tokio::time::advance(FOCUS_STILLNESS * 5).await;
        settle().await;
        assert_eq!(port.sent(), Vec::<Option<Focus>>::new());
        // A focus the webview sent before the fold, arriving after it.
        assert!(!lane.change(1, names(focus("notes/plans.md", None))));
        lane.drive();
        tokio::time::advance(FOCUS_STILLNESS * 5).await;
        settle().await;
        assert_eq!(port.sent(), Vec::<Option<Focus>>::new());
    }

    /// F2: a fold while the focus is on the wire waits for it, so the host
    /// receives the focus and then the clear, never the reverse.
    #[tokio::test(start_paused = true)]
    async fn a_clear_follows_the_focus_on_the_wire() {
        let port = Arc::new(Recorder::default());
        let (open, gate) = oneshot::channel();
        port.gates.lock().expect("lock").push(gate);
        let lane = lane(&port);
        let plans = focus("notes/plans.md", Some("Q3"));
        lane.change(1, names(plans.clone()));
        lane.drive();
        tokio::time::advance(FOCUS_STILLNESS).await;
        settle().await;
        // The focus is on the wire (its send waits on the gate); the fold.
        let closing = tokio::spawn({
            let lane = Arc::clone(&lane);
            async move { lane.close(Some(2)).await }
        });
        settle().await;
        assert_eq!(port.sent(), Vec::<Option<Focus>>::new());
        open.send(()).expect("gate");
        closing.await.expect("task").expect("cleared");
        assert_eq!(port.sent(), vec![Some(plans), None]);
    }

    /// D3: a clear the homeserver did not take is owed and tried again
    /// until it lands.
    #[tokio::test(start_paused = true)]
    async fn a_clear_that_failed_is_tried_again() {
        let port = Arc::new(Recorder::default());
        let lane = lane(&port);
        let plans = focus("notes/plans.md", None);
        lane.change(1, names(plans.clone()));
        lane.drive();
        tokio::time::advance(FOCUS_STILLNESS).await;
        settle().await;
        *port.failures.lock().expect("lock") = 2;
        assert!(lane.close(Some(2)).await.is_err(), "offline");
        // The retry's first wait starts once its task has run.
        settle().await;
        tokio::time::advance(CLEAR_RETRY_FIRST).await;
        settle().await;
        assert_eq!(port.sent(), vec![Some(plans.clone()), None, None]);
        settle().await;
        tokio::time::advance(CLEAR_RETRY_FIRST * 2).await;
        settle().await;
        assert_eq!(port.sent(), vec![Some(plans.clone()), None, None, None]);
        // It landed: nothing more is sent.
        tokio::time::advance(CLEAR_RETRY_MOST * 4).await;
        settle().await;
        assert_eq!(port.sent().len(), 4);
    }

    /// D3: the account signs out (or keeper quits) with the dock open: its
    /// lanes clear what they told.
    #[tokio::test(start_paused = true)]
    async fn an_account_that_goes_clears_its_focus() {
        let lanes = FocusLanes::<(&str, &str)>::default();
        let port = Arc::new(Recorder::default());
        let mine = lanes.lane(&("tgorka", "!dm"));
        mine.connect(Arc::clone(&port) as Arc<dyn FocusPort>);
        let other = Arc::new(Recorder::default());
        let theirs = lanes.lane(&("marta", "!dm"));
        theirs.connect(Arc::clone(&other) as Arc<dyn FocusPort>);
        for lane in [&mine, &theirs] {
            lane.change(1, names(focus("notes/plans.md", None)));
            lane.drive();
        }
        tokio::time::advance(FOCUS_STILLNESS).await;
        settle().await;
        lanes
            .close_where(|(account, _)| *account == "tgorka", Duration::from_secs(1))
            .await;
        assert_eq!(port.sent(), vec![Some(focus("notes/plans.md", None)), None]);
        assert_eq!(other.sent(), vec![Some(focus("notes/plans.md", None))]);
        assert!(lanes.get(&("tgorka", "!dm")).is_none());
    }
}
