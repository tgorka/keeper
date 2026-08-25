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
//!
//! # There is one profile↔TOML mapping, and it is here
//!
//! [`accepted_profile_keys`], [`canonical_key`] and [`canonical_profile_fields`]
//! were `keeper-syncd`'s until Story 46.8 gave a folder its own
//! `.keeper/keeper.toml` (AD-99). Two readers of one shape is how a key comes
//! to mean two things, so the daemon's `[[profile]]` table and a folder's
//! `[folder]` table are canonicalized by the same three functions, against a
//! key set *derived from [`SyncProfile`]* rather than listed. See
//! [`folder`] for what a folder file may and may not carry.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
    error::{Result, SyncError},
    provenance,
};

pub mod folder;

pub use folder::{
    as_stored, folder_faults, folder_field_rule, in_force, install_folder_tier,
    installed_folder_tier, FolderFault, FolderFieldRule, FolderOutcome, FolderTier,
    FOLDER_CONFIG_DIR,
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

/// Where recordings live inside a recordings-flagged folder, by default
/// (AD-66). Its own constant beside [`DEFAULT_NOTES_SUBFOLDER`] for the reason
/// this module exists: one JSON blob has to mean the same thing to the app, to
/// `keeper-syncd` and to whatever reads `sync.db` next, so the default is
/// spelled once, here.
pub const DEFAULT_RECORDINGS_SUBFOLDER: &str = "recordings";

/// Where LLM work sessions live inside a sessions-flagged folder, by default
/// (AD-107). Its own constant beside [`DEFAULT_NOTES_SUBFOLDER`] and
/// [`DEFAULT_RECORDINGS_SUBFOLDER`] for the reason this module exists: one JSON
/// blob has to mean the same thing to the app, to `keeper-syncd` and to
/// whatever reads `sync.db` next, so the default is spelled once, here.
///
/// The value is the zone name both live drives use (`60-sessions/` on tgdrive
/// and neuradrive), not an invention of keeper's — the flag adopts a layout the
/// operator already runs.
pub const DEFAULT_SESSIONS_SUBFOLDER: &str = "60-sessions";

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
    /// Vault-relative path of the template a **quick capture** starts from, if
    /// any (Story 45.16, FR-193).
    ///
    /// Its own setting rather than a reuse of `default_template`, because the
    /// two answer different questions: a capture is two seconds of typing into
    /// a small window and wants a scaffold shaped for that, while the vault
    /// default shapes every note the app creates. Absent means the capture path
    /// falls through to `default_template`, which is exactly what every vault
    /// did before this field existed.
    #[serde(default)]
    pub capture_template: Option<String>,
    /// The tag every quick capture carries, if any (Story 45.16, FR-193).
    ///
    /// Stored in the canonical form `keeper_core::notes::seed::capture_tag`
    /// produces, so the settings form and the note's frontmatter cannot show
    /// two spellings of one tag.
    ///
    /// **`None` by default, and that default is load-bearing.** Story 44.3
    /// seeds Inbox as `is:untagged`, so any capture tag files every captured
    /// thought straight out of the one space designed to receive it — the same
    /// hazard 44.7 refused when it shipped its templates untagged. Defaulting
    /// this to a value would change what an existing vault does on upgrade,
    /// with nobody having asked for it.
    #[serde(default)]
    pub capture_tag: Option<String>,
    #[serde(default)]
    pub cadence: NotesCadence,
}

impl Default for NotesConfig {
    fn default() -> Self {
        Self {
            subfolder: DEFAULT_NOTES_SUBFOLDER.to_owned(),
            journal_template: DEFAULT_JOURNAL_TEMPLATE.to_owned(),
            default_template: None,
            capture_template: None,
            capture_tag: None,
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

/// What this clone keeps of the recordings subtree (AD-66).
///
/// Distinct from the profile-wide [`LfsMode`] because the two answer different
/// questions. `LfsMode` is about the whole repository; this is about one
/// subtree, and a folder can very reasonably want its notes and its documents
/// on disk while its video stays a pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MediaPolicy {
    /// Keep the media bytes on this machine.
    ///
    /// The default, and the honest one: it prefers a self-sufficient local copy
    /// to a smaller one. This is also the machine the recorder writes to, so
    /// the bytes are here regardless — the choice is only whether git keeps
    /// them too. `lfs_prune_local` once shared this rationale and no longer
    /// does, because what that releases is a second copy of content the
    /// worktree still holds, where this is the content itself.
    #[default]
    Materialize,
    /// Leave the recordings subtree as LFS pointer files on this clone.
    ///
    /// This exists for the phone-sized clone: the one that wants the session
    /// list, the notes and the transcripts, and emphatically not forty
    /// gigabytes of screen capture. It is a per-clone answer, which is why it
    /// lives on the profile rather than in the repository.
    PointerOnly,
}

/// When a committed recording is published (FR-136, AD-70).
///
/// Durability and publication are different promises, and only the first is
/// cheap. Committing a closed segment is local and immediate, so it always
/// happens at close; pushing is neither — a long session is a multi-gigabyte
/// LFS object, and sending it while the meeting is still running eats the
/// uplink the meeting runs on. So the push is a policy rather than a reflex,
/// and it sits on the profile because the folder is what knows what its remote
/// costs.
///
/// Internally tagged like [`crate::engine::PendingReason`] and
/// [`crate::copy::CopyOutcome`], which is what keeps old blobs readable: a row
/// written today carries `{"kind":"sessionEnd"}`, and a fourth policy added
/// later neither renumbers nor reshapes it. `rename_all` renames the VARIANTS
/// only — `rename_all_fields` is what reaches the payload of a struct variant,
/// and without it `quiet_from` would cross the boundary as snake_case while
/// every neighbouring field is camelCase.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum PushPolicy {
    /// Push each segment as it is committed. For an uplink the recording does
    /// not have to share, and for the person who wants the bytes off this
    /// machine before the lid can be shut on it.
    Immediate,
    /// Hold every segment locally and push once, when the session finalizes.
    ///
    /// The default, and the reason the type exists: the meeting needs the
    /// uplink more than the archive needs the head start. Nothing is at risk
    /// while it waits — the segments are committed, which is the promise the
    /// recorder actually made.
    #[default]
    SessionEnd,
    /// Defer to quiet hours, as `HH:MM` local wall-clock times.
    ///
    /// The window wraps midnight, so `22:00`–`06:00` is one window and not two;
    /// that is the shape a person actually wants, and refusing it would push
    /// every metered link back onto `SessionEnd`.
    Window {
        quiet_from: String,
        quiet_to: String,
    },
}

/// The recordings flag on a profile, and everything it configures (AD-66).
///
/// `Some` on a [`SyncProfile`] means "this synced folder holds recordings", and
/// `subfolder` says where inside it. This is [`NotesConfig`] applied a second
/// time, deliberately: same `#[serde(default)]` migration, same
/// refuse-rather-than-correct validation, and the same absence of an object
/// with a lifetime of its own — the recordings root's identity *is* the profile
/// id, so there is no picker, no second path to validate and nothing to import.
///
/// Absent is not "recordings with the defaults". A profile with no block holds
/// no recordings, and nothing here writes a default block into one as a side
/// effect of loading it: that would quietly nominate every synced folder on the
/// machine as a recording destination on the first upgrade.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingsConfig {
    /// The recordings root, relative to `local_path`. Never empty, never
    /// absolute, never escaping, and never overlapping this profile's notes
    /// vault — see [`RecordingsConfig::validate`] for why each is refused
    /// rather than corrected.
    #[serde(default = "default_recordings_subfolder")]
    pub subfolder: String,
    #[serde(default)]
    pub media: MediaPolicy,
    #[serde(default)]
    pub push: PushPolicy,
}

