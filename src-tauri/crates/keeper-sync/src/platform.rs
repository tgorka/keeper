//! The `SyncPlatform` port (Story 23.2, AD-40).
//!
//! `keeper-sync` reaches the OS only through this trait, exactly as
//! `keeper-core` reaches it only through `keeper_core::platform::Platform`. It
//! is a *separate* port rather than a reuse of that one because AD-40 keeps
//! this crate free of `keeper-core` — otherwise `keeper-syncd` would link
//! matrix-sdk on a headless server.
//!
//! Two implementations exist: the Tauri shell delegates to its existing
//! `DesktopPlatform`, and `keeper-syncd` implements it directly against XDG
//! paths and the OS keyring. A third, `TestPlatform`, lives here so unit tests
//! never touch the real keychain or the real clock.

use std::path::{Path, PathBuf};

use crate::error::{Result, SyncError};

/// Everything the engine needs from the outside world.
///
/// Object-safe on purpose: the engine holds `Arc<dyn SyncPlatform>` so a
/// profile supervisor can be spawned without threading a generic through every
/// type. Nothing here does real work — each method is a thin capability the
/// host already has.
pub trait SyncPlatform: Send + Sync {
    /// Where `sync.db` and engine-owned state live.
    fn data_dir(&self) -> Result<PathBuf>;

    /// Read a stored secret. `key` is an opaque engine-chosen identifier
    /// (`sync/<profile-id>/token`), never the secret itself.
    ///
    /// `Ok(None)` means "no such secret", which is an ordinary state — a
    /// profile on a public repository has none.
    fn secret_get(&self, key: &str) -> Result<Option<String>>;

    /// Store a secret. Implementations MUST NOT write it anywhere the engine's
    /// own persistence can see it (never `sync.db`, never `config.json`).
    fn secret_set(&self, key: &str, value: &str) -> Result<()>;

    /// Remove a secret. Removing an absent secret succeeds.
    fn secret_delete(&self, key: &str) -> Result<()>;

    /// Raise a user-visible notification. Best-effort by contract: a host with
    /// no notifier returns `Ok(())` rather than failing a sync.
    fn notify(&self, title: &str, body: &str);

    /// Wall-clock milliseconds since the Unix epoch.
    ///
    /// Injected rather than read directly so the quiescence gate (Story 26.3)
    /// and the scheduler (Story 26.6) are testable without sleeping. Must be
    /// wall-clock, not monotonic: the scheduler has to reason about time that
    /// passed while the process was not running.
    fn now_ms(&self) -> i64;

    /// Minutes this machine's local wall clock is ahead of UTC, right now.
    ///
    /// [`Self::now_ms`] is deliberately UTC — an instant, comparable across
    /// machines and across a suspend — but two things a person configures are
    /// not instants at all. A recordings push window (`22:00`–`06:00`, see
    /// [`crate::profile::PushPolicy::Window`]) is a *local wall-clock* range,
    /// and reading it against UTC would open the quiet hours at the wrong
    /// moment for everyone who is not on Greenwich.
    ///
    /// Provided rather than required, and this is the one method here with a
    /// body, because every real host answers it identically — "whatever zone
    /// this machine is in" — and `gix` already carries the zone database that
    /// answers it. A required method would have made three implementations
    /// write the same line. What the default cannot do is *lie*, which is
    /// exactly what a test needs: [`TestPlatform`] overrides it so a window
    /// test is the same test in Reykjavík and in Auckland.
    ///
    /// East of UTC is positive, matching git's own `+0200` sign convention.
    fn utc_offset_minutes(&self) -> i32 {
        machine_utc_offset_minutes()
    }

    /// Free space on the volume holding `path`, in bytes.
    ///
    /// `None` means "could not determine" and callers MUST treat that as
    /// permission to proceed — the recording subsystem's fail-open precedent.
    /// A sync that refuses to run because a statvfs failed is worse than one
    /// that runs out of space and says so.
    fn free_space(&self, path: &Path) -> Option<u64>;

