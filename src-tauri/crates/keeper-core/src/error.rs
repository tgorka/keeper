//! Error root for the keeper hexagon (AD-21).
//!
//! Per-module `thiserror` enums roll up into the [`CoreError`] root. The Tauri
//! shell maps `CoreError` to the IPC `IpcError` envelope exactly once, in its
//! command layer — no module below the shell constructs an `IpcError` directly.

use thiserror::Error;

use crate::recording::SessionState;

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

    /// Excluding a store path from OS device backups failed (Story 14.7,
    /// FR-65) — on iOS, setting `NSURLIsExcludedFromBackupKey` on the file or
    /// directory URL. The message is a non-secret description of the failure
    /// (path + `NSError` text — never store contents). Callers treat this as
    /// best-effort hardening: logged and swallowed, never fatal to startup,
    /// login, or session-restore.
    #[error("backup exclusion failed: {0}")]
    BackupExclusion(String),
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

    /// An approval was requested for a draft whose body is empty or
    /// whitespace-only (Story 7.3). Guarded in [`send_approval`] so the frontend
    /// retains the draft rather than silently discarding unsent text — the
    /// airlock never destroys held text. Non-retriable as-is.
    #[error("cannot approve an empty draft")]
    EmptyBody,

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

/// Errors originating in the receipt/typing signals seam (Story 3.9, AD-14).
///
/// A secret-free taxonomy: no message ever contains a token, event id, session
/// material, or plaintext — only a non-secret description of a best-effort emit
/// failure. Read receipts and typing notices are best-effort: a `Dispatch`
/// failure is logged and swallowed by the caller (no UI error). Maps to
/// `IpcErrorCode::SignalDispatchFailed` (non-retriable, best-effort) in the
/// shell's single funnel.
#[derive(Debug, Error)]
pub enum SignalError {
    /// The SDK failed to dispatch a receipt or typing notice. The wrapped string
    /// is a non-secret description of the failure — never a token, event id, or
    /// plaintext. Best-effort: the caller logs and swallows it (no UI error).
    #[error("could not dispatch the signal: {0}")]
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

/// Errors originating in local archive ingestion (Story 5.1, epic 5, AD-21).
///
/// A secret-free taxonomy: no message ever contains message plaintext, media
/// bytes, or session material — only a non-secret description of a SQLite or
/// serialization failure. These surface at the [`crate::archive`] boundary only
/// during setup (`ArchiveWriter::spawn` opening `archive.db`); a *runtime* write
/// failure inside the writer task is logged with ids only and swallowed, never
/// propagated (the archive path must never block or abort sync/messaging).
#[derive(Debug, Error)]
pub enum ArchiveError {
    /// A SQLite operation on `archive.db` failed (open, PRAGMA, schema, or a
    /// read). The wrapped string is a non-secret description — never content.
    #[error("archive database error: {0}")]
    Sqlite(String),

    /// Serializing media metadata to JSON for the `media_json` column failed. The
    /// wrapped string is a non-secret description — never media bytes.
    #[error("archive serialization error: {0}")]
    Serialization(String),

    /// A filesystem operation during export failed (creating the scope subfolder,
    /// writing a JSON/Markdown/media file, or cleaning up partial output) (Story
    /// 5.5). The wrapped string is a non-secret description — never message content
    /// or media bytes. Surfaces to the export command, which streams it as the
    /// `Failed` terminal batch after cleaning up partial output.
    #[error("archive export IO error: {0}")]
    ExportIo(String),
}

/// Errors originating in the data-driven bridge catalog (Story 6.1, Epic 6,
/// AD-21).
///
/// A secret-free taxonomy: the bridge catalog is static, embedded JSON with no
/// session, token, or network I/O. The only failure mode is a compiled-in data
/// file that fails to parse or validate at first access — surfaced honestly
/// through `Result` (never a panic / `.unwrap()`). The wrapped string is a
/// non-secret description of the parse/validation failure. Maps to
/// `IpcErrorCode::Internal` in the shell's single funnel (a malformed embedded
/// asset is an internal invariant violation, not a user-actionable retry).
///
/// `Clone` so the [`OnceLock`](std::sync::OnceLock)-cached parse result can hand
/// each caller an owned copy of the error without re-parsing.
#[derive(Debug, Clone, Error)]
pub enum BridgeError {
    /// An embedded bridge data file (`risk-tiers.json`, `coupling-caveats.json`,
    /// or `known-bots.json`) failed to parse or validate. The wrapped string names
    /// the file and the failure — never secret material. Maps to
    /// `IpcErrorCode::Internal` (non-retriable — the JSON is compiled in).
    #[error("bridge catalog data error: {0}")]
    Data(String),

