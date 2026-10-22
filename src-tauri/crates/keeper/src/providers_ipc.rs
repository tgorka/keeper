//! The model providers' driving adapter (Story 61.1; Story 108.3a): every
//! `provider_*` command Settings › Model providers and notes search call.
//!
//! **No decisions live here.** The base-URL grammar, the persistence, the
//! health verdict, the model list and where a credential comes from are all
//! `keeper_core::providers`; this module reads a row, asks the account or the
//! keychain for the token, builds the endpoint and calls.
//!
//! Registered in the **shared** half of the `invoke_handler` literal: a
//! provider is a URL plus a credential behind the keychain, so these commands
//! compile and run on every target, and notes search on a phone reaches its
//! embedding provider the same way.

use std::path::{Path, PathBuf};

use keeper_core::providers::error::BotsError;
use keeper_core::providers::{
    self, discover, http, store, Endpoint, Provider, ProviderHealth,
};
use keeper_core::vm::{
    BotModelVm, IpcError, IpcErrorCode, ModelProviderSaveReq, ModelProviderVm, ProviderProbeVm,
};
use tauri::State;

use crate::ipc::{to_ipc_error, AppState};

/// Fold a provider error into the one envelope the frontend understands: a
/// `401`/`403` is `invalidCredentials`, a transport failure or a silence
/// timeout is `serverUnreachable` and retriable, a quirk-table refusal is
/// `unsupported`, everything else `internal`. Every message has already been
/// through `keeper_core::providers::error`, which is what guarantees it
/// carries no credential and no full URL.
fn provider_error(error: BotsError) -> IpcError {
    let (code, retriable) = match &error {
        BotsError::Unsupported { .. } => (IpcErrorCode::Unsupported, false),
        BotsError::Status { status, .. } if *status == 401 || *status == 403 => {
            (IpcErrorCode::InvalidCredentials, false)
        }
        BotsError::Transport { retryable, .. } => (IpcErrorCode::ServerUnreachable, *retryable),
        BotsError::Timeout { .. } => (IpcErrorCode::ServerUnreachable, true),
        BotsError::Status { .. } => (IpcErrorCode::Internal, error.is_retryable()),
        _ => (IpcErrorCode::Internal, false),
    };
    IpcError {
        code,
        message: error.to_string(),
        account_id: None,
        retriable,
    }
}

/// A refusal in keeper's own words, `internal`.
fn refused(message: impl Into<String>) -> IpcError {
    IpcError {
        code: IpcErrorCode::Internal,
        message: message.into(),
        account_id: None,
        retriable: false,
    }
}

fn data_dir(state: &AppState) -> Result<PathBuf, IpcError> {
    state.platform.data_dir().map_err(to_ipc_error)
}

/// Read one provider row, or refuse: by the time a command runs the id came
/// from a view model keeper produced, so one that names nothing is a row
/// deleted under a stale render.
fn provider_of(dir: &Path, provider_id: &str) -> Result<store::ProviderRow, IpcError> {
    store::get_provider(dir, provider_id)
        .map_err(to_ipc_error)?
        .ok_or_else(|| refused(format!("no such provider: {provider_id}")))
}

/// The endpoint a row is reached at, with the token the account or the
/// keychain gives for it now.
async fn endpoint_of(state: &AppState, row: &store::ProviderRow) -> Result<Endpoint, IpcError> {
    let token = crate::account_ipc::provider_credential(state.platform.as_ref(), &row.provider.id)
        .await
        .map_err(crate::account_ipc::account_ipc_error)?;
    Ok(row.provider.endpoint(None, token))
}

/// The projection of a row for the surface: the keychain or the account says
/// whether a credential is there, never what it is.
fn vm_of(state: &AppState, row: &store::ProviderRow) -> ModelProviderVm {
    ModelProviderVm::compose(
        row,
        crate::account_ipc::provider_has_credential(state.platform.as_ref(), &row.provider.id),
    )
}

/// Every configured provider, in insertion order (FR-369). Rows this build
/// cannot speak to are not returned.
///
/// Rejects with: `internal`.
#[tauri::command]
pub fn providers_list(state: State<'_, AppState>) -> Result<Vec<ModelProviderVm>, IpcError> {
    let dir = data_dir(&state)?;
    let listing = store::list_providers(&dir).map_err(to_ipc_error)?;
    Ok(listing.rows.iter().map(|row| vm_of(&state, row)).collect())
}

