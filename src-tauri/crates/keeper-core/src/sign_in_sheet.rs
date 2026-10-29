//! When the iPhone's sign-in sheet opens, and when keeper stops waiting to
//! open it (ForgeSignin).
//!
//! `ASWebAuthenticationSession` starts only over a window whose scene is
//! `foregroundActive`; over any other it fails with
//! `presentationContextInvalid`. An account sign-in opens two sheets one
//! after the other — the identity provider's, then the forge's — and the
//! second is asked for while the first is still going away. kalypso logged
//! the second one refused as "keeper was not in front" five times between
//! 2026-10-17 and 2026-10-28, each about three seconds after the first sheet
//! answered: the shell had looked at the window for three seconds and given
//! up, with keeper in front the whole time, and the person was left with a
//! link to copy by hand.
//!
//! [`SheetWait`] is that decision, without the FFI: the shell
//! (`crates/keeper/src/web_auth.rs`) reads keeper's window every
//! [`POLL`] and hands what it saw to [`SheetWait::next`], which answers
//! present, look again, stop or give up. The rules:
//!
//! - a sheet opens over a window that is active, has been for [`SETTLE`]
//!   (unless it was active from the first look), and once the previous sheet
//!   is off screen and has been gone for [`DISMISSAL`];
//! - while keeper is in front — active, or inactive behind a sheet that is
//!   still going away, an alert, Control Centre — it keeps waiting, for as
//!   long as the sign-in itself waits: keeper in front and waiting is never
//!   a reason to fall back to the link;
//! - it gives up when keeper has been out of front (background, or no scene)
//!   for [`AWAY_GRACE`], when Apple refused [`REFUSALS`] sheets it was
//!   handed, or after [`LIMIT`] whatever happened; the sign-in then keeps
//!   waiting for the link pasted by hand (`sign_in_link`, R480);
//! - it stops without a word once the sign-in no longer waits (answered,
//!   cancelled, timed out).
//!
//! [`Sheets`] is the other half: which sheets keeper has started and not
//! yet retired, which is what [`PreviousSheet`] reports. A sheet retires on
//! every way it can end — its completion, keeper's own cancel, a `start`
//! Apple refused, its sign-in ending however it ends (answered elsewhere,
//! failed, superseded by a new sign-in, timed out) — and each retirement is
//! explicit: Apple promises a completion when the *person* cancels, not
//! after keeper's `cancel()`, so a sheet keeper dismissed must not wait for
//! one (R608-01), and a sheet whose sign-in has ended must not wait for a
//! later sign-in to notice (R608-02). Retiring is idempotent and keyed by
//! [`SheetId`], so a completion that arrives after its sheet retired
//! changes nothing, least of all a newer sheet for the same sign-in.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::oauth::{FlowId, OAuthFlowRegistry};

/// How often the shell looks at keeper's window while it waits.
pub const POLL: Duration = Duration::from_millis(250);

/// How long a window that became active while the sheet waited must stay
/// active before the sheet opens over it: a scene flips back to active
/// while the previous sheet's dismissal is still running.
pub const SETTLE: Duration = Duration::from_millis(500);

/// How long after the previous sheet went away the next one may open: its
/// dismissal animation outlives its completion handler.
pub const DISMISSAL: Duration = Duration::from_secs(1);

/// How long keeper may be out of front (in the background, or a window
/// without a scene) before the sheet gives up.
pub const AWAY_GRACE: Duration = Duration::from_secs(2);

/// How many sheets Apple may refuse after keeper judged the window ready
/// before keeper stops handing it more.
pub const REFUSALS: u32 = 3;

/// The longest the sheet waits, whatever it sees: the account sign-in's own
/// deadline (`org_account::oidc::FLOW_TIMEOUT`).
pub const LIMIT: Duration = Duration::from_secs(300);

/// How long a retired sheet's session, anchor and completion are still held
/// once the main thread is next free after the retirement. A session
/// released before its dismiss transition has finished crashes when the
/// transition completes (FB12132525), and the transition has not started
/// when the completion runs; AppAuth keeps it two seconds for the same
/// reason, counted from the same turn (`OIDRetainSessionBriefly`).
pub const RETENTION: Duration = Duration::from_secs(2);