    /// Absolute path to a usable `git` binary, or why there isn't one.
    ///
    /// AD-41 makes this a hard prerequisite: push, the three merge entry points,
    /// `is_ancestor`, branch handling, the worktree lanes, sparse-checkout and
    /// gc have no in-process implementation.
    ///
    /// **Usable**, not merely present. An implementation MUST answer with a
    /// binary that clears [`git::cli::MIN_GIT_MAJOR`]/[`MIN_GIT_MINOR`], which
    /// means probing candidates rather than taking the first file named `git`
    /// (see [`git::resolve`](crate::git::resolve) for what that costs on a real
    /// machine, and why). [`Engine::open`](crate::engine::Engine::open) probes
    /// again and refuses below the floor, so a host that returns a too-old git
    /// has not degraded sync — it has built a surface the engine will not serve.
    ///
    /// [`git::cli::MIN_GIT_MAJOR`]: crate::git::cli::MIN_GIT_MAJOR
    /// [`MIN_GIT_MINOR`]: crate::git::cli::MIN_GIT_MINOR
    fn git_program(&self) -> Result<PathBuf>;

    /// Stable label for this machine, used in provenance trailers and conflict
    /// filenames (AD-43, AD-44). A hostname is the usual answer.
    fn host_label(&self) -> String;
}

/// This machine's current UTC offset in minutes, east-positive.
///
/// Via `gix`, which already resolves the zone database for the commit
/// signatures it writes, so this adds no dependency and no second idea of what
/// time it is here. A machine whose zone cannot be resolved answers UTC, which
/// is `gix`'s own fallback and the only answer available when the question has
/// no data behind it.
///
/// Seconds are discarded rather than rounded: no political time zone has ever
/// had a sub-minute offset since 1972, and the only consumer compares against
/// an `HH:MM` a person typed.
///
/// `pub(crate)` rather than private because a second consumer arrived that is
/// not a `SyncPlatform` implementation: a freedesktop.org `.trashinfo`
/// `DeletionDate` is a *local* wall-clock stamp with no zone on it
/// ([`crate::files_write::local_now_ms`]), and the Files commands that write
/// one hold no platform port to ask.
pub(crate) fn machine_utc_offset_minutes() -> i32 {
    gix::date::Time::now_local_or_utc().offset / 60
}

/// A wall-clock millisecond count as `(year, month, day, hour, minute, second)`.
///
/// Hand-rolled because `keeper-sync` deliberately has no `chrono`: the engine
/// is time-agnostic and takes wall-clock milliseconds from this port. The
/// civil-date arithmetic is Howard Hinnant's `days_from_civil` inverse, which
/// is exact for every date we can represent.
///
/// **One copy, and it lives here because this is where time enters the crate.**
/// Two consumers now format an instant into words a person reads — the
/// conflict filename `<crate::engine>` stamps and the `.trashinfo` stamp
/// `<crate::files_write>` writes — and they want different separators over the
/// identical decomposition. Two copies of a leap-year calculation is two
/// chances to get 2100 wrong, and only one of them would be found.
///
/// Takes whatever `ms` it is given and says nothing about its zone: the caller
/// decides whether it is handing over UTC or a local clock, because the two
/// consumers genuinely differ (a conflict filename must be comparable across
/// machines; a `DeletionDate` is defined as local time).
pub(crate) fn civil_from_unix_ms(ms: i64) -> (i64, u32, u32, u32, u32, u32) {
    let secs = ms.div_euclid(1_000);
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let (hour, minute, second) = (tod / 3_600, (tod % 3_600) / 60, tod % 60);

    // Shift the epoch to 0000-03-01 so leap days land at the end of the cycle.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = era * 400 + yoe + i64::from(month <= 2);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    (
        year,
        month as u32,
        day as u32,
        hour as u32,
        minute as u32,
        second as u32,
    )
}

/// An in-memory `SyncPlatform` for unit tests.
///
/// Lives in the library rather than behind `#[cfg(test)]` so the sibling crates
/// (`keeper`, `keeper-syncd`) can use it in their own tests without duplicating
/// it. Cheap enough that this costs nothing in a release build beyond a few
/// unreferenced types.
#[derive(Debug)]
pub struct TestPlatform {
    data_dir: PathBuf,
    secrets: std::sync::Mutex<std::collections::HashMap<String, String>>,
    /// Notifications raised, so tests can assert the loud-failure contract.
    pub notifications: std::sync::Mutex<Vec<(String, String)>>,
    now_ms: std::sync::atomic::AtomicI64,
    utc_offset_minutes: std::sync::atomic::AtomicI32,
    free_space: Option<u64>,
    git: Option<PathBuf>,
}

