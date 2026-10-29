//! The shell's [`DriveWrites`] (109.27, contract § 5): where this device
//! keeps a drive, what its checkout holds at a path, the one authored
//! commit that carries an approved `drive_write` out as the person, and the
//! trailer search a relaunch settles by.
//!
//! Decides nothing: every order, check and sentence is
//! `keeper_core::hq::approvals::write`'s. A path is checked only by
//! keeper-sync's `agent_write_check` with an agent host's scope — sessions
//! closed, agents fenced, the profile's own exclusions — and committed only
//! by `Engine::commit_paths` over the keys it answered (AD-65, AD-159).

use std::path::Path;
use std::sync::Arc;

use keeper_core::agents::drive_write::{Refusal, MAX_FILE_BYTES};
use keeper_core::hq::port::{
    DriveAbsent, DriveCommit, DriveCommitted, DriveFile, DriveTarget, DriveWrites,
};
use keeper_sync::engine::{blob_id, CommitFence, CommitPaths, CommitRequest, Engine};
use keeper_sync::exclude::ExcludeSet;
use keeper_sync::files_write::{
    agent_apply_check, agent_write_check, AgentTarget, WriteRefusal, WriteRoute, WriteScope,
};
use keeper_sync::lfs::stage::LfsPolicy;
use keeper_sync::volume::{self, VolumeStatus};
use keeper_sync::{MemoryTrailer, SyncProfile};

/// The open sync engine, if any.
type EngineOf = Box<dyn Fn() -> Option<Arc<Engine>> + Send + Sync>;

/// The notes vault a profile keeps, by profile id.
type VaultOf = Box<dyn Fn(&str) -> Option<crate::notes_vault::Vault> + Send + Sync>;

/// This device's drives, over the sync engine `engine` answers (the open
/// one, in the app).
pub struct ShellDriveWrites {
    engine: EngineOf,
    /// The notes vault kept in a profile, by profile id: told of every
    /// note a write changes, as a save from Files tells it.
    vault: VaultOf,
}

impl ShellDriveWrites {
    /// Over the app's own engine and notes vaults.
    pub fn app() -> ShellDriveWrites {
        ShellDriveWrites {
            engine: Box::new(crate::sync::engine_if_open),
            vault: Box::new(crate::notes_vault::vault),
        }
    }

    /// The synced folder declaring `drive` in its `_drive.toml`.
    fn profile(&self, drive: &str) -> Option<(Arc<Engine>, SyncProfile)> {
        let engine = (self.engine)()?;
        let profiles = engine.list_profiles().ok()?;
        let profile = profiles.into_iter().find(|profile| {
            keeper_agent::surface::declared(profile).is_ok_and(|decl| decl.id == drive)
        })?;
        Some((engine, profile))
    }