/// Why the sheet gave up while keeper was out of front, or at [`LIMIT`].
pub const NOT_IN_FRONT: &str = "keeper was not in front, so the sign-in sheet could not open";

/// Why the sheet gave up after Apple refused it [`REFUSALS`] times.
pub const WOULD_NOT_START: &str = "the sign-in sheet would not start over keeper's window";

/// keeper's window's scene, as `UISceneActivationState` reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activation {
    /// `foregroundActive`: a sheet can open.
    Active,
    /// `foregroundInactive`: on screen, behind something that holds the
    /// attention for now — a sheet going away, an alert, Control Centre.
    Inactive,
    /// `background`.
    Background,
    /// The window has no scene, or the scene answered a state this does not
    /// know.
    Unattached,
}

impl Activation {
    /// From whether the window has a `windowScene`, and that scene's raw
    /// `activationState` (`-1` unattached, `0` foreground active, `1`
    /// foreground inactive, `2` background).
    pub fn of_scene(scene: bool, raw: isize) -> Self {
        match (scene, raw) {
            (true, 0) => Self::Active,
            (true, 1) => Self::Inactive,
            (true, 2) => Self::Background,
            _ => Self::Unattached,
        }
    }

    /// Whether keeper is on screen.
    pub fn in_front(self) -> bool {
        matches!(self, Self::Active | Self::Inactive)
    }
}

/// The sign-in sheet keeper showed before this one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviousSheet {
    /// None since keeper started.
    None,
    /// Still on screen: started and not yet retired ([`Sheets`]).
    OnScreen,
    /// It went away this long ago.
    EndedAgo(Duration),
}

/// What the shell saw at one look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Look {
    pub activation: Activation,
    pub previous: PreviousSheet,
    /// Whether the sign-in still waits for its callback.
    pub flow_waits: bool,
}

/// What the shell does after a look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Hand the sheet to Apple now.
    Present,
    /// Look again after this long.
    LookAgain(Duration),
    /// The sign-in no longer waits: do nothing more.
    Stop,
    /// Stop trying; the sentence is the reason the sheet could not open,
    /// given to the waiting sign-in so its link is offered by hand.
    GiveUp(&'static str),
}

/// One sheet's wait for keeper's window, from the first look on. `now` is
/// always the time since that first look.
#[derive(Debug, Clone, Copy, Default)]
pub struct SheetWait {
    /// When the window was first seen active in the current active streak.
    active_since: Option<Duration>,
    /// When keeper was first seen out of front in the current away streak.
    away_since: Option<Duration>,
    /// Whether the window has been anything but active since the first
    /// look, or Apple refused a sheet: an active window must then settle.
    unsettled: bool,
    refusals: u32,
}

impl SheetWait {
    pub fn new() -> Self {
        Self::default()
    }

    /// Decide what to do after `look`, taken `now` after the first look.
    pub fn next(&mut self, look: Look, now: Duration) -> Step {
        if !look.flow_waits {
            return Step::Stop;
        }
        if now >= LIMIT {
            return Step::GiveUp(NOT_IN_FRONT);
        }
        if look.activation != Activation::Active {
            self.active_since = None;
            self.unsettled = true;
        }
        if look.activation.in_front() {
            self.away_since = None;
        } else {
            let away = *self.away_since.get_or_insert(now);
            if now.saturating_sub(away) >= AWAY_GRACE {
                return Step::GiveUp(NOT_IN_FRONT);
            }
            return Step::LookAgain(POLL);
        }
        if look.activation != Activation::Active {
            return Step::LookAgain(POLL);
        }
        let since = *self.active_since.get_or_insert(now);
        let settled = !self.unsettled || now.saturating_sub(since) >= SETTLE;
        let previous_gone = match look.previous {
            PreviousSheet::None => true,
            PreviousSheet::OnScreen => false,
            PreviousSheet::EndedAgo(ago) => ago >= DISMISSAL,
        };
        if settled && previous_gone {
            Step::Present
        } else {
            Step::LookAgain(POLL)
        }
    }

