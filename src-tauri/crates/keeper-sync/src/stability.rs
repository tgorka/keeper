//! Tiers 2, 3 and 4 of the completeness gate (Stories 26.3, 26.4, 26.5; AD-45).
//!
//! Tier 0 ([`crate::exclude`]) removes known noise and tier 1
//! ([`crate::watch`]) says *when* to look. This module decides whether a file
//! is finished being written, and the governing fact is that **no tier here is
//! a proof except tier 4**:
//!
//! * **Tier 2, quiescence.** `(size, mtime_ns, ctime_ns, inode)` identical
//!   across two samples `W` apart. Cheap (~1.2 µs per `lstat`) and correct for
//!   almost everything, but a writer that pauses longer than `W` fools it.
//! * **Tier 3, open-writer veto.** A single `/proc/locks` read on Linux. Costs
//!   ~0.01 ms and occasionally saves us; almost nothing takes advisory locks,
//!   so it can only ever veto, never approve. **There is no macOS equivalent**
//!   — see [`open_writer_veto`] for why that is a platform limit rather than
//!   an omission.
//! * **Tier 4, verify-on-read.** `fstat` the open fd, stream SHA-256, `fstat`
//!   again. This is the only actual proof in the whole gate, and it is the
//!   reason the other three are allowed to be approximations.
//!
//! # The iCloud hazard
//!
//! Orthogonal to all four tiers, and mandatory: on macOS a file may be a
//! **dataless placeholder** whose bytes live in iCloud. `stat` is safe;
//! `open` is not — opening one silently materializes it. A sync engine that
//! hashes a Documents tree under iCloud Drive without checking `SF_DATALESS`
//! drags the user's entire cloud library down over their network. Every path
//! into an `open` in this module goes through that check first.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::{Result, SyncError};
use crate::exclude::ExcludeSet;
use crate::profile::{SyncProfile, CLOSE_WRITE_SETTLE_MS, SETTLE_CEILING_MS};

const NANOS_PER_MILLI: i128 = 1_000_000;

/// How far ahead of the local clock an mtime may sit before we stop believing
/// it (Nextcloud's escape hatch).
///
/// Without this a machine with a broken clock — or a file copied from one —
/// wedges forever: `now - mtime` never reaches the window, so the file is held
/// back for as long as the profile exists.
pub const FUTURE_MTIME_GRACE_MS: i64 = 10_000;

/// macOS `SF_DATALESS` (`bsd/sys/stat.h`): the file is a File-Provider /
/// iCloud placeholder with no local data.
///
/// Defined on every platform so the constant is greppable and documented in one
/// place; only the macOS build reads it.
pub const SF_DATALESS: u32 = 0x4000_0000;

/// Bytes hashed per read in tier 4.
///
/// Bounded on purpose (NFR-23): a 50 GB profile must not turn into a 50 GB
/// allocation, and the hash is I/O-bound long before it is syscall-bound.
const VERIFY_CHUNK_BYTES: usize = 128 * 1024;

/// The identity tuple tier 2 compares across observations.
///
/// `ctime` and `inode` are in the tuple because `(size, mtime)` alone cannot
/// see an atomic-rename replacement: `mv new old` gives the path a different
/// inode, and a restored backup can reproduce a byte-identical size and mtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileSample {
    pub size: u64,
    pub mtime_ns: i128,
    pub ctime_ns: i128,
    pub inode: u64,
}

impl FileSample {
    /// Sample `path` without following symlinks.
    ///
    /// An absent file is `Ok(None)`, not an error: between an event and the
    /// sample the user may simply have deleted the file, which is an ordinary
    /// outcome the caller turns into [`StabilityVerdict::Vanished`].
    pub fn of(path: &Path) -> Result<Option<Self>> {
        match std::fs::symlink_metadata(path) {
            Ok(md) => Ok(Some(Self::from_metadata(&md))),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(SyncError::io("stat", path, err)),
        }
    }

    /// The file's mtime in wall-clock milliseconds.
    ///
    /// Clamped rather than wrapped: a filesystem reporting a nonsense timestamp
    /// must not silently become a negative or wrapped-around window.
    pub fn mtime_ms(&self) -> i64 {
        let ms =
            (self.mtime_ns / NANOS_PER_MILLI).clamp(i128::from(i64::MIN), i128::from(i64::MAX));
        ms as i64
    }

    #[cfg(unix)]
    fn from_metadata(md: &std::fs::Metadata) -> Self {
        use std::os::unix::fs::MetadataExt as _;
        const NANOS_PER_SEC: i128 = 1_000_000_000;
        Self {
            size: md.len(),
            mtime_ns: i128::from(md.mtime()) * NANOS_PER_SEC + i128::from(md.mtime_nsec()),
            ctime_ns: i128::from(md.ctime()) * NANOS_PER_SEC + i128::from(md.ctime_nsec()),
            inode: md.ino(),
        }
    }

    #[cfg(not(unix))]
    fn from_metadata(md: &std::fs::Metadata) -> Self {
        // No portable `ctime` or inode exists, so both are pinned to zero and
        // the tuple degrades to `(size, mtime)`. That is strictly weaker — an
        // atomic-rename replacement carrying an identical size and mtime
        // becomes invisible to tier 2 — which is one more reason tier 4 is the
        // only thing allowed to authorize a commit.
        let mtime_ns = md
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| i128::try_from(d.as_nanos()).unwrap_or(i128::MAX));
        Self {
            size: md.len(),
            mtime_ns,
            ctime_ns: 0,
            inode: 0,
        }
    }
}