impl Default for RecordingsConfig {
    fn default() -> Self {
        Self {
            subfolder: DEFAULT_RECORDINGS_SUBFOLDER.to_owned(),
            media: MediaPolicy::default(),
            push: PushPolicy::default(),
        }
    }
}

impl RecordingsConfig {
    /// The subfolder, quiet-window and notes-overlap rules, split out of
    /// [`SyncProfile::validate`] for the reason [`NotesConfig::validate`] is:
    /// they are about these fields and not about the profile.
    ///
    /// The profile's notes block is passed IN rather than reached for, because
    /// the rule this adds on top of the notes template is a rule about a pair.
    /// One folder cannot be both a vault and a recordings root: two subsystems
    /// would write the same tree, each with its own idea of what belongs in it,
    /// and the first thing the user would notice is the notes indexer walking a
    /// multi-gigabyte `.mp4`.
    ///
    /// Public where `NotesConfig::validate` is private, because this one takes
    /// an argument the caller has to supply: checking a *candidate* block
    /// against a stored profile is a thing the settings path does before there
    /// is a `SyncProfile` to validate.
    ///
    /// Every rule REFUSES rather than corrects, exactly as the notes rules do
    /// and for the same reason: no profile can carry a bad value by accident —
    /// the field did not exist before this release and `#[serde(default)]` puts
    /// a working one into every block that mentions it — so a bad value is one
    /// a person typed while looking at the field that is about to reject it.
    ///
    /// One notes rule is deliberately absent: FR-121's `.obsidian` refusal is
    /// about where a *vault* may point, and the overlap rule below already
    /// keeps a recordings root out of the vault it would collide with.
    pub fn validate(&self, notes: Option<&NotesConfig>) -> Result<()> {
        let subfolder = self.subfolder.trim();
        if subfolder.is_empty() {
            return Err(SyncError::Config(
                "recordings subfolder must not be empty: recordings live in a folder inside the \
                 profile, never at the profile root"
                    .into(),
            ));
        }
        // `Path::join` with an absolute right-hand side DISCARDS the left one,
        // so an absolute subfolder would not fail loudly — `recordings_root`
        // would quietly point somewhere outside the synced folder entirely, and
        // story 41.4's "is this path inside the recordings root" guard would
        // start answering yes for paths the profile does not own. Tested as a
        // string as well as through `is_absolute`, because absoluteness is
        // platform-shaped (`C:\x` and `\\server\share` are absolute only to
        // Windows) and one profile row has to mean the same thing on every
        // machine that reads it.
        if Path::new(subfolder).is_absolute()
            || subfolder.starts_with('/')
            || subfolder.starts_with('\\')
        {
            return Err(SyncError::Config(format!(
                "recordings subfolder must be relative to the profile folder, got {subfolder}"
            )));
        }
        if subfolder.split(['/', '\\']).any(|c| c == "..") {
            return Err(SyncError::Config(format!(
                "recordings subfolder must not escape the profile folder: {subfolder}"
            )));
        }
        if let PushPolicy::Window {
            quiet_from,
            quiet_to,
        } = &self.push
        {
            validate_quiet_time("quiet_from", quiet_from)?;
            validate_quiet_time("quiet_to", quiet_to)?;
            // Not a wrapping window and not a full day — just ambiguous. Rather
            // than pick one of the two readings and be wrong half the time, say
            // so while the person who typed it is still looking at the field.
            if quiet_from == quiet_to {
                return Err(SyncError::Config(format!(
                    "recordings push window must not start and end at {quiet_from}: a window \
                     that opens and closes at the same moment says nothing about when to push"
                )));
            }
        }
        if let Some(notes) = notes {
            let vault = notes.subfolder.trim();
            if subfolders_overlap(subfolder, vault) {
                return Err(SyncError::Config(format!(
                    "recordings subfolder {subfolder} overlaps notes subfolder {vault}: one \
                     folder cannot be both a vault and a recordings root"
                )));
            }
        }
        Ok(())
    }
}

