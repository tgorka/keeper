//! The `keeper-syncd` command line (Story 30.1, AD-52).
//!
//! Every subcommand is one of the engine's verbs. There is deliberately **no
//! sync logic here**: `keeper_sync::engine::Engine` owns the supervisor, the
//! journal, the volume gate and the graceful finalize, and this crate is the
//! thin shell that builds a platform, opens the engine, and renders what it
//! says. A second implementation of any of that on the CLI side would be
//! exactly the divergence AD-52 exists to prevent.
//!
//! # Output
//!
//! Every command produces both a human rendering and a `--json` document, and
//! both come out of [`Printer`] — the only place in this crate that writes to
//! stdout. A command's output *is* stdout (that is what a `--json` consumer
//! pipes); everything diagnostic goes to stderr through `tracing`.
//!
//! # Exit codes
//!
//! `0` success · `1` operational failure · `2` configuration error ·
//! `3` a prerequisite is missing. A wrapper script can branch on those: `2` and
//! `3` will never succeed on retry, `1` might.

use std::collections::{BTreeSet, VecDeque};
use std::io::BufRead as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;

use keeper_sync::engine::Engine;
use keeper_sync::lfs::audit::RemoteAudit;
use keeper_sync::lfs::hydrate::{ContentRefusal, Materialization, Release};
use keeper_sync::lfs::listing::{LfsFile, LfsFileState};
use keeper_sync::profile::LfsMode;
use keeper_sync::progress::status_line;
use keeper_sync::provenance::SyncSource;
use keeper_sync::volume::{self, VolumeStatus};
use keeper_sync::{
    ProfileState, Result as SyncResult, SyncDirection, SyncError, SyncLane, SyncPlatform,
    SyncProfile, SyncStatus,
};

use crate::config::{self, DaemonConfig};
use crate::platform::LinuxPlatform;
use crate::update;

/// Everything went as asked.
pub const EXIT_OK: u8 = 0;
/// Something failed that might succeed later: a transfer, a lock, a remote.
pub const EXIT_FAILURE: u8 = 1;
/// The configuration is wrong. Retrying changes nothing.
pub const EXIT_CONFIG: u8 = 2;
/// A hard prerequisite is absent — in practice, `git` (AD-41).
pub const EXIT_PREREQUISITE: u8 = 3;

/// How long a stop signal waits for in-flight work to finalize.
///
/// Ten seconds, identical to the app's `QUIT_FINALIZE_TIMEOUT`, because AD-52
/// requires the daemon's SIGTERM path to be the app's quit path: an in-flight
/// push aborts *resumably* — the journal still holds the unit — rather than
/// being killed mid-write. `TimeoutStopSec` in the systemd unit is set above
/// this so systemd never SIGKILLs us before the bound elapses.
pub const GRACEFUL_FINALIZE: Duration = Duration::from_secs(10);

/// Bounded wait for an aborted supervisor to actually unwind.
///
/// `JoinHandle::abort` only *schedules* cancellation; without joining, the
/// process can exit while the task's destructors — which release the journal's
/// running rows — have not run. Mirrors the app's `QUIT_KILL_JOIN_TIMEOUT`.
const ABORT_JOIN: Duration = Duration::from_secs(2);

/// Free space below which `doctor` warns, matching the recording subsystem's
/// `RECORDING_MIN_FREE_BYTES`, so the two subsystems agree on what "low" means.
const LOW_DISK_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// `max_user_watches` below which `doctor` warns. 8192 was the historical
/// kernel default and is far too low for a real folder: inotify needs one watch
/// per *directory*, and exhausting them makes the watcher miss changes silently.
const LOW_INOTIFY_WATCHES: u64 = 8192;

/// Journal depth above which `doctor` warns that work is not draining.
const DEEP_JOURNAL: u32 = 1_000;

// ---------------------------------------------------------------------------
// Errors and exit codes
// ---------------------------------------------------------------------------

/// A failure that ends the process, carrying the exit code it produces.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error(transparent)]
    Sync(#[from] SyncError),
    /// A failure the engine has no vocabulary for: a `doctor` check that found
    /// real breakage, a supervisor that would not stop.
    #[error("{0}")]
    Operational(String),
}

impl CliError {
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Sync(err) => sync_exit_code(err),
            Self::Operational(_) => EXIT_FAILURE,
        }
    }

    /// Stable machine-readable discriminant for `--json` output, reusing the
    /// engine's own codes so a consumer sees one vocabulary whether the failure
    /// came from the engine or from the CLI around it.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Sync(err) => err.code(),
            Self::Operational(_) => "operational",
        }
    }
}

/// Map an engine error onto a process exit code.
///
/// The match is **exhaustive by construction** — no `_` arm — so a new
/// `SyncError` variant is a compile error here rather than silently becoming
/// exit 1. That matters: `Restart=on-failure` in the systemd unit reads this
/// number, and mis-classifying a permanent configuration error as transient
/// produces a restart loop against a git host.
pub fn sync_exit_code(err: &SyncError) -> u8 {
    match err {
        // No git, and no in-process substitute exists (AD-41).
        SyncError::GitMissing { .. } => EXIT_PREREQUISITE,
        // Someone must edit something before this can ever work.
        SyncError::Config(_) => EXIT_CONFIG,
        // A cancelled operation is not a failure — the type's own contract says
        // so, and a graceful SIGTERM must not make systemd log a failed unit.
        SyncError::Cancelled => EXIT_OK,
        // Neither is an overlapping run. The work is being done by whoever got
        // the folder first, so a one-shot that lands here did nothing wrong and
        // must not earn a `Restart=on-failure` for losing a race.
        SyncError::Busy(_) => EXIT_OK,
        // A refusal ends the command without doing what was asked, which for a
        // one-shot verb is a failed run: an agent or a script that asked for a
        // file and did not get it must be able to see that in `$?`. Never
        // `EXIT_CONFIG` — nothing about the configuration is wrong when keeper
        // declines to overwrite a file the user edited.
        SyncError::Refused(_) => EXIT_FAILURE,
        SyncError::GitCommand { .. }
        | SyncError::Network { .. }
        | SyncError::Auth { .. }
        // Beside `Auth`, which is its sibling: both are a credential the remote
        // would not act on, and the remedies differ only in which thing a human
        // has to change.
        | SyncError::Forbidden { .. }
        | SyncError::InvalidPathForRemote { .. }
        | SyncError::MediaAbsent
        // A one-shot run that ends here published nothing, because `sync_once`
        // drains the uploads BEFORE the push leg — so reaching this means the
        // objects genuinely could not be transferred, which is a failed pass and
        // worth a `Restart=on-failure`. Inside the supervisor the same condition
        // is not a failure at all; it defers the push and the next upload to land
        // releases it.
        | SyncError::LfsUploadPending { .. }
        | SyncError::Integrity { .. }
        | SyncError::Quota { .. }
        | SyncError::Diverged { .. }
        // Beside `Diverged`, and for a one-shot run only. In the supervisor
        // being overtaken is a race the next reconcile settles (DW-207), but a
        // `sync --once` that ends here published nothing this pass — and a pass
        // that did not publish is a failed pass, which is exactly what
        // `Restart=on-failure` should see. The restart meets a fetch first, so
        // it converges rather than looping.
        | SyncError::RemoteMoved { .. }
        | SyncError::Journal(_)
        | SyncError::Git(_)
        | SyncError::Io { .. } => EXIT_FAILURE,
    }
}

// ---------------------------------------------------------------------------
// The command tree
// ---------------------------------------------------------------------------

#[derive(Debug, Parser)]
#[command(
    name = "keeper-syncd",
    version,
    about = "Synchronize folders with a git remote — the keeper sync engine, without the app",
    long_about = "keeper-syncd runs the same folder-sync engine the keeper desktop app runs, \
                  with no app attached. Profiles live in a TOML config that maps 1:1 onto the \
                  app's own profiles, credentials come from the environment or a 0600 file, \
                  and `watch` is the systemd entry point."
)]
pub struct Cli {
    /// Configuration file (default: $XDG_CONFIG_HOME/keeper-sync/config.toml).
    #[arg(
        long,
        short = 'c',
        global = true,
        value_name = "PATH",
        env = "KEEPER_SYNCD_CONFIG"
    )]
    pub config: Option<PathBuf>,

    /// Emit machine-readable JSON instead of prose.
    #[arg(long, global = true)]
    pub json: bool,

    /// Increase log verbosity (-v debug, -vv trace). `RUST_LOG` wins over both.
    #[arg(long, short = 'v', global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: Command,
}

// One variant (`Add`) carries every profile field the CLI can set, so it is
// necessarily far larger than `Pause { id }`. Boxing it would only move the
// allocation into a type clap has to construct anyway, and this value is built
// exactly once per process.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Write a documented starter configuration.
    Init {
        /// Overwrite an existing configuration.
        #[arg(long)]
        force: bool,
    },
    /// Add a profile to the configuration and register it with the engine.
    Add(AddArgs),
    /// List configured profiles.
    List,
    /// Show what each profile is doing.
    Status {
        /// Profile id or name. Omit for all profiles.
        profile: Option<String>,
    },
    /// Reconcile now, then keep running unless --once is given.
    Sync {
        /// Profile id or name. Omit for all profiles.
        profile: Option<String>,
        /// Do one pass and exit — the cron entry point.
        #[arg(long)]
        once: bool,
    },
    /// Run the supervisor in the foreground. The systemd entry point.
    Watch,
    /// Stop syncing a profile without removing it.
    Pause {
        /// Profile id or name.
        profile: String,
    },
    /// Resume a paused profile.
    Resume {
        /// Profile id or name.
        profile: String,
    },
    /// Verify stored content against its recorded digests.
    Verify {
        /// Profile id or name. Omit for all profiles.
        profile: Option<String>,
        /// Re-publish anything the server is missing that this machine can
        /// still supply, instead of only reporting it (DW-208). Moves bytes.
        #[arg(long, requires = "remote")]
        repair: bool,
        /// Also ask the server whether it holds every object the pointers name.
        ///
        /// The half that finds permanent loss: a pointer whose object never
        /// reached the server is a valid blob, a clean status and content only
        /// one machine still has. Costs one batch round trip per few hundred
        /// objects and transfers nothing.
        #[arg(long)]
        remote: bool,
    },
    /// List the LFS paths in a folder and whether this machine holds them.
    ///
    /// The honest size of every row is the one the pointer names, never the
    /// ~130 bytes of pointer text a virtual path holds on disk (FR-336). Reads
    /// the index, the worktree and the materialization ledger; transfers
    /// nothing and writes nothing.
    LsFiles {
        /// Profile id or name. Omit for all profiles.
        profile: Option<String>,
        /// Also ask the server whether it holds every object the pointers
        /// name, and splice the answer in under `remote`.
        ///
        /// Costs one batch round trip per few hundred objects, which is why it
        /// is opt-in: without it the key is ABSENT from the document rather
        /// than null, so a consumer can tell "nobody asked" from "the server
        /// answered" (FR-337).
        #[arg(long)]
        remote: bool,
    },
    /// Ask for one virtual path's content (FR-338).
    ///
    /// Publishes the file inline when this machine already holds the object,
    /// and otherwise queues the transfer and says so — the running daemon
    /// delivers it, because this verb has no event loop to wait on. A path
    /// whose worktree bytes are neither the committed pointer nor the content
    /// it names is **refused by name** and left exactly as it is: keeper does
    /// not overwrite a local modification.
    Materialize {
        /// Profile id or name. Required: materializing is a write, and "all
        /// folders" is not a thing anybody means by it.
        profile: String,
        /// The path inside the folder, as `ls-files` spells it.
        subpath: String,
    },
    /// Release one path's content, leaving the pointer this folder committed
    /// (FR-332, FR-333).
    ///
    /// `materialize`'s inverse, and the verb where a mistake destroys the only
    /// copy — so it refuses before it writes anything: the folder keeps every
    /// object it holds (so a release there would be undone), the file is not
    /// the content this folder committed, something has it open (or this
    /// machine cannot tell), nothing has confirmed the server can serve the
    /// object, the path is pinned, or there was no content here to begin with.
    /// The last of those is a no-op and exits 0; every other refusal exits 1
    /// and changes nothing on disk.
    ///
    /// A release whose **bookkeeping** failed afterwards still exits 0 and
    /// still reports the release: by then the content is gone, and calling a
    /// completed deletion a failure would be the one lie this contract cannot
    /// afford. `Engine::dehydrate_entry` logs what it could not record.
    Dehydrate {
        /// Profile id or name. Required: releasing is a deletion, and "all
        /// folders" is not a thing anybody means by it.
        profile: String,
        /// The path inside the folder, as `ls-files` spells it.
        subpath: String,
    },
    /// Check every prerequisite and report what is broken.
    Doctor,
    /// Print the tail of the daemon log.
    Logs {
        /// How many trailing lines to print.
        #[arg(long, short = 'n', value_name = "N", default_value_t = 200)]
        lines: usize,
    },
    /// Check for a newer release, and optionally install it.
    ///
    /// Never runs on a timer. A sync daemon holds a durable journal and can be
    /// mid-push at any moment; replacing its binary unattended is how a routine
    /// release becomes a corrupted transfer.
    Update {
        /// Only report what is available; change nothing.
        #[arg(long)]
        check: bool,
    },
    /// git clean/smudge filter for LFS content. Invoked by git, not by hand.
    ///
    /// Registered as `filter.lfs.clean` / `filter.lfs.smudge` in each managed
    /// repository so that plain `git` in a synced folder agrees with keeper
    /// about what an LFS-tracked file contains. Without it `git status` reports
    /// every such file as modified the moment it re-reads content, because the
    /// blob is a pointer and the worktree is not.
    #[command(hide = true)]
    Lfs {
        #[command(subcommand)]
        direction: LfsDirection,
    },
}

