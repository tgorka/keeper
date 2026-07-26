//! The sync profile — the unit of configuration, concurrency, progress,
//! logging and failure (Story 23.3, AD-42).
//!
//! A profile binds one local folder to one remote repository. Everything the
//! engine does is scoped to one: watchers, journals, supervisors, tray lines
//! and error reports all name a profile. Nothing in the engine is global.
//!
//! Profiles are serde types because they round-trip three ways: into `sync.db`,
//! into the app's `config.json` override namespace, and into `keeper-syncd`'s
//! TOML config (Story 30.2). The same table must mean the same thing in all
//! three, so defaults live here and nowhere else.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{Result, SyncError};

/// Which way changes are allowed to flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncDirection {
    /// Local and remote converge (FR-77). Conflicts become conflict copies.
    Bidirectional,
    /// Local changes are pushed; remote changes are never applied locally.
    /// With `lane = Worktree` this is the bot-to-human airlock (AD-50).
    PushOnly,
    /// Remote changes are applied locally; local changes are never pushed.
    /// A local edit in a pull-only profile is preserved as a conflict copy
    /// rather than silently reverted.
    PullOnly,
}

impl SyncDirection {
    pub fn pushes(self) -> bool {
        matches!(self, Self::Bidirectional | Self::PushOnly)
    }

    pub fn pulls(self) -> bool {
        matches!(self, Self::Bidirectional | Self::PullOnly)
    }
}

/// How the working tree is materialized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncLane {
    /// The profile owns the repository's main worktree — the normal case.
    Main,
    /// The profile writes into a linked worktree on a generated branch, and
    /// hands off to a human by pull request (AD-50). Only meaningful with
    /// `SyncDirection::PushOnly`.
    Worktree,
}

/// What the engine does with large files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LfsMode {
    /// Track anything over the threshold through LFS, materializing objects
    /// locally. The default, and the only mode safe for multi-GB content:
    /// gitoxide has no streaming object read, so a raw 3 GB blob is a 3 GB
    /// allocation (AD-46).
    Materialize,
    /// Track through LFS but leave excluded paths as pointer files — the
    /// `fetchinclude`/`fetchexclude` idiom, for a partial or metered profile.
    PointerOnly,
    /// No LFS at all. Only honest for a text-only repository; the engine warns
    /// when a file over the threshold appears in such a profile rather than
    /// silently committing it raw.
    Disabled,
}

/// Runtime state, as opposed to configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProfileState {
    /// Configured but not started.
    Idle,
    /// Watching and up to date.
    Watching,
    /// A sync operation is in flight.
    Syncing,
    /// Network work is queued; local git still works (AD-49).
    Offline,
    /// The removable volume is not attached. A first-class state, NOT an
    /// error: no deletion is staged, committed or pushed while here (AD-48).
    MediaAbsent,
    /// Paused by the user.
    Paused,
    /// Stopped on a condition that needs a human.
    NeedsAttention,
}

impl ProfileState {
    /// Whether the tray should show activity for this profile (AD-51).
    pub fn is_active(self) -> bool {
        matches!(self, Self::Syncing)
    }

    /// Whether this state should raise a warning surface.
    pub fn is_warning(self) -> bool {
        matches!(self, Self::NeedsAttention | Self::MediaAbsent)
    }
}

/// Default quiescence window (Story 26.3). 5 s sits between Nextcloud's
/// aggressive 2 s and Syncthing's effective 5–15 s; both ship in production.
pub const DEFAULT_SETTLE_MS: u64 = 5_000;
/// Quiescence window on removable and network media, where mtime lags and
/// HFS+/SMB granularity can be a full second.
pub const REMOVABLE_SETTLE_MS: u64 = 10_000;
/// Window after a Linux `IN_CLOSE_WRITE`: the writer demonstrably let go, so
/// this only has to absorb mtime granularity.
pub const CLOSE_WRITE_SETTLE_MS: u64 = 1_000;
/// Hard ceiling. A continuously appended log must eventually sync; Syncthing
/// caps its own `notifyTimeout` at the same 60 s.
pub const SETTLE_CEILING_MS: u64 = 60_000;
/// Files at or above this size are tracked through LFS by default.
pub const DEFAULT_LFS_THRESHOLD_BYTES: u64 = 4 * 1024 * 1024;
/// Default interval between scheduler ticks.
pub const DEFAULT_POLL_INTERVAL_MS: u64 = 15_000;

