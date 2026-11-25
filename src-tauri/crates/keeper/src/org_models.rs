//! The organisation's model set on this device (story 121.3): which version
//! the general domain names, whether a group of it is complete here, and the
//! fetch that brings one. Every decision — the rows, the version check, the
//! completeness rule, the download through the blob port — is
//! `keeper_settings::models`'s and keeper-replica's; this composes them over
//! the agents server's general domain. Outside the scanned `transcribe*.rs`
//! and `voice*.rs`: those call only the completeness check.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use keeper_core::hq::GeneralSource;
use keeper_core::transcription::models as model_files;
use keeper_replica::transfer::{BlobRouteGrants, StoreTransfer, TransferConfig};
use keeper_settings::models::{self as set_models, ActiveSet, Group, HydrateReport};

/// Why a group could not be brought here.
pub enum ModelsFetchError {
    /// No agents server paired, or no general domain held: no models.
    NoServer,
    /// The sentence to show.
    Failed(String),
}

/// Stops the fetch in flight, between files.
static INTERRUPT: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);

fn source() -> Option<GeneralSource> {
    crate::hq_ipc::service()?.general_source()
}

/// Whether this device has a server whose general domain could name models.
pub fn has_server() -> bool {
    source().is_some()
}

/// The version the general domain names now, complete, read from the
/// replica alone. `None` with no server, no active version, or one refused
/// (logged).
pub fn active() -> Option<ActiveSet> {
    let source = source()?;
    match set_models::active_set(source.replica.as_ref()) {
        Ok(set) => set,
        Err(error) => {
            tracing::warn!(%error, "models: the organisation's model set is refused");
            None
        }
    }
}

/// The active set's file paths, for the groups core derives; none without one.
pub fn set_paths(set: Option<&ActiveSet>) -> Vec<String> {
    set.map(|set| set.paths().map(str::to_owned).collect())
        .unwrap_or_default()
}

/// The model set the active version's `models.toml` names; the default set
/// without one, or when it does not parse (the hydrated copy's refusal is
/// what the person is shown).
pub fn repo_set(set: Option<&ActiveSet>) -> model_files::ModelSet {
    set.and_then(
        |set| match model_files::ModelSet::from_toml(&set.models_toml) {
            Ok(parsed) => Some(parsed),
            Err(error) => {
                tracing::warn!(%error, "models: models.toml does not parse; using the default set");
                None
            }
        },
    )
    .unwrap_or_default()
}

/// keeper-settings' view of a group core decided on, reading `models.toml`
/// as core says the group reads it.
pub fn group_of(group: &model_files::FetchGroup) -> Group<'_> {
    Group {
        name: group.name,
        folders: match &group.folders {
            model_files::Folders::Only(names) => set_models::Folders::Only(names),
            model_files::Folders::AllBut(names) => set_models::Folders::AllBut(names),
        },
        fingerprint: Some(group.manifest_fingerprint()),
    }
}

/// Whether `dest` holds exactly what the active version names for `group`.
/// No server, or no active version, is never current.
pub fn models_current(
    set: Option<&ActiveSet>,
    dest: &Path,
    group: &model_files::FetchGroup,
) -> bool {
    set.is_some_and(|set| set_models::is_current(set, group_of(group), dest))
}

/// Bring `group` of the active version into `dest`, off the async workers:
/// each file through the blob port's download from the store the server
/// named. Only the agents server and that store are reached.
pub async fn hydrate_models(
    dest: PathBuf,
    group: &model_files::FetchGroup,
) -> Result<HydrateReport, ModelsFetchError> {
    let Some(source) = source() else {
        return Err(ModelsFetchError::NoServer);
    };
    let interrupt = Arc::new(AtomicBool::new(false));
    *INTERRUPT.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::clone(&interrupt));
    let group = group.clone();
    let ran = tokio::task::spawn_blocking(move || -> Result<HydrateReport, String> {
        let set = set_models::active_set(source.replica.as_ref())
            .map_err(|error| error.to_string())?
            .ok_or_else(|| NO_VERSION.to_owned())?;
        let grants = BlobRouteGrants::new(
            &source.base_url,
            &source.token,
            std::time::Duration::from_secs(30),
        )
        .map_err(|error| error.to_string())?;
        let transfer = StoreTransfer::new(
            Arc::new(grants),
            &source.base_url,
            source.store.clone(),
            TransferConfig::contract(dest.join(".keeper-hydrate-plans")),
        )
        .map_err(|error| error.to_string())?;
        set_models::hydrate(&set, group_of(&group), &transfer, &dest, &interrupt)
            .map_err(|error| error.to_string())
    })
    .await;
    match ran {
        Ok(Ok(report)) => Ok(report),
        Ok(Err(sentence)) => Err(ModelsFetchError::Failed(sentence)),
        Err(error) => Err(ModelsFetchError::Failed(format!(
            "the models task ended unexpectedly: {error}"
        ))),
    }
}

/// Stop the fetch in flight, if any, before its next file.
pub fn interrupt() {
    if let Some(flag) = INTERRUPT
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
    {
        flag.store(true, Ordering::SeqCst);
    }
}

/// The sentence for a server whose general domain names no version yet.
pub const NO_VERSION: &str =
    "Your organisation has not published a model set on its agents server yet.";
