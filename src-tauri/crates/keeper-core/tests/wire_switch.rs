use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use keeper_core::config::keys::{layer_may_set, RefusalReason};
use keeper_core::config::{load_app_layers, parse_layer_file, LayerFaultKind, LayerTier};
use keeper_core::org_account::layout::{DeviceClass, RepoFiles};
use keeper_core::org_account::settings_sync::{
    self, from_portable, local_values, merge, seed_device, seed_shared, synced_file, Catalog,
    SyncedFile, Values,
};
use keeper_core::registry::{self, apply_synced_setting};

const WIRE_ENABLED_KEY: &str = "wire.enabled";
const NEVER_WHY: &str =
    "keeper's local socket is switched on only on this device, by you; no file can switch it on";

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

fn temp_dir(label: &str) -> PathBuf {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "keeper-wire-switch-{label}-{}-{id}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

struct MemoryRepo(BTreeMap<String, Vec<u8>>);

impl MemoryRepo {
    fn from_entries(entries: &[(&str, &str)]) -> Self {
        Self(
            entries
                .iter()
                .map(|(k, v)| ((*k).to_owned(), v.as_bytes().to_vec()))
                .collect(),
        )
    }
}

impl RepoFiles for MemoryRepo {
    fn read(&self, rel: &str) -> Option<Vec<u8>> {
        self.0.get(rel).cloned()
    }

    fn list_dir(&self, rel: &str) -> Vec<String> {
        let prefix = if rel.is_empty() {
            String::new()
        } else {
            format!("{}/", rel.trim_end_matches('/'))
        };
        let mut names = BTreeSet::new();
        for path in self.0.keys() {
            if let Some(rest) = path.strip_prefix(&prefix) {
                if let Some(head) = rest.split('/').next() {
                    if !head.is_empty() {
                        names.insert(head.to_owned());
                    }
                }
            }
        }
        names.into_iter().collect()
    }
}

#[test]
fn synced_file_is_none_for_wire_enabled() {
    assert_eq!(synced_file(WIRE_ENABLED_KEY), None);
}

#[test]
fn merge_ignores_wire_enabled_across_remote_files_and_seeds() {
    let data_dir = temp_dir("merge");
    registry::set_setting(&data_dir, WIRE_ENABLED_KEY, "1").expect("set local wire.enabled");
    let catalog = Catalog::default();
    let resolve = |k: &str, v: &str| from_portable(k, v, &catalog);

    // Local side for both synced files must never pick up this device's wire.enabled.
    let local_device = local_values(&data_dir, SyncedFile::Device, &catalog, None)
        .expect("read local device values");
    let local_shared = local_values(&data_dir, SyncedFile::Shared, &catalog, None)
        .expect("read local shared values");
    assert!(!local_device.values.contains_key(WIRE_ENABLED_KEY));
    assert!(!local_shared.values.contains_key(WIRE_ENABLED_KEY));

    // Writing back with no remote file carries none of this device's value for wire.enabled.
    for (label, local) in [("device", &local_device), ("shared", &local_shared)] {
        let outbound = merge(None, None, local, None, &resolve);
        assert!(
            !outbound.file.values.contains_key(WIRE_ENABLED_KEY),
            "{label}: outbound file values must not carry wire.enabled"
        );
        let rendered = outbound.file.render().expect("render outbound file");
        assert!(
            !rendered.contains(WIRE_ENABLED_KEY),
            "{label}: rendered outbound file must not mention wire.enabled:\n{rendered}"
        );
    }

    let remote_bytes = b"[settings]\n\"wire.enabled\" = 1\n";
    let repo = MemoryRepo::from_entries(&[
        (
            &settings_sync::device_path("alice", "workstation"),
            "[settings]\n\"wire.enabled\" = 1\n",
        ),
        (
            &settings_sync::shared_path("alice"),
            "[settings]\n\"wire.enabled\" = 1\n",
        ),
        (
            "_template/settings/desktop.toml",
            "[settings]\n\"wire.enabled\" = 1\n",
        ),
        (
            "_template/settings.toml",
            "[settings]\n\"wire.enabled\" = 1\n",
        ),
    ]);

    // 1. Remote <login>/settings.<device>.toml and <login>/settings.toml.
    let device_remote = Values::parse(
        &repo
            .read(&settings_sync::device_path("alice", "workstation"))
            .expect("device file"),
        SyncedFile::Device,
    )
    .expect("parse device file");
    let shared_remote = Values::parse(
        &repo
            .read(&settings_sync::shared_path("alice"))
            .expect("shared file"),
        SyncedFile::Shared,
    )
    .expect("parse shared file");

    for (label, remote, local) in [
        ("settings.<device>.toml", &device_remote, &local_device),
        ("settings.toml", &shared_remote, &local_shared),
    ] {
        let merged = merge(Some(remote), None, local, None, &resolve);
        assert!(
            !merged.apply.iter().any(|(k, _)| k == WIRE_ENABLED_KEY),
            "{label}: merge must apply nothing for wire.enabled, got {:?}",
            merged.apply
        );
        assert!(
            !merged.file.values.contains_key(WIRE_ENABLED_KEY),
            "{label}: merged file values must not hold wire.enabled"
        );
        let rendered = merged.file.render().expect("render merged file");
        assert!(
            rendered.contains("\"wire.enabled\" = 1"),
            "{label}: unapplied remote line must stay in written text:\n{rendered}"
        );
    }

    // 2. Seeds: _template/settings/<class>.toml and _template/settings.toml.
    let device_seed = seed_device(&repo, "alice", DeviceClass::Desktop, "workstation", &|_| {
        None
    })
    .expect("device class template seed");
    let shared_seed = seed_shared(&repo).expect("shared template seed");

    for (label, seed, local) in [
        (
            "_template/settings/desktop.toml",
            device_seed,
            &local_device,
        ),
        ("_template/settings.toml", shared_seed, &local_shared),
    ] {
        let merged = merge(None, Some(seed), local, None, &resolve);
        assert!(
            !merged.apply.iter().any(|(k, _)| k == WIRE_ENABLED_KEY),
            "{label}: seeded merge must apply nothing for wire.enabled, got {:?}",
            merged.apply
        );
        assert!(
            !merged.file.values.contains_key(WIRE_ENABLED_KEY),
            "{label}: seeded file values must not hold wire.enabled"
        );
        let rendered = merged.file.render().expect("render seeded file");
        assert!(
            rendered.contains("\"wire.enabled\" = 1"),
            "{label}: unapplied template line must stay in written text:\n{rendered}"
        );
    }

    // Sanity check that real bytes with remote_bytes match parse behavior.
    let parsed = Values::parse(remote_bytes, SyncedFile::Device).expect("parse raw bytes");
    assert!(!parsed.values.contains_key(WIRE_ENABLED_KEY));
    assert!(parsed.preserved.contains_key(WIRE_ENABLED_KEY));

    let _ = std::fs::remove_dir_all(&data_dir);
}

#[test]
fn apply_synced_setting_refuses_wire_enabled() {
    let data_dir = temp_dir("apply-synced");

    let err = apply_synced_setting(&data_dir, WIRE_ENABLED_KEY, Some("1"))
        .expect_err("apply_synced_setting must refuse setting wire.enabled");
    assert!(
        err.to_string().contains(WIRE_ENABLED_KEY),
        "unexpected refusal error: {err}"
    );

    let delete_err = apply_synced_setting(&data_dir, WIRE_ENABLED_KEY, None)
        .expect_err("apply_synced_setting must refuse deleting wire.enabled");
    assert!(
        delete_err.to_string().contains(WIRE_ENABLED_KEY),
        "unexpected delete refusal error: {delete_err}"
    );

    let stored = registry::stored_settings(&data_dir, &[WIRE_ENABLED_KEY]).expect("read settings");
    assert!(
        !stored.contains_key(WIRE_ENABLED_KEY),
        "refused apply_synced_setting must not write wire.enabled"
    );

    let _ = std::fs::remove_dir_all(&data_dir);
}

#[test]
fn layer_files_refuse_wire_enabled_at_every_tier() {
    // Direct layer_may_set check for both shared and machine-scoped tiers.
    for tier in LayerTier::ORDER {
        assert_eq!(
            layer_may_set(WIRE_ENABLED_KEY, tier.machine_scoped()),
            Err(RefusalReason::NotAPreference {
                key: WIRE_ENABLED_KEY.to_owned(),
                why: NEVER_WHY,
            }),
            "layer_may_set must refuse wire.enabled at {tier:?}"
        );
    }

    // parse_layer_file across every LayerTier.
    let toml_body = "[settings]\n\"wire.enabled\" = 1\n";
    for tier in LayerTier::ORDER {
        let folder = match tier {
            LayerTier::MainShared
            | LayerTier::MainMachine
            | LayerTier::FolderShared
            | LayerTier::FolderMachine => Some("/sync/folder"),
            _ => None,
        };
        let parsed = parse_layer_file(Path::new("/tmp/keeper.toml"), tier, folder, toml_body);
        assert!(
            !parsed.settings.contains_key(WIRE_ENABLED_KEY),
            "{tier:?}: wire.enabled must not be accepted into layer settings"
        );
        match tier {
            LayerTier::FolderShared | LayerTier::FolderMachine => {
                assert!(
                    parsed
                        .faults
                        .iter()
                        .any(|f| f.kind == LayerFaultKind::SettingsInNonMainFolder),
                    "{tier:?}: expected SettingsInNonMainFolder fault, got {:?}",
                    parsed.faults
                );
            }
            _ => {
                let fault = parsed
                    .faults
                    .iter()
                    .find(|f| {
                        f.kind == LayerFaultKind::KeyRefused
                            && f.key.as_deref() == Some(WIRE_ENABLED_KEY)
                    })
                    .unwrap_or_else(|| {
                        panic!(
                            "{tier:?}: expected KeyRefused for wire.enabled, got {:?}",
                            parsed.faults
                        )
                    });
                assert!(
                    fault.message.contains(NEVER_WHY),
                    "{tier:?}: refusal message must carry the Never sentence, got: {}",
                    fault.message
                );
            }
        }
    }

    // End-to-end load_app_layers: ~/.keeper/keeper.toml, ~/.keeper/keeper.<host>.toml,
    // and the synced main folder's keeper.toml + keeper.<host>.toml.
    let root = temp_dir("layers");
    let home = root.join("home");
    let main_folder = root.join("main-sync");
    std::fs::create_dir_all(home.join(".keeper")).expect("create ~/.keeper");
    std::fs::create_dir_all(main_folder.join(".keeper")).expect("create main/.keeper");

    let host = "workstation";
    let user_global_body = format!(
        "mainSyncFolder = {:?}\n[settings]\n\"wire.enabled\" = 1\n",
        main_folder.display().to_string()
    );
    std::fs::write(home.join(".keeper/keeper.toml"), user_global_body).expect("write user global");
    std::fs::write(home.join(format!(".keeper/keeper.{host}.toml")), toml_body)
        .expect("write user machine");
    std::fs::write(main_folder.join(".keeper/keeper.toml"), toml_body).expect("write main shared");
    std::fs::write(
        main_folder.join(format!(".keeper/keeper.{host}.toml")),
        toml_body,
    )
    .expect("write main machine");

    let layers = load_app_layers(&home, host);
    assert!(
        !layers.overrides.contains_key(WIRE_ENABLED_KEY),
        "load_app_layers must not override wire.enabled"
    );

    for expected_tier in [
        LayerTier::UserGlobal,
        LayerTier::UserGlobalMachine,
        LayerTier::MainShared,
        LayerTier::MainMachine,
    ] {
        let fault = layers
            .faults
            .iter()
            .find(|f| {
                f.tier == Some(expected_tier)
                    && f.kind == LayerFaultKind::KeyRefused
                    && f.key.as_deref() == Some(WIRE_ENABLED_KEY)
            })
            .unwrap_or_else(|| {
                panic!(
                    "{expected_tier:?}: expected KeyRefused in load_app_layers, got {:?}",
                    layers.faults
                )
            });
        assert!(
            fault.message.contains(NEVER_WHY),
            "{expected_tier:?}: refusal message must carry the Never sentence, got: {}",
            fault.message
        );
    }

    let _ = std::fs::remove_dir_all(&root);
}