/// One folder bound to one repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProfile {
    /// Opaque ULID. Stable across renames, so it is what the journal, the
    /// keychain entry and the log spans key on.
    pub id: String,
    /// Human label, shown in the tray and used in commit subjects.
    pub name: String,
    /// Absolute path to the synced folder.
    pub local_path: PathBuf,
    /// Remote repository URL (https, ssh or a local path).
    pub remote_url: String,
    /// Branch this profile tracks.
    pub branch: String,
    pub direction: SyncDirection,
    pub lane: SyncLane,
    /// Repository subpaths to materialize. Empty means the whole repository.
    /// Applied both as a cone sparse-checkout and as the LFS path filter,
    /// because git-lfs is sparse-checkout-unaware (AD-47).
    #[serde(default)]
    pub subpaths: Vec<String>,
    /// Additional tier-0 exclusion globs, on top of the built-in set.
    #[serde(default)]
    pub excludes: Vec<String>,
    /// The folder lives on removable media: absence is a pause, never a
    /// deletion (AD-48).
    #[serde(default)]
    pub removable: bool,
    pub lfs_mode: LfsMode,
    #[serde(default = "default_lfs_threshold")]
    pub lfs_threshold_bytes: u64,
    #[serde(default = "default_settle_ms")]
    pub settle_ms: u64,
    #[serde(default = "default_poll_interval_ms")]
    pub poll_interval_ms: u64,
    /// Extra `Keeper-Tag:` trailer values stamped on every commit (FR-86).
    #[serde(default)]
    pub tags: Vec<String>,
    /// Overrides the derived git author for this profile.
    #[serde(default)]
    pub author_override: Option<String>,
    /// Whether watch mode is armed.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_lfs_threshold() -> u64 {
    DEFAULT_LFS_THRESHOLD_BYTES
}
fn default_settle_ms() -> u64 {
    DEFAULT_SETTLE_MS
}
fn default_poll_interval_ms() -> u64 {
    DEFAULT_POLL_INTERVAL_MS
}
fn default_true() -> bool {
    true
}