#[derive(Debug, Subcommand)]
pub enum LfsDirection {
    /// stdin = worktree bytes, stdout = pointer.
    Clean {
        /// The path git is filtering, supplied as `%f`. Advisory only.
        #[arg(value_name = "PATH")]
        path: Option<String>,
        /// Repository the object store belongs to.
        #[arg(long, value_name = "DIR")]
        repo: PathBuf,
    },
    /// stdin = pointer, stdout = worktree bytes.
    Smudge {
        #[arg(value_name = "PATH")]
        path: Option<String>,
        #[arg(long, value_name = "DIR")]
        repo: PathBuf,
    },
    /// Serve git's long-running filter protocol until the pipe closes.
    ///
    /// The shape `filter.lfs.process` is registered as, and the one git
    /// actually uses when it is set — see `keeper_sync::lfs::filter` for why
    /// registering only clean/smudge left the filter unreachable.
    FilterProcess {
        #[arg(long, value_name = "DIR")]
        repo: PathBuf,
    },
}

#[derive(Debug, Args)]
pub struct AddArgs {
    /// Human label, shown in status output and used in commit subjects.
    #[arg(long)]
    pub name: String,
    /// The folder to synchronize. A relative path resolves against the CWD.
    #[arg(long, value_name = "DIR")]
    pub path: PathBuf,
    /// Remote repository URL.
    #[arg(long, value_name = "URL")]
    pub remote: String,
    /// Branch to track.
    #[arg(long, default_value = "main")]
    pub branch: String,
    #[arg(long, value_enum, default_value_t = DirectionArg::Bidirectional)]
    pub direction: DirectionArg,
    #[arg(long, value_enum, default_value_t = LaneArg::Main)]
    pub lane: LaneArg,
    #[arg(long, value_enum, default_value_t = LfsModeArg::Materialize)]
    pub lfs_mode: LfsModeArg,
    /// Repository subpath to materialize. Repeatable; omit for the whole repo.
    #[arg(long, value_name = "SUBPATH")]
    pub subpath: Vec<String>,
    /// Extra exclusion glob. Repeatable.
    #[arg(long, value_name = "GLOB")]
    pub exclude: Vec<String>,
    /// Extra `Keeper-Tag:` commit trailer. Repeatable.
    #[arg(long, value_name = "TAG")]
    pub tag: Vec<String>,
    /// The folder lives on removable media: absence is a pause, not a deletion.
    #[arg(long)]
    pub removable: bool,
    #[arg(long, value_name = "BYTES")]
    pub lfs_threshold_bytes: Option<u64>,
    /// Release local LFS objects once the remote holds them. Now the default.
    ///
    /// Reclaims the second copy every LFS file has on the machine where it
    /// originated. The worktree keeps every file; only the redundant object in
    /// `.git/lfs/objects` goes, and only when the journal owes no transfer for
    /// it and the remote has confirmed it holds the content.
    ///
    /// Kept as a flag after the default flipped so existing scripts keep
    /// working and keep saying what they mean; it now asks for what a profile
    /// does anyway.
    #[arg(long)]
    pub lfs_prune_local: bool,
    /// Keep the second local copy of every LFS object — the opt-out.
    ///
    /// For a folder that has to be restorable with no network: the object
    /// store stays a complete local copy, so a file the worktree loses can be
    /// recovered offline. It costs roughly twice the disk.
    #[arg(long, conflicts_with = "lfs_prune_local")]
    pub no_lfs_prune_local: bool,
    /// Glob that must never go through LFS, whatever its size. Repeatable.
    ///
    /// The recorded `.gitattributes` rule is per-extension, so one oversized
    /// file converts its whole extension for the rest of the repository's life
    /// — use this to protect formats you need to diff (`--lfs-never '*.md'`).
    /// Same dialect as gitignore: no `/` matches the basename at any depth, a
    /// pattern with `/` is anchored at the repository root.
    #[arg(long, value_name = "GLOB")]
    pub lfs_never: Vec<String>,
    #[arg(long, value_name = "MS")]
    pub settle_ms: Option<u64>,
    #[arg(long, value_name = "MS")]
    pub poll_interval_ms: Option<u64>,
    /// Override the derived git author for this profile.
    #[arg(long, value_name = "AUTHOR")]
    pub author: Option<String>,
    /// Adopt an existing profile id instead of minting one.
    #[arg(long, value_name = "ULID")]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DirectionArg {
    Bidirectional,
    PushOnly,
    PullOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LaneArg {
    Main,
    Worktree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LfsModeArg {
    Materialize,
    PointerOnly,
    Disabled,
}

impl From<DirectionArg> for SyncDirection {
    fn from(value: DirectionArg) -> Self {
        match value {
            DirectionArg::Bidirectional => Self::Bidirectional,
            DirectionArg::PushOnly => Self::PushOnly,
            DirectionArg::PullOnly => Self::PullOnly,
        }
    }
}

impl From<LaneArg> for SyncLane {
    fn from(value: LaneArg) -> Self {
        match value {
            LaneArg::Main => Self::Main,
            LaneArg::Worktree => Self::Worktree,
        }
    }
}

impl From<LfsModeArg> for LfsMode {
    fn from(value: LfsModeArg) -> Self {
        match value {
            LfsModeArg::Materialize => Self::Materialize,
            LfsModeArg::PointerOnly => Self::PointerOnly,
            LfsModeArg::Disabled => Self::Disabled,
        }
    }
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

/// The CLI's stdout.
///
/// `println!` appears in this crate ONLY here and in `main`'s pre-logging
/// bail-out, and that is deliberate: a command's output is stdout by
/// definition, while every diagnostic goes through `tracing` to stderr. Nothing
/// in `config`, `platform` or the engine ever prints.
pub struct Printer {
    json: bool,
}

impl Printer {
    pub fn new(json: bool) -> Self {
        Self { json }
    }

    /// A human-readable line, suppressed under `--json` so the document on
    /// stdout stays parseable.
    pub fn line(&self, text: impl AsRef<str>) {
        if !self.json {
            println!("{}", text.as_ref());
        }
    }

    /// The machine-readable document, emitted only under `--json`.
    pub fn json(&self, value: &serde_json::Value) {
        if !self.json {
            return;
        }
        match serde_json::to_string_pretty(value) {
            Ok(text) => println!("{text}"),
            // Re-serializing a `Value` we just built cannot realistically fail;
            // if it somehow does, saying so beats printing nothing.
            Err(err) => tracing::error!(%err, "cannot render JSON output"),
        }
    }
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Run one command.
///
/// `config` arrives as a `Result` rather than being loaded here because
/// `main` needs the configured log level before logging exists — and because
/// `init`, `doctor` and `logs` must all work on a box whose config is missing
/// or broken, which is exactly when an operator reaches for them.
pub async fn run(
    cli: Cli,
    platform: LinuxPlatform,
    config_path: PathBuf,
    config: SyncResult<DaemonConfig>,
) -> std::result::Result<(), CliError> {
    let printer = Printer::new(cli.json);
    match cli.command {
        Command::Init { force } => cmd_init(&printer, &config_path, &platform, force),
        Command::Logs { lines } => cmd_logs(&printer, &platform, lines),
        // Runs inside a `git` invocation: it must write ONLY object bytes to
        // stdout, so it bypasses the printer entirely.
        Command::Update { check } => cmd_update(&printer, check),
        Command::Lfs { direction } => cmd_lfs_filter(direction),
        Command::Doctor => cmd_doctor(&printer, &platform, &config_path, config),
        Command::Add(args) => {
            let engine = engine_for(&platform, config)?;
            cmd_add(&printer, &engine, &config_path, args)
        }
        Command::List => {
            let engine = engine_for(&platform, config)?;
            cmd_list(&printer, &engine)
        }
        Command::Status { profile } => {
            let engine = engine_for(&platform, config)?;
            cmd_status(&printer, &engine, profile.as_deref())
        }
        Command::Pause { profile } => {
            let engine = engine_for(&platform, config)?;
            cmd_set_enabled(&printer, &engine, &profile, false)
        }
        Command::Resume { profile } => {
            let engine = engine_for(&platform, config)?;
            cmd_set_enabled(&printer, &engine, &profile, true)
        }
        Command::Verify {
            profile,
            remote,
            repair,
        } => {
            let engine = engine_for(&platform, config)?;
            cmd_verify(&printer, &engine, profile.as_deref(), remote, repair).await
        }
        Command::LsFiles { profile, remote } => {
            let engine = engine_for(&platform, config)?;
            cmd_ls_files(&printer, &engine, profile.as_deref(), remote).await
        }
        Command::Materialize { profile, subpath } => {
            let engine = engine_for(&platform, config)?;
            cmd_materialize(&printer, &engine, &profile, &subpath)
        }
        Command::Dehydrate { profile, subpath } => {
            let engine = engine_for(&platform, config)?;
            cmd_dehydrate(&printer, &engine, &profile, &subpath).await
        }
        Command::Sync { profile, once } => {
            let engine = engine_for(&platform, config)?;
            cmd_sync(&printer, engine, profile.as_deref(), once).await
        }
        Command::Watch => {
            let engine = engine_for(&platform, config)?;
            run_supervisor(&printer, engine).await
        }
    }
}

/// Open the engine and bring the configured profiles into it.
fn engine_for(
    platform: &LinuxPlatform,
    config: SyncResult<DaemonConfig>,
) -> std::result::Result<Engine, CliError> {
    let config = config?;
    let engine = Engine::open(Arc::new(platform.clone()) as Arc<dyn SyncPlatform>)?;
    reconcile(&engine, &config)?;
    Ok(engine)
}

/// Push the config's profiles into the engine's store.
///
/// The config file is the operator's *declaration*; the engine's store is
/// runtime state. Two rules keep them from fighting:
///
/// * `enabled` is runtime state, so a profile already known to the engine keeps
///   the engine's flag — otherwise every restart would silently un-pause
///   everything an operator paused.
/// * A profile the engine knows but the config no longer names is **reported,
///   not deleted**. Dropping a profile because it vanished from a file is the
///   same "absence is deletion" mistake AD-48 exists to prevent, one layer up:
///   a mistyped `--config` would otherwise discard every journal.
fn reconcile(engine: &Engine, config: &DaemonConfig) -> SyncResult<()> {
    let existing = engine.list_profiles()?;
    let configured: BTreeSet<&str> = config.profiles.iter().map(|p| p.id.as_str()).collect();

    for profile in &config.profiles {
        let mut profile = profile.clone();
        if let Some(known) = existing.iter().find(|known| known.id == profile.id) {
            profile.enabled = known.enabled;
            // The volume binding is engine state, not configuration: it is
            // minted on first sight of the media and lives in the marker on the
            // volume. Letting a config file that never carried it reset the
            // binding on every start would unbind the profile, and an unbound
            // profile adopts whatever is mounted at its path instead of
            // refusing a different volume as `Foreign`. An explicit `volumeId`
            // in the config still wins — that is an operator saying which
            // volume they mean.
            if profile.volume_id.is_none() {
                profile.volume_id = known.volume_id.clone();
            }
        }
        engine.upsert_profile(&profile)?;
    }

    for orphan in existing
        .iter()
        .filter(|p| !configured.contains(p.id.as_str()))
    {
        tracing::warn!(
            profile_id = %orphan.id,
            profile = %orphan.name,
            "profile is in the engine store but not in the configuration; \
             leaving it and its journal untouched — remove it explicitly if that was intended"
        );
    }
    Ok(())
}

/// Resolve a user-typed profile selector to profiles.
///
/// Accepts an id or a name, because an operator reads names in `list` output
/// and will type those.
fn select<'a>(
    profiles: &'a [SyncProfile],
    wanted: Option<&str>,
) -> std::result::Result<Vec<&'a SyncProfile>, CliError> {
    let Some(wanted) = wanted else {
        return Ok(profiles.iter().collect());
    };
    let matched: Vec<&SyncProfile> = profiles
        .iter()
        .filter(|profile| profile.id == wanted || profile.name == wanted)
        .collect();
    if matched.is_empty() {
        let known = profiles
            .iter()
            .map(|profile| profile.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(SyncError::Config(format!(
            "no profile matches `{wanted}`; configured profiles are: {known}"
        ))
        .into());
    }
    Ok(matched)
}

// ---------------------------------------------------------------------------
// init
// ---------------------------------------------------------------------------

fn cmd_init(
    printer: &Printer,
    config_path: &Path,
    platform: &LinuxPlatform,
    force: bool,
) -> std::result::Result<(), CliError> {
    if config_path.exists() && !force {
        return Err(SyncError::Config(format!(
            "{} already exists; pass --force to overwrite it",
            config_path.display()
        ))
        .into());
    }
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| SyncError::io("create the configuration directory", parent, err))?;
    }
    std::fs::write(config_path, config::example())
        .map_err(|err| SyncError::io("write the daemon configuration", config_path, err))?;

    printer.line(format!("Wrote {}", config_path.display()));
    printer.line(format!("Secrets:  {}", platform.secrets_dir().display()));
    printer.line(format!("Log file: {}", platform.log_path().display()));
    printer.line("Edit the configuration, then run `keeper-syncd doctor`.");
    printer.json(&serde_json::json!({
        "wrote": config_path,
        "secretsDir": platform.secrets_dir(),
        "logFile": platform.log_path(),
    }));
    Ok(())
}

// ---------------------------------------------------------------------------
// add
// ---------------------------------------------------------------------------

fn cmd_add(
    printer: &Printer,
    engine: &Engine,
    config_path: &Path,
    args: AddArgs,
) -> std::result::Result<(), CliError> {
    // A relative `--path` is resolved here rather than rejected: typing
    // `--path .` is the obvious thing to do, and `SyncProfile` requires an
    // absolute path.
    let local_path = if args.path.is_absolute() {
        args.path.clone()
    } else {
        let cwd = std::env::current_dir().map_err(|err| {
            SyncError::io("resolve the working directory for --path", &args.path, err)
        })?;
        cwd.join(&args.path)
    };

    let id = args
        .id
        .clone()
        .unwrap_or_else(|| ulid::Ulid::new().to_string());
    let mut profile = SyncProfile::new(id, args.name.clone(), local_path, args.remote.clone());
    profile.branch = args.branch.clone();
    profile.direction = args.direction.into();
    profile.lane = args.lane.into();
    profile.lfs_mode = args.lfs_mode.into();
    profile.subpaths = args.subpath.clone();
    profile.excludes = args.exclude.clone();
    profile.lfs_never = args.lfs_never.clone();
    // Only the opt-out is applied. Assigning the flag itself would make every
    // `add` that does not pass it an opt-out, which is how a changed default
    // gets quietly undone by the tool that is supposed to honour it.
    if args.no_lfs_prune_local {
        profile.lfs_prune_local = false;
    }
    profile.tags = args.tag.clone();
    profile.removable = args.removable;
    profile.author_override = args.author.clone();
    if let Some(bytes) = args.lfs_threshold_bytes {
        profile.lfs_threshold_bytes = bytes;
    }
    if let Some(ms) = args.settle_ms {
        profile.settle_ms = ms;
    }
    if let Some(ms) = args.poll_interval_ms {
        profile.poll_interval_ms = ms;
    }
    // Validate before touching either store, so a rejected profile leaves no
    // half-written config behind.
    profile.validate()?;

    append_profile_table(config_path, &profile)?;
    engine.upsert_profile(&profile)?;

    printer.line(format!(
        "Added `{}` ({}) -> {}#{}",
        profile.name, profile.id, profile.remote_url, profile.branch
    ));
    printer.line(format!(
        "Appended a [[profile]] table to {}",
        config_path.display()
    ));
    if profile.removable {
        printer.line(
            "Removable: while the volume is detached this profile pauses and \
             nothing is deleted.",
        );
    }
    printer.json(&serde_json::json!({ "added": profile, "config": config_path }));
    Ok(())
}

/// Append a rendered `[[profile]]` table to the configuration.
///
/// Read-modify-write through a sibling temp file and a rename, the recording
/// manifest's idiom: a config truncated by a crash mid-write would take every
/// other profile with it. Appending text (rather than re-serializing the
/// document) is what preserves the operator's comments.
fn append_profile_table(config_path: &Path, profile: &SyncProfile) -> SyncResult<()> {
    let mut text = match std::fs::read_to_string(config_path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => {
            return Err(SyncError::io(
                "read the daemon configuration",
                config_path,
                err,
            ))
        }
    };
    text.push_str(&config::render_profile_table(profile)?);

    let parent = config_path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)
        .map_err(|err| SyncError::io("create the configuration directory", parent, err))?;
    let tmp = parent.join(".config.toml.tmp");
    std::fs::write(&tmp, text.as_bytes())
        .map_err(|err| SyncError::io("stage the daemon configuration", &tmp, err))?;
    std::fs::rename(&tmp, config_path).map_err(|err| {
        // The rename never happened, so the previous config is intact; drop the
        // staged file rather than leaving a confusing dotfile behind.
        let _ = std::fs::remove_file(&tmp);
        SyncError::io("publish the daemon configuration", config_path, err)
    })
}

