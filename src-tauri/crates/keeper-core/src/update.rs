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
//! 2. **Never a relaunch.** A background install replaces the bundle on disk;
//!    the running process keeps the old code until the person restarts it. The
//!    update flow may say "restart to finish"; it may never restart *for* them,
//!    because the thing it would discard is an unsent message.
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
}
