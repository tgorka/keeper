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

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
    error::{Result, SyncError},
    provenance,
};

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
/// Floor on the scan cadence.
///
/// The field is user-supplied, and a profile written before it meant anything
/// (DW-116) carries a zero — either of which would put a full re-stat of the
/// tree on every 1 Hz supervisor tick, on a pendrive or over a network mount.
pub const MIN_POLL_INTERVAL_MS: u64 = 2_000;

/// Where a vault lives inside a notes-flagged folder, by default (AD-54).
pub const DEFAULT_NOTES_SUBFOLDER: &str = "notes";
/// Where a journal entry for a given day is written, by default (FR-99).
///
/// The same string as `keeper_core::notes::naming::DEFAULT_JOURNAL_TEMPLATE`,
/// spelled again rather than imported: AD-40 keeps this crate core-free. The
/// alternative — leaving the field empty and letting whichever host read the
/// row fill it in — would make one JSON blob mean two different things
/// depending on which binary opened it, which is exactly what this module's
/// "defaults live here and nowhere else" rule exists to prevent.
pub const DEFAULT_JOURNAL_TEMPLATE: &str = "journal/{yyyy}/{yyyy}-{mm}-{dd}.md";
/// Default quiet time after the last edit before a note is committed.
///
/// Long enough that a burst of typing is one commit rather than one per
/// keystroke run, short enough that a thought captured and abandoned is on the
/// other machine before the lid is shut (AD-62).
pub const DEFAULT_COMMIT_IDLE_MS: u64 = 2_000;
/// Floor on the idle window. Below roughly half a second the debounce stops
/// debouncing, and unreadable history plus an unbounded push queue is the
/// outcome AD-62 rejected commit-on-every-save to avoid.
pub const MIN_COMMIT_IDLE_MS: u64 = 500;
/// Default deadline between a notes commit and the push that carries it.
pub const DEFAULT_PUSH_INTERVAL_MS: u64 = 30_000;
/// Floor on the push deadline. Each push is a network round trip; under five
/// seconds a laptop on a flaky link spends its battery on retries rather than
/// on getting the bytes across.
pub const MIN_PUSH_INTERVAL_MS: u64 = 5_000;

/// When a notes-flagged profile commits, and when it pushes (FR-115, AD-62).
///
/// A knob on the profile rather than a scheduler of its own: the 1 Hz
/// supervisor already ticks with exactly the resolution a 2 s idle window
/// needs, and two schedulers over one working tree is how concurrent index
/// locks happen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesCadence {
    /// Quiet time after the last edit before the note is committed.
    pub commit_idle_ms: u64,
    /// How long a commit may sit locally before it is pushed.
    pub push_interval_ms: u64,
    /// Push as soon as the window loses focus, without waiting the interval
    /// out. Leaving is the strongest available signal that these bytes are
    /// wanted somewhere else.
    pub push_on_blur: bool,
}

impl Default for NotesCadence {
    fn default() -> Self {
        Self {
            commit_idle_ms: DEFAULT_COMMIT_IDLE_MS,
            push_interval_ms: DEFAULT_PUSH_INTERVAL_MS,
            push_on_blur: true,
        }
    }
}

/// The notes flag on a profile, and everything it configures (AD-54, FR-120).
///
/// `Some` on a [`SyncProfile`] means "this synced folder contains a vault", and
/// `subfolder` says where inside it. A vault is therefore not an object with a
/// lifetime of its own: the vault list is a filter over the profile list and
/// the vault id *is* the profile id, so there is no picker, no second path to
/// validate, no import and no migration.
///
/// This crate stores the flag and understands nothing behind it. Markdown,
/// frontmatter, links and the index are `keeper-core`'s; the engine sees a
/// folder with files in it, exactly as it does for every other profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesConfig {
    /// The vault root, relative to `local_path`. Never empty, never absolute,
    /// never escaping and never an Obsidian configuration folder — see
    /// [`SyncProfile::validate`] for why each of those is refused rather than
    /// corrected.
    #[serde(default = "default_subfolder")]
    pub subfolder: String,
    /// Vault-relative path template for a day's journal entry.
    #[serde(default = "default_journal_template")]
    pub journal_template: String,
    /// Vault-relative path of the template a new note starts from, if any.
    #[serde(default)]
    pub default_template: Option<String>,
    #[serde(default)]
    pub cadence: NotesCadence,
}

