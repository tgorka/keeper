//! When an Apple port replaces its recognition request (AD-411).
//!
//! A continuous on-device request is rolled onto a fresh one on the same
//! capture: after [`REQUEST_ROLL_AFTER`] at the next quiet moment, at
//! [`REQUEST_LONGEST`] regardless. A request whose audio was ended for its
//! last words is the exception: it is rolled when they come — the result
//! handler's own roll — or once [`FINAL_WORDS_WAIT`] has passed since its
//! audio ended, and never by the routine clock before then, which would
//! cancel it and lose them.

use std::time::Duration;

use super::FINAL_WORDS_WAIT;

/// After this long on one request, roll to a fresh one at the next quiet
/// moment. Under Apple's one-minute guidance for a request, with room for
/// the quiet moment to arrive.
pub const REQUEST_ROLL_AFTER: Duration = Duration::from_secs(45);

/// A request is quiet when its last transcript is this old — the pause
/// between sentences, so a roll does not cut a word in half.
pub const REQUEST_ROLL_QUIET: Duration = Duration::from_millis(1500);

/// Roll regardless of quiet after this long: somebody talking without a
/// pause for a minute is rarer than a request that should not run that long.
pub const REQUEST_LONGEST: Duration = Duration::from_secs(58);

/// What a port knows about its current request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestAge {
    /// Since the request started.
    pub age: Duration,
    /// Since its last transcript.
    pub quiet_for: Duration,
    /// Since its audio was ended for its last words; `None` while it is
    /// still being fed.
    pub finishing_for: Option<Duration>,
}

/// Whether the port replaces `request` with a fresh one now.
pub fn rolls(request: RequestAge) -> bool {
    match request.finishing_for {
        Some(finishing_for) => finishing_for >= FINAL_WORDS_WAIT,
        None => {
            request.age >= REQUEST_LONGEST
                || (request.age >= REQUEST_ROLL_AFTER && request.quiet_for >= REQUEST_ROLL_QUIET)
        }
    }
}
