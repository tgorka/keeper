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

/// Whether anything on this machine currently has a file open.
///
/// Three answers rather than a boolean, because the third one is a real state
/// this machine can be in — a platform with no descriptor table to read, a
/// procfs that hides processes — and a boolean would have to lie about it, in
/// the one direction that deletes data (AD-125). See
/// [`SyncPlatform::open_file_state`] for what each platform answers today, and
/// [`probe_open_file_state`] for the answer Linux gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenFileState {
    /// Some process holds a descriptor on it.
    Open,
    /// Nothing holds a descriptor on it.
    Closed,
    /// This platform cannot answer race-free, so it does not claim to.
    Unknown,
}

/// Whether anything on this machine currently has `path` open.
///
/// **The shared implementation both shipping platforms delegate to** (Story
/// 56.11): `LinuxPlatform` in `keeper-syncd` and `ShellSyncPlatform` in
/// `keeper` each override [`SyncPlatform::open_file_state`] with one call to
/// this function, so the daemon and the app cannot disagree about one folder on
/// one machine.
///
/// A free function rather than the trait's default body, and that is the whole
/// design: an implementor must **opt in**. [`TestPlatform`] keeps its injected
/// answer, and any future platform keeps the honest
/// [`OpenFileState::Unknown`] — which refuses — until somebody has thought
/// about what it can actually see. A default that reached for `/proc` would
/// silently make every new implementor claim an answer it had never checked.
///
/// On Linux the answer comes from [`open_file_state_under_proc`]. Everywhere
/// else it is `Unknown`, so the release keeps refusing; see
/// [`SyncPlatform::open_file_state`] for why macOS is not answered here and
/// what a macOS answer would cost.
#[cfg(target_os = "linux")]
pub fn probe_open_file_state(path: &Path) -> OpenFileState {
    open_file_state_under_proc(Path::new(PROC_ROOT), path)
}

/// Whether anything on this machine currently has `path` open — `Unknown` on
/// every target that is not Linux.
///
/// Not a stub: `Unknown` **refuses** (AD-125), so this is the answer that keeps
/// macOS and Windows fail-closed rather than the answer that makes them guess.
/// See the Linux arm and [`SyncPlatform::open_file_state`].
#[cfg(not(target_os = "linux"))]
pub fn probe_open_file_state(path: &Path) -> OpenFileState {
    // Bound rather than named `_path`, so the signature reads identically to
    // the Linux arm's and a reader comparing the two sees one difference.
    let _ = path;
    OpenFileState::Unknown
}

/// The procfs mount every Linux host has, and the only one production asks.
///
/// Named rather than inlined because [`open_file_state_under_proc`] takes the
/// root as a parameter: a unit test can then point the walk at a directory that
/// is *not* procfs and assert that it refuses, which is the only way to cover
/// the "no procfs" and "a procfs that hides this process" branches without a
/// container.
#[cfg(target_os = "linux")]
const PROC_ROOT: &str = "/proc";