/// What the gate concluded about one path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StabilityVerdict {
    /// Safe to hand to tier 4 and then to the index.
    Stable,
    /// Still moving. `since_ms` is when the current `(size, mtime, ctime,
    /// inode)` tuple was first observed, so a UI can render "unchanged for N s"
    /// without keeping its own clock.
    Settling { since_ms: i64 },
    /// The path no longer exists. Not an error — deletion is ordinary, and the
    /// caller decides whether it propagates (AD-48: on a detached volume it
    /// never does).
    Vanished,
    /// A macOS iCloud placeholder. Deliberately distinct from `Excluded` so the
    /// caller can *warn* the user that a file is being skipped, rather than
    /// silently omitting content they can see in Finder.
    Dataless,
    /// Filtered by tier 0. Invisible, never pending.
    Excluded,
}

/// Whether `path` is a macOS dataless placeholder.
///
/// Cheap (`lstat`) and safe: reading the flag does **not** materialize the
/// file, whereas `open` does. Always `Ok(false)` off macOS.
pub fn is_dataless(path: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(md) => Ok(metadata_is_dataless(&md)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(SyncError::io("stat", path, err)),
    }
}

#[cfg(target_os = "macos")]
fn metadata_is_dataless(md: &std::fs::Metadata) -> bool {
    // `st_flags` lives on the macOS-specific extension trait, not the unix one.
    use std::os::macos::fs::MetadataExt as _;
    md.st_flags() & SF_DATALESS != 0
}

#[cfg(not(target_os = "macos"))]
fn metadata_is_dataless(_md: &std::fs::Metadata) -> bool {
    // There is no `SF_DATALESS` equivalent on Linux or Windows: a FUSE-backed
    // cloud mount either has the bytes or fails the read, and neither one is
    // triggered by opening.
    false
}

/// Per-path tier-2 state: the in-memory mirror of `sync.db`'s `file_state`
/// table, kept hot so a scan of 100 000 files does not become 100 000 queries.
#[derive(Debug, Clone, Copy)]
struct Entry {
    /// The most recent sample.
    last: FileSample,
    /// When `last` was **first** observed. The quiescence window is measured
    /// from here, so repeated identical observations extend the run rather than
    /// restarting it.
    unchanged_since_ms: i64,
    /// When this pending episode began. Anchors the hard ceiling.
    pending_since_ms: i64,
    /// A Linux close-write was seen during the current unchanged run.
    close_write: bool,
}

/// A gate entry in a form the durable store can hold (Story 26.3).
///
/// Deliberately a separate, all-public type rather than exposing `Entry`: the
/// in-memory representation is free to change, but this one is a persistence
/// contract that has to survive a schema round trip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PersistedEntry {
    pub sample: FileSample,
    pub unchanged_since_ms: i64,
    pub pending_since_ms: i64,
    pub close_write: bool,
}

/// The quiescence gate for one profile.
///
/// Holds tier 0 as well, so `is_stable` is a single complete answer rather than
/// a sequence the caller has to remember to compose in the right order — the
/// order matters (an excluded path must never be stat-ed, let alone opened).
#[derive(Debug)]
pub struct StabilityGate {
    root: PathBuf,
    excludes: ExcludeSet,
    settle_ms: u64,
    entries: HashMap<PathBuf, Entry>,
    /// Close-write events that arrived from the watcher but have not yet been
    /// folded into an observation.
    pending_close_write: HashSet<PathBuf>,
}

impl StabilityGate {
    pub fn new(root: impl Into<PathBuf>, excludes: ExcludeSet, settle_ms: u64) -> Self {
        Self {
            root: root.into(),
            excludes,
            settle_ms,
            entries: HashMap::new(),
            pending_close_write: HashSet::new(),
        }
    }

    /// Build the gate a profile implies: its root, its excludes, and its
    /// effective window (which already accounts for removable media).
    pub fn for_profile(profile: &SyncProfile) -> Result<Self> {
        Ok(Self::new(
            profile.local_path.clone(),
            ExcludeSet::new(&profile.excludes)?,
            profile.effective_settle_ms(),
        ))
    }

    /// The window this gate applies when no close-write was seen.
    pub fn settle_ms(&self) -> u64 {
        self.settle_ms
    }

    /// Record a Linux `IN_CLOSE_WRITE` for `path`.
    ///
    /// Never a completeness proof on its own — close-then-reopen is ordinary
    /// behaviour for git, `dd conv=notrunc`, torrent clients and database WALs
    /// — so all it does is shorten the window to
    /// [`CLOSE_WRITE_SETTLE_MS`], which then only has to absorb mtime
    /// granularity.
    pub fn note_close_write(&mut self, path: &Path) {
        self.pending_close_write.insert(path.to_path_buf());
    }

    /// Fold one sample into the per-path state.
    ///
    /// A changed tuple restarts the unchanged run *and* clears the close-write
    /// fast path: if the bytes moved after the close, a writer reopened the
    /// file and the earlier close proved nothing about the new content.
    pub fn observe(&mut self, path: &Path, sample: FileSample, now_ms: i64, close_write: bool) {
        if let Some(entry) = self.entries.get_mut(path) {
            if entry.last == sample {
                entry.close_write |= close_write;
            } else {
                entry.last = sample;
                entry.unchanged_since_ms = now_ms;
                entry.close_write = close_write;
            }
            return;
        }
        self.entries.insert(
            path.to_path_buf(),
            Entry {
                last: sample,
                unchanged_since_ms: now_ms,
                pending_since_ms: now_ms,
                close_write,
            },
        );
    }