    /// Apple refused the sheet [`next`](Self::next) said to present. The
    /// window must settle again before the next one, and the previous
    /// sheet's dismissal gets its time.
    pub fn refused(&mut self) -> Step {
        self.refusals += 1;
        self.active_since = None;
        self.unsettled = true;
        if self.refusals >= REFUSALS {
            Step::GiveUp(WOULD_NOT_START)
        } else {
            Step::LookAgain(DISMISSAL)
        }
    }
}

/// One sheet keeper started, unique for the life of the process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SheetId(u64);

impl SheetId {
    /// A fresh id, taken before the sheet is built so its completion can
    /// carry it.
    pub fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// A sheet in [`Sheets`]: the sign-in it serves, and what the shell holds
/// alive for it (`T`: the native session, its anchor, its completion).
#[derive(Debug)]
pub struct Sheet<T> {
    id: SheetId,
    flows: Arc<OAuthFlowRegistry>,
    flow: Option<FlowId>,
    state: String,
    held: T,
}

impl<T> Sheet<T> {
    pub fn id(&self) -> SheetId {
        self.id
    }

    /// The registry the sign-in waits in: the account's or Matrix's.
    pub fn flows(&self) -> &Arc<OAuthFlowRegistry> {
        &self.flows
    }

    /// The `state` of the sign-in the sheet was opened for.
    pub fn state(&self) -> &str {
        &self.state
    }

    /// Whether this sheet's original registration still waits, not a later
    /// registration that reused its state.
    pub fn flow_waits(&self) -> bool {
        self.flow.is_some() && self.flows.flow_id(&self.state) == self.flow
    }

    pub fn held(&self) -> &T {
        &self.held
    }
}

/// A retired sheet, held for [`RETENTION`] from `since`.
#[derive(Debug)]
struct Retired<T> {
    sheet: Sheet<T>,
    /// When the main thread first got to it after the retirement — the
    /// retention's start. `None` until then.
    since: Option<Instant>,
}

/// What to do with a retired sheet's native objects, asked on the main
/// thread ([`Sheets::retention`]).
#[derive(Debug, PartialEq, Eq)]
pub enum Retention<T> {
    /// Ask again this long from now, on the main thread.
    After(Duration),
    /// Drop these now, outside the table's borrow.
    Release(T),
    /// Nothing held: the sheet is still open, never existed, or was
    /// released already.
    Gone,
}

/// The sheets keeper has started and not yet retired, the ones it retired
/// and still holds for [`RETENTION`], and when the last one retired.
///
/// Two questions kept apart: whether a sheet still stops the next one from
/// opening (until it retires), and how long its native objects stay alive
/// ([`Sheets::retention`]: [`RETENTION`] from the first main-thread turn
/// after the retirement, never in the turn that retired it).
///
/// A sheet lives exactly as long as its sign-in waits: whatever ends the
/// flow — its answer, a failure, keeper's cancel, a new sign-in superseding
/// it, its deadline when the sign-in's guard lets go — ends the sheet with
/// it ([`Sheets::ended`], R608-02). The sheet has no clock of its own: its
/// deadline is its flow's.
///
/// The retirement time is when keeper stopped counting the sheet as on
/// screen. It is not an observed end of the dismissal: nothing reports
/// that. [`DISMISSAL`] after it is an allowance for the animation, and the
/// observed signals are the scene's activation ([`SETTLE`]) and Apple's own
/// `start` refusing a window ([`SheetWait::refused`]).
#[derive(Debug)]
pub struct Sheets<T> {
    open: Vec<Sheet<T>>,
    retired: Vec<Retired<T>>,
    last_retired: Option<Instant>,
}

impl<T> Default for Sheets<T> {
    fn default() -> Self {
        Self {
            open: Vec::new(),
            retired: Vec::new(),
            last_retired: None,
        }
    }
}

impl<T> Sheets<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Hold a sheet for the sign-in waiting under `state` in `flows`, before
    /// the sheet is started: from now until it retires it is the previous
    /// sheet the next one waits for.
    pub fn open(&mut self, id: SheetId, flows: Arc<OAuthFlowRegistry>, state: &str, held: T) {
        self.open.push(Sheet {
            id,
            flow: flows.flow_id(state),
            flows,
            state: state.to_owned(),
            held,
        });
    }

