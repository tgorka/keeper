//! The folder machinery's errors (story 119.1).
//!
//! The four `keeper_sync::SyncError` variants the moved modules raised, with
//! the same messages and the same codes. `keeper-sync` maps each onto its own
//! identical variant (`From<FolderError> for SyncError`), so every surface that
//! renders a sync error renders the same sentence as before the move.
//!
//! Invariant, inherited from `keeper_sync::error`: **a message never carries a
//! credential, a token, or file content.** Ids, hosts, paths, byte counts and
//! status codes are permitted.

use std::path::PathBuf;

/// Whether a failed unit of work is worth attempting again unchanged.
///
/// This is a *policy* answer, not a transport detail: the scheduler consults it
/// to decide between re-queueing with backoff (Story 26.6) and parking the unit
/// for a human. Keeping it on the error means the classification lives next to
/// the thing being classified instead of in a match at every call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retriability {
    /// Try again after backoff — the world may change (network, quota, lock).
    Transient,
    /// Never retry unchanged; something must be reconfigured or fixed first.
    Permanent,
    /// Not an error the scheduler should retry: the work is *deliberately*
    /// deferred until an external condition holds (a volume is re-attached).
    Deferred,
}

/// Errors raised by the folder machinery.
#[derive(Debug, thiserror::Error)]
pub enum FolderError {
    /// A path cannot be represented on the remote or on a peer's filesystem
    /// (reserved name, illegal character, too long). The message names the
    /// path, because the surfaces that show this one only render `to_string()`.
    #[error("{} cannot be synchronized: {reason}", .path.display())]
    InvalidPathForRemote { path: PathBuf, reason: String },

    /// Content did not match its expected digest or length. Always hard: we
    /// discard the staged bytes rather than resume from a poisoned prefix.
    #[error("integrity check failed for {subject}: expected {expected}, got {actual}")]
    Integrity {
        subject: String,
        expected: String,
        actual: String,
    },

    /// Filesystem failure, with the path it happened to. `std::io::Error` alone
    /// loses the path, which makes these reports useless in the field.
    #[error("{operation} failed for {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Configuration the machinery cannot act on (an invalid pattern, a
    /// threshold out of range, an unreadable marker).
    #[error("invalid sync configuration: {0}")]
    Config(String),
}

pub type Result<T, E = FolderError> = std::result::Result<T, E>;

impl FolderError {
    pub fn io(operation: &'static str, path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            operation,
            path: path.into(),
            source,
        }
    }

    /// The same answer `SyncError::retriability` gives the same variant.
    pub fn retriability(&self) -> Retriability {
        match self {
            // Retried from zero: the staged prefix is always discarded first.
            Self::Integrity { .. } => Retriability::Transient,
            Self::InvalidPathForRemote { .. } | Self::Config(_) => Retriability::Permanent,
            // Ambiguous by nature; a bounded retry is cheaper than parking a
            // profile on a transient EINTR or a momentarily locked file.
            Self::Io { .. } => Retriability::Transient,
        }
    }

    /// The same answer `SyncError::needs_user_action` gives the same variant.
    pub fn needs_user_action(&self) -> bool {
        matches!(self, Self::InvalidPathForRemote { .. })
    }

    /// The same code `SyncError::code` gives the same variant.
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidPathForRemote { .. } => "invalidPath",
            Self::Integrity { .. } => "integrity",
            Self::Io { .. } => "io",
            Self::Config(_) => "config",
        }
    }
}