    /// Bridge discovery (Story 6.2) was asked to run for an account that is not
    /// live in the [`crate::account::AccountManager`]. Non-retriable — the account
    /// must be activated first. The wrapped string is the non-secret account id.
    #[error("no live account for bridge discovery: {0}")]
    AccountNotFound(String),

    /// Bridge discovery could not complete because a load-bearing Matrix
    /// transport step failed (e.g. the account has no resolvable user id / server
    /// name to probe against). Individual per-source failures degrade gracefully
    /// and never reach here; this is a *total* discovery failure. Retriable — the
    /// homeserver may be transiently unreachable. The wrapped string is a
    /// non-secret description — never a token or session material.
    #[error("bridge discovery failed: {0}")]
    Discovery(String),

    /// A native bridge login over the mautrix bridgev2 provisioning API (Story
    /// 6.3, FR-26, AD-16) failed: no candidate base URL authenticated the
    /// provisioning endpoint, an HTTP/transport error, a JSON (de)serialization
    /// failure, or a non-2xx response from a login step. The wrapped string is a
    /// non-secret description — the bridge's own error message rendered verbatim,
    /// never the account's Matrix access token or any session/cookie material.
    /// Retriable — the user can retry the login. Maps to
    /// `IpcErrorCode::SyncUnavailable`.
    #[error("bridge login failed: {0}")]
    Provisioning(String),

    /// A bridge login driven over the raw Bridge Bot chat (Story 6.4, FR-27,
    /// AD-16) failed: the bot MXID/room could not be resolved or created, a
    /// command send failed, the bot did not reply within the timeout, or the
    /// bot's reply could not be classified into a known login step. The wrapped
    /// string is a non-secret description — the bot's own reply rendered
    /// verbatim (length-capped, like the provisioning cap), never the account's
    /// Matrix access token or any session/cookie material. Retriable — the user
    /// can retry the login. Maps to `IpcErrorCode::SyncUnavailable`, mirroring
    /// [`BridgeError::Provisioning`].
    #[error("bridge login failed: {0}")]
    Bot(String),

    /// A `bbctl` self-hosted-bridge run (Story 6.7, FR-29) failed or was refused:
    /// the surface was invoked for a non-Beeper account, the requested network is
    /// not self-hostable, the `bbctl` sidecar could not be resolved (the guided
    /// install path), or the `bbctl register`/`run` process errored / exited
    /// non-zero. The wrapped string is a non-secret description — `bbctl`'s own
    /// output rendered verbatim (length-capped, like the provisioning/bot caps),
    /// never the account's Beeper token or any session/cookie material. Retriable —
    /// the user can retry the run. Maps to `IpcErrorCode::SyncUnavailable`,
    /// mirroring [`BridgeError::Provisioning`]/[`BridgeError::Bot`].
    #[error("bbctl run failed: {0}")]
    Bbctl(String),
}

/// Errors originating in the platform-free recording session machine and its
/// [`Recorder`](crate::recording::Recorder) port (Story 16.2, Epic 16, AD-33,
/// AD-21).
///
/// A secret-free taxonomy: no message ever contains a captured-media path, media
/// bytes, token, or session material — only a non-secret description of a state or
/// sidecar failure. Recording does not cross the IPC command surface in this story
/// (a dedicated surface arrives in a later recording story), so all variants map to
/// `IpcErrorCode::Internal` (non-retriable) in the shell's single funnel; the arm
/// exists only to keep that funnel exhaustive.
#[derive(Debug, Error)]
pub enum RecordingError {
    /// A [`RecordingEvent`](crate::recording::RecordingEvent) was applied in a state
    /// where it is not a legal transition — rejected, never silently adopted. `from`
    /// is the state the session was in; `event` is a stable, secret-free event label.
    #[error("illegal recording transition: {event} is not allowed from {from:?}")]
    IllegalTransition {
        /// The state the session was in when the illegal event arrived.
        from: SessionState,
        /// A stable, secret-free label for the rejected event.
        event: String,
    },

    /// The `keeper-rec` sidecar could not be spawned, or its stdout stream failed
    /// with an I/O error. The wrapped string is a non-secret description of the
    /// spawn/IO failure — never a captured-media path or bytes.
    #[error("recording sidecar failed: {0}")]
    SidecarFailed(String),

