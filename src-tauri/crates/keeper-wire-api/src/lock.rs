//! The data directory's lock (MA/AD-4 *the lock, step by step*; A-2, DR-3,
//! SR-4): `<data dir>/keeper.lock`, opened `O_NOFOLLOW` and close-on-exec,
//! mode 0600, taken exclusively without waiting. The kernel releases it when
//! the process dies, so it is never stale. Three cases besides taken: held by
//! another process, cannot be asked, and a data directory that is unsafe
//! (refused before the lock is asked for).

use std::fmt;
use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Read as _, Seek as _, Write as _};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};

use crate::place::{check_data_dir, PlaceError};

/// The lock file's name in the data directory.
pub const LOCK_FILE: &str = "keeper.lock";

/// Why the lock was not taken.
#[derive(Debug)]
pub enum LockError {
    /// The data directory failed its check; the lock was not asked for.
    Unsafe(PlaceError),
    /// Another process holds it. `holder` is what that process wrote for a
    /// person to read; it decides nothing.
    Held { path: PathBuf, holder: String },
    /// The lock cannot be asked for here (`ENOLCK`, a file system without
    /// locks): not "held". The app opens its data and the wire stays off.
    Unsupported { path: PathBuf, error: io::Error },
    /// The lock file could not be opened.
    Io { path: PathBuf, error: io::Error },
}

impl LockError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unsafe(error) => error.code(),
            Self::Held { .. } => "data_dir_locked",
            Self::Unsupported { .. } => "lock_unsupported",
            Self::Io { .. } => "lock_io",
        }
    }
}

impl fmt::Display for LockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsafe(error) => error.fmt(f),
            Self::Held { path, holder } => {
                let holder = if holder.is_empty() {
                    "another keeper process"
                } else {
                    holder.as_str()
                };
                write!(
                    f,
                    "data_dir_locked: {holder} is using {}; keeper opened nothing there",
                    path.parent().unwrap_or(path).display()
                )
            }
            Self::Unsupported { path, error } => write!(
                f,
                "lock_unsupported: {} cannot be locked here ({error}); keeper's wire stays off",
                path.display()
            ),
            Self::Io { path, error } => write!(f, "lock_io: {}: {error}", path.display()),
        }
    }
}

impl std::error::Error for LockError {}

/// The lock, held for as long as this value lives.
#[derive(Debug)]
pub struct DataLock {
    file: File,
    data_dir: PathBuf,
}

/// The one function that asks the kernel for the lock. A debug build can
/// inject "not supported" here (`KEEPER_TEST_LOCK_UNSUPPORTED=1`) to test the
/// third case; a release build has no such seam.
fn ask(file: &File) -> Result<(), TryLockError> {
    #[cfg(debug_assertions)]
    if std::env::var_os("KEEPER_TEST_LOCK_UNSUPPORTED").is_some() {
        return Err(TryLockError::Error(io::Error::from_raw_os_error(
            rustix::io::Errno::NOLCK.raw_os_error(),
        )));
    }
    file.try_lock()
}

impl DataLock {
    /// Check the data directory, then take its lock without waiting. `holder`
    /// names this process for a person who finds the directory busy
    /// (e.g. `keeper-cli serve, pid 4242`).
    pub fn acquire(data_dir: &Path, holder: &str) -> Result<Self, LockError> {
        check_data_dir(data_dir).map_err(LockError::Unsafe)?;
        let path = data_dir.join(LOCK_FILE);
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
            .open(&path)
            .map_err(|error| LockError::Io {
                path: path.clone(),
                error,
            })?;
        match ask(&file) {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                let mut holder = String::new();
                // What the holder wrote is for a person; a read failure leaves it empty.
                let _ = file.read_to_string(&mut holder);
                return Err(LockError::Held {
                    path,
                    holder: holder.trim().to_owned(),
                });
            }
            Err(TryLockError::Error(error)) => return Err(LockError::Unsupported { path, error }),
        }
        // For a person to read; nothing relies on it.
        let _ = file
            .set_len(0)
            .and_then(|()| file.rewind())
            .and_then(|()| writeln!(file, "{holder}"));
        Ok(Self {
            file,
            data_dir: data_dir.to_path_buf(),
        })
    }

    /// The directory this lock is for.
    #[must_use]
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// The open lock file (kept open until this value drops).
    #[must_use]
    pub fn file(&self) -> &File {
        &self.file
    }

    /// [`DataLock::acquire`], asked again every [`RETRY_EVERY`] while another
    /// process holds the lock, for up to `wait` (the app: ten seconds, then it
    /// opens nothing and names the holder). Every other answer is returned at
    /// once.
    pub fn acquire_waiting(
        data_dir: &Path,
        holder: &str,
        wait: std::time::Duration,
    ) -> Result<Self, LockError> {
        let until = std::time::Instant::now() + wait;
        loop {
            match Self::acquire(data_dir, holder) {
                Err(LockError::Held { .. }) if std::time::Instant::now() < until => {
                    std::thread::sleep(RETRY_EVERY);
                }
                answer => return answer,
            }
        }
    }
}

/// What a process that opens the data directory may do, answered before it
/// opens anything there (the app's start; story 123.3, A-2, SR-4).
#[derive(Debug)]
pub enum Admission {
    /// The lock is this process's: open the data; the socket may serve.
    Held(DataLock),
    /// The lock cannot be asked for here: open the data, keep the socket
    /// off, and log the sentence once.
    Unsupported(String),
    /// Another process holds the lock, or the directory is unsafe: open no
    /// data, and show the sentence (it names the holder).
    Refused(String),
}

/// Ask for the data directory's lock as the app does at its start: create a
/// missing directory (0700), ask again for up to `wait` while another process
/// holds the lock, then answer. A lock file that cannot be opened counts as
/// a lock that cannot be asked for, so a broken lock file never keeps a
/// person out of their data.
#[must_use]
pub fn admit(data_dir: &Path, holder: &str, wait: std::time::Duration) -> Admission {
    use std::os::unix::fs::DirBuilderExt as _;

    if !data_dir.exists() {
        let _ = std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(data_dir);
    }
    match DataLock::acquire_waiting(data_dir, holder, wait) {
        Ok(lock) => Admission::Held(lock),
        Err(error @ (LockError::Unsupported { .. } | LockError::Io { .. })) => {
            Admission::Unsupported(error.to_string())
        }
        Err(error @ (LockError::Held { .. } | LockError::Unsafe(_))) => {
            Admission::Refused(error.to_string())
        }
    }
}

/// How often [`DataLock::acquire_waiting`] asks again.
pub const RETRY_EVERY: std::time::Duration = std::time::Duration::from_millis(200);