    /// The sheet before the one being asked for, `now`.
    pub fn previous(&self, now: Instant) -> PreviousSheet {
        if !self.open.is_empty() {
            return PreviousSheet::OnScreen;
        }
        self.last_retired.map_or(PreviousSheet::None, |at| {
            PreviousSheet::EndedAgo(now.saturating_duration_since(at))
        })
    }

    /// Retire sheet `id`: its completion ran, or Apple refused to start it.
    /// `true` the first time only — a completion that finds its sheet
    /// already retired (keeper cancelled it, its sign-in ended) is late,
    /// and its caller delivers nothing.
    pub fn retire(&mut self, id: SheetId, now: Instant) -> bool {
        let Some(at) = self.open.iter().position(|sheet| sheet.id == id) else {
            return false;
        };
        let sheet = self.open.swap_remove(at);
        self.retired.push(Retired { sheet, since: None });
        self.last_retired = Some(now);
        true
    }

    /// keeper ends the open sheets `which` picks, without waiting for any
    /// completion: they retire now, and are returned so the shell can
    /// dismiss each and tell its sign-in it was cancelled.
    pub fn cancel(&mut self, now: Instant, which: impl Fn(&Sheet<T>) -> bool) -> Vec<&Sheet<T>> {
        let ids: Vec<SheetId> = self
            .open
            .iter()
            .filter(|sheet| which(sheet))
            .map(|sheet| sheet.id)
            .collect();
        for &id in &ids {
            self.retire(id, now);
        }
        self.retired
            .iter()
            .map(|retired| &retired.sheet)
            .filter(|sheet| ids.contains(&sheet.id))
            .collect()
    }

    /// Retire every open sheet whose sign-in no longer waits, however it
    /// ended, and return them to be dismissed: such a sheet answers nobody,
    /// and must not hold back a new one. The shell asks on the main thread
    /// as soon as a flow ends (`OAuthFlowRegistry::on_end`), and again
    /// before every look, so no look ever counts an ended sign-in's sheet as
    /// on screen.
    pub fn ended(&mut self, now: Instant) -> Vec<&Sheet<T>> {
        self.cancel(now, |sheet| !sheet.flow_waits())
    }

