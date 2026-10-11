//! One plan at a time per zone (AD-368, FR-778).
//!
//! Two layers, because two kinds of writer share a zone: a process-wide
//! mutex keyed by the zone's canonical root, so two threads of this process
//! never interleave, and an exclusive file lock on
//! `<zone>/.keeper/sessions.lock`, so a second process on the machine — the
//! app beside `keeper-agentd`, or two agentd principals over one checkout —
//! waits too. `.keeper/` is never synced, so the lock file is this machine's
//! alone.
//!
//! A filesystem that cannot lock a file at all (some FUSE and network mounts
//! answer `flock` with `ENOTSUP` or `ENOLCK`) keeps the in-process layer and
//! says so once: on such a mount a second process is not held off.

use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard, PoisonError};

use fs4::fs_std::FileExt;

/// The lock file, zone-relative.
pub const LOCK_REL: &str = ".keeper/sessions.lock";

/// Whether this process holds a zone, and the condition its waiters sleep on.
#[derive(Default)]
struct Gate {
    held: Mutex<bool>,
    freed: Condvar,
    /// The zone's filesystem refused a file lock, and that was logged.
    unlockable_said: AtomicBool,
}

fn gates() -> MutexGuard<'static, HashMap<PathBuf, Arc<Gate>>> {
    static GATES: LazyLock<Mutex<HashMap<PathBuf, Arc<Gate>>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    GATES.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A zone held for one plan. Released on drop: the file lock with its handle,
/// then the in-process gate.
pub struct ZoneLock {
    /// `None` on a filesystem that cannot lock a file.
    file: Option<File>,
    gate: Arc<Gate>,
    zone: PathBuf,
}

impl ZoneLock {
    /// Wait for the zone, then hold it.
    ///
    /// **Not reentrant.** A thread that holds a zone and asks for it again
    /// waits for itself forever, so a caller holding a `ZoneLock` runs its
    /// plan through [`super::exec::run_held`], never [`super::exec::run`].
    pub fn acquire(zone: &Path) -> io::Result<ZoneLock> {
        Self::acquire_with(zone, FileExt::lock_exclusive)
    }

    /// [`ZoneLock::acquire`] with the file lock as an argument, so the gate's
    /// own guarantee and the fallback can be proved without a filesystem that
    /// refuses `flock`.
    fn acquire_with(zone: &Path, lock: fn(&File) -> io::Result<()>) -> io::Result<ZoneLock> {
        let key = zone.canonicalize()?;
        let gate = Arc::clone(gates().entry(key.clone()).or_default());
        {
            let mut held = gate.held.lock().unwrap_or_else(PoisonError::into_inner);
            while *held {
                held = gate
                    .freed
                    .wait(held)
                    .unwrap_or_else(PoisonError::into_inner);
            }
            *held = true;
        }
        let file = match open_and_lock(&key, &gate, lock) {
            Ok(file) => file,
            Err(error) => {
                release(&gate);
                return Err(error);
            }
        };
        Ok(ZoneLock {
            file,
            gate,
            zone: key,
        })
    }

    /// The zone this holds, canonical.
    pub fn zone(&self) -> &Path {
        &self.zone
    }
}

fn open_and_lock(
    zone: &Path,
    gate: &Gate,
    lock: fn(&File) -> io::Result<()>,
) -> io::Result<Option<File>> {
    let path = zone.join(LOCK_REL);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)?;
    match lock(&file) {
        Ok(()) => Ok(Some(file)),
        Err(error) if cannot_lock_here(&error) => {
            if !gate.unlockable_said.swap(true, Ordering::Relaxed) {
                tracing::warn!(
                    zone = %zone.display(),
                    %error,
                    "sessions: this zone's filesystem cannot lock a file, so only this process's plans wait for each other here"
                );
            }
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

/// Whether a failed file lock is the filesystem having no locks at all, rather
/// than a lock that could not be taken. `ENOTSUP`/`EOPNOTSUPP` and `ENOSYS`
/// are std's `Unsupported`; `ENOLCK` has no kind of its own, so its number is
/// named per platform.
fn cannot_lock_here(error: &io::Error) -> bool {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const NO_LOCKS: &[i32] = &[37];
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    const NO_LOCKS: &[i32] = &[77, 45];
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    )))]
    const NO_LOCKS: &[i32] = &[];
    error.kind() == io::ErrorKind::Unsupported
        || error
            .raw_os_error()
            .is_some_and(|code| NO_LOCKS.contains(&code))
}

fn release(gate: &Gate) {
    *gate.held.lock().unwrap_or_else(PoisonError::into_inner) = false;
    gate.freed.notify_one();
}

impl Drop for ZoneLock {
    fn drop(&mut self) {
        // Unlocked before the gate opens, so the next thread of this process
        // never waits on a file lock this one still holds.
        if let Some(file) = &self.file {
            let _ = FileExt::unlock(file);
        }
        release(&self.gate);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    fn no_file_lock(_: &File) -> io::Result<()> {
        Ok(())
    }

    /// The in-process gate on its own: with the file lock taken out of the
    /// picture, a second thread asking for a held zone waits until the first
    /// lets go — the layer that keeps two threads of one process apart even
    /// where the file lock would not.
    #[test]
    fn the_gate_alone_holds_a_second_thread_off_until_the_first_lets_go() {
        let zone = tempfile::tempdir().expect("zone");
        let first = ZoneLock::acquire_with(zone.path(), no_file_lock).expect("first");
        let (took, taken) = mpsc::channel();
        let root = zone.path().to_path_buf();
        let second = std::thread::spawn(move || {
            let held = ZoneLock::acquire_with(&root, no_file_lock).expect("second");
            took.send(()).expect("say so");
            drop(held);
        });
        assert!(
            taken.recv_timeout(Duration::from_millis(300)).is_err(),
            "the second thread took a zone the first still held"
        );
        drop(first);
        taken
            .recv_timeout(Duration::from_secs(10))
            .expect("the second thread takes the zone once it is free");
        second.join().expect("second thread");
    }

    /// A filesystem with no file locks keeps the gate and holds the zone; any
    /// other lock failure is the caller's error, and the gate opens again.
    #[test]
    fn a_filesystem_without_locks_falls_back_to_the_gate_and_any_other_failure_refuses() {
        fn no_locks_here(_: &File) -> io::Result<()> {
            Err(io::Error::from(io::ErrorKind::Unsupported))
        }
        fn denied(_: &File) -> io::Result<()> {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        }
        let zone = tempfile::tempdir().expect("zone");
        let held = ZoneLock::acquire_with(zone.path(), no_locks_here).expect("the gate alone");
        assert!(held.file.is_none());
        drop(held);
        assert!(ZoneLock::acquire_with(zone.path(), denied).is_err());
        ZoneLock::acquire_with(zone.path(), no_file_lock).expect("the gate opened after a refusal");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn enolck_and_enotsup_are_a_filesystem_without_locks() {
        #[cfg(target_os = "linux")]
        let (enolck, enotsup, eacces) = (37, 95, 13);
        #[cfg(target_os = "macos")]
        let (enolck, enotsup, eacces) = (77, 45, 13);
        assert!(cannot_lock_here(&io::Error::from_raw_os_error(enolck)));
        assert!(cannot_lock_here(&io::Error::from_raw_os_error(enotsup)));
        assert!(!cannot_lock_here(&io::Error::from_raw_os_error(eacces)));
    }
}