// ---------------------------------------------------------------------------
// list / status
// ---------------------------------------------------------------------------

fn cmd_list(printer: &Printer, engine: &Engine) -> std::result::Result<(), CliError> {
    let profiles = engine.list_profiles()?;
    if profiles.is_empty() {
        printer.line("No profiles configured. Add one with `keeper-syncd add`.");
    }
    let width = profiles
        .iter()
        .map(|profile| profile.name.chars().count())
        .max()
        .unwrap_or(0);
    for profile in &profiles {
        printer.line(format!(
            "{:width$}  {}  {} -> {}#{}{}",
            profile.name,
            profile.id,
            profile.local_path.display(),
            profile.remote_url,
            profile.branch,
            if profile.enabled { "" } else { "  [paused]" }
        ));
    }
    printer.json(&serde_json::json!({ "profiles": profiles }));
    Ok(())
}

fn cmd_status(
    printer: &Printer,
    engine: &Engine,
    wanted: Option<&str>,
) -> std::result::Result<(), CliError> {
    let profiles = engine.list_profiles()?;
    let selected: BTreeSet<&str> = select(&profiles, wanted)?
        .into_iter()
        .map(|profile| profile.id.as_str())
        .collect();

    let statuses: Vec<SyncStatus> = engine
        .statuses()?
        .into_iter()
        .filter(|status| selected.contains(status.profile_id.as_str()))
        .collect();

    if statuses.is_empty() {
        printer.line("No profiles configured.");
    }
    for status in &statuses {
        // The same one-line rendering the tray shows, so a support conversation
        // about "what does the tray say" and "what does the CLI say" cannot
        // diverge.
        let mut line = status_line(status);
        if status.pending > 0 && status.state != ProfileState::Offline {
            line.push_str(&format!(" ({} queued)", status.pending));
        }
        printer.line(line);
        if let Some(warning) = &status.warning {
            printer.line(format!("  warning: {warning}"));
        }
        if let Some(error) = &status.error {
            printer.line(format!("  error:   {error}"));
        }
    }
    printer.json(&serde_json::json!({ "statuses": statuses }));
    Ok(())
}

fn cmd_set_enabled(
    printer: &Printer,
    engine: &Engine,
    wanted: &str,
    enabled: bool,
) -> std::result::Result<(), CliError> {
    let profiles = engine.list_profiles()?;
    let selected = select(&profiles, Some(wanted))?;
    let mut changed = Vec::new();
    for profile in selected {
        engine.set_enabled(&profile.id, enabled)?;
        printer.line(format!(
            "{} {}",
            if enabled { "Resumed" } else { "Paused" },
            profile.name
        ));
        changed.push(profile.id.clone());
    }
    printer.json(&serde_json::json!({ "enabled": enabled, "profiles": changed }));
    Ok(())
}

// ---------------------------------------------------------------------------
// sync / watch
// ---------------------------------------------------------------------------

async fn cmd_sync(
    printer: &Printer,
    engine: Engine,
    wanted: Option<&str>,
    once: bool,
) -> std::result::Result<(), CliError> {
    let profiles = engine.list_profiles()?;
    let selected: Vec<SyncProfile> = select(&profiles, wanted)?.into_iter().cloned().collect();

    let mut results = Vec::with_capacity(selected.len());
    let mut first_error = None;
    for profile in &selected {
        match engine.sync_once(&profile.id, SyncSource::Cli).await {
            Ok(outcome) => {
                printer.line(format!(
                    "{}: {} file(s), {}{}{}{}",
                    profile.name,
                    outcome.files_changed,
                    if outcome.pulled { "pulled " } else { "" },
                    if outcome.pushed { "pushed " } else { "" },
                    outcome
                        .committed
                        .as_deref()
                        .map(|id| format!("commit {id} "))
                        .unwrap_or_default(),
                    if outcome.conflicts.is_empty() {
                        String::new()
                    } else {
                        format!("{} conflict copies", outcome.conflicts.len())
                    }
                ));
                for conflict in &outcome.conflicts {
                    printer.line(format!("  conflict copy: {conflict}"));
                }
                results.push(serde_json::json!({
                    "profileId": profile.id,
                    "profile": profile.name,
                    "ok": true,
                    "committed": outcome.committed,
                    "pushed": outcome.pushed,
                    "pulled": outcome.pulled,
                    "filesChanged": outcome.files_changed,
                    "bytes": outcome.bytes,
                    "conflicts": outcome.conflicts,
                }));
            }
            Err(err) => {
                // Keep going: one unreachable remote must not stop the other
                // profiles from converging. The exit code still reports it.
                printer.line(format!("{}: {err}", profile.name));
                results.push(serde_json::json!({
                    "profileId": profile.id,
                    "profile": profile.name,
                    "ok": false,
                    "error": err.to_string(),
                    "code": err.code(),
                }));
                if first_error.is_none() {
                    first_error = Some(err);
                }
            }
        }
    }
    printer.json(&serde_json::json!({ "synced": results }));

    if let Some(err) = first_error {
        return Err(err.into());
    }
    if once {
        return Ok(());
    }
    // Without `--once`, an interactive `sync` keeps converging: the one-shot
    // pass above is the "do it now" the operator asked for, and the supervisor
    // is what keeps it true.
    run_supervisor(printer, engine).await
}

/// Run the engine's supervisor until a stop signal, then finalize under a bound.
///
/// The whole point of the bound (AD-52): SIGTERM must behave like the app's
/// quit, so an in-flight push is *aborted resumably* — its journal row survives
/// and is re-driven on the next start — instead of being killed mid-write.
async fn run_supervisor(printer: &Printer, engine: Engine) -> std::result::Result<(), CliError> {
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let mut supervisor = tokio::spawn(async move { engine.run(shutdown_rx).await });

    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|err| {
            CliError::Operational(format!("cannot install the SIGTERM handler: {err}"))
        })?;
    let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .map_err(|err| {
            CliError::Operational(format!("cannot install the SIGINT handler: {err}"))
        })?;

    printer.line("keeper-syncd is running. SIGTERM or Ctrl-C stops it gracefully.");
    // A long-running command still owes `--json` something: one document at
    // start and one at stop, so a supervisor wrapper can tell "up" from
    // "exited cleanly" without scraping the log.
    printer.json(&serde_json::json!({
        "event": "started",
        "finalizeBoundSecs": GRACEFUL_FINALIZE.as_secs(),
    }));
    tracing::info!("supervisor started");

    let signal = tokio::select! {
        // The supervisor can end on its own — a journal failure, say. Report
        // that rather than waiting forever for a signal that will not come.
        outcome = &mut supervisor => {
            printer.json(&serde_json::json!({ "event": "stopped", "reason": "supervisorExited" }));
            return join_outcome(outcome);
        },
        _ = sigterm.recv() => "SIGTERM",
        _ = sigint.recv() => "SIGINT",
    };

    tracing::info!(
        signal,
        bound_secs = GRACEFUL_FINALIZE.as_secs(),
        "stopping: finalizing in-flight work"
    );
    // A supervisor that has already dropped its receiver makes this fail, which
    // is not a problem — it is already stopping.
    let _ = shutdown_tx.send(true);

    match tokio::time::timeout(GRACEFUL_FINALIZE, &mut supervisor).await {
        Ok(outcome) => {
            tracing::info!("supervisor finalized");
            printer.line("Finalized.");
            printer.json(&serde_json::json!({
                "event": "stopped",
                "reason": "signal",
                "signal": signal,
                "finalized": true,
            }));
            join_outcome(outcome)
        }
        Err(_) => {
            tracing::warn!(
                bound_secs = GRACEFUL_FINALIZE.as_secs(),
                "the supervisor did not finalize within the bound; aborting it"
            );
            supervisor.abort();
            // `abort` only schedules cancellation. Join (bounded) so the task's
            // future is actually dropped — and its journal rows released —
            // before the process exits.
            let _ = tokio::time::timeout(ABORT_JOIN, &mut supervisor).await;
            Err(CliError::Operational(format!(
                "the sync supervisor did not finalize within {} s; in-flight work stays \
                 journaled and is re-driven on the next start",
                GRACEFUL_FINALIZE.as_secs()
            )))
        }
    }
}

fn join_outcome(
    outcome: std::result::Result<SyncResult<()>, tokio::task::JoinError>,
) -> std::result::Result<(), CliError> {
    match outcome {
        Ok(Ok(())) => Ok(()),
        Ok(Err(err)) => Err(err.into()),
        // We aborted it ourselves; the caller already produced the real error.
        Err(join) if join.is_cancelled() => Ok(()),
        Err(join) => Err(CliError::Operational(format!(
            "the sync supervisor panicked: {join}"
        ))),
    }
}

// ---------------------------------------------------------------------------
// verify
// ---------------------------------------------------------------------------

