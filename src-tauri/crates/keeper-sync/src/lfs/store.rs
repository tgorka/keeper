//! The local content-addressed LFS object store (Story 25.1, AD-46).
//!
//! Layout, matching git-lfs so a repository stays usable by the `git-lfs`
//! binary if a user ever installs one:
//!
//! ```text
//! <git-dir>/lfs/objects/<oid[0:2]>/<oid[2:4]>/<oid>   published objects
//! <git-dir>/lfs/incomplete/<oid>.part                 resumable download staging
//! <git-dir>/lfs/tmp/                                  atomic-publish scratch
//! <git-dir>/lfs/helpers/<pid>                         one per live filter-process helper
//! ```
//!
//! `helpers/` is keeper's own and git-lfs never reads it: a marker per
//! long-running `filter.lfs.process` helper, held under an exclusive lock for
//! the helper's lifetime so the engine can tell a live helper from a dead one's
//! leftover without a pid table — see [`LfsStore::register_helper`] and
//! [`LfsStore::look_at_helpers`].
//!
//! # Blocking
//!
//! Every method here is **synchronous and potentially long-running** —
//! [`LfsStore::insert_verified`] and [`LfsStore::resume_offset`] both hash
//! whole objects, which for this subsystem means gigabytes. Async callers MUST
//! run them on `tokio::task::spawn_blocking`; [`crate::lfs::basic`] does.
//! Keeping them sync is deliberate: the git clean/smudge path that also uses
//! them is not async at all, and an async duplicate would be a second
//! implementation of the same hash loop.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use fs4::fs_std::FileExt as _;
use sha2::{Digest, Sha256};

use crate::error::{Result, SyncError};

/// Read granularity for the hashing loops.
///
/// Bounded on purpose: an LFS object is routinely larger than RAM, so nothing
/// in this module may size a buffer from the object (NFR-23).
///
/// `pub(crate)` because a second LFS hashing loop arrived that is not in this
/// module and must read at the same granularity:
/// [`crate::lfs::stage::content_oid`], which hashes a materialized worktree
/// file to prove it still holds the object this store would have handed out. A
/// third copy of the literal would be a third thing to keep in step.
pub(crate) const HASH_CHUNK_BYTES: usize = 128 * 1024;

/// A repository's local LFS object store.
#[derive(Debug, Clone)]
pub struct LfsStore {
    /// The `lfs` directory itself, i.e. `<git-dir>/lfs`.
    root: PathBuf,
}

/// Hasher state for partial downloads, for the life of the process.
///
/// # What this removes
///
/// [`LfsStore::resume_offset`] re-reads and re-hashes the entire `.part` file,
/// because a resumed download must still produce a digest over every byte —
/// including the ones it did not fetch this time. That is correct and it is
/// not cheap: a 970 MB partial on an external drive is fourteen seconds of
/// reading before a single new byte is asked for.
///
/// On a link that interrupts often, that cost is paid again on every retry and
/// it *grows with progress*, so the further an object gets the slower it goes.
/// Measured on a folder pulling 44 GB over a 240 kB/s tunnel: three partials
/// above 600 MB, all being resumed repeatedly, and an effective rate that
/// decayed from the link's own 300 kB/s to a fifth of it over an afternoon.
///
/// # Why a cache is safe here
///
/// The state is handed back **only** when the `.part` is exactly as long as it
/// was when the state was taken. Nothing in this crate rewrites a partial in
/// place — a download appends and nothing else — so equal length means equal
/// bytes. And the guarantee does not rest on that reasoning alone: the digest
/// of the finished object is still checked against its oid before the file is
/// committed, so a wrong prefix cannot pass, it can only fail late and cost a
/// re-download.
///
/// In-memory on purpose. `Sha256`'s intermediate state has no serialized form,
/// and a restart is exactly the case where re-reading is the honest answer.
static STAGED_PREFIXES: OnceLock<Mutex<HashMap<String, (u64, Sha256)>>> = OnceLock::new();

fn staged_prefixes() -> &'static Mutex<HashMap<String, (u64, Sha256)>> {
    STAGED_PREFIXES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Keep the hasher for a partial this process wrote, so the next attempt does
/// not read those bytes again.
pub fn remember_prefix(oid: &str, offset: u64, hasher: &Sha256) {
    let mut cache = staged_prefixes().lock().expect("staged prefix lock");
    cache.insert(oid.to_owned(), (offset, hasher.clone()));
}

/// The remembered hasher, if it covers exactly `staged` bytes.
fn recall_prefix(oid: &str, staged: u64) -> Option<Sha256> {
    let cache = staged_prefixes().lock().expect("staged prefix lock");
    cache
        .get(oid)
        .filter(|(offset, _)| *offset == staged)
        .map(|(_, hasher)| hasher.clone())
}

/// Drop what is no longer partial — a committed object, or one whose staged
/// bytes were discarded.
pub fn forget_prefix(oid: &str) {
    staged_prefixes()
        .lock()
        .expect("staged prefix lock")
        .remove(oid);
}

/// What one sweep found and what it took.
///
/// Both halves, because "removed 863" alone cannot say whether the sweep was
/// thorough or whether 900 more are still there being written.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ScratchSweep {
    pub found: u64,
    pub found_bytes: u64,
    pub removed: u64,
    pub removed_bytes: u64,
}

/// What one look at the helpers found.
///
/// Four numbers and not one, because "2 helpers" alone cannot say whether the
/// folder is healthy: `alive` is the count a person expects to see while a git
/// is running here, `stuck` is the count that explains a slow walk, `removed`
/// is the count that says how many helpers died without unregistering — every
/// one of them a process that was killed rather than finished — and `skipped`
/// is the count that says the other three could not be trusted, because a
/// marker the lock could not be tried on is a helper this look cannot place.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HelperLook {
    /// Markers whose lock is held: a helper process is alive behind each.
    pub alive: u64,
    /// The alive helpers idle for at least the look's threshold, as
    /// `(pid, idle)`. A subset of `alive`.
    pub stuck: Vec<(u32, Duration)>,
    /// Markers whose lock could be taken — a dead helper's leftover — and were
    /// unlinked.
    pub removed: u64,
    /// Markers the look could not open or try the lock on, as `(pid, idle)`
    /// with the idle read off the mtime as for `stuck`. Not counted in
    /// `alive` or `stuck`: a filesystem that refuses `flock` (some network
    /// mounts do) would otherwise make every helper on it invisible and the
    /// look read as healthy. The caller falls back to asking the pid table
    /// whether each is alive, which is the question the lock normally answers.
    pub skipped: Vec<(u32, Duration)>,
}

