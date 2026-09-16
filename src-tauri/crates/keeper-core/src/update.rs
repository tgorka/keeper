//! When keeper looks for its own update, and whether it may install one by
//! itself.
//!
//! The in-app updater used to be two clicks: *Check for updates*, then
//! *Download and install*. That is honest and it is also why installs lag —
//! nobody opens Settings → About to ask. Every other desktop app of this shape
//! (Chrome, Slack, VS Code) checks on a cadence, installs in the background,
//! and applies the new build the next time it starts. This module is the
//! keeper version of that decision, and it lives here rather than in the
//! webview because the cadence, the default, and the *refusal to relaunch* are
//! policy, not rendering (AD-55: the shell and the frontend are call sites).
//!
//! Three rules the numbers below encode:
//!
//! 1. **Never on boot.** The first check waits [`FIRST_CHECK_DELAY_MS`], so a
//!    cold launch spends its first seconds on sync and the room list, not on a
//!    release manifest.
//! 2. **Never a relaunch somebody would notice.** A background install replaces
//!    the bundle on disk; the running process keeps the old code until it is
//!    restarted. keeper will do that restart itself — an update nobody restarts
//!    into is an update nobody got — but only at a moment where nothing is lost:
//!    never while a recording is live, not for [`RESTART_GRACE_MS`] after the
//!    install (so a person about to restart is never beaten to it), and then only
//!    in the small hours after a short quiet spell, or after a long one at any
//!    hour. [`decide_restart`] is that judgement, and it is the only place it is
//!    made.
//! 3. **A failure is not a retry storm.** A check or download that fails backs
//!    off to [`RETRY_DELAY_MS`] rather than the full interval (a transient
//!    offline moment should not cost six hours) and never faster than that.
//!
//! The endpoint itself is unchanged and still disclosed: background checks talk
//! to exactly the [`EGRESS_UPDATE_ENDPOINT`](crate::egress::EGRESS_UPDATE_ENDPOINT)
//! already listed in Settings → About, and the artifact is still verified
//! against the committed minisign key before it is installed. Turning this off
//! (`update.auto`) is what makes the app stop contacting it unasked.

use crate::vm::AutoUpdateVm;

/// How long after launch the first background check happens.
///
/// Two minutes: long enough that the initial sync, the room list stream and the
/// timeline of whatever the person opened are already past, short enough that a
/// machine used in ten-minute bursts still gets its updates.
pub const FIRST_CHECK_DELAY_MS: i64 = 120_000;

/// The gap between background checks while the app keeps running.
///
/// Six hours: four checks a day on a machine that never sleeps, which is the
/// cadence the rest of this category ships and is well under the time it takes
/// a release to matter.
pub const CHECK_INTERVAL_MS: i64 = 6 * 60 * 60 * 1_000;

/// The gap after a failed check or download before trying again.
///
/// Shorter than the interval because the overwhelming cause is a laptop that
/// was offline for a minute, and long enough that a broken endpoint costs 48
/// requests a day rather than one a second.
pub const RETRY_DELAY_MS: i64 = 30 * 60 * 1_000;

/// Whether background updates are on for an install that has never said.
///
/// On. The alternative is a switch nobody finds, which is the behaviour this
/// module exists to replace; the update endpoint is disclosed in About, the
/// switch is beside that disclosure, and the artifact is signature-verified
/// either way.
pub const DEFAULT_ENABLED: bool = true;

