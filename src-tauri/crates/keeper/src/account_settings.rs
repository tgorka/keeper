//! What the account's settings sync needs from this device, and how a pulled
//! value lands here (Epic 84, AD-320–AD-325).
//!
//! `keeper-core` decides everything about the person's files — which keys
//! travel, how two devices merge, what a drive or a provider looks like in
//! `drives.toml` — but it cannot see `sync.db` (AD-40), the keychain's
//! contents or the live state beside a setting. This module is those three
//! facts: the drives, providers and Matrix accounts this device has, as
//! portable records with the *choice* of credential and never a credential;
//! and the one table that says which pulled key has in-memory state that a
//! plain registry write would leave stale.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use keeper_core::bots::grant::GrantScope;
use keeper_core::bots::store;
use keeper_core::error::CoreError;
use keeper_core::org_account::device_state::{
    DeviceStateFile, GrantState, MatrixState, ProviderState,
};
use keeper_core::org_account::manifest::{
    self, BotRecord, DriveRecord, MatrixRecord, ProviderRecord,
};
use keeper_core::org_account::settings_sync::{self, Catalog, LocalDrive, Merged, Values};
use keeper_core::platform::Platform;
use keeper_core::registry;
use keeper_sync::SyncProfile;
use tauri::AppHandle;

/// How a grant over every drive is named in the device file, where any
/// other grant names its drive by reference.
pub(crate) const EVERY_DRIVE: &str = "*";

/// This device's drives, bot providers and Matrix accounts as the person's
/// manifests describe them, plus the catalog that translates the settings
/// that name one of them.
pub(crate) struct Mine {
    pub catalog: Catalog,
    /// `None` when the drive list could not be read: `drives.toml` is then
    /// left as it is, rather than this device leaving every drive in it.
    pub drives: Option<Vec<DriveRecord>>,
    pub providers: Vec<ProviderRecord>,
    pub matrix: Vec<MatrixRecord>,
    /// This device as its `device.<slug>.toml` describes it (AD-328), or
    /// `None` when its drives or their schedules could not be read — then
    /// the file is left as it is rather than written without them.
    pub device: Option<DeviceStateFile>,
}

