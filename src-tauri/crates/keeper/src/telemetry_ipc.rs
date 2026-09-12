//! Shell call sites only. All consent, destinations, schema and export policy live
//! in keeper-core; this does not attach to the local debug-log subscriber.
use std::sync::Arc;

use keeper_core::telemetry::{
    PublicConfig, Telemetry, TelemetryConsentVm, TelemetryError, TelemetryEventReq,
    TelemetryRemoteConfigVm, TelemetryStatusVm, TelemetryStudyConfigVm, TelemetryStudyPreviewVm,
};
use keeper_core::vm::{IpcError, IpcErrorCode};
use tauri::{Emitter, Manager, State};

pub fn init(app: &tauri::App) {
    // Build-time public values only. No process-env lookup or secret provider.
    let config = option_env!("KEEPER_POSTHOG_HOST")
        .zip(option_env!("KEEPER_POSTHOG_PROJECT_TOKEN"))
        .and_then(|(host, token)| PublicConfig::parse(host, token));
    let data_dir = app.state::<crate::ipc::AppState>().platform.data_dir().ok();
    let telemetry = Telemetry::open(data_dir.as_deref(), config);
    app.manage(Arc::clone(&telemetry));
    tauri::async_runtime::spawn(telemetry.run());
}

fn error(error: TelemetryError) -> IpcError {
    IpcError {
        code: IpcErrorCode::Internal,
        message: error.to_string(),
        account_id: None,
        retriable: false,
    }
}

#[tauri::command]
pub fn telemetry_status(state: State<'_, Arc<Telemetry>>) -> Result<TelemetryStatusVm, IpcError> {
    state.status().map_err(error)
}

#[tauri::command]
pub async fn telemetry_consent_set(
    app: tauri::AppHandle,
    state: State<'_, Arc<Telemetry>>,
    consent: TelemetryConsentVm,
) -> Result<TelemetryStatusVm, IpcError> {
    let telemetry = Arc::clone(state.inner());
    let worker = Arc::clone(&telemetry);
    let result = tauri::async_runtime::spawn_blocking(move || worker.set_consent(consent))
        .await
        .map_err(|_| error(TelemetryError::Unavailable))?;
    // Notify all windows even when persistence failed and effective state is off.
    if let Ok(status) = telemetry.status() {
        let _ = app.emit("telemetry-status-changed", status);
    }
    result.map_err(error)
}

#[tauri::command]
pub fn telemetry_capture(
    state: State<'_, Arc<Telemetry>>,
    event: TelemetryEventReq,
) -> Result<(), IpcError> {
    state.capture(event).map_err(error)
}

#[tauri::command]
pub async fn telemetry_remote_config(
    state: State<'_, Arc<Telemetry>>,
) -> Result<TelemetryRemoteConfigVm, IpcError> {
    Ok(state.remote_config().await)
}

/// Called only by the explicit synthetic-study start action. It does not enable
/// ordinary telemetry and returns no persistent installation identifier.
#[tauri::command]
pub fn telemetry_study_config(
    state: State<'_, Arc<Telemetry>>,
) -> Result<Option<TelemetryStudyConfigVm>, IpcError> {
    state.study_config().map_err(error)
}

#[tauri::command]
pub fn telemetry_study_stop(state: State<'_, Arc<Telemetry>>) -> Result<(), IpcError> {
    state.study_stop().map_err(error)
}

#[tauri::command]
pub fn telemetry_study_preview(
    state: State<'_, Arc<Telemetry>>,
) -> Result<TelemetryStudyPreviewVm, IpcError> {
    Ok(state.study_preview())
}