/// The sessions flag on a profile, and where the zone lives (AD-107, FR-222).
///
/// `Some` on a [`SyncProfile`] means "this synced folder contains a sessions
/// zone" — the `60-sessions/` layout: `active/`, `archive/YYYY/`, `_template/`,
/// one folder per LLM work session. The root list is a filter over the profile
/// list and the root id *is* the profile id, exactly as [`NotesConfig`] made a
/// vault.
///
/// This crate stores the flag and understands nothing behind it. Session
/// discovery, the promote table, lineage and freshness are `keeper-core`'s; the
/// engine sees a folder with files in it, exactly as it does for every other
/// profile. In particular the zone's `workspace/` subtrees are kept out of
/// version control by the drive's own ignore rules, not by anything this crate
/// does with the flag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionsConfig {
    /// The zone root, relative to `local_path`. Never empty, never absolute,
    /// never escaping, and never overlapping this profile's notes vault or
    /// recordings root — see [`SessionsConfig::validate`] for why each is
    /// refused rather than corrected.
    #[serde(default = "default_sessions_subfolder")]
    pub subfolder: String,
}

impl Default for SessionsConfig {
    fn default() -> Self {
        Self {
            subfolder: DEFAULT_SESSIONS_SUBFOLDER.to_owned(),
        }
    }
}

impl SessionsConfig {
    /// The subfolder rules, split out of [`SyncProfile::validate`] for the
    /// reason [`NotesConfig::validate`] is: they are about this field and not
    /// about the profile.
    ///
    /// The profile's notes and recordings blocks are passed IN rather than
    /// reached for, because the overlap rules are rules about pairs: one folder
    /// cannot be both a vault and a sessions zone (the notes indexer and the
    /// sessions indexer would each claim the same markdown), and cannot be both
    /// a recordings root and a sessions zone (the recorder would write media
    /// into a tree whose contract says media belongs elsewhere).
    ///
    /// Public where `NotesConfig::validate` is private, for the reason
    /// [`RecordingsConfig::validate`] is: checking a *candidate* block against
    /// a stored profile is a thing the settings path does before there is a
    /// `SyncProfile` to validate.
    ///
    /// Every rule REFUSES rather than corrects, exactly as the notes rules do
    /// and for the same reason: a value here is something a person typed, and a
    /// person who typed it is looking at the field.
    pub fn validate(
        &self,
        notes: Option<&NotesConfig>,
        recordings: Option<&RecordingsConfig>,
    ) -> Result<()> {
        let subfolder = self.subfolder.trim();
        if subfolder.is_empty() {
            return Err(SyncError::Config(
                "sessions subfolder must not be empty: a sessions zone is a folder inside the \
                 profile, never the profile root"
                    .into(),
            ));
        }
        // `Path::join` with an absolute right-hand side DISCARDS the left one,
        // so an absolute subfolder would not fail loudly — the zone root would
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
                "sessions subfolder must be relative to the profile folder, got {subfolder}"
            )));
        }
        if subfolder.split(['/', '\\']).any(|c| c == "..") {
            return Err(SyncError::Config(format!(
                "sessions subfolder must not escape the profile folder: {subfolder}"
            )));
        }
        if let Some(notes) = notes {
            let vault = notes.subfolder.trim();
            if subfolders_overlap(subfolder, vault) {
                return Err(SyncError::Config(format!(
                    "sessions subfolder {subfolder} overlaps notes subfolder {vault}: one \
                     folder cannot be both a vault and a sessions zone"
                )));
            }
        }
        if let Some(recordings) = recordings {
            let rec = recordings.subfolder.trim();
            if subfolders_overlap(subfolder, rec) {
                return Err(SyncError::Config(format!(
                    "sessions subfolder {subfolder} overlaps recordings subfolder {rec}: one \
                     folder cannot be both a recordings root and a sessions zone"
                )));
            }
        }
        Ok(())
    }
}

/// The path components of a profile-relative subfolder.
///
/// Empty and `.` components are dropped so `./a//b` compares as `a/b`, and both
/// separators are honoured because one profile row is read on every platform.
fn subfolder_components(subfolder: &str) -> impl Iterator<Item = &str> {
    subfolder
        .trim()
        .split(['/', '\\'])
        .filter(|part| !part.is_empty() && *part != ".")
}

/// Whether one profile-relative subfolder is the other, or contains it.
///
/// Component-wise, and that is the whole point: `rec` is a string prefix of
/// `recordings` and is not an ancestor of it, so a rule written with
/// `starts_with` would refuse an ordinary pair of sibling folders. Comparing
/// components, one of the two runs out first and everything before that matched
/// exactly, or it did not — which is containment in either direction, equality
/// included, and nothing else.
///
/// ASCII-case-insensitive, unlike anything else in this file, because the check
/// asks whether two subsystems would write the same tree and the volumes this
/// ships on answer that question themselves: APFS and exFAT in their default
/// configurations hold `Vault` and `vault` in one directory. ASCII only —
/// full Unicode folding is filesystem-specific enough that guessing at it would
/// be less honest than not trying.
fn subfolders_overlap(a: &str, b: &str) -> bool {
    subfolder_components(a)
        .zip(subfolder_components(b))
        .all(|(x, y)| x.eq_ignore_ascii_case(y))
}