/// Read [`Mine`] for the configured account `account_id`.
pub(crate) fn gather(
    platform: &Arc<dyn Platform>,
    data_dir: &Path,
    account_id: &str,
) -> Result<Mine, CoreError> {
    let (profiles, tasks) = match crate::sync::engine(Arc::clone(platform)) {
        Ok(engine) => {
            let profiles = engine
                .list_profiles()
                .inspect_err(|error| {
                    tracing::warn!(%error, "account: the drives could not be listed for the settings sync");
                })
                .ok();
            let tasks = engine
                .tasks()
                .inspect_err(|error| {
                    tracing::warn!(%error, "account: the drives' schedules could not be listed");
                })
                .ok()
                .map(|listing| listing.tasks);
            (profiles, tasks)
        }
        Err(error) => {
            tracing::warn!(%error, "account: the drives could not be listed for the settings sync");
            (None, None)
        }
    };
    if let Some(profiles) = &profiles {
        crate::account_ipc::note_drives(profiles);
    }
    let mut drive_tables = Vec::new();
    let (drive_refs, drives) = match &profiles {
        Some(profiles) => {
            let mut refs = Vec::with_capacity(profiles.len());
            let mut records = Vec::with_capacity(profiles.len());
            for profile in profiles {
                // Every drive translates the settings that name it here,
                // even one whose remote cannot travel.
                refs.push(local_drive(profile));
                let portable = manifest::portable_remote(&profile.remote_url);
                // This device's own file keeps a local-path remote: it is a
                // fact of this device, and restores only this device.
                if let Some(tasks) = &tasks {
                    let remote = portable.as_deref().unwrap_or(&profile.remote_url);
                    let schedules = crate::account_restore::schedules_of(&profile.id, tasks);
                    match crate::account_restore::drive_table(profile, remote, &schedules) {
                        Ok(table) => drive_tables.push(table),
                        Err(why) => {
                            tracing::warn!(%why, "account: a drive could not be written for this device's file");
                        }
                    }
                }
                let Some(remote_url) = portable else {
                    continue;
                };
                let source =
                    registry::get_sync_credential_source(data_dir, &profile.id, Some(account_id))?;
                let bound = source.as_deref() == Some("account");
                // A drive signing in with a repository source (`forge:<id>`)
                // has a credential of its own: "none" would tell other
                // devices a private repository needs nothing.
                let forge = source
                    .as_deref()
                    .is_some_and(|s| keeper_core::forges::forge_credential_id(s).is_some());
                // Presence only: the value is never read past this line.
                let own = forge
                    || platform
                        .keychain_get(&profile.secret_key())
                        .is_ok_and(|secret| secret.is_some());
                records.push(drive_record(
                    profile,
                    remote_url,
                    credential_choice(bound, own),
                ));
            }
            (Some(refs), Some(records))
        }
        None => (None, None),
    };
    let catalog = catalog(platform.as_ref(), data_dir, account_id, drive_refs)?;

    let listing = store::list_providers(data_dir)?;
    let grants = store::list_grants(data_dir)?.rows;
    let mut providers = Vec::with_capacity(listing.rows.len());
    let mut provider_states = Vec::with_capacity(listing.rows.len());
    for row in &listing.rows {
        let provider = &row.provider;
        let bound = registry::get_bots_provider_credential_source(
            data_dir,
            &provider.id,
            Some(account_id),
        )?
        .is_some();
        let bots: Vec<BotRecord> = store::list_bots_for_provider(data_dir, &provider.id)?
            .into_iter()
            .map(|bot| BotRecord {
                target: bot.target,
                name: bot.name,
                pin_order: bot.pin_order,
                shape: bot.identity.shape,
                colour: bot.identity.colour,
                mark: bot.identity.mark,
                extra: BTreeMap::new(),
            })
            .collect();
        let credential = if bound { "account" } else { "own" };
        let read_timeout_ms = row.read_timeout_ms.and_then(|ms| u64::try_from(ms).ok());
        provider_states.push(ProviderState {
            kind: provider.kind.as_registry_str().to_owned(),
            name: provider.name.clone(),
            base_url: provider.base_url.clone(),
            read_timeout_ms,
            credential: credential.to_owned(),
            bots: bots.clone(),
            grants: grants
                .iter()
                .filter(|row| row.revoked_ms.is_none() && row.grant.provider_id == provider.id)
                .filter_map(|row| grant_state(&row.grant, &catalog))
                .collect(),
            extra: BTreeMap::new(),
        });
        providers.push(ProviderRecord {
            kind: provider.kind.as_registry_str().to_owned(),
            name: provider.name.clone(),
            base_url: provider.base_url.clone(),
            credential: credential.to_owned(),
            read_timeout_ms,
            bots,
            devices: Vec::new(),
            extra: BTreeMap::new(),
        });
    }

    let accounts = registry::list_accounts(data_dir)?;
    // Muted networks are this install's, not one account's: every account
    // carries the list, and a restore puts back their union.
    let muted = registry::get_muted_networks(data_dir)?;
    let mut matrix = Vec::with_capacity(accounts.len());
    let mut matrix_states = Vec::with_capacity(accounts.len());
    for row in accounts {
        let kind = matrix_kind(row.provider.as_deref()).to_owned();
        matrix_states.push(MatrixState {
            user_id: row.user_id.clone(),
            homeserver_url: row.homeserver_url.clone(),
            kind: kind.clone(),
            hue_index: row.hue_index.map(i64::from),
            muted_networks: muted.clone(),
            extra: BTreeMap::new(),
        });
        matrix.push(MatrixRecord {
            user_id: row.user_id,
            homeserver_url: row.homeserver_url,
            kind,
            devices: Vec::new(),
            extra: BTreeMap::new(),
        });
    }

    let device = (profiles.is_some() && tasks.is_some()).then(|| DeviceStateFile {
        drives: drive_tables,
        providers: provider_states,
        matrix: matrix_states,
        // This device's fingerprint is set by the caller, who knows the
        // sign-in it is keyed to.
        machine: None,
        extra: BTreeMap::new(),
    });
    Ok(Mine {
        catalog,
        drives,
        providers,
        matrix,
        device,
    })
}

