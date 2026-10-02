//! agentd's own engine: one profile per `[[drives]]`, behind the mount rule
//! (AD-376, AD-377; C8, R7, S-15).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use keeper_core::agents::agentd::AgentdConfig;
use keeper_core::agents::drive::{self, DriveDecl};
use keeper_core::agents::log::writer::rotate_at;
use keeper_core::agents::mount::{self, MountRefusal};
use keeper_core::agents::zone::NO_DRIVE;
use keeper_sync::engine::Engine;
use keeper_sync::lfs::virtual_policy::{VirtualPolicy, Virtualization};
use keeper_sync::profile::folder::{install_folder_tier, FolderTier};
use keeper_sync::profile::{AgentsConfig, LfsMode};
use keeper_sync::{SyncDirection, SyncPlatform, SyncProfile};

use super::sync_platform::HeadlessSyncPlatform;
use super::{HeadlessError, DRIVES_DIR, MARKER_FILE};
use crate::turn::new_id;

/// A drive this host mounts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountedDrive {
    /// `[[drives]].id`.
    pub id: String,
    pub profile_id: String,
    pub local_path: PathBuf,
}

/// agentd's engine and the profiles it made from `agentd.toml`.
pub struct AgentdEngine {
    pub engine: Arc<Engine>,
    pub drives: Vec<MountedDrive>,
}

/// Whether one drive's agents zone hosts, after its checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoneVerdict {
    pub drive: String,
    /// The declaration that matched its pin, or the sentence saying why the
    /// zone hosts nothing — which `status` and `agents list` print.
    pub hosts: Result<DriveDecl, String>,
}

/// The mount rule on the pins (C8): every mounted drive's pinned readers
/// include every reader of every drive this host homes agents in. Nothing is
/// fetched before it holds.
pub fn enforce_mounts(config: &AgentdConfig) -> Result<(), MountRefusal> {
    let mounted: Vec<_> = config
        .drives
        .iter()
        .map(|pin| (pin.id.clone(), pin.readers()))
        .collect();
    let homes: Vec<String> = config
        .home_drives()
        .into_iter()
        .map(str::to_owned)
        .collect();
    mount::check(&mounted, &homes)
}

/// Open agentd's own engine and give it one profile per `[[drives]]`.
///
/// In this order, each refusal leaving nothing behind it:
/// 1. the mount rule on the pins — a violating drive is never fetched;
/// 2. `sync.db` only beside the `.keeper-agentd` marker (R7): another engine's
///    database is never opened, because opening one requeues its running work;
/// 3. the folder tier armed with this host's slug;
/// 4. every profile the file no longer names removed — the mount rule holds
///    over what `sync.db` mounts, not only over the file — before any sync;
/// 5. a profile per drive, found again by its id (a moved `remote` updates
///    it in place), checked out under `drives/<id>/`, both directions,
///    materialised, `[folder.agents]` and `[folder.sessions]` armed, its
///    credential bound to its `secret:<name>`.
///
/// A removed drive's checkout stays on disk for the operator to delete.
/// The first checkout is the caller's `sync_once` (or the engine's run).
pub fn open_engine(
    config: &AgentdConfig,
    platform: Arc<HeadlessSyncPlatform>,
) -> Result<AgentdEngine, HeadlessError> {
    enforce_mounts(config)?;

    let data = platform.data_dir()?;
    std::fs::create_dir_all(&data)
        .map_err(|err| keeper_sync::SyncError::io("create data directory", &data, err))?;
    let database = data.join(keeper_sync::db::DB_FILE_NAME);
    let marker = data.join(MARKER_FILE);
    if !marker.exists() {
        if database.exists() {
            return Err(HeadlessError::ForeignDatabase { path: database });
        }
        std::fs::write(&marker, b"keeper-agentd\n")
            .map_err(|err| keeper_sync::SyncError::io("write the agentd marker", &marker, err))?;
    }

    install_folder_tier(FolderTier::new(config.host.clone(), None));
    let engine = Arc::new(Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>)?);

    // The checkouts' parent: a first clone creates its own folder, not the
    // directory above it.
    let drives_dir = data.join(DRIVES_DIR);
    std::fs::create_dir_all(&drives_dir)
        .map_err(|err| keeper_sync::SyncError::io("create drives directory", &drives_dir, err))?;
    let existing = engine.list_profiles()?;
    // Keep the first row named after each pinned drive; every other row is a
    // drive this host may no longer mount (or a second checkout of one).
    let mut kept: Vec<SyncProfile> = Vec::with_capacity(config.drives.len());
    for row in existing {
        if config.drive(&row.name).is_some() && !kept.iter().any(|k| k.name == row.name) {
            kept.push(row);
        } else {
            tracing::warn!(
                profile = %row.id,
                drive = %row.name,
                "agentd.toml no longer names this drive; its profile is removed and its checkout left on disk"
            );
            engine.remove_profile(&row.id)?;
        }
    }
    let mut drives = Vec::with_capacity(config.drives.len());
    for pin in &config.drives {
        let local_path = data.join(DRIVES_DIR).join(&pin.id);
        let mut profile = match kept.iter().position(|row| row.name == pin.id) {
            Some(at) => kept.swap_remove(at),
            None => SyncProfile::new(new_id(), pin.id.clone(), &local_path, pin.remote.clone()),
        };
        profile.remote_url = pin.remote.clone();
        profile.local_path = local_path.clone();
        profile.direction = SyncDirection::Bidirectional;
        profile.lfs_mode = LfsMode::Materialize;
        profile.sessions = Some(profile.sessions.take().unwrap_or_default());
        profile.agents = Some(profile.agents.take().unwrap_or_else(AgentsConfig::default));
        if let Some(credential) = &pin.credential {
            platform
                .secrets()
                .bind(profile.secret_key(), credential.name());
        }
        engine.upsert_profile(&profile)?;
        drives.push(MountedDrive {
            id: pin.id.clone(),
            profile_id: profile.id,
            local_path,
        });
    }
    Ok(AgentdEngine { engine, drives })
}

