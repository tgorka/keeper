//! Server-side key backup — enable + restore (Story 3.3, FR-14, AD-1, NFR-9).
//!
//! This module owns **every** recovery/backup SDK call, the `RecoveryState`, and
//! all secret-storage / backup key material for an account. It goes through
//! matrix-sdk 0.18's high-level [`Recovery`] API exclusively
//! (`client.encryption().recovery()`) — never the low-level `backups()` /
//! `secret_storage()` layers, which the SDK docs forbid mixing.
//!
//! The webview receives only a typed [`BackupStatus`] (streamed from the recovery
//! state) and *named* error codes. The ONE sanctioned key-material exception is
//! the base58 **recovery key** string returned by [`enable`]: it crosses IPC
//! because it is meant for the human to save (shown once). It is never written to
//! `tracing`, never held in a persistent JS store, and only re-crosses IPC for an
//! explicit Keychain save or a restore prefill.
//!
//! Three operations:
//! * [`run_status_producer`] — maps `recovery().state_stream()` to a stream of
//!   [`BackupStatus`] snapshots (current first, then changes), self-terminating
//!   when the sink closes.
//! * [`enable`] — `recovery().enable().await`, returning the base58 recovery key
//!   once. Catches `BackupExistsOnServer` and maps it to a *named* error.
//! * [`restore`] — `recovery().recover(key).await`; the SDK downloads room keys
//!   automatically afterward, so 3.1's streams re-render UTD rows for free.

use futures_util::StreamExt;
use matrix_sdk::encryption::recovery::{RecoveryError, RecoveryState};
use matrix_sdk::encryption::secret_storage::{DecryptionError, SecretStorageError};
use matrix_sdk::Client;

use crate::error::{BackupError, CoreError};
use crate::vm::BackupStatus;

/// Sink that receives each produced [`BackupStatus`]. The shell wraps a Tauri
/// `Channel::send`; tests capture into a vector. Returns `true` if the snapshot
/// was delivered, `false` if the channel is closed (the producer then stops).
pub type BackupSink = Box<dyn Fn(BackupStatus) -> bool + Send + Sync>;

/// Pure mapping of the SDK [`RecoveryState`] to a [`BackupStatus`]: `Unknown` →
/// `Unknown` (crypto not synced — "Checking…"), `Enabled` → `Enabled`
/// ("Backup on"), `Disabled` → `Disabled` ("Set up backup"), `Incomplete` →
/// `Incomplete` (the fresh-login "Needs your recovery key" restore case). The
/// Settings backup row derives from this one signal.
pub fn map_recovery_state(state: &RecoveryState) -> BackupStatus {
    match state {
        RecoveryState::Unknown => BackupStatus::Unknown,
        RecoveryState::Enabled => BackupStatus::Enabled,
        RecoveryState::Disabled => BackupStatus::Disabled,
        RecoveryState::Incomplete => BackupStatus::Incomplete,
    }
}

/// Pure mapping of a [`RecoveryError`] to a *named* [`BackupError`] (FR-14).
///
/// * `SecretStorage(SecretStorageError::SecretStorageKey(_))` → a malformed /
///   undecodable recovery key ([`BackupError::MalformedRecoveryKey`]).
/// * `SecretStorage(SecretStorageError::Decryption(DecryptionError::Mac(_)))` → a
///   well-formed-but-wrong key that failed the MAC check
///   ([`BackupError::IncorrectRecoveryKey`]).
/// * `BackupExistsOnServer` → [`BackupError::AlreadyExistsOnServer`].
/// * everything else → [`BackupError::RestoreFailed`] with a non-secret reason.
///
/// The two `SecretStorageError` arms are matched explicitly with a `_` catch-all
/// because `SecretStorageError` is effectively open (many variants); this is what
/// makes an invalid key a *named* error rather than a generic failure.
pub fn map_recover_error(err: &RecoveryError) -> BackupError {
    match err {
        RecoveryError::BackupExistsOnServer => BackupError::AlreadyExistsOnServer,
        RecoveryError::SecretStorage(secret) => match secret {
            SecretStorageError::SecretStorageKey(_) => BackupError::MalformedRecoveryKey,
            SecretStorageError::Decryption(DecryptionError::Mac(_)) => {
                BackupError::IncorrectRecoveryKey
            }
            // `SecretStorageError` is open-ended (`#[non_exhaustive]`-like): any
            // other secret-storage failure (a UTF-8 decode of the decrypted
            // secret, a store error, a missing key info) is an honest generic
            // restore failure, not a malformed/wrong-key claim.
            other => BackupError::RestoreFailed(other.to_string()),
        },
        // A typical SDK error (network / server) during restore.
        other => BackupError::RestoreFailed(other.to_string()),
    }
}