/// A drive as the catalog knows it.
pub(crate) fn local_drive(profile: &SyncProfile) -> LocalDrive {
    LocalDrive {
        profile_id: profile.id.clone(),
        remote_url: profile.remote_url.clone(),
        branch: profile.branch.clone(),
        name: profile.name.clone(),
        local_path: profile.local_path.to_string_lossy().into_owned(),
    }
}

/// The catalog a pulled setting is translated against, bound to the
/// configured account and trusting only its own origins (F5): a pulled
/// "use my account" for a drive or provider elsewhere is not applied.
pub(crate) fn catalog(
    platform: &dyn Platform,
    data_dir: &Path,
    account_id: &str,
    drives: Option<Vec<LocalDrive>>,
) -> Result<Catalog, CoreError> {
    let mut catalog = Catalog::with_bots(data_dir, drives)?;
    catalog.account_id = Some(account_id.to_owned());
    let descriptor = crate::account_ipc::descriptor().filter(|d| d.id == account_id);
    if let Some(d) = &descriptor {
        catalog.trusted_origins = trusted_origins(d);
    }
    // A pulled `forge:<id>` applies only to a drive at that source's origin,
    // and only for a source this device can get a token for.
    catalog.forge_origins = keeper_core::forges::token_origins(
        platform,
        descriptor.as_ref(),
        keeper_core::forges::BUILTIN_GITHUB_CLIENT_ID,
    );
    Ok(catalog)
}

/// The descriptor's issuer, repository and forge origins.
fn trusted_origins(d: &keeper_core::org_account::descriptor::AccountDescriptor) -> Vec<String> {
    use keeper_core::org_account::descriptor::RepoAuthConfig;

    let mut urls = vec![d.auth.issuer.as_str(), d.config.url.as_str()];
    if let RepoAuthConfig::Oauth(forge) = &d.config.auth {
        urls.extend(forge.issuer.as_deref());
        urls.extend(forge.authorize_url.as_deref());
    }
    let mut origins: Vec<String> = urls
        .into_iter()
        .filter_map(settings_sync::url_origin)
        .collect();
    origins.sort();
    origins.dedup();
    origins
}

/// A live grant as the device file carries it: its bot by target, its drive
/// by reference ([`EVERY_DRIVE`] for the whole drive). `None` for a grant
/// naming a bot or drive this device no longer has.
fn grant_state(grant: &keeper_core::bots::grant::Grant, catalog: &Catalog) -> Option<GrantState> {
    let bot = match &grant.bot_id {
        None => None,
        Some(id) => Some(
            catalog
                .bots
                .iter()
                .find(|(bot_id, ..)| bot_id == id)
                .map(|(_, _, target)| target.clone())?,
        ),
    };
    let drive = match grant.scope.profile_id() {
        None => EVERY_DRIVE.to_owned(),
        Some(profile_id) => catalog.drive_reference(profile_id)?,
    };
    Some(GrantState {
        bot,
        drive,
        subtree: grant.scope.subpath().map(str::to_owned),
        mode: grant.mode.as_registry_str().to_owned(),
        extra: BTreeMap::new(),
    })
}