/// One end of a [`PushPolicy::Window`], as a 24-hour local wall-clock time.
///
/// Refused rather than coerced, like every other field here: a window nobody
/// can parse is a window that would silently never open, and a recording that
/// is never pushed is exactly the failure this policy exists to schedule.
fn validate_quiet_time(field: &'static str, value: &str) -> Result<()> {
    let malformed = || {
        SyncError::Config(format!(
            "recordings push {field} must be a 24-hour HH:MM local time, got {value}"
        ))
    };
    let (hours, minutes) = value.split_once(':').ok_or_else(malformed)?;
    let two_digits = |part: &str| part.len() == 2 && part.bytes().all(|byte| byte.is_ascii_digit());
    if !two_digits(hours) || !two_digits(minutes) {
        return Err(malformed());
    }
    let hours: u8 = hours.parse().map_err(|_| malformed())?;
    let minutes: u8 = minutes.parse().map_err(|_| malformed())?;
    if hours > 23 || minutes > 59 {
        return Err(malformed());
    }
    Ok(())
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
    ///
    /// Both consumers read this list through [`crate::sparse::SparseCone`],
    /// which is where "inside the cone" is defined — it is wider than it looks,
    /// and the two must not disagree about it (Story 27.2).
    #[serde(default)]
    pub subpaths: Vec<String>,
    /// Additional tier-0 exclusion globs, on top of the built-in set.
    #[serde(default)]
    pub excludes: Vec<String>,
    /// Paths whose content is deterministically rebuildable from the rest of
    /// the tree, matched with the same anchoring rule as [`Self::excludes`].
    ///
    /// These are still synced like anything else; the list only changes what
    /// happens when they diverge. A generated listing that two devices both
    /// rewrote does not need the local revision preserved beside it (AD-43) —
    /// the tool that wrote it will produce the correct file again from inputs
    /// that are themselves synced, so the conflict copy is litter rather than
    /// recovery material. Divergence therefore resolves to the remote silently.
    ///
    /// Empty by default, and deliberately so: keeper cannot tell which of a
    /// user's files are generated, and guessing wrong here **loses an edit**.
    /// Only a path the user has declared regenerable is treated as one.
    #[serde(default)]
    pub regenerable: Vec<String>,
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
    /// **On by default.** The copy this releases is redundant by construction,
    /// and the three conditions in [`crate::lfs::prune`] are what make that a
    /// fact rather than a hope: nothing the journal still owes, nothing the
    /// remote has not confirmed it holds, and nothing whose worktree file is
    /// not standing there to rebuild it from at the cost of one local read.
    ///
    /// What `false` buys is the offline restore of a file the worktree later
    /// loses — the drive stays self-sufficient without the network. That is a
    /// real want on a machine that is often off it, which is why the opt-out
    /// stays; it is not the common case, and as the *default* it meant the
    /// second copy grew unwatched until a disk filled. A store written before
    /// the change is carried over by [`crate::db`]'s one-shot migration.
    #[serde(default = "default_true")]
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
    /// Repository-relative globs whose **content** may stay unmaterialized
    /// after a pull (AD-122, FR-328).
    ///
    /// Gitignore dialect, the same one every other pattern field in this record
    /// uses: a pattern with no `/` matches that basename at any depth,
    /// otherwise it is anchored at the repository root; `!` negates, and a
    /// negated line wins unconditionally over a matching one, because erring
    /// toward keeping bytes is the only safe direction. A path listed here is
    /// still tracked, still committed and still wanted — only its bytes are
    /// allowed to be absent, and the pointer is what answers for it.
    ///
    /// This is **not** `lfs_never` directly above, which is very nearly the
    /// opposite: `lfs_never` says "never route this through LFS at all", so a
    /// file matched there has no pointer to stay away behind. A file matched
    /// here is one keeper deliberately routed through LFS and may now leave in
    /// the store.
    ///
    /// `#[serde(default)]` here IS the migration, exactly as it is for `notes`
    /// below and for the same reason: a row written by a keeper that had never
    /// heard of virtualization simply has no key, so it loads as an empty list
    /// and says nothing. No SQL change, no schema bump, nothing to run on
    /// upgrade, and a row written by this keeper still loads on the older one.
    #[serde(default)]
    pub virtual_patterns: Vec<String>,
    /// Smallest size, in bytes, a matched path may stay unmaterialized at
    /// (inclusive). `0` means no floor.
    ///
    /// Every term of the policy has to be answerable from the LFS pointer and
    /// never from the bytes (AD-122) — reading the bytes to decide whether the
    /// bytes may be absent is circular — and a size is, because the pointer
    /// carries it. That is also why the floor lives here and has no spelling in
    /// the committed pattern file: a size is not expressible in gitignore
    /// dialect.
    ///
    /// No named `default_*` fn, unlike `lfs_threshold_bytes` above: there the
    /// correct default is a real number a constant has to hold, whereas here it
    /// really is zero, so `Default` is already the right answer and a named fn
    /// would only be a second place for it to disagree from.
    #[serde(default)]
    pub virtual_over_bytes: u64,
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
    /// This folder holds recordings, and where inside it (AD-66).
    ///
    /// `#[serde(default)]` here IS the migration, exactly as it is for `notes`
    /// directly above and for the same reason: a row written by a keeper that
    /// had never heard of recordings simply has no `recordings` key, so it
    /// loads as `None` and says nothing. No SQL change, no schema bump, nothing
    /// to run on upgrade, and a row written by this keeper still loads on the
    /// older one.
    ///
    /// `None` is "holds no recordings", never "recordings with the defaults".
    #[serde(default)]
    pub recordings: Option<RecordingsConfig>,
    /// This folder contains a sessions zone, and where inside it (AD-107).
    ///
    /// `#[serde(default)]` here IS the migration, exactly as it is for `notes`
    /// and `recordings` above and for the same reason: a row written by a
    /// keeper that had never heard of sessions simply has no `sessions` key, so
    /// it loads as `None` and says nothing. No SQL change, no schema bump,
    /// nothing to run on upgrade, and a row written by this keeper still loads
    /// on the older one.
    ///
    /// `None` is "holds no sessions zone", never "sessions with the defaults".
    #[serde(default)]
    pub sessions: Option<SessionsConfig>,
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
fn default_recordings_subfolder() -> String {
    DEFAULT_RECORDINGS_SUBFOLDER.to_owned()
}
fn default_sessions_subfolder() -> String {
    DEFAULT_SESSIONS_SUBFOLDER.to_owned()
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
            regenerable: Vec::new(),
            removable: false,
            volume_id: None,
            lfs_mode: LfsMode::Materialize,
            lfs_threshold_bytes: DEFAULT_LFS_THRESHOLD_BYTES,
            lfs_never: Vec::new(),
            virtual_patterns: Vec::new(),
            virtual_over_bytes: 0,
            lfs_prune_local: true,
            settle_ms: DEFAULT_SETTLE_MS,
            poll_interval_ms: DEFAULT_POLL_INTERVAL_MS,
            tags: Vec::new(),
            commit_subject_template: String::new(),
            author_override: None,
            enabled: true,
            notes: None,
            recordings: None,
            sessions: None,
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

    /// The recordings root — `local_path` joined with the recordings subfolder
    /// — or `None` when this profile holds no recordings.
    ///
    /// Beside [`Self::vault_root`] and for the same reason: the flag and the
    /// folder are stored apart, so "where do this profile's recordings live"
    /// gets one answer rather than one per caller. [`Self::validate`] has
    /// already refused a subfolder that is absolute or escaping, so the join
    /// cannot leave `local_path` — which is what lets a caller treat this as
    /// a boundary rather than a hint.
    pub fn recordings_root(&self) -> Option<PathBuf> {
        self.recordings
            .as_ref()
            .map(|recordings| self.local_path.join(recordings.subfolder.trim()))
    }

    /// The sessions zone root — `local_path` joined with the sessions
    /// subfolder — or `None` when this profile holds no sessions zone.
    ///
    /// Beside [`Self::vault_root`] and [`Self::recordings_root`] and for the
    /// same reason: the flag and the folder are stored apart, so "where does
    /// this profile's sessions zone live" gets one answer rather than one per
    /// caller. [`Self::validate`] has already refused a subfolder that is
    /// absolute or escaping, so the join cannot leave `local_path`.
    pub fn sessions_root(&self) -> Option<PathBuf> {
        self.sessions
            .as_ref()
            .map(|sessions| self.local_path.join(sessions.subfolder.trim()))
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
        // After the notes block, and given it: the overlap rule compares the
        // two subfolders, and comparing against one that has not been checked
        // yet would report the wrong problem.
        if let Some(recordings) = &self.recordings {
            recordings.validate(self.notes.as_ref())?;
        }
        // After both, and given both: the sessions overlap rules compare
        // against the notes and recordings subfolders, and comparing against
        // one that has not been checked yet would report the wrong problem.
        if let Some(sessions) = &self.sessions {
            sessions.validate(self.notes.as_ref(), self.recordings.as_ref())?;
        }
        Ok(())
    }
}

/// Every key a profile table may carry, derived from [`SyncProfile`].
///
/// Derived, not listed. A hand-maintained list is a list that goes stale the
/// first time the engine gains a field, and the failure mode is the worst one
/// available: the daemon rejects a profile the app just wrote.
///
/// Shared by `keeper-syncd`'s `[[profile]]` tables and by a folder's own
/// `[folder]` table. What a *folder* may set is a strictly narrower question,
/// answered by [`folder_field_rule`] against this same set.
pub fn accepted_profile_keys() -> Result<BTreeSet<String>> {
    let probe = SyncProfile::new("id", "name", "/", "remote");
    let value = serde_json::to_value(&probe).map_err(|err| {
        SyncError::Config(format!("cannot enumerate the accepted profile keys: {err}"))
    })?;
    match value {
        serde_json::Value::Object(map) => Ok(map.into_iter().map(|(key, _)| key).collect()),
        _ => Err(SyncError::Config(
            "cannot enumerate the accepted profile keys: a profile is not an object".to_owned(),
        )),
    }
}

/// Fold a snake_case key onto the camelCase spelling [`SyncProfile`] uses.
///
/// TOML convention is snake_case and the profile schema is camelCase (it is the
/// app's JSON schema). Accepting both keeps the 1:1 copy-a-table promise while
/// not making anyone write `localPath` in a `.toml` file.
pub fn canonical_key(key: &str) -> String {
    if !key.contains('_') {
        return key.to_owned();
    }
    let mut out = String::with_capacity(key.len());
    for (index, part) in key.split('_').filter(|part| !part.is_empty()).enumerate() {
        let mut chars = part.chars();
        match chars.next() {
            None => {}
            Some(first) if index == 0 => {
                out.push(first);
                out.push_str(chars.as_str());
            }
            Some(first) => {
                out.extend(first.to_uppercase());
                out.push_str(chars.as_str());
            }
        }
    }
    out
}

/// Fold a table of profile fields onto the canonical spellings, refusing a key
/// [`SyncProfile`] does not have and a key given twice under two spellings.
///
/// Errors carry **no positional prefix**: the daemon names the `[[profile]]`
/// table and its index, the folder tier names the file, and neither wants the
/// other's sentence. Both wrap the message they get here.
pub fn canonical_profile_fields(
    fields: serde_json::Map<String, serde_json::Value>,
    accepted: &BTreeSet<String>,
) -> Result<serde_json::Map<String, serde_json::Value>> {
    let mut canonical = serde_json::Map::with_capacity(fields.len());
    for (key, field) in fields {
        let name = canonical_key(&key);
        if !accepted.contains(&name) {
            return Err(SyncError::Config(format!(
                "unknown key `{key}`; accepted keys are {}",
                accepted.iter().cloned().collect::<Vec<_>>().join(", ")
            )));
        }
        if canonical.insert(name.clone(), field).is_some() {
            // `local_path` and `localPath` in one table: the survivor would
            // depend on map ordering, so refuse rather than pick.
            return Err(SyncError::Config(format!(
                "key `{name}` is given twice \
                 (camelCase and snake_case spellings are the same key)"
            )));
        }
    }
    Ok(canonical)
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

    /// Moved here from `keeper-syncd` when the folder tier became the second
    /// caller (Story 46.8). TOML convention is snake_case and the profile
    /// schema is the app's camelCase JSON one; both spellings are one key, in
    /// a `[[profile]]` table and in a `[folder]` table alike.
    #[test]
    fn canonical_key_folds_snake_case_onto_the_profile_schema() {
        assert_eq!(canonical_key("lfs_threshold_bytes"), "lfsThresholdBytes");
        assert_eq!(canonical_key("localPath"), "localPath");
        assert_eq!(canonical_key("id"), "id");
        assert_eq!(canonical_key("subpaths"), "subpaths");
    }

    /// The accepted set is derived from the type, so it cannot go stale when a
    /// field is added — which is the whole reason it is not a list.
    #[test]
    fn the_accepted_key_set_is_every_serialized_profile_field() {
        let keys = accepted_profile_keys().expect("keys");
        let serialized = serde_json::to_value(profile()).expect("serialize");
        let expected: BTreeSet<String> = serialized
            .as_object()
            .expect("a profile is an object")
            .keys()
            .cloned()
            .collect();
        assert_eq!(keys, expected);
        assert!(keys.contains("localPath") && keys.contains("lfsThresholdBytes"));
    }

    /// One key under two spellings has no defensible winner: the survivor would
    /// depend on map ordering, so both callers refuse rather than pick.
    #[test]
    fn one_key_under_two_spellings_is_refused_rather_than_resolved() {
        let accepted = accepted_profile_keys().expect("keys");
        let mut fields = serde_json::Map::new();
        fields.insert("local_path".to_owned(), serde_json::json!("/a"));
        fields.insert("localPath".to_owned(), serde_json::json!("/b"));
        let err = canonical_profile_fields(fields, &accepted).expect_err("refused");
        assert!(err.to_string().contains("given twice"), "{err}");

        let mut unknown = serde_json::Map::new();
        unknown.insert("remoteUrll".to_owned(), serde_json::json!("/a"));
        let err = canonical_profile_fields(unknown, &accepted).expect_err("refused");
        assert!(
            err.to_string().contains("unknown key `remoteUrll`"),
            "{err}"
        );
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
    /// Two defaults for one field is how they drift: `SyncProfile::new` answers
    /// for a folder the app adds, the serde default answers for a `[[profile]]`
    /// table that omits the key. They have to say the same thing.
    #[test]
    fn a_new_profile_and_an_unset_key_agree_about_the_redundant_copy() {
        let fresh = SyncProfile::new("01N", "n", "/w/n", "https://git.example/r.git");
        assert!(
            fresh.lfs_prune_local,
            "the second copy is redundant by construction; keeping it is the choice"
        );
        let sparse: SyncProfile = serde_json::from_str(
            r#"{"id":"01N","name":"n","localPath":"/w/n",
                "remoteUrl":"https://git.example/r.git","branch":"main",
                "direction":"bidirectional","lane":"main","lfsMode":"materialize"}"#,
        )
        .expect("a table that omits the key still parses");
        assert_eq!(sparse.lfs_prune_local, fresh.lfs_prune_local);
    }

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

    /// `#[serde(default)]` is the whole migration for the two virtualization
    /// keys (AD-122): a row written before this story has neither, and must
    /// load as "nothing may stay away, and no floor" rather than fail to parse
    /// — there is no SQL change and nothing to run on upgrade.
    #[test]
    fn a_profile_row_written_before_virtualization_existed_still_loads() {
        let older = r#"{
            "id": "01JOLD", "name": "tgdrive", "localPath": "/home/u/tgdrive",
            "remoteUrl": "https://git.example/u/tgdrive.git", "branch": "main",
            "direction": "bidirectional", "lane": "main",
            "subpaths": [], "excludes": [], "removable": false, "volumeId": null,
            "lfsMode": "materialize", "lfsThresholdBytes": 4194304,
            "lfsPruneLocal": false, "lfsNever": ["*.csv"],
            "settleMs": 5000, "pollIntervalMs": 15000, "tags": [],
            "commitSubjectTemplate": "", "authorOverride": null, "enabled": true
        }"#;
        let parsed: SyncProfile = serde_json::from_str(older).expect("an older row still loads");
        assert!(
            parsed.virtual_patterns.is_empty(),
            "an absent key means: nothing's content may stay away"
        );
        assert_eq!(
            parsed.virtual_over_bytes, 0,
            "and no floor, which is the only default that cannot surprise anyone"
        );
        assert_eq!(
            parsed.lfs_never,
            vec!["*.csv".to_owned()],
            "the neighbouring field it must never be confused with is untouched"
        );
        assert!(parsed.validate().is_ok(), "and it is still a valid profile");

        // The other direction too: the row this keeper writes carries both new
        // keys and round-trips, so rolling a release back cannot turn them into
        // a parse failure.
        let round = serde_json::to_string(&parsed).expect("encode");
        assert!(
            round.contains("\"virtualPatterns\"") && round.contains("\"virtualOverBytes\""),
            "both keys must be present once written: {round}"
        );
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
        // Story 45.16. `None` here is not laziness: Inbox is `is:untagged`, so
        // a shipped capture tag would file every captured thought out of the
        // space 44.3 seeds to receive them, in a vault the user never edited.
        assert_eq!(sparse.capture_template, None);
        assert_eq!(sparse.capture_tag, None);
        assert_eq!(sparse.cadence.commit_idle_ms, DEFAULT_COMMIT_IDLE_MS);
        assert_eq!(sparse.cadence.push_interval_ms, DEFAULT_PUSH_INTERVAL_MS);
        assert!(sparse.cadence.push_on_blur);
    }

    /// The same argument as the notes row above, one release later: the blob a
    /// 0.6.5 keeper wrote has no `recordings` key, and it has to load as "holds
    /// no recordings" and validate clean, or upgrading would strand every
    /// existing folder.
    #[test]
    fn a_profile_row_written_before_recordings_existed_still_loads() {
        let v065 = r#"{
            "id": "01JOLD", "name": "tgdrive", "localPath": "/home/u/tgdrive",
            "remoteUrl": "https://git.example/u/tgdrive.git", "branch": "main",
            "direction": "bidirectional", "lane": "main",
            "subpaths": [], "excludes": [], "removable": false, "volumeId": null,
            "lfsMode": "materialize", "lfsThresholdBytes": 4194304,
            "lfsPruneLocal": false, "lfsNever": [],
            "settleMs": 5000, "pollIntervalMs": 15000, "tags": [],
            "commitSubjectTemplate": "", "authorOverride": null, "enabled": true,
            "notes": null
        }"#;
        let parsed: SyncProfile = serde_json::from_str(v065).expect("a 0.6.5 row still loads");
        assert_eq!(
            parsed.recordings, None,
            "an absent key means: holds no recordings"
        );
        assert_eq!(parsed.recordings_root(), None);
        assert!(parsed.validate().is_ok(), "and it is still a valid profile");

        // And back: a row this keeper writes for a folder that holds no
        // recordings round-trips to the same thing, so rolling back a release
        // cannot turn `recordings: null` into a parse failure.
        let round = serde_json::to_string(&parsed).expect("encode");
        let back: SyncProfile = serde_json::from_str(&round).expect("decode");
        assert_eq!(parsed, back);
    }

    /// The serialized form is asserted literally, not just round-tripped. A
    /// round trip passes even if the enum's tagging changes underneath it —
    /// and a tagging change is precisely what would make every stored blob
    /// unreadable, which is the one failure `#[serde(default)]` cannot save us
    /// from.
    #[test]
    fn a_configured_recordings_block_round_trips_including_its_push_variant() {
        let configured = RecordingsConfig {
            subfolder: "sessions/raw".to_owned(),
            media: MediaPolicy::PointerOnly,
            push: PushPolicy::Window {
                quiet_from: "22:00".to_owned(),
                quiet_to: "06:00".to_owned(),
            },
        };
        let json = serde_json::to_string(&configured).expect("encode");
        assert_eq!(
            json,
            r#"{"subfolder":"sessions/raw","media":"pointerOnly","push":{"kind":"window","quietFrom":"22:00","quietTo":"06:00"}}"#
        );
        let back: RecordingsConfig = serde_json::from_str(&json).expect("decode");
        assert_eq!(configured, back);

        // Through the profile blob, which is how it actually persists.
        let mut p = profile();
        p.recordings = Some(configured.clone());
        assert!(p.validate().is_ok());
        let round = serde_json::to_string(&p).expect("encode");
        let back: SyncProfile = serde_json::from_str(&round).expect("decode");
        assert_eq!(back.recordings, Some(configured));
        assert_eq!(back, p);
    }

    #[test]
    fn a_recordings_config_fills_in_every_key_it_is_not_given() {
        // The settings form sends what the user touched; the rest has to mean
        // the documented default rather than an empty string.
        let sparse: RecordingsConfig = serde_json::from_str("{}").expect("parse");
        assert_eq!(sparse, RecordingsConfig::default());
        assert_eq!(sparse.subfolder, DEFAULT_RECORDINGS_SUBFOLDER);
        // The recorder's own bytes stay on the machine that recorded them.
        assert_eq!(sparse.media, MediaPolicy::Materialize);
        // Pushing a multi-gigabyte object during the meeting eats the uplink
        // the meeting runs on, so publication waits for the session to end.
        assert_eq!(sparse.push, PushPolicy::SessionEnd);
    }

    #[test]
    fn flagging_a_profile_puts_recordings_in_a_subfolder_of_it() {
        let mut p = profile();
        assert_eq!(p.recordings_root(), None, "unconfigured is not a root");

        p.recordings = Some(RecordingsConfig::default());
        assert_eq!(
            p.recordings_root(),
            Some(PathBuf::from("/home/u/tgdrive/recordings")),
            "the default root is `recordings/` inside the folder keeper already syncs"
        );
        assert!(p.validate().is_ok());

        let recordings = p.recordings.as_mut().expect("just flagged");
        recordings.subfolder = "media/sessions".to_owned();
        assert_eq!(
            p.recordings_root(),
            Some(PathBuf::from("/home/u/tgdrive/media/sessions"))
        );
    }

    #[test]
    fn a_recordings_subfolder_that_leaves_the_profile_folder_is_refused() {
        // Every one of these would put a recorder's output outside the synced
        // folder — and an ABSOLUTE one silently, because `Path::join` drops the
        // left-hand side rather than failing.
        for bad in ["", "   ", "/Users/x/rec", "../evil", "a/../..", ".."] {
            let mut p = profile();
            p.recordings = Some(RecordingsConfig {
                subfolder: bad.to_owned(),
                ..RecordingsConfig::default()
            });
            assert!(p.validate().is_err(), "{bad:?} must be refused");
            // Refused at construction, not deferred to the first use.
            assert!(
                matches!(p.validate(), Err(SyncError::Config(_))),
                "{bad:?} must be a typed config error"
            );
        }
    }

    /// One folder cannot be both a vault and a recordings root: the notes
    /// indexer and the recorder would write the same tree. Symmetric, because
    /// which one was configured first says nothing about which one is wrong.
    #[test]
    fn recordings_and_notes_may_not_overlap_in_either_direction() {
        for (vault, rec) in [
            ("vault", "vault"),
            ("vault", "vault/rec"),
            ("vault/notes", "vault"),
        ] {
            let mut p = profile();
            p.notes = Some(NotesConfig {
                subfolder: vault.to_owned(),
                ..NotesConfig::default()
            });
            p.recordings = Some(RecordingsConfig {
                subfolder: rec.to_owned(),
                ..RecordingsConfig::default()
            });
            let err = p
                .validate()
                .expect_err("an overlapping pair must be refused");
            let message = err.to_string();
            assert!(
                message.contains(vault) && message.contains(rec),
                "the message must name both folders, got: {message}"
            );
        }
    }

    /// The overlap rule compares path COMPONENTS. Written with `starts_with` it
    /// would read `rec` as an ancestor of `recordings` and refuse two ordinary
    /// sibling folders.
    #[test]
    fn a_shared_name_prefix_is_not_an_overlap() {
        for (vault, rec) in [
            ("rec", "recordings"),
            ("recordings", "rec"),
            ("vault", "recordings"),
            ("a/notes", "a/notebooks"),
        ] {
            let mut p = profile();
            p.notes = Some(NotesConfig {
                subfolder: vault.to_owned(),
                ..NotesConfig::default()
            });
            p.recordings = Some(RecordingsConfig {
                subfolder: rec.to_owned(),
                ..RecordingsConfig::default()
            });
            assert!(
                p.validate().is_ok(),
                "{vault} and {rec} are siblings, not one inside the other"
            );
        }
    }

    /// A profile row written before the sessions field existed loads as
    /// `sessions: None` and re-serializes without the key — `#[serde(default)]`
    /// IS the migration (AD-107), exactly as it was for notes and recordings.
    #[test]
    fn a_profile_without_a_sessions_key_loads_and_says_nothing() {
        let p = profile();
        let json = serde_json::to_string(&p).expect("encode");
        let back: SyncProfile = serde_json::from_str(&json).expect("decode");
        assert_eq!(back.sessions, None, "no key means no zone, never defaults");

        let flagged = SessionsConfig::default();
        assert_eq!(flagged.subfolder, DEFAULT_SESSIONS_SUBFOLDER);
        assert_eq!(
            flagged.subfolder, "60-sessions",
            "the default is the zone name the live drives already use"
        );
    }

    /// The sessions subfolder rules mirror the notes rules: refused, never
    /// corrected, because a person typed the value and is looking at the field.
    #[test]
    fn a_sessions_subfolder_that_is_empty_absolute_or_escaping_is_refused() {
        for bad in ["", "  ", "/abs", "\\abs", "a/../b", ".."] {
            let mut p = profile();
            p.sessions = Some(SessionsConfig {
                subfolder: bad.to_owned(),
            });
            assert!(
                matches!(p.validate(), Err(SyncError::Config(_))),
                "{bad:?} must be a typed config error"
            );
        }
        let mut p = profile();
        p.sessions = Some(SessionsConfig::default());
        assert!(p.validate().is_ok());
        p.sessions = Some(SessionsConfig {
            subfolder: "zones/60-sessions".to_owned(),
        });
        assert!(p.validate().is_ok(), "a nested zone root is ordinary");
    }

    /// One folder cannot be both a vault and a sessions zone, nor both a
    /// recordings root and a sessions zone: two indexers over one tree, each
    /// with its own idea of what belongs in it. Symmetric, because which one
    /// was configured first says nothing about which one is wrong.
    #[test]
    fn sessions_may_not_overlap_notes_or_recordings_in_either_direction() {
        for (vault, zone) in [
            ("60-sessions", "60-sessions"),
            ("zone", "zone/60-sessions"),
            ("zone/notes", "zone"),
        ] {
            let mut p = profile();
            p.notes = Some(NotesConfig {
                subfolder: vault.to_owned(),
                ..NotesConfig::default()
            });
            p.sessions = Some(SessionsConfig {
                subfolder: zone.to_owned(),
            });
            let err = p
                .validate()
                .expect_err("an overlapping pair must be refused");
            let message = err.to_string();
            assert!(
                message.contains(vault) && message.contains(zone),
                "the message must name both folders, got: {message}"
            );
        }
        let mut p = profile();
        p.recordings = Some(RecordingsConfig::default());
        p.sessions = Some(SessionsConfig {
            subfolder: "recordings/sessions".to_owned(),
        });
        assert!(
            matches!(p.validate(), Err(SyncError::Config(_))),
            "a zone inside the recordings root must be refused"
        );
    }

    /// Component-wise, not `starts_with`: `60-sessions` beside `60-sessions-x`
    /// are siblings. And the live layout — notes in `10-notes`, sessions in
    /// `60-sessions`, recordings under `40-media` — must validate as the
    /// ordinary case it is.
    #[test]
    fn the_live_drive_layout_validates() {
        let mut p = profile();
        p.notes = Some(NotesConfig {
            subfolder: "10-notes".to_owned(),
            ..NotesConfig::default()
        });
        p.recordings = Some(RecordingsConfig {
            subfolder: "40-media/recordings".to_owned(),
            ..RecordingsConfig::default()
        });
        p.sessions = Some(SessionsConfig::default());
        assert!(p.validate().is_ok(), "tgdrive's own layout must be legal");
        assert_eq!(
            p.sessions_root(),
            Some(p.local_path.join("60-sessions")),
            "the root is the join, and validate already fenced it"
        );
    }

    #[test]
    fn both_media_policies_and_every_push_policy_round_trip() {
        for (policy, encoded) in [
            (MediaPolicy::Materialize, r#""materialize""#),
            (MediaPolicy::PointerOnly, r#""pointerOnly""#),
        ] {
            let json = serde_json::to_string(&policy).expect("encode");
            assert_eq!(json, encoded);
            let back: MediaPolicy = serde_json::from_str(&json).expect("decode");
            assert_eq!(back, policy);
        }

        // Internally tagged, so a fourth policy added later leaves these blobs
        // readable exactly as they are.
        for (policy, encoded) in [
            (PushPolicy::Immediate, r#"{"kind":"immediate"}"#),
            (PushPolicy::SessionEnd, r#"{"kind":"sessionEnd"}"#),
            (
                PushPolicy::Window {
                    quiet_from: "01:30".to_owned(),
                    quiet_to: "05:45".to_owned(),
                },
                r#"{"kind":"window","quietFrom":"01:30","quietTo":"05:45"}"#,
            ),
        ] {
            let json = serde_json::to_string(&policy).expect("encode");
            assert_eq!(json, encoded);
            let back: PushPolicy = serde_json::from_str(&json).expect("decode");
            assert_eq!(back, policy);

            let mut p = profile();
            p.recordings = Some(RecordingsConfig {
                push: policy,
                ..RecordingsConfig::default()
            });
            assert!(p.validate().is_ok(), "{encoded} is a valid policy");
        }
    }

    /// A window nobody can parse is a window that never opens, and a recording
    /// that is never pushed is the failure the policy exists to schedule.
    #[test]
    fn a_malformed_quiet_window_is_refused() {
        for (from, to) in [
            ("2200", "06:00"),
            ("22:00", "6:00"),
            ("24:00", "06:00"),
            ("22:60", "06:00"),
            ("2a:00", "06:00"),
            ("+2:00", "06:00"),
            ("", "06:00"),
            // Opens and closes at the same moment: not a wrapping window and
            // not a full day, just ambiguous.
            ("22:00", "22:00"),
        ] {
            let mut p = profile();
            p.recordings = Some(RecordingsConfig {
                push: PushPolicy::Window {
                    quiet_from: from.to_owned(),
                    quiet_to: to.to_owned(),
                },
                ..RecordingsConfig::default()
            });
            assert!(
                matches!(p.validate(), Err(SyncError::Config(_))),
                "{from:?}–{to:?} must be refused"
            );
        }

        // Wrapping midnight is the normal case, and the boundaries are legal.
        let mut p = profile();
        p.recordings = Some(RecordingsConfig {
            push: PushPolicy::Window {
                quiet_from: "23:59".to_owned(),
                quiet_to: "00:00".to_owned(),
            },
            ..RecordingsConfig::default()
        });
        assert!(p.validate().is_ok());
    }
}