    /// Tier 2's answer for an already-observed path.
    ///
    /// `Stable` requires the tuple to have been identical across two
    /// observations at least `W` apart **and** the mtime itself to be at least
    /// `W` old — the second condition is what stops a file that was written
    /// once, quickly, from being committed inside the filesystem's own
    /// timestamp granularity.
    pub fn verdict(&self, path: &Path, now_ms: i64, settle_ms: u64) -> StabilityVerdict {
        let Some(entry) = self.entries.get(path) else {
            // Never observed: one sample is not two, so it cannot be stable.
            return StabilityVerdict::Settling { since_ms: now_ms };
        };

        // The hard ceiling. A continuously appended log — a build output, a
        // running capture, a syslog — must eventually sync even though it never
        // quiesces. Checked first so no other condition can outvote it.
        let ceiling = i64::try_from(SETTLE_CEILING_MS).unwrap_or(i64::MAX);
        if now_ms.saturating_sub(entry.pending_since_ms) >= ceiling {
            return StabilityVerdict::Stable;
        }

        let effective = if entry.close_write {
            CLOSE_WRITE_SETTLE_MS
        } else {
            settle_ms
        };
        let window = i64::try_from(effective.min(SETTLE_CEILING_MS)).unwrap_or(i64::MAX);

        if now_ms.saturating_sub(entry.unchanged_since_ms) < window {
            return StabilityVerdict::Settling {
                since_ms: entry.unchanged_since_ms,
            };
        }

        // Future-mtime escape hatch: a timestamp implausibly far ahead of our
        // clock is not evidence of a recent write, it is evidence of a broken
        // clock (or a file copied from one). Holding the file on it would wedge
        // the profile forever, so the mtime condition is simply skipped.
        let mtime_ms = entry.last.mtime_ms();
        let implausibly_future = mtime_ms > now_ms.saturating_add(FUTURE_MTIME_GRACE_MS);
        if !implausibly_future && now_ms.saturating_sub(mtime_ms) < window {
            return StabilityVerdict::Settling {
                since_ms: entry.unchanged_since_ms,
            };
        }

        StabilityVerdict::Stable
    }

    /// The full gate for one path: tier 0, the iCloud guard, a fresh sample,
    /// then tier 2.
    ///
    /// Returning `Stable` **ends the episode**: the entry is dropped, so a file
    /// that keeps changing is forced through by the ceiling at most once per
    /// [`SETTLE_CEILING_MS`] rather than on every subsequent call. `Vanished`
    /// drops it too. Callers driving `observe`/`verdict` by hand own that
    /// bookkeeping themselves through [`StabilityGate::forget`].
    pub fn is_stable(&mut self, path: &Path, now_ms: i64) -> StabilityVerdict {
        let relative = path.strip_prefix(&self.root).unwrap_or(path);
        if self.excludes.is_excluded(relative) {
            return StabilityVerdict::Excluded;
        }

        // Before anything that could open the file.
        match is_dataless(path) {
            Ok(true) => {
                self.forget(path);
                return StabilityVerdict::Dataless;
            }
            Ok(false) => {}
            Err(err) => {
                tracing::warn!(path = %path.display(), %err, "dataless probe failed");
                return StabilityVerdict::Settling { since_ms: now_ms };
            }
        }

        let sample = match FileSample::of(path) {
            Ok(Some(sample)) => sample,
            Ok(None) => {
                self.forget(path);
                return StabilityVerdict::Vanished;
            }
            Err(err) => {
                // A path we cannot stat is never "stable": failing closed keeps
                // an unreadable file out of a commit instead of into one.
                tracing::warn!(path = %path.display(), %err, "stat failed; holding file");
                return StabilityVerdict::Settling { since_ms: now_ms };
            }
        };

        let close_write = self.pending_close_write.remove(path);
        let settle_ms = self.settle_ms;
        self.observe(path, sample, now_ms, close_write);
        let verdict = self.verdict(path, now_ms, settle_ms);
        if verdict == StabilityVerdict::Stable {
            self.forget(path);
        }
        verdict
    }

    /// Drop all state for `path`, ending its pending episode.
    pub fn forget(&mut self, path: &Path) {
        self.entries.remove(path);
        self.pending_close_write.remove(path);
    }

    /// Drop all state. Used when a profile is paused or its volume detaches:
    /// windows measured against a clock from before the pause are meaningless.
    pub fn forget_all(&mut self) {
        self.entries.clear();
        self.pending_close_write.clear();
    }

    /// How many paths are mid-episode. Feeds the "N pending" status line.
    pub fn tracked(&self) -> usize {
        self.entries.len()
    }

    /// Export every mid-episode entry so it can be persisted.
    ///
    /// Without this the gate is process-local, and a one-shot `sync --once`
    /// could never reach a second observation — every invocation would start
    /// from zero and hold every file forever. The `file_state` table exists
    /// precisely to carry these across runs.
    pub fn export(&self) -> Vec<(PathBuf, PersistedEntry)> {
        self.entries
            .iter()
            .map(|(path, entry)| {
                (
                    path.clone(),
                    PersistedEntry {
                        sample: entry.last,
                        unchanged_since_ms: entry.unchanged_since_ms,
                        pending_since_ms: entry.pending_since_ms,
                        close_write: entry.close_write,
                    },
                )
            })
            .collect()
    }

    /// Restore previously exported entries.
    ///
    /// An entry already in memory wins: this process observed it, so it is
    /// newer than anything read back from disk.
    pub fn import(&mut self, entries: impl IntoIterator<Item = (PathBuf, PersistedEntry)>) {
        for (path, saved) in entries {
            self.entries.entry(path).or_insert(Entry {
                last: saved.sample,
                unchanged_since_ms: saved.unchanged_since_ms,
                pending_since_ms: saved.pending_since_ms,
                close_write: saved.close_write,
            });
        }
    }
}

