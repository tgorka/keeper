//! Shell call sites only. All consent, destinations, schema, redaction and
//! export policy live in keeper-core; the shell's one subscriber carries the
//! WARN/ERROR export layer (`debug_log`, R461), attached here.
use std::sync::Arc;

use keeper_core::telemetry::{
    AccountConsentKnown, PublicConfig, Telemetry, TelemetryConsentVm, TelemetryError,
    TelemetryEventReq, TelemetryRemoteConfigVm, TelemetryStatusVm, TelemetryStudyConfigVm,
    TelemetryStudyPreviewVm,
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
    // Every account-layer install or clear — a pull that changed the
    // account's `keeper.toml`, sign-out, forget, a refused sign-in — may
    // change effective consent with no settings row moving (R496). Before
    // anything installs a layer, so no transition is missed.
    let handle = app.handle().clone();
    keeper_core::config::set_account_layers_observer(Box::new(move || {
        consent_changed(&handle, "an account settings file changed");
    }));
    tauri::async_runtime::spawn(telemetry.run());
}

/// Once the settings layers are installed and the subscriber exists: re-read
/// the account's consent through them (R462) and attach the WARN/ERROR
/// export layer (R461). With no account on this installation, its settings
/// are already all there is, so an epic-71 "on" may be adopted now; with an
/// account, that waits for a settled settings sync ([`account_settled`],
/// R495 07).
pub fn start_problem_export(app: &tauri::App) {
    let telemetry = Arc::clone(app.state::<Arc<Telemetry>>().inner());
    let read = if crate::account_ipc::descriptor().is_none() {
        telemetry.account_settled(AccountConsentKnown::NO_ACCOUNT)
    } else {
        telemetry.reload_consent()
    };
    if let Err(error) = read {
        tracing::warn!(%error, "telemetry: the account's consent could not be read; collection is off");
    }
    crate::debug_log::attach_telemetry(&telemetry);
}

/// Settings sync applied one of the two consent keys (R462): make it
/// effective here and tell every window.
pub fn consent_pulled(app: &tauri::AppHandle) {
    consent_changed(app, "the synced consent did not take effect");
}

/// A settings sync with the account settled: `known` is what it established
/// about each consent key, so an epic-71 "on" is adopted only where the
/// account provably has none (R495 07, R511), and every window hears the
/// result.
pub fn account_settled(app: &tauri::AppHandle, known: AccountConsentKnown) {
    let telemetry = app.state::<Arc<Telemetry>>();
    match telemetry.account_settled(known) {
        Ok(status) => {
            let _ = app.emit("telemetry-status-changed", status);
        }
        Err(error) => {
            tracing::warn!(%error, "telemetry: an earlier installation's consent was not adopted; it is tried again after the next sync");
        }
    }
}

/// Re-read the effective consent and tell every window (R496).
fn consent_changed(app: &tauri::AppHandle, failure: &'static str) {
    let telemetry = app.state::<Arc<Telemetry>>();
    match telemetry.reload_consent() {
        Ok(status) => {
            let _ = app.emit("telemetry-status-changed", status);
        }
        Err(error) => {
            tracing::warn!(%error, failure, "telemetry: consent could not be re-read; collection is off");
        }
    }
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