/// A grant from the device file as this device stores it, for the provider
/// `provider_id`: its bot resolved among that provider's bots by target, its
/// drive by reference. `None` when the drive is not here (the grant then
/// waits), `Err` when it can never be made (an unknown mode, a bad subtree,
/// a bot the provider does not have).
pub(crate) fn grant_of(
    state: &GrantState,
    provider_id: &str,
    bots: &[keeper_core::bots::Bot],
    catalog: &Catalog,
) -> Result<Option<keeper_core::bots::grant::Grant>, String> {
    use keeper_core::bots::grant::{parse_subpath, Grant, GrantMode};

    let mode = GrantMode::from_registry_str(&state.mode)
        .ok_or_else(|| format!("\"{}\" is not a grant mode", state.mode))?;
    let bot_id = match &state.bot {
        None => None,
        Some(target) => Some(
            bots.iter()
                .find(|bot| bot.provider_id == provider_id && bot.target == *target)
                .map(|bot| bot.id.clone())
                .ok_or_else(|| format!("the provider has no bot {target}"))?,
        ),
    };
    let scope = if state.drive == EVERY_DRIVE {
        GrantScope::Drive
    } else {
        let Some(profile_id) = catalog.resolve_drive(&state.drive) else {
            return Ok(None);
        };
        let profile_id = profile_id.to_owned();
        match &state.subtree {
            None => GrantScope::Profile { profile_id },
            Some(subtree) => GrantScope::Subtree {
                profile_id,
                subpath: parse_subpath(subtree).map_err(|e| format!("{subtree}: {e}"))?,
            },
        }
    };
    Ok(Some(Grant {
        id: crate::bots_ipc::new_id(),
        provider_id: provider_id.to_owned(),
        bot_id,
        scope,
        mode,
        created_ms: chrono::Utc::now().timestamp_millis(),
    }))
}

/// Which credential a drive uses, as the manifest names it: the account when
/// its credential source is bound to this account, its own token when the
/// keychain holds one, else none.
fn credential_choice(account_bound: bool, own_secret: bool) -> &'static str {
    if account_bound {
        "account"
    } else if own_secret {
        "own"
    } else {
        "none"
    }
}

/// A Matrix account's login mechanism as the manifest names it; a legacy row
/// that never recorded one signed in with a password.
fn matrix_kind(provider: Option<&str>) -> &str {
    provider.unwrap_or("password")
}

/// The portable half of a drive (`FolderFieldRule`'s `Allowed` fields plus
/// its identity): nothing about this disk, and never a credential — its
/// remote is `manifest::portable_remote`'s spelling, which carries none.
fn drive_record(profile: &SyncProfile, remote_url: String, credential: &str) -> DriveRecord {
    DriveRecord {
        name: profile.name.clone(),
        remote_url,
        branch: profile.branch.clone(),
        credential: credential.to_owned(),
        notes: profile.notes.as_ref().map(|role| role.subfolder.clone()),
        recordings: profile
            .recordings
            .as_ref()
            .map(|role| role.subfolder.clone()),
        sessions: profile.sessions.as_ref().map(|role| role.subfolder.clone()),
        tasks: profile.tasks.as_ref().map(|role| role.subfolder.clone()),
        excludes: profile.excludes.clone(),
        lfs_threshold_bytes: Some(profile.lfs_threshold_bytes),
        virtual_patterns: Some(profile.virtual_patterns.clone()),
        virtual_over_bytes: Some(profile.virtual_over_bytes),
        release_ttl_ms: Some(profile.release_ttl_ms),
        tags: profile.tags.clone(),
        commit_subject_template: (!profile.commit_subject_template.is_empty())
            .then(|| profile.commit_subject_template.clone()),
        devices: Vec::new(),
        extra: BTreeMap::new(),
    }
}

// ---------------------------------------------------------------------------
// Applying what another device changed
// ---------------------------------------------------------------------------