/// Tier 3 — is a writer holding an advisory write lock on this file?
///
/// Best-effort and cheap: one small read of `/proc/locks` (~0.01 ms for the
/// whole file), matched on `MAJOR:MINOR:INODE`. A `WRITE` lock means "not
/// stable"; the absence of one means nothing at all, because almost nothing
/// takes advisory locks — browsers, `curl`, `cp` and most editors do not. It is
/// kept only because it costs nothing and occasionally saves us.
#[cfg(target_os = "linux")]
pub fn open_writer_veto(path: &Path, sample: &FileSample) -> bool {
    use std::os::unix::fs::MetadataExt as _;

    const PROC_LOCKS: &str = "/proc/locks";

    let Ok(md) = std::fs::symlink_metadata(path) else {
        // Gone, or unreadable. Neither is a veto: `Vanished` and the stat
        // failure path already handle those, and double-reporting them here
        // would just make the reason harder to find.
        return false;
    };
    if md.ino() != sample.inode {
        // Replaced since the sample was taken, so tier 2's window is already
        // invalid and will restart. Vetoing on the new inode adds nothing.
        return false;
    }
    let Ok(locks) = std::fs::read_to_string(PROC_LOCKS) else {
        // No procfs (a container, a hardened kernel). Best-effort means the
        // tier simply does not fire.
        return false;
    };
    locks
        .lines()
        .any(|line| lock_line_vetoes(line, md.dev(), sample.inode))
}

/// Tier 3 does not exist off Linux, and on macOS that is a hard platform limit
/// rather than an omission:
///
/// * **`lsof`** forks a heavyweight binary and takes hundreds of milliseconds
///   to seconds on a busy Mac — unusable per file, and fragile to shell out to
///   from a notarized, sandboxed app.
/// * **`libproc`** (`proc_listpids` + `proc_pidfdinfo`) needs root to see
///   other users' and root-owned processes; without it the answer is silently
///   wrong rather than merely unavailable.
/// * **EndpointSecurity**'s `ES_EVENT_TYPE_NOTIFY_CLOSE` is the only true
///   signal, and `es_new_client` returns
///   `ES_NEW_CLIENT_RESULT_ERR_NOT_ENTITLED` without
///   `com.apple.developer.endpoint-security.client` — an entitlement that must
///   be requested from Apple and also requires a System Extension and Full
///   Disk Access.
///
/// So macOS runs tiers 0, 1, 2 and 4, and leans on tier 4 — which is the only
/// proof on either platform anyway.
#[cfg(not(target_os = "linux"))]
pub fn open_writer_veto(_path: &Path, _sample: &FileSample) -> bool {
    false
}

/// Does one `/proc/locks` line describe a write lock on `(dev, ino)`?
///
/// Kept compiled on every platform — it is pure text parsing — so the macOS CI
/// runner exercises it too.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn lock_line_vetoes(line: &str, dev: u64, ino: u64) -> bool {
    // `ID: [->] TYPE FLAGS ACCESS PID MAJOR:MINOR:INODE START END`
    //
    // The `->` marker precedes a *blocked* lock request; dropping it keeps the
    // field offsets stable. The kernel prints MAJOR and MINOR in **hex**
    // (`%02x:%02x`) and the inode in decimal, which is why they cannot be
    // compared against `st_dev` without reassembly.
    let mut fields = line.split_whitespace().skip(1).filter(|f| *f != "->");
    let _lock_type = fields.next();
    let _flags = fields.next();
    if fields.next() != Some("WRITE") {
        return false;
    }
    let _pid = fields.next();
    let Some(target) = fields.next() else {
        return false;
    };
    let mut parts = target.split(':');
    let (Some(major), Some(minor), Some(inode)) = (parts.next(), parts.next(), parts.next()) else {
        // `<none>:0` — a lock with no backing inode.
        return false;
    };
    let (Ok(major), Ok(minor), Ok(inode)) = (
        u64::from_str_radix(major, 16),
        u64::from_str_radix(minor, 16),
        inode.parse::<u64>(),
    ) else {
        return false;
    };
    inode == ino && makedev(major, minor) == dev
}

/// Reassemble a Linux `dev_t` from its major and minor parts, exactly as
/// glibc's `makedev` does. The encoding is not `(major << 8) | minor`: the high
/// bits of both parts live above bit 32.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn makedev(major: u64, minor: u64) -> u64 {
    ((major & 0xfff) << 8) | (minor & 0xff) | ((major & !0xfff) << 32) | ((minor & !0xff) << 12)
}

/// Tier 4 — the only proof in the gate.
///
/// `fstat`s the **open descriptor**, not the path, before and after streaming
/// the content: fstat-ing the path would reopen a TOCTOU window every time,
/// because a rename between the two stats would compare two different files.
/// Returns `(sha256-hex, bytes)`.
///
/// A size, mtime, ctime or inode change across the read means the bytes we
/// hashed never existed as a single coherent version of the file, so the result
/// is discarded and the unit is retried from zero — never resumed from a
/// poisoned prefix.
pub fn verify_while_reading(path: &Path) -> Result<(String, u64)> {
    verify_while_reading_hooked(path, || {})
}