impl HelperLook {
    /// The stuck helper idle the longest, if any.
    ///
    /// The one worth naming in a warning: on the folder this was measured on
    /// eighteen helpers were stuck at once, and the reader wants the first one
    /// and its parent, not eighteen lines.
    pub fn oldest_stuck(&self) -> Option<(u32, Duration)> {
        self.stuck.iter().copied().max_by_key(|(_, idle)| *idle)
    }
}

/// A live helper's registration: the marker file and the exclusive lock on it.
///
/// Hold it for as long as the helper serves. Dropping it unlocks and unlinks
/// the marker, so a helper that returns leaves nothing behind; a helper that
/// is killed leaves the file, the kernel releases the lock with the process,
/// and the next [`LfsStore::look_at_helpers`] tells the two apart by trying
/// the lock.
#[derive(Debug)]
pub struct HelperMarker {
    /// Kept open because the lock lives on this open file description, not on
    /// the path.
    file: File,
    path: PathBuf,
}

impl HelperMarker {
    /// Where the marker is.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Say "a request just finished": set the marker's mtime to now.
    ///
    /// The mtime is the whole idle signal, so a helper that serves for forty
    /// minutes without pause is never mistaken for a stuck one — every request
    /// it answers moves the clock.
    pub fn touch(&self) -> std::io::Result<()> {
        self.file.set_modified(SystemTime::now())
    }
}

impl Drop for HelperMarker {
    fn drop(&mut self) {
        // Best effort on both counts: a helper on its way out has nobody to
        // report to, and a marker it could not unlink is exactly what the
        // look's `removed` arm exists for.
        let _ = self.file.unlock();
        let _ = std::fs::remove_file(&self.path);
    }
}

/// The two places a test can stand between [`LfsStore::look_at_helpers`] and
/// the filesystem. Private, and not `cfg(test)`: the production look runs
/// through the same function with both seams closed, so what the tests drive
/// is what ships.
struct LookSeams<'a> {
    /// How a marker's lock is tried; the real one is `try_lock_exclusive`.
    try_lock: &'a dyn Fn(&File) -> std::io::Result<bool>,
    /// Runs after a leftover's lock was taken and before its path is
    /// unlinked — the instant a helper under a reused pid can rename a fresh
    /// marker into place.
    before_unlink: &'a mut dyn FnMut(&Path),
}

/// What a file name under `helpers/` says it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkerName {
    /// `<pid>`: a registered helper's marker.
    Helper(u32),
    /// `.<pid>.registering`: a marker being created, private to its process
    /// until the rename that gives it its real name.
    Staging(u32),
}

impl MarkerName {
    fn parse(name: &str) -> Option<Self> {
        if let Ok(pid) = name.parse::<u32>() {
            return Some(Self::Helper(pid));
        }
        name.strip_prefix('.')
            .and_then(|rest| rest.strip_suffix(".registering"))
            .and_then(|pid| pid.parse::<u32>().ok())
            .map(Self::Staging)
    }
}

/// Whether two metadata readings describe the same file, by identity rather
/// than by name: the check that keeps a look from unlinking a fresh marker a
/// reused pid renamed over a leftover's path.
#[cfg(unix)]
fn same_file(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    a.dev() == b.dev() && a.ino() == b.ino()
}

/// Without a stable file identity on this platform the look unlinks by path,
/// as it did before the check existed; the race it guards against needs a
/// pid to be reused inside one look, which is a Unix-shaped problem.
#[cfg(not(unix))]
fn same_file(_a: &std::fs::Metadata, _b: &std::fs::Metadata) -> bool {
    true
}

impl LfsStore {
    /// Wrap an existing `…/lfs` directory path.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The store for a repository whose git directory is `git_dir`.
    ///
    /// Callers must pass the **common** git directory. A linked worktree
    /// (AD-50's bot lane) has a `.git` *file* pointing at
    /// `…/.git/worktrees/<name>`, and giving that per-worktree directory here
    /// would create a second, invisible object store that never shares a
    /// single downloaded byte with the main one.
    pub fn in_git_dir(git_dir: impl AsRef<Path>) -> Self {
        Self::new(git_dir.as_ref().join("lfs"))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where a published object lives.
    ///
    /// The leaf is the **full** oid. Forgejo's *server-side* store looks
    /// deceptively similar but shards as `<oid[0:2]>/<oid[2:4]>/<oid[4:]>` —
    /// tail only. The two layouts are not interchangeable, and quietly using
    /// one for the other yields objects that exist on disk but can never be
    /// found again.
    pub fn object_path(&self, oid: &str) -> PathBuf {
        let (first, second) = shard(oid);
        self.root.join("objects").join(first).join(second).join(oid)
    }

    /// Where a resumable download stages its bytes.
    pub fn incomplete_path(&self, oid: &str) -> PathBuf {
        self.root.join("incomplete").join(format!("{oid}.part"))
    }

    /// Scratch directory for atomic publication. On the same filesystem as
    /// `objects/`, which is what makes the final rename atomic.
    pub fn tmp_dir(&self) -> PathBuf {
        self.root.join("tmp")
    }

    /// Where the live helpers' markers are: `<git-dir>/lfs/helpers`.
    pub fn helpers_dir(&self) -> PathBuf {
        self.root.join("helpers")
    }

    /// How long a helper may go without a request before a look calls it
    /// stuck.
    ///
    /// A helper is idle between the requests of a git that is still running,
    /// and a git walk over this folder is seconds — the field measurement was
    /// two to four minutes with eighteen stuck helpers slowing it. Thirty
    /// minutes is far past any walk that is still making progress and far
    /// short of the three and a half hours the stuck ones were found at, so
    /// the look can say "stuck" without ever saying it about a git that is
    /// merely slow.
    pub const STUCK_HELPER_IDLE: Duration = Duration::from_secs(30 * 60);

    /// Register a long-running helper: create `helpers/<pid>`, take the
    /// exclusive lock, and hand back both so the caller keeps them alive.
    ///
    /// An error means the helper serves unregistered — the caller says so once
    /// on stderr, with the error, and goes on, because a filter that refused to
    /// serve over a directory it could not create would turn housekeeping into
    /// a failed `git status` (DW-206's shape, from the other side). That is
    /// also why `helpers/` is created here and not in [`Self::ensure_layout`]:
    /// the layout is what [`crate::lfs::filter::run_process`] fails on, and a
    /// `helpers` path that cannot be a directory must cost the helper its
    /// registration, never its service.
    ///
    /// The file is created and locked under a staging name and then renamed
    /// into place, so there is no instant at which `helpers/<pid>` exists
    /// unlocked: a look racing the registration either sees the locked marker
    /// or nothing, never a fresh marker it would mistake for a leftover and
    /// unlink out from under a live helper. The rename also replaces a dead
    /// predecessor's leftover under a reused pid.
    pub fn register_helper(&self, pid: u32) -> std::io::Result<HelperMarker> {
        let dir = self.helpers_dir();
        std::fs::create_dir_all(&dir)?;
        let staging = dir.join(format!(".{pid}.registering"));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&staging)?;
        // `try_`, never the blocking form: this file is private to this
        // process until the rename, so a lock somebody else holds is a
        // fact worth refusing on, not waiting out.
        if !file.try_lock_exclusive()? {
            let _ = std::fs::remove_file(&staging);
            return Err(fs4::lock_contended_error());
        }
        file.set_modified(SystemTime::now())?;
        let path = dir.join(pid.to_string());
        if let Err(err) = std::fs::rename(&staging, &path) {
            let _ = std::fs::remove_file(&staging);
            return Err(err);
        }
        Ok(HelperMarker { file, path })
    }