/// Walk `proc_root` looking for a descriptor on the same file as `path`.
///
/// # Identity, not paths
///
/// A match is `st_dev` + `st_ino` of `stat("<proc_root>/<pid>/fd/<n>")`, which
/// procfs resolves through the magic link to the **open file's** inode. That is
/// the kernel's own answer, and it is why no `read_link` string comparison
/// appears here. Every one of the four ways a string comparison is wrong is
/// dissolved by asking for the inode instead:
///
/// * the `read_link` of a deleted-but-open file carries a ` (deleted)` suffix,
///   and `/proc` gives no way to tell that suffix from a filename that
///   genuinely ends in it;
/// * a process in another mount namespace reports a different path prefix for
///   the same file;
/// * a hardlink is the same file under a second name, and a string comparison
///   would miss the opener entirely;
/// * `..` segments and symlink spellings make two different strings name one
///   file, so a comparison would need a `canonicalize` this does not.
///
/// # What `Closed` claims, exactly
///
/// **No process whose descriptor table this process is permitted to read holds
/// this inode open.** `<pid>/fd` is mode `0500`, so a process owned by another
/// uid — root included — is a blind spot, and this is the one narrowing the
/// answer carries. It is stated rather than discovered because the alternative
/// is worse: demanding *total* completeness makes the answer `Unknown` on every
/// Linux box that has ever booted (pid 1 alone guarantees it), which is the
/// refuse-everything failure mode Story 56.11 exists to end.
///
/// What is **not** narrowed is a process keeper cannot even *identify*: a
/// `<pid>` that is listed but cannot be entered, so even its world-readable
/// `stat` file is unreadable. That is the `hidepid=1` shape, and on such a host
/// keeper refuses every release — correctly, because it can see that there are
/// processes it knows nothing about.
///
/// `hidepid=2` is the opposite case and it is worth being exact about, because
/// the obvious reading is wrong: it makes other users' `<pid>` directories
/// *invisible* rather than merely unreadable, so an unprivileged keeper
/// enumerates only its own user's processes — every one of which it can read in
/// full. The walk therefore answers `Closed` there, and that answer is inside
/// the claim above rather than a violation of it: the processes that vanished
/// from the listing are exactly the ones whose descriptor tables keeper was
/// never permitted to read.
///
/// # Whose process table is this, anyway
///
/// The self-check does not compare against [`std::process::id`]: that is this
/// process's pid in *its* PID namespace, and `proc_root` may be a procfs from
/// another one — a container that bind-mounts a foreign `/proc`, or a process
/// that entered a new namespace without remounting. An unrelated process
/// carrying the same number would then satisfy a numeric comparison and the
/// check would be vacuously true over a process table that says nothing about
/// us. So `own` is read from `<proc_root>/self`, the magic link procfs resolves
/// to *the reader's own* directory in *that* procfs. A root with no resolvable
/// `self` is not a procfs this walk can reason about, and answers `Unknown`.
///
/// Requiring to then find that pid, and to read its descriptor table, is a free
/// self-check that the fd-reading limb works at all: a walk that never once
/// managed to read a descriptor table must not report the same word as a walk
/// that read every one of them.
///
/// # Every blind spot refuses
///
/// `Open` is returned on the first match and is definitive. Everything the walk
/// cannot see resolves to `Unknown`, which
/// [`Engine::dehydrate_entry`](crate::engine::Engine::dehydrate_entry) turns
/// into a refusal: a target that cannot be stat'ed, a `proc_root` that cannot
/// be read or has no resolvable `self`, a dirent that cannot be enumerated
/// (in `<proc_root>` *or* inside a `<pid>/fd`), a `<pid>` that cannot be
/// entered, a descriptor whose `stat` fails for any reason but "it is gone",
/// and any `read_dir` error that is neither "gone" nor "not yours".
///
/// The discipline is applied at every level on purpose. A partially enumerated
/// descriptor table is the same defect as a partially enumerated `/proc`: it
/// finds no opener because it stopped looking, and reporting that as `Closed`
/// is the one direction AD-125 forbids.
#[cfg(target_os = "linux")]
fn open_file_state_under_proc(proc_root: &Path, path: &Path) -> OpenFileState {
    use std::os::unix::fs::MetadataExt as _;

    // `metadata` and not `symlink_metadata`: whoever has this file open opened
    // the *resolved* file, so the identity to compare against is the target's,
    // never a symlink's own inode.
    let Ok(target) = std::fs::metadata(path) else {
        return OpenFileState::Unknown;
    };
    let (dev, ino) = (target.dev(), target.ino());

    // Who this walk is, in the procfs it is about to read. See this function's
    // doc: a numeric comparison against `std::process::id()` would be answering
    // about a different namespace's process table.
    let own = std::fs::read_link(proc_root.join("self"))
        .ok()
        .and_then(|link| {
            link.file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.parse::<u32>().ok())
        });
    let Some(own) = own else {
        return OpenFileState::Unknown;
    };

    let Ok(processes) = std::fs::read_dir(proc_root) else {
        return OpenFileState::Unknown;
    };

    let mut own_seen = false;

    for entry in processes {
        // A `/proc` this process cannot enumerate is a blind spot, not an empty
        // machine.
        let Ok(entry) = entry else {
            return OpenFileState::Unknown;
        };
        // `/proc` carries `self`, `net`, `meminfo` and dozens more; a numeric
        // name is the definition of a process entry.
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };

        match std::fs::read_dir(entry.path().join("fd")) {
            Ok(fds) => {
                for fd in fds {
                    // Same rule as the outer loop, and for the same reason: a
                    // descriptor table that stopped part-way through (EIO off a
                    // failing procfs, ENOMEM under pressure) has not been
                    // examined, and an unexamined table may not read as "this
                    // process holds nothing".
                    let Ok(fd) = fd else {
                        return OpenFileState::Unknown;
                    };
                    let open = match std::fs::metadata(fd.path()) {
                        Ok(open) => open,
                        // A descriptor that vanished between the `read_dir` and
                        // the `stat` is a descriptor that holds nothing.
                        Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                        // Anything else — ESTALE off a network mount, EIO —
                        // means this descriptor was never compared, so the
                        // walk cannot claim it did not match.
                        Err(_) => return OpenFileState::Unknown,
                    };
                    if open.ino() == ino && open.dev() == dev {
                        return OpenFileState::Open;
                    }
                }
                if pid == own {
                    own_seen = true;
                }
            }
            Err(err) => match err.kind() {
                // The process exited while the walk was running. It holds
                // nothing, so it changes no answer.
                std::io::ErrorKind::NotFound => {}
                std::io::ErrorKind::PermissionDenied => {
                    // Another uid's descriptor table: `<pid>/fd` is `0500`.
                    // This is the one documented narrowing of `Closed`, and the
                    // scan continues rather than refusing — see this function's
                    // doc for why demanding completeness answers `Unknown`
                    // everywhere.
                    //
                    // But a process keeper cannot even IDENTIFY is a different
                    // thing and a real blind spot: `hidepid=1` makes `<pid>`
                    // itself unenterable, and then its `stat` file — which is
                    // world-readable on every ordinary Linux — cannot be read
                    // either. That distinguishes "somebody else's process" from
                    // "a process hidden from us", and only the second one is
                    // grounds to stop claiming anything.
                    if std::fs::metadata(entry.path().join("stat"))
                        .err()
                        .is_some_and(|err| err.kind() == std::io::ErrorKind::PermissionDenied)
                    {
                        return OpenFileState::Unknown;
                    }
                }
                // Anything else — a procfs that answered EIO, an `/proc`
                // replaced by something that is not one — is unclassified, and
                // an unclassified failure may not read as "nobody has it open".
                _ => return OpenFileState::Unknown,
            },
        }
    }

    if own_seen {
        OpenFileState::Closed
    } else {
        // `self` named a pid the enumeration did not produce, or whose
        // descriptor table did not read: this walk never established that it
        // can see anything, so it claims nothing.
        OpenFileState::Unknown
    }
}

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
    /// * **iOS** — the question cannot be asked at all. The `keeper` manifest
    ///   gates `keeper-sync` behind
    ///   `cfg(not(any(target_os = "ios", target_os = "android")))`, so no
    ///   `SyncPlatform` is linked there to ask.
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

    // -----------------------------------------------------------------------
    // The real probe, over the real `/proc` (Story 56.11)
    // -----------------------------------------------------------------------

    /// Whether this host publishes a readable process table at all.
    ///
    /// A capability skip in the manner of `tests/dehydrate_entry.rs`'s
    /// `if !init_repo(root) { return }`: on a Linux box whose `/proc` is masked
    /// or mounted `hidepid=1` (a hardened container, some CI sandboxes) the
    /// probe correctly answers `Unknown` and the assertions below have nothing
    /// falsifiable to say.
    ///
    /// Deliberately phrased as facts about the **host** — two `read_dir` calls
    /// std makes, not one call into the code under test — because a guard that
    /// asked `probe_open_file_state` whether it works would pass silently over
    /// a probe that had stopped working, which is the entire defect this story
    /// fixes.
    #[cfg(target_os = "linux")]
    fn procfs_is_readable() -> bool {
        std::fs::read_dir(PROC_ROOT).is_ok() && std::fs::read_dir("/proc/self/fd").is_ok()
    }

    /// A probe root that stands in for procfs: `self` resolving to this
    /// process's own (readable, empty) descriptor table, and nothing else.
    ///
    /// The walk reads `<proc_root>/self` to learn whose table it is looking at
    /// — see [`open_file_state_under_proc`] — so a fixture root without that
    /// link answers `Unknown` before it looks at anything, which is right but
    /// proves nothing about the branches below it.
    #[cfg(target_os = "linux")]
    fn answerable_proc_root(at: &Path) -> PathBuf {
        let pid = std::process::id().to_string();
        std::fs::create_dir_all(at.join(&pid).join("fd")).expect("this process's pid directory");
        std::os::unix::fs::symlink(&pid, at.join("self")).expect("the `self` magic link");
        at.to_path_buf()
    }

    /// A descriptor this test holds makes the answer `Open`; dropping it makes
    /// the same path answer `Closed`.
    ///
    /// **One test for both halves, so the pair cannot drift.** A probe that
    /// always answered `Open` would satisfy the first assertion and a probe
    /// that always answered `Closed` would satisfy the second; only asserting
    /// both against one path in one process rules out both. The first half is
    /// the bug this story exists to prevent — a release that deletes content
    /// somebody is reading — and it fails with the `Open` return removed from
    /// the walk. The second half is the bug the story exists to *fix*: an
    /// answer that never says `Closed` refuses every release forever.
    #[test]
    #[cfg(target_os = "linux")]
    fn a_descriptor_this_test_holds_makes_the_answer_open() {
        if !procfs_is_readable() {
            return;
        }
        let work = tempfile::tempdir().expect("temp dir");
        let path = work.path().join("clip.mp4");
        std::fs::write(&path, b"the content a reader is part-way through").expect("write");

        let held = std::fs::File::open(&path).expect("hold a real descriptor");
        assert_eq!(
            probe_open_file_state(&path),
            OpenFileState::Open,
            "this process is in /proc and its own descriptor table names this inode"
        );

        drop(held);
        assert_eq!(
            probe_open_file_state(&path),
            OpenFileState::Closed,
            "and with nothing holding it the release must be allowed to proceed"
        );
    }

    /// A file nobody has ever opened answers `Closed`.
    ///
    /// Fails for the defect Story 56.11 was written against: an answer that is
    /// `Unknown` (or `Open`) unconditionally makes `dehydrate` and the TTL
    /// sweep refuse `OpenUnknown` on every host that could have answered,
    /// which is Epic 56's release half never running once.
    #[test]
    #[cfg(target_os = "linux")]
    fn a_file_nobody_has_open_is_not_open() {
        if !procfs_is_readable() {
            return;
        }
        let work = tempfile::tempdir().expect("temp dir");
        let path = work.path().join("fresh.mp4");
        std::fs::write(&path, b"written and closed again").expect("write");

        assert_eq!(
            probe_open_file_state(&path),
            OpenFileState::Closed,
            "the walk completed and saw this process, so it may say so"
        );
    }

    /// A descriptor on a *different* file with the identical name, length and
    /// bytes does not make the other one look open.
    ///
    /// Fails for a probe that compares `read_link` strings or sizes instead of
    /// `st_dev` + `st_ino`. That defect is the expensive direction: it refuses
    /// a release for a path nothing is holding, forever, whenever some
    /// unrelated process happens to hold a file of the same name — and every
    /// media folder on a machine is full of `clip.mp4`.
    #[test]
    #[cfg(target_os = "linux")]
    fn a_descriptor_on_a_different_file_is_not_confused() {
        if !procfs_is_readable() {
            return;
        }
        let work = tempfile::tempdir().expect("temp dir");
        let held_dir = work.path().join("held");
        let other_dir = work.path().join("other");
        std::fs::create_dir(&held_dir).expect("held dir");
        std::fs::create_dir(&other_dir).expect("other dir");

        // Identical name, identical length, identical bytes. Only the inode
        // differs, which is the only thing the probe is allowed to look at.
        let content = vec![7u8; 64 * 1024];
        let held_path = held_dir.join("clip.mp4");
        let other_path = other_dir.join("clip.mp4");
        std::fs::write(&held_path, &content).expect("write the held file");
        std::fs::write(&other_path, &content).expect("write the twin");

        let held = std::fs::File::open(&held_path).expect("hold a real descriptor");
        assert_eq!(
            probe_open_file_state(&other_path),
            OpenFileState::Closed,
            "identity is dev+ino, so the twin is releasable while its double is held"
        );
        assert_eq!(
            probe_open_file_state(&held_path),
            OpenFileState::Open,
            "and the one actually held is still refused — the two must not have swapped"
        );
        drop(held);
    }

    /// A probe root that is not a working procfs answers `Unknown` — absent,
    /// empty, or naming a `self` the enumeration never produces.
    ///
    /// Fails for the worst defect this file can carry: a walk that finds no
    /// opener because it could not look, and reports that as `Closed`. Over an
    /// empty visible process set "nobody has it open" is vacuous, and a
    /// dangling `self` is a root that claims to be about a process the
    /// enumeration cannot show. The root is a parameter of
    /// [`open_file_state_under_proc`] precisely so these three are reachable
    /// without a container.
    ///
    /// It does **not** stand in for `hidepid=2`: that setting leaves this
    /// process visible and readable, so the walk answers `Closed` there, and
    /// the function's own doc says so.
    #[test]
    #[cfg(target_os = "linux")]
    fn a_probe_root_that_is_not_procfs_cannot_answer() {
        use std::os::unix::fs::PermissionsExt as _;

        let work = tempfile::tempdir().expect("temp dir");
        let file = work.path().join("clip.mp4");
        std::fs::write(&file, b"content").expect("write");

        assert_eq!(
            open_file_state_under_proc(&work.path().join("no-such-proc"), &file),
            OpenFileState::Unknown,
            "no procfs at all: the question was never asked, so nothing may be claimed"
        );

        let empty = work.path().join("empty-proc");
        std::fs::create_dir(&empty).expect("empty proc root");
        assert_eq!(
            open_file_state_under_proc(&empty, &file),
            OpenFileState::Unknown,
            "nothing here says whose process table this is, so it is not one"
        );

        // A `self` link is present but names a process the listing does not
        // contain: the walk never establishes that it can read a descriptor
        // table, so `own_seen` stays false.
        let dangling = work.path().join("dangling-proc");
        std::fs::create_dir(&dangling).expect("dangling proc root");
        std::os::unix::fs::symlink("9999999", dangling.join("self")).expect("dangling self");
        assert_eq!(
            open_file_state_under_proc(&dangling, &file),
            OpenFileState::Unknown,
            "a `self` the enumeration cannot show is a walk that proved nothing"
        );

        // A root whose `self` link resolves — search permission is enough for
        // that — but which cannot be listed. Mode `0o111` is the only way to
        // reach the enumeration guard separately from the `self` guard, since
        // the `self` read comes first.
        let unlistable = answerable_proc_root(&work.path().join("unlistable-proc"));
        let restore = || {
            std::fs::set_permissions(&unlistable, std::fs::Permissions::from_mode(0o700))
                .expect("restore the mode so the temp directory can be removed");
        };
        std::fs::set_permissions(&unlistable, std::fs::Permissions::from_mode(0o111))
            .expect("make the root searchable but not listable");
        // uid 0 ignores the DAC check, so there is nothing falsifiable on a root
        // runner. Read the answer before asserting, or a failure leaks the
        // directory (`TempDir::drop` swallows the error by design).
        if std::fs::read_dir(&unlistable).is_err() {
            let observed = open_file_state_under_proc(&unlistable, &file);
            restore();
            assert_eq!(
                observed,
                OpenFileState::Unknown,
                "a process table that cannot be enumerated is not an empty machine"
            );
        } else {
            restore();
        }
    }

    /// A target this process cannot `stat` cannot be answered about.
    ///
    /// The identity the whole walk compares against comes from one `metadata`
    /// call on the path. Fails for a probe that treats an unreadable target as
    /// "no inode, therefore no match, therefore `Closed`" — which would be the
    /// answer for a path on a disk that is failing, and the one direction that
    /// deletes data.
    #[test]
    #[cfg(target_os = "linux")]
    fn a_target_that_cannot_be_stat_ed_cannot_be_answered_about() {
        let work = tempfile::tempdir().expect("temp dir");
        let root = answerable_proc_root(&work.path().join("proc"));

        assert_eq!(
            open_file_state_under_proc(&root, &work.path().join("no-such-file")),
            OpenFileState::Unknown,
            "with no inode to compare against, the walk has asked nothing"
        );
    }

    /// A descriptor table keeper cannot read *through* answers `Unknown`.
    ///
    /// Two branches that a chmod cannot express, built out of what a filesystem
    /// can: a `<pid>/fd` that is not a directory at all — `ENOTDIR`, which is
    /// neither "gone" nor "not yours", the shape a `/proc` replaced by something
    /// that is not one has — and a descriptor entry whose `stat` fails for a
    /// reason other than "it is gone", which is the `ESTALE`-off-a-network-mount
    /// and `EIO`-off-a-failing-disk shape.
    ///
    /// Both fail for the same defect and it is the expensive one: a descriptor
    /// that was never compared read as a descriptor that did not match, so a
    /// truncated scan speaks with the confidence of a complete one (AD-125).
    #[test]
    #[cfg(target_os = "linux")]
    fn a_descriptor_table_that_cannot_be_read_through_refuses() {
        use std::os::unix::fs::PermissionsExt as _;

        let work = tempfile::tempdir().expect("temp dir");
        let file = work.path().join("clip.mp4");
        std::fs::write(&file, b"content").expect("write");

        // `<pid>/fd` is a regular file: `read_dir` answers `ENOTDIR`.
        let not_a_dir = answerable_proc_root(&work.path().join("notdir-proc"));
        std::fs::create_dir(not_a_dir.join("1234")).expect("a second pid directory");
        std::fs::write(not_a_dir.join("1234").join("fd"), b"not a directory")
            .expect("fd as a file");
        assert_eq!(
            open_file_state_under_proc(&not_a_dir, &file),
            OpenFileState::Unknown,
            "an unclassified failure to read a descriptor table is not an empty one"
        );

        // A descriptor entry pointing into an unsearchable directory: `stat`
        // answers `EACCES`, which is not "it is gone".
        let locked = work.path().join("locked");
        std::fs::create_dir(&locked).expect("locked dir");
        std::fs::write(locked.join("held"), b"content").expect("the file behind the lock");
        let unreachable = answerable_proc_root(&work.path().join("estale-proc"));
        let own_fd = unreachable.join(std::process::id().to_string()).join("fd");
        std::os::unix::fs::symlink(locked.join("held"), own_fd.join("3")).expect("descriptor 3");
        let restore = || {
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700))
                .expect("restore the mode so the temp directory can be removed");
        };
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000))
            .expect("lock the directory");
        // uid 0 ignores the DAC check, so there is nothing falsifiable on a root
        // runner. Read the answer before asserting, or a failure leaks the
        // directory (`TempDir::drop` swallows the error by design).
        if std::fs::metadata(own_fd.join("3")).is_err() {
            let observed = open_file_state_under_proc(&unreachable, &file);
            restore();
            assert_eq!(
                observed,
                OpenFileState::Unknown,
                "a descriptor the walk could not compare is not a descriptor that did not match"
            );
        } else {
            restore();
        }
    }

    /// A process keeper cannot even identify makes the answer `Unknown`, over a
    /// probe root that is otherwise perfectly capable of answering `Closed`.
    ///
    /// The `hidepid=1` shape, built for real: a `<pid>` directory with mode
    /// `000`, so both `<pid>/fd` and the ordinarily world-readable
    /// `<pid>/stat` answer `EACCES`. Fails for a walk that treats every
    /// `EACCES` as "another uid's process, carry on" — which would report
    /// `Closed` on a hardened host where keeper can see nothing at all, and
    /// that is the one direction that deletes data (AD-125).
    ///
    /// **The root carries this process's own pid directory, and the `Closed`
    /// assertion comes first.** Without it the test would pass with the
    /// identifiability check deleted, because the fake root does not contain
    /// this process either and the walk's final `own_seen` gate would answer
    /// `Unknown` for a second, unrelated reason. Establishing `Closed` over the
    /// same root first is what makes the hidden process the only thing the
    /// second assertion can be attributing its answer to.
    #[test]
    #[cfg(target_os = "linux")]
    fn a_process_that_cannot_be_identified_refuses() {
        use std::os::unix::fs::PermissionsExt as _;

        let work = tempfile::tempdir().expect("temp dir");
        let file = work.path().join("clip.mp4");
        std::fs::write(&file, b"content").expect("write");

        // A root that CAN answer: `self` resolving to this process, whose
        // (empty) descriptor table is readable, so the walk completes and may
        // speak.
        let root = answerable_proc_root(&work.path().join("proc"));
        assert_eq!(
            open_file_state_under_proc(&root, &file),
            OpenFileState::Closed,
            "the fixture root is capable of a completed walk, or the assertion \
             below would prove nothing about the hidden process"
        );

        let hidden = root.join("1234");
        std::fs::create_dir_all(hidden.join("fd")).expect("build the pid directory");
        std::fs::write(hidden.join("stat"), "1234 (hidden) S 1 1 1 0\n").expect("write stat");
        let unenterable = std::fs::Permissions::from_mode(0o000);
        std::fs::set_permissions(&hidden, unenterable).expect("hide the pid directory");

        // uid 0 ignores the DAC check entirely, so on a root test runner the
        // fixture cannot express the shape under test and there is nothing
        // falsifiable left here. Restore the mode first, or the temp directory
        // cannot be cleaned up.
        let is_root = std::fs::read_dir(hidden.join("fd")).is_ok();
        let restore = || {
            std::fs::set_permissions(&hidden, std::fs::Permissions::from_mode(0o700))
                .expect("restore the mode so the temp directory can be removed");
        };
        if is_root {
            restore();
            return;
        }

        // Read the answer BEFORE asserting on it: a failing `assert_eq!` here
        // unwinds past the restore, `TempDir::drop` then cannot remove a
        // mode-000 directory and swallows the error by design, and every failing
        // run would leak one under the system temp directory.
        let observed = open_file_state_under_proc(&root, &file);
        restore();
        assert_eq!(
            observed,
            OpenFileState::Unknown,
            "a process whose very existence keeper cannot confirm is a blind spot, \
             not a process that holds nothing"
        );
    }
}
