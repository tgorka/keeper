//! IPC command layer for the keeper shell (AD-8, AD-21).
//!
//! This is the single place where [`CoreError`] is mapped to the `IpcError`
//! envelope, where `#[tauri::command]`s live, and where the concrete
//! [`Platform`] port is implemented. No business logic lives here — commands
//! delegate to `keeper-core`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use keeper_core::account::AccountManager;
use keeper_core::auth;
use keeper_core::demo::snapshot_then_diff;
use keeper_core::error::{AccountError, AuthError, CoreError, PlatformError, TimelineError};
use keeper_core::platform::Platform;
use keeper_core::vm::{
    AccountVm, DemoBatch, IpcError, IpcErrorCode, PingVm, RoomListBatch, TimelineBatch,
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
    pub platform: Box<dyn Platform>,
    pub accounts: AccountManager,
}

impl AppState {
    /// Construct the desktop app state with the real platform implementation.
    pub fn new() -> Self {
        Self {
            platform: Box::new(DesktopPlatform),
            accounts: AccountManager::new(),
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
        // Any account activation / sync-start failure is retriable: the
        // frontend can attempt the subscribe again.
        CoreError::Account(
            AccountError::SessionMissing
            | AccountError::RestoreFailed(_)
            | AccountError::SyncStart(_),
        ) => (IpcErrorCode::SyncUnavailable, true),
        // A room-not-found or timeline-build failure is retriable: the frontend
        // can attempt the subscribe again.
        CoreError::Timeline(TimelineError::RoomNotFound | TimelineError::Build(_)) => {
            (IpcErrorCode::TimelineUnavailable, true)
        }
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
        .subscribe_room_list(state.platform.as_ref(), &account_id, sink)
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
        .subscribe_timeline(state.platform.as_ref(), &account_id, &room_id, sink)
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
}