    /// Where retired sheet `id`'s native objects stand, asked on the main
    /// thread. The first ask starts its [`RETENTION`] and must come on the
    /// first main-thread turn *after* the one that retired it, queued from
    /// there: the dismissal transition has not started while the
    /// retirement's turn runs, and a main thread that stays busy after it
    /// must not use the retention up (AppAuth's `OIDRetainSessionBriefly`,
    /// R608-03). Each sheet has its own clock, so no other sheet's release
    /// takes it early.
    pub fn retention(&mut self, id: SheetId, now: Instant) -> Retention<T> {
        let Some(at) = self
            .retired
            .iter()
            .position(|retired| retired.sheet.id == id)
        else {
            return Retention::Gone;
        };
        let since = *self.retired[at].since.get_or_insert(now);
        let held = now.saturating_duration_since(since);
        if held < RETENTION {
            return Retention::After(RETENTION - held);
        }
        Retention::Release(self.retired.swap_remove(at).sheet.held)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look(activation: Activation, previous: PreviousSheet) -> Look {
        Look {
            activation,
            previous,
            flow_waits: true,
        }
    }

    /// Run one wait the way the shell does — look, then look again after
    /// the step's delay — with `world` answering what each look sees at
    /// `now`. Returns the first step that is not a look-again, and when.
    fn run(mut world: impl FnMut(Duration) -> Look) -> (Step, Duration) {
        let mut wait = SheetWait::new();
        let mut now = Duration::ZERO;
        loop {
            match wait.next(world(now), now) {
                Step::LookAgain(after) => now += after,
                step => return (step, now),
            }
        }
    }

    #[test]
    fn a_window_active_from_the_first_look_gets_the_sheet_at_once() {
        assert_eq!(
            run(|_| look(Activation::Active, PreviousSheet::None)),
            (Step::Present, Duration::ZERO)
        );
        assert_eq!(
            run(|_| look(Activation::Active, PreviousSheet::EndedAgo(DISMISSAL))),
            (Step::Present, Duration::ZERO)
        );
    }

    /// kalypso, 2026-10-28 08:21:50–53Z: the identity provider's sheet has
    /// just answered and the forge's is asked for. The window stays inactive
    /// past the three seconds the shell used to give it — keeper is in front
    /// throughout, so the sheet waits rather than falling back to the link,
    /// and opens once the window has settled and the first sheet has been
    /// gone for its dismissal.
    #[test]
    fn the_second_sheet_waits_out_the_first_sheets_dismissal_with_keeper_in_front() {
        let inactive_until = Duration::from_secs(4);
        let (step, at) = run(|now| Look {
            activation: if now < inactive_until {
                Activation::Inactive
            } else {
                Activation::Active
            },
            previous: if now < Duration::from_millis(500) {
                PreviousSheet::OnScreen
            } else {
                PreviousSheet::EndedAgo(now - Duration::from_millis(500))
            },
            flow_waits: true,
        });
        assert_eq!(step, Step::Present);
        assert_eq!(
            at,
            inactive_until + SETTLE,
            "opened once the window settled"
        );
    }

    #[test]
    fn keeper_in_front_and_inactive_never_gives_up_it_stops_when_the_sign_in_ends() {
        let ends = Duration::from_secs(120);
        let (step, at) = run(|now| Look {
            activation: Activation::Inactive,
            previous: PreviousSheet::None,
            flow_waits: now < ends,
        });
        assert_eq!(step, Step::Stop);
        assert_eq!(at, ends);
    }

    #[test]
    fn a_sheet_still_on_screen_or_just_gone_is_waited_for() {
        let (step, at) = run(|now| {
            let previous = if now < Duration::from_secs(2) {
                PreviousSheet::OnScreen
            } else {
                PreviousSheet::EndedAgo(now - Duration::from_secs(2))
            };
            look(Activation::Active, previous)
        });
        assert_eq!(step, Step::Present);
        assert_eq!(at, Duration::from_secs(2) + DISMISSAL);
    }

    #[test]
    fn keeper_out_of_front_gives_up_after_the_grace_and_not_before() {
        for away in [Activation::Background, Activation::Unattached] {
            let (step, at) = run(|_| look(away, PreviousSheet::None));
            assert_eq!(step, Step::GiveUp(NOT_IN_FRONT), "{away:?}");
            assert_eq!(at, AWAY_GRACE, "{away:?}");
        }
        // Away for less than the grace, then back: the sheet opens.
        let back = AWAY_GRACE - POLL;
        let (step, at) = run(|now| {
            let activation = if now < back {
                Activation::Background
            } else {
                Activation::Active
            };
            look(activation, PreviousSheet::None)
        });
        assert_eq!(step, Step::Present);
        assert_eq!(at, back + SETTLE);
    }

    #[test]
    fn an_away_streak_restarts_after_keeper_was_in_front() {
        // Out of front, briefly back (inactive), out again: each streak is
        // measured on its own.
        let (step, at) = run(|now| {
            let activation =
                if now >= Duration::from_millis(1_500) && now < Duration::from_millis(1_750) {
                    Activation::Inactive
                } else {
                    Activation::Background
                };
            look(activation, PreviousSheet::None)
        });
        assert_eq!(step, Step::GiveUp(NOT_IN_FRONT));
        assert_eq!(at, Duration::from_millis(1_750) + AWAY_GRACE);
    }

    #[test]
    fn apple_refusing_the_sheet_is_retried_after_a_settle_and_given_up_at_the_third() {
        let mut wait = SheetWait::new();
        let ready = look(Activation::Active, PreviousSheet::None);
        assert_eq!(wait.next(ready, Duration::ZERO), Step::Present);
        assert_eq!(wait.refused(), Step::LookAgain(DISMISSAL));
        // The window must settle again before the next sheet.
        assert_eq!(wait.next(ready, DISMISSAL), Step::LookAgain(POLL));
        assert_eq!(wait.next(ready, DISMISSAL + SETTLE), Step::Present);
        assert_eq!(wait.refused(), Step::LookAgain(DISMISSAL));
        assert_eq!(wait.refused(), Step::GiveUp(WOULD_NOT_START));
    }

    #[test]
    fn nothing_waits_past_the_limit() {
        let mut wait = SheetWait::new();
        let inactive = look(Activation::Inactive, PreviousSheet::None);
        assert_eq!(wait.next(inactive, LIMIT - POLL), Step::LookAgain(POLL));
        assert_eq!(wait.next(inactive, LIMIT), Step::GiveUp(NOT_IN_FRONT));
    }

    #[test]
    fn the_scene_states_map_as_uikit_numbers_them() {
        assert_eq!(Activation::of_scene(true, 0), Activation::Active);
        assert_eq!(Activation::of_scene(true, 1), Activation::Inactive);
        assert_eq!(Activation::of_scene(true, 2), Activation::Background);
        assert_eq!(Activation::of_scene(true, -1), Activation::Unattached);
        assert_eq!(Activation::of_scene(false, 0), Activation::Unattached);
        assert_eq!(Activation::of_scene(true, 7), Activation::Unattached);
    }

    fn states<T>(sheets: Vec<&Sheet<T>>) -> Vec<String> {
        sheets
            .into_iter()
            .map(|sheet| sheet.state().to_owned())
            .collect()
    }

    fn registry() -> Arc<OAuthFlowRegistry> {
        Arc::new(OAuthFlowRegistry::new())
    }

    /// The shell's look, on the main thread: retire the sheets of sign-ins
    /// that ended, then report the previous sheet.
    fn shell_look<T>(sheets: &mut Sheets<T>, now: Instant) -> Look {
        sheets.ended(now);
        look(Activation::Active, sheets.previous(now))
    }

    /// R608-01: keeper cancels a sheet and Apple never runs its completion.
    /// The next sign-in's sheet must still open once the cancelled one has
    /// had its dismissal — not wait out the whole [`LIMIT`] behind a sheet
    /// that is gone.
    #[test]
    fn a_sheet_keeper_cancelled_lets_the_next_open_without_its_completion() {
        let start = Instant::now();
        let flows = registry();
        let mut sheets = Sheets::new();
        sheets.open(SheetId::next(), Arc::clone(&flows), "account", ());
        sheets.open(SheetId::next(), Arc::clone(&flows), "matrix", ());
        assert_eq!(sheets.previous(start), PreviousSheet::OnScreen);

        let cancelled_at = start + Duration::from_secs(30);
        assert_eq!(
            states(sheets.cancel(cancelled_at, |sheet| sheet.state() == "account")),
            ["account"]
        );
        // The other registry's sheet is still up, so the next one waits.
        assert_eq!(sheets.previous(cancelled_at), PreviousSheet::OnScreen);
        assert_eq!(
            states(sheets.cancel(cancelled_at, |sheet| sheet.state() == "matrix")),
            ["matrix"]
        );

        // No completion ever arrives. The next sign-in looks at the table
        // at each poll, with keeper in front and active.
        let (step, at) = run(|now| look(Activation::Active, sheets.previous(cancelled_at + now)));
        assert_eq!((step, at), (Step::Present, DISMISSAL));
    }

    /// A completion that runs after keeper retired its sheet — the same
    /// sign-in's next sheet already open under the same `state` — is
    /// refused: the newer sheet stays the one on screen, its own completion
    /// still counts, and the dismissal clock is the real retirement's.
    #[test]
    fn a_late_completion_touches_neither_a_newer_sheet_nor_the_clock() {
        let start = Instant::now();
        let flows = registry();
        let mut sheets = Sheets::new();
        let first = SheetId::next();
        sheets.open(first, Arc::clone(&flows), "s", ());
        let cancelled_at = start + Duration::from_secs(1);
        assert_eq!(states(sheets.cancel(cancelled_at, |_| true)), ["s"]);

        let late = cancelled_at + Duration::from_secs(5);
        assert!(!sheets.retire(first, late), "the first sheet retired once");
        assert_eq!(
            sheets.previous(late),
            PreviousSheet::EndedAgo(Duration::from_secs(5)),
            "a late completion does not restart the dismissal"
        );

        let second = SheetId::next();
        sheets.open(second, Arc::clone(&flows), "s", ());
        assert!(!sheets.retire(first, late + POLL));
        assert_eq!(sheets.previous(late + POLL), PreviousSheet::OnScreen);
        assert!(sheets.retire(second, late + SETTLE));
        assert_eq!(
            sheets.previous(late + SETTLE),
            PreviousSheet::EndedAgo(Duration::ZERO)
        );
        assert!(!sheets.retire(second, late + SETTLE));
    }

    /// R608-02, the reviewer's sequence: the account sign-in's sheet opens
    /// at t=299 of its flow, and the flow times out at t=300 with no
    /// completion — its guard lets go of the registry entry, nothing else.
    /// The registry says so at once, the sheet retires on that, and the next
    /// sign-in's sheet opens after the dismissal allowance — not at t=599,
    /// 300 s after the old sheet opened, where its own wait gives up.
    #[test]
    fn a_sheet_retires_when_its_sign_in_times_out_and_the_next_opens_after_its_dismissal() {
        let flow_start = Instant::now();
        let flows = registry();
        let told = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let hook = Arc::clone(&told);
        assert!(flows.on_end(move |state| {
            hook.lock().expect("not poisoned").push(state.to_owned());
        }));
        let _old = flows.register_with_link("old".to_owned());

        let mut sheets = Sheets::new();
        let presented = flow_start + Duration::from_secs(299);
        sheets.open(SheetId::next(), Arc::clone(&flows), "old", ());
        assert!(sheets.ended(presented + SETTLE).is_empty(), "still waits");
        assert_eq!(sheets.previous(presented + SETTLE), PreviousSheet::OnScreen);

        // `authorize`'s timeout: its `FlowGuard` drops, and only removes.
        let deadline = flow_start + Duration::from_secs(300);
        flows.remove("old");
        assert_eq!(
            *told.lock().expect("not poisoned"),
            ["old"],
            "the shell is told when the flow ends, not at a later sign-in"
        );
        assert_eq!(states(sheets.ended(deadline)), ["old"]);
        assert!(sheets.ended(deadline).is_empty(), "retired once");

        let _next = flows.register_with_link("next".to_owned());
        let (step, at) = run(|now| shell_look(&mut sheets, deadline + now));
        assert_eq!((step, at), (Step::Present, DISMISSAL));
    }

    /// A new sign-in supersedes one whose sheet is still open: ending the
    /// waiting flows (`account_sign_in`) retires that sheet, so the new
    /// sheet opens after its dismissal. A sheet whose sign-in still waits —
    /// the Matrix sign-in beside it, in its own registry — stays on screen
    /// and keeps holding the next one back until its own sign-in ends.
    #[test]
    fn a_new_sign_in_supersedes_an_open_sheet_and_opens_after_its_dismissal() {
        let start = Instant::now();
        let (account, matrix) = (registry(), registry());
        let _old = account.register_with_link("old".to_owned());
        let _matrix = matrix.register_with_link("matrix".to_owned());
        let mut sheets = Sheets::new();
        sheets.open(SheetId::next(), Arc::clone(&account), "old", ());
        sheets.open(SheetId::next(), Arc::clone(&matrix), "matrix", ());

        let retry = start + Duration::from_secs(40);
        account.cancel_all();
        let _new = account.register_with_link("new".to_owned());
        assert_eq!(states(sheets.ended(retry)), ["old"]);
        assert_eq!(
            sheets.previous(retry),
            PreviousSheet::OnScreen,
            "the Matrix sheet's sign-in still waits"
        );

        // The Matrix sign-in ends 3 s later; the new sheet opens after its
        // sheet's dismissal, with keeper in front and active throughout.
        let matrix_ends = Duration::from_secs(3);
        let (step, at) = run(|now| {
            if now >= matrix_ends {
                matrix.remove("matrix");
            }
            shell_look(&mut sheets, retry + now)
        });
        assert_eq!((step, at), (Step::Present, matrix_ends + DISMISSAL));
    }

    /// Delayed end-hook cleanup must retire an old registration's sheet
    /// even after a replacement reuses its state, without touching the
    /// replacement's sheet or accepting the old completion.
    #[test]
    fn delayed_cleanup_retires_only_the_replaced_registration() {
        let now = Instant::now();
        let flows = registry();
        let _old = flows.register_with_link("same".to_owned());
        let mut sheets = Sheets::new();
        let old = SheetId::next();
        sheets.open(old, Arc::clone(&flows), "same", "old");
        let (_new, mut reasons) = flows.register_with_link("same".to_owned());
        let new = SheetId::next();
        sheets.open(new, Arc::clone(&flows), "same", "replacement");

        let ended = sheets.ended(now);
        assert_eq!(ended.len(), 1);
        assert_eq!(ended[0].held(), &"old");
        assert!(
            !ended[0].flow_waits(),
            "do not cancel a replacement on dismissal"
        );
        assert!(!sheets.retire(old, now + POLL), "ignore the old completion");
        assert_eq!(sheets.previous(now + POLL), PreviousSheet::OnScreen);
        assert!(sheets.ended(now + POLL).is_empty());
        assert!(flows.not_shown("same", "the replacement still waits"));
        assert_eq!(
            reasons.try_recv().expect("replacement"),
            "the replacement still waits"
        );
        flows.remove("same");
        let ended = sheets.ended(now + SETTLE);
        assert_eq!(ended.len(), 1);
        assert_eq!(ended[0].id(), new);
    }

    /// R608-03: a retired sheet's objects are held [`RETENTION`] from the
    /// first main-thread turn after its retirement, not from the retirement.
    /// The reviewer's main thread is busy for 2.25 s after it: the queued
    /// turn then starts the retention rather than releasing at once. Each
    /// sheet keeps its own clock, so releasing one never takes another
    /// early, and nothing is released twice.
    #[test]
    fn retention_starts_once_the_main_thread_is_free_and_runs_per_sheet() {
        let retired_at = Instant::now();
        let flows = registry();
        let mut sheets = Sheets::new();
        let (first, second, open) = (SheetId::next(), SheetId::next(), SheetId::next());
        sheets.open(first, Arc::clone(&flows), "first", "session one");
        sheets.open(second, Arc::clone(&flows), "second", "session two");
        sheets.open(open, Arc::clone(&flows), "open", "session three");
        assert!(sheets.retire(first, retired_at));

        let free = retired_at + Duration::from_millis(2_250);
        assert_eq!(sheets.retention(first, free), Retention::After(RETENTION));
        assert!(sheets.retire(second, free + POLL));
        assert_eq!(
            sheets.retention(first, free + RETENTION - POLL),
            Retention::After(POLL),
            "a timer that fires early is asked to wait out the rest"
        );
        assert_eq!(
            sheets.retention(first, free + RETENTION),
            Retention::Release("session one")
        );
        assert_eq!(sheets.retention(first, free + RETENTION), Retention::Gone);

        // The second sheet's first main turn comes later still; the first's
        // release took nothing of it.
        let second_free = free + RETENTION + SETTLE;
        assert_eq!(
            sheets.retention(second, second_free),
            Retention::After(RETENTION)
        );
        assert_eq!(
            sheets.retention(second, second_free + RETENTION),
            Retention::Release("session two")
        );
        assert_eq!(sheets.retention(open, second_free), Retention::Gone);
    }
}
