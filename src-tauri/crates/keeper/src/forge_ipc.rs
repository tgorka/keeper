//! Repository sources: the person's GitHub and Forgejo repositories, listed
//! and added as drives (Epic 86, AD-333–AD-338).
//!
//! `keeper-core::forges` decides — which sources exist, how a token is got
//! (the broker, the account's forge, a device-flow connection), how a listing
//! is fetched and parsed, and how each repository is marked against the
//! drives here and on the person's other devices. This module is where those
//! meet the keychain, the browser, `sync.db` and the disk.
//!
//! # No timers
//!
//! A device-flow connection polls inside [`forge_connect_wait`], a call the
//! person started by pressing Connect; [`forge_connect_cancel`] stops it
//! between two sleeps. Nothing opens a browser without a click: the code is
//! held here until [`forge_connect_open`] is asked for it.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError};

use keeper_core::forges::device_flow::{self, DeviceCode};
use keeper_core::forges::listing::{self, Listing};
use keeper_core::forges::tokens::{self, ForgeError};
use keeper_core::forges::vm::{
    source_vm, DeviceCodeVm, ForgeAddItem, ForgeAddReq, ForgeAddResultVm, ForgeReposVm,
    ForgeSourceVm,
};
use keeper_core::forges::{self, mark, ForgeSource};
use keeper_core::org_account::descriptor::AccountDescriptor;
use keeper_core::org_account::settings_sync::LocalDrive;
use keeper_core::registry;
use keeper_core::vm::{IpcError, IpcErrorCode};
use keeper_sync::SyncProfile;
use tauri::{AppHandle, State};

use crate::ipc::{to_ipc_error, AppState};
use crate::sync_ipc::{self, SyncProfileReq};
use crate::{account_ipc, account_settings};

/// A device-flow connection waiting for the person to approve it: the code
/// (whose device half never leaves this process) and the flag that stops
/// its poll.
struct Pending {
    code: Arc<DeviceCode>,
    cancel: Arc<AtomicBool>,
}

/// One waiting connection per source; a second Connect replaces the first
/// and stops its poll.
static PENDING: LazyLock<Mutex<HashMap<String, Pending>>> = LazyLock::new(Default::default);

/// What the last listing or connection of a source ran into, so the source
/// list says it (unreachable, needs sign-in, no GitHub access) until one
/// succeeds. Memory only.
static LAST_ERROR: LazyLock<Mutex<HashMap<String, ForgeError>>> = LazyLock::new(Default::default);

/// One batch add at a time: each reads the drives before it adds, so two
/// at once would each miss the other's folders.
static ADDING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The IPC envelope for a forge refusal; the sentence is the core's own.
fn forge_ipc_error(error: &ForgeError) -> IpcError {
    let (code, retriable) = match error {
        ForgeError::NeedsConnect | ForgeError::NeedsSignIn(_) => (IpcErrorCode::OauthFailed, true),
        ForgeError::Unreachable(_) | ForgeError::NoAccess(_) => {
            (IpcErrorCode::ServerUnreachable, true)
        }
        ForgeError::Refused(_) | ForgeError::Internal(_) => (IpcErrorCode::Internal, false),
    };
    IpcError {
        code,
        message: error.to_string(),
        account_id: None,
        retriable,
    }
}

fn refusal(sentence: impl Into<String>) -> IpcError {
    forge_ipc_error(&ForgeError::Refused(sentence.into()))
}

fn remember(source_id: &str, error: Option<&ForgeError>) {
    let mut last = lock(&LAST_ERROR);
    match error {
        Some(error) => {
            last.insert(source_id.to_owned(), error.clone());
        }
        None => {
            last.remove(source_id);
        }
    }
}

/// Drop everything learnt under the person signed in until now — core's
/// listings and broker answers, what each source last ran into, and any
/// connection still waiting for approval — so whoever signs in next starts
/// from the servers rather than from someone else's repositories.
pub(crate) fn forget_identity() {
    forges::forget_identity();
    lock(&LAST_ERROR).clear();
    for (_, pending) in lock(&PENDING).drain() {
        pending.cancel.store(true, Ordering::SeqCst);
    }
}

fn http() -> Result<&'static reqwest::Client, IpcError> {
    account_ipc::http().map_err(|sentence| forge_ipc_error(&ForgeError::Internal(sentence)))
}