    /// A `keeper-rec` NDJSON-RPC response violated the typed wire contract (Story
    /// 16.4, AD-34): the sidecar answered with an `error` object, the response was
    /// not parseable JSON, or a required `result` field was missing/mistyped. The
    /// wrapped string is a non-secret description of the wire fault — never a
    /// captured-media path, media bytes, or session material. Distinct from a
    /// protocol-*version* mismatch, which is an honest [`CoreError::Unsupported`].
    #[error("recording protocol error: {0}")]
    Protocol(String),

    /// A session-folder or `manifest.json` filesystem operation failed (Story
    /// 17.2, FR-71, AD-33): creating the session folder, writing the sibling
    /// temp file, renaming it over `manifest.json`, or scanning the folder for
    /// the terminal reconcile. The wrapped string is a secret-free description —
    /// the failing operation name plus the `io::Error` display only, never a
    /// filesystem path, token, or media bytes.
    #[error("recording manifest I/O failed: {0}")]
    ManifestIo(String),

    /// The destination folder failed the validate-on-Start pre-flight (Story
    /// 19.5): `recording_start` probed the chosen folder, the pure
    /// [`evaluate_destination`](crate::recording::evaluate_destination) decision
    /// rejected it, and no capture began — no session folder, no sidecar. The
    /// `reason` carries the actionable, secret-free cause (the message never
    /// leaks a raw path beyond the folder the user already chose and sees).
    #[error("{reason}")]
    DestinationInvalid {
        /// The actionable, secret-free rejection cause.
        reason: DestinationRejection,
    },
}

/// Why the recording destination folder was rejected by the validate-on-Start
/// pre-flight (Story 19.5, Epic 19). Produced by the pure, platform-free
/// [`evaluate_destination`](crate::recording::evaluate_destination) decision
/// from already-probed facts — the shell gathers exists-or-creatable, writable,
/// and free-space; core only decides. Every `Display` is actionable and
/// secret-free: it names what is wrong and what the user can do, never a raw
/// filesystem path, token, or media bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DestinationRejection {
    /// The destination does not exist and could not be created (or exists but
    /// is not a usable directory).
    #[error(
        "the destination folder doesn't exist and couldn't be created — choose another folder"
    )]
    NotADirectory,

    /// The destination exists but a probe-file write failed — recording there
    /// would fail on the first segment.
    #[error("the destination folder isn't writable — choose another folder")]
    NotWritable,

    /// The destination volume has less free space than the shared hard floor
    /// ([`RECORDING_MIN_FREE_BYTES`](crate::recording::RECORDING_MIN_FREE_BYTES)).
    /// Carries the probed figures so the message can name the shortfall.
    #[error(
        "not enough free space to record: {} free, at least {} needed — free up space or choose another folder",
        format_gb(*free_bytes),
        format_gb(*required_bytes)
    )]
    InsufficientSpace {
        /// The probed free bytes on the destination volume.
        free_bytes: u64,
        /// The required hard floor in bytes.
        required_bytes: u64,
    },
}

/// Format a byte count as a short decimal-GB figure for user-facing rejection
/// messages ("1.5 GB"). One decimal place; secret-free (a number, never a path).
fn format_gb(bytes: u64) -> String {
    let gb = bytes as f64 / 1_000_000_000.0;
    format!("{gb:.1} GB")
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

    /// A best-effort receipt/typing signal dispatch failed (Story 3.9, AD-14).
    #[error(transparent)]
    Signal(#[from] SignalError),

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

    /// Local archive setup failed (Story 5.1) — opening `archive.db`. Runtime
    /// archive write failures are swallowed inside the writer task and never reach
    /// here.
    #[error(transparent)]
    Archive(#[from] ArchiveError),

    /// The data-driven bridge catalog failed to parse or validate its embedded
    /// JSON (Story 6.1).
    #[error(transparent)]
    Bridge(#[from] BridgeError),

    /// A recording session transition or `keeper-rec` sidecar run failed (Story
    /// 16.2, AD-33). Does not cross the IPC command surface in this story.
    #[error(transparent)]
    Recording(#[from] RecordingError),

    /// A requested capability is not supported on this platform/build. Honest,
    /// non-panicking signal used by not-yet-wired [`crate::platform::Platform`]
    /// ports.
    #[error("unsupported: {0}")]
    Unsupported(String),

    /// An unexpected internal invariant violation.
    #[error("internal error: {0}")]
    Internal(String),
}
