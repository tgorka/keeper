//! IPC command layer for the keeper shell (AD-8, AD-21).
//!
//! This is the single place where [`CoreError`] is mapped to the `IpcError`
//! envelope, where `#[tauri::command]`s live, and where the concrete
//! [`Platform`] port is implemented. No business logic lives here — commands
//! delegate to `keeper-core`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use keeper_core::account::AccountManager;
use keeper_core::auth;
use keeper_core::auth::BeeperFlowRegistry;
use keeper_core::demo::snapshot_then_diff;
use keeper_core::error::{
    AccountError, AuthError, BackupError, CoreError, InboxError, PlatformError, SendError,
    TimelineError, VerificationError,
};
use keeper_core::oauth::OAuthFlowRegistry;
use keeper_core::platform::Platform;
use keeper_core::vm::{
    AccountVm, BackupStatus, ConnectionStatusBatch, DemoBatch, EncryptionStatusBatch, InboxBatch,
    IpcError, IpcErrorCode, PingVm, RoomListBatch, TimelineBatch, VerificationFlowVm,
};
use tauri::ipc::Channel;
use tauri::State;

/// Tauri-managed application state holding the injected platform port and the
/// single-account supervisor.
///
/// Keeps the concrete [`Platform`] behind a trait object so the command layer
/// depends only on the port, never a concrete type (AD-24). The
/// [`AccountManager`] owns the live `Client`/`SyncService` and per-subscription
/// tasks (AD-19).
pub struct AppState {
    pub platform: Arc<dyn Platform>,
    pub accounts: AccountManager,
    /// In-flight OIDC (OAuth 2.0 / MSC3861) callback registry (Story 2.2). The
    /// deep-link `on_open_url` handler resolves incoming `keeper://oauth/callback`
    /// URLs against it; each `login_oidc` call registers its pending flow here,
    /// and `cancel_oidc` aborts all pending flows.
    pub oauth_flows: Arc<OAuthFlowRegistry>,
    /// In-flight Beeper email-code login registry (Story 2.3). Holds the
    /// intermediate login-request id between `beeper_request_code` and
    /// `login_beeper` (keyed by email) so it never crosses IPC; `cancel_beeper`
    /// clears it. All `api.beeper.com` HTTP is confined to `keeper-core`.
    pub beeper_flows: Arc<BeeperFlowRegistry>,
}

impl AppState {
    /// Construct the desktop app state with the real platform implementation.
    pub fn new() -> Self {
        Self {
            platform: Arc::new(DesktopPlatform),
            accounts: AccountManager::new(),
            oauth_flows: Arc::new(OAuthFlowRegistry::new()),
            beeper_flows: Arc::new(BeeperFlowRegistry::new()),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Monotonic source of subscription ids handed back to the frontend.
static NEXT_SUBSCRIPTION_ID: AtomicU64 = AtomicU64::new(1);

/// macOS Keychain service name under which all keeper secrets are stored (AD-3).
const KEYCHAIN_SERVICE: &str = "dev.tgorka.keeper";

/// Concrete [`Platform`] implementation for the desktop shell.
///
/// The data-dir port is fully wired via `dirs`; the remaining ports return
/// [`CoreError::Unsupported`] until later stories fill them (honest, never
/// panicking).
pub struct DesktopPlatform;

impl Platform for DesktopPlatform {
    fn data_dir(&self) -> Result<PathBuf, CoreError> {
        let base = dirs::data_dir().ok_or_else(|| {
            PlatformError::DirUnavailable("no OS data directory available".to_owned())
        })?;
        Ok(base.join("dev.tgorka.keeper"))
    }

    fn keychain_set(&self, key: &str, value: &str) -> Result<(), CoreError> {
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, key)
            .map_err(|e| PlatformError::Keychain(format!("could not open keychain entry: {e}")))?;
        entry
            .set_password(value)
            .map_err(|e| PlatformError::Keychain(format!("could not store secret: {e}")))?;
        Ok(())
    }

    fn keychain_get(&self, key: &str) -> Result<Option<String>, CoreError> {
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, key)
            .map_err(|e| PlatformError::Keychain(format!("could not open keychain entry: {e}")))?;
        match entry.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(PlatformError::Keychain(format!("could not read secret: {e}")).into()),
        }
    }

    fn keychain_delete(&self, key: &str) -> Result<(), CoreError> {
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, key)
            .map_err(|e| PlatformError::Keychain(format!("could not open keychain entry: {e}")))?;
        match entry.delete_credential() {
            // Deleting a missing entry is a no-op (rollback safety).
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(PlatformError::Keychain(format!("could not delete secret: {e}")).into()),
        }
    }

    fn open_url(&self, url: &str) -> Result<(), CoreError> {
        // Open in the system default browser (no explicit `with` program). Used
        // by the OIDC flow to present the OAuth authorization URL for consent.
        tauri_plugin_opener::open_url(url, None::<&str>)
            .map_err(|e| CoreError::Internal(format!("could not open the system browser: {e}")))
    }

    fn notify(&self, _title: &str, _body: &str) -> Result<(), CoreError> {
        Err(CoreError::Unsupported(
            "notify not wired until a later story".to_owned(),
        ))
    }

    fn sidecar_path(&self, _name: &str) -> Result<PathBuf, CoreError> {
        Err(CoreError::Unsupported(
            "sidecar_path not wired until a later story".to_owned(),
        ))
    }
}