/// Where a pulled key's live state is moved after its row is written. Every
/// key not named here has none: the row is the whole of applying it, and the
/// next read sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    NotifyPreviews,
    Dnd,
    DockBadge,
    /// The tray icon.
    MenuBar,
    /// The on-disk logging gate.
    DebugMode,
    /// The wake switch and phrases and the recogniser's language: the voice
    /// runtime re-reads all four once the batch is written.
    Voice,
    /// The index workers re-embed.
    EmbeddingModel,
    /// The engine holds the ledger choice it was seeded with.
    LedgerVault,
    /// The engine was built from the git binary it resolved.
    GitPath,
    Registry,
}

fn route(key: &str) -> Route {
    match key {
        "notify.previews_enabled" => Route::NotifyPreviews,
        "notify.dnd_global" => Route::Dnd,
        "notify.dock_badge_mode" => Route::DockBadge,
        "system.menu_bar_presence" => Route::MenuBar,
        "debug.mode" => Route::DebugMode,
        "bots.wake_enabled" | "bots.wake_phrase" | "bots.stop_phrase" | "bots.voice_locale" => {
            Route::Voice
        }
        "notes.embedding_model" => Route::EmbeddingModel,
        "tasks.ledger_vault" => Route::LedgerVault,
        "sync.git_path" => Route::GitPath,
        _ => Route::Registry,
    }
}

/// The keys an apply did not carry out, which the caller's base must not
/// record as synced.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Unapplied {
    /// The write failed: the base records this device's own value, so the
    /// next sync neither reverts nor re-pulls blindly.
    pub failed: BTreeSet<String>,
    /// The person changed the key here while the sync ran: left as they set
    /// it, and its base stays where it was, so the change is pushed next.
    pub moved: BTreeSet<String>,
}

/// Write what the merge decided to apply and move each key's live state.
/// A key is the file's: a credential-source key names its drive or provider
/// by reference there and is written under its local id here (one this
/// device lacks is not applied, A2'). `snapshot` is the rows the merge's
/// local side was read from, keyed as stored; a key whose row no longer
/// holds that value was changed here meanwhile and is skipped. The caller
/// runs this under `registry::with_observer_suppressed`, on a thread that
/// may block (the dock badge's setter is async and is driven here on
/// `runtime`).
pub(crate) fn apply(
    app: Option<&AppHandle>,
    platform: &Arc<dyn Platform>,
    data_dir: &Path,
    runtime: &tokio::runtime::Handle,
    catalog: &Catalog,
    changes: &[(String, Option<String>)],
    snapshot: &BTreeMap<String, String>,
) -> Unapplied {
    let mut unapplied = Unapplied::default();
    let mut voice = false;
    let mut embedding = false;
    for (key, value) in changes {
        let Some(stored) = settings_sync::stored_key(key, catalog) else {
            // Its base takes this device's value, so the file's is pulled
            // again once the drive or provider exists here.
            unapplied.failed.insert(key.clone());
            continue;
        };
        let local = snapshot.get(&stored).map(String::as_str);
        if refused_here(
            &stored,
            value.as_deref(),
            local,
            crate::sync::git_path_usable,
        ) {
            // Never written; the base keeps this device's own value.
            unapplied.failed.insert(key.clone());
            continue;
        }
        let route = route(&stored);
        let written = moved_since(data_dir, &stored, snapshot).and_then(|moved| {
            if moved {
                return Ok(false);
            }
            apply_one(
                app,
                platform,
                data_dir,
                runtime,
                route,
                &stored,
                value.as_deref(),
            )
            .map(|()| true)
        });
        match written {
            Ok(true) => {
                voice |= route == Route::Voice;
                embedding |= route == Route::EmbeddingModel;
            }
            Ok(false) => {
                unapplied.moved.insert(key.clone());
            }
            Err(error) => {
                tracing::warn!(%error, key, "account: a synced setting could not be applied");
                unapplied.failed.insert(key.clone());
            }
        }
    }
    if voice {
        crate::voice_ipc::voice_settings_applied();
    }
    if embedding {
        crate::notes_ipc::embedding_model_applied();
    }
    unapplied
}

