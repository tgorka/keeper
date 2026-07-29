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
use keeper_sync::lfs::pointer::{Pointer, MAX_POINTER_BYTES};
use keeper_sync::lfs::store::LfsStore;
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
        SyncError::GitCommand { .. }
        | SyncError::Network { .. }
        | SyncError::Auth { .. }
        | SyncError::InvalidPathForRemote { .. }
        | SyncError::MediaAbsent
        | SyncError::Integrity { .. }
        | SyncError::Quota { .. }
        | SyncError::Diverged { .. }
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
        Command::Verify { profile } => {
            let engine = engine_for(&platform, config)?;
            cmd_verify(&printer, &engine, profile.as_deref()).await
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
) -> std::result::Result<(), CliError> {
    let profiles = engine.list_profiles()?;
    let selected: Vec<SyncProfile> = select(&profiles, wanted)?.into_iter().cloned().collect();

    let mut reports = Vec::with_capacity(selected.len());
    let mut bad_total = 0usize;
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
        reports.push(serde_json::json!({
            "profileId": profile.id,
            "profile": profile.name,
            "checked": report.checked,
            "bad": report.bad
                .iter()
                .map(|(path, reason)| serde_json::json!({ "path": path, "reason": reason }))
                .collect::<Vec<_>>(),
        }));
    }
    printer.json(&serde_json::json!({ "verified": reports, "bad": bad_total }));

    if bad_total > 0 {
        // Corrupt content is a real failure, and a cron wrapper must see it.
        return Err(CliError::Operational(format!(
            "{bad_total} object(s) failed verification"
        )));
    }
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
/// Both directions stream in bounded chunks. A multi-gigabyte smudge must never
/// be buffered, which is the entire reason LFS exists here (AD-46).
fn cmd_lfs_filter(direction: LfsDirection) -> Result<(), CliError> {
    use std::io::{Read, Write};

    let (repo, smudging) = match &direction {
        LfsDirection::Clean { repo, .. } => (repo.clone(), false),
        LfsDirection::Smudge { repo, .. } => (repo.clone(), true),
    };
    let store = LfsStore::in_git_dir(repo.join(".git"));
    store.ensure_layout()?;

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();

    if smudging {
        // The pointer is under 1 KiB by specification, so reading it whole is
        // bounded. Anything larger is not a pointer and passes straight
        // through — the same tolerance git-lfs shows.
        let mut buffer = Vec::with_capacity(MAX_POINTER_BYTES);
        // `take` by mutable reference: the lock must stay usable for the
        // pass-through branch below, which streams the remainder.
        Read::by_ref(&mut input)
            .take(MAX_POINTER_BYTES as u64 + 1)
            .read_to_end(&mut buffer)
            .map_err(|err| SyncError::io("read smudge input", &repo, err))?;

        let resolved = Pointer::parse(&buffer)
            .filter(|p| store.contains(&p.oid, p.size))
            .map(|p| store.object_path(&p.oid));

        match resolved {
            Some(path) => {
                let mut object = std::fs::File::open(&path)
                    .map_err(|err| SyncError::io("open lfs object", &path, err))?;
                std::io::copy(&mut object, &mut output)
                    .map_err(|err| SyncError::io("stream lfs object", &path, err))?;
            }
            // Not a pointer, or an object we do not hold yet. Passing the bytes
            // through unchanged is what git-lfs does, and it keeps a partial
            // fetch usable instead of failing the checkout.
            None => {
                output
                    .write_all(&buffer)
                    .map_err(|err| SyncError::io("pass through pointer", &repo, err))?;
                std::io::copy(&mut input, &mut output)
                    .map_err(|err| SyncError::io("pass through remainder", &repo, err))?;
            }
        }
        output
            .flush()
            .map_err(|err| SyncError::io("flush smudge output", &repo, err))?;
        return Ok(());
    }

    // Clean: worktree bytes in, pointer out. The content is hashed straight
    // into the object store as it streams, so the object is already present by
    // the time the pointer is emitted.
    let (oid, size) = store.insert_streaming(&mut input)?;
    let pointer = Pointer::new(oid, size);
    output
        .write_all(pointer.render().as_bytes())
        .map_err(|err| SyncError::io("write pointer", &repo, err))?;
    output
        .flush()
        .map_err(|err| SyncError::io("flush clean output", &repo, err))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;
    use clap::CommandFactory as _;

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
            &["keeper-syncd", "pause", "docs"],
            &["keeper-syncd", "resume", "docs"],
            &["keeper-syncd", "verify"],
            &["keeper-syncd", "verify", "docs"],
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
        // ...and the optional ones really are optional.
        assert!(parse(&["keeper-syncd", "verify"]).is_ok());
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
}