/// The body of [`verify_while_reading`], with a hook fired once after the first
/// chunk is consumed.
///
/// The hook exists so torn-read detection can be tested deterministically:
/// there is no other way to mutate a file *during* a synchronous read, and a
/// thread-and-sleep test of this specific invariant would be flakier than no
/// test at all. Production passes an empty closure, which compiles away.
fn verify_while_reading_hooked(
    path: &Path,
    mut after_first_chunk: impl FnMut(),
) -> Result<(String, u64)> {
    let md = std::fs::symlink_metadata(path).map_err(|err| SyncError::io("stat", path, err))?;

    if metadata_is_dataless(&md) {
        // Defence in depth: `is_stable` already reports `Dataless`, but this is
        // the function that actually opens the file, so the guard has to live
        // here too. Opening it would start a silent multi-gigabyte download.
        return Err(SyncError::InvalidPathForRemote {
            path: path.to_path_buf(),
            reason: "dataless iCloud placeholder; opening it would materialize the file".into(),
        });
    }
    if !md.is_file() {
        // Not pedantry: opening a FIFO blocks until a writer appears, which
        // would hang the profile supervisor forever.
        return Err(SyncError::io(
            "verify",
            path,
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "not a regular file"),
        ));
    }

    let mut file = File::open(path).map_err(|err| SyncError::io("open", path, err))?;
    let before = FileSample::from_metadata(
        &file
            .metadata()
            .map_err(|err| SyncError::io("fstat", path, err))?,
    );

    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; VERIFY_CHUNK_BYTES];
    let mut bytes: u64 = 0;
    let mut hooked = false;
    loop {
        let read = file
            .read(&mut buf)
            .map_err(|err| SyncError::io("read", path, err))?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
        bytes = bytes.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
        if !hooked {
            hooked = true;
            after_first_chunk();
        }
    }

    let after = FileSample::from_metadata(
        &file
            .metadata()
            .map_err(|err| SyncError::io("fstat", path, err))?,
    );
    if before != after {
        return Err(SyncError::Integrity {
            subject: path.display().to_string(),
            expected: describe(&before),
            actual: describe(&after),
        });
    }

    Ok((hex::encode(hasher.finalize()), bytes))
}