/// A pulled value this device will not take, whatever the merge said: a git
/// path this machine cannot drive (F12), or turning the at-rest encryption
/// off where it is on — only a person here may lower that.
fn refused_here(
    key: &str,
    value: Option<&str>,
    local: Option<&str>,
    git_usable: impl Fn(&str) -> bool,
) -> bool {
    match (key, value) {
        ("sync.git_path", Some(path)) => !path.trim().is_empty() && !git_usable(path),
        ("sdk_encryption", Some("off")) => local == Some("on"),
        _ => false,
    }
}

/// Whether `key`'s row changed since `snapshot` was read.
fn moved_since(
    data_dir: &Path,
    key: &str,
    snapshot: &BTreeMap<String, String>,
) -> Result<bool, CoreError> {
    let now = registry::stored_settings(data_dir, &[key])?;
    Ok(now.get(key) != snapshot.get(key))
}

/// Write the row, then drive the live state from what the registry now
/// answers — so a `keeper.toml` pin keeps winning in memory, and a deleted
/// row gives way to the pin or the default.
fn apply_one(
    app: Option<&AppHandle>,
    platform: &Arc<dyn Platform>,
    data_dir: &Path,
    runtime: &tokio::runtime::Handle,
    route: Route,
    key: &str,
    value: Option<&str>,
) -> Result<(), CoreError> {
    use tauri::Manager;

    let row = || registry::apply_synced_setting(data_dir, key, value);
    row()?;
    let state = app.map(|app| app.state::<crate::ipc::AppState>());
    let accounts = state.as_ref().map(|state| &state.accounts);
    // The facade setters persist what they are given as well as holding it;
    // given the resolved value, they would store a pin into the row, so the
    // pulled row is written back after them.
    match (route, accounts) {
        (Route::NotifyPreviews, Some(accounts)) => {
            accounts.notify_previews_set(platform, registry::get_notify_previews(data_dir)?)?;
            row()?;
        }
        (Route::Dnd, Some(accounts)) => {
            accounts.dnd_set(platform, registry::get_dnd_global(data_dir)?)?;
            row()?;
        }
        (Route::DockBadge, Some(accounts)) => {
            let mode = registry::get_dock_badge_mode(data_dir)?;
            runtime.block_on(accounts.dock_badge_mode_set(platform, mode))?;
            row()?;
        }
        (Route::MenuBar, _) => {
            let enabled = registry::get_menu_bar_presence(data_dir)?;
            #[cfg(desktop)]
            if let Some(app) = app {
                let handle = app.clone();
                if let Err(error) = app.run_on_main_thread(move || {
                    crate::tray::set_tray_presence(&handle, enabled);
                }) {
                    tracing::warn!(%error, "account: the menu-bar icon did not follow the synced setting");
                }
            }
            #[cfg(not(desktop))]
            let _ = enabled;
        }
        (Route::DebugMode, _) => {
            crate::debug_log::set_enabled(registry::get_debug_mode(data_dir)?);
        }
        (Route::LedgerVault, _) => {
            if let Some(engine) = crate::sync::engine_if_open() {
                if let Err(error) = engine.set_ledger_profile(registry::get_ledger_vault(data_dir)?)
                {
                    tracing::warn!(%error, "account: the engine kept its ledger choice");
                }
            }
        }
        // As `sync_git_path_set` does: forget the resolution and rebuild the
        // engine on the binary the pulled path names. Before the app is up,
        // the engine is built from the row written above.
        #[cfg(desktop)]
        (Route::GitPath, _) if app.is_some() => {
            crate::sync::repoint_engine(Arc::clone(platform));
        }
        // Before the app is up there is no live state to move yet: what it
        // is built from is the registry, written above. Voice and the
        // embedding model follow once per batch, in `apply`.
        _ => {}
    }
    Ok(())
}

