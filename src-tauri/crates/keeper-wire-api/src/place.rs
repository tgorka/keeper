//! Where the socket is, and the checks around it (MA/AD-4, DR-2, m7).
//!
//! One place on every platform: `<data dir>/wire/wire.sock`, where the data
//! directory is the app's or the `--state` of a `keeper-cli serve`.
//! `$XDG_RUNTIME_DIR`, `/tmp` and `$TMPDIR` are never used: a server and a
//! client that agree on the data directory agree on the socket.

use std::fmt;
use std::io;
use std::os::unix::fs::{
    DirBuilderExt as _, FileTypeExt as _, MetadataExt as _, PermissionsExt as _,
};
use std::path::{Path, PathBuf};

/// `sun_path`'s size, its terminating NUL included.
pub const SUN_PATH_BYTES: usize = if cfg!(target_os = "macos") { 104 } else { 108 };

/// The socket's directory under the data directory.
pub const WIRE_DIR: &str = "wire";
/// The socket's file name.
pub const SOCKET_FILE: &str = "wire.sock";

/// Why the place refuses.
#[derive(Debug)]
pub enum PlaceError {
    /// The data directory is not the effective user's, or others may write it.
    DataDirUnsafe { path: PathBuf, reason: &'static str },
    /// The socket's path does not fit `sun_path` with its NUL.
    PathTooLong { path: PathBuf, bytes: usize },
    /// `wire/` is a symlink, not a directory, or another user's.
    DirUnsafe { path: PathBuf, reason: &'static str },
    /// Something that is not a socket is at the socket's path; it stays.
    PathOccupied { path: PathBuf },
    /// The file system refused.
    Io { path: PathBuf, error: io::Error },
}

impl PlaceError {
    /// The stable code.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::DataDirUnsafe { .. } => "data_dir_unsafe",
            Self::PathTooLong { .. } => "wire_path_too_long",
            Self::DirUnsafe { .. } => "wire_dir_unsafe",
            Self::PathOccupied { .. } => "wire_path_occupied",
            Self::Io { .. } => "wire_io",
        }
    }
}

impl fmt::Display for PlaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DataDirUnsafe { path, reason } => write!(
                f,
                "data_dir_unsafe: {} {reason}; keeper uses only a data directory of your own that nobody else can write",
                path.display()
            ),
            Self::PathTooLong { path, bytes } => write!(
                f,
                "wire_path_too_long: {} is {bytes} bytes, and a socket's path holds {} with its NUL; the wire stays off",
                path.display(),
                SUN_PATH_BYTES
            ),
            Self::DirUnsafe { path, reason } => {
                write!(f, "wire_dir_unsafe: {} {reason}; the wire stays off", path.display())
            }
            Self::PathOccupied { path } => write!(
                f,
                "wire_path_occupied: {} is not a socket; keeper leaves it in place and the wire stays off",
                path.display()
            ),
            Self::Io { path, error } => write!(f, "wire_io: {}: {error}", path.display()),
        }
    }
}

impl std::error::Error for PlaceError {}

/// The effective uid of this process.
#[must_use]
pub fn effective_uid() -> u32 {
    rustix::process::geteuid().as_raw()
}

fn io(path: &Path) -> impl FnOnce(io::Error) -> PlaceError + '_ {
    move |error| PlaceError::Io {
        path: path.to_path_buf(),
        error,
    }
}

/// `<data dir>/wire/wire.sock`.
#[must_use]
pub fn socket_path(data_dir: &Path) -> PathBuf {
    data_dir.join(WIRE_DIR).join(SOCKET_FILE)
}

/// The path's bytes and its NUL must fit `sun_path` (104 on macOS, 108 on
/// Linux). A path that does not is refused, never shortened or moved.
pub fn check_path_len(socket: &Path) -> Result<(), PlaceError> {
    let bytes = socket.as_os_str().len();
    if bytes + 1 > SUN_PATH_BYTES {
        return Err(PlaceError::PathTooLong {
            path: socket.to_path_buf(),
            bytes,
        });
    }
    Ok(())
}