    /// The apply-time check (contract § 5.1 as amended by Fix603), right
    /// before `commit_paths`: keeper-sync's `agent_apply_check` over every
    /// path `commit` changes — each guarded path, Existing if it was read
    /// and New if not, with the exact bytes the commit puts there, and a
    /// move's destination — so a `.gitattributes` that began routing a path
    /// through LFS since the card was drawn, a link, an ignore or a fence
    /// that appeared, or bytes that are themselves a pointer, refuse the
    /// write. And what `commit_paths` would route by size — the profile's
    /// LFS threshold over the post-image, which `agent_apply_check` does not
    /// ask — is refused too: no agent write ever makes an LFS object.
    fn recheck(&self, profile: &SyncProfile, commit: &DriveCommit) -> Result<(), String> {
        let vault = (self.vault)(&profile.id).map(|vault| vault.config.subfolder.clone());
        let scope = scope(profile, vault.as_deref());
        let excludes = ExcludeSet::new(&profile.excludes).map_err(|error| error.to_string())?;
        let said = |refused: WriteRefusal, path: &str| {
            let said = refusal(refused, path).to_string();
            said.strip_suffix(" Nothing was asked.")
                .map_or(said.clone(), str::to_owned)
        };
        let bytes_at = |key: &str| {
            commit
                .writes
                .iter()
                .find(|(path, _)| path == key)
                .and_then(|(_, bytes)| bytes.as_deref())
        };
        let guards = commit.guards.iter().map(|(key, read)| {
            let target = match read {
                Some(_) => AgentTarget::Existing,
                None => AgentTarget::New,
            };
            (key.as_str(), target, bytes_at(key))
        });
        let moved_to = commit
            .moves
            .iter()
            .map(|(_, to)| (to.as_str(), AgentTarget::New, None));
        for (key, target, post_image) in guards.chain(moved_to) {
            agent_apply_check(
                &profile.local_path,
                &scope,
                &excludes,
                key,
                target,
                post_image,
            )
            .map_err(|refused| said(refused, key))?;
            // No op changes a file over 1 MB (R606-10), checked again on the
            // disk right before the commit.
            if target == AgentTarget::Existing {
                let size = keeper_sync::browse::resolve(&profile.local_path, key)
                    .ok()
                    .flatten()
                    .and_then(|file| std::fs::metadata(file).ok())
                    .map_or(0, |meta| meta.len());
                if size > MAX_FILE_BYTES as u64 {
                    return Err(format!(
                        "\"{key}\" is over 1 MB; agents can't change files that large."
                    ));
                }
            }
        }
        let policy = LfsPolicy::from_profile(profile).map_err(|error| error.to_string())?;
        for (key, bytes) in &commit.writes {
            let routed = bytes
                .as_ref()
                .is_some_and(|bytes| policy.applies(Path::new(key), bytes.len() as u64));
            if routed {
                return Err(WriteRefusal::LfsRouted {
                    subpath: key.clone(),
                }
                .to_string());
            }
        }
        Ok(())
    }

    /// The vault-relative path of `key`, where it is one of the vault's
    /// notes and the file is on the disk now.
    fn note(&self, profile: &SyncProfile, key: &str) -> Option<(String, String)> {
        let vault = (self.vault)(&profile.id)?;
        let subfolder = vault.config.subfolder.clone();
        match scope(profile, Some(subfolder.as_str())).route(Some(()), &profile.local_path, key) {
            Ok(WriteRoute::Vault { path, .. }) => Some((vault.id, path.as_str().to_owned())),
            _ => None,
        }
    }
}

/// An agent host's scope over `profile` (contract § 5.1): its vault, its
/// sessions zone closed, its agents zone fenced.
fn scope<'p>(profile: &'p SyncProfile, vault: Option<&str>) -> WriteScope<'p> {
    WriteScope::new(&profile.name, vault)
        .with_sessions(profile.sessions.as_ref().map(|s| s.subfolder.as_str()))
        .with_sessions_closed(true)
        .with_agents(profile.agents.as_ref().map(|a| a.subfolder.as_str()))
}

/// keeper-sync's answer as the request reads it (contract § 7.3, both
/// sides alike).
fn refusal(refused: WriteRefusal, path: &str) -> Refusal {
    match refused {
        WriteRefusal::Missing { .. } => Refusal::Absent {
            path: path.to_owned(),
        },
        WriteRefusal::Exists { .. } => Refusal::Exists {
            path: path.to_owned(),
        },
        WriteRefusal::IsDirectory { .. } => Refusal::Folder {
            path: path.to_owned(),
        },
        other => Refusal::Path {
            sentence: other.to_string(),
        },
    }
}

impl DriveWrites for ShellDriveWrites {
    fn keeps(&self, drive: &str) -> Result<(), DriveAbsent> {
        let (_, profile) = self.profile(drive).ok_or(DriveAbsent::NotKept)?;
        if !profile.direction.pushes() {
            return Err(DriveAbsent::PullsOnly);
        }
        let attached = !profile.removable
            || matches!(
                volume::scan(&profile.local_path, profile.volume_id.as_deref()),
                Ok(VolumeStatus::Present { .. })
            );
        if !attached {
            return Err(DriveAbsent::NotAttached);
        }
        Ok(())
    }