/// The base to record for one settings file once the sync is over (AD-320,
/// with the fix-wave rulings): the merge's pushed base when the file's
/// write landed or none was needed, its not-pushed base otherwise; a key
/// whose apply failed records this device's own value `l`, and a key the
/// person changed meanwhile keeps its old base `b`.
pub(crate) fn settled_base(
    merged: &Merged,
    written: bool,
    local: &Values,
    base: Option<&Values>,
    unapplied: &Unapplied,
) -> Values {
    let mut settled = if written {
        merged.base_if_pushed.clone()
    } else {
        merged.base_if_not_pushed.clone()
    };
    let mut set = |key: &String, value: Option<&String>| match value {
        Some(value) => {
            settled.values.insert(key.clone(), value.clone());
        }
        None => {
            settled.values.remove(key);
        }
    };
    for key in &unapplied.failed {
        set(key, local.values.get(key));
    }
    for key in &unapplied.moved {
        set(key, base.and_then(|base| base.values.get(key)));
    }
    settled
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every key with live state beside its row goes through the path that
    /// moves it; a key keeper only reads per use is a plain write.
    #[test]
    fn keys_with_live_state_are_routed_to_their_setters() {
        for (key, expected) in [
            ("notify.previews_enabled", Route::NotifyPreviews),
            ("notify.dnd_global", Route::Dnd),
            ("notify.dock_badge_mode", Route::DockBadge),
            ("system.menu_bar_presence", Route::MenuBar),
            ("incognito.global", Route::Registry),
            ("bots.message_details", Route::Registry),
            ("debug.mode", Route::DebugMode),
            ("bots.wake_enabled", Route::Voice),
            ("bots.wake_phrase", Route::Voice),
            ("bots.stop_phrase", Route::Voice),
            ("bots.voice_locale", Route::Voice),
            ("notes.embedding_model", Route::EmbeddingModel),
            ("tasks.ledger_vault", Route::LedgerVault),
            ("sync.git_path", Route::GitPath),
            ("recording.codec", Route::Registry),
            ("undo_send.window", Route::Registry),
        ] {
            assert_eq!(route(key), expected, "{key}");
        }
    }

    /// The manifest carries the choice, in the order a person would expect:
    /// a drive bound to the account uses it even when an old token lingers.
    #[test]
    fn a_drive_credential_is_the_account_then_its_own_then_none() {
        assert_eq!(credential_choice(true, true), "account");
        assert_eq!(credential_choice(true, false), "account");
        assert_eq!(credential_choice(false, true), "own");
        assert_eq!(credential_choice(false, false), "none");
    }

    #[test]
    fn a_matrix_row_with_no_recorded_mechanism_is_a_password_login() {
        assert_eq!(matrix_kind(None), "password");
        assert_eq!(matrix_kind(Some("beeper")), "beeper");
    }

    /// Only the portable fields travel: the folder, direction and every
    /// other machine-local choice stay behind, and a role is present exactly
    /// when the profile has it on.
    #[test]
    fn a_drive_record_carries_the_portable_fields_and_the_roles_that_are_on() {
        let profile: SyncProfile = serde_json::from_value(serde_json::json!({
            "id": "01PROFILE",
            "name": "Notes",
            "localPath": "/Users/me/Notes",
            "remoteUrl": "https://git.example.org/me/notes.git",
            "branch": "main",
            "direction": "bidirectional",
            "lane": "main",
            "lfsMode": "materialize",
            "excludes": ["*.tmp"],
            "commitSubjectTemplate": "",
            "notes": { "subfolder": "vault" },
            "tasks": { "subfolder": "ledger" }
        }))
        .expect("profile");
        let remote = manifest::portable_remote(&profile.remote_url).expect("a network remote");
        let record = drive_record(&profile, remote, "own");
        assert_eq!(record.name, "Notes");
        assert_eq!(record.remote_url, "https://git.example.org/me/notes");
        assert_eq!(record.credential, "own");
        assert_eq!(record.notes.as_deref(), Some("vault"));
        assert_eq!(record.tasks.as_deref(), Some("ledger"));
        assert_eq!(record.recordings, None);
        assert_eq!(record.sessions, None);
        assert_eq!(record.excludes, vec!["*.tmp".to_owned()]);
        assert_eq!(record.commit_subject_template, None);
        assert!(record.devices.is_empty());
    }

    fn values(pairs: &[(&str, &str)]) -> Values {
        Values {
            values: pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            ..Values::default()
        }
    }

    fn merged(pushed: &[(&str, &str)], kept: &[(&str, &str)]) -> Merged {
        Merged {
            file: Values::default(),
            apply: Vec::new(),
            changed: false,
            base_if_pushed: values(pushed),
            base_if_not_pushed: values(kept),
        }
    }

    /// A file whose write landed (or needed none) records the pushed base;
    /// one whose write did not records the base that keeps local changes
    /// dirty.
    #[test]
    fn the_base_follows_whether_the_files_write_landed() {
        let merged = merged(&[("a", "pushed")], &[("a", "kept")]);
        let none = Unapplied::default();
        let local = Values::default();
        assert_eq!(
            settled_base(&merged, true, &local, None, &none),
            values(&[("a", "pushed")])
        );
        assert_eq!(
            settled_base(&merged, false, &local, None, &none),
            values(&[("a", "kept")])
        );
    }

    /// A pulled value that failed to apply is not recorded as synced: the
    /// base holds this device's value, so the next sync does not push the
    /// old row over the other device's choice.
    #[test]
    fn a_failed_apply_records_the_local_value() {
        let merged = merged(&[("a", "remote"), ("b", "remote")], &[]);
        let unapplied = Unapplied {
            failed: ["a".to_owned(), "b".to_owned()].into(),
            moved: BTreeSet::new(),
        };
        let local = values(&[("a", "local")]);
        assert_eq!(
            settled_base(&merged, true, &local, None, &unapplied),
            values(&[("a", "local")])
        );
    }

    /// A key the person changed during the sync keeps its old base, so the
    /// change reads as local and is pushed by the re-run.
    #[test]
    fn a_key_changed_meanwhile_keeps_its_old_base() {
        let merged = merged(&[("a", "remote"), ("b", "remote")], &[]);
        let unapplied = Unapplied {
            failed: BTreeSet::new(),
            moved: ["a".to_owned(), "b".to_owned()].into(),
        };
        let base = values(&[("a", "old")]);
        assert_eq!(
            settled_base(&merged, true, &Values::default(), Some(&base), &unapplied),
            values(&[("a", "old")])
        );
    }
}
#[cfg(test)]
mod refused_tests {
    use super::refused_here;

    /// F12: a git path this machine cannot drive, and encryption turned off
    /// over this device's "on", are never applied; everything else is.
    #[test]
    fn a_pulled_value_this_device_cannot_take_is_refused() {
        let usable = |path: &str| path == "/usr/bin/git";
        assert!(refused_here(
            "sync.git_path",
            Some("/opt/nope/git"),
            None,
            usable
        ));
        assert!(!refused_here(
            "sync.git_path",
            Some("/usr/bin/git"),
            None,
            usable
        ));
        assert!(!refused_here("sync.git_path", Some(""), None, usable));
        assert!(refused_here(
            "sdk_encryption",
            Some("off"),
            Some("on"),
            usable
        ));
        assert!(!refused_here(
            "sdk_encryption",
            Some("off"),
            Some("off"),
            usable
        ));
        assert!(!refused_here(
            "sdk_encryption",
            Some("on"),
            Some("off"),
            usable
        ));
        assert!(!refused_here("debug.mode", Some("1"), None, usable));
    }
}