    /// Look at every registered helper: remove the dead ones' leftovers, count
    /// the live ones, and name those idle for `idle` or longer as of `now`.
    ///
    /// The lock is the liveness test and the mtime the idle signal, so this
    /// costs one `read_dir` and one `open` + `try_lock` per marker — no pid
    /// table, no signal, nothing that touches the helper itself. A helper is
    /// never killed here, whatever it looks like: it belongs to a git keeper
    /// did not start, and that git holds state — an index lock, a half-written
    /// index — that ending its filter would not release cleanly.
    ///
    /// Only a name that is a pid is a helper's marker. A `.<pid>.registering`
    /// staging file is a helper mid-registration and is left alone — it is
    /// counted on a later look, under its real name — unless it is older than
    /// `idle`, when it is a registration that died between `open` and `rename`
    /// and is unlinked if its lock is free, without counting as `removed`.
    /// Anything else in the directory is not keeper's and is not touched.
    ///
    /// Never fails the caller: a look is housekeeping. A marker that cannot be
    /// opened or whose lock cannot be tried is reported in
    /// [`HelperLook::skipped`] rather than dropped, so a filesystem that
    /// refuses locks cannot make the look read as healthy.
    pub fn look_at_helpers(&self, now: SystemTime, idle: Duration) -> HelperLook {
        self.look_at_helpers_via(
            now,
            idle,
            &mut LookSeams {
                try_lock: &|file| file.try_lock_exclusive(),
                before_unlink: &mut |_| {},
            },
        )
    }

    /// [`Self::look_at_helpers`] with its two seams open. One function, not a
    /// tested twin of the production one, so the two cannot drift.
    fn look_at_helpers_via(
        &self,
        now: SystemTime,
        idle: Duration,
        seams: &mut LookSeams<'_>,
    ) -> HelperLook {
        let mut look = HelperLook::default();
        let Ok(entries) = std::fs::read_dir(self.helpers_dir()) else {
            return look;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let name = entry.file_name();
            let Some(kind) = name.to_str().and_then(MarkerName::parse) else {
                continue;
            };
            // A clock that moved backwards reads as "just touched", which is
            // the arm that never accuses anyone.
            let idle_for = meta
                .modified()
                .ok()
                .and_then(|touched| now.duration_since(touched).ok())
                .unwrap_or_default();
            let pid = match kind {
                MarkerName::Helper(pid) => pid,
                MarkerName::Staging(_) => {
                    if idle_for < idle {
                        continue;
                    }
                    let Ok(file) = OpenOptions::new().read(true).open(&path) else {
                        continue;
                    };
                    if matches!((seams.try_lock)(&file), Ok(true)) {
                        Self::unlink_if_unchanged(&path, file, seams);
                    }
                    continue;
                }
            };
            let file = match OpenOptions::new().read(true).open(&path) {
                Ok(file) => file,
                Err(err) => {
                    tracing::debug!(path = %path.display(), %err, "cannot open an lfs helper marker");
                    look.skipped.push((pid, idle_for));
                    continue;
                }
            };
            match (seams.try_lock)(&file) {
                // The kernel handed the lock over: whoever held it is gone.
                Ok(true) => {
                    if Self::unlink_if_unchanged(&path, file, seams) {
                        look.removed += 1;
                    }
                }
                Ok(false) => {
                    look.alive += 1;
                    if idle_for >= idle {
                        look.stuck.push((pid, idle_for));
                    }
                }
                Err(err) => {
                    tracing::debug!(path = %path.display(), %err, "cannot try the lock on an lfs helper marker");
                    look.skipped.push((pid, idle_for));
                }
            }
        }
        look
    }

    /// Unlink a leftover whose lock this look has just taken — but only the
    /// file that was opened, never whatever now stands at its path.
    ///
    /// Between the `open` and the `remove_file` a helper under a reused pid
    /// can rename its fresh, locked marker over the same name; unlinking by
    /// path then would take a live helper's marker out from under it, and the
    /// next look would call that helper dead. So the opened file's identity is
    /// compared with the path's before the unlink, and a mismatch leaves the
    /// path alone. `true` when the leftover is gone — by this unlink, or by
    /// its own helper's between the two calls.
    fn unlink_if_unchanged(path: &Path, opened: File, seams: &mut LookSeams<'_>) -> bool {
        (seams.before_unlink)(path);
        let identity = opened.metadata();
        let _ = opened.unlock();
        drop(opened);
        match (identity, std::fs::metadata(path)) {
            (Ok(opened), Ok(on_disk)) if !same_file(&opened, &on_disk) => {
                tracing::debug!(
                    path = %path.display(),
                    "a new lfs helper took this marker's name between open and unlink; leaving it"
                );
                return false;
            }
            // Unlinked by its own helper between our `open` and now — the
            // helper is gone either way.
            (_, Err(err)) if err.kind() == std::io::ErrorKind::NotFound => return true,
            _ => {}
        }
        match std::fs::remove_file(path) {
            Ok(()) => true,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => true,
            Err(err) => {
                tracing::debug!(path = %path.display(), %err, "cannot remove a dead lfs helper's marker");
                false
            }
        }
    }

    /// How old scratch has to be before a sweep is willing to call it debris.
    ///
    /// A live transfer's temp file is seconds old. An hour is far beyond any
    /// honest write and far short of the days this scratch has been observed to
    /// survive, so the sweep can run at startup — beside a transfer that is
    /// genuinely in flight — without ever taking a file somebody is using.
    const SCRATCH_DEBRIS_AGE: std::time::Duration = std::time::Duration::from_secs(3600);

