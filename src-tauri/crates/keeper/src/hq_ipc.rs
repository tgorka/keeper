//! Shell call sites of `keeper_core::hq` (AD-418, AD-422): the process's one
//! [`Hq`], started once the settings layers are installed, and its commands.
//! Every decision — the address grammar, the pairing, which databases sync,
//! every sentence — is core's; this holds it and streams its status.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, OnceLock, PoisonError};

use keeper_core::hq::syncular::SyncularOpener;
use keeper_core::hq::vm::{HqDeviceVm, HqPairCodeVm, HqStatusVm};
use keeper_core::hq::{Hq, HqError};
use keeper_core::vm::{IpcError, IpcErrorCode};
use tauri::async_runtime::JoinHandle;
use tauri::ipc::Channel;
use tauri::Manager;

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

/// Stream the agents server's status: the current one at once, then every
/// change.
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

/// Forget the server: its pairing and its replicas on this device.
#[tauri::command]
pub async fn hq_server_forget() -> Result<HqStatusVm, IpcError> {
    hq()?.forget().await.map_err(error)
}

/// A code (and its QR) another device of this person can redeem.
#[tauri::command]
pub async fn hq_pair_start() -> Result<HqPairCodeVm, IpcError> {
    hq()?.pair_start().await.map_err(error)
}

/// Redeem a code typed, pasted or scanned on this device.
#[tauri::command]
pub async fn hq_pair_redeem(code: String) -> Result<HqStatusVm, IpcError> {
    hq()?.pair_redeem(&code).await.map_err(error)
}

/// The person's devices, from the replicas' directory.
#[tauri::command]
pub async fn hq_devices_list() -> Result<Vec<HqDeviceVm>, IpcError> {
    hq()?.devices().await.map_err(error)
}

/// Remove one of the person's devices from the server; the devices after.
#[tauri::command]
pub async fn hq_device_revoke(client_id: String) -> Result<Vec<HqDeviceVm>, IpcError> {
    hq()?.revoke(&client_id).await.map_err(error)
}