/// The single `CoreError -> IpcError` mapping (AD-21). Every fallible command
/// funnels its errors through here exactly once.
fn to_ipc_error(err: CoreError) -> IpcError {
    let (code, retriable) = match &err {
        CoreError::Platform(PlatformError::Unsupported(_)) | CoreError::Unsupported(_) => {
            (IpcErrorCode::Unsupported, false)
        }
        CoreError::Platform(PlatformError::DirUnavailable(_)) => (IpcErrorCode::Internal, false),
        CoreError::Platform(PlatformError::Keychain(_)) => (IpcErrorCode::Internal, false),
        CoreError::Internal(_) => (IpcErrorCode::Internal, false),
        CoreError::Auth(AuthError::ServerUnreachable(_)) => (IpcErrorCode::ServerUnreachable, true),
        CoreError::Auth(AuthError::InvalidCredentials) => (IpcErrorCode::InvalidCredentials, false),
        CoreError::Auth(AuthError::UnsupportedLoginType(_)) => {
            (IpcErrorCode::UnsupportedLoginType, false)
        }
        CoreError::Auth(AuthError::SlidingSyncUnsupported) => {
            (IpcErrorCode::SlidingSyncUnsupported, false)
        }
        // OIDC not offered by the homeserver: nothing to retry — the user must
        // pick a different login mechanism.
        CoreError::Auth(AuthError::OAuthUnsupported) => (IpcErrorCode::OauthUnsupported, false),
        // A cancelled / timed-out / failed OIDC flow is retriable: the user can
        // start the browser sign-in again.
        CoreError::Auth(AuthError::OAuthCancelled) => (IpcErrorCode::OauthCancelled, true),
        CoreError::Auth(AuthError::OAuthTimedOut) => (IpcErrorCode::OauthTimedOut, true),
        CoreError::Auth(AuthError::OAuthFailed(_)) => (IpcErrorCode::OauthFailed, true),
        // Every Beeper failure (non-2xx / timeout / transport / shape change /
        // abandoned flow / JWT-login rejection) collapses to this one retriable
        // code: the UI returns to the email step to start a fresh flow.
        CoreError::Auth(AuthError::BeeperUnavailable(_)) => (IpcErrorCode::BeeperUnavailable, true),
        // Any account activation / sync-start failure is retriable: the
        // frontend can attempt the subscribe again.
        CoreError::Account(
            AccountError::SessionMissing
            | AccountError::RestoreFailed(_)
            | AccountError::SyncStart(_),
        ) => (IpcErrorCode::SyncUnavailable, true),
        // A merged-inbox stream start failure is retriable: the frontend can
        // re-subscribe the inbox.
        CoreError::Inbox(InboxError::StreamStart(_)) => (IpcErrorCode::SyncUnavailable, true),
        // A room-not-found or timeline-build failure is retriable: the frontend
        // can attempt the subscribe again.
        CoreError::Timeline(TimelineError::RoomNotFound | TimelineError::Build(_)) => {
            (IpcErrorCode::TimelineUnavailable, true)
        }
        // Any enqueue-time send failure is retriable: the frontend can attempt
        // the send/retry again. Asynchronous delivery failures never reach here —
        // they surface as the `Failed` send-state on the timeline item.
        CoreError::Send(
            SendError::RoomNotFound
            | SendError::NoOpenTimeline
            | SendError::EchoNotFound
            | SendError::Dispatch(_),
        ) => (IpcErrorCode::SendFailed, true),
        // A reply/edit target that isn't in the live timeline, or an edit of a
        // non-own/non-text message, is *not* retriable — re-issuing the same
        // request won't help (Story 3.4). Same `SendFailed` code, `false`.
        CoreError::Send(SendError::TargetNotFound | SendError::NotEditable) => {
            (IpcErrorCode::SendFailed, false)
        }
        // Any verification failure (crypto not ready / flow not found / SDK action
        // failure) is retriable: the user can restart verification.
        CoreError::Verification(
            VerificationError::Unavailable(_)
            | VerificationError::FlowNotFound
            | VerificationError::Action(_),
        ) => (IpcErrorCode::VerificationFailed, true),
        // Key-backup errors carry *named* codes so an invalid recovery key is
        // never a generic failure (FR-14): a malformed key and a
        // well-formed-but-wrong key are distinguished, and an existing-backup
        // race offers restore. All are retriable — the user can try again.
        CoreError::Backup(BackupError::MalformedRecoveryKey) => {
            (IpcErrorCode::BackupMalformedKey, true)
        }
        CoreError::Backup(BackupError::IncorrectRecoveryKey) => {
            (IpcErrorCode::BackupIncorrectKey, true)
        }
        CoreError::Backup(BackupError::AlreadyExistsOnServer) => (IpcErrorCode::BackupExists, true),
        CoreError::Backup(
            BackupError::Unavailable(_) | BackupError::RestoreFailed(_) | BackupError::Action(_),
        ) => (IpcErrorCode::BackupFailed, true),
    };
    IpcError {
        code,
        message: err.to_string(),
        account_id: None,
        retriable,
    }
}

/// Current wall-clock time in milliseconds since the Unix epoch (UTC).
///
/// A skewed clock is clamped (never panics), but the anomaly is surfaced via
/// `tracing` rather than swallowed — a silently-wrong timestamp is a debugging
/// trap for later timeline-ordering stories that consume `ts`.
fn now_ms() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_millis()).unwrap_or_else(|_| {
            tracing::warn!("system clock beyond i64::MAX ms; clamping timestamp to i64::MAX");
            i64::MAX
        }),
        Err(_) => {
            tracing::warn!("system clock is before the Unix epoch; clamping timestamp to 0");
            0
        }
    }
}

/// Liveness command — resolves to a [`PingVm`].
///
/// Exercises the [`Platform`] port end-to-end by resolving the data directory
/// through the injected implementation, proving the platform-free seam.
#[tauri::command]
pub fn app_ping(state: State<'_, AppState>) -> Result<PingVm, IpcError> {
    // Resolve the data dir through the port to prove the seam; discard the
    // path (Story 1.1 does not create it yet).
    let _data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    Ok(PingVm {
        message: "pong".to_owned(),
        ts: now_ms(),
    })
}