    /// Delete transfer scratch that no live transfer can still be using, and
    /// report what was there.
    ///
    /// # Why this is needed at all
    ///
    /// `tempfile` deletes its file when the handle drops — and a process that
    /// is killed drops nothing. Every crash therefore leaks one file per
    /// transfer in flight, and nothing has ever removed them. Measured on one
    /// machine after four days of a crash loop: **100 GB in 863 files**, beside
    /// 4 GB of abandoned resumable downloads, on a volume with 153 GB free.
    ///
    /// # Why by age rather than by name
    ///
    /// The names are random by construction, so there is nothing in a name to
    /// tell a leaked file from a live one. Age can: see
    /// [`Self::SCRATCH_DEBRIS_AGE`].
    ///
    /// Never fails the caller. A sweep is housekeeping, and a folder that
    /// refused to sync because it could not delete a temp file would have traded
    /// a disk-space problem for a sync problem.
    pub fn sweep_scratch(&self, profile: &str) -> ScratchSweep {
        let mut swept = ScratchSweep::default();
        for dir in [self.tmp_dir(), self.root.join("incomplete")] {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let Ok(meta) = entry.metadata() else { continue };
                if !meta.is_file() {
                    continue;
                }
                swept.found += 1;
                swept.found_bytes += meta.len();
                let old_enough = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .is_some_and(|age| age >= Self::SCRATCH_DEBRIS_AGE);
                if old_enough && std::fs::remove_file(entry.path()).is_ok() {
                    swept.removed += 1;
                    swept.removed_bytes += meta.len();
                }
            }
        }
        if swept.removed > 0 {
            crate::anomaly::Anomaly {
                what: "transfer scratch left by an interrupted run",
                measured: format!(
                    "found={} found_bytes={} removed={} removed_bytes={}",
                    swept.found, swept.found_bytes, swept.removed, swept.removed_bytes
                ),
                expected: "near zero; scratch is deleted when a transfer ends, and only a \
                           killed process leaves any",
                consequence: "disk nothing would have reclaimed; removed now, and the space \
                              is back",
            }
            .report(profile);
        }
        swept
    }

    /// Create the directory skeleton. Idempotent.
    pub fn ensure_layout(&self) -> Result<()> {
        for dir in [
            self.root.join("objects"),
            self.root.join("incomplete"),
            self.tmp_dir(),
        ] {
            std::fs::create_dir_all(&dir)
                .map_err(|err| SyncError::io("create lfs directory", dir, err))?;
        }
        Ok(())
    }

    /// Whether a *complete* object is already present.
    ///
    /// The size check is the point. A `kill -9` during an older, non-atomic
    /// write — or a partially-restored backup — leaves a correctly-named file
    /// holding a prefix of the content. Trusting the name alone would hand
    /// that truncated blob to the smudge filter as if it were the object.
    pub fn contains(&self, oid: &str, expected_size: u64) -> bool {
        std::fs::metadata(self.object_path(oid))
            .map(|meta| meta.is_file() && meta.len() == expected_size)
            .unwrap_or(false)
    }

    /// The OID and size a stream would have, without keeping a byte of it.
    ///
    /// The clean direction asks two questions of a file — "what is its pointer"
    /// and "is its object stored" — and only the second one needs the bytes.
    /// [`LfsStore::insert_streaming`] cannot tell them apart: the OID is the
    /// *result* of writing, so it writes a full copy into `tmp/` first and
    /// learns afterwards that the object was already there.
    ///
    /// On a folder whose objects are almost all stored already, that copy is
    /// the dominant cost of a status walk. Measured on the field folder over a
    /// quiet volume: `tmp/` grew at 9.6 MB/s and stood at 5.4 GB mid-walk, so
    /// the 24.2 GB of pointer content a walk re-reads costs about 42 minutes of
    /// writing — every walk, for content that never changed.
    pub fn digest_of(mut reader: impl Read) -> Result<(String, u64)> {
        let mut hasher = Sha256::new();
        let mut size: u64 = 0;
        let mut buf = vec![0u8; HASH_CHUNK_BYTES];
        loop {
            let read = reader.read(&mut buf).map_err(|err| {
                SyncError::io("read lfs source", std::path::Path::new("<stream>"), err)
            })?;
            if read == 0 {
                break;
            }
            hasher.update(&buf[..read]);
            size += read as u64;
        }
        Ok((hex::encode(hasher.finalize()), size))
    }

    /// Hash `reader` into the store and publish it under the digest it turns
    /// out to have.
    ///
    /// The clean direction: staging a worktree file, where the OID is the
    /// *result* rather than an expectation, so [`LfsStore::insert_verified`]
    /// cannot be used. Returns `(oid, bytes)`. Streams in bounded chunks and
    /// never holds the object in memory — which is the reason LFS exists here
    /// at all (AD-46).
    pub fn insert_streaming(&self, mut reader: impl Read) -> Result<(String, u64)> {
        self.ensure_layout()?;

        let tmp_dir = self.tmp_dir();
        let mut staged = tempfile::NamedTempFile::new_in(&tmp_dir)
            .map_err(|err| SyncError::io("create lfs temp file", &tmp_dir, err))?;

        let mut hasher = Sha256::new();
        let mut written: u64 = 0;
        let mut buf = vec![0u8; HASH_CHUNK_BYTES];
        loop {
            let read = reader
                .read(&mut buf)
                .map_err(|err| SyncError::io("read lfs source", staged.path(), err))?;
            if read == 0 {
                break;
            }
            let chunk = &buf[..read];
            hasher.update(chunk);
            staged
                .write_all(chunk)
                .map_err(|err| SyncError::io("write lfs temp file", staged.path(), err))?;
            written += read as u64;
        }
        staged
            .flush()
            .map_err(|err| SyncError::io("flush lfs temp file", staged.path(), err))?;

        let oid = hex::encode(hasher.finalize());
        self.publish(staged, &oid, written)?;
        Ok((oid, written))
    }

    /// Hash `reader` into the store, verifying it against `oid` before it
    /// becomes visible.
    ///
    /// Writes to a temp file, hashes in the same pass, and publishes by rename
    /// only after both the digest and the byte count match. A failure therefore
    /// leaves nothing behind: the temp file is unlinked on drop, so a corrupt
    /// transfer can never be mistaken for a cached object later.
    pub fn insert_verified(
        &self,
        oid: &str,
        mut reader: impl Read,
        expected_size: u64,
    ) -> Result<()> {
        self.ensure_layout()?;

        let tmp_dir = self.tmp_dir();
        let mut staged = tempfile::NamedTempFile::new_in(&tmp_dir)
            .map_err(|err| SyncError::io("create lfs temp file", &tmp_dir, err))?;

        let mut hasher = Sha256::new();
        let mut written: u64 = 0;
        let mut buf = vec![0u8; HASH_CHUNK_BYTES];
        loop {
            let read = reader
                .read(&mut buf)
                .map_err(|err| SyncError::io("read lfs source", staged.path(), err))?;
            if read == 0 {
                break;
            }
            let chunk = &buf[..read];
            hasher.update(chunk);
            staged
                .write_all(chunk)
                .map_err(|err| SyncError::io("write lfs temp file", staged.path(), err))?;
            written += read as u64;
        }
        staged
            .flush()
            .map_err(|err| SyncError::io("flush lfs temp file", staged.path(), err))?;

        let actual = hex::encode(hasher.finalize());
        if actual != oid || written != expected_size {
            return Err(SyncError::Integrity {
                subject: format!("lfs object {oid}"),
                expected: format!("{oid}/{expected_size}B"),
                actual: format!("{actual}/{written}B"),
            });
        }

        self.publish(staged, oid, expected_size)
    }

    /// Publish an already-verified `.part` file into the object store.
    ///
    /// Split from [`LfsStore::insert_verified`] because a resumable download
    /// hashes as it streams — re-reading a multi-gigabyte staged file just to
    /// re-derive a digest we already hold would double the I/O of every
    /// transfer. The caller owns the verification; see
    /// [`crate::lfs::basic::BasicTransfer::download`].
    pub fn commit_partial(&self, oid: &str) -> Result<()> {
        let part = self.incomplete_path(oid);
        let target = self.object_path(oid);
        self.ensure_object_parent(&target)?;

        match std::fs::rename(&part, &target) {
            Ok(()) => Ok(()),
            Err(err) => {
                // A pre-existing target is success: content addressing makes
                // the two files byte-identical, so another process (or a
                // re-driven journal unit whose rename already landed) winning
                // the race is exactly the outcome we wanted.
                if target.is_file() {
                    self.discard_superseded_partial(oid, &part);
                    Ok(())
                } else {
                    Err(SyncError::io("publish lfs object", target, err))
                }
            }
        }
    }

    /// How much of `oid` is already staged, and the hasher primed with it.
    ///
    /// Returning the primed [`Sha256`] rather than just the offset is what
    /// makes resume safe: the digest of the whole object is still computed over
    /// every byte, including the ones we did not download this time, so a
    /// corrupt prefix cannot survive verification. A missing `.part` yields
    /// `(0, Sha256::new())`, which is the same code path as a fresh download.
    pub fn resume_offset(&self, oid: &str) -> Result<(u64, Sha256)> {
        let path = self.incomplete_path(oid);
        let mut hasher = Sha256::new();
        let mut file = match std::fs::File::open(&path) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok((0, hasher)),
            Err(err) => return Err(SyncError::io("open lfs partial", path, err)),
        };

        // The prefix this process hashed on its way to being interrupted, if it
        // is still exactly what is on disk. See `remember_prefix` for why that
        // check is the whole of the safety argument.
        let staged = file
            .metadata()
            .map_err(|err| SyncError::io("stat lfs partial", &path, err))?
            .len();
        if let Some(recalled) = recall_prefix(oid, staged) {
            return Ok((staged, recalled));
        }

        let mut offset: u64 = 0;
        let mut buf = vec![0u8; HASH_CHUNK_BYTES];
        loop {
            let read = file
                .read(&mut buf)
                .map_err(|err| SyncError::io("read lfs partial", &path, err))?;
            if read == 0 {
                break;
            }
            hasher.update(&buf[..read]);
            offset += read as u64;
        }
        Ok((offset, hasher))
    }

    /// Drop a staged download. Idempotent — a missing file is the goal state.
    pub fn remove_partial(&self, oid: &str) -> Result<()> {
        let path = self.incomplete_path(oid);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(SyncError::io("remove lfs partial", path, err)),
        }
    }

    fn ensure_object_parent(&self, target: &Path) -> Result<()> {
        let Some(parent) = target.parent() else {
            return Ok(());
        };
        std::fs::create_dir_all(parent)
            .map_err(|err| SyncError::io("create lfs object directory", parent, err))
    }

    fn publish(
        &self,
        staged: tempfile::NamedTempFile,
        oid: &str,
        expected_size: u64,
    ) -> Result<()> {
        let target = self.object_path(oid);
        self.ensure_object_parent(&target)?;
        match staged.persist(&target) {
            Ok(_) => Ok(()),
            Err(err) => {
                // Same race as `commit_partial`: an already-published object
                // with the right size is the success case. Windows `rename`
                // refuses an existing target where POSIX overwrites it, so this
                // branch is reachable in practice, not just in theory.
                if self.contains(oid, expected_size) {
                    Ok(())
                } else {
                    Err(SyncError::io("publish lfs object", target, err.error))
                }
            }
        }
    }

    fn discard_superseded_partial(&self, oid: &str, part: &Path) {
        if let Err(err) = std::fs::remove_file(part) {
            if err.kind() != std::io::ErrorKind::NotFound {
                // Not fatal: the object is published, and a stray `.part` only
                // costs disk until the next `gc`.
                tracing::debug!(oid, %err, "could not remove superseded lfs partial");
            }
        }
    }
}

