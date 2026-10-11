//! What a host process supplies to the turn loop (AD-367).
//!
//! Four ports, each the narrowest thing the loop asks of its surroundings:
//! where the stream goes, who answers an approval, how a vault file is
//! written, and which drives exist. The app fills them with a webview channel,
//! the approval sheet, the notes vault and the sync engine; a host with no
//! person at it fills them with less, and the loop cannot tell.

use keeper_core::bots::chat::CancelSignal;
use keeper_core::vm::{BotApprovalRequestVm, BotStreamEvent};
use keeper_sync::SyncProfile;

/// Where one turn's stream goes.
///
/// Every hook but [`TurnSink::event`] has an empty default: a sink that only
/// shows the stream needs nothing else, and the spoken turn's sink is the one
/// that listens to the rest.
pub trait TurnSink: Send + Sync {
    /// One event, in order. `false` when nobody is listening any more; the
    /// turn runs to its end either way, because the row is the record.
    fn event(&self, event: BotStreamEvent) -> bool;

    /// The first request is about to leave.
    fn request_sent(&self) {}

    /// Model prose, as it arrives and before its `Delta` is sent. Never the
    /// separator the loop puts between two rounds' prose: a sink that cuts
    /// sentences would otherwise end one at a line break the model never wrote.
    fn answer_text(&self, _text: &str) {}

    /// The turn is over: its row is closed and `Closed` was sent (or the row
    /// was gone and there was nothing to send).
    fn ended(&self, _end: TurnEnd) {}
}

/// How a turn ended, as [`TurnSink::ended`] hears it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnEnd {
    /// The answer finished cleanly.
    Complete,
    /// A person pressed Stop.
    Stopped,
    /// It broke, with the sentence the row and the pane carry.
    Failed(String),
}

/// Who answers when a grant says a person must approve one call.
///
/// Blocking by design: a tool call is a blocking act inside one round. `true`
/// is consent; every other way the wait ends is a refusal.
pub trait ApprovalPort: Send + Sync {
    /// Ask, and wait for the answer or for `signal`.
    fn ask(&self, request: BotApprovalRequestVm, signal: &CancelSignal) -> bool;
}

/// How a write lands inside a notes vault (AD-102).
pub trait VaultWriter: Send + Sync {
    /// The live vault's subfolder in this profile, or `None` when the profile
    /// holds no registered vault.
    fn subfolder(&self, profile_id: &str) -> Option<String>;

    /// Write `text` at `rel` inside the profile's vault, and tell the vault —
    /// refused, nothing written, unless the live vault is still the one at
    /// `subfolder` that `rel` was checked against. `Ok` only once the write
    /// is durable: the bytes and the new name synced to the disk, so a
    /// caller may let go of anything it kept to finish the write.
    fn write(&self, profile_id: &str, subfolder: &str, rel: &str, text: &str)
        -> Result<(), String>;

    /// Change the note at `rel` inside the profile's vault through `amend`,
    /// under the vault's write coordination: `amend` is handed the text as
    /// it is and answers the whole new text, or `None` for no change; a text
    /// that changed between that read and the write is read and amended
    /// again, never overwritten. `Ok(false)` when nothing changed.
    fn amend(
        &self,
        profile_id: &str,
        rel: &str,
        amend: &dyn Fn(&str) -> Option<String>,
    ) -> Result<bool, String>;
}

/// The sync profiles a tool call may name.
pub trait ProfileSource: Send + Sync {
    /// Every profile, or none when the host cannot read them.
    fn profiles(&self) -> Vec<SyncProfile>;
}