impl SyncProfile {
    /// A bidirectional profile with every default applied.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        local_path: impl Into<PathBuf>,
        remote_url: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            local_path: local_path.into(),
            remote_url: remote_url.into(),
            branch: "main".to_owned(),
            direction: SyncDirection::Bidirectional,
            lane: SyncLane::Main,
            subpaths: Vec::new(),
            excludes: Vec::new(),
            removable: false,
            lfs_mode: LfsMode::Materialize,
            lfs_threshold_bytes: DEFAULT_LFS_THRESHOLD_BYTES,
            settle_ms: DEFAULT_SETTLE_MS,
            poll_interval_ms: DEFAULT_POLL_INTERVAL_MS,
            tags: Vec::new(),
            author_override: None,
            enabled: true,
        }
    }

    /// The effective quiescence window, before the close-write fast path.
    ///
    /// Removable and network media get a longer window because their mtime
    /// lags; a profile may still override it explicitly.
    pub fn effective_settle_ms(&self) -> u64 {
        if self.removable && self.settle_ms == DEFAULT_SETTLE_MS {
            REMOVABLE_SETTLE_MS
        } else {
            self.settle_ms.min(SETTLE_CEILING_MS)
        }
    }

    /// Keychain key for this profile's remote credential. Never the secret.
    pub fn secret_key(&self) -> String {
        format!("sync/{}/credential", self.id)
    }

    /// Reject configurations the engine cannot act on, before anything touches
    /// the disk or the network. Called on every create and update, and again on
    /// load, so a hand-edited `config.json` cannot smuggle in a bad profile.
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(SyncError::Config("profile name must not be empty".into()));
        }
        if !self.local_path.is_absolute() {
            return Err(SyncError::Config(format!(
                "local path must be absolute, got {}",
                self.local_path.display()
            )));
        }
        if self.remote_url.trim().is_empty() {
            return Err(SyncError::Config("remote URL must not be empty".into()));
        }
        if self.branch.trim().is_empty() {
            return Err(SyncError::Config("branch must not be empty".into()));
        }
        // A worktree lane only makes sense one-way: its whole purpose is to
        // keep an agent's work off the human's branch until a PR is accepted.
        if self.lane == SyncLane::Worktree && self.direction != SyncDirection::PushOnly {
            return Err(SyncError::Config(
                "a worktree lane requires direction = pushOnly".into(),
            ));
        }
        for sub in &self.subpaths {
            // An absolute or parent-escaping subpath would take the cone
            // sparse-checkout outside the repository.
            if sub.starts_with('/') || sub.split('/').any(|c| c == "..") {
                return Err(SyncError::Config(format!(
                    "subpath must be repository-relative and must not escape: {sub}"
                )));
            }
        }
        if self.settle_ms > SETTLE_CEILING_MS {
            return Err(SyncError::Config(format!(
                "settle window must not exceed {SETTLE_CEILING_MS} ms"
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> SyncProfile {
        SyncProfile::new(
            "01J",
            "tgdrive",
            "/home/u/tgdrive",
            "https://git.example/u/tgdrive.git",
        )
    }

    #[test]
    fn direction_flow_predicates_are_exhaustive_and_correct() {
        assert!(SyncDirection::Bidirectional.pushes() && SyncDirection::Bidirectional.pulls());
        assert!(SyncDirection::PushOnly.pushes() && !SyncDirection::PushOnly.pulls());
        assert!(!SyncDirection::PullOnly.pushes() && SyncDirection::PullOnly.pulls());
    }

    #[test]
    fn removable_profiles_get_the_longer_settle_window() {
        // mtime lags on removable and network media; 5 s is too tight there.
        let mut p = profile();
        assert_eq!(p.effective_settle_ms(), DEFAULT_SETTLE_MS);
        p.removable = true;
        assert_eq!(p.effective_settle_ms(), REMOVABLE_SETTLE_MS);
    }

    #[test]
    fn an_explicit_settle_override_survives_the_removable_default() {
        let mut p = profile();
        p.removable = true;
        p.settle_ms = 30_000;
        assert_eq!(p.effective_settle_ms(), 30_000);
    }

    #[test]
    fn settle_window_is_clamped_to_the_ceiling() {
        let mut p = profile();
        p.settle_ms = u64::MAX;
        assert_eq!(p.effective_settle_ms(), SETTLE_CEILING_MS);
    }

    #[test]
    fn relative_local_paths_are_rejected() {
        let mut p = profile();
        p.local_path = PathBuf::from("relative/path");
        assert!(p.validate().is_err());
    }

    #[test]
    fn subpaths_may_not_escape_the_repository() {
        // Otherwise the cone sparse-checkout would point outside the repo.
        for bad in ["/etc", "a/../../etc", ".."] {
            let mut p = profile();
            p.subpaths = vec![bad.to_owned()];
            assert!(p.validate().is_err(), "{bad} must be rejected");
        }
        let mut ok = profile();
        ok.subpaths = vec!["media/video".to_owned()];
        assert!(ok.validate().is_ok());
    }

    #[test]
    fn a_worktree_lane_must_be_push_only() {
        // A bidirectional bot lane would let an agent pull a human's review
        // edits back under itself — the airlock would leak.
        let mut p = profile();
        p.lane = SyncLane::Worktree;
        assert!(p.validate().is_err());
        p.direction = SyncDirection::PushOnly;
        assert!(p.validate().is_ok());
    }

    #[test]
    fn profiles_round_trip_through_json_with_defaults_absent() {
        // config.json overrides and keeper-syncd TOML both omit defaulted keys.
        let minimal = r#"{
            "id": "01J", "name": "n", "localPath": "/a", "remoteUrl": "u",
            "branch": "main", "direction": "bidirectional", "lane": "main",
            "lfsMode": "materialize"
        }"#;
        let parsed: SyncProfile = serde_json::from_str(minimal).expect("parse");
        assert_eq!(parsed.lfs_threshold_bytes, DEFAULT_LFS_THRESHOLD_BYTES);
        assert_eq!(parsed.settle_ms, DEFAULT_SETTLE_MS);
        assert!(parsed.enabled);

        let round = serde_json::to_string(&parsed).expect("encode");
        let back: SyncProfile = serde_json::from_str(&round).expect("decode");
        assert_eq!(parsed, back);
    }

    #[test]
    fn secret_key_is_derived_from_the_id_not_the_name() {
        // Renaming a profile must not orphan its stored credential.
        let mut p = profile();
        let before = p.secret_key();
        p.name = "renamed".to_owned();
        assert_eq!(before, p.secret_key());
    }
}