/// Source `source_id` as this device has it now, with the account it may
/// lean on. Re-read on every call, so an edited `account.toml` is followed.
fn source_of(source_id: &str) -> Result<(ForgeSource, Option<AccountDescriptor>), IpcError> {
    let d = account_ipc::descriptor();
    let sources = forges::sources(d.as_ref(), forges::BUILTIN_GITHUB_CLIENT_ID);
    let source = forges::find(&sources, source_id).cloned().ok_or_else(|| {
        refusal(format!(
            "keeper has no repository source \"{source_id}\" on this device."
        ))
    })?;
    Ok((source, d))
}

/// This device's drives, recorded for the credential path as every listing
/// of them is.
fn profiles(state: &AppState) -> Result<Vec<SyncProfile>, IpcError> {
    let profiles = sync_ipc::engine_of(state)?
        .list_profiles()
        .map_err(|error| sync_ipc::sync_ipc_error(&error))?;
    account_ipc::note_drives(&profiles);
    Ok(profiles)
}

fn local_drives(profiles: &[SyncProfile]) -> Vec<LocalDrive> {
    profiles.iter().map(account_settings::local_drive).collect()
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Every repository source this device can use, with its state. Empty
/// without an account and without keeper's own GitHub client id, which is
/// what keeps every entry point absent then.
#[tauri::command]
pub async fn forges_list(state: State<'_, AppState>) -> Result<Vec<ForgeSourceVm>, IpcError> {
    let d = account_ipc::descriptor();
    let platform = state.platform.as_ref();
    // A copy: building a source's state reads the keychain, which may wait
    // on a prompt, and nothing else may wait on this lock meanwhile.
    let last = lock(&LAST_ERROR).clone();
    Ok(
        forges::sources(d.as_ref(), forges::BUILTIN_GITHUB_CLIENT_ID)
            .iter()
            .map(|source| source_vm(platform, source, d.as_ref(), last.get(&source.id)))
            .collect(),
    )
}

/// A source's repositories, marked against this device's drives and the
/// person's other devices. From the in-memory copy unless `refresh`.
#[tauri::command]
pub async fn forge_repos(
    state: State<'_, AppState>,
    source_id: String,
    refresh: bool,
) -> Result<ForgeReposVm, IpcError> {
    let (source, d) = source_of(&source_id)?;
    let http = http()?;
    let listing =
        match listing::list(state.platform.as_ref(), http, &source, d.as_ref(), refresh).await {
            Ok(listing) => {
                remember(&source.id, None);
                listing
            }
            Err(error) => {
                remember(&source.id, Some(&error));
                return Err(forge_ipc_error(&error));
            }
        };
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    let local = local_drives(&profiles(&state)?);
    let (records, this_device) = account_ipc::drive_records(&data_dir);
    Ok(mark::repos_vm(
        &source.id,
        &listing,
        &local,
        &records,
        &this_device,
    ))
}

/// Start a device-flow connection: the code to show, held here with the
/// flag that stops its poll. Opens nothing.
#[tauri::command]
pub async fn forge_connect_start(source_id: String) -> Result<DeviceCodeVm, IpcError> {
    let (source, _) = source_of(&source_id)?;
    let code = device_flow::start(http()?, &source)
        .await
        .map_err(|error| forge_ipc_error(&error))?;
    let vm = code.vm();
    let pending = Pending {
        code: Arc::new(code),
        cancel: Arc::new(AtomicBool::new(false)),
    };
    if let Some(replaced) = lock(&PENDING).insert(source.id, pending) {
        replaced.cancel.store(true, Ordering::SeqCst);
    }
    Ok(vm)
}

/// Open the waiting code's verification page — the person pressed the
/// button, after the code was on screen to copy.
#[tauri::command]
pub fn forge_connect_open(state: State<'_, AppState>, source_id: String) -> Result<(), IpcError> {
    let uri = lock(&PENDING)
        .get(&source_id)
        .map(|pending| pending.code.verification_uri.clone())
        .ok_or_else(|| refusal("Nothing is waiting for your approval; press Connect first."))?;
    state.platform.open_url(&uri).map_err(to_ipc_error)
}

/// Wait for the person to approve the waiting code, then keep the
/// connection in the keychain. Answers the source as it now is: connected,
/// `notConnected` after a cancel, or the sentence of what ended it.
#[tauri::command]
pub async fn forge_connect_wait(
    state: State<'_, AppState>,
    source_id: String,
) -> Result<ForgeSourceVm, IpcError> {
    let (source, d) = source_of(&source_id)?;
    let (code, cancel) = {
        let pending = lock(&PENDING);
        let waiting = pending
            .get(&source.id)
            .ok_or_else(|| refusal("Nothing is waiting for your approval; press Connect first."))?;
        (Arc::clone(&waiting.code), Arc::clone(&waiting.cancel))
    };
    let http = http()?;
    let outcome = device_flow::poll(http, &source, &code, &cancel).await;
    {
        // This wait's code is spent either way; a newer Connect keeps its own.
        let mut pending = lock(&PENDING);
        if pending
            .get(&source.id)
            .is_some_and(|waiting| Arc::ptr_eq(&waiting.cancel, &cancel))
        {
            pending.remove(&source.id);
        }
    }
    let platform = state.platform.as_ref();
    let error = match outcome {
        Ok(token) => tokens::store_session(platform, &source, &token).err(),
        Err(error) => Some(error),
    };
    remember(&source.id, error.as_ref());
    Ok(source_vm(platform, &source, d.as_ref(), error.as_ref()))
}

/// Stop waiting for approval; the pending wait answers `notConnected`.
#[tauri::command]
pub fn forge_connect_cancel(source_id: String) -> Result<(), IpcError> {
    if let Some(pending) = lock(&PENDING).remove(&source_id) {
        pending.cancel.store(true, Ordering::SeqCst);
    }
    Ok(())
}

/// Forget this device's connection to a source. The drives that use it ask
/// for it again on their next sync.
#[tauri::command]
pub async fn forge_disconnect(
    state: State<'_, AppState>,
    source_id: String,
) -> Result<ForgeSourceVm, IpcError> {
    forge_connect_cancel(source_id.clone())?;
    let (source, d) = source_of(&source_id)?;
    let platform = state.platform.as_ref();
    tokens::forge_disconnect(platform, &source).map_err(|error| forge_ipc_error(&error))?;
    remember(&source.id, None);
    Ok(source_vm(platform, &source, d.as_ref(), None))
}

/// Where a batch of drives goes unless the person says otherwise: beside
/// the most recently added drive, else `~/Drives`. `None` on a phone, whose
/// drives always live in the app's container.
#[tauri::command]
pub async fn forge_default_base_folder(
    state: State<'_, AppState>,
) -> Result<Option<String>, IpcError> {
    #[cfg(desktop)]
    {
        let profiles = profiles(&state)?;
        Ok(default_base_folder(&profiles).map(|path| path.to_string_lossy().into_owned()))
    }
    #[cfg(not(desktop))]
    {
        let _ = state;
        Ok(None)
    }
}

/// The parent of the newest drive's folder (ids are ULIDs, so the largest
/// is the newest) — a removable one's volume is no default — else
/// `~/Drives`.
#[cfg(desktop)]
fn default_base_folder(profiles: &[SyncProfile]) -> Option<PathBuf> {
    profiles
        .iter()
        .filter(|profile| !profile.removable)
        .max_by(|a, b| a.id.cmp(&b.id))
        .and_then(|profile| profile.local_path.parent().map(Path::to_path_buf))
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Drives")))
}

/// Add the chosen repositories as drives, each through the same save as
/// the add-folder form, signing in with the source's own connection. Each
/// repository answers for itself: some may be added while others say why
/// not.
#[tauri::command]
pub async fn forge_repos_add(
    app: AppHandle,
    state: State<'_, AppState>,
    req: ForgeAddReq,
) -> Result<Vec<ForgeAddResultVm>, IpcError> {
    let _adding = ADDING.lock().await;
    let (source, d) = source_of(&req.source_id)?;
    let listing = listing::list(state.platform.as_ref(), http()?, &source, d.as_ref(), false)
        .await
        .map_err(|error| forge_ipc_error(&error))?;
    let data_dir = state.platform.data_dir().map_err(to_ipc_error)?;
    let all = profiles(&state)?;
    let mut batch = Batch {
        credential: forges::credential_for(&source),
        account: account_ipc::account_id(),
        local: local_drives(&all),
        #[cfg(desktop)]
        base: match req.base_folder.as_deref().map(str::trim) {
            Some(base) if !base.is_empty() => Some(full_path(base)),
            _ => default_base_folder(&all).map(Ok),
        },
        #[cfg(desktop)]
        folders: all
            .iter()
            .map(|profile| (resolved(&profile.local_path), profile.name.clone()))
            .collect(),
        #[cfg(desktop)]
        taken: Vec::new(),
    };
    Ok(req
        .repos
        .iter()
        .map(|item| {
            let outcome = add_one(&app, &state, &data_dir, &listing, item, &mut batch);
            let (profile_id, sentence) = match outcome {
                Ok(id) => (Some(id), None),
                Err(sentence) => (None, Some(sentence)),
            };
            ForgeAddResultVm {
                full_name: item.full_name.clone(),
                profile_id,
                sentence,
            }
        })
        .collect())
}

/// What one batch shares across its repositories.
struct Batch {
    /// `account` for the account's forge, else `forge:<source id>`.
    credential: String,
    account: Option<String>,
    /// This device's drives, growing as the batch adds them.
    local: Vec<LocalDrive>,
    /// Where the batch's drives go unless a row names its own folder, or
    /// why the base the person typed can't be used.
    #[cfg(desktop)]
    base: Option<Result<PathBuf, String>>,
    /// Every drive's folder here, as [`resolved`], with the drive's name.
    #[cfg(desktop)]
    folders: Vec<(PathBuf, String)>,
    /// The folders this batch has given out already, as [`resolved`].
    #[cfg(desktop)]
    taken: Vec<PathBuf>,
}

/// Add one repository: its folder, its credential choice, then the profile
/// through `sync_profile_save`'s own path. The new drive's id, or the
/// sentence saying why not.
fn add_one(
    app: &AppHandle,
    state: &AppState,
    data_dir: &Path,
    listing: &Listing,
    item: &ForgeAddItem,
    batch: &mut Batch,
) -> Result<String, String> {
    let Some(repo) = listing.repos.iter().find(|r| r.full_name == item.full_name) else {
        return Err(format!(
            "{} is no longer in the list; refresh it and try again.",
            item.full_name
        ));
    };
    // A second drive of a repository synced here already is the single
    // form's to make, where the person picks its folder on purpose.
    let (added_as, _) = mark::marks(&repo.clone_url, &batch.local, &[], "");
    if let Some(drive) = added_as.first() {
        return Err(format!("Already syncing here as {drive}."));
    }
    let name = item.drive_name.trim();
    let mut parts = Path::new(name).components();
    if !matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(_)), None)
    ) {
        return Err(format!("\"{name}\" can't be a drive's folder name."));
    }
    let (local_path, created) = folder_for(item, name, &repo.clone_url, batch)?;
    let id = sync_ipc::new_ulid();
    // The choice goes in before the drive: the engine may fetch the moment
    // the profile lands, and must not try the keychain for it first.
    if let Err(error) = registry::set_sync_credential_source(
        data_dir,
        &id,
        Some(batch.credential.as_str()),
        batch.account.as_deref(),
    ) {
        undo_folders(&created);
        return Err(error.to_string());
    }
    let branch = if repo.default_branch.is_empty() {
        "main".to_owned()
    } else {
        repo.default_branch.clone()
    };
    let req = SyncProfileReq {
        id: Some(id.clone()),
        name: name.to_owned(),
        local_path,
        remote_url: repo.clone_url.clone(),
        branch,
        // A repository this connection can't push to is only downloaded;
        // a two-way drive would pile up local edits it can never send.
        direction: if repo.pull_only() {
            "pullOnly"
        } else {
            "bidirectional"
        }
        .to_owned(),
        lane: "main".to_owned(),
        subpaths: Vec::new(),
        excludes: Vec::new(),
        removable: false,
        lfs_mode: "materialize".to_owned(),
        lfs_threshold_bytes: None,
        virtual_patterns: None,
        virtual_over_bytes: None,
        release_ttl_ms: None,
        settle_ms: None,
        poll_interval_ms: None,
        tags: Vec::new(),
        author_override: None,
        commit_subject_template: None,
        notes: None,
        notes_subfolder: None,
        recordings: None,
        recordings_subfolder: None,
        sessions: None,
        sessions_subfolder: None,
        tasks: None,
        tasks_subfolder: None,
    };
    match sync_ipc::save_profile(app, state, req) {
        Ok(profile) => {
            batch.local.push(account_settings::local_drive(&profile));
            Ok(profile.id)
        }
        Err(error) => {
            if let Err(undo) = registry::set_sync_credential_source(data_dir, &id, None, None) {
                tracing::warn!(%undo, "forges: a refused drive's credential choice stayed behind");
            }
            undo_folders(&created);
            Err(error.message)
        }
    }
}