/// Per-account backup-status producer (Story 3.3). Emits the current mapped
/// [`BackupStatus`] as an initial snapshot, then a batch on every
/// `recovery().state_stream()` change, deduping consecutive-equal statuses (the
/// snapshot-then-diff contract, AD-8). Stops when the sink reports the channel is
/// closed or the state stream ends.
///
/// Reads only the SDK recovery state — no recovery key, secret-storage key, or
/// backup material is ever touched here (NFR-9, AD-1).
pub async fn run_status_producer(client: Client, sink: BackupSink, account_id: &str) {
    let recovery = client.encryption().recovery();
    let mut states = recovery.state_stream();

    // `state_stream()` emits the current state first, so the initial snapshot
    // arrives as the first item of the loop — no separate seed read is needed.
    let mut last: Option<BackupStatus> = None;
    while let Some(state) = states.next().await {
        let status = map_recovery_state(&state);
        if Some(status) == last {
            continue;
        }
        last = Some(status);
        if !(sink)(status) {
            tracing::info!(account_id = %account_id, "backup status channel closed, stopping producer");
            return;
        }
    }
    tracing::info!(account_id = %account_id, "backup status stream ended");
}

/// Enable server-side key backup + secret storage, returning the base58 recovery
/// key *once* (Story 3.3, FR-14). This is the deliberate boundary exception: the
/// returned key is meant for the human to save and is shown once in `mono`.
///
/// Does NOT chain `.wait_for_backups_to_upload()` — the key is returned promptly
/// and the status stream reflects the upload progress. A race with an existing
/// server backup surfaces as a *named* [`BackupError::AlreadyExistsOnServer`]
/// (the modal then offers restore); any other failure is
/// [`BackupError::Action`]. The recovery key is NEVER logged.
pub async fn enable(client: &Client) -> Result<String, CoreError> {
    match client.encryption().recovery().enable().await {
        Ok(recovery_key) => {
            tracing::info!("key backup enabled; recovery key returned to be shown once");
            Ok(recovery_key)
        }
        // Map the existing-backup race to a named state before any generic
        // mapping so the modal can offer restore instead of a generic failure.
        Err(RecoveryError::BackupExistsOnServer) => Err(BackupError::AlreadyExistsOnServer.into()),
        Err(other) => Err(BackupError::Action(other.to_string()).into()),
    }
}

/// Restore from server-side key backup with a base58 recovery key (Story 3.3,
/// FR-14). `recovery().recover(key)` opens the secret store, imports secrets, and
/// the SDK downloads room keys automatically afterward — so 3.1's
/// encryption-status + timeline streams re-render previously-undecryptable rows
/// with no extra code here (do NOT call `download_room_keys`).
///
/// Invalid keys are mapped to *named* errors via [`map_recover_error`]: a
/// malformed key and a well-formed-but-wrong key are distinct. The recovery key
/// is NEVER logged.
pub async fn restore(client: &Client, recovery_key: &str) -> Result<(), CoreError> {
    match client.encryption().recovery().recover(recovery_key).await {
        Ok(()) => {
            tracing::info!("restored secrets from key backup; room keys downloading");
            Ok(())
        }
        Err(err) => Err(map_recover_error(&err).into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_recovery_state_covers_all_variants() {
        assert_eq!(
            map_recovery_state(&RecoveryState::Unknown),
            BackupStatus::Unknown
        );
        assert_eq!(
            map_recovery_state(&RecoveryState::Enabled),
            BackupStatus::Enabled
        );
        assert_eq!(
            map_recovery_state(&RecoveryState::Disabled),
            BackupStatus::Disabled
        );
        assert_eq!(
            map_recovery_state(&RecoveryState::Incomplete),
            BackupStatus::Incomplete
        );
    }

    #[test]
    fn map_recover_error_maps_backup_exists_to_named() {
        // The only `RecoveryError` variant unit-constructible without SDK-private
        // payloads is the fieldless `BackupExistsOnServer`; it must map to the
        // named `AlreadyExistsOnServer`, never a generic failure.
        assert!(matches!(
            map_recover_error(&RecoveryError::BackupExistsOnServer),
            BackupError::AlreadyExistsOnServer
        ));
    }

    // The malformed-vs-wrong-key split (`SecretStorageError::SecretStorageKey`
    // → `MalformedRecoveryKey` and `Decryption(Mac)` → `IncorrectRecoveryKey`)
    // cannot be exercised in a unit test here: `DecodeError` and `MacError` are
    // re-exported from `matrix_sdk_base::crypto::secret_storage` with no public
    // constructor (like Story 3.2's `CancelInfo`). Those two arms are covered by
    // the spec's manual second-session restore check (paste a malformed key and a
    // well-formed-but-wrong key and confirm each shows its own named inline error);
    // the mapping code above wires each variant to its distinct `BackupError`.
}
