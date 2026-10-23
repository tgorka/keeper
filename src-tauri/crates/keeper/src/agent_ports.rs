//! The app's side of `keeper_agent`'s vault port (AD-367): how a write the
//! sessions board's verbs make lands in a notes vault (`sessions_ipc.rs`).
//!
//! Everything else this file held left with what used it: the ⌘9 surface's
//! stream sinks, approval sheet and error envelope, and the scheduled prompt
//! task's turn environment (Story 108.3a); the agents host's (108.2).
//! Desktop-only: a phone has no sessions verbs that write a vault.

#[cfg(desktop)]
pub(crate) mod drive {
    use keeper_agent::promote::VaultWriter;

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
}
