//! The Tasks surface's wire types, and the one decision AD-137 turns on: which
//! host will *actually* run a given task (Story 57.5, FR-351, FR-352).
//!
//! # Why these view models live in `keeper-core` and not in the shell
//!
//! `keeper/src/sync_ipc.rs:3-7` states what looks like the opposite rule —
//! *"View models live here rather than in `keeper-core::vm` because sync is not
//! part of the Matrix hexagon and `keeper-core` must never learn about it
//! (AD-40)"*. Three things settle it in favour of this file:
//!
//! 1. **Precedent.** The `Files*Vm` family — [`crate::vm::FilesEntryVm`],
//!    [`crate::vm::FilesListingVm`], [`crate::vm::FilesReleaseVm`],
//!    [`crate::vm::FilesWriteVm`], [`crate::vm::FilesSyncStatusVm`] — already
//!    lives in [`crate::vm`] and is imported *from* there by
//!    `sync_ipc.rs:17-21`. A sync-domain wire shape in core is the established
//!    case, not a new one.
//! 2. **AD-40 forbids a *dependency*, and this module takes none.** Nothing
//!    here names a `keeper_sync` type: every fact arrives as `&str`,
//!    `Option<&str>` or `Option<&Path>`, and the `keeper_sync::db::TaskRow` →
//!    [`TaskVm`] mapping stays in `sync_ipc.rs`. `keeper-core/Cargo.toml` gains
//!    no dependency for this module, so `keeper-core` still never learns about
//!    `keeper-sync` and AD-40 is intact.
//! 3. **The decisive one: regenerability.** The `keeper` shell crate does not
//!    compile on a Linux host, so a `#[ts(export)]` type defined *there* cannot
//!    have its TypeScript binding regenerated without a Mac. Defining these
//!    here is what makes `cargo test -p keeper-core` emit `src/lib/ipc/gen/`
//!    on this machine, and therefore what lets Story 57.6's frontend typecheck
//!    at all.
//!
//! # Conventions
//!
//! Every wire type follows [`crate::vm`]'s rules: serde `camelCase`,
//! `#[ts(export)]` into `src/lib/ipc/gen/`, timestamps as `i64` milliseconds
//! since the Unix epoch (UTC), and `null` — never an absent key — where null is
//! a real value, mirroring the CLI's `taskDoc`/`runDoc`. Every 64-bit field
//! carries `#[ts(type = "number")]` for [`crate::vm::PingVm::ts`]'s reason: the
//! default ts-rs mapping is `bigint`, which no `JSON.parse` ever produces.

use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The stored `mode` spellings this build understands.
///
/// These mirror `keeper_sync::tasks::TaskMode::as_str` (`keeper-sync/src/tasks.rs:160-166`),
/// which this crate must not import (AD-40, and see the module doc). A mirror
/// can drift, so the drift is *handled* rather than assumed away: an
/// unrecognised spelling falls through [`task_host`]'s last arm to
/// [`TaskHostKind::Unhosted`] instead of being quietly treated as schedulable —
/// NFR-43's "a row of a kind this build does not know is skipped rather than
/// fatal", applied to the mode column. Private on purpose: the shell owns the
/// real enum and must keep reading its own `as_str`, not a copy in core.
const MODE_OFF: &str = "off";
const MODE_MANUAL: &str = "manual";
const MODE_SCHEDULED: &str = "scheduled";

/// What the daemon sentence says when `keeper-syncd`'s unit will run the task.
///
/// The six sentences below are `pub const` rather than inline literals because
/// three parties quote them: this module builds them, its tests assert them, and
/// the Tasks pane renders them. Two copies of a sentence is how a surface ends
/// up claiming a host it no longer has.
pub const HOST_SENTENCE_DAEMON: &str =
    "the keeper-syncd unit on this machine runs this, logged in or not";

/// The honest macOS sentence, and the honest Linux-without-a-unit one: the app
/// is a real background host — closing the window calls `prevent_close()` +
/// `hide()` and keeps the process, engine and notifications alive
/// (`keeper/src/lib.rs:1106-1112`) — but **quit means quit** (AD-137).
pub const HOST_SENTENCE_APP: &str = "keeper runs this — only while keeper is running";