/// The folder a batch drive syncs, and the folders this call made for it,
/// deepest first, to remove again if the drive is refused. It is `folder`,
/// or `<base>/<name>`; it must be no drive's folder, neither inside one nor
/// holding one, and absent, empty or already this repository's clone.
#[cfg(desktop)]
fn folder_for(
    item: &ForgeAddItem,
    name: &str,
    clone_url: &str,
    batch: &mut Batch,
) -> Result<(String, Vec<PathBuf>), String> {
    let folder = match (item.folder.as_deref().map(str::trim), &batch.base) {
        (Some(folder), _) if !folder.is_empty() => full_path(folder)?,
        (_, Some(Ok(base))) => base.join(name),
        (_, Some(Err(why))) => return Err(why.clone()),
        (_, None) => return Err("Choose where these drives should go.".to_owned()),
    };
    let at = folder.display();
    let key = resolved(&folder);
    let overlaps = |other: &PathBuf| key.starts_with(other) || other.starts_with(&key);
    if let Some((drive, drive_name)) = batch.folders.iter().find(|(drive, _)| overlaps(drive)) {
        return Err(if *drive == key {
            format!("{at} is already the folder of the drive {drive_name}.")
        } else if key.starts_with(drive) {
            format!("{at} is inside the drive {drive_name}; choose a folder outside it.")
        } else {
            format!("{at} holds the drive {drive_name}; choose a folder that holds no drive.")
        });
    }
    if let Some(other) = batch.taken.iter().find(|&other| overlaps(other)) {
        return Err(if *other == key {
            format!("{at} is the folder of another repository in this batch.")
        } else {
            format!("{at} overlaps the folder of another repository in this batch.")
        });
    }
    batch.taken.push(key);
    if folder.exists() {
        if folder.is_dir() && crate::account_restore::folder_accepts(&folder, clone_url) {
            return Ok((folder.to_string_lossy().into_owned(), Vec::new()));
        }
        return Err(format!("{at} holds other files."));
    }
    let created: Vec<PathBuf> = folder
        .ancestors()
        .take_while(|ancestor| !ancestor.exists())
        .map(Path::to_path_buf)
        .collect();
    if let Err(error) = std::fs::create_dir_all(&folder) {
        undo_folders(&created);
        return Err(format!("keeper could not make {at}: {error}"));
    }
    Ok((folder.to_string_lossy().into_owned(), created))
}

