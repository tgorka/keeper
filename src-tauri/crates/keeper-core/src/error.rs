//! Error root for the keeper hexagon (AD-21).
//!
//! Per-module `thiserror` enums roll up into the [`CoreError`] root. The Tauri
//! shell maps `CoreError` to the IPC `IpcError` envelope exactly once, in its
//! command layer — no module below the shell constructs an `IpcError` directly.

use thiserror::Error;

/// Errors originating in a [`crate::platform::Platform`] port implementation.
///
/// This is the first module-level enum rolling up into [`CoreError`]; later
/// stories add sibling enums (sync, send, store, …) that roll up the same way.
#[derive(Debug, Error)]
pub enum PlatformError {
    /// A platform capability is not available on this host / build.
    #[error("platform capability unsupported: {0}")]
    Unsupported(String),

    /// A required directory could not be resolved on this platform.
    #[error("could not resolve platform directory: {0}")]
    DirUnavailable(String),

    /// An OS keychain operation (store/retrieve/delete a secret) failed.
    ///
    /// The message is a non-secret description of the failure — it never
    /// contains the secret value that was being stored or retrieved.
    #[error("keychain operation failed: {0}")]
    Keychain(String),
}

/// Errors originating in the password login flow.
///
/// A stable, secret-free taxonomy: no message ever contains a password, token,
/// or `MatrixSession` material. Each variant maps to a distinct `IpcErrorCode`
/// in the shell's single `to_ipc_error` funnel.
#[derive(Debug, Error)]
pub enum AuthError {
    /// The homeserver could not be reached (DNS/connection/build/probe
    /// failure). The wrapped string is a non-secret transport description.
    #[error("could not reach homeserver: {0}")]
    ServerUnreachable(String),

    /// The homeserver rejected the supplied username/password.
    #[error("invalid username or password")]
    InvalidCredentials,

    /// The homeserver does not offer `m.login.password`. The wrapped string is
    /// a non-secret description of the unsupported login flow.
    #[error("password login is not supported by this homeserver: {0}")]
    UnsupportedLoginType(String),

    /// The homeserver does not support Simplified Sliding Sync (MSC4186), which
    /// keeper requires. Detected before any persistent state is created.
    #[error("homeserver does not support Simplified Sliding Sync")]
    SlidingSyncUnsupported,
}

/// Errors originating in account activation / room-list supervision (AD-19,
/// AD-21).
///
/// A secret-free taxonomy: no message ever contains a token, session material,
/// or message plaintext. All variants map to `IpcErrorCode::SyncUnavailable`
/// (retriable) in the shell's single funnel.
#[derive(Debug, Error)]
pub enum AccountError {
    /// No persisted `MatrixSession` was found in the Keychain for this account.
    #[error("no stored session for this account")]
    SessionMissing,

    /// Rebuilding the `Client` or restoring the session failed. The wrapped
    /// string is a non-secret description of the failure.
    #[error("could not restore the account session: {0}")]
    RestoreFailed(String),

    /// The `SyncService` (or its room list) failed to build or start. The
    /// wrapped string is a non-secret description of the failure.
    #[error("could not start syncing: {0}")]
    SyncStart(String),
}

/// Errors originating in per-room timeline subscription (AD-8, AD-19, AD-21).
///
/// A secret-free taxonomy: no message ever contains a token, session material,
/// or message plaintext. All variants map to `IpcErrorCode::TimelineUnavailable`
/// (retriable) in the shell's single funnel.
#[derive(Debug, Error)]
pub enum TimelineError {
    /// The requested room was not found on the live `Client` (unknown or
    /// unparsable room id).
    #[error("room not found")]
    RoomNotFound,

    /// The SDK `Timeline` failed to build for the room. The wrapped string is a
    /// non-secret description of the failure.
    #[error("could not open the room timeline: {0}")]
    Build(String),
}

/// The hexagon error root. Every fallible core operation surfaces one of these.
#[derive(Debug, Error)]
pub enum CoreError {
    /// A platform port failed or is unavailable.
    #[error(transparent)]
    Platform(#[from] PlatformError),

    /// A password login attempt failed with a named, actionable cause.
    #[error(transparent)]
    Auth(#[from] AuthError),

    /// Account activation or room-list supervision failed.
    #[error(transparent)]
    Account(#[from] AccountError),

    /// A per-room timeline subscription failed to open.
    #[error(transparent)]
    Timeline(#[from] TimelineError),

    /// A requested capability is not supported on this platform/build. Honest,
    /// non-panicking signal used by not-yet-wired [`crate::platform::Platform`]
    /// ports.
    #[error("unsupported: {0}")]
    Unsupported(String),

    /// An unexpected internal invariant violation.
    #[error("internal error: {0}")]
    Internal(String),
}
