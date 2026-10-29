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
//! Apple refused, the flow's deadline — and each retirement is explicit:
//! Apple promises a completion when the *person* cancels, not after
//! keeper's `cancel()`, so a sheet keeper dismissed must not wait for one
//! (R608-01). Retiring is idempotent and keyed by [`SheetId`], so a
//! completion that arrives after its sheet retired changes nothing, least
//! of all a newer sheet for the same sign-in.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

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
/// after it retired. A session released before its dismiss transition has
/// finished crashes when the transition completes (FB12132525), and the
/// transition has not started when the completion runs; AppAuth keeps it
/// two seconds for the same reason (`OIDRetainSessionBriefly`).
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
    state: String,
    opened: Instant,
    held: T,
}

impl<T> Sheet<T> {
    /// The `state` of the sign-in the sheet was opened for.
    pub fn state(&self) -> &str {
        &self.state
    }

    pub fn held(&self) -> &T {
        &self.held
    }
}

/// The sheets keeper has started and not yet retired, the ones it retired
/// and still holds for [`RETENTION`], and when the last one retired.
///
/// Two questions kept apart: whether a sheet still stops the next one from
/// opening (until it retires), and how long its native objects stay alive
/// (until [`RETENTION`] after that, released on a later turn than the
/// retirement — never inside the completion that retired it).
///
/// The retirement time is when keeper stopped counting the sheet as on
/// screen. It is not an observed end of the dismissal: nothing reports
/// that. [`DISMISSAL`] after it is an allowance for the animation, and the
/// observed signals are the scene's activation ([`SETTLE`]) and Apple's own
/// `start` refusing a window ([`SheetWait::refused`]).
#[derive(Debug)]
pub struct Sheets<T> {
    open: Vec<Sheet<T>>,
    retired: Vec<(Sheet<T>, Instant)>,
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

    /// Hold a sheet for `state` before it is started: from now until it
    /// retires it is the previous sheet the next one waits for.
    pub fn open(&mut self, id: SheetId, state: &str, held: T, now: Instant) {
        self.open.push(Sheet {
            id,
            state: state.to_owned(),
            opened: now,
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
    /// already retired (keeper cancelled it, its deadline passed) is late,
    /// and its caller delivers nothing.
    pub fn retire(&mut self, id: SheetId, now: Instant) -> bool {
        let Some(at) = self.open.iter().position(|sheet| sheet.id == id) else {
            return false;
        };
        let sheet = self.open.swap_remove(at);
        self.retired.push((sheet, now));
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
            .map(|(sheet, _)| sheet)
            .filter(|sheet| ids.contains(&sheet.id))
            .collect()
    }

    /// Cancel every sheet opened at least [`LIMIT`] ago. Its sign-in's own
    /// deadline has passed by then, so the sheet answers nobody, and it
    /// must not hold back a new one whatever became of its completion.
    pub fn overdue(&mut self, now: Instant) -> Vec<&Sheet<T>> {
        self.cancel(now, |sheet| {
            now.saturating_duration_since(sheet.opened) >= LIMIT
        })
    }

    /// Hand back what the shell held for every sheet retired at least
    /// [`RETENTION`] ago, to be dropped on the main thread — outside any
    /// borrow of this table, since a session may run its completion as it
    /// goes.
    pub fn release_due(&mut self, now: Instant) -> Vec<T> {
        let (due, kept) = std::mem::take(&mut self.retired)
            .into_iter()
            .partition(|(_, at)| now.saturating_duration_since(*at) >= RETENTION);
        self.retired = kept;
        due.into_iter().map(|(sheet, _)| sheet.held).collect()
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
    fn run(world: impl Fn(Duration) -> Look) -> (Step, Duration) {
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

    /// R608-01: keeper cancels a sheet and Apple never runs its completion.
    /// The next sign-in's sheet must still open once the cancelled one has
    /// had its dismissal — not wait out the whole [`LIMIT`] behind a sheet
    /// that is gone.
    #[test]
    fn a_sheet_keeper_cancelled_lets_the_next_open_without_its_completion() {
        let start = Instant::now();
        let mut sheets = Sheets::new();
        sheets.open(SheetId::next(), "account", (), start);
        sheets.open(SheetId::next(), "matrix", (), start);
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
        let mut sheets = Sheets::new();
        let first = SheetId::next();
        sheets.open(first, "s", (), start);
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
        sheets.open(second, "s", (), late);
        assert!(!sheets.retire(first, late + POLL));
        assert_eq!(sheets.previous(late + POLL), PreviousSheet::OnScreen);
        assert!(sheets.retire(second, late + SETTLE));
        assert_eq!(
            sheets.previous(late + SETTLE),
            PreviousSheet::EndedAgo(Duration::ZERO)
        );
        assert!(!sheets.retire(second, late + SETTLE));
    }

    /// Whatever became of a sheet's completion, once its sign-in's deadline
    /// has passed it no longer holds back a new one.
    #[test]
    fn a_sheet_past_its_sign_ins_deadline_stops_holding_back_the_next() {
        let start = Instant::now();
        let mut sheets = Sheets::new();
        sheets.open(SheetId::next(), "stale", (), start);
        let before = start + LIMIT - POLL;
        assert!(sheets.overdue(before).is_empty());
        assert_eq!(sheets.previous(before), PreviousSheet::OnScreen);
        assert_eq!(states(sheets.overdue(start + LIMIT)), ["stale"]);
        assert_eq!(
            sheets.previous(start + LIMIT),
            PreviousSheet::EndedAgo(Duration::ZERO)
        );
    }

    /// A retired sheet's native objects outlive its retirement by
    /// [`RETENTION`] — never released in the turn that retired it, which
    /// may be its own completion — and are handed back exactly once.
    #[test]
    fn a_retired_sheet_is_held_through_its_dismissal_and_released_once() {
        let start = Instant::now();
        let mut sheets = Sheets::new();
        let (done, cancelled) = (SheetId::next(), SheetId::next());
        sheets.open(done, "done", "session one", start);
        sheets.open(cancelled, "cancelled", "session two", start);
        assert!(sheets.retire(done, start));
        let later = start + Duration::from_secs(1);
        assert_eq!(states(sheets.cancel(later, |_| true)), ["cancelled"]);

        assert!(sheets.release_due(start).is_empty());
        assert!(sheets.release_due(start + RETENTION - POLL).is_empty());
        assert_eq!(sheets.release_due(start + RETENTION), ["session one"]);
        assert!(sheets.release_due(start + RETENTION).is_empty());
        assert_eq!(sheets.release_due(later + RETENTION), ["session two"]);
        assert!(sheets.release_due(later + LIMIT).is_empty());
    }
}