impl TestPlatform {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            secrets: std::sync::Mutex::new(std::collections::HashMap::new()),
            notifications: std::sync::Mutex::new(Vec::new()),
            now_ms: std::sync::atomic::AtomicI64::new(1_700_000_000_000),
            // UTC, so a test that reasons about wall-clock times reasons about
            // the same ones on every machine that runs it.
            utc_offset_minutes: std::sync::atomic::AtomicI32::new(0),
            free_space: Some(100 * 1024 * 1024 * 1024),
            git: Some(PathBuf::from("/usr/bin/git")),
        }
    }

    /// Simulate a machine with no usable git (Story 23.5).
    pub fn without_git(mut self) -> Self {
        self.git = None;
        self
    }

    /// Point the port at one specific binary, for tests about resolution.
    ///
    /// Absent this, `git_program` answers `/usr/bin/git` — fine for the engine
    /// tests that only need *a* git, useless for asserting that the engine
    /// refuses the binaries [`crate::git::resolve`] rejects.
    pub fn with_git(mut self, program: impl Into<PathBuf>) -> Self {
        self.git = Some(program.into());
        self
    }

    /// Advance the injected clock, for quiescence and backoff tests.
    pub fn advance_ms(&self, delta: i64) {
        self.now_ms
            .fetch_add(delta, std::sync::atomic::Ordering::SeqCst);
    }

    /// Put the test machine in a zone, for the recordings push window.
    ///
    /// Settable rather than fixed at construction, and settable *while the
    /// engine holds this platform*, because the interesting case is a session
    /// that starts outside the quiet hours and ends inside them — which is one
    /// clock and one zone, not two platforms.
    pub fn set_utc_offset_minutes(&self, minutes: i32) {
        self.utc_offset_minutes
            .store(minutes, std::sync::atomic::Ordering::SeqCst);
    }

    /// Poison-tolerant lock: a panicking test must not cascade into every other
    /// assertion in the same process.
    fn lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
        mutex
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl SyncPlatform for TestPlatform {
    fn data_dir(&self) -> Result<PathBuf> {
        Ok(self.data_dir.clone())
    }

    fn secret_get(&self, key: &str) -> Result<Option<String>> {
        Ok(Self::lock(&self.secrets).get(key).cloned())
    }

    fn secret_set(&self, key: &str, value: &str) -> Result<()> {
        Self::lock(&self.secrets).insert(key.to_owned(), value.to_owned());
        Ok(())
    }

    fn secret_delete(&self, key: &str) -> Result<()> {
        Self::lock(&self.secrets).remove(key);
        Ok(())
    }

    fn notify(&self, title: &str, body: &str) {
        Self::lock(&self.notifications).push((title.to_owned(), body.to_owned()));
    }

    fn now_ms(&self) -> i64 {
        self.now_ms.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn utc_offset_minutes(&self) -> i32 {
        self.utc_offset_minutes
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    fn free_space(&self, _path: &Path) -> Option<u64> {
        self.free_space
    }

    fn git_program(&self) -> Result<PathBuf> {
        self.git.clone().ok_or_else(|| SyncError::GitMissing {
            reason: "no git binary in the test platform".to_owned(),
        })
    }

    fn host_label(&self) -> String {
        "test-host".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_round_trip_and_delete_is_idempotent() {
        let p = TestPlatform::new("/tmp/keeper-test");
        assert_eq!(p.secret_get("k").expect("get"), None);
        p.secret_set("k", "v").expect("set");
        assert_eq!(p.secret_get("k").expect("get"), Some("v".to_owned()));
        p.secret_delete("k").expect("delete");
        p.secret_delete("k")
            .expect("deleting an absent secret must succeed");
        assert_eq!(p.secret_get("k").expect("get"), None);
    }

    #[test]
    fn missing_git_is_reported_as_gitmissing_not_a_panic() {
        let p = TestPlatform::new("/tmp/keeper-test").without_git();
        let err = p.git_program().expect_err("must fail");
        assert_eq!(err.code(), "gitMissing");
        assert!(err.needs_user_action());
    }

    #[test]
    fn clock_is_injected_so_tests_never_sleep() {
        let p = TestPlatform::new("/tmp/keeper-test");
        let before = p.now_ms();
        p.advance_ms(5_000);
        assert_eq!(p.now_ms() - before, 5_000);
    }
}