/// The plan the frontend runs: whether a background install is possible here at
/// all, whether it is switched on, and on what cadence.
///
/// One place builds this, so the webview cannot invent a cadence and the two
/// surfaces that consume it (the background loop and the About switch) cannot
/// disagree about what is on.
///
/// `supported` is the shell's answer, not this module's — `keeper-core` gains no
/// platform `cfg` for it (AD-55). It exists because "install this update" is not
/// the same operation on every desktop: replacing a `.app` bundle leaves the
/// running process alone, while Windows' updater hands off to an installer that
/// requires the app to exit, which is precisely the thing a *background* install
/// may never do to somebody mid-sentence. Where `supported` is false the whole
/// background path is absent and the two-click manual control is the only one —
/// and `enabled` is reported as false, because nothing would honour it.
#[must_use]
pub fn plan(enabled: bool, supported: bool) -> AutoUpdateVm {
    AutoUpdateVm {
        supported,
        enabled: enabled && supported,
        first_check_delay_ms: FIRST_CHECK_DELAY_MS,
        check_interval_ms: CHECK_INTERVAL_MS,
        retry_delay_ms: RETRY_DELAY_MS,
        restart_check_interval_ms: RESTART_CHECK_INTERVAL_MS,
    }
}

/// How often the loop asks whether this is a good moment to restart, once a
/// build is installed and waiting.
///
/// A minute: the question is cheap (one recording-state read and a clock), and
/// the windows below are measured in minutes, so a coarser poll would sit out
/// the night window it is watching for.
pub const RESTART_CHECK_INTERVAL_MS: i64 = 60_000;

/// How long an installed build waits for a person to restart into it before
/// keeper considers doing it itself.
///
/// Half an hour. Somebody who sees "restart to finish" and reaches for ⌘Q must
/// win that race; this is the margin that lets them.
pub const RESTART_GRACE_MS: i64 = 30 * 60 * 1_000;

/// Start of the night window, in minutes after local midnight (02:00).
pub const NIGHT_START_MINUTE: i64 = 2 * 60;

/// End of the night window, in minutes after local midnight (05:00).
///
/// 02:00–05:00 rather than "after midnight": the hours somebody is most likely
/// to still be working are the ones just after midnight, and a machine woken at
/// 05:00 for the day should already be on the new build.
pub const NIGHT_END_MINUTE: i64 = 5 * 60;

/// Quiet spell required inside the night window.
///
/// Fifteen minutes. In the middle of the night this is enough to tell "asleep"
/// from "reading one message"; requiring hours would mean a laptop shut at
/// 02:30 never gets its restart.
pub const NIGHT_IDLE_MS: i64 = 15 * 60 * 1_000;

/// Quiet spell required at any other hour.
///
/// Four hours with no interaction at all — a lunch, a meeting, a machine left
/// on overnight in a different timezone than the one this code guessed. Long
/// enough that it is never "while someone is working on it".
pub const AWAY_IDLE_MS: i64 = 4 * 60 * 60 * 1_000;

/// What keeper knows when it asks whether to restart into an installed build.
///
/// Every field is measured by a caller — the shell reads the recording state and
/// the local clock, the webview measures its own idleness — and none of it is
/// decided here. `idle_ms` is time since the last interaction *with keeper*,
/// which is the only idleness a webview can honestly report; it is not a
/// system-wide idle timer, and the thresholds are chosen so that the difference
/// does not matter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestartFacts {
    /// How long the installed build has been waiting for a restart.
    pub installed_for_ms: i64,
    /// Time since the last interaction with keeper.
    pub idle_ms: i64,
    /// Minutes after local midnight, right now.
    pub minute_of_day: i64,
    /// Whether a recording session is live (capture running or winding down).
    pub recording_live: bool,
}

/// Why keeper is not restarting itself yet. Each variant is a sentence the
/// About surface can render, so "waiting" is never unexplained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartHold {
    /// A recording is live. The one absolute refusal: a restart mid-capture
    /// costs a file somebody cannot re-record.
    Recording,
    /// Inside the grace window — the person gets first refusal.
    Grace,
    /// Somebody is using keeper.
    InUse,
}

/// The answer: restart now, or hold for a named reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartVerdict {
    /// Restart into the installed build now.
    Restart,
    /// Not yet, because of this.
    Hold(RestartHold),
}

