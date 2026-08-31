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
//! `3` a prerequisite is missing · `4` the work did not run, and that is not a
//! failure. A wrapper script can branch on those: `2` and `3` will never
//! succeed on retry, `1` might, and `4` says there was nothing wrong and
//! nothing to alert about.
//!
//! `4` is reachable only from `tasks run` — see [`EXIT_DEFERRED`] for why
//! deferral needs a number of its own rather than being folded into `0` or `1`.

use std::collections::{BTreeSet, VecDeque};
use std::io::BufRead as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;

use keeper_sync::db::{TaskListing, TaskRow, TaskRunRow, UnitStanding, UnknownTask};
use keeper_sync::engine::{Engine, VerifyReport};
use keeper_sync::lfs::audit::RemoteAudit;
use keeper_sync::lfs::hydrate::{
    ContentRefusal, KeepFor, Materialization, MaterializeOutcome, Pin, Release,
};
use keeper_sync::lfs::listing::{LfsFile, LfsFileState};
use keeper_sync::profile::LfsMode;
use keeper_sync::progress::status_line;
use keeper_sync::provenance::SyncSource;
use keeper_sync::tasks::{TaskKind, TaskMissedPolicy, TaskMode, TaskOutcome, TaskRunDriver};
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
/// The task ran nothing, and that is not a failure (Story 57.3).
///
/// A cron wrapper needs **four** answers, not three: *worked* (`EXIT_OK`),
/// *did not run, do not alert* (this), *failed, alert* (`EXIT_FAILURE`) and
/// *fix your configuration* (`EXIT_CONFIG`). The three that existed could not
/// express the second, and both ways of folding it in are wrong in a direction
/// somebody pays for:
///
/// * Folding deferral into `EXIT_OK` makes an external drive that has been
///   unplugged for a month indistinguishable from a nightly sweep that is
///   working. Housekeeping that silently stopped is the invisible-failure shape
///   this epic exists to close.
/// * Folding it into `EXIT_FAILURE` pages somebody every night for AD-48's
///   *"an unplugged volume is absence, never failure"* — and an alert that
///   fires nightly for a normal condition is an alert nobody reads.
///
/// **Additive, so nothing that already works changes.** No pre-existing verb
/// can return it: [`sync_exit_code`] never yields `4`, and the only producer is
/// [`task_exit_code`] via `tasks run`. Story 57.7's `RestartPreventExitStatus`
/// will therefore list it beside `2` and `3` — the three numbers that mean "a
/// restart cannot help" — while `1` stays restartable.
pub const EXIT_DEFERRED: u8 = 4;

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
        // A one-shot run against a folder whose working copy was never made
        // did nothing it was asked to do, so `$?` has to say so — and
        // `Restart=on-failure` is exactly right here: the repair is mechanical
        // and the next run attempts it. Never `EXIT_CONFIG`: the configuration
        // is fine, the checkout is not.
        SyncError::CheckoutUnfinished { .. } => EXIT_FAILURE,
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
    /// Ask for one virtual path's content, optionally for a stated time
    /// (FR-338).
    ///
    /// Publishes the file inline when this machine already holds the object,
    /// and otherwise **fetches it before returning**: the verb performs its own
    /// transfer, because the person or the cron entry that ran it is who the
    /// bytes are for. Story 56.13 changed that — it used to queue the work and
    /// exit `0`, which read as success on a machine where nothing was draining
    /// the queue. The exit code follows the requested unit's own standing, so a
    /// wrapper can act on it. A path whose worktree bytes are neither the
    /// committed pointer nor the content it names is **refused by name** and
    /// left exactly as it is: keeper does not overwrite a local modification.
    ///
    /// With `--for`, this path is kept for the time you name and then let go on
    /// the first successful sync after it runs out — whatever the folder's own
    /// release interval says, in both directions.
    Materialize {
        /// Profile id or name. Required: materializing is a write, and "all
        /// folders" is not a thing anybody means by it.
        profile: String,
        /// The path inside the folder, as `ls-files` spells it.
        subpath: String,
        /// How long to keep this one path's content: a whole number of
        /// minutes, hours or days — `30m`, `2h`, `1d` — or `0` for no limit.
        ///
        /// It replaces this folder's own release interval for this path alone,
        /// in both directions: an hour inside a day-long window goes sooner,
        /// and a week stays longer. A pin still outranks it, and content this
        /// machine wrote that nothing has confirmed the server holds is still
        /// never released at any age. Omit it, or pass `0`, to leave the path
        /// on the folder's own interval.
        #[arg(long = "for", value_name = "DURATION", value_parser = parse_keep_for)]
        keep_for: Option<u64>,
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
    /// Keep one path's content on this machine, whatever the release sweep
    /// thinks (FR-334).
    ///
    /// A standing instruction about a path, not an operation on content: it
    /// writes one column and touches no file, so a path may be pinned before it
    /// has ever been materialized. It is the absolute floor `dehydrate` and the
    /// automatic release sweep both read — a pinned path is never released, at
    /// any age, by either.
    ///
    /// Refused for a path this folder does not track as an LFS path: a pin
    /// recorded against something no release will ever consider would be a
    /// promise nothing keeps.
    Pin {
        /// Profile id or name. Required: a pin is about one folder's path.
        profile: String,
        /// The path inside the folder, as `ls-files` spells it.
        subpath: String,
    },
    /// Withdraw a pin, making the path an ordinary release candidate again
    /// (FR-334).
    ///
    /// `pin`'s inverse and nothing more: it clears one column and leaves both
    /// release clocks exactly as they were, so the path is due whenever it
    /// would have been due had it never been pinned.
    Unpin {
        /// Profile id or name. Required: a pin is about one folder's path.
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
    /// Name, schedule, inspect and run keeper's own housekeeping tasks
    /// (FR-347, FR-349, AD-136).
    ///
    /// The door onto the task record for a machine with no app attached: a
    /// `cron` entry, a systemd timer or a person at a prompt all reach the same
    /// `keeper_sync::engine::Engine` verbs the desktop host's own tick reaches,
    /// so a requested run and a scheduled run leave the same row behind. There
    /// is no scheduler here — this process exits — and nothing on this
    /// subcommand creates a task, a schedule or a default: a box with no
    /// `tasks` rows behaves exactly as it did before they existed.
    Tasks {
        #[command(subcommand)]
        command: TaskCommand,
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

/// The seven things an operator does to a task.
///
/// Every doc comment below describes what the verb does **here**, in this
/// build. Story 56.13 shipped a `--help` for `materialize` that still described
/// the queue-and-exit behaviour it had stopped having, and it survived review
/// precisely because this prose sits on a clap enum a long way from the code it
/// claims to describe. So each of these was written against the function it
/// dispatches to, and the test `tasks_run_help_names_the_exit_codes_it_can_produce`
/// holds the one that a wrapper script cannot afford to have go stale.
#[derive(Debug, Subcommand)]
pub enum TaskCommand {
    /// List every stored task, with its target, schedule and last run.
    ///
    /// A row this build cannot read — a `kind`, `mode` or schedule written by a
    /// newer keeper on the other host — is listed as **unknown with its reason**
    /// rather than hidden (NFR-43). Hiding it would make a machine with one
    /// task nobody here can run look like a machine with no tasks, which is the
    /// difference between "nothing is scheduled" and "something is scheduled
    /// and this binary is too old to run it".
    List,
    /// Show one task and its recorded runs, newest first.
    ///
    /// Reads only. The history is capped at `keeper_sync::db::TASK_RUNS_CAP`
    /// runs by the store itself, which is what answers "has this been working
    /// lately" without letting `sync.db` grow because a schedule is doing its
    /// job.
    Status {
        /// The task's id, exactly as `tasks list` spells it.
        task: String,
    },
    // The exit codes below are one paragraph each, separated by blank lines,
    // rather than a markdown list: clap collapses a list into a single
    // paragraph, and the whole point of the block is that a person reading
    // `--help` can find the number they got in `$?`.
    /// Run one task now, and exit with what happened.
    ///
    /// The same engine path the scheduled tick takes: the same lease, the same
    /// row in `task_runs`, the same refusals. This verb does **not** consult the
    /// schedule — asking for a run now is asking for the work, not asking
    /// whether it is due — and a schedule window still in the future is left
    /// alone, so a run now is not a request to skip tonight's. The one
    /// exception, which `Engine::next_task_window` implements and this help used
    /// to omit: a window that was **already open** when this run finished
    /// has just been served, so it is re-armed to the following instant rather
    /// than fired again on the next tick.
    ///
    /// # Exit codes, which is what this verb is for
    ///
    /// `0` — the work ran and did what it was asked to.
    ///
    /// `4` — the work did NOT run, and nothing is wrong: its drive is
    /// unplugged, its folder is paused, the folder was already syncing, or the
    /// other host holds this task's lease. Recorded as `deferred` or `busy`. Do
    /// not alert on this.
    ///
    /// `1` — the work ran and failed, or a stored outcome this build cannot
    /// read was recorded. The reason is in the run's `detail`.
    ///
    /// `2` — the selector is wrong, or the task is off or disabled. Retrying
    /// changes nothing; somebody has to edit something.
    ///
    /// `3` — a prerequisite is missing, in practice `git` (AD-41). Raised by
    /// `Engine::open` before this verb reaches a task at all, so it says nothing
    /// about the task you named.
    Run {
        /// The task's id, exactly as `tasks list` spells it.
        task: String,
        /// Say that a **timer** is asking, not a person (Story 58.6).
        ///
        /// Set by the shipped `keeper-syncd-tasks@.service` unit and by nothing
        /// else. It exists because a box running both this timer and
        /// `keeper-syncd watch` against a `--mode scheduled` task otherwise gets
        /// TWO runs for one missed window: an ordinary request deliberately
        /// claims without asking whether a window is open, while the daemon's
        /// next tick claims the same past window independently.
        ///
        /// With this flag the run claims like the daemon's own due-gate **when
        /// the task is `--mode scheduled`**, so the two drivers race for one
        /// window and exactly one wins. On a `--mode manual` task — the
        /// timer-only arrangement, where this timer IS the schedule — it changes
        /// nothing and the run happens as always.
        ///
        /// The consequence a wrapper should know: on a `--mode scheduled` task
        /// whose window is not open, this exits 4 rather than doing the work.
        /// That is the point, and 4 already means "did not run, nothing wrong".
        #[arg(long)]
        timer: bool,
    },
    /// Create a task, or change one that exists.
    ///
    /// On **create** `--kind` is required: a task with no kind is a row nothing
    /// could ever run. On **update** every flag you omit keeps its stored value,
    /// and the two clearing flags are what make a binding and a schedule
    /// removable — a knob you can set but never unset is exactly the dead knob
    /// this epic exists to close.
    ///
    /// A stored task's **kind cannot be changed**, and passing a different one
    /// is refused: the armed window and the whole run history belong to the kind
    /// that made them, so a `sync` task turned into a `release` task would
    /// delete at an instant armed for a sync and show a sync's history as its
    /// own. Forget it and create it again.
    ///
    /// Never writes `enabled`: `tasks enable` and `tasks disable` own that
    /// column, so a settings-shaped read-modify-write here cannot silently
    /// un-pause a task somebody paused. Bringing a task back into service —
    /// enabling it, or putting it back on `--mode scheduled` — arms its schedule
    /// afresh rather than firing a window that fell into the past while it was
    /// out of service.
    Set(TaskSetArgs),
    /// Make a task live again, leaving its mode and schedule exactly as they are.
    ///
    /// `enabled` and `mode` answer different questions (AD-135): `mode` says who
    /// may trigger the task, `enabled` says whether the row is live at all. A
    /// disabled `scheduled` task keeps its schedule and resumes on it.
    Enable {
        /// The task's id, exactly as `tasks list` spells it.
        task: String,
    },
    /// Take a task out of service without forgetting anything about it.
    ///
    /// Neither the tick nor `tasks run` will touch it while it is disabled — an
    /// "off" that still runs when asked is not off — and its schedule, its
    /// binding and its whole run history survive, so re-enabling it is one word.
    Disable {
        /// The task's id, exactly as `tasks list` spells it.
        task: String,
    },
    /// Delete a task and everything it recorded.
    ///
    /// Its run history goes with it. This is the only destructive verb here, and
    /// it destroys bookkeeping rather than content: nothing a task ever did is
    /// undone by forgetting the task.
    Forget {
        /// The task's id, exactly as `tasks list` spells it.
        task: String,
    },
}

/// Everything `tasks set` can write, as one `Args` struct.
///
/// A struct rather than an inline variant, the shape [`AddArgs`] already has,
/// because a verb with seven knobs read by one function is a verb whose
/// arguments want a name — and because `conflicts_with` reads far better beside
/// the field it contradicts than in a `#[command(group)]` somewhere else.
#[derive(Debug, Args)]
pub struct TaskSetArgs {
    /// The task's id. Creates it when nothing is stored under that name.
    pub task: String,
    /// What the task does. Required to create one; kept as stored otherwise.
    #[arg(long, value_enum)]
    pub kind: Option<TaskKindArg>,
    /// Bind the task to one folder, by id or name.
    #[arg(long, value_name = "SEL")]
    pub profile: Option<String>,
    /// Unbind the task: it belongs to the machine, not to one folder.
    ///
    /// `--profile`'s inverse, and the reason a binding is not write-once.
    #[arg(long, conflicts_with = "profile")]
    pub host_wide: bool,
    /// When the task is due: `every 30m`, `every 2h`, or a 5-field cron
    /// expression like `0 3 * * *`.
    ///
    /// Refused at the write door when keeper cannot parse it, with the
    /// expression quoted — a schedule nobody can read is a schedule that would
    /// report itself enabled and never fire.
    #[arg(long, value_name = "EXPR")]
    pub schedule: Option<String>,
    /// Forget the schedule. `--schedule`'s inverse.
    ///
    /// Refused together with `--mode scheduled`, by the write door rather than
    /// by clap: a scheduled task with no schedule is the silent-no-op row
    /// `keeper_sync::db::upsert_task` exists to refuse.
    #[arg(long, conflicts_with = "schedule")]
    pub no_schedule: bool,
    /// Who may trigger the task: `off`, `manual` or `scheduled`.
    ///
    /// On create this defaults to `scheduled` when you gave a `--schedule` and
    /// `manual` when you did not. On update it keeps its stored value.
    #[arg(long, value_enum)]
    pub mode: Option<TaskModeArg>,
    /// What to do about a window that fell due while nobody was home:
    /// `run-now`, `delay` or `skip`.
    ///
    /// All three serve a window normally while a host is present; they differ
    /// only about a window nobody was here to serve, which keeper concludes
    /// after 15 minutes.
    ///
    /// `run-now` is the default and is what an ordinary restart already does —
    /// the stored past window fires on the first tick that sees it, **once**,
    /// however many windows went by. `delay` runs it 30 minutes after a host
    /// noticed it: the anchor is the **noticing**, not the window, because a
    /// host back two hours late is already past any instant derived from the
    /// window itself and would serve it immediately, which is `run-now`. `skip`
    /// abandons a window nobody served within those 15 minutes and arms the next
    /// one instead.
    ///
    /// A delay that lands past the next scheduled instant simply wins: the
    /// window is one stored instant rather than a queue, so the natural windows
    /// inside the delay are not run and no backlog accrues.
    ///
    /// On create this defaults to `run-now`, so a task created by an older
    /// script means exactly what it meant before. On update it keeps its stored
    /// value.
    #[arg(long, value_enum)]
    pub on_missed: Option<TaskMissedArg>,
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

/// `--kind`'s vocabulary, which is [`TaskKind`]'s and nothing more.
///
/// A separate enum for the same reason [`DirectionArg`] is one: clap's
/// [`ValueEnum`] derive belongs to the CLI, and putting it on the engine's own
/// type would let a `--help`-driven rename reach a stored `kind` column. The
/// `From` impl below is the only bridge, and it is exhaustive, so a kind added
/// to the engine is a compile error here rather than a flag value that silently
/// does not exist.
///
/// **There is no `update` here and there must never be one.** `TaskKind` has no
/// such variant by design — a schedule that replaces the keeper binary is what
/// the anti-timer stance forbids — and adding one to this enum could not
/// convert.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TaskKindArg {
    Sync,
    Release,
}

/// `--mode`'s vocabulary, which is [`TaskMode`]'s and nothing more.
///
/// Kebab-case by clap's own convention, which costs nothing here: all three
/// words are single ones, and they are the same three spellings the store uses,
/// so `tasks list` output and `--mode` input read identically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TaskModeArg {
    Off,
    Manual,
    Scheduled,
}

/// `--on-missed`'s vocabulary, which is [`TaskMissedPolicy`]'s and nothing more
/// (Story 58.4).
///
/// Kebab-case by clap's own convention, which is the one place this vocabulary
/// is spelled differently from the store: `run-now` on the command line,
/// `run_now` in the column and in both renderings. That is deliberate — a
/// clap flag with an underscore reads wrong beside `--host-wide` and
/// `--no-schedule` — and it is the only divergence, which is why the conversion
/// below is the single place it lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TaskMissedArg {
    RunNow,
    Delay,
    Skip,
}

impl From<TaskMissedArg> for TaskMissedPolicy {
    fn from(value: TaskMissedArg) -> Self {
        match value {
            TaskMissedArg::RunNow => Self::RunNow,
            TaskMissedArg::Delay => Self::Delay,
            TaskMissedArg::Skip => Self::Skip,
        }
    }
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

impl From<TaskKindArg> for TaskKind {
    fn from(value: TaskKindArg) -> Self {
        match value {
            TaskKindArg::Sync => Self::Sync,
            TaskKindArg::Release => Self::Release,
        }
    }
}

impl From<TaskModeArg> for TaskMode {
    fn from(value: TaskModeArg) -> Self {
        match value {
            TaskModeArg::Off => Self::Off,
            TaskModeArg::Manual => Self::Manual,
            TaskModeArg::Scheduled => Self::Scheduled,
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

/// Run one command, and answer with the exit code it earned.
///
/// `config` arrives as a `Result` rather than being loaded here because
/// `main` needs the configured log level before logging exists — and because
/// `init`, `doctor` and `logs` must all work on a box whose config is missing
/// or broken, which is exactly when an operator reaches for them.
///
/// # Why the code is part of the answer and not only of the failure
///
/// This used to return `Result<(), CliError>`, and `main` mapped `Ok` to
/// [`EXIT_OK`] — which was true of every verb until `tasks run`, because
/// `tasks run` can **succeed at doing nothing**: an unplugged drive is
/// [`EXIT_DEFERRED`], and it is not an error in any sense a `CliError` could
/// carry. Saying it through the error channel would have been wrong twice over:
/// a deferral is not a failure, so nothing would log it correctly; and `main`
/// prints a JSON failure envelope for every `Err`, which would have put a
/// *second* document on stdout over the one the verb had already emitted —
/// breaking `ls_files_document`'s own rule that one invocation produces exactly
/// one JSON document.
///
/// So every other arm ends in `.map(|()| EXIT_OK)`, which says the same thing
/// it always did, and the `Tasks` arm returns the number it measured.
pub async fn run(
    cli: Cli,
    platform: LinuxPlatform,
    config_path: PathBuf,
    config: SyncResult<DaemonConfig>,
) -> std::result::Result<u8, CliError> {
    let printer = Printer::new(cli.json);
    match cli.command {
        Command::Init { force } => {
            cmd_init(&printer, &config_path, &platform, force).map(|()| EXIT_OK)
        }
        Command::Logs { lines } => cmd_logs(&printer, &platform, lines).map(|()| EXIT_OK),
        // Runs inside a `git` invocation: it must write ONLY object bytes to
        // stdout, so it bypasses the printer entirely.
        Command::Update { check } => cmd_update(&printer, check).map(|()| EXIT_OK),
        Command::Lfs { direction } => cmd_lfs_filter(direction).map(|()| EXIT_OK),
        Command::Doctor => cmd_doctor(&printer, &platform, &config_path, config).map(|()| EXIT_OK),
        Command::Add(args) => {
            let engine = engine_for(&platform, config)?;
            cmd_add(&printer, &engine, &config_path, args).map(|()| EXIT_OK)
        }
        Command::List => {
            let engine = engine_for(&platform, config)?;
            cmd_list(&printer, &engine).map(|()| EXIT_OK)
        }
        Command::Status { profile } => {
            let engine = engine_for(&platform, config)?;
            cmd_status(&printer, &engine, profile.as_deref()).map(|()| EXIT_OK)
        }
        Command::Pause { profile } => {
            let engine = engine_for(&platform, config)?;
            cmd_set_enabled(&printer, &engine, &profile, false).map(|()| EXIT_OK)
        }
        Command::Resume { profile } => {
            let engine = engine_for(&platform, config)?;
            cmd_set_enabled(&printer, &engine, &profile, true).map(|()| EXIT_OK)
        }
        Command::Verify {
            profile,
            remote,
            repair,
        } => {
            let engine = engine_for(&platform, config)?;
            cmd_verify(&printer, &engine, profile.as_deref(), remote, repair)
                .await
                .map(|()| EXIT_OK)
        }
        Command::LsFiles { profile, remote } => {
            let engine = engine_for(&platform, config)?;
            cmd_ls_files(&printer, &engine, profile.as_deref(), remote)
                .await
                .map(|()| EXIT_OK)
        }
        Command::Materialize {
            profile,
            subpath,
            keep_for,
        } => {
            let engine = engine_for(&platform, config)?;
            cmd_materialize(&printer, &engine, &profile, &subpath, keep_for)
                .await
                .map(|()| EXIT_OK)
        }
        Command::Dehydrate { profile, subpath } => {
            let engine = engine_for(&platform, config)?;
            cmd_dehydrate(&printer, &engine, &profile, &subpath)
                .await
                .map(|()| EXIT_OK)
        }
        Command::Pin { profile, subpath } => {
            let engine = engine_for(&platform, config)?;
            cmd_pin(&printer, &engine, &profile, &subpath, true).map(|()| EXIT_OK)
        }
        Command::Unpin { profile, subpath } => {
            let engine = engine_for(&platform, config)?;
            cmd_pin(&printer, &engine, &profile, &subpath, false).map(|()| EXIT_OK)
        }
        Command::Sync { profile, once } => {
            let engine = engine_for(&platform, config)?;
            cmd_sync(&printer, engine, profile.as_deref(), once)
                .await
                .map(|()| EXIT_OK)
        }
        Command::Watch => {
            let engine = engine_for(&platform, config)?;
            run_supervisor(&printer, engine).await.map(|()| EXIT_OK)
        }
        Command::Tasks { command } => {
            let engine = engine_for(&platform, config)?;
            // One instant for the whole invocation, so every countdown a verb
            // renders is measured from the same "now" — and so the commands
            // below need nothing from the platform but this number.
            let now_ms = platform.now_ms();
            match command {
                TaskCommand::List => cmd_task_list(&printer, &engine, now_ms),
                TaskCommand::Status { task } => cmd_task_status(&printer, &engine, now_ms, &task),
                TaskCommand::Run { task, timer } => {
                    let driver = if timer {
                        TaskRunDriver::Timer
                    } else {
                        TaskRunDriver::Person
                    };
                    cmd_task_run(&printer, &engine, now_ms, &task, driver).await
                }
                TaskCommand::Set(args) => cmd_task_set(&printer, &engine, now_ms, &args),
                TaskCommand::Enable { task } => {
                    cmd_task_set_enabled(&printer, &engine, now_ms, &task, true)
                }
                TaskCommand::Disable { task } => {
                    cmd_task_set_enabled(&printer, &engine, now_ms, &task, false)
                }
                TaskCommand::Forget { task } => cmd_task_forget(&printer, &engine, &task),
            }
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
///
/// **An exact id match wins outright** (story 56.14). This used to be one
/// `filter` over `id == wanted || name == wanted` with no precedence, and the
/// consequence was not a cosmetic one: a selector that is one folder's id and a
/// different folder's name matched BOTH, so the single-profile destructure in
/// `cmd_materialize`, `cmd_dehydrate`, `cmd_pin` and `cmd_unpin` failed and every
/// one of those verbs refused — permanently, for both folders — with
/// "`{wanted}` matches 2 folders; name the one you mean by its id", advising the
/// one thing that could not work, because the id was what was ambiguous. Neither
/// folder could be materialized, released, pinned or unpinned by any spelling
/// until someone renamed one of them.
///
/// Asking the id first fixes that and makes the surviving message TRUE: ids are
/// the primary key of the profile row, so an id match is always exactly one
/// folder, and the only ambiguity left is two folders sharing a NAME — for which
/// "name the one you mean by its id" is the correct instruction.
///
/// The precedence is the safe direction as well as the unambiguous one. An id is
/// keeper's own opaque identifier, so a person typing one is naming a specific
/// row; a name is a label they chose and may reuse. Resolving to the id can
/// therefore never address a folder the caller did not name, whereas preferring
/// the name could.
fn select<'a>(
    profiles: &'a [SyncProfile],
    wanted: Option<&str>,
) -> std::result::Result<Vec<&'a SyncProfile>, CliError> {
    let Some(wanted) = wanted else {
        return Ok(profiles.iter().collect());
    };
    if let Some(by_id) = profiles.iter().find(|profile| profile.id == wanted) {
        return Ok(vec![by_id]);
    }
    let matched: Vec<&SyncProfile> = profiles
        .iter()
        .filter(|profile| profile.name == wanted)
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

/// One profile's verification as a human reads it (Story 25.6, Story 56.6).
///
/// **Pure, and returning lines rather than printing them**, for the reason
/// [`ls_files_lines`] is: [`Printer`] writes to process stdout, so an assertion
/// about what this verb *says* would otherwise need the binary spawned — and
/// the claim worth asserting is a claim about a string.
///
/// The `virtual` count is the whole anti-quietness argument in one word. Since
/// Story 56.6 `verify` excuses an authorized virtual path instead of calling it
/// a fault, and a row that is suppressed and reported *nowhere* is
/// indistinguishable from a check that stopped running. So the count is
/// rendered unconditionally, in both forms, beside the two numbers that were
/// always there.
///
/// `Err` is a folder whose verify could not run at all — a `.keepervirtual`
/// that will not compile (FR-329) is the ordinary way — and it is one folder's
/// line rather than the end of the verb. See [`cmd_verify`].
fn verify_lines(profile: &SyncProfile, report: Result<&VerifyReport, &str>) -> Vec<String> {
    let report = match report {
        Ok(report) => report,
        Err(error) => return vec![format!("{}: could not be checked: {error}", profile.name)],
    };
    let mut lines = vec![format!(
        "{}: {} checked, {} bad, {} virtual",
        profile.name,
        report.checked,
        report.bad.len(),
        report.virtual_paths
    )];
    lines.extend(
        report
            .bad
            .iter()
            .map(|(path, reason)| format!("  {path}: {reason}")),
    );
    lines
}

/// One profile's verification as the `--json` document carries it.
///
/// The key set is the contract, and `virtual` joins it for the reason
/// [`verify_lines`] renders the count: suppressed and nowhere reported is not a
/// state this document may describe. `profileId` and `profile` are the same two
/// keys [`ls_files_entry`] carries, so an operator with three folders reads
/// both documents the same way.
///
/// An `Err` entry carries `error` and **no** `bad` array. A consumer must be
/// able to tell "this folder was checked and nothing was wrong" from "this
/// folder was never checked", and an empty array cannot say the second.
fn verify_entry(profile: &SyncProfile, report: Result<&VerifyReport, &str>) -> serde_json::Value {
    let report = match report {
        Ok(report) => report,
        Err(error) => {
            return serde_json::json!({
                "profileId": profile.id,
                "profile": profile.name,
                "error": error,
            })
        }
    };
    serde_json::json!({
        "profileId": profile.id,
        "profile": profile.name,
        "checked": report.checked,
        "virtual": report.virtual_paths,
        "bad": report.bad
            .iter()
            .map(|(path, reason)| serde_json::json!({ "path": path, "reason": reason }))
            .collect::<Vec<_>>(),
    })
}

/// Check every selected folder, and let one folder's failure be one folder's.
///
/// A `?` on the per-profile `verify` used to discard every report already
/// computed, skip every folder still to come, and emit no `--json` document at
/// all — so a typo in one folder's `.keepervirtual` blinded the operator to the
/// other two. Contained instead: the failure is that folder's own entry, it
/// counts against the exit code, and the loop carries on.
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
    let mut refused_total = 0usize;
    for profile in &selected {
        let checked = match engine.verify(&profile.id).await {
            Ok(report) => report,
            Err(err) => {
                let reason = err.to_string();
                refused_total += 1;
                for line in verify_lines(profile, Err(reason.as_str())) {
                    printer.line(line);
                }
                reports.push(verify_entry(profile, Err(reason.as_str())));
                // The remote half of a folder whose local half never ran would
                // be an audit of an unknown, and `--repair` over it would be a
                // write decided from one.
                continue;
            }
        };
        bad_total += checked.bad.len();
        for line in verify_lines(profile, Ok(&checked)) {
            printer.line(line);
        }
        let mut entry = verify_entry(profile, Ok(&checked));

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
    if refused_total > 0 {
        // Last, so neither gate above is weakened or reworded, and non-zero
        // regardless: a check that could not run is not a check that passed.
        return Err(CliError::Operational(format!(
            "{refused_total} folder(s) could not be checked"
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

/// `--for`'s value: a whole number of minutes, hours or days, in milliseconds
/// (Story 56.17).
///
/// A clap `value_parser`, so a malformed duration is refused **before any
/// engine is opened, any profile is resolved and any pointer is looked at** —
/// the same discipline `SyncProfile::validate`'s `validate_quiet_time` applies
/// to a quiet window, and for the same reason: a duration nobody can parse is
/// a duration that would silently mean something else, and here the something
/// else is when keeper deletes a copy of somebody's file.
///
/// **Refused rather than coerced, with the input quoted.** `1w` is not a week
/// rounded to seven days, `1.5h` is not ninety minutes and `2` is not two of
/// anything: each is somebody expecting a unit this verb does not have, and
/// guessing which would be worse than saying so. The sentence names all four
/// accepted forms, because the reader is looking at it precisely because they
/// do not know them.
///
/// **`0` is accepted and means indefinite**, which is what the verb already
/// does with no flag at all — see [`keeper_sync::lfs::hydrate::KeepFor`] for
/// the one difference between saying it and not saying it.
///
/// Three units and no seconds: the shortest release window `SyncProfile`
/// accepts is a minute (`MIN_RELEASE_TTL_MS`) and the sweep looks at most once
/// an hour, so a duration in seconds would be a promise nothing keeps.
/// Overflow is refused by the same sentence rather than saturating, for
/// `validate`'s reason: a number that large is one somebody typed.
fn parse_keep_for(value: &str) -> std::result::Result<u64, String> {
    let malformed = || {
        format!(
            "--for must be a whole number of minutes, hours or days like 30m, 2h or 1d, \
             or 0 for no limit, got {value}"
        )
    };
    if value == "0" {
        return Ok(0);
    }
    let (digits, unit_ms) = match value.as_bytes().last() {
        Some(b'm') => (&value[..value.len() - 1], 60_000u64),
        Some(b'h') => (&value[..value.len() - 1], 60 * 60_000),
        Some(b'd') => (&value[..value.len() - 1], 24 * 60 * 60_000),
        _ => return Err(malformed()),
    };
    // `str::parse::<u64>` accepts a leading `+` and rejects a leading `-`, but
    // it would also accept `+2h`; the digit test is what keeps the accepted set
    // exactly the four forms the sentence names.
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(malformed());
    }
    let count: u64 = digits.parse().map_err(|_| malformed())?;
    if count == 0 {
        // `0m` is a duration of zero, which is not "no limit" — it is a person
        // asking for a window that has already closed. Refused rather than
        // silently promoted to indefinite, because those are opposite answers.
        return Err(malformed());
    }
    count.checked_mul(unit_ms).ok_or_else(malformed)
}

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
    // One line, and only one. The second line this used to carry promised a
    // supervisor the process does not have; now that the verb performs the
    // transfer itself, everything a `queued` outcome still needs to say depends
    // on where the journal row stands, and [`undelivered`] is the one place that
    // reads it — a half-sentence here would be a second, weaker copy.
    vec![format!(
        "{profile_name}: {outcome}  {size}  {path}",
        outcome = done.outcome,
        size = format_bytes(done.size_bytes),
        path = done.path,
    )]
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

/// The sentence a run that did **not** deliver has to end on, or `None` when it
/// did.
///
/// Pure, and separate from [`cmd_materialize`] for the reason
/// [`materialize_json`] gives about its own absent keys: the branch that
/// decides whether this invocation kept its promise is the whole contract of a
/// one-shot verb, and it belongs somewhere a test can call it rather than in an
/// `if` around an `await`.
///
/// # Four "not delivered"s, and only one of them is a fault
///
/// `standing` is where the journal row this run queued actually stands
/// ([`UnitStanding`]), and the four answers want four different sentences.
/// Collapsing them into "it is still owed; see the logs" was wrong in two
/// directions at once: it sent an operator to start a `keeper-syncd watch` for a
/// **parked** row no watcher will ever claim, and it reported a **healthy**
/// transfer another keeper process was performing as this run's failure — which
/// for a cron wrapper is an alert on a folder that is working.
///
/// A recorded reason is quoted as the **last recorded** failure rather than as
/// this run's, because `db::claim_ready` does not clear `last_error` and this
/// run may never have attempted the row: `claim_ready` keeps one slot back for
/// background work and can bump the youngest requested row out of a full batch.
fn undelivered(
    profile_name: &str,
    done: &Materialization,
    standing: &UnitStanding,
) -> Option<String> {
    if done.outcome != MaterializeOutcome::Queued {
        return None;
    }
    let unit = match done.unit_id {
        Some(unit) => format!("unit {unit}"),
        None => "the queued unit".to_owned(),
    };
    let because = |reason: &Option<String>| match reason {
        Some(reason) => format!("last recorded failure: {reason}"),
        None => "nothing recorded against it — see `keeper-syncd logs`".to_owned(),
    };
    let tail = match standing {
        UnitStanding::InFlight => {
            "another keeper process is transferring it right now; ask again once it finishes"
                .to_owned()
        }
        UnitStanding::Parked { reason } => format!(
            "keeper has given up on {unit} ({}); asking again queues a fresh attempt",
            because(reason)
        ),
        UnitStanding::Waiting { reason } => {
            format!("{unit} is still owed ({})", because(reason))
        }
        // The row is gone, which is what completing does, and yet the content
        // is not here: the object was fetched and something removed it again
        // before this pass looked — `lfs_prune_local` is the one thing that
        // deletes a store copy on purpose. Worth its own sentence, because
        // "asking again" really is the remedy and nothing is broken.
        UnitStanding::Settled => {
            format!("{unit} finished, but the content is not on disk; ask again to fetch it")
        }
    };
    Some(format!(
        "{profile_name}: {path} was not delivered — {tail}",
        path = done.path,
    ))
}

/// Ask for one path's content in one folder, and wait for it.
///
/// [`Engine::materialize_entry_now`] holds every decision — containment, the
/// mode, the index, the cone, whether the bytes on disk may be replaced,
/// whether the object is here or has to be fetched, and the bounded drain that
/// fetches it. This function selects the profile and prints; adding any policy
/// here would be the second implementation AD-52 exists to prevent.
///
/// # Why this door waits and the app's does not
///
/// `Engine::materialize_entry` — the queueing verb — is right for the app,
/// whose engine *is* the supervisor that will drain the row. It is wrong here:
/// this process exits, so a queued row it reported as a success would be
/// delivered by nobody. That is the defect an end-to-end run of this binary
/// found, and `sync --once` is documented as the cron entry point for exactly
/// the same kind of caller.
///
/// # A run that did not deliver exits non-zero, and prints one document
///
/// The `--json` form is emitted only on the paths that delivered, so stdout
/// carries exactly one JSON document per invocation: the materialization on
/// success, or `main`'s failure envelope — with the reason and the exit code in
/// it — when the bytes are not here. Two documents would break the consumer
/// this flag exists for.
///
/// A required profile, resolved through [`select`] so a mistyped name is
/// refused with the alternatives named rather than silently addressing a
/// different folder. `select` returns a list because its other callers accept
/// `None` for "every profile"; this verb writes to one worktree, so it takes
/// the one match and prints its document exactly once.
async fn cmd_materialize(
    printer: &Printer,
    engine: &Engine,
    wanted: &str,
    subpath: &str,
    keep_for_ms: Option<u64>,
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
        .materialize_entry_now(
            &profile.id,
            subpath,
            SyncSource::Cli,
            KeepFor::from_ms(keep_for_ms),
        )
        .await
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
    // Before the document, so a run that delivered nothing prints no
    // materialization at all — see this function's doc.
    let standing = done
        .unit_id
        .map(|unit| engine.unit_standing(unit))
        // No unit means nothing was ever queued, so no outcome that reaches
        // here can be `Queued` and `undelivered` will not read this.
        .unwrap_or(UnitStanding::Waiting { reason: None });
    if let Some(failure) = undelivered(&profile.name, &done, &standing) {
        return Err(CliError::Operational(failure));
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
// pin / unpin
// ---------------------------------------------------------------------------

/// One pin instruction as a human reads it.
///
/// Pure, taking the profile's *name*, for [`dehydrate_line`]'s reason. Prose
/// rather than the wire word: `pinned: true` in a sentence a person reads is a
/// field that escaped from a JSON document.
fn pin_line(profile_name: &str, done: &Pin) -> String {
    format!(
        "{profile_name}: {state}  {path}",
        state = if done.pinned { "pinned" } else { "unpinned" },
        path = done.path,
    )
}

/// One pin instruction as the `--json` document carries it.
///
/// The key set is the contract: `profileId`, `profile`, `path`, `pinned` — and
/// nothing else. No `oid` and no `sizeBytes`, because a pin says nothing about
/// content and may be recorded for a path whose bytes are not here; a size of
/// `0` or a null oid would both be answers to a question nobody asked.
/// Built by naming the keys rather than by serializing [`Pin`], for
/// [`dehydrate_json`]'s reason.
fn pin_json(profile: &SyncProfile, done: &Pin) -> serde_json::Value {
    serde_json::json!({
        "profileId": profile.id,
        "profile": profile.name,
        "path": done.path,
        "pinned": done.pinned,
    })
}

/// Set or clear one path's pin in one folder.
///
/// [`Engine::pin_entry`] holds every decision — containment, whether this
/// folder tracks the path at all, and the write. This function selects the
/// profile and prints, exactly as [`cmd_dehydrate`] does.
///
/// A required profile resolved through [`select`], and a two-folder name match
/// refused by asking for the id: [`cmd_materialize`]'s rule.
///
/// Synchronous, unlike [`cmd_dehydrate`], because a pin takes no reservation
/// and makes no round trip — there is nothing here to await, and so no `Busy`
/// to remap either.
fn cmd_pin(
    printer: &Printer,
    engine: &Engine,
    wanted: &str,
    subpath: &str,
    pinned: bool,
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

    let done = engine.pin_entry(&profile.id, subpath, pinned)?;
    printer.line(pin_line(&profile.name, &done));
    printer.json(&pin_json(profile, &done));
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

// ---------------------------------------------------------------------------
// tasks
// ---------------------------------------------------------------------------

/// Map one recorded outcome onto the number a wrapper script branches on
/// (Story 57.3).
///
/// **Exhaustive with no `_` arm**, the discipline [`sync_exit_code`] already
/// carries and for a sharper reason: a wildcard here would give a
/// [`TaskOutcome`] added later a number silently, and the two numbers it could
/// silently get are "everything is fine" and "wake somebody up". A compile
/// error is the only acceptable way to learn that a new outcome exists.
///
/// # Why `Busy` defers here while `SyncError::Busy` succeeds elsewhere
///
/// [`sync_exit_code`] maps [`SyncError::Busy`] to [`EXIT_OK`] because a
/// `sync --once` that lost a race did nothing wrong and must not earn a
/// `Restart=on-failure`. This verb answers a different question — *did the work
/// happen* — and `Engine::next_task_window` has already settled it: it groups
/// [`TaskOutcome::Busy`] with [`TaskOutcome::Deferred`] as "the run did not
/// happen, do not consume the window". The exit code says the same thing, so
/// every "did not run" answer this verb can give is **one number** — including
/// the `Err(SyncError::Busy)` that [`Engine::run_task_now`] raises for a lease
/// held by the other host, which [`cmd_task_run`] remaps to
/// [`EXIT_DEFERRED`] rather than letting it fall through to `EXIT_OK`.
///
/// [`TaskOutcome::Abandoned`] cannot arise from this verb's own run: the row
/// [`cmd_task_run`] reports is the one it just closed, and an abandoned run is
/// written *by the next host* when it reclaims an expired lease. It is mapped
/// anyway because `tasks status` reports history, the map has to be total, and
/// a run nobody closed is a run that failed to finish — which is a failure.
pub fn task_exit_code(outcome: TaskOutcome) -> u8 {
    match outcome {
        // The work ran and did what it was asked to.
        TaskOutcome::Ok => EXIT_OK,
        // The target was already in use, which is NFR-42's one-operation-per-
        // folder rule working rather than something to alert about.
        TaskOutcome::Busy => EXIT_DEFERRED,
        // A condition the work waits on was not met — an unplugged drive above
        // all, which AD-48 settles by name: absence, never failure.
        TaskOutcome::Deferred => EXIT_DEFERRED,
        // There was no run at all, and nothing is wrong: the task's
        // missed-window policy declined this window and armed the next one
        // (Story 58.5). Beside the two above because all three are the same
        // answer to the only question this verb asks — *was the work done* — and
        // giving them three numbers would make every caller learn the taxonomy.
        //
        // It cannot arise from this verb's own run: a request never passes
        // through the due-gate, so no policy declines it. Mapped anyway because
        // `tasks status` reports history and this map has to be total.
        TaskOutcome::Declined => EXIT_DEFERRED,
        // Held back rather than dropped, and still "the work did not happen" to
        // the only caller that reads this number (Story 58.5).
        TaskOutcome::Postponed => EXIT_DEFERRED,
        // It ran and failed. `detail` carries the reason.
        TaskOutcome::Failed => EXIT_FAILURE,
        // Nobody closed it, so nothing can claim it succeeded.
        TaskOutcome::Abandoned => EXIT_FAILURE,
    }
}

/// One recorded run's exit code, including the two cases that have no
/// [`TaskOutcome`] at all.
///
/// Both of them are [`EXIT_FAILURE`], and the direction matters. A run whose
/// outcome was spelled by a **newer keeper** (`unknown_outcome`) is a run that
/// happened and whose result this build cannot classify — reading it as
/// [`EXIT_OK`] would report an unknown result as a working nightly sweep, which
/// is the invisible-failure shape this epic exists to close. A run with neither
/// column set is still in flight, which [`Engine::run_task_now`] cannot return
/// (it closes the run it opened) but which the map must still cover rather than
/// leaving a `_` arm to decide.
fn run_exit_code(run: &TaskRunRow) -> u8 {
    match run.outcome {
        Some(outcome) => task_exit_code(outcome),
        None => EXIT_FAILURE,
    }
}

/// The one error from [`Engine::run_task_now`] that means "did not run" rather
/// than "went wrong", and the task it is about.
///
/// A live lease held by the other host arrives as [`SyncError::Busy`]: nothing
/// failed, and nothing happened either. Left to [`sync_exit_code`] it would
/// become [`EXIT_OK`] — correct about the absence of a fault and useless to a
/// caller that needs to know whether the work was done.
///
/// A pure function with its own test, for exactly [`nothing_to_release`]'s
/// reason: "this refusal really is distinguishable from a failure" is the claim
/// being made, and it has to be distinguishable **by type**. Matching on the
/// message would have been the other reading, and it is the one that stops
/// working the first time a sentence is reworded. It also puts the branch
/// somewhere a test can reach, which an `await`'s error arm is not: manufacturing
/// a lease held by another process inside a unit test would mean racing two
/// engines and asserting on which one won.
fn lease_held_elsewhere(err: &SyncError) -> Option<&str> {
    match err {
        SyncError::Busy(task) => Some(task),
        _ => None,
    }
}

/// The word a run's outcome is called, in both renderings.
///
/// One function so the human line and the `--json` document cannot come to call
/// one run two different things — the rule [`ls_files_lines`] follows for
/// `LfsFileState`. A spelling this build cannot read is passed through **as
/// stored** rather than replaced by a placeholder: it is what the other host
/// actually recorded, and it is the only clue anybody has about what ran.
fn run_outcome_word(run: &TaskRunRow) -> &str {
    match (run.outcome, run.unknown_outcome.as_deref()) {
        (Some(outcome), _) => outcome.as_str(),
        (None, Some(stored)) => stored,
        // Neither column set: opened and not yet closed.
        (None, None) => "running",
    }
}

/// The refusal for an id that names a row this build cannot read, or `None`.
///
/// Factored out because two callers need the identical sentence and the
/// identical precedence: [`select_task`], where it is the third of three
/// distinguishable answers, and [`cmd_task_set`], which must reach it *before*
/// its own "a new task must name its kind" — otherwise somebody looking at a
/// newer keeper's row is told to supply a `--kind`, which is the one instruction
/// that cannot work. `db::upsert_task` refuses the write as well (the write half
/// of NFR-43); this is what makes the CLI's advice right rather than merely
/// making the write safe.
fn unreadable_task(listing: &TaskListing, wanted: &str) -> Option<CliError> {
    listing
        .unknown
        .iter()
        .find(|row| row.id == wanted)
        .map(|row| {
            CliError::from(SyncError::Config(format!(
                "task `{}` is stored, but this keeper cannot read it: {}",
                row.id, row.reason
            )))
        })
}

/// Resolve a user-typed task selector to exactly one stored task.
///
/// **Three refusals, and they are three different sentences on purpose.** All
/// are [`SyncError::Config`] and therefore [`EXIT_CONFIG`], which is what keeps
/// a selector mistake distinct from an operational one: exit 2 says "retrying
/// changes nothing, edit something", and a cron wrapper that got 2 must not
/// treat it the way it treats 1.
///
/// 1. **Malformed** — a spelling [`keeper_sync::db::upsert_task`] could never
///    have stored, so no list of known ids would help; the input itself is the
///    problem. The rule is **not duplicated here**:
///    [`keeper_sync::tasks::validate_id`] is the engine's own function, called
///    by the write door as well, so a spelling this refuses is a spelling that
///    could never have been stored. A second copy in the CLI would drift, and it
///    would drift silently in the direction of accepting an id the write door
///    refuses.
/// 2. **Unknown** — well formed, but no such row, so the alternatives are
///    exactly what helps. [`select`]'s own shape, down to listing what is known
///    and saying so when there is nothing to list: "known tasks are: " with
///    nothing after it reads as a rendering bug rather than as an empty store.
/// 3. **Stored, but unreadable** — see [`unreadable_task`]. Reported with *that*
///    reason rather than as "no task matches", because an unreadable row
///    reported as absent is a row somebody will recreate on top of a newer
///    keeper's task.
///
/// Unlike [`select`] this returns exactly one row and never a list: `id` is the
/// primary key of the `tasks` table, so a match is one row by construction and
/// there is no name to be ambiguous about.
fn select_task<'a>(
    listing: &'a TaskListing,
    wanted: &str,
) -> std::result::Result<&'a TaskRow, CliError> {
    keeper_sync::tasks::validate_id(wanted)?;
    if let Some(task) = listing.tasks.iter().find(|task| task.id == wanted) {
        return Ok(task);
    }
    if let Some(refusal) = unreadable_task(listing, wanted) {
        return Err(refusal);
    }
    let known = listing
        .tasks
        .iter()
        .map(|task| task.id.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    Err(SyncError::Config(if known.is_empty() {
        format!("no task matches `{wanted}`; no tasks are stored on this host")
    } else {
        format!("no task matches `{wanted}`; known tasks are: {known}")
    })
    .into())
}

/// The name of the folder a task is bound to, when this engine has it.
///
/// `None` covers two facts the caller keeps apart: a **host-wide** task, which
/// has no `profile_id` at all, and a task bound to a folder this engine does not
/// know. [`task_json`] renders the first as `profileId: null, profile: null` and
/// the second as `profileId: "01…", profile: null`, which is what makes them
/// distinguishable on the wire.
fn task_profile_name<'a>(profiles: &'a [SyncProfile], task: &TaskRow) -> Option<&'a str> {
    let bound = task.profile_id.as_deref()?;
    profiles
        .iter()
        .find(|profile| profile.id == bound)
        .map(|profile| profile.name.as_str())
}

/// One minute, in milliseconds — the resolution below which a countdown is
/// noise.
const RELATIVE_MINUTE_MS: i64 = 60_000;
/// One hour, in milliseconds.
const RELATIVE_HOUR_MS: i64 = 60 * RELATIVE_MINUTE_MS;
/// One day, in milliseconds, and the coarsest unit this renders.
const RELATIVE_DAY_MS: i64 = 24 * RELATIVE_HOUR_MS;

/// `in 4h`, `12m ago`, or `now` — one coarse unit, never two.
///
/// **Pure integer arithmetic, and the only date rendering in this crate.** There
/// is no formatter here and no `chrono` dependency, and this deliberately does
/// not become an excuse to add one: the wire carries the **absolute instant**
/// (`nextDueMs`, `startedMs`, `finishedMs`, `leaseUntilMs`) and the countdown is
/// subtracted where a clock ticks. That is the rule `keeper-sync`'s own
/// `ReleaseSchedule` records for the same reason — a rendered duration is
/// already stale by the time a consumer reads it, while an instant never is, so
/// a UI that wants to keep counting can and one that does not need not.
///
/// One unit and no seconds, because the finest schedule the write door accepts
/// is a minute (`MIN_SCHEDULE_INTERVAL_MS`) and the coarsest is a year: `in
/// 4h 12m` would be precision about a fire time nothing promises to hit to the
/// minute, and weeks would be a unit no accepted schedule can produce.
///
/// Saturating rather than wrapping, and `unsigned_abs` rather than `abs`,
/// because `i64::MIN - i64::MAX` is exactly the arithmetic a clock that jumped
/// produces and a panic in a rendering function is never the right answer.
fn relative_ms(now_ms: i64, at_ms: i64) -> String {
    let delta = at_ms.saturating_sub(now_ms);
    let magnitude = i64::try_from(delta.unsigned_abs()).unwrap_or(i64::MAX);
    let (count, unit) = if magnitude < RELATIVE_MINUTE_MS {
        // Under a minute in either direction. `now` rather than `in 0m`, which
        // reads as a rounding error instead of as an answer.
        return "now".to_owned();
    } else if magnitude < RELATIVE_HOUR_MS {
        (magnitude / RELATIVE_MINUTE_MS, 'm')
    } else if magnitude < RELATIVE_DAY_MS {
        (magnitude / RELATIVE_HOUR_MS, 'h')
    } else {
        (magnitude / RELATIVE_DAY_MS, 'd')
    };
    if delta.is_negative() {
        format!("{count}{unit} ago")
    } else {
        format!("in {count}{unit}")
    }
}

/// One task plus the two things a rendering needs beside it.
///
/// Three borrows rather than three parallel slices, because the parallel-slice
/// version is the shape where a `zip` in the wrong order silently reports one
/// task's history against another's row. The profile name and the last run are
/// carried rather than looked up because both renderers are **pure**: neither
/// [`task_lines`] nor [`task_json`] may reach an engine, which is what lets a
/// test that owns no database assert what this verb says — the discipline
/// [`ls_files_entry`] and [`materialize_json`] already state.
struct TaskView<'a> {
    task: &'a TaskRow,
    /// The bound folder's name, or `None` — see [`task_profile_name`].
    profile_name: Option<&'a str>,
    /// The newest recorded run, or `None` for a task that has never run.
    last: Option<&'a TaskRunRow>,
}

/// A listing of tasks as a human reads it, unreadable rows included.
///
/// **Pure, and returning lines rather than printing them**, for
/// [`ls_files_lines`]'s reason: [`Printer`] writes to process stdout, so an
/// assertion about what this verb *says* would otherwise need the binary
/// spawned.
///
/// Two claims are worth the length. **An empty store says so**, rather than
/// printing nothing — a verb that prints nothing is indistinguishable from a
/// verb that did not run. And **every unreadable row is a line** (NFR-43): a
/// machine with one task this build cannot run must not read as a machine with
/// no tasks, because those two states want opposite actions from the person
/// looking.
///
/// The words are the store's own — [`TaskKind::as_str`], [`TaskMode::as_str`],
/// [`run_outcome_word`] — so the human form and the `--json` document cannot
/// come to disagree about what one row is called.
fn task_lines(now_ms: i64, views: &[TaskView<'_>], unknown: &[UnknownTask]) -> Vec<String> {
    if views.is_empty() && unknown.is_empty() {
        return vec![
            "No tasks. Create one with `keeper-syncd tasks set <id> --kind sync`.".to_owned(),
        ];
    }
    let mut lines = Vec::with_capacity(views.len() * 2 + unknown.len());
    for view in views {
        let task = view.task;
        let target = match (task.profile_id.as_deref(), view.profile_name) {
            (None, _) => "host-wide".to_owned(),
            (Some(_), Some(name)) => name.to_owned(),
            // Bound to a folder this engine does not have. Named rather than
            // rendered as host-wide, which is what dropping the id would make
            // it look like — and "this task belongs to the machine" is not the
            // same fact as "this task points at a folder somebody removed".
            (Some(id), None) => format!("[no such folder: {id}]"),
        };
        lines.push(format!(
            "{id}  {kind}  {mode}  {target}  {schedule}  {due}{on_missed}{disabled}{running}",
            id = task.id,
            kind = task.kind.as_str(),
            mode = task.mode.as_str(),
            schedule = task.schedule.as_deref().unwrap_or("no schedule"),
            due = match task.next_due_ms {
                Some(at) => format!("due {}", relative_ms(now_ms, at)),
                // `Action::Arm` on first sight is why this is a state and not a
                // fault: a task created at noon arms tonight rather than firing
                // at noon.
                None => "never armed".to_owned(),
            },
            // A marker when it is not the default, exactly as `[disabled]`
            // below: `run_now` is what every row on every install already did,
            // so its absence is the honest rendering of it and a column here
            // would say the same thing about every row on most machines. The
            // `--json` document carries it unconditionally, because a fixed key
            // set is that document's contract.
            on_missed = match task.on_missed {
                TaskMissedPolicy::RunNow => String::new(),
                other => format!("  [on missed: {}]", other.as_str()),
            },
            disabled = if task.enabled { "" } else { "  [disabled]" },
            running = match &task.running_host {
                Some(host) => format!("  [running on {host}]"),
                None => String::new(),
            },
        ));
        lines.push(match view.last {
            Some(run) => format!(
                "  last: {outcome}  {when}{detail}",
                outcome = run_outcome_word(run),
                when = relative_ms(now_ms, run.started_ms),
                detail = run
                    .detail
                    .as_deref()
                    .map(|detail| format!("  {detail}"))
                    .unwrap_or_default(),
            ),
            // Said rather than left blank: an empty column reads as a missing
            // value, and "this has never run" is the answer somebody wants when
            // they are asking why nothing has happened.
            None => "  last: never run".to_owned(),
        });
    }
    lines.extend(unknown.iter().map(|row| {
        // `db::list_tasks` reports a row whose primary key itself will not read
        // with an **empty** id, which would print as a line starting with a bare
        // colon and tell the operator a row exists while naming nothing they
        // could type. There is genuinely no id to give them — `validate_id`
        // refuses `""` before any lookup, so `tasks forget` cannot reach it
        // either — and saying that is better than implying an id was omitted.
        let id = if row.id.is_empty() {
            "<a row whose id will not read>"
        } else {
            row.id.as_str()
        };
        format!(
            "{id}: stored, but this keeper cannot read it — {}",
            row.reason
        )
    }));
    lines
}

/// One task's run history as a human reads it.
///
/// Pure, taking the id rather than the row, for [`materialize_lines`]'s reason.
/// An empty history is a **line**, not a silence: a task that has never run and
/// a task whose history this verb failed to read must not look the same, and
/// only one of them is normal.
fn task_run_lines(now_ms: i64, task_id: &str, runs: &[TaskRunRow]) -> Vec<String> {
    if runs.is_empty() {
        return vec![format!("{task_id}: no runs recorded")];
    }
    let mut lines = Vec::with_capacity(runs.len() + 1);
    lines.push(format!("{task_id}: {} run(s), newest first", runs.len()));
    lines.extend(runs.iter().map(|run| {
        format!(
            "  {outcome:<10}  {when}  {host}{detail}",
            outcome = run_outcome_word(run),
            when = relative_ms(now_ms, run.started_ms),
            host = run.host,
            detail = run
                .detail
                .as_deref()
                .map(|detail| format!("  {detail}"))
                .unwrap_or_default(),
        )
    }));
    lines
}

/// One task as the `--json` document carries it.
///
/// The key set is the contract, and it is **fixed**: all twelve keys are always
/// present. Wave 3's Tasks view reads exactly this document, so a key that
/// appears only sometimes would be a key every consumer has to guard —
/// `onMissed` therefore appears on every row, including the `run_now` ones the
/// human rendering leaves unmarked.
///
/// camelCase, matching `sizeBytes` and `profileId` across `ls-files`, `verify`,
/// `materialize` and `dehydrate`, so an operator reads every document the same
/// way. Built by naming the keys rather than by serializing [`TaskRow`], for
/// [`materialize_json`]'s reason and one of its own: the stored row carries the
/// lease columns under their SQL names and has no idea what its folder is
/// called.
///
/// # Why `profileId` and `profile` are null rather than absent
///
/// This is the case where the absent-versus-null rule comes out the *other*
/// way. Elsewhere — [`ls_files_entry`]'s `remote`, [`materialize_json`]'s
/// `unitId` — absence means "nobody asked" or "there is nothing here", which
/// `null` cannot say. Here `null` is a **real value**: a host-wide task belongs
/// to the machine rather than to a folder, and that is an answer, not a missing
/// one. Making the keys absent would force every consumer to distinguish
/// host-wide from a truncated document.
///
/// A binding whose folder is gone renders `profileId` **set** and `profile`
/// null, which is how the two are told apart — see [`task_profile_name`].
///
/// `lastRun` is the newest run or `null`, and it is nested rather than flattened
/// so `tasks list` and `tasks status` describe a run with one shape; see
/// [`task_run_json`], which is the same function both reach.
fn task_json(
    task: &TaskRow,
    profile_name: Option<&str>,
    last: Option<&TaskRunRow>,
) -> serde_json::Value {
    serde_json::json!({
        "id": task.id,
        "kind": task.kind.as_str(),
        "mode": task.mode.as_str(),
        "enabled": task.enabled,
        "profileId": task.profile_id,
        "profile": profile_name,
        "schedule": task.schedule,
        "nextDueMs": task.next_due_ms,
        "runningHost": task.running_host,
        "leaseUntilMs": task.lease_until_ms,
        "onMissed": task.on_missed.as_str(),
        "lastRun": last.map(task_run_json),
    })
}

/// One recorded run as the `--json` document carries it.
///
/// Seven keys always, plus `unknownOutcome` **only** when the store held a
/// spelling this build cannot read — and that conditional key is the whole
/// contract of this document. A consumer has to be able to tell:
///
/// * **still in flight** — `outcome: null`, no `unknownOutcome`;
/// * **a newer keeper recorded something we cannot read** — `outcome: null`
///   *plus* `unknownOutcome: "…"`.
///
/// Both have a null `outcome`, so `null` alone cannot separate them; the
/// presence of the second key is the only signal, which is exactly why it is
/// absent rather than null in the first case. The branch that decides absence
/// lives here, in a function a test can call, rather than in an `if` around a
/// print — [`materialize_json`]'s rule.
///
/// `startedMs` and `finishedMs` are absolute instants for [`relative_ms`]'s
/// reason: a countdown is stale the moment it is serialized.
fn task_run_json(run: &TaskRunRow) -> serde_json::Value {
    let mut document = serde_json::json!({
        "id": run.id,
        "taskId": run.task_id,
        "startedMs": run.started_ms,
        "finishedMs": run.finished_ms,
        "outcome": run.outcome.map(TaskOutcome::as_str),
        "detail": run.detail,
        "host": run.host,
    });
    if let Some(stored) = &run.unknown_outcome {
        document["unknownOutcome"] = serde_json::json!(stored);
    }
    document
}

/// `tasks list`'s whole document, envelope included.
///
/// Built in a named function a test can call, for the reason
/// [`ls_files_document`] states about its own: the envelope is as much of the
/// contract as the rows are — renaming `tasks` or `unknown` breaks every
/// consumer — and inline in [`cmd_task_list`] nothing could assert it.
///
/// `unknown` is always present, empty array included. An absent key would make
/// "this build can read everything stored" indistinguishable from an older
/// consumer's document, and the array is the one place NFR-43's forward
/// compatibility is visible to a script.
fn tasks_list_document(
    tasks: Vec<serde_json::Value>,
    unknown: &[UnknownTask],
) -> serde_json::Value {
    serde_json::json!({
        "tasks": tasks,
        "unknown": unknown
            .iter()
            .map(|row| serde_json::json!({ "id": row.id, "reason": row.reason }))
            .collect::<Vec<_>>(),
    })
}

/// `tasks status`'s whole document: the task, and its runs newest first.
///
/// A named function for [`ls_files_document`]'s reason. The task is nested under
/// `task` rather than spliced into the top level so `runs` can never collide
/// with a key [`task_json`] gains later.
fn task_status_document(
    task: serde_json::Value,
    runs: Vec<serde_json::Value>,
) -> serde_json::Value {
    serde_json::json!({ "task": task, "runs": runs })
}

/// `tasks run`'s whole document: what was asked, what happened, and `$?`.
///
/// `exit` is in the document as well as in the process status because a caller
/// that captured stdout should not have to consult two channels to learn one
/// fact — the shape `main`'s own failure envelope already has.
///
/// `run` is `Option` and **null** when a lease held elsewhere meant no run of
/// ours was ever opened. Null rather than absent, and rather than a second
/// document shape: "there is no row" is a real answer, and one verb emitting two
/// key sets is what makes a `--json` consumer branch before it can parse.
fn task_run_document(
    task_id: &str,
    run: Option<serde_json::Value>,
    outcome: &str,
    exit: u8,
) -> serde_json::Value {
    serde_json::json!({ "task": task_id, "run": run, "outcome": outcome, "exit": exit })
}

/// The document `tasks set`, `tasks enable` and `tasks disable` all emit.
///
/// One envelope for three verbs because all three answer the same question —
/// *what does the row look like now* — and it is the row **read back from the
/// store**, never the row that was submitted; see [`report_task`].
fn task_saved_document(task: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "task": task })
}

/// `tasks forget`'s document.
///
/// The id and nothing else: the row is gone, so there is no row to describe, and
/// echoing the fields of something that no longer exists would invite a consumer
/// to believe it still does.
fn task_forgotten_document(id: &str) -> serde_json::Value {
    serde_json::json!({ "forgot": id })
}

/// Every stored task, with its target, schedule and last run.
///
/// The clock arrives as a **value**, not as a platform, and every function in
/// this section takes it that way. One instant is sampled per invocation in
/// [`run`], so a three-task listing renders every countdown against the same
/// "now" rather than against three of them; and a command that needs nothing
/// from a `SyncPlatform` but a timestamp does not have to be handed one to be
/// callable from a test.
fn cmd_task_list(
    printer: &Printer,
    engine: &Engine,
    now_ms: i64,
) -> std::result::Result<u8, CliError> {
    let listing = engine.tasks()?;
    let profiles = engine.list_profiles()?;
    // One row per task, not the whole capped history: a listing answers "is this
    // working", and fifty rows per task is the question `tasks status` asks.
    let mut newest = Vec::with_capacity(listing.tasks.len());
    for task in &listing.tasks {
        newest.push(engine.task_history(&task.id, 1)?.into_iter().next());
    }
    let views: Vec<TaskView<'_>> = listing
        .tasks
        .iter()
        .zip(newest.iter())
        .map(|(task, last)| TaskView {
            task,
            profile_name: task_profile_name(&profiles, task),
            last: last.as_ref(),
        })
        .collect();

    for line in task_lines(now_ms, &views, &listing.unknown) {
        printer.line(line);
    }
    let documents = views
        .iter()
        .map(|view| task_json(view.task, view.profile_name, view.last))
        .collect();
    printer.json(&tasks_list_document(documents, &listing.unknown));
    Ok(EXIT_OK)
}

/// One task and everything it has recorded.
fn cmd_task_status(
    printer: &Printer,
    engine: &Engine,
    now_ms: i64,
    wanted: &str,
) -> std::result::Result<u8, CliError> {
    let listing = engine.tasks()?;
    let task = select_task(&listing, wanted)?;
    let profiles = engine.list_profiles()?;
    // The store caps this itself, so asking for the cap asks for everything
    // there is rather than for a number this CLI invented.
    let runs = engine.task_history(&task.id, keeper_sync::db::TASK_RUNS_CAP)?;
    let view = TaskView {
        task,
        profile_name: task_profile_name(&profiles, task),
        // Newest first, so the head of the history IS the last run — one read
        // rather than a second query for a row already in hand.
        last: runs.first(),
    };

    for line in task_lines(now_ms, std::slice::from_ref(&view), &[]) {
        printer.line(line);
    }
    for line in task_run_lines(now_ms, &task.id, &runs) {
        printer.line(line);
    }
    printer.json(&task_status_document(
        task_json(task, view.profile_name, view.last),
        runs.iter().map(task_run_json).collect(),
    ));
    Ok(EXIT_OK)
}

/// Run one task now, and exit with what happened.
///
/// [`Engine::run_task_now`] holds every decision — the lease, the mode and
/// `enabled` gates, the target selection, the work itself and the recorded row —
/// so a requested run and a scheduled run are the same code path taking the same
/// reservation. This function selects, prints, and maps the outcome onto a
/// number; a second implementation of any of that is what AD-52 exists to
/// prevent.
///
/// The task is selected from [`Engine::tasks`] **before** the engine is asked to
/// run anything, which is what makes a mistyped selector three distinguishable
/// refusals ([`select_task`]) rather than `run_task_now`'s single "no such
/// task": that door cannot tell a malformed id from an absent one, and cannot
/// see an unreadable row at all.
///
/// # The one error that is not an error
///
/// A lease held by the other host arrives as `Err(SyncError::Busy)`. Left alone
/// it would reach [`sync_exit_code`] and become [`EXIT_OK`] — "nothing went
/// wrong", which is true and useless: the work did not happen and a wrapper has
/// to know. It is remapped to [`EXIT_DEFERRED`] here, beside the recorded
/// [`TaskOutcome::Busy`] that means the same thing, so every "did not run"
/// answer this verb can give is one number. It is not turned into a `CliError`
/// either, because `main` prints a failure envelope for every `Err` and that
/// would put a second JSON document on stdout over this one.
///
/// # The clock this verb renders against is not the one it was handed
///
/// `run()` samples one instant before dispatching, so every countdown a verb
/// prints is measured from the same `now` — which is right for `tasks list` and
/// `tasks status`, whose rows all pre-date the sample. It is wrong here and
/// only here: this is the verb that *performs work between the sample and the
/// render*. `claim_and_run` opens the run a moment after `run()` read the
/// clock, so `run.started_ms` always sits just *after* `now_ms` — and a nightly
/// sweep that took five minutes therefore printed its finished run as having
/// started `now`, whatever it had actually spent.
///
/// The render therefore uses `now_ms.max(run.finished_ms)`: the later of when
/// this process read the clock and when the run actually ended, which is the
/// earliest instant that provably is not in the past. Taking the maximum rather
/// than re-reading a clock keeps this function callable from a test that owns no
/// platform, and the `--json` document is unaffected either way because it
/// carries absolute instants.
async fn cmd_task_run(
    printer: &Printer,
    engine: &Engine,
    now_ms: i64,
    wanted: &str,
    driver: TaskRunDriver,
) -> std::result::Result<u8, CliError> {
    let listing = engine.tasks()?;
    let id = select_task(&listing, wanted)?.id.clone();

    let run = match engine.run_task_now(&id, driver).await {
        Ok(run) => run,
        Err(err) => {
            // The one error that is a deferral. See [`lease_held_elsewhere`].
            let Some(task) = lease_held_elsewhere(&err) else {
                return Err(err.into());
            };
            // One sentence for both shapes of "did not run", because a wrapper
            // branches on the number and a person reads the line: for a person
            // the only cause is a lease held elsewhere, and for a timer on a
            // scheduled task the ordinary cause is that no window was open — the
            // other host had it, or nothing was due (Story 58.6).
            printer.line(match driver {
                TaskRunDriver::Person => {
                    format!("{task}: a run is already in flight elsewhere, so nothing ran here")
                }
                TaskRunDriver::Timer => {
                    format!("{task}: no window was open for a timer to claim, so nothing ran here")
                }
            });
            printer.json(&task_run_document(
                &id,
                None,
                TaskOutcome::Busy.as_str(),
                EXIT_DEFERRED,
            ));
            return Ok(EXIT_DEFERRED);
        }
    };

    // See this function's doc: the run happened between the sample and here, so
    // rendering against the sample alone puts a finished run in the future.
    let rendered_at = now_ms.max(run.finished_ms.unwrap_or(now_ms));
    for line in task_run_lines(rendered_at, &id, std::slice::from_ref(&run)) {
        printer.line(line);
    }
    let code = run_exit_code(&run);
    printer.json(&task_run_document(
        &id,
        Some(task_run_json(&run)),
        run_outcome_word(&run),
        code,
    ));
    Ok(code)
}

/// Read a task back and report it, so what a write verb says is what the store
/// now holds.
///
/// Deliberately **not** the row that was submitted.
/// [`keeper_sync::db::upsert_task`] owns `next_due_ms`, `running_host` and
/// `lease_until_ms` and ignores whatever a caller passes for them, and it clears
/// the window when the schedule text changes — so echoing the submitted row
/// would report a window that was never written. Re-reading is also the only
/// rendering that can be trusted the moment the other host is writing too.
fn report_task(
    printer: &Printer,
    engine: &Engine,
    now_ms: i64,
    id: &str,
) -> std::result::Result<u8, CliError> {
    let listing = engine.tasks()?;
    let task = select_task(&listing, id)?;
    let profiles = engine.list_profiles()?;
    let newest = engine.task_history(id, 1)?;
    let view = TaskView {
        task,
        profile_name: task_profile_name(&profiles, task),
        last: newest.first(),
    };
    for line in task_lines(now_ms, std::slice::from_ref(&view), &[]) {
        printer.line(line);
    }
    printer.json(&task_saved_document(task_json(
        task,
        view.profile_name,
        view.last,
    )));
    Ok(EXIT_OK)
}

/// Create a task, or change the fields a caller named.
///
/// Every rule here is about *not* silently doing something else.
///
/// * **On create, `--kind` is required**, refused with the id quoted. A row with
///   no kind is a row nothing can run, and there is no defensible default: the
///   two kinds do opposite things, and one of them deletes content.
/// * **On update, an omitted flag keeps its stored value.** The stored row is
///   read from [`Engine::tasks`] rather than guessed, so a caller changing a
///   schedule cannot unbind a folder by not mentioning it.
/// * **`--host-wide` and `--no-schedule` are how a binding and a schedule get
///   removed.** Without them either could be set and never unset, which is the
///   dead-knob shape this epic exists to close; each `conflicts_with` its
///   positive twin so a contradiction is refused by clap rather than resolved.
/// * **`--mode` defaults, on create only**, to `scheduled` when a `--schedule`
///   was given and `manual` otherwise. This is the only derived default here,
///   and it is derivable *because* it cannot produce a refused row:
///   [`keeper_sync::db::upsert_task`] refuses a `Scheduled` row with no
///   schedule, and both branches respect that by construction. Defaulting to
///   `scheduled` unconditionally would have made `tasks set nightly --kind sync`
///   fail at the write door for a reason the caller never mentioned.
/// * **`enabled` is `true` on create and untouched on update.** `tasks enable`
///   and `tasks disable` own that column; writing it from here is exactly how
///   [`reconcile`] would silently un-pause everything an operator paused, one
///   layer down.
///
/// `--profile` resolves through the same [`select`] every other verb uses
/// (story 56.14's exact-id-first selector) and requires **exactly one** match,
/// refusing a two-folder name match by asking for the id — [`cmd_materialize`]'s
/// rule, and it matters here because a `release` task bound to the wrong folder
/// deletes from the wrong folder.
fn cmd_task_set(
    printer: &Printer,
    engine: &Engine,
    now_ms: i64,
    args: &TaskSetArgs,
) -> std::result::Result<u8, CliError> {
    let listing = engine.tasks()?;
    // Not [`select_task`]: this is the one verb for which "no such task" is not
    // a refusal. Its other two answers still apply, in this order — a malformed
    // id can never be stored, and a row this build cannot read must not be
    // overwritten by a kind of ours — because reaching the `--kind`-is-required
    // refusal first would tell somebody looking at a newer keeper's row to
    // supply the one thing that cannot help.
    keeper_sync::tasks::validate_id(&args.task)?;
    if let Some(refusal) = unreadable_task(&listing, &args.task) {
        return Err(refusal);
    }
    let existing = listing.tasks.iter().find(|row| row.id == args.task);

    let kind = match (args.kind, existing) {
        // Repurposing a stored task is refused rather than performed, and the
        // reason is not tidiness. A `sync` task changed to `release` keeps its
        // armed window, so the deletion fires at an instant armed for a sync —
        // on the very next tick if that window is already open — with none of
        // the fresh-arm delay a newly created release task gets. It also keeps
        // its whole `task_runs` history, so `tasks status` renders `1 synced, 0
        // already syncing…` as a release task's record. Neither is fixable by
        // clearing a column: the history genuinely belongs to a different verb.
        // `tasks forget` then `tasks set` says the same thing and leaves nothing
        // mislabelled.
        (Some(wanted), Some(row)) if TaskKind::from(wanted) != row.kind => {
            return Err(SyncError::Config(format!(
                "task `{}` is stored as kind `{}` and a task's kind cannot be \
                 changed: its schedule window and its whole run history belong to \
                 that kind. Use `keeper-syncd tasks forget {}` and create it again",
                args.task,
                row.kind.as_str(),
                args.task
            ))
            .into());
        }
        (Some(kind), _) => TaskKind::from(kind),
        (None, Some(row)) => row.kind,
        (None, None) => {
            return Err(SyncError::Config(format!(
                "task `{}` does not exist yet, so a new one must name its kind: \
                 pass --kind sync or --kind release",
                args.task
            ))
            .into());
        }
    };

    let profile_id = if args.host_wide {
        None
    } else if let Some(wanted) = args.profile.as_deref() {
        let profiles = engine.list_profiles()?;
        let matched = select(&profiles, Some(wanted))?;
        let [profile] = matched[..] else {
            return Err(SyncError::Config(format!(
                "`{wanted}` matches {} folders; name the one you mean by its id",
                matched.len()
            ))
            .into());
        };
        Some(profile.id.clone())
    } else {
        existing.and_then(|row| row.profile_id.clone())
    };

    let schedule = if args.no_schedule {
        None
    } else if args.schedule.is_some() {
        args.schedule.clone()
    } else {
        existing.and_then(|row| row.schedule.clone())
    };

    let mode = match (args.mode, existing) {
        (Some(mode), _) => TaskMode::from(mode),
        (None, Some(row)) => row.mode,
        (None, None) if schedule.is_some() => TaskMode::Scheduled,
        (None, None) => TaskMode::Manual,
    };

    // The same shape as `mode` above minus the derivation: there is nothing to
    // derive, because the default is not a guess about intent — it is the
    // behaviour every task on every install already had.
    let on_missed = match (args.on_missed, existing) {
        (Some(policy), _) => TaskMissedPolicy::from(policy),
        (None, Some(row)) => row.on_missed,
        (None, None) => TaskMissedPolicy::RunNow,
    };

    let row = TaskRow {
        id: args.task.clone(),
        profile_id,
        kind,
        schedule,
        mode,
        // Carried, not computed. The write door owns all three and ignores what
        // it is handed for them; passing the stored values keeps this struct
        // honest about the row it describes rather than claiming a task with no
        // window and no lease.
        next_due_ms: existing.and_then(|row| row.next_due_ms),
        enabled: existing.is_none_or(|row| row.enabled),
        updated_ms: now_ms,
        running_host: existing.and_then(|row| row.running_host.clone()),
        lease_until_ms: existing.and_then(|row| row.lease_until_ms),
        on_missed,
    };
    engine.save_task(&row, None)?;
    report_task(printer, engine, now_ms, &row.id)
}

/// Take a task in or out of service, leaving everything else exactly as stored.
///
/// One function for both verbs because they differ in one boolean, and the
/// rest — read the row, keep every other column, write it back — is the part
/// that must not diverge. `mode` and `schedule` survive untouched: AD-135 keeps
/// `enabled` and `mode` apart because they answer different questions, so a
/// disabled `scheduled` task keeps its schedule and resumes on it.
fn cmd_task_set_enabled(
    printer: &Printer,
    engine: &Engine,
    now_ms: i64,
    wanted: &str,
    enabled: bool,
) -> std::result::Result<u8, CliError> {
    let listing = engine.tasks()?;
    let stored = select_task(&listing, wanted)?;
    let row = TaskRow {
        enabled,
        updated_ms: now_ms,
        ..stored.clone()
    };
    engine.save_task(&row, None)?;
    printer.line(format!(
        "{} task {}",
        if enabled { "Enabled" } else { "Disabled" },
        row.id
    ));
    report_task(printer, engine, now_ms, &row.id)
}

/// Forget a task and everything it recorded.
///
/// Selected through [`select_task`] first, so forgetting an id nobody stored is
/// a refusal rather than a silent success: `db::delete_task` cannot tell "gone
/// now" from "was never here", and a typo that reports success is how somebody
/// comes to believe a task is deleted while it is still on a schedule.
///
/// That also means a row this build **cannot read** is refused rather than
/// deleted, and that is the right way round: it belongs to a newer keeper on the
/// other host, and deleting a task on somebody else's behalf because we cannot
/// parse it is the "absence is deletion" mistake AD-48 exists to prevent.
fn cmd_task_forget(
    printer: &Printer,
    engine: &Engine,
    wanted: &str,
) -> std::result::Result<u8, CliError> {
    let listing = engine.tasks()?;
    let id = select_task(&listing, wanted)?.id.clone();
    engine.forget_task(&id)?;
    printer.line(format!("Forgot task {id} and its run history"));
    printer.json(&task_forgotten_document(&id));
    Ok(EXIT_OK)
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
            &["keeper-syncd", "pin", "docs", "40-media/clip.mp4"],
            &["keeper-syncd", "unpin", "docs", "40-media/clip.mp4"],
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
            &["keeper-syncd", "tasks", "list"],
            &["keeper-syncd", "tasks", "status", "nightly"],
            &["keeper-syncd", "tasks", "run", "nightly"],
            &["keeper-syncd", "tasks", "set", "nightly", "--kind", "sync"],
            &[
                "keeper-syncd",
                "tasks",
                "set",
                "nightly",
                "--kind",
                "release",
            ],
            &[
                "keeper-syncd",
                "tasks",
                "set",
                "nightly",
                "--kind",
                "sync",
                "--profile",
                "docs",
                "--schedule",
                "every 5m",
                "--mode",
                "scheduled",
            ],
            &["keeper-syncd", "tasks", "set", "nightly", "--host-wide"],
            &["keeper-syncd", "tasks", "set", "nightly", "--no-schedule"],
            &["keeper-syncd", "tasks", "set", "nightly", "--mode", "off"],
            &[
                "keeper-syncd",
                "tasks",
                "set",
                "nightly",
                "--mode",
                "manual",
            ],
            &["keeper-syncd", "tasks", "enable", "nightly"],
            &["keeper-syncd", "tasks", "disable", "nightly"],
            &["keeper-syncd", "tasks", "forget", "nightly"],
        ];
        for args in invocations {
            assert!(parse(args).is_ok(), "must parse: {args:?}");
        }
    }

    #[test]
    fn a_task_selector_is_required_by_every_verb_that_names_one() {
        // `tasks list` is the only verb that addresses no task; every other one
        // acts on exactly one row, and a missing selector must be told rather
        // than read as "all of them" — which for `run` and `forget` would be a
        // fan-out nobody asked for.
        for verb in ["status", "run", "set", "enable", "disable", "forget"] {
            let err = parse(&["keeper-syncd", "tasks", verb])
                .expect_err("a task id is required to name one");
            assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument, "{verb}");
        }
        assert!(parse(&["keeper-syncd", "tasks", "list"]).is_ok());
    }

    #[test]
    fn the_two_clearing_flags_conflict_with_their_positive_twins() {
        // `--host-wide` and `--no-schedule` exist so a binding and a schedule
        // can be UNSET; passing one beside the flag it clears is a contradiction
        // clap must refuse rather than silently resolve in one direction. A knob
        // whose two spellings disagree quietly is the dead-knob shape this epic
        // exists to close.
        for args in [
            vec![
                "keeper-syncd",
                "tasks",
                "set",
                "nightly",
                "--profile",
                "docs",
                "--host-wide",
            ],
            vec![
                "keeper-syncd",
                "tasks",
                "set",
                "nightly",
                "--schedule",
                "every 5m",
                "--no-schedule",
            ],
        ] {
            let err = parse(&args).expect_err("contradictory flags must be refused");
            assert_eq!(err.kind(), ErrorKind::ArgumentConflict, "{args:?}");
        }
    }

    #[test]
    fn tasks_run_help_names_the_exit_codes_it_can_produce() {
        // Story 56.13 shipped a `--help` that described what `materialize` used
        // to do, and it survived review because the prose sits on a clap enum
        // far from the code it describes. `tasks run` is the verb whose whole
        // purpose is to be called from a wrapper that branches on `$?`, so the
        // help that omits a code is help that cannot be acted on — and this
        // asserts every number a caller can actually see in `$?` is named where
        // the caller looks. `3` joined the list in Story 57.7: it is reachable
        // because `Engine::open` resolves `git` before any task verb runs, the
        // shipped systemd unit lists it in `RestartPreventExitStatus`, and
        // `docs/sync.md` §14 documents it — so a wrapper author reading only
        // this help would have been the one consumer left unable to handle it.
        let mut cli = Cli::command();
        let tasks = cli.find_subcommand_mut("tasks").expect("a `tasks` verb");
        let run = tasks
            .find_subcommand_mut("run")
            .expect("a `tasks run` verb");
        let help = run.render_long_help().to_string();
        for expected in ["0", "1", "2", "3", "4", "deferred"] {
            assert!(
                help.contains(expected),
                "`tasks run --help` must name {expected}; got:\n{help}"
            );
        }
    }

    /// `tasks set --help`'s account of `--on-missed` is computed from the two
    /// constants that decide it, not written beside them (Story 58.9).
    ///
    /// The guard exists because this help was **already wrong**, in exactly
    /// Story 56.13's shape. Story 58.4 wrote *"`delay` serves it no sooner than
    /// fifteen minutes after it fell due"*; the review then rejected that anchor
    /// and the fix moved it to the instant a host **noticed** the window, with a
    /// separate and longer constant. `TASK_MISSED_DELAY_MS` and
    /// [`keeper_sync::tasks::decide`]'s doc were updated; this string and the
    /// app's form note were not. A wrong number therefore survived a review pass
    /// and a full gate, because prose that sits on a clap enum is far from the
    /// code it describes and nothing compared them.
    ///
    /// So the numbers are derived here. Changing either constant without
    /// rewriting the sentence fails this test rather than shipping.
    #[test]
    fn tasks_set_help_names_the_missed_window_numbers_its_constants_actually_use() {
        let grace_minutes = keeper_sync::tasks::TASK_MISSED_GRACE_MS / 60_000;
        let delay_minutes = keeper_sync::tasks::TASK_MISSED_DELAY_MS / 60_000;
        assert_ne!(
            grace_minutes, delay_minutes,
            "the two numbers answer different questions — when do we conclude \
             nobody was home, and how long do we then wait — so a help text that \
             could use one for the other is a help text this test cannot guard"
        );

        let mut cli = Cli::command();
        let tasks = cli.find_subcommand_mut("tasks").expect("a `tasks` verb");
        let set = tasks
            .find_subcommand_mut("set")
            .expect("a `tasks set` verb");
        let help = set.render_long_help().to_string();

        // Each number is asserted **in its role**, not merely present. Written
        // as two bare `contains("N minutes")` checks this test passed on a
        // sentence that said *fifteen* minutes, because the clause contrasting
        // it with the wrong anchor also mentioned thirty — the guard was
        // satisfied by the very phrase warning against the error. The number and
        // the instant it is measured from are one claim, so they are one
        // assertion.
        for expected in [
            format!("concludes after {grace_minutes} minutes"),
            format!("runs it {delay_minutes} minutes after a host noticed it"),
        ] {
            assert!(
                help.contains(&expected),
                "`tasks set --help` must state `{expected}`; got:\n{help}"
            );
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
        // `materialize`, `dehydrate`, `pin` and `unpin` each need two: a folder
        // and a path inside it. One argument is the shape a person trying
        // `materialize clip.mp4` produces, and it must be told what is missing
        // rather than have `clip.mp4` read as a profile name.
        for args in [
            vec!["keeper-syncd", "materialize"],
            vec!["keeper-syncd", "materialize", "docs"],
            vec!["keeper-syncd", "dehydrate"],
            vec!["keeper-syncd", "dehydrate", "docs"],
            vec!["keeper-syncd", "pin"],
            vec!["keeper-syncd", "pin", "docs"],
            vec!["keeper-syncd", "unpin"],
            vec!["keeper-syncd", "unpin", "docs"],
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
    fn the_five_exit_codes_are_distinct() {
        // A wrapper script branches on these; two of them colliding silently
        // would make "fix your config" indistinguishable from "retry later".
        // `EXIT_DEFERRED` joined them in Story 57.3, and it is the one whose
        // collision would be worst: sharing a number with `EXIT_OK` hides a
        // month of unplugged drive, sharing one with `EXIT_FAILURE` pages
        // somebody nightly for a condition AD-48 calls normal.
        let codes = [
            EXIT_OK,
            EXIT_FAILURE,
            EXIT_CONFIG,
            EXIT_PREREQUISITE,
            EXIT_DEFERRED,
        ];
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

    /// Story 56.14: a selector that is one folder's id and a different folder's
    /// name resolves to the folder whose ID it is, and to exactly one folder.
    ///
    /// Fails without the precedence in [`select`]: the old
    /// `id == wanted || name == wanted` filter matched both rows, so the
    /// single-profile destructure in `cmd_materialize`, `cmd_dehydrate`, `cmd_pin`
    /// and `cmd_unpin` failed and all four verbs refused for BOTH folders, for
    /// ever, telling the caller to name the one they meant by its id — which is
    /// what they had just typed.
    #[test]
    fn an_id_that_is_also_another_folders_name_resolves_to_the_id() {
        // `archive`'s id is literally `media`, which is `media`'s name. Contrived
        // to type and not at all contrived to HAVE: ids come from the config file
        // and an operator writing `id = "media"` for the folder holding media is
        // doing the obvious thing.
        let media = SyncProfile::new("01MEDIA", "media", "/srv/media", "https://x/m.git");
        let archive = SyncProfile::new("media", "archive", "/srv/archive", "https://x/a.git");
        let profiles = vec![media.clone(), archive.clone()];

        let matched = select(&profiles, Some("media")).expect("an id match is never ambiguous");
        assert_eq!(
            matched,
            vec![&archive],
            "the id owns the selector; the folder merely NAMED `media` does not answer for it"
        );

        // Declaration order must not decide it either, so the same collision the
        // other way round.
        let reversed = vec![archive.clone(), media.clone()];
        assert_eq!(
            select(&reversed, Some("media")).expect("still unambiguous"),
            vec![&archive]
        );

        // And the other folder is still reachable by everything that identifies
        // it — its own id, and its name, which nothing else claims as an id.
        assert_eq!(
            select(&profiles, Some("01MEDIA")).expect("by id"),
            vec![&media]
        );
        assert_eq!(
            select(&profiles, Some("archive")).expect("by name"),
            vec![&archive]
        );
    }

    /// The ambiguity that is left after story 56.14, and the reason the message
    /// the single-path verbs raise for it is now TRUE: two folders may share a
    /// name, and then naming one by its id is exactly the way out.
    #[test]
    fn two_folders_sharing_a_name_still_both_match_that_name() {
        let one = SyncProfile::new("01ONE", "media", "/srv/one", "https://x/1.git");
        let two = SyncProfile::new("01TWO", "media", "/srv/two", "https://x/2.git");
        let profiles = vec![one.clone(), two.clone()];

        assert_eq!(
            select(&profiles, Some("media")).expect("a name is not required to be unique"),
            vec![&one, &two],
            "a read verb takes both listings; a write verb refuses and asks for an id"
        );
        assert_eq!(
            select(&profiles, Some("01TWO")).expect("and the id is the way out"),
            vec![&two]
        );
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

    // --- verify: the two renderings are the contract (Story 56.6) ----------

    fn verified(checked: u64, virtual_paths: u64, bad: &[(&str, &str)]) -> VerifyReport {
        VerifyReport {
            checked,
            virtual_paths,
            bad: bad
                .iter()
                .map(|(path, reason)| ((*path).to_owned(), (*reason).to_owned()))
                .collect(),
        }
    }

    /// The whole anti-quietness argument is this string, and until now nothing
    /// asserted it.
    ///
    /// `verify` no longer reports an authorized virtual path as a fault, so the
    /// only thing standing between "excused correctly" and "stopped looking" is
    /// a count on the line an operator reads. A thousand quiet paths with no
    /// number beside them is the failure this story could have shipped.
    #[test]
    fn the_human_line_counts_what_was_excused_beside_what_was_wrong() {
        let profile = SyncProfile::new("01DOCS", "Field", "/srv/docs", "https://x/d.git");
        let lines = verify_lines(
            &profile,
            Ok(&verified(
                1_001,
                1_000,
                &[("30-docs/report.bin", "LFS object abc is missing locally")],
            )),
        );

        assert_eq!(
            lines.len(),
            2,
            "a header and one line per bad path: {lines:?}"
        );
        assert_eq!(lines[0], "Field: 1001 checked, 1 bad, 1000 virtual");
        assert_eq!(
            lines[1],
            "  30-docs/report.bin: LFS object abc is missing locally"
        );
    }

    /// A clean folder still says how many paths are deliberately away, because
    /// zero bad and a thousand virtual is the normal state Epic 56 created and
    /// an operator has to be able to see it.
    #[test]
    fn a_clean_folder_still_names_its_virtual_count() {
        let profile = SyncProfile::new("01DOCS", "Field", "/srv/docs", "https://x/d.git");
        assert_eq!(
            verify_lines(&profile, Ok(&verified(1_000, 1_000, &[]))),
            vec!["Field: 1000 checked, 0 bad, 1000 virtual"]
        );
    }

    /// The JSON form's key set is what a consumer parses, so `virtual` is
    /// asserted as a KEY and not as a substring: a renamed or dropped field is
    /// the breakage that matters, and a `contains` check would miss both.
    #[test]
    fn the_verify_entrys_key_set_carries_the_virtual_count() {
        let profile = SyncProfile::new("01DOCS", "Field", "/srv/docs", "https://x/d.git");
        let entry = verify_entry(
            &profile,
            Ok(&verified(
                7,
                4,
                &[("a.bin", "LFS object abc is missing locally")],
            )),
        );

        let object = entry.as_object().expect("an object");
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec!["bad", "checked", "profile", "profileId", "virtual"]
        );
        assert_eq!(object["virtual"], serde_json::json!(4));
        assert_eq!(object["checked"], serde_json::json!(7));
        assert_eq!(
            object["bad"],
            serde_json::json!([{ "path": "a.bin", "reason": "LFS object abc is missing locally" }])
        );
    }

    /// A folder whose verify could not run is its own entry, and it carries
    /// `error` with **no** `bad` array.
    ///
    /// Both halves matter. The entry exists because a `?` used to discard every
    /// report already computed and emit no document at all, so one folder's
    /// `.keepervirtual` typo blinded the operator to the folders beside it. The
    /// absent `bad` matters because an empty array cannot distinguish "checked,
    /// nothing wrong" from "never checked", and those are the two answers a
    /// consumer must never confuse.
    #[test]
    fn a_folder_that_could_not_be_checked_is_its_own_entry_and_not_a_clean_one() {
        let profile = SyncProfile::new("01DOCS", "Field", "/srv/docs", "https://x/d.git");
        let reason = "40-media/[unclosed in .keepervirtual is not a valid pattern";

        assert_eq!(
            verify_lines(&profile, Err(reason)),
            vec![format!("Field: could not be checked: {reason}")]
        );

        let entry = verify_entry(&profile, Err(reason));
        let object = entry.as_object().expect("an object");
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["error", "profile", "profileId"]);
        assert_eq!(object["error"], serde_json::json!(reason));
        assert!(
            !object.contains_key("bad"),
            "an empty `bad` would read as a folder that was checked: {entry}"
        );
        assert!(!object.contains_key("virtual"));
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

        // One line whatever happened. The second line this used to carry
        // promised a `keeper-syncd watch` that would deliver the transfer, and
        // that promise is the defect this story removed: a queued outcome's
        // remaining sentence depends on where the journal row stands and belongs
        // to `undelivered`, which is the only thing that reads it.
        for outcome in [
            MaterializeOutcome::Materialized,
            MaterializeOutcome::AlreadyMaterialized,
            MaterializeOutcome::Queued,
        ] {
            let lines = materialize_lines("Field", &materialization(outcome, Some(42)));
            assert_eq!(lines.len(), 1, "{outcome:?}: {lines:?}");
            assert!(
                !lines[0].contains("watch"),
                "no surface may promise a supervisor this process does not have: {}",
                lines[0]
            );
        }
    }

    /// Only a queued outcome is a failed run, and each of the four places its
    /// journal row can stand gets its own sentence (Story 56.13).
    ///
    /// This is the branch that decides `$?` for the cron entry this verb exists
    /// for. Before the drain existed the verb exited `0` on `queued`, which is
    /// the whole defect: a no-op wearing a success message.
    ///
    /// The four arms are not decoration. Two of them were single-handedly wrong
    /// before review: a **parked** row sent the operator to start a watcher that
    /// will never claim it, and a row another keeper process was **transferring**
    /// was reported as this run's failure — an alert on a healthy folder.
    #[test]
    fn only_an_undelivered_run_is_a_failure_and_each_standing_says_its_own_thing() {
        let waiting = UnitStanding::Waiting { reason: None };
        assert_eq!(
            undelivered(
                "Field",
                &materialization(MaterializeOutcome::Materialized, None),
                &waiting
            ),
            None,
            "the bytes are here, whoever carried them"
        );
        assert_eq!(
            undelivered(
                "Field",
                &materialization(MaterializeOutcome::AlreadyMaterialized, None),
                &waiting
            ),
            None
        );

        let queued = materialization(MaterializeOutcome::Queued, Some(42));
        let say = |standing: &UnitStanding| {
            undelivered("Field", &queued, standing)
                .expect("a queued outcome is a run that did not do what was asked")
        };

        let quoted = say(&UnitStanding::Waiting {
            reason: Some("remote storage quota exceeded for git.example".to_owned()),
        });
        assert!(quoted.contains("40-media/clip.mp4"), "{quoted}");
        assert!(quoted.contains("unit 42"), "{quoted}");
        assert!(
            quoted.contains("remote storage quota exceeded for git.example"),
            "the journal's reason is the only place the transfer's own error \
             survives a drain: {quoted}"
        );
        assert!(
            quoted.contains("last recorded"),
            "and it is qualified, because `claim_ready` does not clear \
             `last_error` and this run may never have attempted the row: {quoted}"
        );

        let bare = say(&waiting);
        assert!(
            bare.contains("keeper-syncd logs"),
            "a run with no recorded reason must still say where to look: {bare}"
        );

        let elsewhere = say(&UnitStanding::InFlight);
        assert!(
            elsewhere.contains("another keeper process") && elsewhere.contains("ask again"),
            "a transfer somebody else is performing is not this run's failure: {elsewhere}"
        );
        assert!(
            !elsewhere.contains("failure"),
            "and must not be described as one: {elsewhere}"
        );

        let dead = say(&UnitStanding::Parked {
            reason: Some("git.example rejected the access token".to_owned()),
        });
        assert!(
            dead.contains("given up") && dead.contains("rejected the access token"),
            "{dead}"
        );
        assert!(
            !dead.contains("watch"),
            "nothing claims a parked row, so no watcher can help: {dead}"
        );

        let vanished = say(&UnitStanding::Settled);
        assert!(
            vanished.contains("finished") && vanished.contains("not on disk"),
            "a completed row over absent content is its own state — the prune \
             raced the pass — and 'ask again' really is the remedy: {vanished}"
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

    /// Two positionals, in that order, and neither optional; `--for` is the
    /// only thing that is.
    #[test]
    fn materialize_takes_a_folder_and_a_path_inside_it() {
        let cli =
            parse(&["keeper-syncd", "materialize", "docs", "40-media/clip.mp4"]).expect("parse");
        let Command::Materialize {
            profile,
            subpath,
            keep_for,
        } = cli.command
        else {
            panic!("expected materialize");
        };
        assert_eq!(profile, "docs");
        assert_eq!(subpath, "40-media/clip.mp4");
        assert_eq!(
            keep_for, None,
            "no flag is no statement about how long, which is what every \
             invocation written before Story 56.17 means"
        );
    }

    /// `--for` reaches the verb as milliseconds, and a value nobody can read
    /// is refused **at parse time** with the value quoted (Story 56.17).
    ///
    /// At parse time is the claim, not merely "refused": clap's `value_parser`
    /// runs before `run` opens an engine, resolves a profile or looks at a
    /// pointer, so a typo cannot half-materialize anything. The same discipline
    /// `validate_quiet_time` applies to a quiet window, and here the cost of
    /// guessing at a unit is when keeper deletes a copy of somebody's file.
    #[test]
    fn for_is_parsed_into_milliseconds_and_a_typo_is_refused_with_the_value_quoted() {
        for (spelling, expected) in [
            ("30m", Some(1_800_000)),
            ("2h", Some(7_200_000)),
            ("1d", Some(86_400_000)),
            // Accepted, and it means indefinite — the folder's own interval and
            // nothing else, which is what the verb does with no flag.
            ("0", Some(0)),
        ] {
            let cli = parse(&[
                "keeper-syncd",
                "materialize",
                "docs",
                "clip.mp4",
                "--for",
                spelling,
            ])
            .unwrap_or_else(|err| panic!("`--for {spelling}` should parse: {err}"));
            let Command::Materialize { keep_for, .. } = cli.command else {
                panic!("expected materialize");
            };
            assert_eq!(keep_for, expected, "--for {spelling}");
        }

        // Every one of these is somebody expecting a unit this verb does not
        // have, or a shape it does not read. Guessing at any of them would be
        // worse than saying so.
        for spelling in [
            "1w",
            "2",
            "1.5h",
            "2 h",
            "h",
            "",
            "0m",
            "+2h",
            "1H",
            "99999999999999999999d",
        ] {
            let err = parse(&[
                "keeper-syncd",
                "materialize",
                "docs",
                "clip.mp4",
                "--for",
                spelling,
            ])
            .expect_err(&format!("`--for {spelling}` must be refused"))
            .to_string();
            assert!(
                err.contains(spelling),
                "the refusal has to quote what was typed, or the person cannot \
                 see their own typo: {err}"
            );
            assert!(
                err.contains("30m") && err.contains("2h") && err.contains("1d"),
                "and it has to name the forms that do work, because the reader \
                 is looking at it precisely because they do not know them: {err}"
            );
        }

        // `-1h` never reaches the parser: clap reads a leading `-` as short
        // flags and refuses `-1` before any value parser runs. Still refused at
        // parse time, which is the claim, but by clap's own message — asserted
        // separately rather than folded into the loop above, because a test
        // that expected THIS sentence to quote `-1h` would be asserting
        // something clap does not do and would have to be weakened to pass.
        let err = parse(&[
            "keeper-syncd",
            "materialize",
            "docs",
            "clip.mp4",
            "--for",
            "-1h",
        ])
        .expect_err("a negative duration is refused")
        .to_string();
        assert!(
            err.contains("-1"),
            "and it still shows what was typed: {err}"
        );
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
            ContentRefusal::LocallyModified { path: path.clone() },
            ContentRefusal::NotVirtual { path: path.clone() },
            ContentRefusal::ContentNotHere { path },
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

    // --- pin / unpin: the verb that makes 56.4's floor reachable (56.5) -----

    /// Two positionals, in that order, and neither optional — for both verbs.
    #[test]
    fn pin_and_unpin_each_take_a_folder_and_a_path_inside_it() {
        let cli = parse(&["keeper-syncd", "pin", "docs", "40-media/clip.mp4"]).expect("parse");
        let Command::Pin { profile, subpath } = cli.command else {
            panic!("expected pin");
        };
        assert_eq!(profile, "docs");
        assert_eq!(subpath, "40-media/clip.mp4");

        let cli = parse(&["keeper-syncd", "unpin", "docs", "40-media/clip.mp4"]).expect("parse");
        let Command::Unpin { profile, subpath } = cli.command else {
            panic!("expected unpin");
        };
        assert_eq!(profile, "docs");
        assert_eq!(subpath, "40-media/clip.mp4");
    }

    /// The human line says which way the instruction went, in prose.
    #[test]
    fn the_pin_line_says_pinned_or_unpinned_in_prose() {
        let pinned = pin_line(
            "Field",
            &Pin {
                path: "40-media/clip.mp4".to_owned(),
                pinned: true,
            },
        );
        assert!(pinned.contains("pinned"), "{pinned}");
        assert!(pinned.contains("40-media/clip.mp4"), "{pinned}");
        assert!(
            !pinned.contains("true"),
            "the boolean belongs to the document, not the sentence: {pinned}"
        );

        let unpinned = pin_line(
            "Field",
            &Pin {
                path: "40-media/clip.mp4".to_owned(),
                pinned: false,
            },
        );
        assert!(unpinned.contains("unpinned"), "{unpinned}");
        assert!(
            !unpinned.contains("false"),
            "same rule the other way round: {unpinned}"
        );
    }

    /// The pin document's key set is exactly the contract.
    ///
    /// No `oid` and no `sizeBytes`, and that is the sharp part: a pin says
    /// nothing about content and may be recorded for a path whose bytes are not
    /// here, so either key would be an answer to a question nobody asked.
    #[test]
    fn the_pin_json_key_set_is_exactly_the_contract() {
        let profile = SyncProfile::new("01DOCS", "docs", "/srv/docs", "https://x/d.git");
        let entry = pin_json(
            &profile,
            &Pin {
                path: "40-media/clip.mp4".to_owned(),
                pinned: true,
            },
        );

        let object = entry.as_object().expect("an object");
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["path", "pinned", "profile", "profileId"]);
        assert_eq!(object["profileId"], serde_json::json!("01DOCS"));
        assert_eq!(object["profile"], serde_json::json!("docs"));
        assert_eq!(object["path"], serde_json::json!("40-media/clip.mp4"));
        assert_eq!(
            object["pinned"],
            serde_json::json!(true),
            "a boolean, so a consumer branches on a type rather than on a word"
        );

        let cleared = pin_json(
            &profile,
            &Pin {
                path: "40-media/clip.mp4".to_owned(),
                pinned: false,
            },
        );
        assert_eq!(cleared["pinned"], serde_json::json!(false));
    }

    // -----------------------------------------------------------------------
    // tasks (Story 57.3)
    // -----------------------------------------------------------------------

    /// A readable stored task, mutated per test rather than rebuilt.
    fn a_task(id: &str) -> TaskRow {
        TaskRow {
            id: id.to_owned(),
            profile_id: None,
            kind: TaskKind::Sync,
            schedule: None,
            mode: TaskMode::Manual,
            next_due_ms: None,
            enabled: true,
            updated_ms: 1_700_000_000_000,
            running_host: None,
            lease_until_ms: None,
            on_missed: TaskMissedPolicy::RunNow,
        }
    }

    /// One closed run of `task_id`.
    fn a_run(task_id: &str, outcome: TaskOutcome) -> TaskRunRow {
        TaskRunRow {
            id: 7,
            task_id: task_id.to_owned(),
            started_ms: 1_700_000_000_000,
            finished_ms: Some(1_700_000_001_000),
            outcome: Some(outcome),
            unknown_outcome: None,
            detail: Some("1 synced, 0 already syncing, 0 waiting, 0 failed".to_owned()),
            host: "server-a".to_owned(),
        }
    }

    /// `tasks set` with nothing named — the shape every "an omitted flag keeps
    /// what was stored" assertion starts from.
    fn set_args(task: &str) -> TaskSetArgs {
        TaskSetArgs {
            task: task.to_owned(),
            kind: None,
            profile: None,
            host_wide: false,
            schedule: None,
            no_schedule: false,
            mode: None,
            on_missed: None,
        }
    }

    fn sorted_keys(value: &serde_json::Value) -> Vec<&str> {
        let mut keys: Vec<&str> = value
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        keys
    }

    #[test]
    fn every_task_outcome_maps_to_the_number_a_wrapper_branches_on() {
        // Arm by arm, because the classification is the contract and a single
        // "they are all mapped" assertion would pass with every one of them
        // wrong. `Busy` and `Deferred` share a number on purpose: `Engine::
        // next_task_window` already groups them as "the run did not happen",
        // and the exit code says the same thing to a cron wrapper.
        assert_eq!(task_exit_code(TaskOutcome::Ok), EXIT_OK);
        assert_eq!(task_exit_code(TaskOutcome::Busy), EXIT_DEFERRED);
        assert_eq!(task_exit_code(TaskOutcome::Deferred), EXIT_DEFERRED);
        assert_eq!(task_exit_code(TaskOutcome::Failed), EXIT_FAILURE);
        assert_eq!(task_exit_code(TaskOutcome::Abandoned), EXIT_FAILURE);
    }

    #[test]
    fn a_run_with_no_readable_outcome_is_a_failure_rather_than_a_success() {
        // A newer keeper's spelling is a run that happened and whose result we
        // cannot classify, so the safe number is the one that alerts: reading it
        // as `EXIT_OK` would report an unknown result as a working nightly sweep.
        let mut unreadable = a_run("nightly", TaskOutcome::Ok);
        unreadable.outcome = None;
        unreadable.unknown_outcome = Some("teleported".to_owned());
        assert_eq!(run_exit_code(&unreadable), EXIT_FAILURE);
        assert_eq!(run_outcome_word(&unreadable), "teleported");

        // Neither column set is a row still in flight. This verb closes the run
        // it opened so it cannot arise here, but the mapping must still be total.
        let mut in_flight = unreadable.clone();
        in_flight.unknown_outcome = None;
        in_flight.finished_ms = None;
        assert_eq!(run_exit_code(&in_flight), EXIT_FAILURE);
        assert_eq!(run_outcome_word(&in_flight), "running");

        assert_eq!(
            run_exit_code(&a_run("nightly", TaskOutcome::Busy)),
            EXIT_DEFERRED
        );
        assert_eq!(
            run_outcome_word(&a_run("nightly", TaskOutcome::Busy)),
            "busy"
        );
    }

    #[test]
    fn only_a_held_lease_is_the_deferral_that_arrives_as_an_error() {
        // `Busy` is the ONE error `tasks run` answers with `EXIT_DEFERRED`
        // rather than propagating, so the set has to be exactly that: every
        // other error is a real failure whose code `sync_exit_code` decides.
        assert_eq!(
            lease_held_elsewhere(&SyncError::Busy("nightly".to_owned())),
            Some("nightly"),
            "and it names the task, so the sentence a person reads is about \
             something they typed"
        );
        for other in [
            SyncError::Config("off".to_owned()),
            SyncError::MediaAbsent,
            SyncError::Cancelled,
            SyncError::Journal("locked".to_owned()),
        ] {
            assert_eq!(lease_held_elsewhere(&other), None, "{}", other.code());
        }
        // Distinguished by TYPE, never by the message: `Config` also mentions a
        // task by name, and matching on prose would have caught it.
        assert_eq!(
            lease_held_elsewhere(&SyncError::Config(
                "task nightly is off, so nothing runs it".to_owned()
            )),
            None
        );
    }

    #[test]
    fn a_malformed_selector_an_unknown_id_and_an_unreadable_row_are_three_refusals() {
        let listing = TaskListing {
            tasks: vec![a_task("nightly")],
            unknown: vec![UnknownTask {
                id: "teleport-1".to_owned(),
                reason: "kind 'teleport' is not one this keeper can run".to_owned(),
            }],
        };

        // The one that resolves.
        assert_eq!(
            select_task(&listing, "nightly").expect("an exact id").id,
            "nightly"
        );

        // 1. Malformed: a spelling `upsert_task` could never have stored, so no
        //    list of known ids would help — the input itself is the problem.
        let malformed = select_task(&listing, "nightly ").expect_err("a trailing space");
        assert_eq!(malformed.exit_code(), EXIT_CONFIG);
        assert!(
            malformed.to_string().contains("nightly "),
            "the input must be quoted: {malformed}"
        );
        assert!(
            malformed.to_string().contains("whitespace"),
            "and the rule named: {malformed}"
        );
        let blank = select_task(&listing, "   ").expect_err("blank");
        assert_eq!(blank.exit_code(), EXIT_CONFIG);

        // 2. Unknown: well formed, but no such row — so the alternatives are
        //    exactly what helps.
        let unknown = select_task(&listing, "nope").expect_err("no such task");
        assert_eq!(unknown.exit_code(), EXIT_CONFIG);
        assert!(unknown.to_string().contains("nope"), "{unknown}");
        assert!(
            unknown.to_string().contains("nightly"),
            "the known ids must be listed: {unknown}"
        );

        // 3. Stored, but unreadable — a third answer, and hiding it would make
        //    an unreadable row indistinguishable from an absent one.
        let unreadable = select_task(&listing, "teleport-1").expect_err("an unreadable row");
        assert_eq!(unreadable.exit_code(), EXIT_CONFIG);
        assert!(
            unreadable.to_string().contains("teleport-1"),
            "{unreadable}"
        );
        assert!(
            unreadable.to_string().contains("teleport"),
            "the stored reason must survive: {unreadable}"
        );
        assert!(
            !unreadable.to_string().contains("no task matches"),
            "an unreadable row must not be reported as absent: {unreadable}"
        );
    }

    #[test]
    fn an_unknown_selector_says_so_honestly_when_nothing_is_stored() {
        // `select`'s own shape: "known tasks are: " with an empty list after it
        // reads as a rendering bug, not as an empty database.
        let err = select_task(&TaskListing::default(), "nightly").expect_err("nothing stored");
        assert_eq!(err.exit_code(), EXIT_CONFIG);
        assert!(err.to_string().contains("no tasks are stored"), "{err}");
    }

    #[test]
    fn a_host_wide_task_document_carries_every_key_and_nulls_the_binding() {
        let document = task_json(&a_task("nightly"), None, None);
        assert_eq!(
            sorted_keys(&document),
            vec![
                "enabled",
                "id",
                "kind",
                "lastRun",
                "leaseUntilMs",
                "mode",
                "nextDueMs",
                "onMissed",
                "profile",
                "profileId",
                "runningHost",
                "schedule",
            ]
        );
        assert_eq!(document["id"], serde_json::json!("nightly"));
        assert_eq!(document["kind"], serde_json::json!("sync"));
        assert_eq!(document["mode"], serde_json::json!("manual"));
        assert_eq!(document["enabled"], serde_json::json!(true));
        // Null, not absent: "this task belongs to the machine" is a real value.
        assert_eq!(document["profileId"], serde_json::Value::Null);
        assert_eq!(document["profile"], serde_json::Value::Null);
        assert_eq!(document["schedule"], serde_json::Value::Null);
        assert_eq!(document["nextDueMs"], serde_json::Value::Null);
        assert_eq!(document["runningHost"], serde_json::Value::Null);
        assert_eq!(document["leaseUntilMs"], serde_json::Value::Null);
        // Present on every row including the default ones, unlike the human
        // rendering's marker: a fixed key set is this document's contract.
        assert_eq!(document["onMissed"], serde_json::json!("run_now"));
        assert_eq!(document["lastRun"], serde_json::Value::Null);
    }

    #[test]
    fn a_bound_scheduled_task_document_carries_the_values_it_was_given() {
        let mut task = a_task("docs-nightly");
        task.profile_id = Some("01DOCS".to_owned());
        task.kind = TaskKind::Release;
        task.mode = TaskMode::Scheduled;
        task.schedule = Some("every 5m".to_owned());
        task.next_due_ms = Some(1_700_000_300_000);
        task.enabled = false;
        task.running_host = Some("server-b".to_owned());
        task.lease_until_ms = Some(1_700_000_060_000);

        let last = a_run("docs-nightly", TaskOutcome::Deferred);
        let document = task_json(&task, Some("docs"), Some(&last));
        assert_eq!(document["profileId"], serde_json::json!("01DOCS"));
        assert_eq!(document["profile"], serde_json::json!("docs"));
        assert_eq!(document["kind"], serde_json::json!("release"));
        assert_eq!(document["mode"], serde_json::json!("scheduled"));
        assert_eq!(document["enabled"], serde_json::json!(false));
        assert_eq!(document["schedule"], serde_json::json!("every 5m"));
        assert_eq!(
            document["nextDueMs"],
            serde_json::json!(1_700_000_300_000i64)
        );
        assert_eq!(document["runningHost"], serde_json::json!("server-b"));
        assert_eq!(
            document["leaseUntilMs"],
            serde_json::json!(1_700_000_060_000i64)
        );
        assert_eq!(document["lastRun"], task_run_json(&last));

        // A binding whose folder is gone: the id survives and the name is null,
        // which is what makes it distinguishable from host-wide.
        let dangling = task_json(&task, None, None);
        assert_eq!(dangling["profileId"], serde_json::json!("01DOCS"));
        assert_eq!(dangling["profile"], serde_json::Value::Null);
    }

    #[test]
    fn the_run_document_omits_unknown_outcome_unless_there_is_one() {
        let readable = a_run("nightly", TaskOutcome::Ok);
        let document = task_run_json(&readable);
        assert_eq!(
            sorted_keys(&document),
            vec![
                "detail",
                "finishedMs",
                "host",
                "id",
                "outcome",
                "startedMs",
                "taskId",
            ],
            "no `unknownOutcome` when the outcome was readable"
        );
        assert_eq!(document["id"], serde_json::json!(7));
        assert_eq!(document["taskId"], serde_json::json!("nightly"));
        assert_eq!(
            document["startedMs"],
            serde_json::json!(1_700_000_000_000i64)
        );
        assert_eq!(
            document["finishedMs"],
            serde_json::json!(1_700_000_001_000i64)
        );
        assert_eq!(document["outcome"], serde_json::json!("ok"));
        assert_eq!(
            document["detail"],
            serde_json::json!("1 synced, 0 already syncing, 0 waiting, 0 failed")
        );
        assert_eq!(document["host"], serde_json::json!("server-a"));

        // Still in flight: `outcome` null and NO `unknownOutcome`.
        let mut flight = readable.clone();
        flight.outcome = None;
        flight.finished_ms = None;
        flight.detail = None;
        let document = task_run_json(&flight);
        assert_eq!(
            sorted_keys(&document),
            vec![
                "detail",
                "finishedMs",
                "host",
                "id",
                "outcome",
                "startedMs",
                "taskId",
            ]
        );
        assert_eq!(document["outcome"], serde_json::Value::Null);
        assert_eq!(document["finishedMs"], serde_json::Value::Null);

        // A newer keeper's spelling: the key APPEARS, which is the only way a
        // consumer can tell this from the row above.
        let mut newer = readable.clone();
        newer.outcome = None;
        newer.unknown_outcome = Some("teleported".to_owned());
        let document = task_run_json(&newer);
        assert_eq!(
            sorted_keys(&document),
            vec![
                "detail",
                "finishedMs",
                "host",
                "id",
                "outcome",
                "startedMs",
                "taskId",
                "unknownOutcome",
            ]
        );
        assert_eq!(document["outcome"], serde_json::Value::Null);
        assert_eq!(document["unknownOutcome"], serde_json::json!("teleported"));
    }

    /// A row so corrupt its primary key will not read still has to be a line
    /// somebody can make sense of (NFR-43).
    ///
    /// `db::list_tasks` reports it with an **empty** id, which rendered as a
    /// line starting with a bare colon: it told the operator a row exists and
    /// named nothing they could type. There genuinely is no id to give them —
    /// `tasks::validate_id` refuses `""` before any lookup, so `tasks forget`
    /// cannot reach it either — and the line has to say that rather than look
    /// like a value went missing on the way to the screen.
    #[test]
    fn an_unreadable_row_with_no_readable_id_still_reads_as_something() {
        let lines = task_lines(
            1_700_000_000_000,
            &[],
            &[UnknownTask {
                id: String::new(),
                reason: "unreadable task row: Invalid column type".to_owned(),
            }],
        );
        let rendered = lines.join("\n");
        assert!(
            !rendered.starts_with(':'),
            "a bare colon is a value that went missing, not an answer: {rendered}"
        );
        assert!(
            rendered.contains("id will not read"),
            "the line says what is unknowable about it: {rendered}"
        );
        assert!(
            rendered.contains("Invalid column type"),
            "and still carries the reason: {rendered}"
        );
    }

    #[test]
    fn the_three_envelopes_are_the_contract_key_by_key() {
        let task = a_task("nightly");
        let run = a_run("nightly", TaskOutcome::Ok);
        let rendered = task_json(&task, None, Some(&run));

        let listing = tasks_list_document(
            vec![rendered.clone()],
            &[UnknownTask {
                id: "teleport-1".to_owned(),
                reason: "kind 'teleport'".to_owned(),
            }],
        );
        assert_eq!(sorted_keys(&listing), vec!["tasks", "unknown"]);
        assert_eq!(listing["tasks"], serde_json::json!([rendered]));
        assert_eq!(
            listing["unknown"],
            serde_json::json!([{ "id": "teleport-1", "reason": "kind 'teleport'" }])
        );

        let status = task_status_document(rendered.clone(), vec![task_run_json(&run)]);
        assert_eq!(sorted_keys(&status), vec!["runs", "task"]);
        assert_eq!(status["task"], rendered);
        assert_eq!(status["runs"], serde_json::json!([task_run_json(&run)]));

        let performed = task_run_document(
            "nightly",
            Some(task_run_json(&run)),
            run_outcome_word(&run),
            EXIT_OK,
        );
        assert_eq!(
            sorted_keys(&performed),
            vec!["exit", "outcome", "run", "task"]
        );
        assert_eq!(performed["task"], serde_json::json!("nightly"));
        assert_eq!(performed["run"], task_run_json(&run));
        assert_eq!(performed["outcome"], serde_json::json!("ok"));
        assert_eq!(performed["exit"], serde_json::json!(0));

        // A lease held elsewhere opened no run of ours. Same key set, `run`
        // null — a real value ("there is no row"), not "nobody asked".
        let lost = task_run_document("nightly", None, "busy", EXIT_DEFERRED);
        assert_eq!(sorted_keys(&lost), vec!["exit", "outcome", "run", "task"]);
        assert_eq!(lost["run"], serde_json::Value::Null);
        assert_eq!(lost["outcome"], serde_json::json!("busy"));
        assert_eq!(lost["exit"], serde_json::json!(4));

        let saved = task_saved_document(rendered.clone());
        assert_eq!(sorted_keys(&saved), vec!["task"]);
        assert_eq!(saved["task"], rendered);

        let forgotten = task_forgotten_document("nightly");
        assert_eq!(sorted_keys(&forgotten), vec!["forgot"]);
        assert_eq!(forgotten["forgot"], serde_json::json!("nightly"));
    }

    #[test]
    fn a_countdown_is_one_coarse_unit_at_every_boundary() {
        let now = 1_700_000_000_000i64;
        // Under a minute in either direction is not worth a unit.
        assert_eq!(relative_ms(now, now), "now");
        assert_eq!(relative_ms(now, now + 59_999), "now");
        assert_eq!(relative_ms(now, now - 59_999), "now");
        // Minutes.
        assert_eq!(relative_ms(now, now + 60_000), "in 1m");
        assert_eq!(relative_ms(now, now - 60_000), "1m ago");
        assert_eq!(relative_ms(now, now - 12 * 60_000), "12m ago");
        assert_eq!(relative_ms(now, now + 3_599_999), "in 59m");
        // Hours.
        assert_eq!(relative_ms(now, now + 3_600_000), "in 1h");
        assert_eq!(relative_ms(now, now + 4 * 3_600_000), "in 4h");
        assert_eq!(relative_ms(now, now + 86_399_999), "in 23h");
        // Days, and no unit above them: a schedule slower than a year is
        // refused at the write door, so weeks would be a unit nothing produces.
        assert_eq!(relative_ms(now, now + 86_400_000), "in 1d");
        assert_eq!(relative_ms(now, now - 30 * 86_400_000), "30d ago");
        // A clock that moved backwards past the epoch must not overflow.
        assert_eq!(relative_ms(i64::MAX, i64::MIN), "106751991167d ago");
    }

    #[test]
    fn the_human_listing_is_honest_when_there_is_nothing_to_show() {
        let now = 1_700_000_000_000i64;
        let lines = task_lines(now, &[], &[]);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].contains("No tasks"), "{lines:?}");

        // An unreadable row is a LINE, not a silence: a folder with one task
        // this build cannot run must not read as a folder with no tasks.
        let lines = task_lines(
            now,
            &[],
            &[UnknownTask {
                id: "teleport-1".to_owned(),
                reason: "kind 'teleport' is not one this keeper can run".to_owned(),
            }],
        );
        assert!(
            lines.iter().any(|line| line.contains("teleport-1")
                && line.contains("cannot read")
                && line.contains("kind 'teleport'")),
            "{lines:?}"
        );
    }

    #[test]
    fn a_rendered_task_line_names_its_target_its_schedule_and_its_last_run() {
        let now = 1_700_000_000_000i64;
        let mut task = a_task("nightly");
        task.mode = TaskMode::Scheduled;
        task.schedule = Some("0 3 * * *".to_owned());
        task.next_due_ms = Some(now + 4 * 3_600_000);
        let last = a_run("nightly", TaskOutcome::Deferred);

        let host_wide = task_lines(
            now,
            &[TaskView {
                task: &task,
                profile_name: None,
                last: Some(&last),
            }],
            &[],
        );
        let joined = host_wide.join("\n");
        assert!(joined.contains("nightly"), "{joined}");
        assert!(joined.contains("sync"), "{joined}");
        assert!(joined.contains("scheduled"), "{joined}");
        assert!(joined.contains("host-wide"), "{joined}");
        assert!(joined.contains("0 3 * * *"), "{joined}");
        assert!(joined.contains("in 4h"), "{joined}");
        assert!(joined.contains("deferred"), "{joined}");
        assert!(!joined.contains("[disabled]"), "{joined}");

        // Story 58.5: a declined window has to be READABLE, or it is not a fact.
        // Both renderings reach it through `TaskOutcome::as_str`, so this asserts
        // the word and the detail actually land rather than trusting that they
        // must — and `task_exit_code` puts it beside `Busy` and `Deferred`,
        // because all three answer "the work did not happen, and nothing is
        // wrong".
        let mut declined = a_run("nightly", TaskOutcome::Declined);
        declined.finished_ms = Some(declined.started_ms);
        declined.detail =
            Some("skip: the window at 1700000000000 was not run, and 1700000300000 is armed in its place".to_owned());
        let history = task_run_lines(now, "nightly", std::slice::from_ref(&declined)).join("\n");
        assert!(history.contains("declined"), "{history}");
        assert!(history.contains("was not run"), "{history}");
        assert_eq!(run_outcome_word(&declined), "declined");
        assert_eq!(task_exit_code(TaskOutcome::Declined), EXIT_DEFERRED);

        // And its twin, which a reader must be able to tell apart from it: one
        // window will never be served, the other will be. Same exit code,
        // because to a wrapper script both mean "the work did not happen".
        let mut postponed = a_run("nightly", TaskOutcome::Postponed);
        postponed.finished_ms = Some(postponed.started_ms);
        postponed.detail = Some(
            "delay: the window at 1700000000000 is held back, and will be run at 1700001800000"
                .to_owned(),
        );
        let held = task_run_lines(now, "nightly", std::slice::from_ref(&postponed)).join("\n");
        assert!(held.contains("postponed"), "{held}");
        assert!(
            held.contains("will be run at"),
            "the one fact that separates a postponement from a decline has to \
             survive into the rendering: {held}"
        );
        assert_eq!(run_outcome_word(&postponed), "postponed");
        assert_eq!(task_exit_code(TaskOutcome::Postponed), EXIT_DEFERRED);
        let row = task_lines(
            now,
            &[TaskView {
                task: &task,
                profile_name: None,
                last: Some(&declined),
            }],
            &[],
        )
        .join("\n");
        assert!(
            row.contains("last: declined"),
            "the last-run line moves, which is the whole point of 58.5: {row}"
        );

        // Bound AND disabled. The binding has to be set for the folder's name
        // to be the target at all: `profile_id: None` is host-wide whatever
        // name is passed beside it, which is the renderer refusing to invent a
        // folder for a task that belongs to the machine.
        task.enabled = false;
        task.profile_id = Some("01DOCS".to_owned());
        let paused = task_lines(
            now,
            &[TaskView {
                task: &task,
                profile_name: Some("docs"),
                last: None,
            }],
            &[],
        )
        .join("\n");
        assert!(paused.contains("docs"), "{paused}");
        assert!(paused.contains("[disabled]"), "{paused}");
        assert!(
            paused.contains("never run"),
            "a task with no history says so rather than showing an empty column: {paused}"
        );

        // A binding whose folder this engine does not have: named, never
        // silently rendered as host-wide.
        let dangling = task_lines(
            now,
            &[TaskView {
                task: &task,
                profile_name: None,
                last: None,
            }],
            &[],
        )
        .join("\n");
        assert!(dangling.contains("no such folder: 01DOCS"), "{dangling}");
        assert!(!dangling.contains("host-wide"), "{dangling}");
    }

    #[test]
    fn the_history_rendering_is_honest_when_there_is_none() {
        let now = 1_700_000_000_000i64;
        let empty = task_run_lines(now, "nightly", &[]);
        assert_eq!(empty.len(), 1, "{empty:?}");
        assert!(empty[0].contains("no runs"), "{empty:?}");

        let mut newer = a_run("nightly", TaskOutcome::Ok);
        newer.outcome = None;
        newer.unknown_outcome = Some("teleported".to_owned());
        let lines = task_run_lines(
            now,
            "nightly",
            &[a_run("nightly", TaskOutcome::Failed), newer],
        );
        let joined = lines.join("\n");
        assert!(joined.contains("failed"), "{joined}");
        assert!(joined.contains("server-a"), "{joined}");
        assert!(
            joined.contains("teleported"),
            "a spelling this build cannot read is still a run that happened: {joined}"
        );
    }

    /// End to end over a real engine: the verb's own path, and the row it left.
    ///
    /// The claim worth making here is the one no pure test can: a run this CLI
    /// requested is recorded exactly as a scheduled one is — closed, with a
    /// host, an outcome and a detail — and the number the verb returns is the
    /// mapped one. A host-wide `sync` task on a box with no folders configured
    /// is the smallest shape that reaches `claim_and_run` and comes back `Ok`.
    #[tokio::test]
    async fn a_requested_run_is_recorded_and_its_exit_code_is_the_mapped_one() {
        let dir = tempfile::tempdir().expect("temp dir");
        let platform = Arc::new(keeper_sync::platform::TestPlatform::new(dir.path()));
        // The idiom `keeper-sync`'s own tests use: a box with no usable git
        // cannot open an engine, and that is not this test's claim.
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let mut task = a_task("nightly");
        task.updated_ms = platform.now_ms();
        engine
            .save_task(&task, None)
            .expect("a host-wide sync task");

        // `Printer::new(false)` prints the human form to this test's stdout,
        // which is exactly what `cargo test` captures and discards.
        let printer = Printer::new(false);
        let code = cmd_task_run(
            &printer,
            &engine,
            platform.now_ms(),
            "nightly",
            TaskRunDriver::Person,
        )
        .await
        .expect("the verb itself must not fail");
        assert_eq!(code, EXIT_OK);

        let runs = engine.task_history("nightly", 10).expect("history");
        let [run] = &runs[..] else {
            panic!("exactly one run, got {}", runs.len());
        };
        assert_eq!(run.task_id, "nightly");
        assert_eq!(run.outcome, Some(TaskOutcome::Ok));
        assert!(run.unknown_outcome.is_none());
        assert!(
            run.finished_ms.is_some(),
            "a requested run is closed by the host that opened it, exactly as a \
             scheduled one is"
        );
        assert!(!run.host.is_empty(), "the host is recorded either way");
        assert_eq!(run.detail.as_deref(), Some("no folders to sync"));
        assert_eq!(
            task_exit_code(run.outcome.expect("an outcome")),
            code,
            "the number the verb returned is the recorded outcome's own"
        );

        let listing = engine.tasks().expect("tasks");
        let stored = select_task(&listing, "nightly").expect("the row survives");
        assert!(
            stored.running_host.is_none() && stored.lease_until_ms.is_none(),
            "the lease is released"
        );
        assert_eq!(
            stored.next_due_ms, None,
            "asking for a run now is not asking to skip the next one"
        );
    }

    /// Every flag `tasks set` can be given, against a real store.
    ///
    /// The claims a pure test cannot make: what `--kind`-on-create refuses,
    /// which columns an omitted flag preserves, that the two clearing flags
    /// really clear, and that `set` cannot resurrect a task somebody disabled.
    #[test]
    fn set_requires_a_kind_to_create_and_preserves_everything_omitted_to_update() {
        let dir = tempfile::tempdir().expect("temp dir");
        let platform = Arc::new(keeper_sync::platform::TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let printer = Printer::new(false);
        let now = platform.now_ms();
        let stored = |id: &str| -> TaskRow {
            let listing = engine.tasks().expect("tasks");
            select_task(&listing, id).expect("a stored row").clone()
        };

        // A folder to bind to, so `--profile` and `--host-wide` are exercised
        // against a real selector rather than against a literal id.
        let folder = SyncProfile::new(
            "01DOCS",
            "docs",
            dir.path().join("docs"),
            "https://example.com/docs.git",
        );
        engine.upsert_profile(&folder).expect("a folder");

        // 1. Create with no kind: refused, quoting the id and naming the flag.
        let err = cmd_task_set(&printer, &engine, now, &set_args("nightly"))
            .expect_err("a new task must name its kind");
        assert_eq!(err.exit_code(), EXIT_CONFIG);
        assert!(err.to_string().contains("nightly"), "{err}");
        assert!(err.to_string().contains("--kind"), "{err}");

        // 2. Create with a schedule: `mode` derives to `scheduled`, and
        //    `enabled` is true because nothing stored says otherwise.
        let mut args = set_args("nightly");
        args.kind = Some(TaskKindArg::Release);
        args.schedule = Some("every 30m".to_owned());
        assert_eq!(
            cmd_task_set(&printer, &engine, now, &args).expect("create"),
            EXIT_OK
        );
        let row = stored("nightly");
        assert_eq!(row.kind, TaskKind::Release);
        assert_eq!(row.mode, TaskMode::Scheduled);
        assert_eq!(row.schedule.as_deref(), Some("every 30m"));
        assert!(row.enabled);
        assert_eq!(row.profile_id, None, "no --profile means host-wide");

        // 3. Create WITHOUT a schedule: `manual`, because `upsert_task` refuses
        //    a scheduled row with nothing to schedule — the one derived default
        //    that cannot produce a refused row.
        let mut args = set_args("adhoc");
        args.kind = Some(TaskKindArg::Sync);
        assert_eq!(
            cmd_task_set(&printer, &engine, now, &args).expect("create"),
            EXIT_OK
        );
        assert_eq!(stored("adhoc").mode, TaskMode::Manual);

        // 4. Update naming nothing: every stored value survives.
        assert_eq!(
            cmd_task_set(&printer, &engine, now, &set_args("nightly")).expect("update"),
            EXIT_OK
        );
        let row = stored("nightly");
        assert_eq!(row.kind, TaskKind::Release);
        assert_eq!(row.mode, TaskMode::Scheduled);
        assert_eq!(row.schedule.as_deref(), Some("every 30m"));

        // 5. `--profile` binds through `select`, by NAME as an operator types it.
        let mut args = set_args("nightly");
        args.profile = Some("docs".to_owned());
        assert_eq!(
            cmd_task_set(&printer, &engine, now, &args).expect("bind"),
            EXIT_OK
        );
        assert_eq!(stored("nightly").profile_id.as_deref(), Some("01DOCS"));
        // ...and a selector nothing matches is refused rather than silently
        // binding the task to nothing.
        let mut args = set_args("nightly");
        args.profile = Some("nope".to_owned());
        assert_eq!(
            cmd_task_set(&printer, &engine, now, &args)
                .expect_err("no such folder")
                .exit_code(),
            EXIT_CONFIG
        );

        // 6. `--host-wide` unbinds it again. A knob that could only be set is
        //    the dead knob this epic exists to close.
        let mut args = set_args("nightly");
        args.host_wide = true;
        assert_eq!(
            cmd_task_set(&printer, &engine, now, &args).expect("unbind"),
            EXIT_OK
        );
        assert_eq!(stored("nightly").profile_id, None);

        // 7. `--no-schedule` clears the expression. The mode has to come down
        //    with it, and the write door is what says so.
        let mut args = set_args("nightly");
        args.no_schedule = true;
        args.mode = Some(TaskModeArg::Manual);
        assert_eq!(
            cmd_task_set(&printer, &engine, now, &args).expect("clear the schedule"),
            EXIT_OK
        );
        let row = stored("nightly");
        assert_eq!(row.schedule, None);
        assert_eq!(row.mode, TaskMode::Manual);

        // 8. `disable` touches one column, and `set` must not undo it: writing
        //    `enabled` from a settings-shaped update is exactly how a restart
        //    un-pauses everything somebody paused.
        assert_eq!(
            cmd_task_set_enabled(&printer, &engine, now, "nightly", false).expect("disable"),
            EXIT_OK
        );
        let row = stored("nightly");
        assert!(!row.enabled);
        assert_eq!(row.kind, TaskKind::Release, "and nothing else moved");
        assert_eq!(
            cmd_task_set(&printer, &engine, now, &set_args("nightly")).expect("update"),
            EXIT_OK
        );
        assert!(
            !stored("nightly").enabled,
            "`set` must not resurrect a disabled task"
        );
        assert_eq!(
            cmd_task_set_enabled(&printer, &engine, now, "nightly", true).expect("enable"),
            EXIT_OK
        );
        assert!(stored("nightly").enabled);

        // 9. `forget` refuses an id nobody stored, and takes the row when it is
        //    real.
        assert_eq!(
            cmd_task_forget(&printer, &engine, "nope")
                .expect_err("no such task")
                .exit_code(),
            EXIT_CONFIG
        );
        assert_eq!(
            cmd_task_forget(&printer, &engine, "nightly").expect("forget"),
            EXIT_OK
        );
        let listing = engine.tasks().expect("tasks");
        assert!(select_task(&listing, "nightly").is_err(), "the row is gone");

        // 10. And both read verbs run over a real store, so a rendering that
        //     panics on a stored row is caught here rather than by an operator.
        assert_eq!(
            cmd_task_list(&printer, &engine, now).expect("list"),
            EXIT_OK
        );
        assert_eq!(
            cmd_task_status(&printer, &engine, now, "adhoc").expect("status"),
            EXIT_OK
        );
    }

    /// `--on-missed` is writable from the CLI, keeps its stored value when
    /// omitted, and reaches both renderings (Story 58.4, FR-356, AD-139).
    ///
    /// Both surfaces are asserted because the policy has to be **reachable and
    /// readable** from each of them: a knob writable from neither is born
    /// unreachable, which is the defect class this epic exists to close, and a
    /// knob nobody can read back is one nobody can verify took. The human line
    /// carries a marker only when the setting is not the default — `run_now` is
    /// what every row already did — and the `--json` document carries it always,
    /// because a fixed key set is that document's contract.
    #[test]
    fn the_missed_window_policy_is_writable_from_the_cli_and_read_back_by_both_renderings() {
        let dir = tempfile::tempdir().expect("temp dir");
        let platform = Arc::new(keeper_sync::platform::TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let printer = Printer::new(false);
        let now = platform.now_ms();
        let stored = |id: &str| -> TaskRow {
            let listing = engine.tasks().expect("tasks");
            select_task(&listing, id).expect("a stored row").clone()
        };
        let rendered = |id: &str| -> (String, serde_json::Value) {
            let row = stored(id);
            let view = TaskView {
                task: &row,
                profile_name: None,
                last: None,
            };
            (
                task_lines(now, std::slice::from_ref(&view), &[]).join("\n"),
                task_json(&row, None, None),
            )
        };

        // Create without the flag: the default is what an ordinary restart
        // already did, so an older script means what it always meant.
        let mut args = set_args("nightly");
        args.kind = Some(TaskKindArg::Sync);
        args.schedule = Some("0 3 * * *".to_owned());
        cmd_task_set(&printer, &engine, now, &args).expect("create");
        assert_eq!(stored("nightly").on_missed, TaskMissedPolicy::RunNow);
        let (line, document) = rendered("nightly");
        assert!(
            !line.contains("on missed"),
            "the default is the absence of a marker, exactly as `enabled` is: {line}"
        );
        assert_eq!(
            document["onMissed"], "run_now",
            "but the document says so on every row"
        );

        // Set it, and read it back from both.
        args.on_missed = Some(TaskMissedArg::Skip);
        cmd_task_set(&printer, &engine, now, &args).expect("set the policy");
        assert_eq!(stored("nightly").on_missed, TaskMissedPolicy::Skip);
        let (line, document) = rendered("nightly");
        assert!(
            line.contains("[on missed: skip]"),
            "a non-default policy is visible in the human rendering: {line}"
        );
        assert_eq!(document["onMissed"], "skip");

        // Omitted keeps stored, which is the rule every other flag here follows:
        // a caller changing a schedule must not silently reset a policy.
        let mut only_schedule = set_args("nightly");
        only_schedule.schedule = Some("0 4 * * *".to_owned());
        cmd_task_set(&printer, &engine, now, &only_schedule).expect("change the schedule");
        assert_eq!(
            stored("nightly").on_missed,
            TaskMissedPolicy::Skip,
            "an omitted flag keeps what was stored"
        );
        assert_eq!(stored("nightly").schedule.as_deref(), Some("0 4 * * *"));

        // And it is settable back, because a knob you can set and never unset is
        // the dead knob this epic exists to close.
        args.schedule = Some("0 4 * * *".to_owned());
        args.on_missed = Some(TaskMissedArg::RunNow);
        cmd_task_set(&printer, &engine, now, &args).expect("back to the default");
        assert_eq!(stored("nightly").on_missed, TaskMissedPolicy::RunNow);

        // The kebab-case flag and the stored spelling are the one place this
        // vocabulary diverges, and the conversion is asserted so a third
        // spelling cannot appear.
        for (arg, policy) in [
            (TaskMissedArg::RunNow, TaskMissedPolicy::RunNow),
            (TaskMissedArg::Delay, TaskMissedPolicy::Delay),
            (TaskMissedArg::Skip, TaskMissedPolicy::Skip),
        ] {
            assert_eq!(TaskMissedPolicy::from(arg), policy);
            assert_eq!(
                TaskMissedPolicy::from_stored(policy.as_str()),
                Some(policy),
                "and the stored spelling this writes is one the store reads back"
            );
        }
    }

    /// A stored task cannot be repurposed into another kind.
    ///
    /// Two things would go wrong if it could, and neither is cosmetic. The
    /// armed window survives — `upsert_task` clears it only when the schedule
    /// *text* moves — so a `sync` task whose window is already open becomes a
    /// `release` task that **deletes** on the very next tick, at an instant
    /// nobody armed for a deletion. And the run history survives, so a sync's
    /// `1 synced, 0 already syncing…` renders as a release task's own record.
    #[test]
    fn a_stored_tasks_kind_cannot_be_changed_out_from_under_its_window_and_history() {
        let dir = tempfile::tempdir().expect("temp dir");
        let platform = Arc::new(keeper_sync::platform::TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let printer = Printer::new(false);
        let now = platform.now_ms();

        let mut args = set_args("nightly");
        args.kind = Some(TaskKindArg::Sync);
        args.schedule = Some("every 30m".to_owned());
        cmd_task_set(&printer, &engine, now, &args).expect("create a sync task");

        let mut repurpose = set_args("nightly");
        repurpose.kind = Some(TaskKindArg::Release);
        let err = cmd_task_set(&printer, &engine, now, &repurpose)
            .expect_err("a task's kind is not a knob");
        assert_eq!(err.exit_code(), EXIT_CONFIG);
        let message = err.to_string();
        assert!(message.contains("nightly"), "{message}");
        assert!(
            message.contains("sync"),
            "the refusal names what it IS stored as: {message}"
        );
        assert!(
            message.contains("forget"),
            "and the one instruction that works: {message}"
        );

        let listing = engine.tasks().expect("tasks");
        assert_eq!(
            select_task(&listing, "nightly").expect("the row").kind,
            TaskKind::Sync,
            "and nothing was written"
        );

        // Naming the kind it already has is not a change, so it is not refused:
        // a wrapper script that always passes `--kind` still works.
        let mut same = set_args("nightly");
        same.kind = Some(TaskKindArg::Sync);
        assert_eq!(
            cmd_task_set(&printer, &engine, now, &same).expect("a no-op kind"),
            EXIT_OK
        );
    }

    /// A finished run is never rendered in the future tense.
    ///
    /// `run()` samples one instant before dispatching, and `tasks run` is the
    /// only verb that performs work between that sample and its render — so
    /// `run.started_ms` is always at or after the sample, and a sweep that took
    /// five minutes printed `in 5m` for work that had already finished. The
    /// render therefore measures from `now_ms.max(run.finished_ms)`.
    ///
    /// Both renderings are built from **one** run, so the only difference
    /// between them is the instant they are measured against — which is exactly
    /// the line `cmd_task_run` changed.
    #[test]
    fn a_run_that_took_minutes_is_not_printed_as_having_just_started() {
        let now = 1_700_000_000_000i64;
        // A five-minute run. `claim_and_run` opens it a moment after `run()`
        // read the clock, so `started_ms` sits just *after* the sample — which
        // is what makes the stale rendering say "now" however long the run took.
        let mut run = a_run("nightly", TaskOutcome::Ok);
        run.started_ms = now + 20;
        run.finished_ms = Some(run.started_ms + 5 * 60_000);

        let stale = task_run_lines(now, "nightly", std::slice::from_ref(&run)).join("\n");
        assert!(
            stale.contains("  now  "),
            "the defect has to be reproducible or the fix proves nothing: {stale}"
        );

        let rendered_at = now.max(run.finished_ms.expect("a closed run"));
        let honest = task_run_lines(rendered_at, "nightly", &[run]).join("\n");
        assert!(
            honest.contains("5m ago"),
            "a run that finished five minutes later started five minutes before \
             the render: {honest}"
        );
    }

    // -----------------------------------------------------------------------
    // packaging (Story 57.7)
    // -----------------------------------------------------------------------

    /// The daemon's own unit, the posture every other unit here copies.
    const DAEMON_UNIT_FILE: &str = "keeper-syncd.service";
    /// The one-shot that calls `tasks run`.
    const TASK_SERVICE_FILE: &str = "keeper-syncd-tasks@.service";
    /// The timer that triggers it.
    const TASK_TIMER_FILE: &str = "keeper-syncd-tasks@.timer";

    /// One shipped unit file, read from `packaging/` beside this crate.
    fn unit_file(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("packaging")
            .join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("packaging/{name} must be readable: {err}"))
    }

    /// A unit file as `(section, key, value)` triples, in file order.
    ///
    /// **A parse, not a substring search**, and that distinction is the whole
    /// reason this exists: `systemd-analyze verify` is not installed on the host
    /// these files were written on (nor is `systemctl` — this crate's own tests
    /// are the only local gate the packaging has), so anything that is not a
    /// comment, a blank line, a `[Section]` header or a `Key=Value` **inside** a
    /// section fails the test rather than being skipped over.
    ///
    /// Duplicates are kept rather than folded into a map: systemd allows a key
    /// to repeat, and a test that silently kept the last one could not notice a
    /// second `ExecStart=` somebody added below the first.
    fn parse_unit(name: &str) -> Vec<(String, String, String)> {
        let text = unit_file(name);
        let mut section: Option<String> = None;
        let mut entries = Vec::new();
        for (index, raw) in text.lines().enumerate() {
            let line = raw.trim();
            let number = index + 1;
            // Asserted rather than implemented: a continuation would make every
            // claim below read only half a value, and none of these files needs
            // one.
            assert!(
                !line.ends_with('\\'),
                "{name}:{number}: this parser does not implement systemd's line \
                 continuation, so the file must not use one"
            );
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            if let Some(inner) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                assert!(
                    !inner.is_empty(),
                    "{name}:{number}: an empty section header"
                );
                section = Some(inner.to_owned());
                continue;
            }
            let (key, value) = line
                .split_once('=')
                .unwrap_or_else(|| panic!("{name}:{number}: not a Key=Value line: {line:?}"));
            let owner = section
                .clone()
                .unwrap_or_else(|| panic!("{name}:{number}: {key} sits outside any section"));
            entries.push((owner, key.trim().to_owned(), value.trim().to_owned()));
        }
        entries
    }

    /// Every value stored under one section's key, in file order.
    fn unit_values<'a>(
        entries: &'a [(String, String, String)],
        section: &str,
        key: &str,
    ) -> Vec<&'a str> {
        entries
            .iter()
            .filter(|(found_section, found_key, _)| found_section == section && found_key == key)
            .map(|(_, _, value)| value.as_str())
            .collect()
    }

    /// The one value under a section's key, asserting there is exactly one.
    fn unit_value<'a>(
        entries: &'a [(String, String, String)],
        section: &str,
        key: &str,
    ) -> &'a str {
        let found = unit_values(entries, section, key);
        assert_eq!(
            found.len(),
            1,
            "expected exactly one {section}/{key}, got {found:?}"
        );
        found[0]
    }

    /// Which sections a unit file declares, deduplicated in first-seen order.
    fn unit_sections(entries: &[(String, String, String)]) -> Vec<&str> {
        let mut seen: Vec<&str> = Vec::new();
        for (section, _, _) in entries {
            if !seen.contains(&section.as_str()) {
                seen.push(section);
            }
        }
        seen
    }

    /// Both new files are well-formed unit files with the sections their type
    /// requires.
    ///
    /// A `.timer` with no `[Timer]` section, or a `.service` with no
    /// `[Service]`, is a file systemd refuses to load — and the symptom is a
    /// `daemon-reload` warning nobody is watching at the moment the schedule
    /// silently stops existing.
    #[test]
    fn the_shipped_task_units_are_well_formed_and_carry_the_sections_their_type_needs() {
        let service = parse_unit(TASK_SERVICE_FILE);
        assert_eq!(
            unit_sections(&service),
            vec!["Unit", "Service"],
            "a timer-driven one-shot has no [Install]: enabling the SERVICE \
             would run it once at login and never again, which looks like it \
             worked"
        );

        let timer = parse_unit(TASK_TIMER_FILE);
        assert_eq!(unit_sections(&timer), vec!["Unit", "Timer", "Install"]);
        assert_eq!(
            unit_value(&timer, "Install", "WantedBy"),
            "timers.target",
            "the timer is the unit an operator enables"
        );
        // Without this a laptop shut overnight silently skips the run it was
        // asleep for, which is the invisible-non-execution shape Epic 57 exists
        // to close.
        assert_eq!(unit_value(&timer, "Timer", "Persistent"), "true");
        assert!(
            !unit_values(&timer, "Timer", "OnCalendar").is_empty(),
            "a timer with no trigger fires never"
        );
    }

    /// The one-shot calls a verb this binary actually has, with the flags it
    /// actually has.
    ///
    /// The failure this prevents is a unit that fails silently at 3 a.m.
    /// `ExecStart=` is prose as far as Rust is concerned and nothing else in
    /// this repository reads it, so it is fed to the **real** clap parser here —
    /// and the parse has to come out as `tasks run <the instance name>` rather
    /// than merely as something clap tolerates. A rename of the verb, of the
    /// subcommand, or of the positional breaks this test in the same commit that
    /// made the unit wrong.
    #[test]
    fn the_shipped_task_service_runs_a_verb_this_binary_has() {
        let service = parse_unit(TASK_SERVICE_FILE);
        let exec = unit_value(&service, "Service", "ExecStart");
        assert_eq!(
            unit_value(&service, "Service", "Type"),
            "oneshot",
            "one run per trigger, and no second scheduler (AD-136)"
        );

        // The two specifiers systemd expands before it ever runs this. `%i` is
        // the instance name **verbatim**: a task id normally contains hyphens
        // (`release-nightly`), and `%I` would unescape those into slashes and ask
        // keeper for a task nobody ever stored.
        assert!(
            !exec.contains("%I"),
            "%I would turn a hyphenated task id into a path: {exec}"
        );

        // **Sentinels, not plausible values.** Substituting `%i` with `nightly`
        // would let a unit that had DROPPED `%i` and hard-coded `nightly`
        // produce a byte-identical argv and pass every assertion below — and
        // that unit is the silent 3 a.m. failure this test exists to catch, with
        // `keeper-syncd-tasks@weekly.service` sweeping `nightly` forever. A
        // string no author would type is what makes the assertion about the
        // template rather than about the string.
        const HOME: &str = "/home/specifier-h-under-test";
        const INSTANCE: &str = "specifier-i-under-test";

        // Split FIRST, then expand each word — which is the order systemd uses.
        // Expanding first and splitting after models a different program: an
        // instance name containing a space would be one argument here and two
        // there, and the whole point of this test is to build the argv systemd
        // builds.
        let argv: Vec<String> = exec
            .split_whitespace()
            .map(|word| word.replace("%h", HOME).replace("%i", INSTANCE))
            .collect();
        assert!(
            argv.iter().all(|word| !word.contains('%')),
            "an unexpanded specifier would reach argv verbatim: {argv:?}"
        );
        assert_eq!(
            argv.first().map(String::as_str),
            Some(&*format!("{HOME}/.local/bin/keeper-syncd")),
            "a user unit runs the binary the user installed under $HOME: {argv:?}"
        );

        // clap reads argv[0] as the program name, which is exactly what that is.
        let borrowed: Vec<&str> = argv.iter().map(String::as_str).collect();
        let cli = parse(&borrowed)
            .unwrap_or_else(|err| panic!("{argv:?} must parse as a keeper-syncd command:\n{err}"));
        match cli.command {
            Command::Tasks {
                // `timer: true` is an assertion, not a pattern that happens to
                // fit (Story 58.6). The flag is what tells keeper a SCHEDULED
                // driver is asking, and nothing about the process can reveal
                // that — a person types the same argv. Dropping it from
                // `ExecStart=` would silently restore the two-runs-for-one-
                // missed-window defect on every box running this timer beside
                // `keeper-syncd watch`, and no other test in the tree reads this
                // line.
                command: TaskCommand::Run { task, timer: true },
            } => assert_eq!(
                task, INSTANCE,
                "the unit must ask for the task its instance names: {argv:?}"
            ),
            other => panic!("{argv:?} must be `tasks run --timer <task>`, not {other:?}"),
        }
    }

    /// A deferral is not a failed unit, and only `SuccessExitStatus=` says so.
    ///
    /// `RestartPreventExitStatus=` suppresses the *restart*; it leaves systemd's
    /// own verdict alone. Without this line every night an external drive is out
    /// ends with the instance in `failed` — red in `systemctl --user status`,
    /// listed in `systemctl --user --failed`, firing any `OnFailure=` hook — which
    /// is the nightly alert nobody reads, raised by systemd itself, in the one
    /// unit whose whole premise is that a deferral must not raise one. It also
    /// breaks `docs/sync.md` §14's own install step 5, which starts the service
    /// directly to prove the install works.
    #[test]
    fn the_shipped_task_service_does_not_call_a_deferral_a_failure() {
        let service = parse_unit(TASK_SERVICE_FILE);
        let success: Vec<u8> = unit_value(&service, "Service", "SuccessExitStatus")
            .split_whitespace()
            .map(|word| word.parse().expect("an exit status is a number"))
            .collect();
        assert_eq!(
            success,
            vec![EXIT_DEFERRED],
            "exactly the deferral: 2 and 3 stay genuine failures, and 0 needs no \
             mention"
        );
    }

    /// One `[Service]` key that is absent is worth a test of its own.
    ///
    /// `RemainAfterExit=yes` would leave this instance `active` after its first
    /// run, and systemd does not start a unit that is already active — so every
    /// later trigger becomes a silent no-op and the schedule stops after exactly
    /// one run, looking for all the world like it worked. Nothing else in this
    /// module constrains a key it does not name, so the one whose default must
    /// not change is named here.
    #[test]
    fn the_shipped_task_service_does_not_stay_active_after_its_run() {
        let service = parse_unit(TASK_SERVICE_FILE);
        assert!(
            unit_values(&service, "Service", "RemainAfterExit").is_empty(),
            "RemainAfterExit would make every trigger after the first a no-op"
        );
    }

    /// The unit never retries a number a retry cannot help — and still retries
    /// the one it can.
    ///
    /// `EXIT_DEFERRED`'s own doc comment names this story and asks for exactly
    /// this list. Written against the constants rather than against the literals
    /// so renumbering the taxonomy breaks the packaging here rather than in
    /// somebody's journal: a `4` left out of this list makes an unplugged drive
    /// retry every minute forever, which is AD-48's *absence, never failure*
    /// turned into the alert nobody reads.
    #[test]
    fn the_shipped_task_service_refuses_to_retry_what_a_retry_cannot_fix() {
        let service = parse_unit(TASK_SERVICE_FILE);
        assert_eq!(
            unit_value(&service, "Service", "Restart"),
            "on-failure",
            "the policy this unit wants; Type=oneshot refuses only `always` and \
             `on-success`, and needs systemd 244+ to accept any Restart= at all"
        );

        let mut prevented: Vec<u8> = unit_value(&service, "Service", "RestartPreventExitStatus")
            .split_whitespace()
            .map(|word| word.parse().expect("an exit status is a number"))
            .collect();
        prevented.sort_unstable();
        // Exhaustive, so this covers both directions at once: `1` and `0` are
        // absent because they are not in the list, and stating them separately
        // below an `assert_eq!` would only look like an independent guard.
        // `1` is the restartable one — the work ran and failed, often on a
        // transient remote, and the next OnCalendar may be a day away.
        assert_eq!(
            prevented,
            vec![EXIT_CONFIG, EXIT_PREREQUISITE, EXIT_DEFERRED],
            "exactly the three numbers a restart cannot help — never {EXIT_FAILURE} \
             (restartable) and never {EXIT_OK} (not a restart condition at all)"
        );

        // **The values, not their presence.** `StartLimitBurst=0` disables
        // systemd's start rate limiting entirely, and so does
        // `StartLimitIntervalSec=0` — either one turns `RestartSec=60` into the
        // unbounded every-minute loop this bound exists to prevent, while
        // passing any assertion that only checks the key is set. These two
        // numbers are also quoted in `docs/sync.md` §14 ("three attempts in ten
        // minutes"), and the chapter test below is what keeps that in step.
        assert_eq!(
            unit_value(&service, "Unit", "StartLimitIntervalSec"),
            "600",
            "ten minutes; 0 would disable rate limiting"
        );
        assert_eq!(
            unit_value(&service, "Unit", "StartLimitBurst"),
            "3",
            "three attempts; 0 would disable rate limiting"
        );
    }

    /// The cadence lives in the timer and the work lives in the verb — one of
    /// each, in exactly one file.
    ///
    /// **`tasks run` does not consult a task's schedule**: `TaskTrigger::
    /// Requested` passes `due_at_most: None` to `db::claim_task`, so a requested
    /// run performs the work unconditionally. The timer's `OnCalendar=` is
    /// therefore the real cadence for this driver, and the honest arrangement is
    /// one cadence in one place: the timer states when, the service states what,
    /// and neither states the other's half. A second `OnCalendar` in the service
    /// or an `ExecStart` in the timer would give one question two answers.
    ///
    /// What AD-136 actually forbids — a schedule with no task row behind it, so
    /// `tasks list` and the Tasks view have nothing to show — is prevented by
    /// `ExecStart` naming a stored task at all, which the test above asserts.
    #[test]
    fn the_shipped_pair_keeps_the_cadence_in_the_timer_and_the_work_in_the_verb() {
        let service = parse_unit(TASK_SERVICE_FILE);
        for cadence in [
            "OnCalendar",
            "OnUnitActiveSec",
            "OnBootSec",
            "OnStartupSec",
            "OnUnitInactiveSec",
            "OnActiveSec",
        ] {
            assert!(
                unit_values(&service, "Timer", cadence).is_empty()
                    && unit_values(&service, "Service", cadence).is_empty(),
                "{cadence} in the service would give one cadence two homes"
            );
        }

        let timer = parse_unit(TASK_TIMER_FILE);
        for verb in ["ExecStart", "ExecStartPre", "ExecStartPost", "ExecStop"] {
            assert!(
                unit_values(&timer, "Timer", verb).is_empty()
                    && unit_values(&timer, "Service", verb).is_empty(),
                "{verb} in a .timer is work in the file that states only when"
            );
        }
        // No `Unit=`: systemd derives `keeper-syncd-tasks@nightly.service` from
        // `keeper-syncd-tasks@nightly.timer` by name, and a written-out `Unit=`
        // is one more thing to keep in step with a rename.
        assert!(unit_values(&timer, "Timer", "Unit").is_empty());
    }

    /// `docs/sync.md` §14 quotes several lines out of these unit files verbatim,
    /// and this is what stops those quotations going stale.
    ///
    /// Story 56.13 shipped a `--help` describing behaviour that had already been
    /// replaced, and it survived review precisely because the prose sat a long
    /// way from the code. A documented `OnCalendar=` default is the same shape of
    /// claim: nothing in the build reads it, an operator plans around it, and the
    /// person who retunes the shipped timer has no reason to open a 2 500-line
    /// document.
    ///
    /// **Anchored to the sentence, not to any occurrence.** §14 also prints
    /// `OnCalendar=*-*-* 03:00:00` in its install block as the value an operator
    /// might choose, so an unanchored `contains("OnCalendar=…")` would go green
    /// the moment somebody retuned the shipped timer to that value — leaving the
    /// chapter's "the shipped default is" sentence stale, which is precisely what
    /// this test exists to prevent.
    #[test]
    fn the_chapter_quotes_the_units_as_they_are_actually_shipped() {
        let chapter = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../docs/sync.md")
                .canonicalize()
                .expect("docs/sync.md must be reachable from this crate"),
        )
        .expect("docs/sync.md must be readable");
        let quotes = |claim: String, why: &str| {
            assert!(
                chapter.contains(&claim),
                "docs/sync.md §14 must carry the sentence {claim:?} — {why}"
            );
        };

        let timer = parse_unit(TASK_TIMER_FILE);
        let cadence = unit_value(&timer, "Timer", "OnCalendar");
        quotes(
            format!("The shipped default is `OnCalendar={cadence}`"),
            "the cadence an operator plans around",
        );
        // The two directives that move a run away from the instant the operator
        // wrote, so the chapter has to name both with their real values.
        quotes(
            format!(
                "`RandomizedDelaySec={}`",
                unit_value(&timer, "Timer", "RandomizedDelaySec")
            ),
            "the jitter, which shifts every run by up to that much",
        );
        quotes(
            format!("`Persistent={}`", unit_value(&timer, "Timer", "Persistent")),
            "the boot catch-up, which runs a missed sweep at an unplanned hour",
        );

        let service = parse_unit(TASK_SERVICE_FILE);
        let prevented = unit_value(&service, "Service", "RestartPreventExitStatus");
        quotes(
            format!("`RestartPreventExitStatus={prevented}`"),
            "the statuses the unit refuses to retry",
        );
        quotes(
            format!(
                "`SuccessExitStatus={}`",
                unit_value(&service, "Service", "SuccessExitStatus")
            ),
            "what stops a deferral leaving the unit in `failed`",
        );
        // The retry ceiling, quoted as the chapter spells it — the two numbers
        // rather than a prose paraphrase, so retuning either one breaks the
        // sentence rather than leaving "three attempts in ten minutes" standing
        // over a unit that now says something else.
        for key in ["StartLimitBurst", "StartLimitIntervalSec"] {
            quotes(
                format!("`{key}={}`", unit_value(&service, "Unit", key)),
                "the retry ceiling an operator reasons about",
            );
        }

        // The systemd floor. `Restart=` on a `Type=oneshot` unit is refused
        // before v244, so an operator on an older distribution gets a unit that
        // never loads and a timer that fires onto nothing every night — the
        // invisible non-execution this epic exists to close. Both the unit
        // header and the chapter must name the same version, or bumping one
        // leaves the other advising a floor that is no longer true.
        const SYSTEMD_FLOOR: &str = "systemd 244";
        assert!(
            unit_file(TASK_SERVICE_FILE).contains(SYSTEMD_FLOOR),
            "the unit that needs it must name {SYSTEMD_FLOOR}"
        );
        quotes(
            SYSTEMD_FLOOR.to_owned(),
            "the version floor, which decides whether the unit loads at all",
        );
        // Both filenames appear in the chapter, so the install block cannot name
        // a file this crate does not ship.
        for name in [TASK_SERVICE_FILE, TASK_TIMER_FILE] {
            assert!(
                chapter.contains(name),
                "§14 must name {name}, the file an operator has to install"
            );
        }
    }

    /// The new units keep the daemon unit's posture — user-scoped, and hardened
    /// the same two ways.
    ///
    /// Read out of `keeper-syncd.service` rather than restated, so the two
    /// cannot drift: a hardening line added or dropped there is a failure here
    /// until somebody decides what it means for a task run.
    #[test]
    fn the_shipped_task_service_keeps_the_daemon_units_user_posture() {
        let daemon = parse_unit(DAEMON_UNIT_FILE);
        let service = parse_unit(TASK_SERVICE_FILE);

        for key in ["NoNewPrivileges", "PrivateTmp"] {
            assert_eq!(
                unit_value(&service, "Service", key),
                unit_value(&daemon, "Service", key),
                "{key} must say what the daemon's own unit says"
            );
        }
        assert_eq!(
            unit_value(&service, "Service", "WorkingDirectory"),
            unit_value(&daemon, "Service", "WorkingDirectory"),
        );

        // A **user** unit synchronizes the user's own files with the user's own
        // credentials (AD-52). A `User=` here would be a service account holding
        // somebody's token, and `Environment=` is visible to `systemctl show`
        // (AD-53).
        for forbidden in ["User", "Group", "Environment", "EnvironmentFile"] {
            for (file, entries) in [(DAEMON_UNIT_FILE, &daemon), (TASK_SERVICE_FILE, &service)] {
                assert!(
                    unit_values(entries, "Service", forbidden).is_empty(),
                    "{file} must not set {forbidden}"
                );
            }
        }

        // Deliberately absent in both: each would break a folder that is
        // perfectly valid. The daemon unit records why in prose; this keeps the
        // two files agreeing about it.
        for absent in [
            "ProtectHome",
            "ProtectSystem",
            "ReadOnlyPaths",
            "PrivateNetwork",
        ] {
            for (file, entries) in [(DAEMON_UNIT_FILE, &daemon), (TASK_SERVICE_FILE, &service)] {
                assert!(
                    unit_values(entries, "Service", absent).is_empty(),
                    "{file} must not set {absent}"
                );
            }
        }

        // `watch` installs a SIGTERM handler and finalizes under a bound, which
        // is why its unit sets a stop contract. `tasks run` installs no handler,
        // so stating one here would document a promise this verb does not make —
        // it does not need one, because a killed run is closed as `abandoned` by
        // the next host to reclaim its expired lease.
        for stop in ["KillSignal", "TimeoutStopSec"] {
            assert!(
                !unit_values(&daemon, "Service", stop).is_empty(),
                "the daemon unit still carries {stop}"
            );
            assert!(
                unit_values(&service, "Service", stop).is_empty(),
                "{stop} on the one-shot would claim a stop contract `tasks run` \
                 does not implement"
            );
        }
    }
}