/// Open the demo subscription. Emits the snapshot-then-diff batches produced by
/// the tauri-free core over `channel` in order, then returns the subscription
/// id. The first batch delivered is always the snapshot.
#[tauri::command]
pub fn demo_subscribe(channel: Channel<DemoBatch>) -> Result<u64, IpcError> {
    let subscription_id = NEXT_SUBSCRIPTION_ID.fetch_add(1, Ordering::Relaxed);
    for batch in snapshot_then_diff() {
        channel.send(batch).map_err(|e| {
            to_ipc_error(CoreError::Internal(format!(
                "failed to send demo batch: {e}"
            )))
        })?;
    }
    Ok(subscription_id)
}

/// Password login command (FR-1, FR-5).
///
/// Delegates the full ordered flow (store-less SSS probe → persistent login →
/// Keychain + registry, with rollback on failure) to `keeper-core`. The
/// `password` argument is transient: it drives the SDK login only and is never
/// returned, stored, or logged. On success resolves to a non-secret
/// [`AccountVm`]; on failure funnels the `CoreError` through [`to_ipc_error`].
#[tauri::command]
pub async fn login_password(
    state: State<'_, AppState>,
    homeserver: String,
    username: String,
    password: String,
) -> Result<AccountVm, IpcError> {
    auth::login_password(state.platform.as_ref(), &homeserver, &username, &password)
        .await
        .map_err(to_ipc_error)
}

/// OIDC (OAuth 2.0 / MSC3861) login command (Story 2.2).
///
/// Runs the shared add-account flow with the OIDC mechanism: the whole browser
/// round-trip (open the system browser, await the `keeper://oauth/callback` deep
/// link, finish the token exchange) happens inside the core `authenticate` step.
/// The pending flow is keyed by its OAuth `state` in the shared registry so the
/// deep-link `on_open_url` handler can route the callback back to it; a
/// concurrent `cancel_oidc` aborts it. On success resolves to a non-secret
/// [`AccountVm`]; on failure (unsupported / timed-out / cancelled / failed /
/// non-SSS) funnels the `CoreError` through [`to_ipc_error`]. No token or
/// authorization `code`/`state` ever crosses back to JavaScript.
#[tauri::command]
pub async fn login_oidc(
    state: State<'_, AppState>,
    homeserver: String,
) -> Result<AccountVm, IpcError> {
    auth::login_oidc(
        state.platform.as_ref(),
        &homeserver,
        state.oauth_flows.clone(),
    )
    .await
    .map_err(to_ipc_error)
}

/// Cancel any in-progress OIDC flow(s) (Story 2.2).
///
/// Aborts every pending flow in the registry (there is at most one add-account
/// flow at a time in the UI); the awaiting `authenticate` resolves as cancelled,
/// `add_account` rolls back, and the UI returns quietly to the form. Idempotent —
/// with no pending flow it is a no-op.
#[tauri::command]
pub fn cancel_oidc(state: State<'_, AppState>) -> Result<(), IpcError> {
    state.oauth_flows.cancel_all();
    Ok(())
}

/// Request a Beeper email login code (Story 2.3, step 1). Delegates to the core,
/// which runs `POST /user/login` → `POST /user/login/email` and stores the
/// intermediate request id (keyed by `email`) in the registry so it never
/// crosses IPC. Resolves on success (a code has been emailed); any Beeper failure
/// funnels through [`to_ipc_error`] to the retriable `beeperUnavailable` code. No
/// bearer token, request id, or JWT ever crosses back to JavaScript.
#[tauri::command]
pub async fn beeper_request_code(
    state: State<'_, AppState>,
    email: String,
) -> Result<(), IpcError> {
    state
        .beeper_flows
        .request_code(&email)
        .await
        .map_err(to_ipc_error)
}

/// Complete a Beeper email-code login (Story 2.3, step 2). Delegates to the core,
/// which takes the stored request id for `email`, runs `POST
/// /user/login/response` to obtain the JWT, then completes login via
/// `org.matrix.login.jwt` through the shared add-account pipeline (store-less SSS
/// gate → persistent store → Keychain → registry, with rollback on failure). On
/// success resolves to a non-secret [`AccountVm`]; any Beeper failure (including
/// an abandoned flow with no stored request id) funnels through [`to_ipc_error`]
/// to the retriable `beeperUnavailable` code. The emailed `code` is transient —
/// never returned, stored, or logged.
#[tauri::command]
pub async fn login_beeper(
    state: State<'_, AppState>,
    email: String,
    code: String,
) -> Result<AccountVm, IpcError> {
    state
        .beeper_flows
        .login(state.platform.as_ref(), &email, &code)
        .await
        .map_err(to_ipc_error)
}

/// Cancel any in-progress Beeper login flow(s) (Story 2.3). Clears the registry
/// so no pending request id lingers; nothing is persisted. Idempotent — with no
/// pending flow it is a no-op.
#[tauri::command]
pub fn cancel_beeper(state: State<'_, AppState>) -> Result<(), IpcError> {
    state.beeper_flows.cancel_all();
    Ok(())
}

/// Persist the app-wide at-rest encryption posture (Story 2.6, AD-22). Writes
/// `on`/`off` to the `settings` table in `keeper.db`. Sync — the value is a
/// non-secret app-wide flag; the per-account passphrase is generated and stored
/// (Keychain only) later, inside `add_account`. Failures funnel through
/// [`to_ipc_error`].
#[tauri::command]
pub fn set_encryption_posture(state: State<'_, AppState>, enabled: bool) -> Result<(), IpcError> {
    auth::set_encryption_posture(state.platform.as_ref(), enabled).map_err(to_ipc_error)
}

/// Read the app-wide at-rest encryption posture (Story 2.6). Resolves to
/// `Some(true)` (on), `Some(false)` (off), or `None` (unchosen — the fresh-install
/// state that gates the first-run choice). `Option<bool>` serializes to
/// `boolean | null` across IPC. Failures funnel through [`to_ipc_error`].
#[tauri::command]
pub fn encryption_posture(state: State<'_, AppState>) -> Result<Option<bool>, IpcError> {
    auth::get_encryption_posture(state.platform.as_ref()).map_err(to_ipc_error)
}