/// Add (`req.id` absent) or edit one provider (FR-379). The base URL goes
/// through `keeper_core::providers::parse_base_url` and the normalized form
/// is stored. The credential is written only when `req.token` is `Some`, and
/// deleted only when `req.clear_token` is set: the form cannot render a
/// stored token, so an empty field means *unchanged*. An edit drops the
/// previous health verdict (`store::update_provider` does not carry it); the
/// surface re-probes.
///
/// Rejects with: `internal` (a base URL the grammar refuses, a blank name, or
/// an unknown id).
#[tauri::command]
pub fn provider_save(
    state: State<'_, AppState>,
    req: ModelProviderSaveReq,
) -> Result<ModelProviderVm, IpcError> {
    let dir = data_dir(&state)?;
    let parsed = providers::parse_base_url(&req.base_url)
        .map_err(|err| provider_error(BotsError::BaseUrl(err)))?;
    let name = req.name.trim();
    if name.is_empty() {
        return Err(refused(
            "a provider needs a name you will recognise in the picker",
        ));
    }
    let id = req.id.clone().unwrap_or_else(crate::sync_ipc::new_ulid);
    let provider = Provider {
        id: id.clone(),
        kind: req.kind,
        name: name.to_owned(),
        base_url: parsed.normalized,
        created_ms: chrono::Utc::now().timestamp_millis(),
    };
    match &req.id {
        None => store::insert_provider(&dir, &provider).map_err(to_ipc_error)?,
        Some(existing) => {
            if !store::update_provider(&dir, &provider).map_err(to_ipc_error)? {
                return Err(refused(format!("no such provider: {existing}")));
            }
        }
    }
    if let Some(token) = req.token.as_deref() {
        providers::save_provider_token(state.platform.as_ref(), &id, token).map_err(to_ipc_error)?;
    } else if req.clear_token {
        providers::delete_provider_token(state.platform.as_ref(), &id).map_err(to_ipc_error)?;
    }
    let row = provider_of(&dir, &id)?;
    // The provider travels in the person's providers file (Epic 84, AD-325).
    crate::account_ipc::note_local_change();
    Ok(vm_of(&state, &row))
}

/// Remove one provider and its credential (FR-379): the row first — the
/// thing a surface reads — then the secret, so a crash between them leaves
/// an orphaned secret nothing can reach rather than a provider keeper can no
/// longer authenticate. Idempotent.
///
/// Rejects with: `internal`.
#[tauri::command]
pub fn provider_remove(state: State<'_, AppState>, provider_id: String) -> Result<(), IpcError> {
    let dir = data_dir(&state)?;
    store::delete_provider(&dir, &provider_id).map_err(to_ipc_error)?;
    providers::delete_provider_token(state.platform.as_ref(), &provider_id)
        .map_err(to_ipc_error)?;
    crate::account_ipc::note_local_change();
    Ok(())
}

/// Ask a provider whether it is there and what it is, and store the verdict
/// (FR-375). An endpoint that refused or never answered is a fact about the
/// endpoint, so it comes back as a [`ProviderProbeVm`]; the only errors are
/// keeper's own.
///
/// Rejects with: `internal`, `invalidCredentials` (the account could not give
/// a token).
#[tauri::command]
pub async fn provider_probe(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<ProviderProbeVm, IpcError> {
    let dir = data_dir(&state)?;
    let row = provider_of(&dir, &provider_id)?;
    let endpoint = endpoint_of(&state, &row).await?;
    let client = http::client(row.read_timeout()).map_err(provider_error)?;
    let probe = discover::health(&client, &endpoint).await;
    let health = ProviderHealth {
        state: discover::health_state(&probe),
        checked_ms: Some(chrono::Utc::now().timestamp_millis()),
        detail: probe.reason.clone(),
    };
    store::set_provider_health(&dir, &provider_id, &health).map_err(to_ipc_error)?;
    Ok(probe)
}

/// Every model this provider lists (FR-377) — what the embedding picker
/// chooses from. Capability flags are `keeper-core`'s tri-state: `null` is
/// unknown and never `false`.
///
/// Rejects with: `internal`, `invalidCredentials`, `serverUnreachable`,
/// `unsupported`.
#[tauri::command]
pub async fn provider_models(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<Vec<BotModelVm>, IpcError> {
    let dir = data_dir(&state)?;
    let row = provider_of(&dir, &provider_id)?;
    let endpoint = endpoint_of(&state, &row).await?;
    let client = http::client(row.read_timeout()).map_err(provider_error)?;
    discover::models(&client, &endpoint)
        .await
        .map_err(provider_error)
}
