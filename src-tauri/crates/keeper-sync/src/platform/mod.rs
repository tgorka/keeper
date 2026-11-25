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

pub use keeper_folder::platform::{probe_open_file_state, OpenFileState};

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

    /// Whether anything on this machine currently has `path` open.
    ///
    /// Asked by [`Engine::dehydrate_entry`](crate::engine::Engine::dehydrate_entry)
    /// before it releases a path's content back to its pointer: the release
    /// renames a small file over a large one, and a reader holding the old
    /// inode keeps reading it, but a reader that opens the path *afterwards*
    /// gets ~130 bytes of pointer text where it expected a video.
    ///
    /// # Fail-CLOSED, and that is the deliberate inverse of [`Self::free_space`]
    ///
    /// `free_space` answers `None` for "could not determine" and its doc makes
    /// callers treat that as permission to proceed: a sync that refuses to run
    /// because a `statvfs` failed is worse than one that runs out of space and
    /// says so. That is the right polarity for a guess about *capacity*.
    ///
    /// This is a guess about destroying the only local copy of somebody's
    /// bytes, so [`OpenFileState::Unknown`] **refuses**. The two guesses cost
    /// different things when they are wrong, so they point different ways, and
    /// the polarity is written down here rather than left to each caller.
    ///
    /// # What each platform answers today
    ///
    /// * **Linux** — answered. `keeper-syncd`'s `LinuxPlatform` (AD-52 makes it
    ///   a Linux-first daemon, so it is the platform that can) and the app's
    ///   `ShellSyncPlatform` on a Linux desktop both override this with one
    ///   call to [`probe_open_file_state`]: `/proc/<pid>/fd` read in-process,
    ///   matched by **device + inode identity**, no spawn, no new dependency.
    ///   `Closed` there claims exactly "no process whose descriptor table this
    ///   process is permitted to read holds this inode open" — the one stated
    ///   narrowing is a process owned by another uid, root included. Every
    ///   blind spot the walk *has* resolves to `Unknown`, which refuses; a
    ///   `hidepid=1` mount makes that every release on that host, and
    ///   `hidepid=2` deliberately does not, because it removes from the listing
    ///   exactly the processes whose tables keeper could never read anyway. See
    ///   [`probe_open_file_state`] and the walk it calls for the whole list.
    /// * **macOS / Windows** — still `Unknown`, so a release still refuses
    ///   there, and that is the honest answer rather than a skipped one. Three
    ///   options were weighed and all three rejected. `libproc` (the safe
    ///   wrapper) carries an unconditional `bindgen` **build** dependency,
    ///   which puts libclang on the critical path of every macOS release build
    ///   and ~30 crates into `Cargo.lock` for one function. Hand-written
    ///   `proc_pidfdinfo` FFI would land in the one crate that cannot be
    ///   compiled, let alone run, on the host this was written on, and `libc`
    ///   supplies `vnode_info`, `vinfo_stat` and `proc_fdinfo` but **not**
    ///   `proc_fileinfo`, so the struct carrying the answer would be declared
    ///   by hand — and a wrong layout reads the wrong offsets in silence.
    ///   `lsof` is a process spawn per candidate, absent on minimal systems,
    ///   and refused by AD-125 by name.
    /// * **iOS** — the phone links `keeper-sync` since Epic 66, and the
    ///   shared probe answers `Unknown` there, as on macOS: no process may ask
    ///   another what it holds open.
    ///
    /// # Why the DEFAULT stays `Unknown`
    ///
    /// Provided rather than required, and the body stays the refusing answer,
    /// because a platform must **opt in**. A new implementor that has not
    /// thought about what it can actually see must refuse, not guess — and
    /// moving the `/proc` walk into this body would instead make every future
    /// implementor claim an answer nobody had checked for it. [`TestPlatform`]
    /// overrides it with an injected answer, which is what makes the refusal and
    /// its absence both testable.
    ///
    /// # Why a snapshot is adequate for THIS guard
    ///
    /// Without a lease (`F_SETLEASE`, which needs `libc`, `unsafe` and file
    /// ownership) every "is it open" answer is a snapshot the instant after it
    /// is read, so TOCTOU is not the discriminator between a good answer and a
    /// bad one. Which *way* the snapshot is allowed to be wrong is, and that is
    /// what mapping every blind spot to `Unknown` decides.
    ///
    /// What makes that sufficient here, and it is the argument the whole design
    /// rests on: **this refusal does not carry NFR-40.** Refusal 1 (content
    /// identity) and refusal 3 (the per-object remote proof, taken at the
    /// moment of the deletion) do. A release is a `rename(2)` and AD-125
    /// forbids truncation, so a reader that already holds the file keeps
    /// reading the old inode intact; the harm from a missed opener is that its
    /// *next* open sees ~130 bytes of pointer text, and the content it wanted
    /// is retrievable — by asking for the path again, which materializes it
    /// from the remote that refusal 3 had just proved can serve that exact
    /// object. Not from the local store: `lfs_prune_local` defaults to on and
    /// deletes the store copy precisely when the worktree holds the content, so
    /// a materialized path is exactly the state in which the store copy may be
    /// gone, and this doc must not assume one is there.
    ///
    /// So the cost of being wrong here is a reader that has to ask again, and a
    /// download. Not a lost byte — that is what refusals 1 and 3 are for. The
    /// cost of `Unknown` forever was the entire release half of Epic 56.
    fn open_file_state(&self, _path: &Path) -> OpenFileState {
        OpenFileState::Unknown
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
    open_file_state: std::sync::Mutex<OpenFileState>,
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
            // `Closed`, not the trait's `Unknown`: a test platform that
            // refused every release by default would make the happy path of
            // `dehydrate_entry` unreachable, and the refusal has its own test
            // that sets this deliberately.
            open_file_state: std::sync::Mutex::new(OpenFileState::Closed),
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

    /// Say what the machine's open-file answer is, for the release refusals.
    ///
    /// Settable rather than fixed at construction, and for
    /// [`Self::set_utc_offset_minutes`]'s reason: the interesting case is one
    /// engine holding one platform while the answer changes, not two
    /// platforms. All three answers are reachable from here, including
    /// [`OpenFileState::Unknown`] — which is what the trait default gives, and
    /// what macOS and Windows still answer, so it must be assertable through
    /// this platform too.
    pub fn set_open_file_state(&self, state: OpenFileState) {
        *Self::lock(&self.open_file_state) = state;
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

    fn open_file_state(&self, _path: &Path) -> OpenFileState {
        *Self::lock(&self.open_file_state)
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

    /// A platform that implements only the required methods answers
    /// `Unknown` — an implementor must opt in before it claims anything.
    ///
    /// Spelled out as a bare implementation rather than asserted through
    /// [`TestPlatform`], which deliberately overrides it: what is under test
    /// is the *trait's* body, because that body is the reason a platform which
    /// has never been taught to look — macOS and Windows today — refuses
    /// instead of guessing, and the reason Story 56.11 could add the Linux
    /// answer as an opt-in rather than as a claim made on everybody's behalf.
    #[test]
    fn the_trait_default_admits_it_cannot_tell() {
        struct Bare;

        impl SyncPlatform for Bare {
            fn data_dir(&self) -> Result<PathBuf> {
                Ok(PathBuf::from("/tmp/keeper-bare"))
            }
            fn secret_get(&self, _key: &str) -> Result<Option<String>> {
                Ok(None)
            }
            fn secret_set(&self, _key: &str, _value: &str) -> Result<()> {
                Ok(())
            }
            fn secret_delete(&self, _key: &str) -> Result<()> {
                Ok(())
            }
            fn notify(&self, _title: &str, _body: &str) {}
            fn now_ms(&self) -> i64 {
                0
            }
            fn free_space(&self, _path: &Path) -> Option<u64> {
                None
            }
            fn git_program(&self) -> Result<PathBuf> {
                Ok(PathBuf::from("/usr/bin/git"))
            }
            fn host_label(&self) -> String {
                "bare".to_owned()
            }
        }

        assert_eq!(
            Bare.open_file_state(Path::new("/tmp/keeper-bare/clip.mp4")),
            OpenFileState::Unknown,
            "an implementor that has not opted in must refuse, not invent an answer"
        );
    }

    /// `TestPlatform` overrides it, so a release can be driven — and all three
    /// answers are reachable, including the trait's own.
    #[test]
    fn the_test_platform_says_closed_until_it_is_told_otherwise() {
        let p = TestPlatform::new("/tmp/keeper-test");
        let path = Path::new("/tmp/keeper-test/clip.mp4");
        assert_eq!(
            p.open_file_state(path),
            OpenFileState::Closed,
            "otherwise the happy path of a release would be unreachable in a test"
        );
        for state in [
            OpenFileState::Open,
            OpenFileState::Unknown,
            OpenFileState::Closed,
        ] {
            p.set_open_file_state(state);
            assert_eq!(p.open_file_state(path), state);
        }
    }
}