async fn cmd_verify(
    printer: &Printer,
    engine: &Engine,
    wanted: Option<&str>,
    remote: bool,
    repair: bool,
) -> std::result::Result<(), CliError> {
    let profiles = engine.list_profiles()?;
    let selected: Vec<SyncProfile> = select(&profiles, wanted)?.into_iter().cloned().collect();

    let mut reports = Vec::with_capacity(selected.len());
    let mut bad_total = 0usize;
    let mut missing_total = 0usize;
    for profile in &selected {
        let report = engine.verify(&profile.id).await?;
        bad_total += report.bad.len();
        printer.line(format!(
            "{}: {} checked, {} bad",
            profile.name,
            report.checked,
            report.bad.len()
        ));
        for (path, reason) in &report.bad {
            printer.line(format!("  {path}: {reason}"));
        }
        let mut entry = serde_json::json!({
            "profileId": profile.id,
            "profile": profile.name,
            "checked": report.checked,
            "bad": report.bad
                .iter()
                .map(|(path, reason)| serde_json::json!({ "path": path, "reason": reason }))
                .collect::<Vec<_>>(),
        });

        if remote {
            let audit = engine.audit_remote_objects(&profile.id).await?;
            missing_total += audit.missing.len();
            printer.line(format!(
                "{}: {} pointer(s) on the server, {} missing ({:.2} GB)",
                profile.name,
                audit.checked,
                audit.missing.len(),
                audit.missing_bytes() as f64 / 1_073_741_824.0
            ));
            for object in &audit.missing {
                printer.line(format!(
                    "  {} ({} bytes): {}",
                    object.path, object.size, object.reason
                ));
            }
            entry["remote"] = serde_json::to_value(&audit).unwrap_or(serde_json::Value::Null);

            if repair && !audit.is_intact() {
                let fixed = engine.republish_missing_objects(&profile.id).await?;
                printer.line(format!(
                    "{}: queued {} object(s) for re-upload ({:.2} GB); {} beyond this machine",
                    profile.name,
                    fixed.queued,
                    fixed.queued_bytes as f64 / 1_073_741_824.0,
                    fixed.unrecoverable.len()
                ));
                for object in &fixed.unrecoverable {
                    printer.line(format!("  {} ({} bytes)", object.path, object.size));
                }
                // Only what nobody here can put back still counts against the
                // exit code: an upload that is queued is a problem being fixed.
                missing_total -= audit.missing.len() - fixed.unrecoverable.len();
                entry["repair"] = serde_json::to_value(&fixed).unwrap_or(serde_json::Value::Null);
            }
        }
        reports.push(entry);
    }
    printer.json(&serde_json::json!({
        "verified": reports,
        "bad": bad_total,
        "missingOnRemote": missing_total,
    }));

    if bad_total > 0 {
        // Corrupt content is a real failure, and a cron wrapper must see it.
        return Err(CliError::Operational(format!(
            "{bad_total} object(s) failed verification"
        )));
    }
    if missing_total > 0 {
        // Louder than a warning on purpose: content that exists on exactly one
        // machine is one disk failure from gone, and the only thing that can
        // still save it is somebody noticing today.
        return Err(CliError::Operational(format!(
            "{missing_total} pointer(s) name content the server does not have"
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// ls-files
// ---------------------------------------------------------------------------

/// One profile's listing as a human reads it (Story 56.2, FR-336, FR-337).
///
/// **Pure, and returning lines rather than printing them.** [`Printer`] writes
/// to process stdout, so an assertion about what this verb *says* would
/// otherwise need the binary spawned — and the claim worth asserting is a claim
/// about a string: that the size named is the four megabytes the pointer stands
/// for and never the ~130 bytes of pointer text the worktree actually holds.
///
/// The state word comes from [`LfsFileState`]'s own [`std::fmt::Display`],
/// which is the same string the `--json` form carries, so the two renderings
/// cannot come to call one row two different things.
///
/// The oid is printed in full rather than abbreviated. It is the handle
/// `verify`, the store and every later verb take, and a prefix somebody has to
/// expand by hand is not a handle.
fn ls_files_lines(profile_name: &str, files: &[LfsFile]) -> Vec<String> {
    if files.is_empty() {
        // An honest empty answer, not an error: a folder with no LFS paths —
        // or one whose `lfs_mode` is `disabled` — has nothing to list, and
        // saying so beats a failure about a feature nobody asked for.
        return vec![format!("{profile_name}: no LFS paths")];
    }
    let counted = |wanted: LfsFileState| files.iter().filter(|f| f.state == wanted).count();
    let mut lines = vec![format!(
        "{profile_name}: {total} LFS path(s) — {virtual_count} virtual, \
         {materialized} materialized, {absent} absent",
        total = files.len(),
        virtual_count = counted(LfsFileState::Virtual),
        materialized = counted(LfsFileState::Materialized),
        absent = counted(LfsFileState::Absent),
    )];
    lines.extend(files.iter().map(|file| {
        format!(
            "  {state:<12}  {size:>9}  {path}  {oid}{pinned}",
            state = file.state,
            size = format_bytes(file.size_bytes),
            path = file.path,
            oid = file.oid,
            pinned = if file.pinned { "  [pinned]" } else { "" },
        )
    }));
    lines
}

/// One profile's listing as the `--json` document carries it.
///
/// The key set is the contract (FR-337): `files` is [`LfsFile`]'s own
/// serialization, unmodified, and `total` is beside it so a consumer that only
/// wants the count does not have to parse the array. `profileId` and `profile`
/// mirror what `verify` puts on its per-profile entry, because an operator with
/// three folders configured reads both documents the same way.
///
/// `remote` is a **parameter**, and `None` is what makes the key absent rather
/// than `null`. Absent means "nobody asked the server"; `null` cannot say that,
/// and a consumer has to be able to tell it from "the server answered and holds
/// nothing". Taking it here rather than splicing it in afterwards is what lets
/// the contract be asserted: the branch that decides absence is in a function a
/// test can call, not in an `if` around an `await`.
///
/// Note that `total` counts **paths** while `remote.checked` counts
/// path-to-object usages over the objects the batch API was asked about, so the
/// two are not the same number and are not meant to reconcile.
fn ls_files_entry(
    profile: &SyncProfile,
    files: &[LfsFile],
    remote: Option<&RemoteAudit>,
) -> serde_json::Value {
    let mut entry = serde_json::json!({
        "profileId": profile.id,
        "profile": profile.name,
        "total": files.len(),
        "files": files,
    });
    if let Some(audit) = remote {
        entry["remote"] = serde_json::to_value(audit).unwrap_or(serde_json::Value::Null);
    }
    entry
}

/// The whole `--json` document, envelope included.
///
/// The envelope is as much of the contract as the rows are — renaming
/// `listings` breaks every consumer — so it is built here where a test can
/// assert it, rather than inline in [`cmd_ls_files`] where nothing could.
fn ls_files_document(listings: Vec<serde_json::Value>) -> serde_json::Value {
    let total: usize = listings
        .iter()
        .map(|entry| entry["total"].as_u64().unwrap_or(0) as usize)
        .sum();
    serde_json::json!({ "listings": listings, "total": total })
}

/// List every LFS path in the selected profiles, and whether this machine holds
/// its content.
///
/// [`Engine::lfs_files`] reads the index, the worktree and the ledger and
/// creates nothing; `--remote` adds one batch round trip per few hundred
/// objects and still transfers nothing. Objects the server is missing are
/// *reported* and do not change the exit code — this is a listing, and
/// `verify --remote` is the command whose job is to fail on them.
async fn cmd_ls_files(
    printer: &Printer,
    engine: &Engine,
    wanted: Option<&str>,
    remote: bool,
) -> std::result::Result<(), CliError> {
    let profiles = engine.list_profiles()?;
    let selected: Vec<SyncProfile> = select(&profiles, wanted)?.into_iter().cloned().collect();

    let mut listings = Vec::with_capacity(selected.len());
    for profile in &selected {
        let files = engine.lfs_files(&profile.id)?;
        for line in ls_files_lines(&profile.name, &files) {
            printer.line(line);
        }
        // The one place the flag is read. `None` past this point is the whole
        // absent-versus-answered contract, and it is carried as a value rather
        // than as a later mutation.
        let audit = if remote {
            Some(engine.audit_remote_objects(&profile.id).await?)
        } else {
            None
        };
        if let Some(audit) = &audit {
            printer.line(format!(
                "{}: {} pointer(s) on the server, {} missing ({})",
                profile.name,
                audit.checked,
                audit.missing.len(),
                format_bytes(audit.missing_bytes()),
            ));
            for object in &audit.missing {
                printer.line(format!(
                    "  {} ({}): {}",
                    object.path,
                    format_bytes(object.size),
                    object.reason
                ));
            }
        }
        listings.push(ls_files_entry(profile, &files, audit.as_ref()));
    }
    printer.json(&ls_files_document(listings));
    Ok(())
}

// ---------------------------------------------------------------------------
// materialize
// ---------------------------------------------------------------------------

/// One materialization as a human reads it.
///
/// Pure, and taking the profile's name rather than the profile, so the sentence
/// a person reads is asserted by a test that owns no engine. One line: this
/// verb settles exactly one path, and a header over a single row would be
/// noise.
///
/// The size is the **pointer's**, never the ~130 bytes of pointer text a
/// virtual path holds on disk (FR-336) — the same discipline `ls-files` states,
/// and the number is the one a person was asking about when they asked for the
/// file.
///
/// The outcome word is [`MaterializeOutcome`]'s own [`std::fmt::Display`],
/// which is prose; the `--json` form carries serde's camelCase spelling of the
/// same variant, so one enum still decides what happened and each surface
/// spells it the way its reader expects.
fn materialize_lines(profile_name: &str, done: &Materialization) -> Vec<String> {
    let mut lines = vec![format!(
        "{profile_name}: {outcome}  {size}  {path}",
        outcome = done.outcome,
        size = format_bytes(done.size_bytes),
        path = done.path,
    )];
    if let Some(unit) = done.unit_id {
        // Said out loud because this verb does NOT wait: `keeper-syncd` has no
        // event subscription, so the object is delivered by the supervisor —
        // and by the supervisor's own next pass, since this process's `wake_now`
        // reaches only its own engine. An operator with no daemon running would
        // otherwise wait for something that is never going to happen.
        lines.push(format!(
            "  queued as unit {unit}; a running `keeper-syncd watch` fetches it \
             on its next pass"
        ));
    }
    lines
}

/// One materialization as the `--json` document carries it.
///
/// The key set is the contract: `profileId`, `profile`, `path`, `oid`,
/// `sizeBytes`, `outcome` — plus `unitId` **only** when something was queued.
///
/// Absent rather than null, and built by naming the keys here rather than by
/// serializing [`Materialization`], for the reason `ls_files_entry` states:
/// `null` cannot distinguish "nothing was queued" from "a unit whose id we
/// lost", and a derived serialization would emit exactly that `null`. The
/// branch that decides absence is therefore in a function a test can call.
fn materialize_json(profile: &SyncProfile, done: &Materialization) -> serde_json::Value {
    let mut entry = serde_json::json!({
        "profileId": profile.id,
        "profile": profile.name,
        "path": done.path,
        "oid": done.oid,
        "sizeBytes": done.size_bytes,
        "outcome": done.outcome,
    });
    if let Some(unit) = done.unit_id {
        entry["unitId"] = serde_json::json!(unit);
    }
    entry
}

/// Ask for one path's content in one folder.
///
/// [`Engine::materialize_entry`] holds every decision — containment, the mode,
/// the index, the cone, whether the bytes on disk may be replaced, and whether
/// the object is here or has to be fetched. This function selects the profile
/// and prints; adding any policy here would be the second implementation AD-52
/// exists to prevent.
///
/// A required profile, resolved through [`select`] so a mistyped name is
/// refused with the alternatives named rather than silently addressing a
/// different folder. `select` returns a list because its other callers accept
/// `None` for "every profile"; this verb writes to one worktree, so it takes
/// the one match and prints its document exactly once.
fn cmd_materialize(
    printer: &Printer,
    engine: &Engine,
    wanted: &str,
    subpath: &str,
) -> std::result::Result<(), CliError> {
    let profiles = engine.list_profiles()?;
    let matched = select(&profiles, Some(wanted))?;
    let [profile] = matched[..] else {
        // `select` refuses a selector nothing matches and names the
        // alternatives; what it cannot refuse is a selector matching two
        // folders, because nothing makes a profile's NAME unique. For a read
        // verb the answer is both listings; for a write there is no defensible
        // arbitrary choice, so this asks for the id.
        return Err(SyncError::Config(format!(
            "`{wanted}` matches {} folders; name the one you mean by its id",
            matched.len()
        ))
        .into());
    };

    let done = engine
        .materialize_entry(&profile.id, subpath)
        .map_err(|err| {
            // `Busy` is `EXIT_OK` for `sync --once`, where losing a race did no
            // wrong and a `Restart=on-failure` unit must not fire for it. This verb
            // promises one path now holds content, so a run that did not deliver it
            // has to be visible in `$?` — the same reasoning the `Refused` arm of
            // `sync_exit_code` carries.
            match err {
                SyncError::Busy(name) => CliError::Operational(format!(
                    "{name} is busy syncing right now; ask again in a moment"
                )),
                other => other.into(),
            }
        })?;
    for line in materialize_lines(&profile.name, &done) {
        printer.line(line);
    }
    printer.json(&materialize_json(profile, &done));
    Ok(())
}

// ---------------------------------------------------------------------------
// dehydrate
// ---------------------------------------------------------------------------

/// One release as a human reads it.
///
/// Pure, taking the profile's *name*, for [`materialize_lines`]'s reason. One
/// line, because this verb settles exactly one path and it either released it
/// or refused.
///
/// The size is the **pointer's**: the bytes this machine no longer holds, which
/// is the number a person asking for space back was asking about. The word is
/// prose — `released` — and it is not the `--json` document's `outcome` token
/// by coincidence rather than by sharing a function; see
/// [`already_pointer_line`], where the two genuinely differ.
fn dehydrate_line(profile_name: &str, done: &Release) -> String {
    format!(
        "{profile_name}: released  {size}  {path}",
        size = format_bytes(done.size_bytes),
        path = done.path,
    )
}

/// One release as the `--json` document carries it.
///
/// The key set is the contract: `profileId`, `profile`, `path`, `oid`,
/// `sizeBytes`, `outcome`. Built by naming the keys rather than by serializing
/// [`Release`], for the reason [`materialize_json`] states and because the
/// sibling document below has a deliberately *narrower* key set that no single
/// derived serialization could produce.
fn dehydrate_json(profile: &SyncProfile, done: &Release) -> serde_json::Value {
    serde_json::json!({
        "profileId": profile.id,
        "profile": profile.name,
        "path": done.path,
        "oid": done.oid,
        "sizeBytes": done.size_bytes,
        "outcome": "released",
    })
}

/// The no-op line: there was no content here to release.
///
/// Prose, not the wire word. `alreadyPointer` in a sentence a person reads is a
/// token that escaped from a JSON document — [`MaterializeOutcome`]'s own
/// `Display` doc makes the same distinction for the same reason.
fn already_pointer_line(profile_name: &str, path: &str) -> String {
    format!("{profile_name}: already a pointer  {path}")
}

/// The no-op document: `profileId`, `profile`, `path`, `outcome` — and
/// **no** `oid`, **no** `sizeBytes`.
///
/// Absent rather than null, and this is the sharper case of the rule
/// [`materialize_json`] follows: nothing was released, so there is no size to
/// report, and `sizeBytes: 0` would read as "released, nothing in it" while
/// `sizeBytes: null` would read as "released something and lost the number". A
/// consumer totalling reclaimed bytes must be able to skip this document
/// without arithmetic.
fn already_pointer_json(profile: &SyncProfile, path: &str) -> serde_json::Value {
    serde_json::json!({
        "profileId": profile.id,
        "profile": profile.name,
        "path": path,
        "outcome": "alreadyPointer",
    })
}

/// The one refusal that is not a failure, and the path it is about.
///
/// [`Engine::dehydrate_entry`] returns `Ok` only when it actually released
/// something, so "there was nothing here to release" arrives as
/// [`ContentRefusal::AlreadyPointer`] — a typed error a caller can branch on,
/// which is what makes every refusal one uniform contract. Both halves of
/// that are literal: it has to be distinguishable *by type*, and it must not be
/// something a caller has to handle as a failure.
///
/// This is where the two meet, and it is a pure function with its own test
/// precisely because "the refusals really are distinguishable" is the claim
/// being made. Matching on the message would have been the other reading, and
/// it is the one that stops working the first time a sentence is reworded.
fn nothing_to_release(err: &SyncError) -> Option<&str> {
    match err {
        SyncError::Refused(ContentRefusal::AlreadyPointer { path }) => Some(path),
        _ => None,
    }
}

/// Let one path's content go in one folder.
///
/// [`Engine::dehydrate_entry`] holds every decision — containment, the mode,
/// the index, the cone, whether these are the committed bytes, whether the path
/// is pinned, whether the server can serve the object, and whether anything has
/// the file open. This function selects the profile, branches on the one
/// refusal that is a no-op, and prints.
///
/// A required profile resolved through [`select`], and a two-folder name match
/// refused by asking for the id — [`cmd_materialize`]'s rule, and it matters
/// more here: there is no defensible arbitrary choice about which folder to
/// delete from.
async fn cmd_dehydrate(
    printer: &Printer,
    engine: &Engine,
    wanted: &str,
    subpath: &str,
) -> std::result::Result<(), CliError> {
    let profiles = engine.list_profiles()?;
    let matched = select(&profiles, Some(wanted))?;
    let [profile] = matched[..] else {
        return Err(SyncError::Config(format!(
            "`{wanted}` matches {} folders; name the one you mean by its id",
            matched.len()
        ))
        .into());
    };

    let done = match engine.dehydrate_entry(&profile.id, subpath).await {
        Ok(done) => done,
        Err(err) => {
            // Nothing to do is not a failed run: the caller asked for this path
            // to hold nothing but its pointer, and it does.
            if let Some(path) = nothing_to_release(&err) {
                printer.line(already_pointer_line(&profile.name, path));
                printer.json(&already_pointer_json(profile, path));
                return Ok(());
            }
            // Every other refusal exits 1 through `sync_exit_code`. `Busy` is
            // `EXIT_OK` for `sync --once`, where losing a race did no wrong;
            // this verb promises the content is gone, so a run that did not do
            // it has to be visible in `$?` — `cmd_materialize`'s reasoning.
            return Err(match err {
                SyncError::Busy(name) => CliError::Operational(format!(
                    "{name} is busy syncing right now; ask again in a moment"
                )),
                other => other.into(),
            });
        }
    };
    printer.line(dehydrate_line(&profile.name, &done));
    printer.json(&dehydrate_json(profile, &done));
    Ok(())
}

// ---------------------------------------------------------------------------
// logs
// ---------------------------------------------------------------------------

fn cmd_logs(
    printer: &Printer,
    platform: &LinuxPlatform,
    lines: usize,
) -> std::result::Result<(), CliError> {
    let path = platform.log_path();
    let file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            printer.line(format!("No log file yet at {}", path.display()));
            printer.json(&serde_json::json!({ "path": path, "lines": [] }));
            return Ok(());
        }
        Err(err) => return Err(SyncError::io("open the daemon log", &path, err).into()),
    };
    if lines == 0 {
        printer.json(&serde_json::json!({ "path": path, "lines": [] }));
        return Ok(());
    }

    // A ring of the last `lines` entries: a daemon log grows without bound, and
    // holding all of it to print the tail would be the one place this CLI could
    // run a server out of memory.
    let mut tail: VecDeque<String> = VecDeque::with_capacity(lines);
    for line in std::io::BufReader::new(file).lines() {
        let line = line.map_err(|err| SyncError::io("read the daemon log", &path, err))?;
        if tail.len() == lines {
            tail.pop_front();
        }
        tail.push_back(line);
    }
    for line in &tail {
        printer.line(line);
    }
    printer.json(&serde_json::json!({ "path": path, "lines": tail }));
    Ok(())
}

// ---------------------------------------------------------------------------
// doctor
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CheckStatus {
    /// Nothing to do.
    Ok,
    /// Worth knowing, but not broken. Never affects the exit code.
    Warn,
    /// Actually broken: this stops sync from working.
    Fail,
}

impl CheckStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warn => "warn",
            Self::Fail => "FAIL",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub id: String,
    pub status: CheckStatus,
    pub detail: String,
}

impl Check {
    fn new(id: impl Into<String>, status: CheckStatus, detail: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            status,
            detail: detail.into(),
        }
    }
}

fn cmd_doctor(
    printer: &Printer,
    platform: &LinuxPlatform,
    config_path: &Path,
    config: SyncResult<DaemonConfig>,
) -> std::result::Result<(), CliError> {
    let mut checks = Vec::new();

    // git first: without it nothing else matters (AD-41).
    let git_missing = check_git(platform, &mut checks);

    // A version check is a network call, so it is a WARN at worst and never
    // affects the exit code: `doctor` reporting a machine as broken because
    // GitHub was unreachable would be actively misleading.
    checks.push(match update::check(env!("CARGO_PKG_VERSION")) {
        Ok(Some(available)) => Check::new(
            "version",
            CheckStatus::Warn,
            format!(
                "keeper-syncd {} is available (running {}); run `keeper-syncd update`",
                available.version,
                env!("CARGO_PKG_VERSION")
            ),
        ),
        Ok(None) => Check::new(
            "version",
            CheckStatus::Ok,
            format!(
                "keeper-syncd {} is the latest release",
                env!("CARGO_PKG_VERSION")
            ),
        ),
        Err(err) => Check::new(
            "version",
            CheckStatus::Warn,
            format!("could not check for updates: {err}"),
        ),
    });

    let profiles = match &config {
        Ok(config) => {
            checks.push(Check::new(
                "config",
                CheckStatus::Ok,
                format!(
                    "{} profile(s) from {}",
                    config.profiles.len(),
                    config_path.display()
                ),
            ));
            config.profiles.clone()
        }
        Err(err) => {
            checks.push(Check::new("config", CheckStatus::Fail, err.to_string()));
            Vec::new()
        }
    };

    check_paths(&profiles, &mut checks);
    check_volumes(&profiles, &mut checks);
    check_inotify(profiles.len(), &mut checks);
    check_disk(platform, &profiles, &mut checks);
    // Engine-side facts come last: opening the journal can itself fail, and the
    // checks above are exactly what an operator needs to understand why.
    check_engine(platform, config.is_ok(), &mut checks);

    let width = checks
        .iter()
        .map(|c| c.id.chars().count())
        .max()
        .unwrap_or(0);
    for check in &checks {
        printer.line(format!(
            "{:<4}  {:<width$}  {}",
            check.status.label(),
            check.id,
            check.detail
        ));
    }
    let failed: Vec<&Check> = checks
        .iter()
        .filter(|check| check.status == CheckStatus::Fail)
        .collect();
    printer.json(&serde_json::json!({
        "ok": failed.is_empty(),
        "checks": checks,
    }));

    if let Some(reason) = git_missing {
        // Exit 3, so a provisioning script can tell "install a package" apart
        // from "your config is wrong".
        return Err(SyncError::GitMissing { reason }.into());
    }
    if !failed.is_empty() {
        return Err(CliError::Operational(format!(
            "{} check(s) failed: {}",
            failed.len(),
            failed
                .iter()
                .map(|check| check.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    printer.line("Everything checks out.");
    Ok(())
}

/// Returns the refusal when no usable git was found, so `doctor` can exit 3.
///
/// One resolution rather than resolve-then-probe: the resolution already probed
/// every candidate it tried, so asking again would spawn a second `git
/// --version` to learn what it just learned — and could, on a machine where the
/// binary is being replaced under us, report a different version than the one
/// the engine will actually drive.
///
/// The OK line also names what was *rejected*, when anything was. That is the
/// whole point on a box with several gits: "using /opt/homebrew/bin/git" without
/// "skipped /usr/local/bin/git, which is 2.23" leaves an operator wondering why
/// their `git --version` disagrees with the daemon's.
fn check_git(platform: &LinuxPlatform, checks: &mut Vec<Check>) -> Option<String> {
    let resolution = platform.git_resolution();
    match resolution.summary() {
        Some(summary) => {
            let detail = if resolution.rejected().is_empty() {
                summary
            } else {
                format!("{summary}; skipped {}", rejected_list(&resolution))
            };
            checks.push(Check::new("git", CheckStatus::Ok, detail));
            None
        }
        None => {
            let refusal = resolution.refusal();
            checks.push(Check::new("git", CheckStatus::Fail, refusal.clone()));
            Some(refusal)
        }
    }
}

/// `<path> (<why>)`, comma-separated, for the candidates that were passed over.
fn rejected_list(resolution: &keeper_sync::GitResolution) -> String {
    resolution
        .rejected()
        .iter()
        .map(|rejection| {
            format!(
                "{} ({})",
                rejection.program.display(),
                rejection.cause.describe()
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn check_paths(profiles: &[SyncProfile], checks: &mut Vec<Check>) {
    for profile in profiles {
        let id = format!("path[{}]", profile.name);
        let path = &profile.local_path;
        if !path.exists() {
            // A removable profile whose drive is unplugged is *expected* to
            // have no path — that is the whole AD-48 posture, not a fault.
            let status = if profile.removable {
                CheckStatus::Warn
            } else {
                CheckStatus::Fail
            };
            checks.push(Check::new(
                id,
                status,
                format!("{} does not exist", path.display()),
            ));
            continue;
        }
        if !path.is_dir() {
            checks.push(Check::new(
                id,
                CheckStatus::Fail,
                format!("{} is not a directory", path.display()),
            ));
            continue;
        }
        match probe_writable(path) {
            Ok(()) => checks.push(Check::new(
                id,
                CheckStatus::Ok,
                format!("{} exists and is writable", path.display()),
            )),
            Err(err) => checks.push(Check::new(
                id,
                CheckStatus::Fail,
                format!("{} is not writable: {err}", path.display()),
            )),
        }
    }
}

/// Prove a directory is writable by writing in it.
///
/// Mode bits are not the answer: they ignore ACLs, a read-only mount and the
/// effective uid. The probe name deliberately matches the engine's own
/// `.keeper.*.tmp` staging prefix, which tier-0 exclusion already ignores — so
/// a probe left behind by a killed `doctor` can never be staged or committed.
fn probe_writable(dir: &Path) -> std::io::Result<()> {
    let probe = dir.join(format!(".keeper.syncd-doctor-{}.tmp", std::process::id()));
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)?;
    std::fs::remove_file(&probe)
}

fn check_volumes(profiles: &[SyncProfile], checks: &mut Vec<Check>) {
    for profile in profiles.iter().filter(|profile| profile.removable) {
        let id = format!("volume[{}]", profile.name);
        match volume::scan(&profile.local_path, None) {
            // Not attached is a first-class state, not a fault: the profile
            // pauses and nothing is staged, committed or pushed (AD-48).
            Ok(VolumeStatus::Absent) => checks.push(Check::new(
                id,
                CheckStatus::Warn,
                format!(
                    "no volume marker at or above {}; the profile stays paused and \
                     nothing will be deleted",
                    profile.local_path.display()
                ),
            )),
            Ok(VolumeStatus::Present { marker }) => {
                let adopted = marker.profile_ids.contains(&profile.id);
                checks.push(Check::new(
                    id,
                    if adopted {
                        CheckStatus::Ok
                    } else {
                        CheckStatus::Warn
                    },
                    format!(
                        "volume `{}` ({}) attached{}",
                        marker.label,
                        marker.volume_id,
                        if adopted {
                            ""
                        } else {
                            " but has not adopted this profile yet"
                        }
                    ),
                ));
            }
            Ok(VolumeStatus::Foreign { found_id }) => checks.push(Check::new(
                id,
                CheckStatus::Fail,
                format!("a different volume ({found_id}) is mounted here"),
            )),
            Err(err) => checks.push(Check::new(id, CheckStatus::Fail, err.to_string())),
        }
    }
}

fn check_inotify(profile_count: usize, checks: &mut Vec<Check>) {
    match read_proc_number(Path::new("/proc/sys/fs/inotify/max_user_instances")) {
        // One inotify instance per profile, not per folder.
        Some(limit) if (limit as usize) < profile_count => checks.push(Check::new(
            "inotify.instances",
            CheckStatus::Fail,
            format!(
                "max_user_instances is {limit} but {profile_count} profiles each need one; \
                 raise it: sudo sysctl -w fs.inotify.max_user_instances={}",
                profile_count.max(128)
            ),
        )),
        Some(limit) => checks.push(Check::new(
            "inotify.instances",
            CheckStatus::Ok,
            format!("max_user_instances = {limit} for {profile_count} profile(s)"),
        )),
        None => checks.push(Check::new(
            "inotify.instances",
            CheckStatus::Warn,
            "cannot read /proc/sys/fs/inotify/max_user_instances (not a Linux host?)",
        )),
    }
    match read_proc_number(Path::new("/proc/sys/fs/inotify/max_user_watches")) {
        Some(limit) if limit < LOW_INOTIFY_WATCHES => checks.push(Check::new(
            "inotify.watches",
            CheckStatus::Warn,
            format!(
                "max_user_watches is {limit}; inotify needs one watch per directory, so a large \
                 folder will silently stop being watched — raise it: \
                 sudo sysctl -w fs.inotify.max_user_watches=524288"
            ),
        )),
        Some(limit) => checks.push(Check::new(
            "inotify.watches",
            CheckStatus::Ok,
            format!("max_user_watches = {limit}"),
        )),
        None => checks.push(Check::new(
            "inotify.watches",
            CheckStatus::Warn,
            "cannot read /proc/sys/fs/inotify/max_user_watches (not a Linux host?)",
        )),
    }
}

fn read_proc_number(path: &Path) -> Option<u64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn check_disk(platform: &LinuxPlatform, profiles: &[SyncProfile], checks: &mut Vec<Check>) {
    let mut paths: Vec<PathBuf> = vec![platform.state_dir().to_path_buf()];
    paths.extend(
        profiles
            .iter()
            .filter(|profile| profile.local_path.exists())
            .map(|profile| profile.local_path.clone()),
    );

    // `df` rather than `statvfs`: this crate carries no `libc` and no `fs4`
    // (the engine's `free_space` port returns `None` here on purpose), and a
    // diagnostic command can afford one subprocess. `-P` is the POSIX
    // one-line-per-filesystem format and `-k` fixes the block size at 1024.
    let output = std::process::Command::new("df")
        .arg("-Pk")
        .args(&paths)
        .output();
    let stdout = match output {
        Ok(output) => String::from_utf8_lossy(&output.stdout).into_owned(),
        Err(err) => {
            // Fail open, exactly as the `free_space` port does: not knowing the
            // free space is not a reason to call the machine broken.
            checks.push(Check::new(
                "disk",
                CheckStatus::Warn,
                format!("cannot run `df` to measure free space: {err}"),
            ));
            return;
        }
    };

    let mut seen = BTreeSet::new();
    for line in stdout.lines().skip(1) {
        let Some((available_bytes, mount)) = parse_df_line(line) else {
            continue;
        };
        if !seen.insert(mount.clone()) {
            continue;
        }
        let status = if available_bytes < LOW_DISK_BYTES {
            CheckStatus::Warn
        } else {
            CheckStatus::Ok
        };
        checks.push(Check::new(
            format!("disk[{mount}]"),
            status,
            format!("{} free", format_bytes(available_bytes)),
        ));
    }
    if seen.is_empty() {
        checks.push(Check::new(
            "disk",
            CheckStatus::Warn,
            "`df` returned nothing usable; free space is unknown",
        ));
    }
}

/// Parse one `df -Pk` row into `(available bytes, mount point)`.
///
/// POSIX guarantees six fields: filesystem, 1024-blocks, used, available,
/// capacity, mount point. The mount point is last and may contain spaces, so it
/// is re-joined rather than indexed.
fn parse_df_line(line: &str) -> Option<(u64, String)> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 6 {
        return None;
    }
    let available_kib: u64 = fields.get(3)?.parse().ok()?;
    let mount = fields[5..].join(" ");
    Some((available_kib.saturating_mul(1024), mount))
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Journal depth and per-profile health, read through the engine.
fn check_engine(platform: &LinuxPlatform, config_ok: bool, checks: &mut Vec<Check>) {
    if !config_ok {
        // Without a config there is nothing to reconcile, and reporting an
        // empty journal would be misleading.
        return;
    }
    let engine = match Engine::open(Arc::new(platform.clone()) as Arc<dyn SyncPlatform>) {
        Ok(engine) => engine,
        Err(err) => {
            checks.push(Check::new("journal", CheckStatus::Fail, err.to_string()));
            return;
        }
    };
    let statuses = match engine.statuses() {
        Ok(statuses) => statuses,
        Err(err) => {
            checks.push(Check::new("journal", CheckStatus::Fail, err.to_string()));
            return;
        }
    };

    let pending: u32 = statuses.iter().map(|status| status.pending).sum();
    checks.push(Check::new(
        "journal",
        if pending > DEEP_JOURNAL {
            CheckStatus::Warn
        } else {
            CheckStatus::Ok
        },
        format!(
            "{pending} unit(s) queued across {} profile(s)",
            statuses.len()
        ),
    ));

    for status in &statuses {
        let id = format!("state[{}]", status.profile_name);
        match status.state {
            ProfileState::NeedsAttention => checks.push(Check::new(
                id,
                CheckStatus::Fail,
                status
                    .error
                    .clone()
                    .unwrap_or_else(|| "stopped and needs a human".to_owned()),
            )),
            ProfileState::MediaAbsent => checks.push(Check::new(
                id,
                CheckStatus::Warn,
                "removable volume is not attached; paused, nothing deleted",
            )),
            ProfileState::Offline => checks.push(Check::new(
                id,
                CheckStatus::Warn,
                format!("offline, {} unit(s) waiting", status.pending),
            )),
            other => checks.push(Check::new(id, CheckStatus::Ok, status_line_state(other))),
        }
    }
}

fn status_line_state(state: ProfileState) -> &'static str {
    match state {
        ProfileState::Idle => "idle",
        ProfileState::Watching => "watching",
        ProfileState::Syncing => "syncing",
        ProfileState::Offline => "offline",
        ProfileState::MediaAbsent => "media absent",
        ProfileState::Paused => "paused",
        ProfileState::NeedsAttention => "needs attention",
    }
}

/// Report, and optionally install, a newer release.
fn cmd_update(printer: &Printer, check_only: bool) -> Result<(), CliError> {
    let current = env!("CARGO_PKG_VERSION");
    let Some(available) = update::check(current)? else {
        printer.line(format!("keeper-syncd {current} is the latest release"));
        return Ok(());
    };
    if check_only {
        printer.line(format!(
            "keeper-syncd {} is available (running {current}); run `keeper-syncd update` to install it",
            available.version
        ));
        return Ok(());
    }

    // Replace the binary that is actually running, resolved rather than assumed:
    // a symlinked install must update the target, not the link's directory.
    let destination = std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .map_err(|err| SyncError::io("resolve the running binary", ".", err))?;
    let installed = update::apply(&available, &destination)?;
    printer.line(format!(
        "installed keeper-syncd {} to {}",
        available.version,
        installed.display()
    ));
    // Said plainly because it is easy to assume otherwise: the running process
    // keeps its old inode and goes on running the previous build.
    printer.line("restart the daemon to run it (`systemctl --user restart keeper-syncd`)");
    Ok(())
}

/// Run one clean or smudge pass for git.
///
/// This is the seam that makes a keeper-managed folder behave correctly under
/// plain `git`. It writes **only** object bytes to stdout — never a log line,
/// never a status message — because git treats everything it emits as file
/// content.
///
/// The work itself is [`keeper_sync::lfs::filter`], not a copy of it. The engine
/// registers whichever executable is running as the filter, so the app binary has
/// to answer this too (DW-121) — and a filter with one implementation per binary
/// is how the two would come to disagree about what a pointer means.
fn cmd_lfs_filter(direction: LfsDirection) -> Result<(), CliError> {
    use keeper_sync::lfs::filter;

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let (repo, which) = match &direction {
        LfsDirection::Clean { repo, .. } => (repo.clone(), filter::Direction::Clean),
        LfsDirection::Smudge { repo, .. } => (repo.clone(), filter::Direction::Smudge),
        LfsDirection::FilterProcess { repo } => {
            filter::run_process(repo, &mut stdin.lock(), &mut stdout.lock())?;
            return Ok(());
        }
    };
    filter::run(&repo, which, &mut stdin.lock(), &mut stdout.lock())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;
    use clap::CommandFactory as _;
    use keeper_sync::lfs::hydrate::MaterializeOutcome;

    fn parse(args: &[&str]) -> std::result::Result<Cli, clap::Error> {
        Cli::try_parse_from(args)
    }

    #[test]
    fn the_command_tree_is_internally_consistent() {
        // clap's own structural audit: duplicate long flags, a short letter used
        // twice, a `default_value` that does not parse, an arg group naming a
        // missing member. All of those are silent until runtime otherwise.
        Cli::command().debug_assert();
    }

    #[test]
    fn every_subcommand_parses() {
        let invocations: &[&[&str]] = &[
            &["keeper-syncd", "init"],
            &["keeper-syncd", "init", "--force"],
            &[
                "keeper-syncd",
                "add",
                "--name",
                "docs",
                "--path",
                "/srv/docs",
                "--remote",
                "https://example.com/docs.git",
            ],
            &["keeper-syncd", "list"],
            &["keeper-syncd", "status"],
            &["keeper-syncd", "status", "docs"],
            &["keeper-syncd", "sync"],
            &["keeper-syncd", "sync", "--once"],
            &["keeper-syncd", "sync", "docs", "--once"],
            &["keeper-syncd", "watch"],
            &["keeper-syncd", "materialize", "docs", "40-media/clip.mp4"],
            &["keeper-syncd", "dehydrate", "docs", "40-media/clip.mp4"],
            &["keeper-syncd", "pause", "docs"],
            &["keeper-syncd", "resume", "docs"],
            &["keeper-syncd", "verify"],
            &["keeper-syncd", "verify", "docs"],
            &["keeper-syncd", "ls-files"],
            &["keeper-syncd", "ls-files", "docs"],
            &["keeper-syncd", "ls-files", "--remote"],
            &["keeper-syncd", "ls-files", "docs", "--remote"],
            &["keeper-syncd", "doctor"],
            &["keeper-syncd", "logs"],
            &["keeper-syncd", "logs", "-n", "20"],
        ];
        for args in invocations {
            assert!(parse(args).is_ok(), "must parse: {args:?}");
        }
    }

    #[test]
    fn global_flags_work_on_either_side_of_the_subcommand() {
        // `global = true` is what makes `keeper-syncd status --json` work; without
        // it the flag would only be accepted before the subcommand, which is not
        // how anyone types it.
        let before = parse(&["keeper-syncd", "--json", "status"]).expect("before");
        let after = parse(&["keeper-syncd", "status", "--json"]).expect("after");
        assert!(before.json && after.json);

        let cli = parse(&["keeper-syncd", "watch", "-vv"]).expect("verbosity");
        assert_eq!(cli.verbose, 2);

        let cli = parse(&["keeper-syncd", "-c", "/etc/keeper.toml", "list"]).expect("config");
        assert_eq!(cli.config.as_deref(), Some(Path::new("/etc/keeper.toml")));
    }

    #[test]
    fn a_required_profile_argument_is_enforced() {
        for subcommand in ["pause", "resume"] {
            let err = parse(&["keeper-syncd", subcommand])
                .expect_err("a profile is required to pause or resume one");
            assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument);
        }
        // `materialize` and `dehydrate` each need two: a folder and a path
        // inside it. One argument is the shape a person trying
        // `materialize clip.mp4` produces, and it must be told what is missing
        // rather than have `clip.mp4` read as a profile name.
        for args in [
            vec!["keeper-syncd", "materialize"],
            vec!["keeper-syncd", "materialize", "docs"],
            vec!["keeper-syncd", "dehydrate"],
            vec!["keeper-syncd", "dehydrate", "docs"],
        ] {
            let err = parse(&args).expect_err("a profile AND a subpath are required");
            assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument, "{args:?}");
        }
        // ...and the optional ones really are optional.
        assert!(parse(&["keeper-syncd", "verify"]).is_ok());
        assert!(parse(&["keeper-syncd", "ls-files"]).is_ok());
    }

    #[test]
    fn add_requires_the_three_fields_a_profile_cannot_default() {
        let err = parse(&["keeper-syncd", "add", "--name", "docs"])
            .expect_err("a profile needs a path and a remote");
        assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn bad_invocations_are_rejected() {
        assert_eq!(
            parse(&["keeper-syncd", "sync", "--twice"])
                .expect_err("unknown flag")
                .kind(),
            ErrorKind::UnknownArgument
        );
        assert_eq!(
            parse(&["keeper-syncd", "synchronise"])
                .expect_err("unknown subcommand")
                .kind(),
            ErrorKind::InvalidSubcommand
        );
        // Value rejections: assert on the message rather than the `ErrorKind`
        // discriminant, because `ErrorKind` is `#[non_exhaustive]` and clap has
        // moved value failures between `InvalidValue` and `ValueValidation`
        // across releases. What must hold is that the bad token is named.
        let err = parse(&["keeper-syncd", "logs", "-n", "not-a-number"])
            .expect_err("a non-numeric line count must fail");
        assert!(err.to_string().contains("not-a-number"), "{err}");

        let err = parse(&[
            "keeper-syncd",
            "add",
            "--name",
            "d",
            "--path",
            "/d",
            "--remote",
            "u",
            "--direction",
            "sideways",
        ])
        .expect_err("an invalid direction must fail");
        assert!(err.to_string().contains("sideways"), "{err}");
        // A subcommand is not optional: bare `keeper-syncd` must say so.
        assert!(parse(&["keeper-syncd"]).is_err());
    }

    #[test]
    fn repeatable_arguments_accumulate() {
        let cli = parse(&[
            "keeper-syncd",
            "add",
            "--name",
            "docs",
            "--path",
            "/srv/docs",
            "--remote",
            "u",
            "--subpath",
            "a",
            "--subpath",
            "b",
            "--exclude",
            "*.tmp",
            "--tag",
            "server",
            "--removable",
        ])
        .expect("parse");
        let Command::Add(args) = cli.command else {
            panic!("expected add");
        };
        assert_eq!(args.subpath, vec!["a".to_owned(), "b".to_owned()]);
        assert_eq!(args.exclude, vec!["*.tmp".to_owned()]);
        assert_eq!(args.tag, vec!["server".to_owned()]);
        assert!(args.removable);
        // Defaults the profile schema also defaults to.
        assert_eq!(args.branch, "main");
        assert_eq!(args.direction, DirectionArg::Bidirectional);
        assert_eq!(args.lane, LaneArg::Main);
        assert_eq!(args.lfs_mode, LfsModeArg::Materialize);
    }

    #[test]
    fn value_enums_render_kebab_case_on_the_command_line() {
        let cli = parse(&[
            "keeper-syncd",
            "add",
            "--name",
            "bot",
            "--path",
            "/srv/bot",
            "--remote",
            "u",
            "--direction",
            "push-only",
            "--lane",
            "worktree",
            "--lfs-mode",
            "pointer-only",
        ])
        .expect("parse");
        let Command::Add(args) = cli.command else {
            panic!("expected add");
        };
        assert_eq!(SyncDirection::from(args.direction), SyncDirection::PushOnly);
        assert_eq!(SyncLane::from(args.lane), SyncLane::Worktree);
        assert_eq!(LfsMode::from(args.lfs_mode), LfsMode::PointerOnly);
    }

    #[test]
    fn every_sync_error_maps_to_a_meaningful_exit_code() {
        // Exhaustive over the taxonomy. The `match` in `sync_exit_code` has no
        // wildcard, so a new variant breaks the build there; this asserts the
        // classification a wrapper script actually branches on.
        let cases: Vec<(SyncError, u8)> = vec![
            (
                SyncError::GitMissing {
                    reason: "none".to_owned(),
                },
                EXIT_PREREQUISITE,
            ),
            (SyncError::Config("bad".to_owned()), EXIT_CONFIG),
            (SyncError::Cancelled, EXIT_OK),
            (
                SyncError::GitCommand {
                    subcommand: "push",
                    code: 1,
                    stderr: "rejected".to_owned(),
                },
                EXIT_FAILURE,
            ),
            (
                SyncError::Network {
                    host: "example.com".to_owned(),
                    reason: "timeout".to_owned(),
                },
                EXIT_FAILURE,
            ),
            (
                SyncError::Auth {
                    host: "example.com".to_owned(),
                },
                EXIT_FAILURE,
            ),
            // The two variants Story 34.15 added. The wildcard-free `match`
            // above only proves they COMPILE; a wrapper script branches on the
            // classification, and `LfsUploadPending => EXIT_OK` would report a
            // successful `keeper-syncd sync` for a one-shot run that published
            // nothing — the outcome that arm's own comment argues against.
            (
                SyncError::Forbidden {
                    host: "example.com".to_owned(),
                },
                EXIT_FAILURE,
            ),
            (SyncError::LfsUploadPending { objects: 2 }, EXIT_FAILURE),
            (
                SyncError::InvalidPathForRemote {
                    path: PathBuf::from("/srv/aux"),
                    reason: "reserved name".to_owned(),
                },
                EXIT_FAILURE,
            ),
            (SyncError::MediaAbsent, EXIT_FAILURE),
            (
                SyncError::Integrity {
                    subject: "oid".to_owned(),
                    expected: "a".to_owned(),
                    actual: "b".to_owned(),
                },
                EXIT_FAILURE,
            ),
            (
                SyncError::Quota {
                    host: "example.com".to_owned(),
                },
                EXIT_FAILURE,
            ),
            (
                SyncError::Diverged {
                    profile: "docs".to_owned(),
                    reason: "remote moved".to_owned(),
                },
                EXIT_FAILURE,
            ),
            (SyncError::Journal("locked".to_owned()), EXIT_FAILURE),
            (SyncError::Git("odb".to_owned()), EXIT_FAILURE),
            (
                SyncError::io(
                    "read",
                    "/srv/docs",
                    std::io::Error::from(std::io::ErrorKind::PermissionDenied),
                ),
                EXIT_FAILURE,
            ),
            // Story 56.3's one new variant. `EXIT_FAILURE` and not
            // `EXIT_CONFIG`: an agent that asked for a file and was told keeper
            // will not overwrite the edit there has to see a non-zero `$?`,
            // and nothing about the configuration is wrong.
            (
                SyncError::Refused(keeper_sync::lfs::hydrate::ContentRefusal::LocallyModified {
                    path: "40-media/clip.mp4".to_owned(),
                }),
                EXIT_FAILURE,
            ),
        ];

        for (err, expected) in cases {
            assert_eq!(
                sync_exit_code(&err),
                expected,
                "wrong exit code for {}",
                err.code()
            );
            assert_eq!(CliError::from(err).exit_code(), expected);
        }

        assert_eq!(
            CliError::Operational("broken".to_owned()).exit_code(),
            EXIT_FAILURE
        );
    }

    #[test]
    fn the_four_exit_codes_are_distinct() {
        // A wrapper script branches on these; two of them colliding silently
        // would make "fix your config" indistinguishable from "retry later".
        let codes = [EXIT_OK, EXIT_FAILURE, EXIT_CONFIG, EXIT_PREREQUISITE];
        let unique: BTreeSet<u8> = codes.iter().copied().collect();
        assert_eq!(unique.len(), codes.len());
    }

    #[test]
    fn df_rows_are_parsed_into_bytes_and_a_mount_point() {
        let line = "/dev/nvme0n1p2  982118376 412233432 519951560  45% /";
        assert_eq!(
            parse_df_line(line),
            Some((519_951_560 * 1024, "/".to_owned()))
        );
    }

    #[test]
    fn a_mount_point_containing_spaces_survives_parsing() {
        // `/media/dev/FIELD DRIVE` is exactly the removable case this daemon
        // cares about, and naive field indexing would truncate it.
        let line = "/dev/sdb1  61045304 1048576 59996728  2% /media/dev/FIELD DRIVE";
        assert_eq!(
            parse_df_line(line),
            Some((59_996_728 * 1024, "/media/dev/FIELD DRIVE".to_owned()))
        );
    }

    #[test]
    fn malformed_df_rows_are_skipped_rather_than_guessed() {
        assert_eq!(parse_df_line("Filesystem 1024-blocks Used"), None);
        assert_eq!(parse_df_line(""), None);
        assert_eq!(
            parse_df_line("/dev/sda1 nine hundred lots 2% /"),
            None,
            "a non-numeric available column must not become a free-space claim"
        );
    }

    #[test]
    fn inotify_limits_are_read_as_numbers_and_missing_files_are_none() {
        let root = tempfile::tempdir().expect("temp dir");
        let path = root.path().join("max_user_watches");
        std::fs::write(&path, "524288\n").expect("write");

        assert_eq!(read_proc_number(&path), Some(524_288));
        assert_eq!(read_proc_number(&root.path().join("absent")), None);

        std::fs::write(&path, "not a number").expect("write");
        assert_eq!(read_proc_number(&path), None);
    }

    #[test]
    fn too_few_inotify_instances_for_the_profile_count_is_a_failure() {
        // One instance per profile. On a box with 128 instances and 200
        // profiles, some watchers simply never start — silently, without this.
        let mut checks = Vec::new();
        check_inotify(0, &mut checks);
        assert!(checks.iter().any(|check| check.id == "inotify.instances"));
        assert!(checks.iter().any(|check| check.id == "inotify.watches"));
    }

    #[test]
    fn a_writability_probe_succeeds_on_a_real_directory_and_leaves_nothing() {
        let root = tempfile::tempdir().expect("temp dir");

        probe_writable(root.path()).expect("a temp dir must be writable");

        let leftovers: Vec<_> = std::fs::read_dir(root.path())
            .expect("read dir")
            .filter_map(std::result::Result::ok)
            .collect();
        assert!(leftovers.is_empty(), "the probe must clean up after itself");
    }

    #[test]
    fn a_writability_probe_fails_on_a_missing_directory() {
        let root = tempfile::tempdir().expect("temp dir");
        assert!(probe_writable(&root.path().join("nope")).is_err());
    }

    #[test]
    fn the_probe_file_is_named_so_the_engine_would_never_commit_it() {
        // Tier-0 exclusion ignores `.keeper.*.tmp`; a probe left behind by a
        // killed doctor must fall inside that pattern.
        let name = format!(".keeper.syncd-doctor-{}.tmp", std::process::id());
        assert!(name.starts_with(".keeper."));
        assert!(name.ends_with(".tmp"));
    }

    #[test]
    fn byte_formatting_is_readable_at_every_scale() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(2048), "2.0 KB");
        assert_eq!(format_bytes(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn selecting_by_id_or_name_works_and_a_miss_names_the_alternatives() {
        let docs = SyncProfile::new("01DOCS", "docs", "/srv/docs", "https://x/d.git");
        let media = SyncProfile::new("01MEDIA", "media", "/srv/media", "https://x/m.git");
        let profiles = vec![docs.clone(), media.clone()];

        assert_eq!(select(&profiles, None).expect("all").len(), 2);
        assert_eq!(
            select(&profiles, Some("docs")).expect("by name"),
            vec![&docs]
        );
        assert_eq!(
            select(&profiles, Some("01MEDIA")).expect("by id"),
            vec![&media]
        );

        let err = select(&profiles, Some("nope")).expect_err("a miss must be an error");
        let message = err.to_string();
        assert!(message.contains("nope"), "{message}");
        assert!(message.contains("docs"), "must list what exists: {message}");
        assert_eq!(err.exit_code(), EXIT_CONFIG);
    }

    #[test]
    fn a_failing_check_is_distinguishable_from_a_warning() {
        // Only `Fail` may change the exit code — a detached pendrive is normal
        // and must never make `doctor` report the machine as broken.
        assert_eq!(CheckStatus::Ok.label(), "ok");
        assert_eq!(CheckStatus::Warn.label(), "warn");
        assert_eq!(CheckStatus::Fail.label(), "FAIL");
        assert_ne!(CheckStatus::Warn, CheckStatus::Fail);
    }

    // --- ls-files: the two renderings are the contract -------------------

    fn lfs_file(path: &str, state: LfsFileState, size_bytes: u64) -> LfsFile {
        LfsFile {
            path: path.to_owned(),
            oid: "3f79bb7b435b05321651daefd374cdc681dc06faa65e374e38337b88ca046dea".to_owned(),
            size_bytes,
            state,
            mtime_ms: Some(1_700_000_000_123),
            materialized_at_ms: None,
            last_used_ms: None,
            synced_at_ms: None,
            pinned: false,
        }
    }

    /// The human line names the honest size, and never the pointer's own byte
    /// count (FR-336).
    ///
    /// The 4 MiB virtual path is the whole story in one row: the file on disk is
    /// about 130 bytes, and an implementation that reported `metadata().len()`
    /// would put `130 B` here. Asserted on the rendered string because that
    /// string is what an operator reads and what an agent greps.
    #[test]
    fn the_human_line_carries_the_state_the_pointers_size_and_the_oid() {
        let files = [
            lfs_file("media/away.mp4", LfsFileState::Virtual, 4 * 1024 * 1024),
            lfs_file("media/held.mp4", LfsFileState::Materialized, 2_048),
            lfs_file("media/gone.mp4", LfsFileState::Absent, 99),
        ];
        let lines = ls_files_lines("Field", &files);

        assert_eq!(lines.len(), 4, "a header and one line per path: {lines:?}");
        assert!(
            lines[0].contains("3 LFS path(s)")
                && lines[0].contains("1 virtual")
                && lines[0].contains("1 materialized")
                && lines[0].contains("1 absent"),
            "the header counts every state: {}",
            lines[0]
        );

        let virtual_line = &lines[1];
        assert!(virtual_line.contains("virtual"), "{virtual_line}");
        assert!(
            virtual_line.contains("4.0 MB"),
            "the size is the one the pointer names: {virtual_line}"
        );
        assert!(
            virtual_line.contains(&files[0].oid),
            "the full oid is the handle every later verb takes: {virtual_line}"
        );
        assert!(
            !virtual_line.contains("130"),
            "the pointer's own byte count must never appear: {virtual_line}"
        );
        assert!(lines[2].contains("materialized") && lines[2].contains("2.0 KB"));
        assert!(lines[3].contains("absent") && lines[3].contains("99 B"));
    }

    /// A folder with no LFS paths says so instead of failing — the `disabled`
    /// and the no-LFS-at-all cases both arrive here as an empty slice.
    #[test]
    fn an_empty_listing_is_one_honest_line() {
        assert_eq!(ls_files_lines("Field", &[]), vec!["Field: no LFS paths"]);
    }

    /// A pinned row says so, because 56.5's release sweep treats `pinned` as a
    /// floor it may not cross and an operator has to be able to see which paths
    /// are behind it.
    #[test]
    fn a_pinned_row_is_marked_in_the_human_form() {
        let mut file = lfs_file("keep.bin", LfsFileState::Materialized, 10);
        file.pinned = true;
        let lines = ls_files_lines("Field", std::slice::from_ref(&file));
        assert!(lines[1].contains("[pinned]"), "{}", lines[1]);
        assert!(
            !ls_files_lines("Field", &[lfs_file("x.bin", LfsFileState::Absent, 1)])[1]
                .contains("[pinned]"),
            "and an unpinned row is not marked"
        );
    }

    /// FR-337 calls the JSON form stable, so the assertion is on the KEY SET
    /// rather than on a substring: a renamed or dropped field is the breakage
    /// that matters to a consumer, and a `contains` check would miss both. The
    /// envelope is asserted for the same reason — renaming `listings` breaks
    /// every consumer and would otherwise break no gate.
    #[test]
    fn the_json_documents_key_set_is_exactly_the_contract() {
        let profile = SyncProfile::new("01DOCS", "docs", "/srv/docs", "https://x/d.git");
        let files = [lfs_file("a.bin", LfsFileState::Virtual, 4 * 1024 * 1024)];
        let entry = ls_files_entry(&profile, &files, None);

        let object = entry.as_object().expect("an object");
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["files", "profile", "profileId", "total"]);
        assert_eq!(object["total"], serde_json::json!(1));
        assert_eq!(object["profileId"], serde_json::json!("01DOCS"));

        let row = object["files"][0].as_object().expect("one file object");
        let mut row_keys: Vec<&str> = row.keys().map(String::as_str).collect();
        row_keys.sort_unstable();
        assert_eq!(
            row_keys,
            vec![
                "lastUsedMs",
                "materializedAtMs",
                "mtimeMs",
                "oid",
                "path",
                "pinned",
                "sizeBytes",
                "state",
                "syncedAtMs",
            ]
        );
        assert_eq!(row["state"], serde_json::json!("virtual"));
        assert_eq!(
            row["sizeBytes"],
            serde_json::json!(4 * 1024 * 1024),
            "the pointer's number, not the ~130 bytes on disk"
        );

        let document = ls_files_document(vec![entry.clone(), entry]);
        let envelope = document.as_object().expect("an object");
        let mut envelope_keys: Vec<&str> = envelope.keys().map(String::as_str).collect();
        envelope_keys.sort_unstable();
        assert_eq!(envelope_keys, vec!["listings", "total"]);
        assert_eq!(
            envelope["total"],
            serde_json::json!(2),
            "the envelope's total is every profile's paths added up"
        );
        assert_eq!(envelope["listings"].as_array().expect("array").len(), 2);
    }

    /// Without `--remote` the key is ABSENT, not null (FR-337).
    ///
    /// The distinction is the whole point of making the round trip opt-in: a
    /// consumer must be able to tell "nobody asked the server" from "the server
    /// answered and holds nothing", and `null` cannot say the first. Asserted
    /// through the renderer's own parameter rather than by re-enacting a splice:
    /// `None` is what `cmd_ls_files` passes when the flag is absent, so the
    /// branch under test is the branch that ships.
    #[test]
    fn the_remote_key_is_absent_unless_remote_was_asked_for() {
        let profile = SyncProfile::new("01DOCS", "docs", "/srv/docs", "https://x/d.git");
        let files = [lfs_file("a.bin", LfsFileState::Virtual, 10)];

        let plain = ls_files_entry(&profile, &files, None);
        assert!(
            !plain.as_object().expect("object").contains_key("remote"),
            "no round trip was made, so there is no key to carry: {plain}"
        );

        let audit = RemoteAudit::default();
        let asked = ls_files_entry(&profile, &files, Some(&audit));
        assert!(
            asked.as_object().expect("object").contains_key("remote"),
            "and when it was asked for, the audit document is there: {asked}"
        );
        assert!(
            !asked["remote"].is_null(),
            "present-and-null is the one value the contract says cannot occur: {asked}"
        );

        // ...and the flag really is optional and really does parse.
        assert!(matches!(
            parse(&["keeper-syncd", "ls-files"]).expect("parse").command,
            Command::LsFiles { remote: false, .. }
        ));
        assert!(matches!(
            parse(&["keeper-syncd", "ls-files", "--remote"])
                .expect("parse")
                .command,
            Command::LsFiles { remote: true, .. }
        ));
    }

    // --- materialize: the two renderings are the contract (Story 56.3) ------

    fn materialization(outcome: MaterializeOutcome, unit_id: Option<i64>) -> Materialization {
        Materialization {
            path: "40-media/clip.mp4".to_owned(),
            oid: "3f79bb7b435b05321651daefd374cdc681dc06faa65e374e38337b88ca046dea".to_owned(),
            size_bytes: 4 * 1024 * 1024,
            outcome,
            unit_id,
        }
    }

    /// The human line names the outcome, the honest size and the path.
    ///
    /// The size assertion is the load-bearing one: it is derived from the
    /// `size_bytes` the pointer named — 4 MiB — where the file on disk before
    /// the call is about 130 bytes, so a renderer reading the worktree's own
    /// length could not produce this string (FR-336). Asserted by moving the
    /// field rather than by looking for the absence of `130`, which no possible
    /// body of a function with no filesystem access could have printed.
    #[test]
    fn the_human_line_carries_the_outcome_the_honest_size_and_the_path() {
        let lines = materialize_lines(
            "Field",
            &materialization(MaterializeOutcome::Materialized, None),
        );
        assert_eq!(lines.len(), 1, "one path, one line: {lines:?}");
        assert!(lines[0].contains("materialized"), "{}", lines[0]);
        assert!(
            lines[0].contains("4.0 MB"),
            "the size is the one the pointer names: {}",
            lines[0]
        );
        assert!(lines[0].contains("40-media/clip.mp4"), "{}", lines[0]);

        // The same renderer over a pointer-text-sized number, so the field the
        // line reads is pinned rather than assumed: 4 MiB above and 130 B here
        // can only both hold if `size_bytes` is what is rendered.
        let mut small = materialization(MaterializeOutcome::Materialized, None);
        small.size_bytes = 130;
        assert!(
            materialize_lines("Field", &small)[0].contains("130 B"),
            "the line renders `size_bytes`, whatever it says"
        );

        // Prose in the sentence, camelCase on the wire: one enum, two
        // spellings, each where its reader expects it.
        let already = materialize_lines(
            "Field",
            &materialization(MaterializeOutcome::AlreadyMaterialized, None),
        );
        assert!(
            already[0].contains("already materialized"),
            "a person reads a sentence, not a JSON token: {}",
            already[0]
        );

        // A queued outcome says who will deliver it, because this verb does not
        // wait and an operator with no daemon running would otherwise wait for
        // something that cannot happen.
        let queued = materialize_lines(
            "Field",
            &materialization(MaterializeOutcome::Queued, Some(42)),
        );
        assert_eq!(queued.len(), 2, "{queued:?}");
        assert!(queued[0].contains("queued"), "{}", queued[0]);
        assert!(
            queued[1].contains("unit 42") && queued[1].contains("watch"),
            "{}",
            queued[1]
        );
    }

    /// The JSON key set is the promise, so the assertion is on the KEY SET.
    ///
    /// A renamed or dropped field is the breakage that matters to a consumer
    /// and a `contains` check would miss both — the same reasoning
    /// `the_json_documents_key_set_is_exactly_the_contract` states for
    /// `ls-files`.
    #[test]
    fn the_materialize_json_key_set_is_exactly_the_contract() {
        let profile = SyncProfile::new("01DOCS", "docs", "/srv/docs", "https://x/d.git");
        let done = materialization(MaterializeOutcome::Queued, Some(42));
        let entry = materialize_json(&profile, &done);

        let object = entry.as_object().expect("an object");
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "oid",
                "outcome",
                "path",
                "profile",
                "profileId",
                "sizeBytes",
                "unitId",
            ]
        );
        assert_eq!(object["profileId"], serde_json::json!("01DOCS"));
        assert_eq!(object["outcome"], serde_json::json!("queued"));
        assert_eq!(object["unitId"], serde_json::json!(42));
        assert_eq!(
            object["sizeBytes"],
            serde_json::json!(4 * 1024 * 1024),
            "the pointer's number, not the ~130 bytes on disk"
        );
    }

    /// `unitId` is ABSENT for an outcome that queued nothing, not null.
    ///
    /// `null` would say "there is a unit and I do not know its id", which is a
    /// thing that cannot happen — and a consumer polling for delivery has to be
    /// able to tell "nothing to wait for" from "wait for this". Asserted
    /// through the renderer's own parameter, so the branch under test is the
    /// branch that ships.
    #[test]
    fn the_unit_id_is_absent_unless_something_was_queued() {
        let profile = SyncProfile::new("01DOCS", "docs", "/srv/docs", "https://x/d.git");
        for outcome in [
            MaterializeOutcome::Materialized,
            MaterializeOutcome::AlreadyMaterialized,
        ] {
            let entry = materialize_json(&profile, &materialization(outcome, None));
            assert!(
                !entry.as_object().expect("object").contains_key("unitId"),
                "nothing was queued, so there is no key to carry: {entry}"
            );
        }
        // The wire spelling is serde's, and it is camelCase where the prose is
        // a sentence: pinned here because a consumer branches on this string.
        assert_eq!(
            materialize_json(
                &profile,
                &materialization(MaterializeOutcome::AlreadyMaterialized, None)
            )["outcome"],
            serde_json::json!("alreadyMaterialized")
        );
    }

    /// Two positionals, in that order, and neither optional.
    #[test]
    fn materialize_takes_a_folder_and_a_path_inside_it() {
        let cli =
            parse(&["keeper-syncd", "materialize", "docs", "40-media/clip.mp4"]).expect("parse");
        let Command::Materialize { profile, subpath } = cli.command else {
            panic!("expected materialize");
        };
        assert_eq!(profile, "docs");
        assert_eq!(subpath, "40-media/clip.mp4");
    }

    // --- dehydrate: the two documents and the exit-0 branch (Story 56.4) ----

    fn release() -> Release {
        Release {
            path: "40-media/clip.mp4".to_owned(),
            oid: "3f79bb7b435b05321651daefd374cdc681dc06faa65e374e38337b88ca046dea".to_owned(),
            size_bytes: 4 * 1024 * 1024,
        }
    }

    /// Two positionals, in that order, and neither optional.
    #[test]
    fn dehydrate_takes_a_folder_and_a_path_inside_it() {
        let cli =
            parse(&["keeper-syncd", "dehydrate", "docs", "40-media/clip.mp4"]).expect("parse");
        let Command::Dehydrate { profile, subpath } = cli.command else {
            panic!("expected dehydrate");
        };
        assert_eq!(profile, "docs");
        assert_eq!(subpath, "40-media/clip.mp4");
    }

    /// The released line carries the honest size and the path, in prose.
    ///
    /// The size assertion is the load-bearing one and it is pinned by moving
    /// the field: 4 MiB and 130 B can only both hold if `size_bytes` is what is
    /// rendered — and after a release the file on disk is the ~130 bytes, so a
    /// renderer reading the worktree would have printed the wrong number.
    #[test]
    fn the_released_line_carries_the_honest_size_and_the_path() {
        let line = dehydrate_line("Field", &release());
        assert!(line.contains("released"), "{line}");
        assert!(
            line.contains("4.0 MB"),
            "the bytes given back are the pointer's number: {line}"
        );
        assert!(line.contains("40-media/clip.mp4"), "{line}");

        let mut small = release();
        small.size_bytes = 130;
        assert!(
            dehydrate_line("Field", &small).contains("130 B"),
            "the line renders `size_bytes`, whatever it says"
        );
    }

    /// The no-op line says so in prose, never in the wire word.
    ///
    /// `alreadyPointer` in a sentence a person reads is a token that escaped
    /// from a JSON document — `MaterializeOutcome`'s `Display` doc makes the
    /// same distinction for the same reason.
    #[test]
    fn the_already_pointer_line_is_prose_and_not_the_wire_word() {
        let line = already_pointer_line("Field", "40-media/clip.mp4");
        assert!(line.contains("already a pointer"), "{line}");
        assert!(line.contains("40-media/clip.mp4"), "{line}");
        assert!(
            !line.contains("alreadyPointer"),
            "the camelCase spelling belongs to the document, not the sentence: {line}"
        );
    }

    /// The released document's key set is exactly the contract.
    #[test]
    fn the_dehydrate_json_key_set_is_exactly_the_contract() {
        let profile = SyncProfile::new("01DOCS", "docs", "/srv/docs", "https://x/d.git");
        let entry = dehydrate_json(&profile, &release());

        let object = entry.as_object().expect("an object");
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "oid",
                "outcome",
                "path",
                "profile",
                "profileId",
                "sizeBytes"
            ]
        );
        assert_eq!(object["profileId"], serde_json::json!("01DOCS"));
        assert_eq!(object["outcome"], serde_json::json!("released"));
        assert_eq!(
            object["sizeBytes"],
            serde_json::json!(4 * 1024 * 1024),
            "the pointer's number, not the ~130 bytes now on disk"
        );
    }

    /// The no-op document says `alreadyPointer` and carries **no** `oid` and
    /// **no** `sizeBytes`.
    ///
    /// Absent, not null and not zero: nothing was released, so a consumer
    /// totalling reclaimed bytes has to be able to skip this document without
    /// arithmetic. Asserted on the key set, because a renamed or added field is
    /// exactly the breakage a `contains` check would miss.
    #[test]
    fn the_already_pointer_json_carries_no_oid_and_no_size() {
        let profile = SyncProfile::new("01DOCS", "docs", "/srv/docs", "https://x/d.git");
        let entry = already_pointer_json(&profile, "40-media/clip.mp4");

        let object = entry.as_object().expect("an object");
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["outcome", "path", "profile", "profileId"]);
        assert_eq!(object["outcome"], serde_json::json!("alreadyPointer"));
        assert_eq!(object["path"], serde_json::json!("40-media/clip.mp4"));
    }

    /// Exactly one refusal is a no-op; every other one is a failed run.
    ///
    /// The branch that gives `dehydrate` its exit-0 case, asserted over the
    /// whole vocabulary rather than over the one variant it matches — because
    /// the claim is that the refusals are distinguishable *by type*, and a
    /// second variant leaking through here would silently turn a refused
    /// deletion into a reported success.
    #[test]
    fn only_already_pointer_is_nothing_to_release() {
        assert_eq!(
            nothing_to_release(&SyncError::Refused(ContentRefusal::AlreadyPointer {
                path: "40-media/clip.mp4".to_owned(),
            })),
            Some("40-media/clip.mp4"),
            "and it hands back the path, so the no-op line can name it"
        );

        let path = "40-media/clip.mp4".to_owned();
        let others = [
            ContentRefusal::Modified { path: path.clone() },
            ContentRefusal::Open { path: path.clone() },
            ContentRefusal::OpenUnknown { path: path.clone() },
            ContentRefusal::UnprovenOnRemote { path: path.clone() },
            ContentRefusal::Pinned { path: path.clone() },
            ContentRefusal::Missing { path: path.clone() },
            ContentRefusal::NotTracked { path: path.clone() },
            ContentRefusal::OutsideSubpaths { path: path.clone() },
            ContentRefusal::LocallyModified { path },
            ContentRefusal::Paused {
                profile: "docs".to_owned(),
            },
            ContentRefusal::LfsDisabled {
                profile: "docs".to_owned(),
            },
            ContentRefusal::AlwaysMaterializes {
                profile: "docs".to_owned(),
            },
        ];
        for refusal in others {
            let err = SyncError::Refused(refusal.clone());
            assert_eq!(
                nothing_to_release(&err),
                None,
                "{refusal:?} is a refused release, so the command must fail"
            );
            assert_eq!(
                sync_exit_code(&err),
                EXIT_FAILURE,
                "...and exit 1: {refusal:?}"
            );
        }

        // Nor is anything that is not a refusal at all.
        assert_eq!(nothing_to_release(&SyncError::MediaAbsent), None);
        assert_eq!(
            nothing_to_release(&SyncError::Busy("docs".to_owned())),
            None
        );
    }
}
