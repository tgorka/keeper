//! The app's side of `keeper_agent`'s ports (AD-367) that the agent runtime
//! still in keeper uses: how a vault file is written and which drives exist.
//!
//! The ⌘9 surface's half — its stream sinks, its approval sheet and its
//! error envelope — left with that surface (Story 108.3a). What remains is
//! [`task_env`], the environment this Mac hosts an agent's turn in
//! (`agents_host.rs`), and [`drive`], whose vault writer the sessions board's
//! verbs share (`sessions_ipc.rs`). Both are desktop-only: a phone hosts no
//! agent and has no drive tools.

#[cfg(desktop)]
use std::sync::Arc;

#[cfg(desktop)]
use keeper_agent::turn::{AccountCredential, TurnEnv};
#[cfg(desktop)]
use keeper_core::platform::Platform;

/// The configured account as a credential source, read now.
#[cfg(desktop)]
fn account() -> Option<AccountCredential> {
    crate::account_ipc::descriptor().map(|descriptor| AccountCredential {
        descriptor,
        http: crate::account_ipc::http().cloned(),
    })
}

/// A hosted turn's environment: the drive, the vault, and nobody to approve
/// (AD-244).
#[cfg(desktop)]
pub(crate) fn task_env(platform: Arc<dyn Platform>) -> TurnEnv {
    TurnEnv {
        drive: Some(drive::ports(Arc::clone(&platform), None)),
        account: account(),
        platform,
    }
}

#[cfg(desktop)]
pub(crate) mod drive {
    use std::sync::Arc;

    use keeper_agent::ports::{ApprovalPort, ProfileSource, VaultWriter};
    use keeper_agent::turn::DrivePorts;
    use keeper_core::platform::Platform;
    use keeper_sync::SyncProfile;

    pub(super) fn ports(
        platform: Arc<dyn Platform>,
        approval: Option<Arc<dyn ApprovalPort>>,
    ) -> DrivePorts {
        DrivePorts {
            profiles: Arc::new(EngineProfiles(platform)),
            vault: Some(Arc::new(NotesVaultWriter)),
            approval,
        }
    }

    /// A write that lands in a registered notes vault: written durably,
    /// then the reconciler and the dirty mark told, as an edit in the
    /// editor would be.
    pub(crate) struct NotesVaultWriter;

    impl VaultWriter for NotesVaultWriter {
        fn subfolder(&self, profile_id: &str) -> Option<String> {
            crate::notes_vault::vault(profile_id).map(|vault| vault.config.subfolder)
        }

        fn write(
            &self,
            profile_id: &str,
            subfolder: &str,
            rel: &str,
            text: &str,
        ) -> Result<(), String> {
            let vault = crate::notes_vault::vault(profile_id)
                .ok_or_else(|| format!("the notes vault in {profile_id} is no longer open"))?;
            if vault.config.subfolder != subfolder {
                return Err(format!(
                    "the notes vault in {profile_id} is at {} now, not at {subfolder} where this write was checked, so nothing was written",
                    vault.config.subfolder
                ));
            }
            crate::notes_vault::write_vault_file_durable(&vault, rel, text)
                .map_err(|error| error.to_string())?;
            crate::notes_vault::touch(&vault.id, vec![rel.to_owned()]);
            crate::notes_vault::mark_dirty(&vault.id);
            Ok(())
        }

        /// Through the notes editor's own guarded amend: every live editor
        /// on the note held, the note read, amended and written only over
        /// exactly the text it read — the check and the write one step to
        /// every other write keeper makes to it, open in an editor or not
        /// (`notes_vault::write_note_if`) — and an editor that has it open
        /// adopts the new block.
        fn amend(
            &self,
            profile_id: &str,
            rel: &str,
            amend: &dyn Fn(&str) -> Option<String>,
        ) -> Result<bool, String> {
            let vault = crate::notes_vault::vault(profile_id)
                .ok_or_else(|| format!("the notes vault in {profile_id} is no longer open"))?;
            crate::notes_ipc::amend_block(&vault, rel, amend).map_err(|error| error.to_string())
        }
    }

    /// Every sync profile keeper holds, or none when the engine is
    /// unavailable — every tool call is then refused as naming no folder.
    struct EngineProfiles(Arc<dyn Platform>);

    impl ProfileSource for EngineProfiles {
        fn profiles(&self) -> Vec<SyncProfile> {
            let Ok(engine) = crate::sync::engine(Arc::clone(&self.0)) else {
                return Vec::new();
            };
            engine.list_profiles().unwrap_or_default()
        }
    }
}
