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

    /// The homeserver does not offer OAuth 2.0 / MSC3861 delegated auth, so an
    /// OIDC (single sign-on) login cannot be performed. Detected before any
    /// persistent state is created; non-retriable.
    #[error("homeserver does not support OIDC (OAuth 2.0 / MSC3861) login")]
    OAuthUnsupported,

    /// The OIDC browser round-trip did not complete before the timeout (the
    /// browser was abandoned / the callback never arrived). Retriable.
    #[error("single sign-on timed out; the sign-in was not completed in time")]
    OAuthTimedOut,

    /// The user cancelled the in-progress OIDC flow (explicit Cancel). Not a
    /// scary error — the UI returns quietly to the form. Retriable.
    #[error("single sign-on was cancelled")]
    OAuthCancelled,

    /// The OIDC flow failed. The wrapped string is a non-secret description of
    /// the failure (e.g. a server `error=` callback param or an exchange
    /// failure) — it never contains the authorization `code`, `state`, tokens,
    /// or session material. Retriable.
    #[error("single sign-on failed: {0}")]
    OAuthFailed(String),
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

/// Errors originating in the outgoing-send dispatch gate (FR-41, AD-13, AD-21).
///
/// A secret-free taxonomy: no message ever contains a token, txn id, message
/// plaintext, or event id. All variants map to `IpcErrorCode::SendFailed`
/// (retriable) in the shell's single funnel. These are *enqueue-time* failures
/// — asynchronous delivery failures surface as the `Failed` send-state on the
/// timeline item, not as one of these.
#[derive(Debug, Error)]
pub enum SendError {
    /// The target room was not found on the live `Client` (unknown or
    /// unparsable room id).
    #[error("room not found")]
    RoomNotFound,

    /// No open timeline is registered for the room, so send/retry has no live
    /// `Timeline` to operate on (the room must be open/subscribed to send).
    #[error("no open timeline for this room")]
    NoOpenTimeline,

    /// The wedged local echo referenced by a retry was not found in the live
    /// timeline (it may have already reconciled or been removed).
    #[error("outgoing message not found")]
    EchoNotFound,

    /// The SDK failed to enqueue (or re-drive) the send. The wrapped string is a
    /// non-secret description of the failure — never message plaintext.
    #[error("could not send the message: {0}")]
    Dispatch(String),
}

/// Errors originating in the merged unified-inbox stream (AD-20, AD-21).
///
/// A secret-free taxonomy: no message ever contains a token, session material,
/// or message plaintext. Maps to `IpcErrorCode::SyncUnavailable` (retriable) in
/// the shell's single funnel — a failed merge is a sync problem the frontend can
/// re-subscribe to.
#[derive(Debug, Error)]
pub enum InboxError {
    /// A per-account room-list stream feeding the merge could not start. The
    /// wrapped string is a non-secret description of the failure.
    #[error("could not start the merged inbox: {0}")]
    StreamStart(String),
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

    /// The merged unified-inbox stream failed to start.
    #[error(transparent)]
    Inbox(#[from] InboxError),

    /// A per-room timeline subscription failed to open.
    #[error(transparent)]
    Timeline(#[from] TimelineError),

    /// An outgoing message could not be enqueued for send.
    #[error(transparent)]
    Send(#[from] SendError),

    /// A requested capability is not supported on this platform/build. Honest,
    /// non-panicking signal used by not-yet-wired [`crate::platform::Platform`]
    /// ports.
    #[error("unsupported: {0}")]
    Unsupported(String),

    /// An unexpected internal invariant violation.
    #[error("internal error: {0}")]
    Internal(String),
}