impl Default for NotesConfig {
    fn default() -> Self {
        Self {
            subfolder: DEFAULT_NOTES_SUBFOLDER.to_owned(),
            journal_template: DEFAULT_JOURNAL_TEMPLATE.to_owned(),
            default_template: None,
            cadence: NotesCadence::default(),
        }
    }
}

impl NotesConfig {
    /// The subfolder and cadence rules, split out of [`SyncProfile::validate`]
    /// because they are about these fields and not about the profile.
    ///
    /// Every rule here REFUSES rather than corrects, which is the opposite of
    /// what `poll_interval_ms` does with a value below its floor. The reason is
    /// that the two fields have different histories: a stored zero poll
    /// interval is a row written when the field meant nothing (DW-116) and no
    /// human ever chose it, whereas `notes` is absent from every profile
    /// written before it existed and `#[serde(default)]` gives every profile
    /// that does mention it a working cadence. So a value below the floor here
    /// is something a person typed, and a person who typed it is looking at the
    /// field — which is the moment to say so.
    fn validate(&self) -> Result<()> {
        let subfolder = self.subfolder.trim();
        if subfolder.is_empty() {
            return Err(SyncError::Config(
                "notes subfolder must not be empty: a vault is a folder inside the profile, \
                 never the profile root"
                    .into(),
            ));
        }
        // `Path::join` with an absolute right-hand side DISCARDS the left one,
        // so an absolute subfolder would not fail loudly — `vault_root` would
        // quietly point somewhere outside the synced folder entirely. Tested as
        // a string as well as through `is_absolute`, because absoluteness is
        // platform-shaped (`C:\x` and `\\server\share` are absolute only to
        // Windows) and one profile row has to mean the same thing on every
        // machine that reads it.
        if Path::new(subfolder).is_absolute()
            || subfolder.starts_with('/')
            || subfolder.starts_with('\\')
        {
            return Err(SyncError::Config(format!(
                "notes subfolder must be relative to the profile folder, got {subfolder}"
            )));
        }
        let component = |part: &str| subfolder.split(['/', '\\']).any(|c| c == part);
        if component("..") {
            return Err(SyncError::Config(format!(
                "notes subfolder must not escape the profile folder: {subfolder}"
            )));
        }
        // FR-121: keeper never reads, writes or walks an Obsidian configuration
        // folder. Nothing keeper constructs names it, but "we never generate
        // that path" is not the same as "that path cannot be configured", and
        // pointing a vault root at it would put every one of keeper's writes
        // inside somebody else's application state.
        if component(".obsidian") {
            return Err(SyncError::Config(format!(
                "notes subfolder must not name .obsidian; keeper never reads or writes an \
                 Obsidian configuration folder: {subfolder}"
            )));
        }
        if self.cadence.commit_idle_ms < MIN_COMMIT_IDLE_MS {
            return Err(SyncError::Config(format!(
                "notes commit idle window must be at least {MIN_COMMIT_IDLE_MS} ms"
            )));
        }
        if self.cadence.push_interval_ms < MIN_PUSH_INTERVAL_MS {
            return Err(SyncError::Config(format!(
                "notes push interval must be at least {MIN_PUSH_INTERVAL_MS} ms"
            )));
        }
        Ok(())
    }
}

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
    /// The volume this profile is bound to, once it has seen it.
    ///
    /// `None` means "not bound yet": the next scan that finds the folder
    /// present adopts the volume there — minting `.keeper-sync/volume.json` if
    /// the media carries no marker — and records its id here. From then on the
    /// id is what [`crate::volume::status`] matches on, which is what makes
    /// `Foreign` (a *different* stick mounted at this path) distinguishable
    /// from `Absent` (no stick at all). Only meaningful with `removable`.
    ///
    /// Never edited by hand and never re-minted: rewriting it would silently
    /// re-point a profile at another volume's history, which is exactly the
    /// confusion AD-48 exists to prevent.
    #[serde(default)]
    pub volume_id: Option<String>,
    pub lfs_mode: LfsMode,
    #[serde(default = "default_lfs_threshold")]
    pub lfs_threshold_bytes: u64,
    /// Release local LFS objects once the remote holds them.
    ///
    /// On the machine where content originates every LFS file exists twice —
    /// worktree and object store — because the clean path has to read the bytes
    /// to compute the pointer. Measured on a 211 GB archive that is 215 GB of
    /// worktree plus 215 GB of store on one 920 GB drive.
    ///
    /// Off by default, and the default is the honest one: turning this on trades
    /// a self-sufficient local copy for a smaller one. The drive keeps every
    /// file — only the redundant second copy goes — but restoring a file the
    /// worktree later loses then needs the network.
    ///
    /// See [`crate::lfs::prune`] for the three conditions an object must meet.
    #[serde(default)]
    pub lfs_prune_local: bool,
    /// Globs that must never be routed through LFS, whatever their size.
    ///
    /// The size rule alone is right for a media folder and wrong for a mixed
    /// repository, because the `.gitattributes` rule keeper records is
    /// **per-extension**: one 300 KB note crossing the threshold writes
    /// `*.md filter=lfs`, and from then on every note in the repository is an
    /// opaque pointer — no diff, no merge, no blame. Lowering the threshold to
    /// catch bulk media makes that outcome more likely, not less.
    ///
    /// This is the escape hatch, and it is deliberately the user's call rather
    /// than a built-in list: the trade-off it accepts is real. A file matched
    /// here stays an ordinary git blob however large it grows, and gitoxide has
    /// no streaming object read — a 6 GB `.csv` excluded this way means a 6 GB
    /// allocation. Exclude formats you need to diff, not formats you have a lot
    /// of.
    #[serde(default)]
    pub lfs_never: Vec<String>,
    #[serde(default = "default_settle_ms")]
    pub settle_ms: u64,
    #[serde(default = "default_poll_interval_ms")]
    pub poll_interval_ms: u64,
    /// Extra `Keeper-Tag:` trailer values stamped on every commit (FR-86).
    #[serde(default)]
    pub tags: Vec<String>,
    /// Shapes the generated commit subject, or empty for keeper's own
    /// `sync(<profile>): 3 added, 1 modified`.
    ///
    /// Placeholders are the closed set in
    /// [`crate::provenance::SUBJECT_PLACEHOLDERS`]; an unknown one is refused by
    /// [`SyncProfile::validate`] rather than carried into every commit. Only the
    /// subject is shapeable — the trailer block is provenance, and a clone has
    /// to be able to trust its shape without keeper installed.
    #[serde(default)]
    pub commit_subject_template: String,
    /// Overrides the derived git author for this profile.
    #[serde(default)]
    pub author_override: Option<String>,
    /// Whether watch mode is armed.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// This folder contains a notes vault, and where inside it (AD-54).
    ///
    /// `#[serde(default)]` here IS the migration. A profile persists as one
    /// JSON blob per row, so a row written by a keeper that had never heard of
    /// notes simply has no `notes` key; it loads as `None`, which means exactly
    /// what it should — not a vault. There is no SQL change, no schema bump and
    /// nothing to run on upgrade, and a row written by a newer keeper still
    /// loads on an older one, which is what makes rolling back safe.
    #[serde(default)]
    pub notes: Option<NotesConfig>,
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
fn default_subfolder() -> String {
    DEFAULT_NOTES_SUBFOLDER.to_owned()
}
fn default_journal_template() -> String {
    DEFAULT_JOURNAL_TEMPLATE.to_owned()
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
            volume_id: None,
            lfs_mode: LfsMode::Materialize,
            lfs_threshold_bytes: DEFAULT_LFS_THRESHOLD_BYTES,
            lfs_never: Vec::new(),
            lfs_prune_local: false,
            settle_ms: DEFAULT_SETTLE_MS,
            poll_interval_ms: DEFAULT_POLL_INTERVAL_MS,
            tags: Vec::new(),
            commit_subject_template: String::new(),
            author_override: None,
            enabled: true,
            notes: None,
        }
    }

    /// The effective quiescence window, before the close-write fast path.
    ///
    /// Removable and network media get a longer window because their mtime
    /// lags; a profile may still override it explicitly. `settle_ms` still
    /// holding [`DEFAULT_SETTLE_MS`] is how "let keeper choose the wait" is
    /// encoded, which is why a removable profile that has never pinned one gets
    /// [`REMOVABLE_SETTLE_MS`] and one that pinned exactly 5 s does not.
    pub fn effective_settle_ms(&self) -> u64 {
        if self.removable && self.settle_ms == DEFAULT_SETTLE_MS {
            REMOVABLE_SETTLE_MS
        } else {
            self.settle_ms.min(SETTLE_CEILING_MS)
        }
    }

    /// The scan cadence the scheduler will actually use.
    ///
    /// Beside `effective_settle_ms` because it is the same kind of thing: the
    /// number in force is not always the number stored, and both the form and
    /// the degraded-watcher warning have to be able to say which is which
    /// (AD-34-8).
    pub fn effective_poll_interval_ms(&self) -> u64 {
        self.poll_interval_ms.max(MIN_POLL_INTERVAL_MS)
    }

    /// The vault root — `local_path` joined with the notes subfolder — or
    /// `None` when this profile is not a vault.
    ///
    /// The one place the two halves are put together, so "where is the vault"
    /// has a single answer even though the flag and the folder are stored
    /// apart. [`Self::validate`] has already refused a subfolder that is
    /// absolute or escaping, so the join cannot leave `local_path`.
    pub fn vault_root(&self) -> Option<PathBuf> {
        self.notes
            .as_ref()
            .map(|notes| self.local_path.join(notes.subfolder.trim()))
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
        // Refused here, where the user can still see the field, rather than
        // rendered literally into every commit the folder makes from now on.
        // `validate` also runs on load, so a hand-edited `config.json` cannot
        // smuggle a typo past this either.
        if let Some(unknown) =
            provenance::unknown_subject_placeholder(&self.commit_subject_template)
        {
            let known = provenance::SUBJECT_PLACEHOLDERS
                .map(|name| format!("{{{name}}}"))
                .join(", ");
            return Err(SyncError::Config(format!(
                "unknown commit subject placeholder {{{unknown}}}; keeper knows {known}"
            )));
        }
        if let Some(notes) = &self.notes {
            notes.validate()?;
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
    fn the_scan_cadence_is_floored_but_never_capped() {
        let mut p = profile();
        assert_eq!(p.effective_poll_interval_ms(), DEFAULT_POLL_INTERVAL_MS);
        // A row written before the field meant anything (DW-116) carries a zero,
        // which would re-stat the tree on every 1 Hz tick.
        p.poll_interval_ms = 0;
        assert_eq!(p.effective_poll_interval_ms(), MIN_POLL_INTERVAL_MS);
        assert!(p.validate().is_ok(), "a zero is floored, never refused");
        p.poll_interval_ms = 45_000;
        assert_eq!(p.effective_poll_interval_ms(), 45_000);
    }

    #[test]
    fn a_commit_subject_template_is_optional_and_its_placeholders_are_checked() {
        let mut p = profile();
        assert_eq!(p.commit_subject_template, "", "no template is the default");
        assert!(p.validate().is_ok());

        p.commit_subject_template = "{profile}: {changed} files".to_owned();
        assert!(p.validate().is_ok());

        // A typo caught at the edge, not rendered into every commit the folder
        // makes from here on. The message has to name it to be actionable.
        p.commit_subject_template = "{Profile} moved {count}".to_owned();
        let err = p.validate().expect_err("an unknown placeholder is refused");
        assert!(
            err.to_string().contains("{Profile}"),
            "the message must name the placeholder: {err}"
        );
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
        assert_eq!(parsed.poll_interval_ms, DEFAULT_POLL_INTERVAL_MS);
        assert_eq!(parsed.commit_subject_template, "");
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

    /// A profile persists as one JSON blob per row, so `#[serde(default)]` IS
    /// the migration — there is no SQL to run and nothing to backfill. This is
    /// the blob a 0.6.5 keeper wrote, every key it emitted and not one more:
    /// it must load as "not a vault" and validate clean, or upgrading would
    /// strand every existing folder.
    #[test]
    fn a_profile_row_written_before_notes_existed_still_loads() {
        let v065 = r#"{
            "id": "01JOLD", "name": "tgdrive", "localPath": "/home/u/tgdrive",
            "remoteUrl": "https://git.example/u/tgdrive.git", "branch": "main",
            "direction": "bidirectional", "lane": "main",
            "subpaths": [], "excludes": [], "removable": false, "volumeId": null,
            "lfsMode": "materialize", "lfsThresholdBytes": 4194304,
            "lfsPruneLocal": false, "lfsNever": [],
            "settleMs": 5000, "pollIntervalMs": 15000, "tags": [],
            "commitSubjectTemplate": "", "authorOverride": null, "enabled": true
        }"#;
        let parsed: SyncProfile = serde_json::from_str(v065).expect("a 0.6.5 row still loads");
        assert_eq!(parsed.notes, None, "an absent key means: not a vault");
        assert_eq!(parsed.vault_root(), None);
        assert!(parsed.validate().is_ok(), "and it is still a valid profile");

        // The other direction too: a row this keeper writes for a non-vault
        // folder round-trips to the same thing, so rolling back a release
        // cannot turn `notes: null` into a parse failure.
        let round = serde_json::to_string(&parsed).expect("encode");
        let back: SyncProfile = serde_json::from_str(&round).expect("decode");
        assert_eq!(parsed, back);
    }

    #[test]
    fn flagging_a_profile_puts_the_vault_in_a_subfolder_of_it() {
        let mut p = profile();
        p.notes = Some(NotesConfig::default());
        assert_eq!(
            p.vault_root(),
            Some(PathBuf::from("/home/u/tgdrive/notes")),
            "the default vault is `notes/` inside the folder keeper already syncs"
        );
        assert!(p.validate().is_ok());

        let notes = p.notes.as_mut().expect("just flagged");
        notes.subfolder = "work/journal".to_owned();
        assert_eq!(
            p.vault_root(),
            Some(PathBuf::from("/home/u/tgdrive/work/journal"))
        );
    }

    #[test]
    fn a_notes_subfolder_that_leaves_the_profile_folder_is_refused() {
        // Every one of these would put keeper's writes outside the synced
        // folder — and an ABSOLUTE one silently, because `Path::join` drops the
        // left-hand side rather than failing.
        for bad in ["", "   ", "/etc/vault", "../../elsewhere", "a/../../etc"] {
            let mut p = profile();
            p.notes = Some(NotesConfig {
                subfolder: bad.to_owned(),
                ..NotesConfig::default()
            });
            assert!(p.validate().is_err(), "{bad:?} must be refused");
        }
        // FR-121: keeper never reads, writes or walks an Obsidian configuration
        // folder, at the vault root or below it.
        for bad in [".obsidian", "vault/.obsidian", ".obsidian/notes"] {
            let mut p = profile();
            p.notes = Some(NotesConfig {
                subfolder: bad.to_owned(),
                ..NotesConfig::default()
            });
            let err = p.validate().expect_err("an .obsidian component is refused");
            assert!(
                err.to_string().contains(".obsidian"),
                "the message must say which folder it means: {err}"
            );
        }
    }

    /// The floors REFUSE rather than clamp, unlike `poll_interval_ms`. Nothing
    /// can carry a below-floor cadence by accident — the field did not exist
    /// before this release and `#[serde(default)]` fills in a working one — so
    /// a value under the floor is one a person typed, while looking at the
    /// field that is about to reject it.
    #[test]
    fn a_cadence_faster_than_its_floors_is_refused() {
        let mut p = profile();
        p.notes = Some(NotesConfig::default());
        assert!(p.validate().is_ok(), "the defaults are inside the floors");

        for cadence in [
            NotesCadence {
                commit_idle_ms: 0,
                ..NotesCadence::default()
            },
            NotesCadence {
                commit_idle_ms: MIN_COMMIT_IDLE_MS - 1,
                ..NotesCadence::default()
            },
            NotesCadence {
                push_interval_ms: MIN_PUSH_INTERVAL_MS - 1,
                ..NotesCadence::default()
            },
        ] {
            let mut p = profile();
            p.notes = Some(NotesConfig {
                cadence,
                ..NotesConfig::default()
            });
            assert!(
                p.validate().is_err(),
                "a cadence under the floor is refused"
            );
        }

        // Exactly at the floor is a choice, not a mistake.
        let mut edge = profile();
        edge.notes = Some(NotesConfig {
            cadence: NotesCadence {
                commit_idle_ms: MIN_COMMIT_IDLE_MS,
                push_interval_ms: MIN_PUSH_INTERVAL_MS,
                push_on_blur: false,
            },
            ..NotesConfig::default()
        });
        assert!(edge.validate().is_ok());
    }

    #[test]
    fn a_notes_config_fills_in_every_key_it_is_not_given() {
        // The settings form sends what the user touched; the rest has to mean
        // the documented default rather than an empty string.
        let sparse: NotesConfig = serde_json::from_str("{}").expect("parse");
        assert_eq!(sparse, NotesConfig::default());
        assert_eq!(sparse.subfolder, DEFAULT_NOTES_SUBFOLDER);
        assert_eq!(sparse.journal_template, DEFAULT_JOURNAL_TEMPLATE);
        assert_eq!(sparse.default_template, None);
        assert_eq!(sparse.cadence.commit_idle_ms, DEFAULT_COMMIT_IDLE_MS);
        assert_eq!(sparse.cadence.push_interval_ms, DEFAULT_PUSH_INTERVAL_MS);
        assert!(sparse.cadence.push_on_blur);
    }
}