/// Subscribe to an account's sliding-sync room list (FR-8, AD-8/9/19/20).
///
/// Lazily activates the account (session restore + `SyncService`), then streams
/// [`RoomListBatch`]es over `channel` — a `Reset` snapshot first, then diffs —
/// and returns the subscription id. The sink forwards each batch to the channel;
/// a closed channel simply drops the batch (the frontend has unsubscribed).
#[tauri::command]
pub async fn room_list_subscribe(
    state: State<'_, AppState>,
    account_id: String,
    channel: Channel<RoomListBatch>,
) -> Result<u64, IpcError> {
    let sink = Box::new(move |batch: RoomListBatch| channel.send(batch).is_ok());
    state
        .accounts
        .subscribe_room_list(&state.platform, &account_id, sink)
        .await
        .map_err(to_ipc_error)
}

/// Unsubscribe exactly one room-list subscription, aborting its producer task
/// (AD-19). Other account state is untouched. Idempotent.
#[tauri::command]
pub async fn room_list_unsubscribe(
    state: State<'_, AppState>,
    account_id: String,
    subscription_id: u64,
) -> Result<(), IpcError> {
    state
        .accounts
        .unsubscribe_room_list(&account_id, subscription_id)
        .await;
    Ok(())
}

/// Subscribe to a room's timeline (FR-8, FR-9, AD-4/AD-8/AD-19).
///
/// Reuses the account's live session (activating it lazily), opens the room's
/// SDK `Timeline`, and streams [`TimelineBatch`]es over `channel` — a `Reset`
/// snapshot first, then diffs — returning the subscription id. The sink forwards
/// each batch to the channel; a closed channel simply drops the batch (the
/// frontend has unsubscribed). A room-not-found / timeline-build failure funnels
/// through [`to_ipc_error`] to `TimelineUnavailable`.
#[tauri::command]
pub async fn timeline_subscribe(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    channel: Channel<TimelineBatch>,
) -> Result<u64, IpcError> {
    let sink = Box::new(move |batch: TimelineBatch| channel.send(batch).is_ok());
    state
        .accounts
        .subscribe_timeline(&state.platform, &account_id, &room_id, sink)
        .await
        .map_err(to_ipc_error)
}

/// Unsubscribe exactly one timeline subscription, aborting its producer task and
/// dropping its `Timeline` (AD-19). Other account state is untouched. Idempotent.
#[tauri::command]
pub async fn timeline_unsubscribe(
    state: State<'_, AppState>,
    account_id: String,
    subscription_id: u64,
) -> Result<(), IpcError> {
    state
        .accounts
        .unsubscribe_timeline(&account_id, subscription_id)
        .await;
    Ok(())
}

/// Send a plain-text message to a room through the single dispatch gate (FR-9,
/// FR-41, AD-13). Delegates to the core, which enqueues the message on the room's
/// open `Timeline`; the local echo and every send-state transition arrive back
/// over the existing timeline subscription (no echo is synthesized). An
/// enqueue-time failure funnels through [`to_ipc_error`] to `SendFailed`.
#[tauri::command]
pub async fn send_text(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    body: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .send_text(&account_id, &room_id, &body)
        .await
        .map_err(to_ipc_error)
}

/// Send a plain-text reply to a message through the single dispatch gate (FR-10,
/// FR-41, AD-13, Story 3.4). `inReplyToKey` is the *original* message's opaque
/// render `key` (its `unique_id`); the Rust core resolves it to the event id and
/// enqueues the reply. The reply's local echo and send-state transitions arrive
/// back over the existing timeline subscription (no echo is synthesized). A
/// missing target / enqueue failure funnels through [`to_ipc_error`] to
/// `SendFailed`.
#[tauri::command]
pub async fn send_reply(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    in_reply_to_key: String,
    body: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .send_reply(&account_id, &room_id, &in_reply_to_key, &body)
        .await
        .map_err(to_ipc_error)
}

/// Edit an own text message in place through the single dispatch gate (FR-11,
/// FR-41, AD-13, Story 3.4). `itemKey` is the message's opaque render `key` (its
/// `unique_id`); the Rust core resolves it, gates on editability (own + text), and
/// enqueues the edit. The `Set` diff that updates the content in place (and flips
/// `isEdited`) arrives back over the existing timeline subscription. A missing
/// target / non-editable message / enqueue failure funnels through
/// [`to_ipc_error`] to `SendFailed`.
#[tauri::command]
pub async fn edit_message(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    item_key: String,
    body: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .edit_message(&account_id, &room_id, &item_key, &body)
        .await
        .map_err(to_ipc_error)
}

/// Subscribe to an account's connection status (FR-8/FR-9, UX-DR18, AD-8).
///
/// Lazily activates the account (reusing the room-list/timeline path), then
/// streams [`ConnectionStatusBatch`]es over `channel` — an initial snapshot of
/// the current status, then deduped changes — and returns the subscription id.
/// The sink forwards each batch to the channel; a closed channel simply drops
/// the batch (the frontend has unsubscribed). An activation failure funnels
/// through [`to_ipc_error`] to the existing `SyncUnavailable` code.
#[tauri::command]
pub async fn connection_status_subscribe(
    state: State<'_, AppState>,
    account_id: String,
    channel: Channel<ConnectionStatusBatch>,
) -> Result<u64, IpcError> {
    let sink = Box::new(move |batch: ConnectionStatusBatch| channel.send(batch).is_ok());
    state
        .accounts
        .subscribe_connection_status(&state.platform, &account_id, sink)
        .await
        .map_err(to_ipc_error)
}

/// Unsubscribe exactly one connection-status subscription, aborting its producer
/// task (AD-19). Other account state is untouched. Idempotent.
#[tauri::command]
pub async fn connection_status_unsubscribe(
    state: State<'_, AppState>,
    account_id: String,
    subscription_id: u64,
) -> Result<(), IpcError> {
    state
        .accounts
        .unsubscribe_connection_status(&account_id, subscription_id)
        .await;
    Ok(())
}