/// The app sentence plus the fact that makes it surprising on Linux: a unit is
/// enabled here, yet it reads a different `sync.db`, so it never sees this row.
/// Saying only "keeper runs this" would leave the user believing the enabled
/// unit is the host — the exact over-claim AD-137 forbids.
pub const HOST_SENTENCE_APP_OTHER_DATA_DIR: &str = "keeper runs this — only while keeper is running; the keeper-syncd unit here reads a different data directory, so it never sees this task";

/// A `manual` task: remembered, schedulable-looking, and deliberately not
/// scheduled. Stated so the row cannot be read as enabled-and-quiet.
pub const HOST_SENTENCE_ON_REQUEST: &str = "nothing schedules this — it runs when you ask";

/// A task that is off or disabled. It says *"not even a request"* because
/// `TaskMode::Off` refuses a direct human ask too
/// (`keeper-sync/src/tasks.rs:150-151`), which `manual` does not.
pub const HOST_SENTENCE_OFF: &str = "switched off — nothing runs this, not even a request";

/// A task that looks enabled and that no present host can run. The reason is
/// carried separately in [`TaskHostVm::reason`], because the sentence is the
/// verdict and the reason is the evidence.
pub const HOST_SENTENCE_UNHOSTED: &str = "nothing will run this";

/// Unhosted because the task names a profile that is no longer a sync profile.
pub const UNHOSTED_FOLDER_GONE: &str =
    "it names a folder keeper does not sync, so no host here can run it";

/// Unhosted because `mode = scheduled` and the schedule column is null — the
/// "reports itself enabled while parsing to never" shape the epic names as the
/// invisible-failure case.
pub const UNHOSTED_NO_SCHEDULE: &str =
    "it is set to run on a schedule but none is stored, so nothing will ever make it due";

/// Unhosted because a newer keeper wrote a `mode` this build cannot read.
pub const UNHOSTED_UNKNOWN_MODE: &str =
    "its mode is one this build does not understand, so nothing here will run it";

/// Whether a `keeper-syncd` unit on this machine can run tasks out of *this*
/// app's database.
///
/// Two independent facts collapse into one enum, because either alone is
/// misleading: a unit can be enabled and still be irrelevant, and an absent unit
/// is not a failure. See [`daemon_presence`] for how the shell establishes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum DaemonPresence {
    /// A `keeper-syncd` user unit is enabled here **and** reads the same data
    /// directory, so it sees this app's `tasks` table and will run due rows
    /// whether or not anybody is logged in.
    Runs,
    /// A unit is enabled here but resolves a different data directory — the
    /// default on Linux — so it never sees this app's tasks.
    OtherDataDir,
    /// No enabled `keeper-syncd` unit on this machine. Always the case on macOS,
    /// where no launchd plist for the daemon exists anywhere in the repository
    /// (AD-137).
    Absent,
}

/// Which host will run a task — the vocabulary the Tasks view renders.
///
/// Five variants and not four: [`Self::Off`] and [`Self::Unhosted`] are
/// different facts, and conflating them is precisely the bug AD-137 exists to
/// prevent. A switched-off task is honestly off and the user did that on
/// purpose; an unhosted task looks enabled and will never fire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum TaskHostKind {
    /// The `keeper-syncd` unit on this machine runs it, logged in or not.
    Daemon,
    /// The desktop app runs it, and only while the app is running.
    App,
    /// Nothing schedules it; it runs when asked (`mode = manual`).
    OnRequest,
    /// It looks enabled and no present host can run it — the honest negative
    /// AD-137 names, never rendered as enabled-and-quiet.
    Unhosted,
    /// It is switched off or disabled. Not [`Self::Unhosted`]: nothing is wrong.
    Off,
}

/// The host verdict for one task: the machine-readable kind, the sentence the
/// row shows, and — for [`TaskHostKind::Unhosted`] only — why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TaskHostVm {
    /// The verdict, for branching and for tests.
    pub kind: TaskHostKind,
    /// The sentence to render verbatim — one of the `HOST_SENTENCE_*` constants.
    /// Composed in Rust so no platform sniff in TypeScript can disagree with it.
    pub sentence: String,
    /// Why nothing will run this, `null` for every hosted kind. Present only on
    /// [`TaskHostKind::Unhosted`], so a non-null reason *is* the alarm.
    pub reason: Option<String>,
}