/// Render a sample for an error message. Sizes and timestamps only — never
/// content, per the error-taxonomy invariant.
fn describe(sample: &FileSample) -> String {
    format!(
        "size={} mtime_ns={} ctime_ns={} ino={}",
        sample.size, sample.mtime_ns, sample.ctime_ns, sample.inode
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const SETTLE: u64 = 5_000;

    fn gate() -> StabilityGate {
        StabilityGate::new(
            "/tmp/profile-root",
            ExcludeSet::new(&[]).expect("built-in corpus compiles"),
            SETTLE,
        )
    }

    /// A synthetic sample whose mtime is expressed in the same millisecond
    /// clock the tests inject, so the two conditions in `verdict` line up.
    fn sample(size: u64, mtime_ms: i64, inode: u64) -> FileSample {
        FileSample {
            size,
            mtime_ns: i128::from(mtime_ms) * NANOS_PER_MILLI,
            ctime_ns: i128::from(mtime_ms) * NANOS_PER_MILLI,
            inode,
        }
    }

    fn p(name: &str) -> PathBuf {
        PathBuf::from("/tmp/profile-root").join(name)
    }

    #[test]
    fn an_unobserved_path_is_never_stable() {
        let g = gate();
        assert!(matches!(
            g.verdict(&p("a.txt"), 10_000, SETTLE),
            StabilityVerdict::Settling { .. }
        ));
    }

    #[test]
    fn a_file_whose_size_changes_between_samples_stays_settling() {
        let mut g = gate();
        let path = p("growing.bin");
        g.observe(&path, sample(1_000, 0, 7), 0, false);
        // Still growing at t=5000: the run restarts here, so even though the
        // path has existed for a full window it is nowhere near stable.
        g.observe(&path, sample(2_000, 5_000, 7), 5_000, false);
        assert_eq!(
            g.verdict(&path, 5_000, SETTLE),
            StabilityVerdict::Settling { since_ms: 5_000 }
        );
        // ...and remains unstable partway through the fresh window.
        assert_eq!(
            g.verdict(&path, 9_000, SETTLE),
            StabilityVerdict::Settling { since_ms: 5_000 }
        );
    }

    #[test]
    fn an_unchanged_file_becomes_stable_exactly_at_the_boundary() {
        let mut g = gate();
        let path = p("done.txt");
        let s = sample(4_096, 0, 7);
        g.observe(&path, s, 0, false);

        // One millisecond early is still Settling. This is the assertion that
        // fails if someone "simplifies" the `>=` boundary into a `>`.
        assert_eq!(
            g.verdict(&path, 4_999, SETTLE),
            StabilityVerdict::Settling { since_ms: 0 }
        );

        // A repeated identical observation extends the run rather than
        // restarting it, so the boundary is still measured from t=0.
        g.observe(&path, s, 4_999, false);
        assert_eq!(g.verdict(&path, 5_000, SETTLE), StabilityVerdict::Stable);
    }

    #[test]
    fn a_fresh_mtime_holds_the_file_even_after_two_matching_samples() {
        let mut g = gate();
        let path = p("touched.txt");
        // Both samples agree, and they are a full window apart — but the mtime
        // says the last write was 1 ms ago, which is inside the filesystem's
        // own granularity. Quiescence alone must not be enough.
        let s = sample(10, 9_999, 7);
        g.observe(&path, s, 0, false);
        g.observe(&path, s, 5_000, false);
        assert_eq!(
            g.verdict(&path, 10_000, SETTLE),
            StabilityVerdict::Settling { since_ms: 0 }
        );
        assert_eq!(g.verdict(&path, 14_999, SETTLE), StabilityVerdict::Stable);
    }

    #[test]
    fn the_close_write_fast_path_shortens_the_window_to_one_second() {
        let mut g = gate();
        let path = p("saved.odt");
        let s = sample(2_048, 0, 7);
        g.observe(&path, s, 0, true);
        assert_eq!(
            CLOSE_WRITE_SETTLE_MS, 1_000,
            "the fast path is defined as one second"
        );
        assert_eq!(g.verdict(&path, 1_000, SETTLE), StabilityVerdict::Stable);
        assert!(matches!(
            g.verdict(&path, 999, SETTLE),
            StabilityVerdict::Settling { .. }
        ));

        // Control: the identical timeline without the close-write event is
        // still held for the full profile window.
        let mut slow = gate();
        slow.observe(&path, s, 0, false);
        assert!(matches!(
            slow.verdict(&path, 1_000, SETTLE),
            StabilityVerdict::Settling { .. }
        ));
    }

    #[test]
    fn a_write_after_a_close_write_clears_the_fast_path() {
        let mut g = gate();
        let path = p("reopened.db");
        g.observe(&path, sample(10, 0, 7), 0, true);
        // Close-then-reopen: git, `dd conv=notrunc` and database WALs all do
        // this, which is exactly why the close is never a proof.
        g.observe(&path, sample(20, 1_000, 7), 1_000, false);
        assert!(matches!(
            g.verdict(&path, 2_000, SETTLE),
            StabilityVerdict::Settling { .. }
        ));
        assert_eq!(g.verdict(&path, 6_000, SETTLE), StabilityVerdict::Stable);
    }

    #[test]
    fn the_ceiling_forces_stable_on_a_file_that_never_quiesces() {
        let mut g = gate();
        let path = p("build.log");
        // Appended to every second forever: tier 2 alone would hold it until
        // the heat death of the universe.
        let mut t = 0i64;
        while t < 59_000 {
            g.observe(&path, sample(100 + t as u64, t, 7), t, false);
            assert!(
                matches!(
                    g.verdict(&path, t, SETTLE),
                    StabilityVerdict::Settling { .. }
                ),
                "still settling at t={t}"
            );
            t += 1_000;
        }
        g.observe(&path, sample(100_000, 59_999, 7), 59_999, false);
        assert!(matches!(
            g.verdict(&path, 59_999, SETTLE),
            StabilityVerdict::Settling { .. }
        ));
        g.observe(&path, sample(101_000, 60_000, 7), 60_000, false);
        assert_eq!(
            g.verdict(&path, 60_000, SETTLE),
            StabilityVerdict::Stable,
            "the {SETTLE_CEILING_MS} ms ceiling must win over an active writer"
        );
    }

    #[test]
    fn a_future_mtime_does_not_wedge_the_file() {
        let mut g = gate();
        let path = p("from-a-broken-clock.txt");
        // mtime a full minute ahead of our clock: `now - mtime` will never
        // reach the window, so without the escape hatch this file is stuck.
        let s = sample(64, 65_000, 7);
        g.observe(&path, s, 0, false);
        assert_eq!(g.verdict(&path, 5_000, SETTLE), StabilityVerdict::Stable);

        // ...but ordinary clock skew, inside the grace, is still respected:
        // this is not a licence to ignore mtime whenever it is inconvenient.
        let mut skewed = gate();
        let near = p("slightly-skewed.txt");
        skewed.observe(&near, sample(64, 7_000, 7), 0, false);
        assert!(matches!(
            skewed.verdict(&near, 5_000, SETTLE),
            StabilityVerdict::Settling { .. }
        ));
    }

    #[test]
    fn an_inode_change_resets_the_window_instead_of_reading_as_stability() {
        let mut g = gate();
        let path = p("config.yaml");
        // `mv tmp config.yaml` — an editor's atomic replacement. Size, mtime
        // and ctime can all coincide; only the inode betrays it, and mistaking
        // it for stability would commit the *old* file's window against the
        // new file's bytes.
        g.observe(&path, sample(512, 0, 7), 0, false);
        g.observe(&path, sample(512, 0, 9), 5_000, false);
        assert_eq!(
            g.verdict(&path, 5_000, SETTLE),
            StabilityVerdict::Settling { since_ms: 5_000 }
        );

        // Control: without the inode change the same timeline is Stable.
        let mut same = gate();
        same.observe(&path, sample(512, 0, 7), 0, false);
        same.observe(&path, sample(512, 0, 7), 5_000, false);
        assert_eq!(same.verdict(&path, 5_000, SETTLE), StabilityVerdict::Stable);
    }

    #[test]
    fn reporting_stable_ends_the_episode_so_the_ceiling_cannot_fire_repeatedly() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("note.txt");
        std::fs::write(&path, b"hello").expect("write");
        let mut g = StabilityGate::new(
            dir.path(),
            ExcludeSet::new(&[]).expect("corpus compiles"),
            0,
        );
        // A zero window makes the first observation sufficient — the real
        // file's epoch mtime is far ahead of the injected clock, so the
        // future-mtime hatch retires the mtime arm as well.
        assert_eq!(g.is_stable(&path, 1_000), StabilityVerdict::Stable);
        assert_eq!(g.tracked(), 0, "a stable path must not stay pending");

        // ...and the ceiling anchor moves with the episode. Without that, a
        // file that keeps changing would be forced through on *every* call
        // after the first minute instead of at most once per minute.
        let mut ceiling = gate();
        let log = p("append.log");
        ceiling.observe(&log, sample(1, 0, 7), 0, false);
        assert_eq!(
            ceiling.verdict(&log, 60_000, SETTLE),
            StabilityVerdict::Stable
        );
        ceiling.forget(&log);
        ceiling.observe(&log, sample(2, 60_000, 7), 60_000, false);
        assert!(matches!(
            ceiling.verdict(&log, 60_001, SETTLE),
            StabilityVerdict::Settling { .. }
        ));
    }

    #[test]
    fn a_deleted_file_yields_vanished_and_is_forgotten() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("gone.txt");
        std::fs::write(&path, b"x").expect("write");
        let mut g = StabilityGate::new(
            dir.path(),
            ExcludeSet::new(&[]).expect("corpus compiles"),
            SETTLE,
        );
        assert!(matches!(
            g.is_stable(&path, 0),
            StabilityVerdict::Settling { .. }
        ));
        assert_eq!(g.tracked(), 1);

        std::fs::remove_file(&path).expect("remove");
        assert_eq!(g.is_stable(&path, 1_000), StabilityVerdict::Vanished);
        assert_eq!(g.tracked(), 0);
        assert_eq!(FileSample::of(&path).expect("absent is not an error"), None);
    }

    #[test]
    fn an_excluded_path_short_circuits_before_the_filesystem_is_touched() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut g = StabilityGate::new(
            dir.path(),
            ExcludeSet::new(&[]).expect("corpus compiles"),
            SETTLE,
        );
        // The file does not exist. If tier 0 did not run first this would be
        // `Vanished`, so the verdict pins the ordering, not just the filter.
        let path = dir.path().join("movie.mkv.crdownload");
        assert_eq!(g.is_stable(&path, 0), StabilityVerdict::Excluded);
        assert_eq!(g.tracked(), 0);
    }

    #[test]
    fn file_sample_reflects_real_metadata() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("sized.bin");
        std::fs::write(&path, vec![0u8; 1234]).expect("write");
        let s = FileSample::of(&path)
            .expect("stat succeeds")
            .expect("file exists");
        assert_eq!(s.size, 1234);
        // A real file always has a positive epoch mtime; a zero here would mean
        // the extraction silently produced a default.
        assert!(s.mtime_ns > 0, "mtime_ns was {}", s.mtime_ns);
        assert!(s.mtime_ms() > 0);
    }

    #[test]
    fn verify_while_reading_returns_the_digest_and_length() {
        let dir = tempfile::tempdir().expect("tempdir");

        let empty = dir.path().join("empty.bin");
        std::fs::write(&empty, b"").expect("write");
        assert_eq!(
            verify_while_reading(&empty).expect("verify empty"),
            (
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_owned(),
                0
            )
        );

        let abc = dir.path().join("abc.bin");
        std::fs::write(&abc, b"abc").expect("write");
        assert_eq!(
            verify_while_reading(&abc).expect("verify abc"),
            (
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_owned(),
                3
            )
        );

        // Larger than VERIFY_CHUNK_BYTES, so the streaming loop runs three
        // times: a chunking bug that dropped or re-hashed a block would change
        // this digest.
        let big = dir.path().join("big.bin");
        let content: Vec<u8> = (0..300_000usize).map(|i| (i % 251) as u8).collect();
        std::fs::write(&big, &content).expect("write");
        assert!(content.len() > VERIFY_CHUNK_BYTES * 2);
        assert_eq!(
            verify_while_reading(&big).expect("verify big"),
            (
                "3c65ea93424a9c362fec0e3a69ea36031e8a358441479dd665cc6110eabe7b08".to_owned(),
                300_000
            )
        );
    }

    #[test]
    fn a_file_mutated_mid_read_is_reported_as_changed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("torn.bin");
        std::fs::write(&path, vec![b'a'; 4096]).expect("write");

        let err = verify_while_reading_hooked(&path, || {
            let mut appended = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .expect("reopen for append");
            appended.write_all(&[b'b'; 4096]).expect("append");
            appended.flush().expect("flush");
        })
        .expect_err("a torn read must not produce a digest");

        assert!(
            matches!(err, SyncError::Integrity { .. }),
            "expected Integrity, got {err:?}"
        );
        assert_eq!(err.code(), "integrity");
        // Retried from zero, never resumed: the staged prefix is poisoned.
        assert_eq!(
            err.retriability(),
            crate::error::Retriability::Transient,
            "a torn read is worth another attempt"
        );

        // The unmutated file still verifies, so the hook is not what failed.
        assert!(verify_while_reading(&path).is_ok());
    }

    #[test]
    fn verify_refuses_a_non_regular_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        // A directory stands in for the whole class; the hazard the guard
        // actually exists for is a FIFO, whose `open` blocks forever.
        let err = verify_while_reading(dir.path()).expect_err("a directory is not verifiable");
        assert!(matches!(err, SyncError::Io { .. }), "got {err:?}");
    }

    #[test]
    fn verify_reports_a_missing_file_as_io_rather_than_panicking() {
        let dir = tempfile::tempdir().expect("tempdir");
        let err = verify_while_reading(&dir.path().join("nope.bin"))
            .expect_err("an absent file cannot be verified");
        assert!(matches!(err, SyncError::Io { .. }), "got {err:?}");
    }

    #[test]
    fn proc_locks_lines_veto_only_a_write_lock_on_the_same_inode() {
        // Real `/proc/locks` shapes. Major and minor are hex, the inode is
        // decimal, and `08:02` is dev 0x0802 = 2050.
        let dev = makedev(0x08, 0x02);
        assert_eq!(dev, 2050);

        assert!(lock_line_vetoes(
            "1: POSIX  ADVISORY  WRITE 3512 08:02:1234567 0 EOF",
            dev,
            1_234_567
        ));
        assert!(lock_line_vetoes(
            "2: FLOCK  ADVISORY  WRITE 3502 08:02:1234567 0 EOF",
            dev,
            1_234_567
        ));
        // A blocked request carries a `->` marker that shifts every field.
        assert!(lock_line_vetoes(
            "3: -> POSIX  ADVISORY  WRITE 9001 08:02:1234567 0 EOF",
            dev,
            1_234_567
        ));
        // OFD locks report pid -1, which must not break the parse.
        assert!(lock_line_vetoes(
            "4: OFDLCK ADVISORY  WRITE -1 08:02:1234567 0 EOF",
            dev,
            1_234_567
        ));

        // A read lock is not a writer.
        assert!(!lock_line_vetoes(
            "5: POSIX  ADVISORY  READ  3512 08:02:1234567 0 EOF",
            dev,
            1_234_567
        ));
        // Same inode number on a different device is a different file.
        assert!(!lock_line_vetoes(
            "6: POSIX  ADVISORY  WRITE 3512 fd:01:1234567 0 EOF",
            dev,
            1_234_567
        ));
        // Different inode on our device.
        assert!(!lock_line_vetoes(
            "7: POSIX  ADVISORY  WRITE 3512 08:02:7654321 0 EOF",
            dev,
            1_234_567
        ));
        // A lock with no backing inode, and assorted junk.
        assert!(!lock_line_vetoes(
            "8: POSIX  ADVISORY  WRITE 42 <none>:0",
            dev,
            0
        ));
        assert!(!lock_line_vetoes("", dev, 1));
        assert!(!lock_line_vetoes("garbage", dev, 1));
    }

    #[test]
    fn makedev_uses_the_full_linux_encoding_not_the_eight_bit_shorthand() {
        assert_eq!(makedev(0, 0), 0);
        assert_eq!(makedev(8, 2), (8 << 8) | 2);
        // The bits above the classic 12/8 split live at 32 and 12 — a naive
        // `(major << 8) | minor` silently collides for large device numbers.
        assert_eq!(makedev(0x1000, 0), 0x1000u64 << 32);
        assert_eq!(makedev(0, 0x100), 0x100u64 << 12);
        assert_ne!(makedev(0x1000, 0), makedev(0, 0));
    }

    #[test]
    fn is_dataless_is_false_for_an_ordinary_file_and_for_an_absent_one() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("plain.txt");
        std::fs::write(&path, b"local bytes").expect("write");
        // The guard must not fire on ordinary content, or every macOS profile
        // would silently sync nothing.
        assert!(!is_dataless(&path).expect("probe succeeds"));
        assert!(!is_dataless(&dir.path().join("absent")).expect("absent is not an error"));
    }

    #[test]
    fn a_gate_built_from_a_profile_inherits_its_window_and_excludes() {
        let mut profile =
            SyncProfile::new("01J", "docs", "/tmp/docs", "https://example.test/r.git");
        profile.excludes = vec!["*.scratch".to_owned()];
        profile.removable = true;
        let mut g = StabilityGate::for_profile(&profile).expect("profile compiles");
        // `removable` widens the default window; taking `settle_ms` raw would
        // silently drop that.
        assert_eq!(g.settle_ms(), profile.effective_settle_ms());
        assert_eq!(
            g.is_stable(Path::new("/tmp/docs/notes.scratch"), 0),
            StabilityVerdict::Excluded
        );
    }

    #[test]
    fn note_close_write_shortens_the_window_through_the_is_stable_facade() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("doc.odt");
        std::fs::write(&path, b"content").expect("write");

        // A real file's mtime is epoch-now, which is far ahead of the injected
        // clock, so the future-mtime escape hatch retires the mtime arm and
        // the quiescence run is the only thing gating these calls. That is
        // exactly the arm the close-write fast path shortens.
        let mut fast = StabilityGate::new(
            dir.path(),
            ExcludeSet::new(&[]).expect("corpus compiles"),
            SETTLE,
        );
        fast.note_close_write(&path);
        assert!(matches!(
            fast.is_stable(&path, 0),
            StabilityVerdict::Settling { .. }
        ));
        assert_eq!(fast.is_stable(&path, 1_000), StabilityVerdict::Stable);

        // Control: the identical timeline without the event waits the full
        // profile window.
        let mut slow = StabilityGate::new(
            dir.path(),
            ExcludeSet::new(&[]).expect("corpus compiles"),
            SETTLE,
        );
        assert!(matches!(
            slow.is_stable(&path, 0),
            StabilityVerdict::Settling { .. }
        ));
        assert!(matches!(
            slow.is_stable(&path, 1_000),
            StabilityVerdict::Settling { .. }
        ));
        assert_eq!(slow.is_stable(&path, 5_000), StabilityVerdict::Stable);

        // The flag is one-shot: `fast` consumed it, so the next episode on the
        // same gate gets the ordinary window again.
        assert!(matches!(
            fast.is_stable(&path, 10_000),
            StabilityVerdict::Settling { .. }
        ));
        assert!(matches!(
            fast.is_stable(&path, 11_000),
            StabilityVerdict::Settling { .. }
        ));
    }

    #[test]
    fn forget_all_clears_every_pending_episode() {
        let mut g = gate();
        g.observe(&p("a"), sample(1, 0, 1), 0, false);
        g.observe(&p("b"), sample(2, 0, 2), 0, false);
        assert_eq!(g.tracked(), 2);
        g.forget_all();
        assert_eq!(g.tracked(), 0);
    }

    #[test]
    fn the_open_writer_veto_does_not_fire_on_an_unlocked_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("unlocked.bin");
        std::fs::write(&path, b"no lock here").expect("write");
        let s = FileSample::of(&path).expect("stat").expect("file exists");
        // Nothing holds an advisory lock on a file we just created; a veto here
        // would mean the parser is matching the wrong inode or device.
        assert!(!open_writer_veto(&path, &s));
    }
}