/// The two shard components of the client-side layout.
///
/// Tolerates a short oid rather than panicking on a slice: nothing should ever
/// reach here with one, but a panic in the sync loop is a far worse failure
/// than an oddly-named directory.
fn shard(oid: &str) -> (&str, &str) {
    (oid.get(..2).unwrap_or(oid), oid.get(2..4).unwrap_or(""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, LfsStore) {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = LfsStore::in_git_dir(dir.path());
        store.ensure_layout().expect("layout");
        (dir, store)
    }

    fn oid_of(content: &[u8]) -> String {
        hex::encode(Sha256::digest(content))
    }

    #[test]
    fn object_path_uses_the_client_layout_with_the_full_oid_as_leaf() {
        let (_dir, store) = store();
        let oid = "4d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa5e2ca24d17e2393";
        let path = store.object_path(oid);

        assert!(path.ends_with(format!("objects/4d/7a/{oid}")), "{path:?}");
        assert!(store.incomplete_path(oid).ends_with(format!("{oid}.part")));
    }

    #[test]
    fn insert_then_contains_round_trip() {
        let (_dir, store) = store();
        let content = b"the quick brown fox jumps over the lazy dog";
        let oid = oid_of(content);

        assert!(!store.contains(&oid, content.len() as u64));
        store
            .insert_verified(&oid, &content[..], content.len() as u64)
            .expect("insert");

        assert!(store.contains(&oid, content.len() as u64));
        let stored = std::fs::read(store.object_path(&oid)).expect("read back");
        assert_eq!(stored, content);
    }

    #[test]
    fn wrong_digest_insert_fails_with_integrity_and_leaves_no_object() {
        let (_dir, store) = store();
        let content = b"actual content";
        let claimed = oid_of(b"something else entirely");

        let err = store
            .insert_verified(&claimed, &content[..], content.len() as u64)
            .expect_err("digest mismatch must fail");

        assert!(matches!(err, SyncError::Integrity { .. }), "{err:?}");
        assert_eq!(err.code(), "integrity");
        assert!(!store.object_path(&claimed).exists());
        // Nothing may be left in the scratch directory either, or the next run
        // fills the disk with unreferenced garbage.
        let leftovers = std::fs::read_dir(store.tmp_dir()).expect("tmp dir").count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn size_mismatch_alone_fails_even_when_the_digest_matches() {
        let (_dir, store) = store();
        let content = b"twelve bytes";
        let oid = oid_of(content);

        let err = store
            .insert_verified(&oid, &content[..], 999)
            .expect_err("size mismatch must fail");

        assert!(matches!(err, SyncError::Integrity { .. }), "{err:?}");
        assert!(!store.contains(&oid, content.len() as u64));
    }

    #[test]
    fn contains_rejects_a_right_name_wrong_size_file() {
        let (_dir, store) = store();
        let content = b"a complete object";
        let oid = oid_of(content);

        // Simulate a truncated leftover: correct path, short content.
        let path = store.object_path(&oid);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&path, b"a comp").expect("write truncated");

        assert!(!store.contains(&oid, content.len() as u64));
        assert!(
            store.contains(&oid, 6),
            "size is what decides, not the name"
        );
    }

    #[test]
    fn resume_offset_returns_the_prefix_hash_of_a_partial_file() {
        let (_dir, store) = store();
        let full = b"0123456789abcdefghij";
        let prefix = &full[..12];
        let oid = oid_of(full);

        // No `.part` yet: a fresh start, with a virgin hasher.
        let (offset, hasher) = store.resume_offset(&oid).expect("missing part");
        assert_eq!(offset, 0);
        assert_eq!(hex::encode(hasher.finalize()), oid_of(b""));

        std::fs::write(store.incomplete_path(&oid), prefix).expect("stage prefix");
        let (offset, mut hasher) = store.resume_offset(&oid).expect("partial part");
        assert_eq!(offset, prefix.len() as u64);

        // The primed hasher must be the *running* state, so feeding it the
        // remainder yields the digest of the whole object. This is the
        // property that makes resume safe.
        hasher.update(&full[prefix.len()..]);
        assert_eq!(hex::encode(hasher.finalize()), oid);
    }

    #[test]
    fn commit_partial_publishes_and_clears_the_staging_file() {
        let (_dir, store) = store();
        let content = b"streamed and already verified";
        let oid = oid_of(content);

        std::fs::write(store.incomplete_path(&oid), content).expect("stage");
        store.commit_partial(&oid).expect("commit");

        assert!(store.contains(&oid, content.len() as u64));
        assert!(!store.incomplete_path(&oid).exists());
    }

    #[test]
    fn a_pre_existing_target_is_success_not_a_conflict() {
        let (_dir, store) = store();
        let content = b"one object, two writers";
        let oid = oid_of(content);
        let size = content.len() as u64;

        // First writer publishes.
        store
            .insert_verified(&oid, &content[..], size)
            .expect("first insert");

        // Second writer races in with byte-identical content: still success.
        store
            .insert_verified(&oid, &content[..], size)
            .expect("duplicate insert must succeed");
        // As must a `.part` publish that loses the same race.
        std::fs::write(store.incomplete_path(&oid), content).expect("stage");
        store
            .commit_partial(&oid)
            .expect("duplicate commit must succeed");

        assert!(store.contains(&oid, size));
        assert_eq!(
            std::fs::read(store.object_path(&oid)).expect("read back"),
            content
        );
        assert!(!store.incomplete_path(&oid).exists());
    }

    #[test]
    fn remove_partial_is_idempotent() {
        let (_dir, store) = store();
        let oid = oid_of(b"nothing staged");

        store.remove_partial(&oid).expect("no-op removal");
        std::fs::write(store.incomplete_path(&oid), b"x").expect("stage");
        store.remove_partial(&oid).expect("removal");
        assert!(!store.incomplete_path(&oid).exists());
        store
            .remove_partial(&oid)
            .expect("second removal is a no-op");
    }

    /// The whole point: a resume does not read again what this process already
    /// hashed. Proven by making the file's bytes DISAGREE with the remembered
    /// state — if `resume_offset` read the file, it could not return the digest
    /// the cache holds.
    #[test]
    fn a_remembered_prefix_is_handed_back_without_reading_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = LfsStore::in_git_dir(dir.path().join(".git"));
        store.ensure_layout().expect("layout");
        let oid = "a".repeat(64);
        std::fs::write(store.incomplete_path(&oid), vec![0u8; 1_000]).expect("part");

        // State that could only have come from the cache: it covers the same
        // number of bytes, and different ones.
        let mut primed = Sha256::new();
        primed.update(vec![7u8; 1_000]);
        remember_prefix(&oid, 1_000, &primed);

        let (offset, hasher) = store.resume_offset(&oid).expect("resume");
        assert_eq!(offset, 1_000);
        assert_eq!(
            hex::encode(hasher.finalize()),
            hex::encode(primed.finalize()),
            "the remembered hasher came back, so the file was not re-read"
        );
        forget_prefix(&oid);
    }

    /// The safety check, and the whole of the argument for keeping state at
    /// all: it is handed back only while the partial is exactly as long as it
    /// was. A file that grew or shrank under us falls back to reading, which is
    /// always correct and merely slower.
    #[test]
    fn a_prefix_whose_file_has_moved_on_is_ignored() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = LfsStore::in_git_dir(dir.path().join(".git"));
        store.ensure_layout().expect("layout");
        let oid = "b".repeat(64);
        std::fs::write(store.incomplete_path(&oid), vec![0u8; 1_000]).expect("part");

        let mut stale = Sha256::new();
        stale.update(vec![7u8; 900]);
        remember_prefix(&oid, 900, &stale);

        let (offset, hasher) = store.resume_offset(&oid).expect("resume");
        assert_eq!(offset, 1_000, "the file is what it is, not what we recall");
        let mut honest = Sha256::new();
        honest.update(vec![0u8; 1_000]);
        assert_eq!(
            hex::encode(hasher.finalize()),
            hex::encode(honest.finalize()),
            "and the digest is over the bytes actually on disk"
        );
        forget_prefix(&oid);
    }

    /// Nothing is kept for an object that is no longer partial.
    #[test]
    fn a_committed_object_leaves_no_state_behind() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = LfsStore::in_git_dir(dir.path().join(".git"));
        store.ensure_layout().expect("layout");
        let oid = "c".repeat(64);
        std::fs::write(store.incomplete_path(&oid), vec![0u8; 10]).expect("part");
        let mut primed = Sha256::new();
        primed.update(vec![7u8; 10]);
        remember_prefix(&oid, 10, &primed);

        forget_prefix(&oid);

        let (offset, hasher) = store.resume_offset(&oid).expect("resume");
        let mut honest = Sha256::new();
        honest.update(vec![0u8; 10]);
        assert_eq!(offset, 10);
        assert_eq!(
            hex::encode(hasher.finalize()),
            hex::encode(honest.finalize()),
            "forgotten state means the file is read, as it is after a restart"
        );
    }

    /// The mechanism behind the helper look, one matrix row at a time and
    /// without a process: the marker is a locked file, and a lock is what a
    /// dead process cannot keep.
    mod helpers {
        use super::*;

        const A_MINUTE: Duration = Duration::from_secs(60);

        #[test]
        fn a_registered_helper_holds_a_locked_marker_a_look_counts_alive() {
            let (_dir, store) = store();
            let marker = store.register_helper(4242).expect("register");
            assert_eq!(marker.path(), store.helpers_dir().join("4242"));
            assert!(marker.path().is_file());
            assert!(
                !store.helpers_dir().join(".4242.registering").exists(),
                "the staging name is gone once the marker is in place"
            );

            let look = store.look_at_helpers(SystemTime::now(), LfsStore::STUCK_HELPER_IDLE);
            assert_eq!(
                look,
                HelperLook {
                    alive: 1,
                    stuck: Vec::new(),
                    removed: 0,
                    skipped: Vec::new()
                },
                "a locked marker is a live helper, and one seconds old is not stuck"
            );
            assert!(
                marker.path().is_file(),
                "a look never unlinks a live helper's marker"
            );
        }

        #[test]
        fn a_helper_idle_past_the_threshold_is_stuck_and_a_touch_makes_it_busy_again() {
            let (_dir, store) = store();
            let marker = store.register_helper(4242).expect("register");

            // The clock, not the file, is what moves: `now` is what the look
            // measures idle against, and thirty-one minutes of it is one past
            // the line.
            let later = SystemTime::now() + LfsStore::STUCK_HELPER_IDLE + A_MINUTE;
            let look = store.look_at_helpers(later, LfsStore::STUCK_HELPER_IDLE);
            assert_eq!(look.alive, 1);
            assert_eq!(look.removed, 0);
            let (pid, idle) = look.oldest_stuck().expect("stuck after the threshold");
            assert_eq!(pid, 4242);
            assert!(idle >= LfsStore::STUCK_HELPER_IDLE, "idle={idle:?}");

            // A request finished: the helper on a forty-minute conversion is
            // as busy as one answering every second.
            marker.touch().expect("touch");
            let look =
                store.look_at_helpers(SystemTime::now() + A_MINUTE, LfsStore::STUCK_HELPER_IDLE);
            assert_eq!(look.alive, 1);
            assert!(
                look.stuck.is_empty(),
                "a touched marker is not stuck: {look:?}"
            );
        }

        #[test]
        fn a_dead_helpers_leftover_is_removed_and_never_stuck() {
            let (_dir, store) = store();
            // What a killed helper leaves: the file, and no lock — the kernel
            // released that with the process. Old enough that an idle rule
            // alone would have called it stuck.
            std::fs::create_dir_all(store.helpers_dir()).expect("helpers dir");
            let leftover = store.helpers_dir().join("31337");
            std::fs::write(&leftover, b"").expect("write leftover");
            let long_ago = SystemTime::now() - Duration::from_secs(4 * 3600);
            File::options()
                .write(true)
                .open(&leftover)
                .expect("open leftover")
                .set_modified(long_ago)
                .expect("age leftover");

            let look = store.look_at_helpers(SystemTime::now(), LfsStore::STUCK_HELPER_IDLE);
            assert_eq!(
                look,
                HelperLook {
                    alive: 0,
                    stuck: Vec::new(),
                    removed: 1,
                    skipped: Vec::new()
                }
            );
            assert!(!leftover.exists(), "the leftover is unlinked");

            // And the next look has nothing to say about it.
            let look = store.look_at_helpers(SystemTime::now(), LfsStore::STUCK_HELPER_IDLE);
            assert_eq!(look, HelperLook::default());
        }

        #[test]
        fn a_live_marker_beside_a_leftover_is_told_apart_by_the_lock_alone() {
            let (_dir, store) = store();
            let live = store.register_helper(1).expect("register");
            std::fs::write(store.helpers_dir().join("2"), b"").expect("write leftover");

            let later = SystemTime::now() + LfsStore::STUCK_HELPER_IDLE + A_MINUTE;
            let look = store.look_at_helpers(later, LfsStore::STUCK_HELPER_IDLE);
            assert_eq!(look.alive, 1);
            assert_eq!(look.removed, 1);
            assert_eq!(look.stuck, vec![(1, look.stuck[0].1)]);
            assert!(live.path().exists());
            assert!(!store.helpers_dir().join("2").exists());
        }

        #[test]
        fn dropping_the_registration_unlinks_the_marker() {
            let (_dir, store) = store();
            let marker = store.register_helper(4242).expect("register");
            let path = marker.path().to_path_buf();
            drop(marker);
            assert!(
                !path.exists(),
                "a helper that returns leaves nothing behind"
            );
            assert_eq!(
                store.look_at_helpers(SystemTime::now(), LfsStore::STUCK_HELPER_IDLE),
                HelperLook::default()
            );
        }

        #[test]
        fn a_look_with_no_helpers_directory_is_not_an_error() {
            let dir = tempfile::tempdir().expect("temp dir");
            let store = LfsStore::in_git_dir(dir.path());
            assert_eq!(
                store.look_at_helpers(SystemTime::now(), LfsStore::STUCK_HELPER_IDLE),
                HelperLook::default()
            );
        }

        #[test]
        fn registration_that_cannot_create_the_directory_is_an_error_not_a_failure() {
            let dir = tempfile::tempdir().expect("temp dir");
            let store = LfsStore::in_git_dir(dir.path());
            std::fs::create_dir_all(store.root()).expect("lfs dir");
            // A regular file where the directory has to go.
            std::fs::write(store.helpers_dir(), b"not a directory").expect("block it");
            let err = store.register_helper(4242).expect_err("cannot register");
            assert!(
                !err.to_string().is_empty(),
                "the caller puts the reason on stderr, so there has to be one"
            );
        }

        /// The layout is what `run_process` fails on before it serves, so
        /// `helpers/` must not be part of it: a helper that cannot register
        /// still serves, and that needs the directory to be registration's
        /// problem alone.
        #[test]
        fn the_layout_leaves_helpers_to_registration() {
            let (_dir, store) = store();
            assert!(
                !store.helpers_dir().exists(),
                "ensure_layout does not create helpers/"
            );
            let marker = store.register_helper(4242).expect("register");
            assert!(store.helpers_dir().is_dir(), "register_helper does");
            drop(marker);
        }

        /// Eighteen stuck at once is the field case; the warning names one,
        /// and it has to be the one idle longest.
        #[test]
        fn of_two_stuck_helpers_the_look_names_the_older() {
            let (_dir, store) = store();
            let younger = store.register_helper(10).expect("register 10");
            let older = store.register_helper(20).expect("register 20");
            let now = SystemTime::now();
            older
                .file
                .set_modified(now - Duration::from_secs(2 * 3600))
                .expect("age the older");
            younger
                .file
                .set_modified(now - Duration::from_secs(3600))
                .expect("age the younger");

            let look = store.look_at_helpers(now, LfsStore::STUCK_HELPER_IDLE);
            assert_eq!(look.alive, 2);
            assert_eq!(look.stuck.len(), 2, "{look:?}");
            let (pid, idle) = look.oldest_stuck().expect("two stuck");
            assert_eq!(pid, 20, "the older of the two");
            assert!(idle >= Duration::from_secs(2 * 3600), "idle={idle:?}");
        }

        /// The pid-reuse race: between the look's `open` of a leftover and its
        /// `remove_file`, a new helper that drew the same pid renames its
        /// fresh, locked marker over the leftover's name. Unlinking by path
        /// would take the live helper's marker; unlinking by identity leaves
        /// it.
        #[test]
        fn a_leftover_replaced_by_a_new_helper_under_the_same_pid_is_left_alone() {
            let (_dir, store) = store();
            let path = store.helpers_dir().join("4242");
            std::fs::create_dir_all(store.helpers_dir()).expect("helpers dir");
            std::fs::write(&path, b"").expect("write leftover");

            // The seam: the new helper registers at the instant the look has
            // the leftover's lock and has not yet unlinked its path.
            let mut replacement: Option<HelperMarker> = None;
            let store_for_seam = store.clone();
            let mut before_unlink = |at: &Path| {
                assert_eq!(at, path.as_path());
                replacement = Some(
                    store_for_seam
                        .register_helper(4242)
                        .expect("the new helper registers"),
                );
            };
            let look = store.look_at_helpers_via(
                SystemTime::now(),
                LfsStore::STUCK_HELPER_IDLE,
                &mut LookSeams {
                    try_lock: &|file| file.try_lock_exclusive(),
                    before_unlink: &mut before_unlink,
                },
            );
            assert_eq!(look.removed, 0, "nothing unlinked by name: {look:?}");
            let replacement = replacement.expect("the seam ran");
            assert!(
                path.is_file(),
                "the new helper's marker still stands at the reused name"
            );
            let probe = File::options()
                .read(true)
                .write(true)
                .open(&path)
                .expect("open the marker");
            assert!(
                !probe.try_lock_exclusive().expect("try"),
                "and it is the new helper's, still locked"
            );
            drop(probe);

            // The next look sees one live helper and no leftover.
            let look = store.look_at_helpers(SystemTime::now(), LfsStore::STUCK_HELPER_IDLE);
            assert_eq!(look.alive, 1);
            assert_eq!(look.removed, 0);
            drop(replacement);
        }

        /// A filesystem that refuses `flock` must not make the look read as
        /// healthy: the markers it could not place are reported, with their
        /// idle, for the caller to place by pid.
        #[test]
        fn markers_whose_lock_cannot_be_tried_are_skipped_not_dropped() {
            let (_dir, store) = store();
            let live = store.register_helper(1).expect("register");
            live.file
                .set_modified(SystemTime::now() - Duration::from_secs(3600))
                .expect("age");
            std::fs::write(store.helpers_dir().join("2"), b"").expect("leftover");

            let refused = |_: &File| -> std::io::Result<bool> {
                Err(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "no locks here",
                ))
            };
            let look = store.look_at_helpers_via(
                SystemTime::now(),
                LfsStore::STUCK_HELPER_IDLE,
                &mut LookSeams {
                    try_lock: &refused,
                    before_unlink: &mut |_| {},
                },
            );
            assert_eq!(look.alive, 0);
            assert_eq!(look.removed, 0);
            assert!(look.stuck.is_empty());
            let mut skipped: Vec<u32> = look.skipped.iter().map(|(pid, _)| *pid).collect();
            skipped.sort_unstable();
            assert_eq!(skipped, vec![1, 2], "both markers, neither placed");
            let (_, idle) = look
                .skipped
                .iter()
                .find(|(pid, _)| *pid == 1)
                .expect("pid 1");
            assert!(
                *idle >= Duration::from_secs(3600),
                "with its idle: {idle:?}"
            );
            assert!(
                store.helpers_dir().join("2").exists(),
                "nothing is unlinked on a lock the look could not try"
            );
        }

        /// A staging file is a helper mid-registration: young, it is left
        /// alone; old and unlocked, it is a registration that died and is
        /// unlinked without counting as a removed helper. Anything that is
        /// not keeper's name is not keeper's to touch.
        #[test]
        fn staging_files_are_left_alone_unless_stale_and_foreign_files_always() {
            let (_dir, store) = store();
            let dir = store.helpers_dir();
            std::fs::create_dir_all(&dir).expect("helpers dir");
            let young = dir.join(".7.registering");
            let stale = dir.join(".8.registering");
            let foreign = dir.join("README");
            for path in [&young, &stale, &foreign] {
                std::fs::write(path, b"").expect("write");
            }
            let long_ago = SystemTime::now() - Duration::from_secs(4 * 3600);
            for path in [&stale, &foreign] {
                File::options()
                    .write(true)
                    .open(path)
                    .expect("open")
                    .set_modified(long_ago)
                    .expect("age");
            }

            let look = store.look_at_helpers(SystemTime::now(), LfsStore::STUCK_HELPER_IDLE);
            assert_eq!(
                look,
                HelperLook::default(),
                "none of these is a helper, alive, stuck, removed or skipped"
            );
            assert!(young.exists(), "a registration in progress is left alone");
            assert!(!stale.exists(), "a registration that died is cleaned up");
            assert!(
                foreign.exists(),
                "a file keeper did not name is not keeper's"
            );
        }
    }
}