/// Whether now is a moment keeper may restart itself into an installed build.
///
/// Order matters and is the policy: a live recording refuses regardless of hour
/// or idleness; the grace window refuses next, so the person who just read
/// "restart to finish" is never beaten to it; then the night window with a short
/// quiet spell, or any hour with a long one.
#[must_use]
pub fn decide_restart(facts: RestartFacts) -> RestartVerdict {
    if facts.recording_live {
        return RestartVerdict::Hold(RestartHold::Recording);
    }
    if facts.installed_for_ms < RESTART_GRACE_MS {
        return RestartVerdict::Hold(RestartHold::Grace);
    }
    let night = facts.minute_of_day >= NIGHT_START_MINUTE && facts.minute_of_day < NIGHT_END_MINUTE;
    if night && facts.idle_ms >= NIGHT_IDLE_MS {
        return RestartVerdict::Restart;
    }
    if facts.idle_ms >= AWAY_IDLE_MS {
        return RestartVerdict::Restart;
    }
    RestartVerdict::Hold(RestartHold::InUse)
}

impl From<RestartVerdict> for crate::vm::AutoUpdateRestartVm {
    /// The verdict as the webview receives it. Here rather than in the shell so
    /// the wire shape of a hold cannot drift from the decision that produced it.
    fn from(verdict: RestartVerdict) -> Self {
        use crate::vm::AutoUpdateHold;

        match verdict {
            RestartVerdict::Restart => Self {
                restart: true,
                hold: None,
            },
            RestartVerdict::Hold(hold) => Self {
                restart: false,
                hold: Some(match hold {
                    RestartHold::Recording => AutoUpdateHold::Recording,
                    RestartHold::Grace => AutoUpdateHold::Grace,
                    RestartHold::InUse => AutoUpdateHold::InUse,
                }),
            },
        }
    }
}

/// The cadence relations the loop depends on, checked at compile time rather
/// than in a test: a first check that landed inside the boot window, a retry
/// that outran the interval, or an interval long enough to never fire on a
/// machine that is closed every evening would each turn this feature into a
/// slower version of the manual button. They are `const` facts, so a test would
/// only ever re-read what the compiler can refuse outright.
const _: () = assert!(
    FIRST_CHECK_DELAY_MS >= 30_000,
    "the first check must not compete with boot"
);
const _: () = assert!(
    FIRST_CHECK_DELAY_MS < CHECK_INTERVAL_MS,
    "a session shorter than the interval must still get one check"
);
const _: () = assert!(
    RETRY_DELAY_MS >= 60_000 && RETRY_DELAY_MS < CHECK_INTERVAL_MS,
    "a failure backs off, but by less than a whole interval"
);
const _: () = assert!(
    CHECK_INTERVAL_MS <= 24 * 60 * 60 * 1_000,
    "a machine left open must be checked at least daily"
);
const _: () = assert!(
    NIGHT_START_MINUTE < NIGHT_END_MINUTE && NIGHT_END_MINUTE <= 24 * 60,
    "the night window is a window, inside one day"
);
const _: () = assert!(
    NIGHT_IDLE_MS < AWAY_IDLE_MS,
    "the night is what buys the shorter quiet spell; equal thresholds would make it pointless"
);
const _: () = assert!(
    NIGHT_IDLE_MS > RESTART_CHECK_INTERVAL_MS,
    "the poll must be able to observe the quiet spell it waits for"
);

#[cfg(test)]
mod tests {
    use super::*;

    /// The plan is the constants plus the stored answer — nothing else, and in
    /// particular the cadence does not change with the switch, so turning
    /// background updates back on does not have to wait out a different clock.
    #[test]
    fn the_plan_carries_the_stored_answer_and_the_shipped_cadence() {
        for enabled in [true, false] {
            let plan = plan(enabled, true);
            assert!(plan.supported);
            assert_eq!(plan.enabled, enabled);
            assert_eq!(plan.first_check_delay_ms, FIRST_CHECK_DELAY_MS);
            assert_eq!(plan.check_interval_ms, CHECK_INTERVAL_MS);
            assert_eq!(plan.retry_delay_ms, RETRY_DELAY_MS);
            assert_eq!(plan.restart_check_interval_ms, RESTART_CHECK_INTERVAL_MS);
        }
    }

