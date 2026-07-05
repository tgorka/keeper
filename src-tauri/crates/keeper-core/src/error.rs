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

    /// The Beeper unofficial email-code login flow failed (Story 2.3, AD-17).
    ///
    /// A single collapse point for *every* Beeper failure: a non-2xx from any
    /// `api.beeper.com` step, a network/transport error, a request timeout, a
    /// missing/renamed JSON field (the private API changed shape), an abandoned
    /// flow whose request id is gone, or a JWT / `org.matrix.login.jwt`
    /// rejection. The wrapped string is a non-secret description — it never
    /// contains the emailed code, the JWT, or the bearer token. Retriable — the
    /// UI returns to the email step to start a fresh flow.
    #[error("Beeper login is unavailable: {0}")]
    BeeperUnavailable(String),
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

    /// The reply/edit target (referenced by its opaque render key) was not found
    /// in the live timeline, or a reply target carried no resolvable event id
    /// (Story 3.4). Non-retriable — re-issuing the same request won't help.
    #[error("referenced message not found")]
    TargetNotFound,

    /// An edit was requested on a message that is not editable — not the user's
    /// own message, or not a text message (Story 3.4). Non-retriable.
    #[error("message can't be edited")]
    NotEditable,

    /// The SDK failed to enqueue (or re-drive) the send. The wrapped string is a
    /// non-secret description of the failure — never message plaintext.
    #[error("could not send the message: {0}")]
    Dispatch(String),

    /// The SDK failed to enqueue an attachment upload, or to abort an in-flight
    /// one (Story 3.7, FR-13). The wrapped string is a non-secret description of
    /// the failure — never the file bytes, path, or `mxc`. Retriable (an
    /// enqueue-time failure the frontend can attempt again); asynchronous upload
    /// delivery failures instead surface as the `Failed` send-state on the echo.
    #[error("could not upload the attachment: {0}")]
    Upload(String),
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

/// Errors originating in interactive device self-verification (Story 3.2, FR-14,
/// AD-1, AD-21).
///
/// A secret-free taxonomy: no message ever contains a SAS key, decimal, QR
/// crypto, session material, or plaintext — only non-secret descriptions. All
/// variants map to `IpcErrorCode::VerificationFailed` (retriable) in the shell's
/// single funnel.
#[derive(Debug, Error)]
pub enum VerificationError {
    /// Verification could not be started or driven because the account's crypto
    /// identity is not ready (no cross-signing identity yet, no signed-in user).
    /// The wrapped string is a non-secret description.
    #[error("verification is not available yet: {0}")]
    Unavailable(String),

    /// No live verification flow was found for the given flow id (it already
    /// reached a terminal state, or the id is unknown).
    #[error("verification flow not found")]
    FlowNotFound,

    /// An SDK verification action (accept / start_sas / confirm / mismatch /
    /// cancel / request) failed. The wrapped string is a non-secret description.
    #[error("verification action failed: {0}")]
    Action(String),
}

/// Errors originating in server-side key backup enable / restore (Story 3.3,
/// FR-14, AD-1, AD-21).
///
/// A secret-free taxonomy: no message ever contains the recovery key, a
/// secret-storage key, backup key material, or plaintext — only non-secret
/// descriptions. The base58 recovery key returned by `enable` is a value on the
/// success path, never inside an error. Each variant maps to a distinct
/// `IpcErrorCode` in the shell's single funnel so invalid keys are *named*, never
/// generic.
#[derive(Debug, Error)]
pub enum BackupError {
    /// Backup enable/restore could not proceed because the account's recovery
    /// subsystem is not available yet (crypto not synced). The wrapped string is
    /// a non-secret description.
    #[error("key backup is not available yet: {0}")]
    Unavailable(String),

    /// Enabling backup raced an existing server-side backup — a backup already
    /// exists on the homeserver, so the user should restore instead of enabling.
    #[error("a key backup already exists on the server")]
    AlreadyExistsOnServer,

    /// The pasted recovery key could not be decoded (wrong length / not a valid
    /// base58 recovery key). Distinct from a well-formed-but-wrong key.
    #[error("the recovery key is malformed")]
    MalformedRecoveryKey,

    /// A well-formed recovery key failed the MAC check — it does not match this
    /// account's backup. Distinct from a malformed key.
    #[error("the recovery key did not match this account")]
    IncorrectRecoveryKey,

    /// Restore failed for another reason (network / other SDK error). The wrapped
    /// string is a non-secret description of the failure.
    #[error("could not restore from key backup: {0}")]
    RestoreFailed(String),

    /// An SDK backup action (enable) failed for a reason other than an existing
    /// server backup. The wrapped string is a non-secret description.
    #[error("key backup action failed: {0}")]
    Action(String),
}

/// Errors originating in media resolution / fetch for the `keeper-media://`
/// protocol (Story 3.6, FR-13, AD-4, AD-21).
///
/// A secret-free taxonomy: no message ever contains an `mxc` uri, `EncryptedFile`
/// key material, a decryption key, or plaintext — only non-secret descriptions.
/// The protocol handler maps these to HTTP status codes directly (404 for
/// `NotFound`, 404 for a `Fetch` failure it retries), so they do **not** flow
/// through the `CoreError → IpcError` funnel — no media bytes or errors cross the
/// IPC command surface (bytes travel only over the custom protocol).
#[derive(Debug, Error)]
pub enum MediaError {
    /// The `keeper-media://` handle could not be resolved to a live media source:
    /// the account is not live, the room has no open timeline, the `item_key` is
    /// not in the timeline, or the resolved item is not a media message. The
    /// protocol handler serves a 404; the frontend retries once it is resolvable.
    #[error("media not found")]
    NotFound,

    /// The SDK failed to download or decrypt the media content. The wrapped string
    /// is a non-secret description of the failure — never key material or
    /// plaintext. The protocol handler serves a 404; the frontend can retry.
    #[error("could not fetch media: {0}")]
    Fetch(String),
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

    /// An interactive device self-verification action failed.
    #[error(transparent)]
    Verification(#[from] VerificationError),

    /// A server-side key-backup enable / restore action failed.
    #[error(transparent)]
    Backup(#[from] BackupError),

    /// A `keeper-media://` media resolution / fetch failed (Story 3.6). Surfaced
    /// to the protocol handler for an HTTP status; never crosses the IPC command
    /// funnel.
    #[error(transparent)]
    Media(#[from] MediaError),

    /// A requested capability is not supported on this platform/build. Honest,
    /// non-panicking signal used by not-yet-wired [`crate::platform::Platform`]
    /// ports.
    #[error("unsupported: {0}")]
    Unsupported(String),

    /// An unexpected internal invariant violation.
    #[error("internal error: {0}")]
    Internal(String),
}