/// A folder the person typed, as a full path: a leading `~/` is their home
/// folder, and anything else must already be absolute — a relative one
/// would land wherever keeper happened to be started.
#[cfg(desktop)]
fn full_path(text: &str) -> Result<PathBuf, String> {
    const REFUSED: &str = "Choose a full folder path.";
    let text = text.trim();
    let path = match text.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => {
            let home = std::env::var_os("HOME").ok_or_else(|| REFUSED.to_owned())?;
            PathBuf::from(home).join(rest.trim_start_matches('/'))
        }
        _ => PathBuf::from(text),
    };
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(REFUSED.to_owned())
    }
}

/// `path` as the disk resolves it — symlinks followed through its deepest
/// existing ancestor — and case-folded where the disk ignores case (macOS),
/// so two spellings of one folder compare equal.
#[cfg(desktop)]
fn resolved(path: &Path) -> PathBuf {
    let mut missing = Vec::new();
    let mut at = path;
    let found = loop {
        if let Ok(real) = std::fs::canonicalize(at) {
            break real;
        }
        match (at.parent(), at.file_name()) {
            (Some(parent), Some(name)) => {
                missing.push(name);
                at = parent;
            }
            _ => break at.to_path_buf(),
        }
    };
    let full = missing
        .into_iter()
        .rev()
        .fold(found, |full, name| full.join(name));
    if cfg!(target_os = "macos") {
        PathBuf::from(full.to_string_lossy().to_lowercase())
    } else {
        full
    }
}

/// On a phone the folder is the app container's, made by the save itself
/// (`phone_shaped_request`), as for any drive added there.
#[cfg(not(desktop))]
fn folder_for(
    _item: &ForgeAddItem,
    _name: &str,
    _clone_url: &str,
    _batch: &mut Batch,
) -> Result<(String, Vec<PathBuf>), String> {
    Ok((String::new(), Vec::new()))
}

/// Remove the folders this batch made for a drive that was then refused,
/// deepest first, each only while it is still empty.
fn undo_folders(created: &[PathBuf]) {
    for folder in created {
        match std::fs::remove_dir(folder) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                tracing::warn!(%error, path = %folder.display(), "forges: a refused drive's folder stayed behind");
                break;
            }
        }
    }
}
