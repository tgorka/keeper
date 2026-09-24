//! This device's drives as `device.<slug>.toml` carries them, and back —
//! and the restore that brings this device back from that file (Epic 85,
//! AD-328, AD-329).
//!
//! keeper-core treats a drive's full profile as an opaque table (AD-40): it
//! cannot see `sync.db`. This module is the one place a `SyncProfile` and its
//! schedules become that table and a table becomes a profile again — the whole
//! profile but the two facts no other install may reuse, its `id` and the
//! `volume_id` of the disk it was bound to.
//!
//! The file is written for people too, so its keys are snake_case where the
//! profile's stored JSON is camelCase; both directions rename keys only,
//! never values.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use keeper_core::bots::{self, store, Bot, BotIdentity, Provider, ProviderKind};
use keeper_core::error::CoreError;
use keeper_core::org_account::device_state::{
    drive_key, DeviceStateFile, DriveKey, GrantState, PendingGrant, ProviderState, RestorePending,
    RestorePlan,
};
use keeper_core::org_account::manifest::BotRecord;
use keeper_core::org_account::settings_sync::{Catalog, DriveRef};
use keeper_core::platform::Platform;
use keeper_core::registry;
use keeper_sync::db::TaskRow;
use keeper_sync::tasks::{TaskKind, TaskMissedPolicy, TaskMode, COPY_LOOKBACK_DEFAULT_MS};
use keeper_sync::{Engine, SyncProfile};
use serde_json::{Map, Value};
use tauri::AppHandle;

use crate::account_settings;

/// The table key the drive's schedules live under.
const SCHEDULES: &str = "schedules";

/// The profile fields another install must not reuse, and the git identity
/// override, which names a person and never travels (F6).
const LOCAL_ONLY: [&str; 3] = ["id", "volumeId", "authorOverride"];

/// The task kinds a drive's schedules carry: the ones whose whole meaning is
/// the folder and the schedule. A bot task names a bot id and a copy task
/// absolute paths on this disk, neither of which means anything elsewhere.
fn travels(kind: TaskKind) -> bool {
    matches!(
        kind,
        TaskKind::Sync | TaskKind::Release | TaskKind::Verify | TaskKind::Gc
    )
}