/// One recorded run of a task, projected from a `task_runs` row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TaskRunVm {
    /// The run's autoincrement row id — the history list's stable key.
    #[ts(type = "number")]
    pub id: i64,
    /// The task this run belongs to.
    pub task_id: String,
    /// When the run started: ms since the Unix epoch (UTC).
    #[ts(type = "number")]
    pub started_ms: i64,
    /// When it ended, `null` while it is still in flight.
    #[ts(type = "number | null")]
    pub finished_ms: Option<i64>,
    /// The outcome in this build's vocabulary, `null` when the run is in flight
    /// **or** when the stored spelling is unreadable — see
    /// [`Self::unknown_outcome`].
    pub outcome: Option<String>,
    /// The stored outcome spelling when this build cannot read it, else `null`.
    ///
    /// Both keys always exist, which is what lets the pair be unambiguous
    /// without a conditional key: `outcome: null, unknownOutcome: null` means
    /// in flight, and a string here means a newer keeper wrote a spelling we
    /// render verbatim rather than as "unknown" (NFR-43).
    pub unknown_outcome: Option<String>,
    /// The run's detail line — an error message, a summary — `null` when none.
    pub detail: Option<String>,
    /// Which host recorded the run, as stored (e.g. `"app"`, `"daemon"`).
    pub host: String,
}

/// One task row, with its host verdict already computed.
///
/// `kind` and `mode` are `String` rather than enums on purpose: a row a newer
/// keeper wrote must reach the view as the spelling it has, so the pane can show
/// it instead of hiding it (NFR-43). The verdict in [`Self::host`] is what makes
/// that safe — an unreadable `mode` reads [`TaskHostKind::Unhosted`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TaskVm {
    /// The task id, unique per database.
    pub id: String,
    /// The task kind as stored — one of keeper's own verbs, never a shell
    /// string (Epic 57's standing constraint).
    pub kind: String,
    /// The stored mode: `"off"`, `"manual"`, `"scheduled"`, or a spelling this
    /// build does not know.
    pub mode: String,
    /// The row's enabled flag, independent of the mode.
    pub enabled: bool,
    /// The profile this task is scoped to, `null` for host-wide work.
    pub profile_id: Option<String>,
    /// That profile's human name, `null` when the id names no current profile —
    /// which is exactly the "folder is gone" fact [`task_host`] acts on.
    pub profile: Option<String>,
    /// The stored schedule expression, `null` when none is stored.
    pub schedule: Option<String>,
    /// When it next comes due: ms since the Unix epoch, `null` when never.
    #[ts(type = "number | null")]
    pub next_due_ms: Option<i64>,
    /// The host currently holding a run lease, `null` when idle.
    pub running_host: Option<String>,
    /// When that lease expires: ms since the Unix epoch, `null` when idle.
    #[ts(type = "number | null")]
    pub lease_until_ms: Option<i64>,
    /// The most recent recorded run, `null` when it has never run.
    pub last_run: Option<TaskRunVm>,
    /// Which host will actually run this — [`task_host`]'s verdict.
    pub host: TaskHostVm,
}

/// A task row this build cannot read, surfaced rather than dropped.
///
/// NFR-43's tolerance made visible: the CLI already skips such a row, and the
/// view shows it with the reason so a task written by a newer keeper is not
/// silently missing from the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UnknownTaskVm {
    /// The row's id, which is readable even when the rest is not.
    pub id: String,
    /// Why this build could not read the row.
    pub reason: String,
}

/// The Tasks view's whole payload: the rows we understand, and the rows we do
/// not but still show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TaskListingVm {
    /// Every readable task, with its host verdict.
    pub tasks: Vec<TaskVm>,
    /// Every unreadable task row (NFR-43).
    pub unknown: Vec<UnknownTaskVm>,
}

