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

use keeper_core::bots::store;
use keeper_core::error::CoreError;
use keeper_core::org_account::manifest::{
    self, BotRecord, DriveRecord, MatrixRecord, ProviderRecord,
};
use keeper_core::org_account::settings_sync::{Catalog, DriveRef, Merged, Values};
use keeper_core::platform::Platform;
use keeper_core::registry;
use keeper_sync::SyncProfile;
use tauri::AppHandle;

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
}

/// Read [`Mine`] for the configured account `account_id`.
pub(crate) fn gather(
    platform: &Arc<dyn Platform>,
    data_dir: &Path,
    account_id: &str,
) -> Result<Mine, CoreError> {
    let profiles = match crate::sync::engine(Arc::clone(platform))
        .and_then(|engine| engine.list_profiles())
    {
        Ok(profiles) => Some(profiles),
        Err(error) => {
            tracing::warn!(%error, "account: the drives could not be listed for the settings sync");
            None
        }
    };
    let (drive_refs, drives) = match profiles {
        Some(profiles) => {
            let mut refs = Vec::with_capacity(profiles.len());
            let mut records = Vec::with_capacity(profiles.len());
            for profile in &profiles {
                // Every drive translates the settings that name it here,
                // even one whose remote cannot travel.
                refs.push((
                    profile.id.clone(),
                    DriveRef::new(&profile.remote_url, &profile.branch),
                ));
                let Some(remote_url) = manifest::portable_remote(&profile.remote_url) else {
                    continue;
                };
                let bound =
                    registry::get_sync_credential_source(data_dir, &profile.id, Some(account_id))?
                        .is_some();
                // Presence only: the value is never read past this line.
                let own = platform
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
    let catalog = Catalog::with_bots(data_dir, drive_refs)?;

    let listing = store::list_providers(data_dir)?;
    let mut providers = Vec::with_capacity(listing.rows.len());
    for row in &listing.rows {
        let provider = &row.provider;
        let bound = registry::get_bots_provider_credential_source(
            data_dir,
            &provider.id,
            Some(account_id),
        )?
        .is_some();
        let bots = store::list_bots_for_provider(data_dir, &provider.id)?;
        providers.push(ProviderRecord {
            kind: provider.kind.as_registry_str().to_owned(),
            name: provider.name.clone(),
            base_url: provider.base_url.clone(),
            credential: if bound { "account" } else { "own" }.to_owned(),
            read_timeout_ms: row.read_timeout_ms.and_then(|ms| u64::try_from(ms).ok()),
            bots: bots
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
                .collect(),
            devices: Vec::new(),
            extra: BTreeMap::new(),
        });
    }

    let matrix = registry::list_accounts(data_dir)?
        .into_iter()
        .map(|row| MatrixRecord {
            user_id: row.user_id,
            homeserver_url: row.homeserver_url,
            kind: matrix_kind(row.provider.as_deref()).to_owned(),
            devices: Vec::new(),
            extra: BTreeMap::new(),
        })
        .collect();

    Ok(Mine {
        catalog,
        drives,
        providers,
        matrix,
    })
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
/// `snapshot` is the rows the merge's local side was read from; a key whose
/// row no longer holds that value was changed here meanwhile and is skipped.
/// The caller runs this under `registry::with_observer_suppressed`, on a
/// thread that may block (the dock badge's setter is async and is driven
/// here on `runtime`).
pub(crate) fn apply(
    app: Option<&AppHandle>,
    platform: &Arc<dyn Platform>,
    data_dir: &Path,
    runtime: &tokio::runtime::Handle,
    changes: &[(String, Option<String>)],
    snapshot: &BTreeMap<String, String>,
) -> Unapplied {
    let mut unapplied = Unapplied::default();
    let mut voice = false;
    let mut embedding = false;
    for (key, value) in changes {
        let route = route(key);
        let written = moved_since(data_dir, key, snapshot).and_then(|moved| {
            if moved {
                return Ok(false);
            }
            apply_one(
                app,
                platform,
                data_dir,
                runtime,
                route,
                key,
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