/// Subscribe to an account's encryption (device-verification) status (Story 3.1,
/// AD-8).
///
/// Lazily activates the account (reusing the room-list/timeline/connection path),
/// then streams [`EncryptionStatusBatch`]es over `channel` — an initial snapshot
/// of the current status, then deduped changes — and returns the subscription id.
/// The sink forwards each batch to the channel; a closed channel simply drops the
/// batch (the frontend has unsubscribed). An activation failure funnels through
/// [`to_ipc_error`] to the existing `SyncUnavailable` code.
#[tauri::command]
pub async fn encryption_status_subscribe(
    state: State<'_, AppState>,
    account_id: String,
    channel: Channel<EncryptionStatusBatch>,
) -> Result<u64, IpcError> {
    let sink = Box::new(move |batch: EncryptionStatusBatch| channel.send(batch).is_ok());
    state
        .accounts
        .subscribe_encryption_status(&state.platform, &account_id, sink)
        .await
        .map_err(to_ipc_error)
}

/// Unsubscribe exactly one encryption-status subscription, aborting its producer
/// task (AD-19). Other account state is untouched. Idempotent.
#[tauri::command]
pub async fn encryption_status_unsubscribe(
    state: State<'_, AppState>,
    account_id: String,
    subscription_id: u64,
) -> Result<(), IpcError> {
    state
        .accounts
        .unsubscribe_encryption_status(&account_id, subscription_id)
        .await;
    Ok(())
}

/// Subscribe to an account's interactive device self-verification flow (Story
/// 3.2, FR-14, AD-1, AD-8).
///
/// Lazily activates the account, then streams [`VerificationFlowVm`] snapshots
/// over `channel` — the flow's state machine (waiting → compare emoji / show QR →
/// confirmed → done/cancelled/failed). An *incoming* request (the peer started it)
/// surfaces here as a `Requested` snapshot so the UI can auto-open the modal. The
/// sink forwards each snapshot to the channel; a closed channel drops the snapshot
/// (the frontend unsubscribed). NO `Verification`/SAS/QR object, key, or plaintext
/// crosses IPC — only the rendered VM. Activation failure funnels through
/// [`to_ipc_error`] to `SyncUnavailable`.
#[tauri::command]
pub async fn verification_subscribe(
    state: State<'_, AppState>,
    account_id: String,
    channel: Channel<VerificationFlowVm>,
) -> Result<u64, IpcError> {
    let sink = Box::new(move |vm: VerificationFlowVm| channel.send(vm).is_ok());
    state
        .accounts
        .subscribe_verification(&state.platform, &account_id, sink)
        .await
        .map_err(to_ipc_error)
}

/// Unsubscribe exactly one verification subscription, aborting its producer task
/// and clearing the account's flow sender (AD-19). Idempotent.
#[tauri::command]
pub async fn verification_unsubscribe(
    state: State<'_, AppState>,
    account_id: String,
    subscription_id: u64,
) -> Result<(), IpcError> {
    state
        .accounts
        .unsubscribe_verification(&account_id, subscription_id)
        .await;
    Ok(())
}

/// Start an interactive self-verification from keeper against the user's other
/// session (Story 3.2, FR-14). Requests the verification in Rust and feeds the new
/// flow id into the live verification producer so it streams over the existing
/// verification subscription. Requires an active verification subscription.
/// Failures funnel through [`to_ipc_error`] to `VerificationFailed`.
#[tauri::command]
pub async fn verification_start(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .verification_start(&account_id)
        .await
        .map_err(to_ipc_error)
}

/// Accept an incoming verification request the peer started (Story 3.2). Moves the
/// flow from `Requested` to `Ready`. Failures funnel through [`to_ipc_error`].
#[tauri::command]
pub async fn verification_accept(
    state: State<'_, AppState>,
    account_id: String,
    flow_id: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .verification_accept(&account_id, &flow_id)
        .await
        .map_err(to_ipc_error)
}

/// Start the emoji/SAS sub-flow on a ready request (Story 3.2). The SAS state
/// transition arrives over the verification stream. Failures funnel through
/// [`to_ipc_error`].
#[tauri::command]
pub async fn verification_start_sas(
    state: State<'_, AppState>,
    account_id: String,
    flow_id: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .verification_start_sas(&account_id, &flow_id)
        .await
        .map_err(to_ipc_error)
}

/// Confirm the SAS emoji match on our side (Story 3.2). On both sides confirming,
/// the SDK completes verification and 3.1's `verification_state()` stream flips the
/// account to `Verified`. Failures funnel through [`to_ipc_error`].
#[tauri::command]
pub async fn verification_confirm(
    state: State<'_, AppState>,
    account_id: String,
    flow_id: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .verification_confirm(&account_id, &flow_id)
        .await
        .map_err(to_ipc_error)
}

/// Signal that the SAS emoji do NOT match (Story 3.2). Cancels the flow with the
/// SDK mismatch code, which surfaces as `Failed`. Failures funnel through
/// [`to_ipc_error`].
#[tauri::command]
pub async fn verification_mismatch(
    state: State<'_, AppState>,
    account_id: String,
    flow_id: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .verification_mismatch(&account_id, &flow_id)
        .await
        .map_err(to_ipc_error)
}

/// Cancel the verification flow (Story 3.2) — the user closed the modal / pressed
/// Esc. Cancels the active SAS or the request; a missing flow is a no-op. Failures
/// funnel through [`to_ipc_error`].
#[tauri::command]
pub async fn verification_cancel(
    state: State<'_, AppState>,
    account_id: String,
    flow_id: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .verification_cancel(&account_id, &flow_id)
        .await
        .map_err(to_ipc_error)
}