/// `lfsThresholdBytes` → `lfs_threshold_bytes`.
fn snake(key: &str) -> String {
    let mut out = String::with_capacity(key.len() + 4);
    for c in key.chars() {
        if c.is_ascii_uppercase() {
            out.push('_');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// `lfs_threshold_bytes` → `lfsThresholdBytes`.
fn camel(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    let mut upper = false;
    for c in key.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.push(c.to_ascii_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// Every object key renamed by `rename`, and every `null` dropped — TOML has
/// no null, and an absent optional field reads back as `None`.
fn rekey(value: Value, rename: fn(&str) -> String) -> Option<Value> {
    match value {
        Value::Null => None,
        Value::Array(items) => Some(Value::Array(
            items.into_iter().filter_map(|v| rekey(v, rename)).collect(),
        )),
        Value::Object(map) => Some(Value::Object(
            map.into_iter()
                .filter_map(|(k, v)| rekey(v, rename).map(|v| (rename(&k), v)))
                .collect::<Map<String, Value>>(),
        )),
        other => Some(other),
    }
}

/// The schedules of `profile_id` that travel, in the order the store lists
/// them.
pub(crate) fn schedules_of<'a>(profile_id: &str, tasks: &'a [TaskRow]) -> Vec<&'a TaskRow> {
    tasks
        .iter()
        .filter(|task| task.profile_id.as_deref() == Some(profile_id) && travels(task.kind))
        .collect()
}

/// One drive as this device's file carries it: the whole profile but its
/// `id`, `volume_id` and `author_override`, `remote_url` as the caller made
/// it portable, and `schedules = [{kind, schedule, mode, enabled, on_missed,
/// missed_delay_ms, description}]` — no ids, no leases, no next due time.
pub(crate) fn drive_table(
    profile: &SyncProfile,
    remote_url: &str,
    schedules: &[&TaskRow],
) -> Result<toml::Table, String> {
    let mut json = serde_json::to_value(profile).map_err(|e| e.to_string())?;
    if let Value::Object(map) = &mut json {
        for key in LOCAL_ONLY {
            map.remove(key);
        }
        map.insert("remoteUrl".to_owned(), Value::String(remote_url.to_owned()));
    }
    let json = rekey(json, snake).unwrap_or(Value::Null);
    let mut table: toml::Table = serde_json::from_value(json).map_err(|e| e.to_string())?;
    if !schedules.is_empty() {
        let rows = schedules
            .iter()
            .map(|task| {
                let mut row = toml::Table::new();
                row.insert("kind".to_owned(), task.kind.as_str().into());
                if let Some(schedule) = &task.schedule {
                    row.insert("schedule".to_owned(), schedule.clone().into());
                }
                row.insert("mode".to_owned(), task.mode.as_str().into());
                row.insert("enabled".to_owned(), task.enabled.into());
                row.insert("on_missed".to_owned(), task.on_missed.as_str().into());
                if let Some(delay) = task.missed_delay_ms {
                    row.insert("missed_delay_ms".to_owned(), delay.into());
                }
                if let Some(description) = &task.description {
                    row.insert("description".to_owned(), description.clone().into());
                }
                toml::Value::Table(row)
            })
            .collect();
        table.insert(SCHEDULES.to_owned(), toml::Value::Array(rows));
    }
    Ok(table)
}

/// Where a drive table's folder is on this device.
pub(crate) fn local_path(table: &toml::Table) -> Option<PathBuf> {
    table
        .get("local_path")
        .and_then(toml::Value::as_str)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

/// A drive table as a profile again, under the new `id`. Fields this build
/// does not know are ignored; one it needs and cannot read refuses the table.
pub(crate) fn profile_of(table: &toml::Table, id: &str) -> Result<SyncProfile, String> {
    let mut table = table.clone();
    table.remove(SCHEDULES);
    let json = serde_json::to_value(&table).map_err(|e| e.to_string())?;
    let mut json = rekey(json, camel).unwrap_or(Value::Null);
    if let Value::Object(map) = &mut json {
        map.insert("id".to_owned(), Value::String(id.to_owned()));
    }
    serde_json::from_value(json).map_err(|e| e.to_string())
}

/// A drive table's schedules as task rows for the profile `profile_id`. A
/// `gc` schedule takes the id keeper seeds a folder's own under, so the two
/// are one row; every other gets `new_id()`. An entry this build cannot read
/// is skipped with a warning rather than guessed.
pub(crate) fn tasks_of(
    table: &toml::Table,
    profile_id: &str,
    now_ms: i64,
    mut new_id: impl FnMut() -> String,
) -> Vec<TaskRow> {
    let Some(rows) = table.get(SCHEDULES).and_then(toml::Value::as_array) else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|row| {
            let row = row.as_table()?;
            let kind = row.get("kind").and_then(toml::Value::as_str);
            let mode = row.get("mode").and_then(toml::Value::as_str);
            let (Some(kind), Some(mode)) = (
                kind.and_then(TaskKind::from_stored).filter(|k| travels(*k)),
                mode.and_then(TaskMode::from_stored),
            ) else {
                tracing::warn!(?kind, ?mode, "account: a drive's schedule was not restored");
                return None;
            };
            let id = if kind == TaskKind::Gc {
                keeper_sync::db::gc_task_id(profile_id)
            } else {
                new_id()
            };
            Some(TaskRow {
                id,
                profile_id: Some(profile_id.to_owned()),
                kind,
                schedule: row
                    .get("schedule")
                    .and_then(toml::Value::as_str)
                    .map(str::to_owned),
                mode,
                next_due_ms: None,
                enabled: row
                    .get("enabled")
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(true),
                updated_ms: now_ms,
                running_host: None,
                lease_until_ms: None,
                on_missed: row
                    .get("on_missed")
                    .and_then(toml::Value::as_str)
                    .and_then(TaskMissedPolicy::from_stored)
                    .unwrap_or_default(),
                description: row
                    .get("description")
                    .and_then(toml::Value::as_str)
                    .map(str::to_owned),
                missed_delay_ms: row.get("missed_delay_ms").and_then(toml::Value::as_integer),
                bot_id: None,
                prompt_subpath: None,
                model: None,
                copy_source: None,
                copy_destination: None,
                replace_existing: false,
                prune_destination: false,
                refresh_missing: true,
                copy_lookback_ms: COPY_LOOKBACK_DEFAULT_MS,
            })
        })
        .collect()
}

/// This device's file as the next sync writes it: its current state, plus
/// everything still waiting to be restored — a drive table not yet here,
/// and a grant under its provider — so nothing waiting is ever dropped from
/// the file; with the fields the tip carries that this build does not know
/// kept at the file, provider and account level.
pub(crate) fn rendered(
    mine: &DeviceStateFile,
    pending: &RestorePending,
    tip: Option<&DeviceStateFile>,
) -> DeviceStateFile {
    let mut file = mine.clone();
    for table in &pending.drives {
        let key = drive_key(table);
        if key.is_some() && !file.drives.iter().any(|t| drive_key(t) == key) {
            file.drives.push(table.clone());
        }
    }
    for waiting in &pending.grants {
        if let Some(provider) = file
            .providers
            .iter_mut()
            .find(|p| p.key().reference() == waiting.provider)
        {
            if !provider.grants.contains(&waiting.grant) {
                provider.grants.push(waiting.grant.clone());
            }
        }
    }
    for provider in &pending.providers {
        if !file.providers.iter().any(|p| p.key() == provider.key()) {
            file.providers.push(provider.clone());
        }
    }
    for account in &pending.matrix {
        if !file
            .matrix
            .iter()
            .any(|m| m.user_id.trim() == account.user_id.trim())
        {
            file.matrix.push(account.clone());
        }
    }
    if let Some(tip) = tip {
        file.extra = tip.extra.clone();
        let known = known_drive_keys();
        for drive in &mut file.drives {
            let key = drive_key(drive);
            let Some(old) = tip
                .drives
                .iter()
                .find(|t| key.is_some() && drive_key(t) == key)
            else {
                continue;
            };
            for (name, value) in old {
                if !known.contains(name.as_str()) && !drive.contains_key(name) {
                    drive.insert(name.clone(), value.clone());
                }
            }
        }
        for provider in &mut file.providers {
            let Some(old) = tip.providers.iter().find(|p| p.key() == provider.key()) else {
                continue;
            };
            provider.extra = old.extra.clone();
            for bot in &mut provider.bots {
                if let Some(was) = old.bots.iter().find(|b| b.target == bot.target) {
                    bot.extra = was.extra.clone();
                }
            }
            for grant in &mut provider.grants {
                let same = |g: &&GrantState| {
                    (&g.bot, &g.drive, &g.subtree) == (&grant.bot, &grant.drive, &grant.subtree)
                };
                if let Some(was) = old.grants.iter().find(same) {
                    grant.extra = was.extra.clone();
                }
            }
        }
        for account in &mut file.matrix {
            if let Some(old) = tip
                .matrix
                .iter()
                .find(|a| a.user_id.trim() == account.user_id.trim())
            {
                account.extra = old.extra.clone();
            }
        }
    }
    file
}

/// Every key a drive table of this build can hold — the profile's fields,
/// set or not, and its schedules. Anything else in a tip's table was written
/// by a newer keeper and is kept.
fn known_drive_keys() -> std::collections::BTreeSet<String> {
    let fields = serde_json::to_value(SyncProfile::new("", "", "", ""))
        .ok()
        .and_then(|value| {
            value
                .as_object()
                .map(|map| map.keys().map(|k| snake(k)).collect())
        })
        .unwrap_or_default();
    let mut known: std::collections::BTreeSet<String> = fields;
    known.insert(SCHEDULES.to_owned());
    known
}

// ---------------------------------------------------------------------------
// Restoring this device (AD-329)
// ---------------------------------------------------------------------------

/// What one restore pass created.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Outcome {
    pub drives: usize,
    pub providers: usize,
}

enum DriveRestore {
    Created,
    /// Not yet: the folder's parent or volume is missing, the folder holds
    /// other files, or the disk or database refused for now. Tried again on
    /// every later sync.
    Waiting,
    /// The table itself does not read or does not validate; said in the log
    /// and dropped.
    Refused,
}

/// Restore what `plan` names that this device lacks (the first restore;
/// `None` on every later sync) and the grants `present_grants` of providers
/// this device already has, then retry what `pending` holds — each
/// idempotent against identity, so nothing already here is made twice.
/// Everything that could not be made yet, bar a table that can never be,
/// comes back in `pending`.
pub(crate) fn run(
    app: Option<&AppHandle>,
    platform: &Arc<dyn Platform>,
    data_dir: &Path,
    account_id: &str,
    plan: Option<RestorePlan>,
    present_grants: Vec<PendingGrant>,
    pending: &mut RestorePending,
) -> Result<Outcome, CoreError> {
    let engine = crate::sync::engine(Arc::clone(platform))
        .map_err(|error| CoreError::Internal(error.to_string()))?;
    let here = |engine: &Engine| -> Result<Vec<SyncProfile>, CoreError> {
        engine
            .list_profiles()
            .map_err(|error| CoreError::Internal(error.to_string()))
    };
    let mut outcome = Outcome::default();
    let (plan_drives, plan_providers, plan_matrix) = plan
        .map(|plan| (plan.drives, plan.providers, plan.matrix))
        .unwrap_or_default();

    // Drives: the waiting ones first, then the plan's. One whose identity or
    // folder is on this device by now is done.
    let profiles = here(&engine)?;
    let mut local: Vec<(DriveKey, Option<PathBuf>)> = profiles
        .iter()
        .map(|p| {
            (
                DriveRef::named(&p.remote_url, &p.branch, &p.name),
                canonical(&p.local_path),
            )
        })
        .collect();
    let mut waiting: Vec<toml::Table> = Vec::new();
    let queued = std::mem::take(&mut pending.drives);
    for table in queued.into_iter().chain(plan_drives) {
        let Some(key) = drive_key(&table) else {
            continue;
        };
        let folder = local_path(&table).and_then(|path| canonical(&path));
        let present = local
            .iter()
            .any(|(k, path)| *k == key || (folder.is_some() && *path == folder));
        if present || waiting.iter().any(|t| drive_key(t).as_ref() == Some(&key)) {
            continue;
        }
        match restore_drive(platform, &engine, &table) {
            DriveRestore::Created => {
                outcome.drives += 1;
                local.push((key, folder));
            }
            DriveRestore::Waiting => waiting.push(table),
            DriveRestore::Refused => {}
        }
    }
    pending.drives = waiting;
    let profiles = here(&engine)?;
    crate::account_ipc::note_drives(&profiles);
    if outcome.drives > 0 {
        drives_added(app);
    }

    // Providers with their bots; one that could not be added waits.
    let mut catalog = catalog_of(data_dir, account_id, &profiles)?;
    let mut grants = present_grants;
    let queued = std::mem::take(&mut pending.providers);
    for state in queued.into_iter().chain(plan_providers) {
        let key = state.key();
        if catalog.providers.iter().any(|(_, p)| *p == key)
            || pending.providers.iter().any(|p| p.key() == key)
        {
            continue;
        }
        match add_provider_state(data_dir, account_id, &state) {
            Ok(_) => {
                outcome.providers += 1;
                let reference = key.reference();
                grants.extend(state.grants.iter().map(|grant| PendingGrant {
                    provider: reference.clone(),
                    grant: grant.clone(),
                }));
            }
            Err(why) => {
                tracing::warn!(%why, "account: a bot provider was not restored; it waits");
                pending.providers.push(state);
            }
        }
    }
    if outcome.providers > 0 {
        catalog = catalog_of(data_dir, account_id, &profiles)?;
    }

    // Grants, for providers matched by identity; the same grant already
    // here is not made twice, and one that cannot be made yet waits.
    let bots = store::list_bots(data_dir)?;
    let live: Vec<_> = store::list_grants(data_dir)?
        .rows
        .into_iter()
        .filter(|row| row.revoked_ms.is_none())
        .map(|row| row.grant)
        .collect();
    let queued = std::mem::take(&mut pending.grants);
    for waiting in grants.into_iter().chain(queued) {
        let provider_id = catalog
            .providers
            .iter()
            .find(|(_, provider)| provider.reference() == waiting.provider)
            .map(|(id, _)| id.clone());
        // The person removed the provider since: nothing to grant to.
        let Some(provider_id) = provider_id else {
            continue;
        };
        match account_settings::grant_of(&waiting.grant, &provider_id, &bots, &catalog) {
            Ok(Some(grant)) => {
                let exists = live.iter().any(|g| {
                    (&g.provider_id, &g.bot_id, &g.scope, g.mode)
                        == (&grant.provider_id, &grant.bot_id, &grant.scope, grant.mode)
                });
                if exists {
                    continue;
                }
                if let Err(error) = store::save_grant(data_dir, &grant) {
                    tracing::warn!(%error, "account: a bot's folder grant waits");
                    pending.grants.push(waiting);
                }
            }
            Ok(None) => pending.grants.push(waiting),
            Err(why) => {
                tracing::warn!(%why, "account: a bot's folder grant waits");
                pending.grants.push(waiting);
            }
        }
    }

    // Matrix accounts wait for their sign-in; their preferences go on when
    // each is added (`account_ipc::matrix_account_added`).
    for account in plan_matrix {
        if !pending
            .matrix
            .iter()
            .any(|m| m.user_id.trim() == account.user_id.trim())
        {
            pending.matrix.push(account);
        }
    }
    Ok(outcome)
}

fn catalog_of(
    data_dir: &Path,
    account_id: &str,
    profiles: &[SyncProfile],
) -> Result<Catalog, CoreError> {
    let drives = profiles.iter().map(account_settings::local_drive).collect();
    account_settings::catalog(data_dir, account_id, Some(drives))
}

/// A path as the filesystem resolves it, or `None` when it does not exist.
fn canonical(path: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(path).ok()
}

/// Make one drive from its table: its folder (created when its parent
/// exists; on a phone, in the app's container as a new phone drive is), the
/// profile under a new id, then its schedules. Only a table that does not
/// read or validate is refused; everything else waits.
fn restore_drive(
    platform: &Arc<dyn Platform>,
    engine: &Engine,
    table: &toml::Table,
) -> DriveRestore {
    let id = crate::sync_ipc::new_ulid();
    let mut profile = match profile_of(table, &id) {
        Ok(profile) => profile,
        Err(why) => {
            tracing::warn!(%why, "account: a drive in this device's file does not read");
            return DriveRestore::Refused;
        }
    };
    let folder = match drive_folder(platform, table, &profile, &id) {
        Some(folder) => folder,
        None => return DriveRestore::Waiting,
    };
    profile.local_path = folder;
    if let Err(error) = profile.validate() {
        tracing::warn!(%error, "account: a drive in this device's file is not valid here");
        return DriveRestore::Refused;
    }
    if let Err(error) = engine.upsert_profile(&profile) {
        tracing::warn!(%error, "account: a drive waits to be restored");
        return DriveRestore::Waiting;
    }
    // The profile's own gc row may be seeded already: it keeps keeper's
    // sentence unless the file carried one, and takes the file's schedule.
    let seeded = engine
        .tasks()
        .map(|listing| listing.tasks)
        .unwrap_or_default();
    let now = chrono::Utc::now().timestamp_millis();
    for mut task in tasks_of(table, &id, now, crate::sync_ipc::new_ulid) {
        if let Some(existing) = seeded.iter().find(|t| t.id == task.id) {
            if task.description.is_none() {
                task.description = existing.description.clone();
            }
        }
        if let Err(error) = engine.save_task(&task, None) {
            tracing::warn!(%error, "account: a drive's schedule was not restored");
        }
    }
    DriveRestore::Created
}

/// The folder a restored drive syncs into, or `None` while it must wait: its
/// parent (or its removable volume) is missing, it cannot be made, or it
/// already holds files that are not this drive's clone.
#[cfg(desktop)]
fn drive_folder(
    _platform: &Arc<dyn Platform>,
    table: &toml::Table,
    profile: &SyncProfile,
    _id: &str,
) -> Option<PathBuf> {
    let folder = local_path(table)?;
    if folder.is_dir() {
        if folder_accepts(&folder, &profile.remote_url) {
            return Some(folder);
        }
        tracing::info!(path = %folder.display(), "account: a drive's folder holds other files; it waits");
        return None;
    }
    if profile.removable || !folder.parent().is_some_and(Path::is_dir) {
        return None;
    }
    match std::fs::create_dir_all(&folder) {
        Ok(()) => Some(folder),
        Err(error) => {
            tracing::warn!(%error, path = %folder.display(), "account: a drive's folder could not be made; it waits");
            None
        }
    }
}

/// Whether an existing folder may take a restored drive: it is empty, or it
/// is already a clone whose `origin` is the drive's remote.
#[cfg(desktop)]
fn folder_accepts(folder: &Path, remote: &str) -> bool {
    let Ok(mut entries) = std::fs::read_dir(folder) else {
        return false;
    };
    if entries.next().is_none() {
        return true;
    }
    std::fs::read_to_string(folder.join(".git").join("config"))
        .ok()
        .and_then(|config| origin_url(&config))
        .is_some_and(|origin| DriveRef::new(&origin, "") == DriveRef::new(remote, ""))
}

/// `[remote "origin"] url` from a git config's text.
#[cfg_attr(not(desktop), allow(dead_code))]
fn origin_url(config: &str) -> Option<String> {
    let mut in_origin = false;
    for line in config.lines().map(str::trim) {
        if line.starts_with('[') {
            in_origin = line.replace(char::is_whitespace, "") == "[remote\"origin\"]";
            continue;
        }
        if in_origin {
            if let Some((key, value)) = line.split_once('=') {
                if key.trim() == "url" {
                    return Some(value.trim().to_owned());
                }
            }
        }
    }
    None
}

/// On a phone the folder is always the app container's, as for a drive
/// added there (`phone_shaped_request`): the old path means nothing here.
#[cfg(not(desktop))]
fn drive_folder(
    platform: &Arc<dyn Platform>,
    _table: &toml::Table,
    _profile: &SyncProfile,
    id: &str,
) -> Option<PathBuf> {
    let folder = match platform.data_dir() {
        Ok(dir) => dir.join(crate::sync_ipc::PHONE_FOLDERS_DIR).join(id),
        Err(error) => {
            tracing::warn!(%error, "account: no data directory; a drive waits");
            return None;
        }
    };
    if let Err(error) = std::fs::create_dir_all(&folder) {
        tracing::warn!(%error, "account: a drive's folder could not be made; it waits");
        return None;
    }
    if let Err(error) = platform.exclude_from_backup(&folder) {
        tracing::warn!(%error, "account: a restored folder was not excluded from backup");
    }
    Some(folder)
}

/// What a saved drive sets moving, as `sync_profile_save` does: the sessions
/// roots and the recordings archive follow the profile list.
fn drives_added(app: Option<&AppHandle>) {
    use tauri::Manager;

    let Some(app) = app else {
        return;
    };
    #[cfg(desktop)]
    crate::sessions_root::refresh(app);
    crate::ipc::spawn_recordings_index_rebuild(
        app.state::<crate::ipc::AppState>().inner(),
        crate::ipc::RecordingsIndexTrigger::because("a drive was restored from the account"),
    );
}

/// Add a provider from this device's file, with its read timeout, its bots
/// and — when it used the account — its account credential; `own` is left
/// for the person, whom the provider's health then asks for its key.
fn add_provider_state(
    data_dir: &Path,
    account_id: &str,
    state: &ProviderState,
) -> Result<String, String> {
    let kind = ProviderKind::from_registry_str(&state.kind)
        .ok_or_else(|| format!("this keeper cannot talk to a {} provider", state.kind))?;
    let base_url = bots::parse_base_url(&state.base_url)
        .map_err(|error| format!("{}: {error}", state.base_url))?
        .normalized;
    let provider = Provider {
        id: crate::bots_ipc::new_id(),
        kind,
        name: state.name.clone(),
        base_url,
        created_ms: chrono::Utc::now().timestamp_millis(),
    };
    let account = (state.credential == "account").then_some(account_id);
    add_provider(
        data_dir,
        &provider,
        account,
        state.read_timeout_ms,
        state.bots.clone(),
    )
    .map_err(|error| error.to_string())?;
    Ok(provider.id)
}

/// Write a provider the account describes: the row, its read timeout, its
/// account credential when `account` is set, and its bots after the ones
/// already pinned, in their own order. All or nothing: a failed write — a
/// bot target the store refuses among them — removes what was written.
pub(crate) fn add_provider(
    data_dir: &Path,
    provider: &Provider,
    account: Option<&str>,
    read_timeout_ms: Option<u64>,
    mut offered: Vec<BotRecord>,
) -> Result<(), CoreError> {
    offered.sort_by_key(|bot| bot.pin_order);
    let written = (|| -> Result<(), CoreError> {
        store::insert_provider(data_dir, provider)?;
        if let Some(ms) = read_timeout_ms {
            store::set_provider_read_timeout(
                data_dir,
                &provider.id,
                Some(i64::try_from(ms).unwrap_or(i64::MAX)),
            )?;
        }
        if let Some(account_id) = account {
            registry::set_bots_provider_credential_source(
                data_dir,
                &provider.id,
                Some("account"),
                Some(account_id),
            )?;
        }
        let pinned = store::list_bots(data_dir)?.len();
        for (offset, bot) in offered.into_iter().enumerate() {
            let target = bot.target.trim().to_owned();
            let name = match bot.name.trim() {
                "" => target.clone(),
                name => name.to_owned(),
            };
            store::insert_bot(
                data_dir,
                &Bot {
                    id: crate::bots_ipc::new_id(),
                    provider_id: provider.id.clone(),
                    target,
                    name,
                    pin_order: i64::try_from(pinned + offset).unwrap_or(i64::MAX),
                    identity: BotIdentity {
                        shape: bot.shape,
                        colour: bot.colour,
                        mark: bot.mark,
                    },
                    created_ms: provider.created_ms,
                },
            )?;
        }
        Ok(())
    })();
    if let Err(error) = written {
        let undone = store::delete_provider(data_dir, &provider.id).and_then(|()| {
            registry::set_bots_provider_credential_source(data_dir, &provider.id, None, None)
        });
        if let Err(undo) = undone {
            tracing::warn!(%undo, "account: a half-added provider could not be removed");
        }
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use keeper_sync::profile::{PushPolicy, RecordingsConfig};

    fn profile() -> SyncProfile {
        let mut p = SyncProfile::new(
            "01LOCAL",
            "tgdrive-light",
            "/Users/t/tgdrive-light",
            "https://t@git.acme.dev/t/tgdrive.git",
        );
        p.volume_id = Some("VOL-1".to_owned());
        p.removable = true;
        p.lfs_never = vec!["*.md".to_owned()];
        p.virtual_patterns = vec!["media/**".to_owned()];
        p.virtual_over_bytes = 1 << 20;
        p.settle_ms = 4_000;
        p.author_override = Some("T <t@acme.dev>".to_owned());
        p.recordings = Some(RecordingsConfig {
            subfolder: "rec".to_owned(),
            push: PushPolicy::Window {
                quiet_from: "22:00".to_owned(),
                quiet_to: "06:00".to_owned(),
            },
            ..RecordingsConfig::default()
        });
        p
    }

    fn task(id: &str, profile: Option<&str>, kind: TaskKind) -> TaskRow {
        let mut rows = tasks_of(
            &toml::from_str(&format!(
                "[[schedules]]\nkind = \"{}\"\nschedule = \"every 1d\"\nmode = \"scheduled\"\nenabled = false\n",
                TaskKind::Sync.as_str()
            ))
            .expect("table"),
            profile.unwrap_or("none"),
            1,
            || id.to_owned(),
        );
        let mut row = rows.remove(0);
        row.profile_id = profile.map(str::to_owned);
        row.kind = kind;
        row.next_due_ms = Some(99);
        row.lease_until_ms = Some(98);
        row
    }

    /// The table is the whole profile but its id, its volume and its commit
    /// author (F6), in snake_case, with the portable remote; read back under a
    /// new id it is the same profile, nested policies and all.
    #[test]
    fn a_drive_table_carries_the_whole_profile_but_its_id_and_volume() {
        let original = profile();
        let table =
            drive_table(&original, "https://git.acme.dev/t/tgdrive.git", &[]).expect("renders");
        assert!(!table.contains_key("id"));
        assert!(!table.contains_key("volume_id"));
        assert!(!table.contains_key("volumeId"));
        assert_eq!(table["name"].as_str(), Some("tgdrive-light"));
        assert_eq!(
            table["remote_url"].as_str(),
            Some("https://git.acme.dev/t/tgdrive.git")
        );
        assert_eq!(table["branch"].as_str(), Some("main"));
        assert_eq!(table["virtual_over_bytes"].as_integer(), Some(1 << 20));
        assert_eq!(
            table["recordings"]["push"]["quiet_from"].as_str(),
            Some("22:00")
        );
        assert_eq!(
            local_path(&table),
            Some(PathBuf::from("/Users/t/tgdrive-light"))
        );

        // Through the file's own text, as another sync would read it.
        let text = toml::to_string(&table).expect("toml");
        let read: toml::Table = toml::from_str(&text).expect("parses");
        let back = profile_of(&read, "01NEW").expect("a profile");
        let expected = SyncProfile {
            id: "01NEW".to_owned(),
            volume_id: None,
            author_override: None,
            remote_url: "https://git.acme.dev/t/tgdrive.git".to_owned(),
            ..original
        };
        assert_eq!(back, expected);
    }

    /// F9: only an `origin` URL identifies an existing clone.
    #[test]
    fn the_origin_url_is_read_from_its_own_section() {
        let config = "[core]\n\turl = nope\n[remote \"upstream\"]\n\turl = https://u/x.git\n[remote \"origin\"]\n\turl = https://git.acme.dev/t/tgdrive.git\n";
        assert_eq!(
            origin_url(config).as_deref(),
            Some("https://git.acme.dev/t/tgdrive.git")
        );
        assert_eq!(origin_url("[remote \"upstream\"]\n url = x\n"), None);
    }

    /// F8/m11: what waits is written back into the file, never dropped, and a
    /// field a newer keeper wrote into a drive table survives; one this build
    /// knows and left out does not come back.
    #[test]
    fn the_rendered_file_keeps_what_waits_and_unknown_fields() {
        let mine_table =
            drive_table(&profile(), "https://git.acme.dev/t/tgdrive.git", &[]).expect("renders");
        let mine = DeviceStateFile {
            drives: vec![mine_table.clone()],
            ..DeviceStateFile::default()
        };
        let mut waiting = mine_table.clone();
        waiting.insert("name".to_owned(), "tgdrive".into());
        let pending = RestorePending {
            drives: vec![waiting],
            ..RestorePending::default()
        };
        let mut old = mine_table;
        old.insert("future_knob".to_owned(), 7.into());
        old.insert("author_override".to_owned(), "T <t@x>".into());
        let tip = DeviceStateFile {
            drives: vec![old],
            ..DeviceStateFile::default()
        };
        let file = rendered(&mine, &pending, Some(&tip));
        assert_eq!(file.drives.len(), 2, "the waiting drive stays in the file");
        let light = file
            .drives
            .iter()
            .find(|d| d["name"].as_str() == Some("tgdrive-light"))
            .expect("this device's drive");
        assert_eq!(light["future_knob"].as_integer(), Some(7));
        assert!(!light.contains_key("author_override"));
    }

    /// Only this folder's schedules of the kinds that mean something on
    /// another install travel, without ids, leases or due times, but with
    /// what a missed window does (F14); restored, a `gc` schedule takes the
    /// seeded row's id.
    #[test]
    fn schedules_travel_without_ids_or_leases_and_gc_joins_its_seed() {
        let tasks = [
            task("t-sync", Some("01LOCAL"), TaskKind::Sync),
            task("t-gc", Some("01LOCAL"), TaskKind::Gc),
            task("t-bot", Some("01LOCAL"), TaskKind::Bot),
            task("t-other", Some("01OTHER"), TaskKind::Verify),
            task("t-host", None, TaskKind::Release),
        ];
        let mine = schedules_of("01LOCAL", &tasks);
        let table = drive_table(&profile(), "r", &mine).expect("renders");
        let rows = table["schedules"].as_array().expect("schedules");
        assert_eq!(rows.len(), 2);
        let first = rows[0].as_table().expect("row");
        let mut keys: Vec<&str> = first.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["enabled", "kind", "mode", "on_missed", "schedule"]);
        assert_eq!(first["kind"].as_str(), Some("sync"));
        assert_eq!(first["enabled"].as_bool(), Some(false));

        let mut minted = 0;
        let restored = tasks_of(&table, "01NEW", 7, || {
            minted += 1;
            format!("new-{minted}")
        });
        assert_eq!(
            restored
                .iter()
                .map(|t| (t.id.as_str(), t.kind))
                .collect::<Vec<_>>(),
            [("new-1", TaskKind::Sync), ("gc-01NEW", TaskKind::Gc)]
        );
        assert!(restored
            .iter()
            .all(|t| t.profile_id.as_deref() == Some("01NEW")
                && t.next_due_ms.is_none()
                && t.lease_until_ms.is_none()
                && t.schedule.as_deref() == Some("every 1d")
                && !t.enabled));
    }
}