    /// A platform whose install cannot happen behind somebody's back reports the
    /// switch OFF however the setting reads, so no surface can render an armed
    /// background updater that nothing would run — and a machine that stored
    /// `1` before moving to such a build does not silently arm one either.
    #[test]
    fn an_unsupported_platform_is_never_enabled() {
        let plan = plan(true, false);
        assert!(!plan.supported);
        assert!(!plan.enabled);
    }

    /// The facts of a machine nobody is touching in the middle of the night —
    /// the case the whole self-restart exists for.
    fn asleep() -> RestartFacts {
        RestartFacts {
            installed_for_ms: RESTART_GRACE_MS + 1,
            idle_ms: NIGHT_IDLE_MS + 1,
            minute_of_day: NIGHT_START_MINUTE + 30,
            recording_live: false,
        }
    }

    /// A live recording refuses the restart at every hour and at every degree
    /// of idleness, because a capture cut in half is the one loss here that
    /// cannot be undone — and an idle machine is exactly what a long unattended
    /// recording looks like, which is why this is checked before anything else.
    #[test]
    fn a_live_recording_refuses_however_quiet_and_late_it_is() {
        let facts = RestartFacts {
            recording_live: true,
            idle_ms: AWAY_IDLE_MS * 10,
            ..asleep()
        };
        assert_eq!(
            decide_restart(facts),
            RestartVerdict::Hold(RestartHold::Recording)
        );
    }

    /// The person who just read "restart to finish" wins the race.
    #[test]
    fn the_grace_window_holds_even_when_everything_else_says_go() {
        let facts = RestartFacts {
            installed_for_ms: RESTART_GRACE_MS - 1,
            ..asleep()
        };
        assert_eq!(
            decide_restart(facts),
            RestartVerdict::Hold(RestartHold::Grace)
        );
        assert_eq!(decide_restart(asleep()), RestartVerdict::Restart);
    }

    /// Outside the night window a quarter of an hour is not "nobody is working
    /// on it" — a coffee is not an absence — and four hours is.
    #[test]
    fn daytime_needs_a_real_absence_and_the_night_needs_only_a_quiet_spell() {
        let noon = RestartFacts {
            minute_of_day: 12 * 60,
            ..asleep()
        };
        assert_eq!(
            decide_restart(noon),
            RestartVerdict::Hold(RestartHold::InUse)
        );
        assert_eq!(
            decide_restart(RestartFacts {
                idle_ms: AWAY_IDLE_MS,
                ..noon
            }),
            RestartVerdict::Restart
        );
    }

    /// The window's edges, because "after midnight" is the version of this that
    /// restarts under somebody still working at 00:30, and 05:00 is the hour a
    /// machine should already be on the new build rather than about to restart.
    #[test]
    fn the_night_window_excludes_late_evening_and_the_morning() {
        for minute in [0, NIGHT_START_MINUTE - 1, NIGHT_END_MINUTE, 23 * 60 + 59] {
            assert_eq!(
                decide_restart(RestartFacts {
                    minute_of_day: minute,
                    ..asleep()
                }),
                RestartVerdict::Hold(RestartHold::InUse),
                "minute {minute} is not the night window"
            );
        }
        for minute in [NIGHT_START_MINUTE, NIGHT_END_MINUTE - 1] {
            assert_eq!(
                decide_restart(RestartFacts {
                    minute_of_day: minute,
                    ..asleep()
                }),
                RestartVerdict::Restart,
                "minute {minute} is inside it"
            );
        }
    }
}