    fn target(&self, drive: &str, path: &str, target: DriveTarget) -> Result<DriveFile, Refusal> {
        let (_, profile) = self.profile(drive).ok_or_else(|| Refusal::Path {
            sentence: format!("This device does not keep {drive}."),
        })?;
        let vault = (self.vault)(&profile.id).map(|vault| vault.config.subfolder.clone());
        let scope = scope(&profile, vault.as_deref());
        let excludes = ExcludeSet::new(&profile.excludes).map_err(|error| Refusal::Path {
            sentence: error.to_string(),
        })?;
        let agent_target = match target {
            DriveTarget::Existing => AgentTarget::Existing,
            DriveTarget::New => AgentTarget::New,
        };
        let key = agent_write_check(&profile.local_path, &scope, &excludes, path, agent_target)
            .map_err(|refused| refusal(refused, path))?;
        if target == DriveTarget::New {
            return Ok(DriveFile {
                key,
                head: None,
                disk: None,
            });
        }
        let unreadable = |why: String| Refusal::Unreadable {
            drive: drive.to_owned(),
            why,
        };
        let head = keeper_sync::git::history::blob_at(&profile.local_path, "HEAD", &key)
            .map_err(|error| unreadable(error.to_string()))?
            .map(|bytes| blob_id(&bytes));
        let file = keeper_sync::browse::resolve(&profile.local_path, &key)
            .map_err(|refused| Refusal::Path {
                sentence: refused.to_string(),
            })?
            .ok_or_else(|| Refusal::Absent {
                path: path.to_owned(),
            })?;
        let bytes = std::fs::read(&file).map_err(|error| unreadable(error.to_string()))?;
        Ok(DriveFile {
            key,
            head,
            disk: Some((blob_id(&bytes), bytes)),
        })
    }

    fn commit(&self, drive: &str, commit: DriveCommit) -> Result<DriveCommitted, String> {
        let (engine, profile) = self
            .profile(drive)
            .ok_or_else(|| format!("This device does not keep {drive}."))?;
        // The notes the commit changes: those on the disk now (changed,
        // moved away, deleted), then those it puts there (made, moved to).
        let mut notes: Vec<(String, String)> = commit
            .writes
            .iter()
            .map(|(key, _)| key)
            .chain(commit.moves.iter().map(|(from, _)| from))
            .filter_map(|key| self.note(&profile, key))
            .collect();
        self.recheck(&profile, &commit)?;
        let request = CommitRequest {
            writes: commit.writes.clone(),
            moves: commit.moves.clone(),
            guards: commit.guards,
            subject: commit.subject,
            trailers: vec![MemoryTrailer::ApprovalRecord(commit.approval)],
        };
        // The person's own device carries it out: no lease to lose.
        let fence: CommitFence = Arc::new(|| true);
        let done =
            tauri::async_runtime::block_on(engine.commit_paths(&profile.id, &request, fence))
                .map_err(|error| error.to_string())?;
        let outcome = match done {
            CommitPaths::Committed { commit } => DriveCommitted::Committed(commit),
            CommitPaths::Unchanged => DriveCommitted::Unchanged,
            CommitPaths::Guarded { path } => DriveCommitted::Guarded(path),
            CommitPaths::Fenced => {
                return Err("keeper stopped this write before it was made.".to_owned())
            }
        };
        if matches!(outcome, DriveCommitted::Committed(_)) {
            notes.extend(
                commit
                    .writes
                    .iter()
                    .filter(|(_, bytes)| bytes.is_some())
                    .map(|(key, _)| key)
                    .chain(commit.moves.iter().map(|(_, to)| to))
                    .filter_map(|key| self.note(&profile, key)),
            );
            notes.sort();
            notes.dedup();
            for (vault, path) in notes {
                crate::notes_vault::touch(&vault, vec![path]);
            }
        }
        Ok(outcome)
    }

    fn carried(&self, drive: &str, approval: &str) -> Result<Option<String>, String> {
        let (engine, profile) = self
            .profile(drive)
            .ok_or_else(|| format!("This device does not keep {drive}."))?;
        tauri::async_runtime::block_on(engine.approval_commit(&profile.id, approval))
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
#[path = "hq_apply_tests.rs"]
mod tests;
