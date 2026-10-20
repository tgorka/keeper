//! Shell call sites of `keeper_core::hq` (AD-418, AD-422): the process's one
//! [`Hq`], started once the settings layers are installed and reconciled
//! whenever a setting or a settings layer changes, its commands, and the
//! pairing link a scanned QR opens. Every decision — the address grammar,
//! the pairing, which databases sync, every sentence — is core's; this holds
//! it and streams its status.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, OnceLock, PoisonError};

use keeper_core::hq::routes::HttpRoutes;
use keeper_core::hq::syncular::SyncularOpener;
use keeper_core::hq::vm::{HqPairCodeVm, HqStatusVm};
use keeper_core::hq::{Hq, HqError};
use keeper_core::vm::{IpcError, IpcErrorCode};
use tauri::async_runtime::JoinHandle;
use tauri::ipc::Channel;
use tauri::{Emitter, Manager};

/// The event a pairing link raises, so the window opens Settings on it.
pub const PAIR_OFFER_EVENT: &str = "hq-pair-offer";

/// The process's agents server service, set once by [`init`].
static HQ: OnceLock<Arc<Hq>> = OnceLock::new();

/// The live status subscriptions, so an unmounting webview can end its own.
static SUBSCRIPTIONS: LazyLock<Mutex<HashMap<String, JoinHandle<()>>>> =
    LazyLock::new(Mutex::default);

/// Build the service and open what this device holds. Called from setup
/// after the settings layers are installed, so `agents.server_url` is read
/// through them.
pub fn init(app: &tauri::App) {
    let platform = Arc::clone(&app.state::<crate::ipc::AppState>().platform);
    let hq = Hq::new(
        platform,
        Arc::new(SyncularOpener),
        Arc::new(HttpRoutes::new()),
        format!("keeper on {}", keeper_core::config::read_host_label()),
    );
    if HQ.set(Arc::clone(&hq)).is_err() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        if let Err(error) = hq.start().await {
            tracing::warn!(%error, "the agents server could not be started");
        }
    });
}

/// The effective settings may have changed (a setting written, a settings
/// layer installed or cleared, a settings file pulled): follow
/// `agents.server_url` if it moved. Before [`init`] there is nothing to follow.
pub fn reconcile() {
    let Some(hq) = HQ.get().cloned() else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        if let Err(error) = hq.reconcile().await {
            tracing::warn!(%error, "the agents server did not follow its setting");
        }
    });
}

/// The agents servers a request may still reach, for the egress list.
pub fn contacting() -> Vec<String> {
    HQ.get().map(|hq| hq.contacting()).unwrap_or_default()
}

/// A `keeper://agents/pair` link opened this app (a scanned QR): Rust checks
/// it and keeps it; the window opens Settings, where the person confirms.
pub fn link_arrived(app: &tauri::AppHandle, link: &str) {
    let Some(hq) = HQ.get() else {
        tracing::warn!("a pairing link arrived before the agents server started");
        return;
    };
    if let Err(error) = hq.offer_link(link) {
        tracing::info!(%error, "a pairing link was refused");
        return;
    }
    if let Err(error) = app.emit(PAIR_OFFER_EVENT, ()) {
        tracing::warn!(%error, "the pairing link could not be shown");
    }
    #[cfg(desktop)]
    crate::tray::show_main_window(app);
}

fn hq() -> Result<&'static Arc<Hq>, IpcError> {
    HQ.get().ok_or_else(|| IpcError {
        code: IpcErrorCode::Internal,
        message: "the agents server is not started yet".to_owned(),
        account_id: None,
        retriable: true,
    })
}

fn error(error: HqError) -> IpcError {
    IpcError {
        code: IpcErrorCode::Internal,
        message: error.to_string(),
        account_id: None,
        retriable: false,
    }
}

/// Stream the agents server's status — the databases, the devices, a pending
/// pairing link — at once, then every change.
#[tauri::command]
pub fn hq_status_subscribe(channel: Channel<HqStatusVm>) -> Result<String, IpcError> {
    let mut status = hq()?.status();
    let current = status.borrow_and_update().clone();
    if let Err(error) = channel.send(current) {
        tracing::warn!(%error, "the agents server status subscription could not start");
    }
    let task = tauri::async_runtime::spawn(async move {
        while status.changed().await.is_ok() {
            let current = status.borrow_and_update().clone();
            if channel.send(current).is_err() {
                return;
            }
        }
    });
    let id = ulid::Ulid::new().to_string();
    SUBSCRIPTIONS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(id.clone(), task);
    Ok(id)
}

/// End an [`hq_status_subscribe`] stream. An unknown id is a no-op.
#[tauri::command]
pub fn hq_status_unsubscribe(subscription_id: String) {
    if let Some(task) = SUBSCRIPTIONS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&subscription_id)
    {
        task.abort();
    }
}

/// Set `agents.server_url`: blank clears it; a refused address is returned
/// as its sentence and nothing is stored.
#[tauri::command]
pub async fn hq_server_set(url: String) -> Result<HqStatusVm, IpcError> {
    hq()?.set_server(&url).await.map_err(error)
}

/// Forget the server: this device's copies of it, then its pairing.
#[tauri::command]
pub async fn hq_server_forget() -> Result<HqStatusVm, IpcError> {
    hq()?.forget().await.map_err(error)
}

/// A code (and its QR) another device of this person can redeem.
#[tauri::command]
pub async fn hq_pair_start() -> Result<HqPairCodeVm, IpcError> {
    hq()?.pair_start().await.map_err(error)
}

/// Redeem a code, or a pairing link, typed or pasted on this device.
#[tauri::command]
pub async fn hq_pair_redeem(code: String) -> Result<HqStatusVm, IpcError> {
    hq()?.pair_redeem(&code).await.map_err(error)
}

/// Pair with the server of the link a scanned QR opened.
#[tauri::command]
pub async fn hq_pair_offer_accept() -> Result<HqStatusVm, IpcError> {
    hq()?.accept_offer().await.map_err(error)
}

/// Forget the link a scanned QR opened.
#[tauri::command]
pub fn hq_pair_offer_dismiss() -> Result<(), IpcError> {
    hq()?.dismiss_offer();
    Ok(())
}

/// Remove one of the person's devices from the server; the status after.
#[tauri::command]
pub async fn hq_device_revoke(client_id: String) -> Result<HqStatusVm, IpcError> {
    hq()?.revoke(&client_id).await.map_err(error)
}