/// A save request from the Tasks view.
///
/// Deliberately narrower than [`TaskVm`]: `next_due_ms`, the lease columns and
/// the run history are the engine's to write, never the view's, so they have no
/// key here. The schedule is refused rather than coerced when it does not parse
/// (FR-347), which happens in the engine — this type only carries the string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TaskSaveReq {
    /// The task to create or replace.
    pub id: String,
    /// The task kind.
    pub kind: String,
    /// The requested mode.
    pub mode: String,
    /// The requested enabled flag.
    pub enabled: bool,
    /// The profile to scope it to, `null` for host-wide.
    pub profile_id: Option<String>,
    /// The schedule expression, `null` to store none.
    pub schedule: Option<String>,
}

/// Exactly the facts [`task_host`] needs, borrowed.
///
/// A borrowed struct rather than five positional arguments because the two
/// `Option<&str>` fields would otherwise be swappable at the call site without a
/// type error, and swapping `profile_id` with `profile` inverts the folder-gone
/// verdict. Borrowed rather than owned because the caller is projecting a row it
/// already holds and this function allocates nothing but its verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskHostFacts<'a> {
    /// The row's enabled flag.
    pub enabled: bool,
    /// The stored mode spelling, verbatim.
    pub mode: &'a str,
    /// The stored schedule expression, `None` when none is stored.
    pub schedule: Option<&'a str>,
    /// The profile id the task is scoped to, `None` for host-wide.
    pub profile_id: Option<&'a str>,
    /// The resolved name of that profile, `None` when the id names no current
    /// profile — i.e. the folder is gone.
    pub profile: Option<&'a str>,
}

/// Decide whether a `keeper-syncd` unit on this machine can run *this* app's
/// tasks.
///
/// The load-bearing fact, because it is counter-intuitive: **by default the two
/// hosts do not share one `sync.db`.** `keeper-syncd` resolves its data
/// directory to `$XDG_DATA_HOME` (or `~/.local/share`) plus `keeper-sync`
/// (`keeper-syncd/src/platform.rs:29-30`, `:180-187`), while the desktop app
/// uses `dirs::data_dir()` plus `dev.tgorka.keeper`
/// (`DesktopPlatform::data_dir`, `keeper/src/ipc.rs:651-656`). So on a stock
/// Linux box with the unit enabled, the daemon is running and is *still* not the
/// host for anything the app wrote — which is exactly what AD-137 records at its
/// *"the record is shared; the schedule is per host by default"* bullet.
///
/// The caller canonicalizes both paths before calling: this function compares
/// them as given and does no I/O, so a symlinked or `..`-bearing path is the
/// caller's problem to resolve, not a difference to report.
///
/// `daemon_data_dir` is `None` when the daemon's directory could not be resolved
/// at all, and that reads [`DaemonPresence::OtherDataDir`] rather than
/// [`DaemonPresence::Runs`]. The safe direction is never to over-claim a daemon:
/// crediting a host that turns out not to see the row produces a task that looks
/// hosted and never fires, while under-crediting one produces a row that says
/// "keeper runs this" on a machine where the daemon might also run it — visible,
/// recoverable, and not a silent non-execution.
pub fn daemon_presence(
    unit_enabled: bool,
    daemon_data_dir: Option<&Path>,
    app_data_dir: &Path,
) -> DaemonPresence {
    if !unit_enabled {
        return DaemonPresence::Absent;
    }
    match daemon_data_dir {
        Some(dir) if dir == app_data_dir => DaemonPresence::Runs,
        _ => DaemonPresence::OtherDataDir,
    }
}