/// The data directory must be owned by the effective uid with no group or
/// other write bit (m7). Checked before the lock is asked for, by the server
/// and by the client.
pub fn check_data_dir(data_dir: &Path) -> Result<(), PlaceError> {
    let meta = std::fs::metadata(data_dir).map_err(io(data_dir))?;
    let unsafe_dir = |reason| PlaceError::DataDirUnsafe {
        path: data_dir.to_path_buf(),
        reason,
    };
    if !meta.is_dir() {
        return Err(unsafe_dir("is not a directory"));
    }
    if meta.uid() != effective_uid() {
        return Err(unsafe_dir("belongs to another user"));
    }
    if meta.mode() & 0o022 != 0 {
        return Err(unsafe_dir("may be written by others"));
    }
    Ok(())
}

/// `lstat` the `wire/` directory: a directory of the effective uid with no
/// permission bit for group or others.
fn checked_dir(dir: &Path) -> Result<Option<std::fs::Metadata>, PlaceError> {
    let meta = match std::fs::symlink_metadata(dir) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io(dir)(error)),
    };
    let unsafe_dir = |reason| PlaceError::DirUnsafe {
        path: dir.to_path_buf(),
        reason,
    };
    if meta.file_type().is_symlink() {
        return Err(unsafe_dir("is a symlink"));
    }
    if !meta.is_dir() {
        return Err(unsafe_dir("is not a directory"));
    }
    if meta.uid() != effective_uid() {
        return Err(unsafe_dir("belongs to another user"));
    }
    Ok(Some(meta))
}

/// The server's side: create `wire/` with mode 0700, narrow one of our own
/// with a wider mode to 0700 and check again, refuse a symlink or another
/// owner's. Returns the socket's path, its length checked.
pub fn prepare_wire_dir(data_dir: &Path) -> Result<PathBuf, PlaceError> {
    let socket = socket_path(data_dir);
    check_path_len(&socket)?;
    let dir = data_dir.join(WIRE_DIR);
    match checked_dir(&dir)? {
        None => {
            std::fs::DirBuilder::new()
                .mode(0o700)
                .create(&dir)
                .map_err(io(&dir))?;
            // The umask can only narrow 0700; set it exactly all the same.
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
                .map_err(io(&dir))?;
        }
        Some(meta) if meta.mode() & 0o077 != 0 => {
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
                .map_err(io(&dir))?;
        }
        Some(_) => {}
    }
    match checked_dir(&dir)? {
        Some(meta) if meta.mode() & 0o077 == 0 => Ok(socket),
        Some(_) => Err(PlaceError::DirUnsafe {
            path: dir,
            reason: "could not be narrowed to 0700",
        }),
        None => Err(PlaceError::DirUnsafe {
            path: dir,
            reason: "disappeared",
        }),
    }
}

/// The client's side, before dialling: the data directory and `wire/` pass
/// the server's checks, and the path fits. Nothing is created or changed.
pub fn check_for_dial(data_dir: &Path) -> Result<PathBuf, PlaceError> {
    check_data_dir(data_dir)?;
    let socket = socket_path(data_dir);
    check_path_len(&socket)?;
    let dir = data_dir.join(WIRE_DIR);
    match checked_dir(&dir)? {
        Some(meta) if meta.mode() & 0o077 == 0 => Ok(socket),
        Some(_) => Err(PlaceError::DirUnsafe {
            path: dir,
            reason: "may be entered by others",
        }),
        None => Err(PlaceError::Io {
            path: dir,
            error: io::Error::from(io::ErrorKind::NotFound),
        }),
    }
}

/// Under the lock: a socket at the path is stale by construction and is
/// unlinked; anything else there is refused and stays.
pub fn clear_stale(_lock: &crate::DataLock, socket: &Path) -> Result<(), PlaceError> {
    match std::fs::symlink_metadata(socket) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io(socket)(error)),
        Ok(meta) if meta.file_type().is_socket() => {
            std::fs::remove_file(socket).map_err(io(socket))
        }
        Ok(_) => Err(PlaceError::PathOccupied {
            path: socket.to_path_buf(),
        }),
    }
}