/// After a checkout: whether each mounted drive's agents zone hosts.
///
/// A zone hosts nothing when a virtual pattern would leave its agents or
/// sessions zone as pointers (AD-376), when it has no valid `_drive.toml`
/// (89.2), or when its declaration differs from the pin (S-15) — the pin is
/// what this host believes, and the file never widens it.
pub fn zone_verdicts(
    config: &AgentdConfig,
    agentd: &AgentdEngine,
) -> Result<Vec<ZoneVerdict>, HeadlessError> {
    let profiles = agentd.engine.list_profiles()?;
    let mut verdicts = Vec::with_capacity(agentd.drives.len());
    for mounted in &agentd.drives {
        let Some(profile) = profiles.iter().find(|row| row.id == mounted.profile_id) else {
            continue;
        };
        let Some(pin) = config.drive(&mounted.id) else {
            continue;
        };
        let hosts = virtual_refusal(profile)
            .map_or_else(|| read_declaration(profile), Err)
            .and_then(|decl| {
                mount::pin_matches(&decl, pin)
                    .map(|()| decl)
                    .map_err(|difference| difference.sentence())
            });
        verdicts.push(ZoneVerdict {
            drive: mounted.id.clone(),
            hosts,
        });
    }
    Ok(verdicts)
}

fn read_declaration(profile: &SyncProfile) -> Result<DriveDecl, String> {
    let Some(zone) = profile.agents_root() else {
        return Err(NO_DRIVE.to_owned());
    };
    let path = keeper_sync::browse::resolve(&zone, drive::FILE_NAME)
        .map_err(|refusal| refusal.to_string())?
        .ok_or_else(|| NO_DRIVE.to_owned())?;
    let text = std::fs::read_to_string(&path)
        .map_err(|err| format!("_drive.toml could not be read: {err}"))?;
    drive::parse(&text).map_err(|refusal| {
        format!(
            "This agents zone's _drive.toml is refused, so it hosts no agent. {}",
            refusal.sentence()
        )
    })
}

/// The sentence naming a virtual pattern that would leave the agents or the
/// sessions zone as pointers, or `None` when both materialise.
///
/// Each pattern is tried on its own so the sentence names the one that bites;
/// then the whole policy in force (the committed `.keepervirtual` and the size
/// floor included) is tried, so nothing that virtualises a zone goes unnamed.
/// The floor is judged at the largest file a zone must hold as bytes — a log
/// chunk, which rotates before 192 KiB (R3) — so a floor above that leaves the
/// zone whole; a log blob above it may be a pointer, which replay hydrates.
fn virtual_refusal(profile: &SyncProfile) -> Option<String> {
    let zones: Vec<(&str, String)> = [
        (
            "agents",
            profile
                .agents
                .as_ref()
                .map(|z| z.subfolder.trim().to_owned()),
        ),
        (
            "sessions",
            profile
                .sessions
                .as_ref()
                .map(|z| z.subfolder.trim().to_owned()),
        ),
    ]
    .into_iter()
    .filter_map(|(name, sub)| sub.map(|sub| (name, sub)))
    .collect();
    let largest = rotate_at(u64::MAX) - 1;
    let covers = |candidate: &SyncProfile, zone: &str| {
        VirtualPolicy::compile(candidate).is_ok_and(|policy| {
            policy.resolve(&Path::new(zone).join("x.md"), largest) == Virtualization::Virtual
        })
    };
    for pattern in &profile.virtual_patterns {
        let mut alone = profile.clone();
        alone.virtual_patterns = vec![pattern.clone()];
        alone.virtual_over_bytes = 0;
        for (name, zone) in &zones {
            if covers(&alone, zone) {
                return Some(format!(
                    "The virtual pattern \"{pattern}\" would leave the {name} zone ({zone}/) as pointers, \
                     so this drive hosts no agent here; keeper-agentd must read every file of it. Remove the pattern."
                ));
            }
        }
    }
    for (name, zone) in &zones {
        if covers(profile, zone) {
            return Some(format!(
                "This folder's virtual policy (.keepervirtual or its size floor) would leave the {name} zone \
                 ({zone}/) as pointers, so this drive hosts no agent here; keeper-agentd must read every file of it."
            ));
        }
    }
    None
}
