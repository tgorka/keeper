//! Raise `RLIMIT_NOFILE` before the object store needs it.
//!
//! **A macOS app launched from Finder inherits launchd's limit, not a shell's.**
//! `launchctl limit maxfiles` is `256 unlimited` on a stock machine, while the
//! same user's terminal reports 1048576 — so a bug that never appears in
//! development appears the moment somebody double-clicks the app.
//!
//! 256 is not enough for what this crate does. gitoxide's loose object store
//! memory-maps the objects it reads, and a checkout that touches a few hundred
//! of them exhausts the descriptors mid-tree. What the user sees is:
//!
//! ```text
//! git object store failure: status failed: Could not create index from tree at
//! 3f729bcb…: Could not open or map data at '…/.git/objects/d5/64d0bc…':
//! Too many open files (os error 24)
//! ```
//!
//! and then nothing: the pull stays queued, the folder never catches up, and
//! the journal records no error because the run never got far enough to write
//! one. Observed in the field on a repository that had just gained ~200 files.
//!
//! The fix is to raise the soft limit once at startup. The hard limit is
//! `RLIM_INFINITY` on macOS, but the kernel still refuses anything above
//! `kern.maxfilesperproc`, so asking for infinity fails while asking for a sane
//! number succeeds — which is why [`target`] clamps rather than maximises.
//!
//! The syscall goes through the `rlimit` crate rather than `libc` directly:
//! this crate denies `unsafe_code`, and one `setrlimit` is not worth an
//! exception to that.

/// What a raise attempt did, for the log line and for the tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Raised {
    /// The soft limit was already at or above what we need.
    AlreadyEnough(u64),
    /// The soft limit moved from the first value to the second.
    Raised { from: u64, to: u64 },
    /// The platform has no such limit, or the call failed.
    Unchanged,
}

/// What a sync needs at once, with room to spare.
///
/// Not a measurement — a budget. The object store, the LFS filter processes,
/// SQLite, the watcher and the git child all draw from the same pool, and the
/// cost of asking for too much is zero on every system that allows it.
const WANT: u64 = 10_240;

/// The soft limit to ask for, or `None` when the current one is already fine.
///
/// Pure so the clamping rule is testable: never below the current soft limit
/// (raising is the only direction that is safe to do behind the user's back),
/// never above the hard limit, and never above what the kernel will actually
/// grant per process.
pub fn target(soft: u64, hard: u64, per_process_cap: Option<u64>) -> Option<u64> {
    if soft >= WANT {
        return None;
    }
    let mut want = WANT.min(hard);
    if let Some(cap) = per_process_cap {
        want = want.min(cap);
    }
    if want > soft {
        Some(want)
    } else {
        None
    }
}

#[cfg(unix)]
pub fn raise() -> Raised {
    use rlimit::Resource;

    let Ok((soft, hard)) = Resource::NOFILE.get() else {
        return Raised::Unchanged;
    };
    if target(soft, hard, None).is_none() {
        return Raised::AlreadyEnough(soft);
    }
    // `increase_nofile_limit` asks for what it can actually get: it clamps to
    // the hard limit and, on macOS, to `kern.maxfilesperproc`, which is the
    // ceiling that makes asking for `RLIM_INFINITY` fail outright.
    match rlimit::increase_nofile_limit(WANT) {
        Ok(now) if now > soft => Raised::Raised {
            from: soft,
            to: now,
        },
        Ok(now) => Raised::AlreadyEnough(now),
        Err(_) => Raised::Unchanged,
    }
}

#[cfg(not(unix))]
pub fn raise() -> Raised {
    Raised::Unchanged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_finder_launched_app_gets_raised() {
        // The case this module exists for: launchd hands out 256 and the hard
        // limit is unbounded.
        assert_eq!(target(256, u64::MAX, Some(245_760)), Some(WANT));
    }

    #[test]
    fn a_shell_launched_process_is_left_alone() {
        // Already generous; touching it would be noise.
        assert_eq!(target(1_048_576, u64::MAX, Some(245_760)), None);
        assert_eq!(target(WANT, u64::MAX, None), None);
    }

    #[test]
    fn the_hard_limit_is_never_exceeded() {
        // Asking above the hard limit fails the syscall outright, which would
        // turn a fixable startup into no fix at all.
        assert_eq!(target(64, 1_024, None), Some(1_024));
        assert_eq!(
            target(64, 32, None),
            None,
            "hard below soft: nothing safe to ask for"
        );
    }

    #[test]
    fn the_kernel_cap_is_never_exceeded() {
        // macOS refuses anything above `kern.maxfilesperproc` even when the
        // hard limit says infinity.
        assert_eq!(target(256, u64::MAX, Some(4_096)), Some(4_096));
        assert_eq!(target(256, u64::MAX, Some(128)), None);
    }

    #[test]
    fn raising_is_the_only_direction() {
        // A user who deliberately set a high limit keeps it.
        assert_eq!(target(50_000, u64::MAX, Some(245_760)), None);
    }
}