/// Decide which host will actually run a task — AD-137's decision, and the whole
/// point of this module.
///
/// **Why this is architecture and not UI copy.** The alternative is the failure
/// this tree has already paid for twice: a feature that looks enabled and does
/// nothing (`sprint-status.yaml`'s recurring `incorrect` lesson, DW-140/DW-206).
/// A schedule is exactly the kind of thing whose non-execution is invisible —
/// nobody notices the absence of housekeeping — so the surface must *assert* the
/// host rather than imply it, and a test must assert the unhosted case (AD-137,
/// "Why this is an architecture decision and not UI copy"). Keeping the decision
/// here, pure and over borrowed facts, is also what stops it being re-derived
/// from `navigator.platform` in TypeScript, where it would be a guess.
///
/// The gates are ordered, and the order is the decision:
///
/// 1. **Off or disabled first.** An off task is honestly off; AD-137's rule is
///    about a task that *looks enabled* and never fires, so this case must never
///    reach [`TaskHostKind::Unhosted`]. It precedes the folder gate because a
///    switched-off task whose folder also vanished is still just off — raising an
///    alarm about a row the user deliberately silenced is noise.
/// 2. **The missing folder next**, ahead of every mode gate, because it defeats
///    all of them: a `manual` task whose folder is gone cannot run when asked
///    either, so answering [`TaskHostKind::OnRequest`] would be a false offer.
/// 3. **`manual` before the schedule gate**, because a manual task's schedule is
///    "remembered, not obeyed" (`keeper-sync/src/tasks.rs:152-153`) — a null
///    schedule there is normal, not a fault, and gate 4 would misreport it.
/// 4. **Scheduled with no schedule is unhosted.** This is the epic's named
///    invisible-failure shape: a row that reports itself enabled while nothing
///    will ever make it due.
/// 5. **Scheduled with a schedule** is the only case where the daemon can be the
///    host, so [`daemon_presence`] is consulted only here.
/// 6. **Any other mode spelling is unhosted**, exhaustively and without a panic —
///    NFR-43's tolerance applied to a row a newer keeper wrote.
pub fn task_host(facts: TaskHostFacts<'_>, daemon: DaemonPresence) -> TaskHostVm {
    if !facts.enabled || facts.mode == MODE_OFF {
        return verdict(TaskHostKind::Off, HOST_SENTENCE_OFF);
    }
    if facts.profile_id.is_some() && facts.profile.is_none() {
        return unhosted(UNHOSTED_FOLDER_GONE);
    }
    if facts.mode == MODE_MANUAL {
        return verdict(TaskHostKind::OnRequest, HOST_SENTENCE_ON_REQUEST);
    }
    if facts.mode == MODE_SCHEDULED {
        if facts.schedule.is_none() {
            return unhosted(UNHOSTED_NO_SCHEDULE);
        }
        return match daemon {
            DaemonPresence::Runs => verdict(TaskHostKind::Daemon, HOST_SENTENCE_DAEMON),
            DaemonPresence::Absent => verdict(TaskHostKind::App, HOST_SENTENCE_APP),
            DaemonPresence::OtherDataDir => {
                verdict(TaskHostKind::App, HOST_SENTENCE_APP_OTHER_DATA_DIR)
            }
        };
    }
    unhosted(UNHOSTED_UNKNOWN_MODE)
}

/// One hosted verdict. Private, and the only constructor with no reason, which
/// is what makes "a non-null reason means unhosted" a property of the module
/// rather than a convention each call site has to remember.
fn verdict(kind: TaskHostKind, sentence: &str) -> TaskHostVm {
    TaskHostVm {
        kind,
        sentence: sentence.to_owned(),
        reason: None,
    }
}