/// Subscribe to an account's server-side key-backup status (Story 3.3, FR-14,
/// AD-8).
///
/// Lazily activates the account (reusing the shared session path), then streams
/// [`BackupStatus`] snapshots over `channel` — an initial snapshot of the current
/// status, then deduped changes — and returns the subscription id. The sink
/// forwards each status to the channel; a closed channel drops the status (the
/// frontend unsubscribed). NO recovery key or secret-storage material crosses IPC
/// — only the enum tag. An activation failure funnels through [`to_ipc_error`] to
/// the existing `SyncUnavailable` code.
#[tauri::command]
pub async fn backup_status_subscribe(
    state: State<'_, AppState>,
    account_id: String,
    channel: Channel<BackupStatus>,
) -> Result<u64, IpcError> {
    let sink = Box::new(move |status: BackupStatus| channel.send(status).is_ok());
    state
        .accounts
        .subscribe_backup_status(&state.platform, &account_id, sink)
        .await
        .map_err(to_ipc_error)
}

/// Unsubscribe exactly one backup-status subscription, aborting its backend
/// producer task (AD-19). Other account state is untouched. Idempotent.
#[tauri::command]
pub async fn backup_status_unsubscribe(
    state: State<'_, AppState>,
    account_id: String,
    subscription_id: u64,
) -> Result<(), IpcError> {
    state
        .accounts
        .unsubscribe_backup_status(&account_id, subscription_id)
        .await;
    Ok(())
}

