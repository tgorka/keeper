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

use std::time::Duration;

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
    /// Still on screen: its completion has not run yet.
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
}