/// The honest negative: one sentence, always with its evidence attached.
fn unhosted(reason: &str) -> TaskHostVm {
    TaskHostVm {
        kind: TaskHostKind::Unhosted,
        sentence: HOST_SENTENCE_UNHOSTED.to_owned(),
        reason: Some(reason.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// A scheduled, enabled, host-wide task — the shape every gate is measured
    /// against, mutated per test by struct update.
    fn scheduled() -> TaskHostFacts<'static> {
        TaskHostFacts {
            enabled: true,
            mode: MODE_SCHEDULED,
            schedule: Some("every 5m"),
            profile_id: None,
            profile: None,
        }
    }

    #[test]
    fn a_disabled_task_reads_off_and_never_unhosted() {
        let host = task_host(
            TaskHostFacts {
                enabled: false,
                ..scheduled()
            },
            DaemonPresence::Runs,
        );
        assert_eq!(host.kind, TaskHostKind::Off);
        assert_eq!(host.sentence, HOST_SENTENCE_OFF);
        assert_eq!(host.reason, None);
    }

    #[test]
    fn a_task_whose_mode_is_off_reads_off() {
        let host = task_host(
            TaskHostFacts {
                mode: MODE_OFF,
                ..scheduled()
            },
            DaemonPresence::Runs,
        );
        assert_eq!(host.kind, TaskHostKind::Off);
        assert_eq!(host.sentence, HOST_SENTENCE_OFF);
    }

    #[test]
    fn an_off_task_whose_folder_is_gone_is_still_only_off() {
        // Gate 1 precedes gate 2: a row the user silenced raises no alarm.
        let host = task_host(
            TaskHostFacts {
                mode: MODE_OFF,
                profile_id: Some("p1"),
                profile: None,
                ..scheduled()
            },
            DaemonPresence::Absent,
        );
        assert_eq!(host.kind, TaskHostKind::Off);
    }

    #[test]
    fn a_task_whose_folder_is_gone_reads_unhosted_with_the_reason() {
        // The failure mode the whole decision exists to prevent.
        let host = task_host(
            TaskHostFacts {
                profile_id: Some("gone"),
                profile: None,
                ..scheduled()
            },
            DaemonPresence::Runs,
        );
        assert_eq!(host.kind, TaskHostKind::Unhosted);
        assert_eq!(host.sentence, HOST_SENTENCE_UNHOSTED);
        assert_eq!(host.reason.as_deref(), Some(UNHOSTED_FOLDER_GONE));
    }

    #[test]
    fn a_manual_task_whose_folder_is_gone_is_not_offered_on_request() {
        // Gate 2 precedes gate 3: asking cannot run it either.
        let host = task_host(
            TaskHostFacts {
                mode: MODE_MANUAL,
                profile_id: Some("gone"),
                profile: None,
                ..scheduled()
            },
            DaemonPresence::Absent,
        );
        assert_eq!(host.kind, TaskHostKind::Unhosted);
        assert_eq!(host.reason.as_deref(), Some(UNHOSTED_FOLDER_GONE));
    }

    #[test]
    fn a_task_whose_folder_still_resolves_is_hosted_normally() {
        let host = task_host(
            TaskHostFacts {
                profile_id: Some("p1"),
                profile: Some("tgdrive"),
                ..scheduled()
            },
            DaemonPresence::Runs,
        );
        assert_eq!(host.kind, TaskHostKind::Daemon);
    }

    #[test]
    fn a_manual_task_reads_on_request() {
        let host = task_host(
            TaskHostFacts {
                mode: MODE_MANUAL,
                ..scheduled()
            },
            DaemonPresence::Runs,
        );
        assert_eq!(host.kind, TaskHostKind::OnRequest);
        assert_eq!(host.sentence, HOST_SENTENCE_ON_REQUEST);
        assert_eq!(host.reason, None);
    }

    #[test]
    fn a_manual_task_with_no_schedule_stored_is_still_on_request() {
        // Gate 3 precedes gate 4: a manual task's schedule is remembered, not
        // obeyed, so its absence is normal rather than a fault.
        let host = task_host(
            TaskHostFacts {
                mode: MODE_MANUAL,
                schedule: None,
                ..scheduled()
            },
            DaemonPresence::Absent,
        );
        assert_eq!(host.kind, TaskHostKind::OnRequest);
    }

    #[test]
    fn a_scheduled_task_with_no_schedule_reads_unhosted_with_the_reason() {
        let host = task_host(
            TaskHostFacts {
                schedule: None,
                ..scheduled()
            },
            DaemonPresence::Runs,
        );
        assert_eq!(host.kind, TaskHostKind::Unhosted);
        assert_eq!(host.sentence, HOST_SENTENCE_UNHOSTED);
        assert_eq!(host.reason.as_deref(), Some(UNHOSTED_NO_SCHEDULE));
    }

    #[test]
    fn a_mode_this_build_cannot_read_reads_unhosted_with_the_reason() {
        let host = task_host(
            TaskHostFacts {
                mode: "teleport",
                ..scheduled()
            },
            DaemonPresence::Runs,
        );
        assert_eq!(host.kind, TaskHostKind::Unhosted);
        assert_eq!(host.sentence, HOST_SENTENCE_UNHOSTED);
        assert_eq!(host.reason.as_deref(), Some(UNHOSTED_UNKNOWN_MODE));
    }

    #[test]
    fn a_linux_box_whose_enabled_unit_shares_the_data_dir_reads_daemon() {
        let host = task_host(scheduled(), DaemonPresence::Runs);
        assert_eq!(host.kind, TaskHostKind::Daemon);
        assert_eq!(host.sentence, HOST_SENTENCE_DAEMON);
        assert_eq!(host.reason, None);
    }

    #[test]
    fn a_mac_with_no_daemon_anywhere_reads_app_only_while_keeper_is_running() {
        let host = task_host(scheduled(), DaemonPresence::Absent);
        assert_eq!(host.kind, TaskHostKind::App);
        assert_eq!(host.sentence, HOST_SENTENCE_APP);
        assert!(host.sentence.contains("only while keeper is running"));
        assert_eq!(host.reason, None);
    }

    #[test]
    fn a_daemon_reading_another_data_dir_reads_app_and_says_why_it_is_not_the_host() {
        let other = task_host(scheduled(), DaemonPresence::OtherDataDir);
        assert_eq!(other.kind, TaskHostKind::App);
        assert_eq!(other.sentence, HOST_SENTENCE_APP_OTHER_DATA_DIR);
        assert_eq!(other.reason, None);
        // Same kind, different sentence: the app is the host either way, but a
        // machine with an enabled-and-blind unit needs to be told so.
        let absent = task_host(scheduled(), DaemonPresence::Absent);
        assert_eq!(absent.kind, other.kind);
        assert_ne!(absent.sentence, other.sentence);
    }

    #[test]
    fn only_the_unhosted_verdicts_carry_a_reason() {
        let cases = [
            task_host(scheduled(), DaemonPresence::Runs),
            task_host(scheduled(), DaemonPresence::Absent),
            task_host(scheduled(), DaemonPresence::OtherDataDir),
            task_host(
                TaskHostFacts {
                    mode: MODE_MANUAL,
                    ..scheduled()
                },
                DaemonPresence::Runs,
            ),
            task_host(
                TaskHostFacts {
                    mode: MODE_OFF,
                    ..scheduled()
                },
                DaemonPresence::Runs,
            ),
            task_host(
                TaskHostFacts {
                    schedule: None,
                    ..scheduled()
                },
                DaemonPresence::Runs,
            ),
        ];
        for host in cases {
            assert_eq!(
                host.reason.is_some(),
                host.kind == TaskHostKind::Unhosted,
                "a non-null reason must mean unhosted and nothing else: {host:?}"
            );
        }
    }

    fn dir(path: &str) -> PathBuf {
        PathBuf::from(path)
    }

    #[test]
    fn a_unit_that_is_not_enabled_is_absent() {
        let app = dir("/home/dev/.local/share/dev.tgorka.keeper");
        // Even pointing at the very same directory: not enabled is not a host.
        assert_eq!(
            daemon_presence(false, Some(&app), &app),
            DaemonPresence::Absent
        );
    }

    #[test]
    fn an_enabled_unit_reading_the_same_data_dir_runs_tasks() {
        let app = dir("/home/dev/.local/share/dev.tgorka.keeper");
        assert_eq!(
            daemon_presence(true, Some(&app), &app),
            DaemonPresence::Runs
        );
    }

    #[test]
    fn an_enabled_unit_reading_the_stock_daemon_dir_cannot_see_this_database() {
        // The stock Linux pairing, and the reason this function exists.
        let app = dir("/home/dev/.local/share/dev.tgorka.keeper");
        let daemon = dir("/home/dev/.local/share/keeper-sync");
        assert_eq!(
            daemon_presence(true, Some(&daemon), &app),
            DaemonPresence::OtherDataDir
        );
    }

    #[test]
    fn an_enabled_unit_whose_data_dir_cannot_be_resolved_is_not_credited() {
        let app = dir("/home/dev/.local/share/dev.tgorka.keeper");
        assert_eq!(
            daemon_presence(true, None, &app),
            DaemonPresence::OtherDataDir
        );
    }

    #[test]
    fn the_stock_linux_pairing_makes_the_app_the_host_of_a_scheduled_task() {
        // The two functions composed, which is how the shell calls them.
        let app = dir("/home/dev/.local/share/dev.tgorka.keeper");
        let daemon = dir("/home/dev/.local/share/keeper-sync");
        let presence = daemon_presence(true, Some(&daemon), &app);
        let host = task_host(scheduled(), presence);
        assert_eq!(host.kind, TaskHostKind::App);
        assert_eq!(host.sentence, HOST_SENTENCE_APP_OTHER_DATA_DIR);
    }
}