/// Enable server-side key backup for the account (Story 3.3, FR-14). Delegates to
/// the core, which creates the backup + secret store and returns the base58
/// **recovery key** *once* — the deliberate boundary exception, meant for the
/// human to save (shown once in `mono`). A race with an existing server backup
/// funnels through [`to_ipc_error`] to the named `backupExists` code so the modal
/// can offer restore; any other failure maps to `backupFailed`. The recovery key
/// is never logged.
#[tauri::command]
pub async fn backup_enable(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<String, IpcError> {
    state
        .accounts
        .backup_enable(&account_id)
        .await
        .map_err(to_ipc_error)
}

/// Restore from server-side key backup with a recovery key (Story 3.3, FR-14).
/// Delegates to the core, which opens the secret store and imports secrets; the
/// SDK then downloads room keys automatically, so 3.1's streams re-render
/// previously-undecryptable rows with no extra code. An invalid key funnels
/// through [`to_ipc_error`] to a *named* code (`backupMalformedKey` vs
/// `backupIncorrectKey`), never a generic failure. The recovery key is never
/// logged.
#[tauri::command]
pub async fn backup_restore(
    state: State<'_, AppState>,
    account_id: String,
    recovery_key: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .backup_restore(&account_id, &recovery_key)
        .await
        .map_err(to_ipc_error)
}

/// Save a recovery key to the OS Keychain (Story 3.3, FR-14) — the user's opt-in
/// after seeing the key once. Delegates to the core, which writes it at
/// `recovery_key/<account_id>` via the [`Platform`] keychain port. A write
/// failure funnels through [`to_ipc_error`] so the modal can keep the key visible
/// for manual copy. The recovery key is never logged.
#[tauri::command]
pub async fn backup_save_recovery_key(
    state: State<'_, AppState>,
    account_id: String,
    recovery_key: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .backup_save_recovery_key(&state.platform, &account_id, &recovery_key)
        .await
        .map_err(to_ipc_error)
}

/// Read a previously-saved recovery key from the OS Keychain (Story 3.3) to
/// prefill the restore textarea, or `None` if none was saved. `Option<String>`
/// serializes to `string | null` across IPC. Failures funnel through
/// [`to_ipc_error`]. The recovery key is never logged.
#[tauri::command]
pub async fn backup_saved_recovery_key(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<Option<String>, IpcError> {
    state
        .accounts
        .backup_saved_recovery_key(&state.platform, &account_id)
        .await
        .map_err(to_ipc_error)
}

/// Retry a failed outgoing message by re-driving its wedged local echo through
/// the controlled send path (`unwedge`, not a new dispatch — FR-41). `item_key`
/// is the timeline item's opaque `unique_id`. A missing echo / no open timeline
/// funnels through [`to_ipc_error`] to `SendFailed`.
#[tauri::command]
pub async fn send_retry(
    state: State<'_, AppState>,
    account_id: String,
    room_id: String,
    item_key: String,
) -> Result<(), IpcError> {
    state
        .accounts
        .retry_send(&account_id, &room_id, &item_key)
        .await
        .map_err(to_ipc_error)
}

/// Report every persisted account that can be restored on launch (FR-8, AD-20).
/// Identity only — delegates to the core, which lists the registry rows and
/// returns each whose Keychain session is present as a non-secret [`AccountVm`]
/// (with hue). Resolves to an empty array on a cold install; a row whose session
/// is gone is skipped, not fatal. No eager activation: the lazy inbox subscribe
/// restores each session. Failures funnel through [`to_ipc_error`].
#[tauri::command]
pub async fn session_restore(state: State<'_, AppState>) -> Result<Vec<AccountVm>, IpcError> {
    auth::find_restorable_accounts(state.platform.as_ref()).map_err(to_ipc_error)
}

/// Subscribe to the merged unified inbox across every restorable account (FR-18,
/// AD-20). Activates each account, opens its room-list stream, and streams one
/// recency-ordered [`InboxBatch`] over `channel` (a `Reset` window that updates
/// as accounts sync or are added/removed). Returns the inbox subscription id.
/// Ordering and filtering are computed in `keeper-core::inbox`, never in JS. A
/// stream-start failure funnels through [`to_ipc_error`] to `SyncUnavailable`.
#[tauri::command]
pub async fn inbox_subscribe(
    state: State<'_, AppState>,
    channel: Channel<InboxBatch>,
) -> Result<u64, IpcError> {
    let sink = Box::new(move |batch: InboxBatch| channel.send(batch).is_ok());
    state
        .accounts
        .subscribe_inbox(&state.platform, sink)
        .await
        .map_err(to_ipc_error)
}

/// Unsubscribe the merged inbox, aborting every per-account producer feeding it
/// (AD-20). Idempotent — a mismatched/unknown id is a no-op.
#[tauri::command]
pub async fn inbox_unsubscribe(
    state: State<'_, AppState>,
    subscription_id: u64,
) -> Result<(), IpcError> {
    state.accounts.unsubscribe_inbox(subscription_id).await;
    Ok(())
}

/// Sign out an account locally (AD-10, Story 1.8). Delegates to the core, which
/// tears down the account's live supervision tasks then deletes exactly its SDK
/// store dir, Keychain session entry, and registry row — no server-side logout,
/// works offline, and is idempotent whether or not the account was ever
/// activated. Failures funnel through [`to_ipc_error`].
#[tauri::command]
pub async fn sign_out(state: State<'_, AppState>, account_id: String) -> Result<(), IpcError> {
    state
        .accounts
        .sign_out(&state.platform, &account_id)
        .await
        .map_err(to_ipc_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_ms_is_positive() {
        assert!(now_ms() > 0);
    }

    #[test]
    fn unsupported_core_error_maps_to_unsupported_code() {
        let ipc = to_ipc_error(CoreError::Unsupported("nope".to_owned()));
        assert_eq!(ipc.code, IpcErrorCode::Unsupported);
        assert!(!ipc.retriable);
        assert_eq!(ipc.account_id, None);
    }

    #[test]
    fn dir_unavailable_maps_to_internal_code() {
        let ipc = to_ipc_error(CoreError::Platform(PlatformError::DirUnavailable(
            "x".to_owned(),
        )));
        assert_eq!(ipc.code, IpcErrorCode::Internal);
    }

    #[test]
    fn desktop_platform_data_dir_is_wired() {
        let p = DesktopPlatform;
        let dir = p
            .data_dir()
            .expect("data_dir should resolve on the test host");
        assert!(dir.ends_with("dev.tgorka.keeper"));
    }

    #[test]
    fn keychain_error_maps_to_internal_code() {
        let ipc = to_ipc_error(CoreError::Platform(PlatformError::Keychain(
            "boom".to_owned(),
        )));
        assert_eq!(ipc.code, IpcErrorCode::Internal);
        assert!(!ipc.retriable);
    }

    #[test]
    fn auth_server_unreachable_maps_to_retriable_code() {
        let ipc = to_ipc_error(CoreError::Auth(AuthError::ServerUnreachable(
            "x".to_owned(),
        )));
        assert_eq!(ipc.code, IpcErrorCode::ServerUnreachable);
        assert!(ipc.retriable, "unreachable server should be retriable");
    }

    #[test]
    fn auth_invalid_credentials_maps_to_non_retriable_code() {
        let ipc = to_ipc_error(CoreError::Auth(AuthError::InvalidCredentials));
        assert_eq!(ipc.code, IpcErrorCode::InvalidCredentials);
        assert!(!ipc.retriable);
    }

    #[test]
    fn auth_unsupported_login_type_maps_to_non_retriable_code() {
        let ipc = to_ipc_error(CoreError::Auth(AuthError::UnsupportedLoginType(
            "x".to_owned(),
        )));
        assert_eq!(ipc.code, IpcErrorCode::UnsupportedLoginType);
        assert!(!ipc.retriable);
    }

    #[test]
    fn auth_sliding_sync_unsupported_maps_to_non_retriable_code() {
        let ipc = to_ipc_error(CoreError::Auth(AuthError::SlidingSyncUnsupported));
        assert_eq!(ipc.code, IpcErrorCode::SlidingSyncUnsupported);
        assert!(!ipc.retriable);
    }

    #[test]
    fn auth_oauth_unsupported_maps_to_non_retriable_code() {
        let ipc = to_ipc_error(CoreError::Auth(AuthError::OAuthUnsupported));
        assert_eq!(ipc.code, IpcErrorCode::OauthUnsupported);
        assert!(!ipc.retriable, "an unsupported server is not retriable");
    }

    #[test]
    fn auth_oauth_timed_out_maps_to_retriable_code() {
        let ipc = to_ipc_error(CoreError::Auth(AuthError::OAuthTimedOut));
        assert_eq!(ipc.code, IpcErrorCode::OauthTimedOut);
        assert!(ipc.retriable, "a timed-out sign-in may be retried");
    }

    #[test]
    fn auth_oauth_cancelled_maps_to_retriable_code() {
        let ipc = to_ipc_error(CoreError::Auth(AuthError::OAuthCancelled));
        assert_eq!(ipc.code, IpcErrorCode::OauthCancelled);
        assert!(ipc.retriable, "a cancelled sign-in may be retried");
    }

    #[test]
    fn auth_oauth_failed_maps_to_retriable_code() {
        let ipc = to_ipc_error(CoreError::Auth(AuthError::OAuthFailed(
            "access_denied".to_owned(),
        )));
        assert_eq!(ipc.code, IpcErrorCode::OauthFailed);
        assert!(ipc.retriable, "a failed sign-in may be retried");
    }

    #[test]
    fn auth_beeper_unavailable_maps_to_retriable_code() {
        let ipc = to_ipc_error(CoreError::Auth(AuthError::BeeperUnavailable(
            "the Beeper login service returned an error".to_owned(),
        )));
        assert_eq!(ipc.code, IpcErrorCode::BeeperUnavailable);
        assert!(ipc.retriable, "a Beeper failure may be retried");
    }

    #[test]
    fn account_session_missing_maps_to_retriable_sync_unavailable() {
        let ipc = to_ipc_error(CoreError::Account(AccountError::SessionMissing));
        assert_eq!(ipc.code, IpcErrorCode::SyncUnavailable);
        assert!(ipc.retriable, "sync unavailable should be retriable");
    }

    #[test]
    fn account_restore_failed_maps_to_retriable_sync_unavailable() {
        let ipc = to_ipc_error(CoreError::Account(AccountError::RestoreFailed(
            "boom".to_owned(),
        )));
        assert_eq!(ipc.code, IpcErrorCode::SyncUnavailable);
        assert!(ipc.retriable);
    }

    #[test]
    fn account_sync_start_maps_to_retriable_sync_unavailable() {
        let ipc = to_ipc_error(CoreError::Account(AccountError::SyncStart(
            "boom".to_owned(),
        )));
        assert_eq!(ipc.code, IpcErrorCode::SyncUnavailable);
        assert!(ipc.retriable);
    }

    #[test]
    fn timeline_room_not_found_maps_to_retriable_timeline_unavailable() {
        let ipc = to_ipc_error(CoreError::Timeline(TimelineError::RoomNotFound));
        assert_eq!(ipc.code, IpcErrorCode::TimelineUnavailable);
        assert!(ipc.retriable, "timeline unavailable should be retriable");
    }

    #[test]
    fn timeline_build_maps_to_retriable_timeline_unavailable() {
        let ipc = to_ipc_error(CoreError::Timeline(TimelineError::Build("boom".to_owned())));
        assert_eq!(ipc.code, IpcErrorCode::TimelineUnavailable);
        assert!(ipc.retriable);
    }

    #[test]
    fn send_room_not_found_maps_to_retriable_send_failed() {
        let ipc = to_ipc_error(CoreError::Send(SendError::RoomNotFound));
        assert_eq!(ipc.code, IpcErrorCode::SendFailed);
        assert!(ipc.retriable, "send failure should be retriable");
    }

    #[test]
    fn send_no_open_timeline_maps_to_retriable_send_failed() {
        let ipc = to_ipc_error(CoreError::Send(SendError::NoOpenTimeline));
        assert_eq!(ipc.code, IpcErrorCode::SendFailed);
        assert!(ipc.retriable);
    }

    #[test]
    fn send_echo_not_found_maps_to_retriable_send_failed() {
        let ipc = to_ipc_error(CoreError::Send(SendError::EchoNotFound));
        assert_eq!(ipc.code, IpcErrorCode::SendFailed);
        assert!(ipc.retriable);
    }

    #[test]
    fn send_dispatch_maps_to_retriable_send_failed() {
        let ipc = to_ipc_error(CoreError::Send(SendError::Dispatch("boom".to_owned())));
        assert_eq!(ipc.code, IpcErrorCode::SendFailed);
        assert!(ipc.retriable);
    }

    #[test]
    fn send_target_not_found_maps_to_non_retriable_send_failed() {
        let ipc = to_ipc_error(CoreError::Send(SendError::TargetNotFound));
        assert_eq!(ipc.code, IpcErrorCode::SendFailed);
        assert!(
            !ipc.retriable,
            "a missing reply/edit target is not retriable"
        );
    }

    #[test]
    fn send_not_editable_maps_to_non_retriable_send_failed() {
        let ipc = to_ipc_error(CoreError::Send(SendError::NotEditable));
        assert_eq!(ipc.code, IpcErrorCode::SendFailed);
        assert!(!ipc.retriable, "a non-editable message is not retriable");
    }

    #[test]
    fn verification_unavailable_maps_to_retriable_verification_failed() {
        let ipc = to_ipc_error(CoreError::Verification(VerificationError::Unavailable(
            "no identity".to_owned(),
        )));
        assert_eq!(ipc.code, IpcErrorCode::VerificationFailed);
        assert!(ipc.retriable, "verification failure should be retriable");
    }

    #[test]
    fn verification_flow_not_found_maps_to_retriable_verification_failed() {
        let ipc = to_ipc_error(CoreError::Verification(VerificationError::FlowNotFound));
        assert_eq!(ipc.code, IpcErrorCode::VerificationFailed);
        assert!(ipc.retriable);
    }

    #[test]
    fn verification_action_maps_to_retriable_verification_failed() {
        let ipc = to_ipc_error(CoreError::Verification(VerificationError::Action(
            "boom".to_owned(),
        )));
        assert_eq!(ipc.code, IpcErrorCode::VerificationFailed);
        assert!(ipc.retriable);
    }

    #[test]
    fn backup_malformed_key_maps_to_named_code() {
        let ipc = to_ipc_error(CoreError::Backup(BackupError::MalformedRecoveryKey));
        assert_eq!(ipc.code, IpcErrorCode::BackupMalformedKey);
        assert!(ipc.retriable);
    }

    #[test]
    fn backup_incorrect_key_maps_to_named_code() {
        let ipc = to_ipc_error(CoreError::Backup(BackupError::IncorrectRecoveryKey));
        assert_eq!(ipc.code, IpcErrorCode::BackupIncorrectKey);
        assert!(ipc.retriable);
    }

    #[test]
    fn backup_already_exists_maps_to_backup_exists_code() {
        let ipc = to_ipc_error(CoreError::Backup(BackupError::AlreadyExistsOnServer));
        assert_eq!(ipc.code, IpcErrorCode::BackupExists);
        assert!(ipc.retriable);
    }

    #[test]
    fn backup_unavailable_maps_to_backup_failed_code() {
        let ipc = to_ipc_error(CoreError::Backup(BackupError::Unavailable("x".to_owned())));
        assert_eq!(ipc.code, IpcErrorCode::BackupFailed);
        assert!(ipc.retriable);
    }

    #[test]
    fn backup_restore_failed_maps_to_backup_failed_code() {
        let ipc = to_ipc_error(CoreError::Backup(BackupError::RestoreFailed(
            "boom".to_owned(),
        )));
        assert_eq!(ipc.code, IpcErrorCode::BackupFailed);
        assert!(ipc.retriable);
    }

    #[test]
    fn backup_action_maps_to_backup_failed_code() {
        let ipc = to_ipc_error(CoreError::Backup(BackupError::Action("boom".to_owned())));
        assert_eq!(ipc.code, IpcErrorCode::BackupFailed);
        assert!(ipc.retriable);
    }
}
