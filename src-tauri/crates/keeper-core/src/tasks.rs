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

use std::path::{Path, PathBuf};

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

/// What the daemon sentence says when `keeper-syncd`'s unit will run the task
/// **and survives logout**.
///
/// The seven sentences below are `pub const` rather than inline literals because
/// three parties quote them: this module builds them, its tests assert them, and
/// the Tasks pane renders them. Two copies of a sentence is how a surface ends
/// up claiming a host it no longer has.
///
/// *"logged in or not"* is a load-bearing promise, not a flourish, and it is
/// true here because [`daemon_presence`] refuses to reach this sentence until a
/// caller has established that lingering is on — see
/// [`DaemonPresence::RunsUntilLogout`] for the machine where it is not, and
/// [`linger_marker_path`] for the fact that separates them (Story 57.5's
/// review, finding 2).
pub const HOST_SENTENCE_DAEMON: &str =
    "the keeper-syncd unit on this machine runs this, logged in or not";

/// The daemon sentence for a unit that is enabled, shares the database, and
/// **dies with the session** because lingering was never enabled.
///
/// The host is still the daemon — the unit really does run the task — so this
/// is a second sentence under one [`TaskHostKind::Daemon`], exactly as
/// [`HOST_SENTENCE_APP_OTHER_DATA_DIR`] is a second sentence under
/// [`TaskHostKind::App`]. What changes is the one thing the person planning a
/// nightly sweep needs: a `--user` unit is stopped when its user's last session
/// ends, so on this machine the schedule stops at logout and nothing says so
/// unless this sentence does. It names the switch (`lingering is off`) rather
/// than printing a command, because the command lives in `docs/sync.md` §14
/// beside the verification step that proves it took.
pub const HOST_SENTENCE_DAEMON_UNTIL_LOGOUT: &str =
    "the keeper-syncd unit on this machine runs this while you are logged in — \
     lingering is off, so its schedule stops when your session ends";

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
///
/// "Gone", strictly: the id is in no profile list *and* the store holds no row
/// for it that merely failed to read. That second case has its own reason —
/// [`UNHOSTED_FOLDER_UNREADABLE`] — because telling somebody their folder is
/// gone when keeper simply could not parse its row sends them to delete a task
/// that is fine.
pub const UNHOSTED_FOLDER_GONE: &str =
    "it names a folder keeper does not sync, so no host here can run it";

/// Unhosted because the task's folder **has** a stored row and this build could
/// not read it (Story 57.5's review, finding 3).
///
/// A fault to surface, not a folder to forget. `db::list_profiles` skips a row
/// it cannot deserialize — so an older keeper is not bricked by a newer one's
/// profile — and that skip made a task scoped to a healthy, actively-syncing
/// folder report *"it names a folder keeper does not sync"* **persistently**,
/// which is the one thing AD-137 exists to prevent, produced by the projection
/// rather than by this function.
///
/// The verdict is still *unhosted*, and truthfully so: the engine's own
/// `run_due_tasks` reads the same skipped list, so nothing on this host will run
/// the task either. What changes is what the person is told to do about it.
pub const UNHOSTED_FOLDER_UNREADABLE: &str =
    "keeper could not read this folder's configuration, so no host here can run it — \
     the folder itself may be fine";

/// Unhosted because `mode = scheduled` and the schedule column is null — the
/// "reports itself enabled while parsing to never" shape the epic names as the
/// invisible-failure case.
pub const UNHOSTED_NO_SCHEDULE: &str =
    "it is set to run on a schedule but none is stored, so nothing will ever make it due";

/// Unhosted because the `mode` spelling is one this build cannot read.
///
/// **Unreachable from any production caller, and deliberately kept.** A row
/// whose stored `mode` this build cannot read never becomes a `TaskRow` at all:
/// `db::decode_task` files it under `TaskListing.unknown` before anything
/// projects it, so NFR-43's newer-keeper path belongs to `db::list_tasks` and
/// the pane's *"written by a newer keeper"* section, not here. This gate is the
/// total-function defence against a future in-crate caller that builds
/// [`TaskHostFacts`] by hand — which is why it is asserted by a unit test that
/// constructs exactly that.
pub const UNHOSTED_UNKNOWN_MODE: &str =
    "its mode is one this build does not understand, so nothing here will run it";

/// Whether a `keeper-syncd` unit on this machine can run tasks out of *this*
/// app's database, and for how long.
///
/// Three independent facts collapse into one enum, because no one of them is
/// enough on its own: a unit can be enabled and still be irrelevant, an absent
/// unit is not a failure, and a unit that is enabled *and* relevant still stops
/// at logout unless lingering was enabled for the user. See [`daemon_presence`]
/// for how the shell establishes all three.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum DaemonPresence {
    /// A `keeper-syncd` user unit is enabled here, reads the same data
    /// directory, **and** lingers — so it sees this app's `tasks` table and will
    /// run due rows whether or not anybody is logged in.
    Runs,
    /// Everything [`Self::Runs`] needs except lingering: the unit is enabled and
    /// shares the database, so it really is the host, but a `--user` unit is
    /// stopped when its user's last session ends and `loginctl enable-linger`
    /// was never run here. The schedule therefore stops at logout (Story 57.5's
    /// review, finding 2).
    RunsUntilLogout,
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
    /// The `keeper-syncd` unit on this machine runs it. Whether that survives
    /// logout is the sentence's to say, not this variant's — see
    /// [`HOST_SENTENCE_DAEMON`] and [`HOST_SENTENCE_DAEMON_UNTIL_LOGOUT`].
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
    /// What the operator called this task, `null` when they called it nothing
    /// (Story 59.5).
    ///
    /// The only human-editable name a task will ever have. [`Self::id`] is
    /// minted by Rust when the form leaves it blank, and it can never be changed
    /// afterwards because `task_runs.task_id` joins on it — so an id is either a
    /// ULID nobody chose or a word chosen once, and this is where a second
    /// thought about the wording has to go.
    ///
    /// Carried verbatim, so `null` and `""` both reach the view and are
    /// different facts: `null` is every row written before the column existed,
    /// `""` is a person who cleared the box. A surface drawing this renders
    /// **either** as nothing — `taskReportText`'s rule, for its reason: a
    /// heading over an empty string is the one shape a reader takes for a failed
    /// read.
    pub description: Option<String>,
    /// What to do about a window that fell due while nobody was home: as
    /// stored, `"run_now"`, `"delay"`, `"skip"`, or a spelling this build does
    /// not know (Story 58.4).
    ///
    /// A `String` for [`Self::kind`]'s reason: a row a newer keeper wrote must
    /// reach the view as the spelling it has. An unreadable policy makes the row
    /// unreadable, so such a spelling only arrives here past a `list_tasks` that
    /// could read it — which is why the view may render it verbatim rather than
    /// having to guess.
    pub on_missed: String,
    /// How long **this** task holds a missed window back, in milliseconds, or
    /// `null` to use keeper's own default (Story 59.6).
    ///
    /// Only meaningful while [`Self::on_missed`] is `"delay"`, and carried on
    /// every row regardless, because the store keeps the number across a policy
    /// change rather than forgetting what somebody typed.
    ///
    /// `null` is **not** the same fact as the default's own number: a row that
    /// chose thirty minutes keeps thirty minutes if keeper's constant is ever
    /// retuned, and a row that chose nothing follows it. A surface that wants to
    /// state the effective wait therefore composes it from this value *or* the
    /// mirrored default, and never substitutes one for the other.
    #[ts(type = "number | null")]
    pub missed_delay_ms: Option<i64>,
    /// When it next comes due: ms since the Unix epoch, `null` when never.
    #[ts(type = "number | null")]
    pub next_due_ms: Option<i64>,
    /// The host currently holding a run lease, `null` when idle.
    pub running_host: Option<String>,
    /// When that lease expires: ms since the Unix epoch, `null` when idle.
    #[ts(type = "number | null")]
    pub lease_until_ms: Option<i64>,
    /// When this row was last written: ms since the Unix epoch (UTC).
    ///
    /// On the wire for one reason and it is not display: it is the baseline an
    /// edit form sends back on [`TaskSaveReq::baseline_updated_ms`], so a save
    /// whose reading has gone stale is refused instead of reverting whatever
    /// another host moved meanwhile.
    #[ts(type = "number")]
    pub updated_ms: i64,
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
    /// What to call this task, `null` to store no description.
    ///
    /// Sent exactly as typed, and `null` rather than `""` for the absent case on
    /// `schedule`'s rule above: the empty string is a value, and a form that
    /// coerced whitespace to absence would be storing something other than what
    /// was in the box. Nothing refuses a spelling here — there is no vocabulary
    /// and no grammar to be wrong about, which is what makes this the one task
    /// field with no refusal attached to it.
    pub description: Option<String>,
    /// The requested missed-window policy, as one of the stored spellings.
    ///
    /// A `String` rather than an enum for [`TaskVm::on_missed`]'s reason, and
    /// refused rather than coerced when this build cannot read it — the same
    /// answer `kind` and `mode` already get.
    pub on_missed: String,
    /// The requested per-task missed-window delay in milliseconds, or `null` to
    /// store none and so use keeper's default (Story 59.6).
    ///
    /// Refused rather than clamped when it is shorter than the grace period or
    /// longer than a schedule may be — `keeper_sync::tasks::validate_missed_delay_ms`
    /// owns both bounds and the sentences, and the caller renders whichever
    /// arrives.
    #[ts(type = "number | null")]
    pub missed_delay_ms: Option<i64>,
    /// The `updated_ms` the caller's reading of this row carried, or `null`.
    ///
    /// The lost-update guard, and it is a **request** field rather than a
    /// protocol detail because only the caller knows whether it holds a reading
    /// worth checking. An edit form does — it seeded its values once, and every
    /// field it is about to send is as old as that seeding — so it sends the row's
    /// `updated_ms` and the store refuses the write if the stored value has
    /// moved. A create sends `null`: there is no reading to be stale.
    #[ts(type = "number | null")]
    pub baseline_updated_ms: Option<i64>,
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
    /// The resolved name of that profile, `None` when the id names no profile
    /// the caller could read.
    pub profile: Option<&'a str>,
    /// Whether the store holds a row for `profile_id` that the caller **could
    /// not read**, as opposed to holding none at all.
    ///
    /// The one bit that separates "this folder is not configured" from "keeper
    /// could not read this folder's configuration", and it has to be a fact the
    /// caller establishes because this function does no I/O. `false` whenever
    /// `profile` is `Some` (there was nothing to fail to read) and whenever
    /// `profile_id` is `None` (a host-wide task names no folder).
    pub profile_unreadable: bool,
}

/// Exactly the facts [`daemon_presence`] needs, borrowed.
///
/// A named-field struct rather than four positional arguments for the reason
/// [`TaskHostFacts`] states, applied to a worse case: `unit_enabled` and
/// `lingering` are two adjacent `bool`s, so a call site could swap them without
/// a type error — and swapping them turns *"no unit here"* into *"runs, logged
/// in or not"*, which is the over-claim AD-137 exists to forbid.
///
/// Every field is a fact the **caller** establishes, because this function does
/// no I/O. `keeper/src/sync_ipc.rs`'s `daemon_presence_here` owns all four:
/// `systemctl --user is-enabled` for the unit, [`linger_marker_path`] plus one
/// `Path::exists` for lingering, and two `canonicalize` calls for the paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DaemonHostFacts<'a> {
    /// Whether a `keeper-syncd` **user** unit is enabled on this machine.
    pub unit_enabled: bool,
    /// Whether lingering is enabled for the user this app runs as — i.e.
    /// whether that user's systemd manager, and every `--user` unit under it,
    /// survives the end of their last session.
    ///
    /// Only consulted when the unit is enabled and shares the database, since
    /// nothing else can make it matter. `false` is the safe direction: it
    /// under-claims the daemon's reach rather than promising a run that will not
    /// happen, so a caller that cannot establish the fact must pass `false`.
    pub lingering: bool,
    /// Where the daemon resolves its data directory, `None` when the caller
    /// could not resolve it at all.
    pub daemon_data_dir: Option<&'a Path>,
    /// Where *this* app resolves its own.
    pub app_data_dir: &'a Path,
}

/// The directory `systemd-logind` keeps one file in per lingering user.
///
/// A hardcoded literal in logind, in all three places it uses it —
/// `method_set_user_linger` (`src/login/logind-dbus.c:1668`),
/// `user_check_linger_file` (`src/login/logind-user.c:728`) and
/// `manager_enumerate_linger_users` (`src/login/logind.c:294`) — and unchanged
/// from systemd v219 (2015) to today. It is not derived from a build-time state
/// directory, so there is no second spelling to look for.
pub const LINGER_DIR: &str = "/var/lib/systemd/linger";

/// The file whose existence **is** the answer to *"does this user linger?"*, or
/// `None` for a user name that must not be turned into a path.
///
/// # Why a `Path::exists` and not `loginctl show-user --property=Linger`
///
/// Because they are the same question, and one of them is a fork/exec onto a
/// D-Bus round trip. `loginctl` reads the `Linger` property of
/// `org.freedesktop.login1.User`; that property's getter,
/// `property_get_linger` (`src/login/logind-user-dbus.c:172-190`), is nothing
/// but a call to `user_check_linger_file`, whose entire body is
/// `access("/var/lib/systemd/linger/<name>", F_OK)`
/// (`src/login/logind-user.c:717-737`). So this is not a proxy signal standing
/// in for the real one — it is the byte logind itself stats, read directly. The
/// file is also the state of record rather than a cache: `loginctl
/// enable-linger` creates it (`touch`, mode 0644, in a directory logind creates
/// mode 0755 — `logind-dbus.c:1655-1671`), `disable-linger` unlinks it, and
/// logind re-stats it per query as well as enumerating the directory at
/// startup. Both modes are world-readable, so the process asking about its own
/// user needs no privilege.
///
/// # What it answers on a machine with no systemd at all
///
/// `false`, and not as an error: [`Path::exists`] returns `bool` and folds
/// `ENOENT` — a missing `/var/lib/systemd`, a missing `linger/`, a missing file
/// — into the same `false`. On such a machine `systemctl --user is-enabled`
/// also fails, so [`DaemonPresence::Absent`] is reached before lingering is
/// consulted at all; the answer is unreachable *and* safe.
///
/// # The names refused, and why refusing beats escaping
///
/// `None` for an empty name, for `.` or `..`, and for anything containing `/`
/// or a NUL. This is not hygiene theatre: the name arrives from the
/// environment, and `Path::join("")` or `join(".")` yields
/// [`LINGER_DIR`] itself — which exists on every systemd box — so an empty
/// `$USER` would report *lingering* for a user who does not linger. That is the
/// one direction this module may never fail in.
///
/// Logind writes the name through `cescape`, so a name holding bytes C would
/// escape (control characters, `"`, `\`) is stored escaped and this lookup
/// misses it, answering `false`. Deliberate: `cescape` is the identity on every
/// POSIX-portable user name, and the failure is the under-claiming direction.
pub fn linger_marker_path(user: &str) -> Option<PathBuf> {
    if user.is_empty() || user == "." || user == ".." || user.contains('/') || user.contains('\0') {
        return None;
    }
    Some(Path::new(LINGER_DIR).join(user))
}

/// Decide whether a `keeper-syncd` unit on this machine can run *this* app's
/// tasks, and whether it keeps doing so after logout.
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
/// The second counter-intuitive fact, and the reason this function has a fourth
/// input: **an enabled `--user` unit is not a background service until the user
/// lingers.** `systemctl --user is-enabled` answers *wanted at login*, not
/// *survives logout*; without `loginctl enable-linger` the user's manager is
/// torn down with their last session and every unit under it goes with it. So
/// lingering decides which of two sentences a row shows
/// ([`DaemonPresence::Runs`] against [`DaemonPresence::RunsUntilLogout`]), and
/// it is asked rather than assumed because assuming it is how *"logged in or
/// not"* became an over-claim (Story 57.5's review, finding 2).
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
/// recoverable, and not a silent non-execution. `lingering: false` is the same
/// direction one step in: the daemon is still credited, for exactly as long as
/// it will actually be there.
pub fn daemon_presence(facts: DaemonHostFacts<'_>) -> DaemonPresence {
    if !facts.unit_enabled {
        return DaemonPresence::Absent;
    }
    match facts.daemon_data_dir {
        Some(dir) if dir == facts.app_data_dir => {
            if facts.lingering {
                DaemonPresence::Runs
            } else {
                DaemonPresence::RunsUntilLogout
            }
        }
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
/// 2. **The unresolvable folder next**, ahead of every mode gate, because it
///    defeats all of them: a `manual` task whose folder is gone cannot run when
///    asked either, so answering [`TaskHostKind::OnRequest`] would be a false
///    offer. It answers with **two** different reasons, and the difference is
///    load-bearing: a folder that is genuinely not configured
///    ([`UNHOSTED_FOLDER_GONE`]) versus one whose stored row the caller could
///    not read ([`UNHOSTED_FOLDER_UNREADABLE`]). The second is a fault to
///    surface, not a folder to forget.
/// 3. **`manual` before the schedule gate**, because a manual task's schedule is
///    "remembered, not obeyed" (`keeper-sync/src/tasks.rs:152-153`) — a null
///    schedule there is normal, not a fault, and gate 4 would misreport it.
/// 4. **Scheduled with no schedule is unhosted.** This is the epic's named
///    invisible-failure shape: a row that reports itself enabled while nothing
///    will ever make it due.
/// 5. **Scheduled with a schedule** is the only case where the daemon can be the
///    host, so [`daemon_presence`] is consulted only here — and its two daemon
///    answers give the same [`TaskHostKind::Daemon`] two sentences, because how
///    long the host lasts is exactly what the person setting a schedule needs
///    and exactly what the kind cannot carry.
/// 6. **Any other mode spelling is unhosted**, exhaustively and without a panic.
///    Unreachable in production — see [`UNHOSTED_UNKNOWN_MODE`] — and kept as a
///    total function for a future in-crate caller.
pub fn task_host(facts: TaskHostFacts<'_>, daemon: DaemonPresence) -> TaskHostVm {
    if !facts.enabled || facts.mode == MODE_OFF {
        return verdict(TaskHostKind::Off, HOST_SENTENCE_OFF);
    }
    if facts.profile_id.is_some() && facts.profile.is_none() {
        return unhosted(if facts.profile_unreadable {
            UNHOSTED_FOLDER_UNREADABLE
        } else {
            UNHOSTED_FOLDER_GONE
        });
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
            DaemonPresence::RunsUntilLogout => {
                verdict(TaskHostKind::Daemon, HOST_SENTENCE_DAEMON_UNTIL_LOGOUT)
            }
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

/// What kind of paced work one projected row describes (Story 58.7).
///
/// Three variants and not one more, because the inventory found only three
/// periodic things on this machine that have **both** an identity and a
/// cadence. Queue reads, event drains, watcher re-arms, credential TTLs and the
/// hourly release *look* gate all run on a clock of some sort and none of them
/// is projected: a row nobody can act on, whose interval means something other
/// than "this is when it happens", is worse than its absence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PacedWorkKind {
    /// The per-profile look for changes — the paced *backstop* behind the
    /// watcher, not the only thing that starts a sync.
    Scan,
    /// The hourly deletion of transfer scratch this folder will never reuse.
    ScratchSweep,
    /// A notes vault's commit-after-quiet and push-within-deadline pair, which
    /// only the running app drives.
    NotesCadence,
}

/// Whether the clock really paces a projected row right now.
///
/// The three non-paced variants are three different reasons for the same
/// absence of a cadence, and collapsing them would lose the actionable half: a
/// paused folder is paused because somebody paused it, a governed folder's scan
/// happens on a *schedule the user can see and edit* in the task list above,
/// and an unregistered vault is waiting on a folder that is not there. Only
/// [`Self::Paced`] ever carries a cadence — see [`PacedWorkVm::cadence`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PacedWorkStanding {
    /// The clock in the running app paces it, at [`PacedWorkVm::cadence`].
    Paced,
    /// Its folder is paused, so `tick` skips it entirely and nothing paces it.
    Paused,
    /// A scheduled Sync task has taken this folder's paced backstop, so the
    /// backstop has stood down and the schedule above decides (Story 58.8).
    Governed,
    /// The work is configured but nothing is registered to do it: a notes vault
    /// whose folder could not be resolved when the registry was last built —
    /// the drive is away, or the folder moved.
    ///
    /// Separate from [`Self::Paused`] because the remedy is different and
    /// neither sentence would be true of the other: nobody paused this, and
    /// re-enabling nothing would bring it back.
    Unregistered,
}

/// One thing this host paces, projected at read time from the profile list.
///
/// # Why there is no `next_due_ms`, no `last_run` and no run history
///
/// Not "not rendered yet" — **unrepresentable**, on purpose. Two facts make any
/// next-run claim a lie this type must not be able to tell:
///
/// 1. The engine's `next_scan_ms` / `next_sweep_ms` are an in-memory
///    `Mutex<HashMap>` discarded when the process ends, so after a restart
///    there is no stored instant to project at all.
/// 2. `scan_due` is `paced || watch_wake_pending || settle_window_elapsed` —
///    two of the scan's three triggers are filesystem events. A file the
///    watcher sees settle brings the next look forward by an amount nothing can
///    predict, so even a live map would only ever be an upper bound.
///
/// And a finished unit leaves no trace to project a *last* run from either:
/// `db::complete` is `DELETE FROM journal WHERE id = ?1`, and `activity` says of
/// itself that it is a human-facing log and not a source of truth (AD-135). A
/// key that could hold either number is a key a future edit would fill in, so
/// this type has neither (AD-141).
///
/// The **invariant**, enforced by [`paced_work`] and by nothing in TypeScript:
/// `cadence.is_some()` if and only if `standing == PacedWorkStanding::Paced`.
/// That is what makes *"paused, about every 15 seconds"* impossible to build
/// rather than merely untested.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PacedWorkVm {
    /// The row's stable key: `"scan:<profileId>"`, `"sweep:<profileId>"` or
    /// `"notes:<profileId>"`. A composite because the identity really is the
    /// pair — one folder contributes up to three rows — and no store holds an
    /// id of its own for work that is projected rather than recorded.
    pub id: String,
    /// Which of the three things this is.
    pub kind: PacedWorkKind,
    /// The folder this work belongs to. Never `null`: every projected row is
    /// per-profile, which is why there is no host-wide case here.
    pub profile_id: String,
    /// That folder's human name, as the profile stores it.
    pub profile: String,
    /// Whether the clock paces it, and if not, why not.
    pub standing: PacedWorkStanding,
    /// The cadence phrase, `null` unless [`Self::standing`] is
    /// [`PacedWorkStanding::Paced`] — see this type's invariant.
    pub cadence: Option<String>,
    /// The honest explanation, composed here and rendered verbatim.
    ///
    /// One of the `PACED_SENTENCE_*` constants, possibly with
    /// [`PACED_SENTENCE_REMOVABLE_CLAUSE`] appended. Composed in Rust for
    /// `HOST_SENTENCE_*`'s reason: each of these carries a fact the browser
    /// cannot re-derive — that a saved file brings the next look forward, that a
    /// governed folder's backstop has stood down, that only the running app
    /// paces a vault — and a second copy in TypeScript is how a surface ends up
    /// contradicting the engine.
    pub sentence: String,
}

/// A paced scan, described from the folder's own poll interval.
///
/// The cadence is hedged — *about* every — and the sentence says why in words:
/// the interval is a backstop, and two of the three things that start a scan are
/// filesystem events. An unhedged "every 15 seconds" would be the over-claim
/// [`PacedWorkVm`]'s missing keys exist to prevent, one level up.
pub const PACED_SENTENCE_SCAN_PACED: &str = "keeper looks for changes on this cadence while it is running. The cadence is a backstop and not the only trigger: a file the watcher sees settle, or a write that closes, brings the next look forward.";

/// A scan whose paced backstop a scheduled Sync task has taken (Story 58.8).
///
/// It still says the watcher can bring a look forward, because that is true
/// under governance too: what stood down is the *interval*, not the event
/// triggers. Saying only "a schedule decides" would tell somebody their edits
/// wait until 3 a.m.
pub const PACED_SENTENCE_SCAN_GOVERNED: &str = "a scheduled sync task decides when this folder is looked at, so the paced backstop has stood down. A file the watcher sees settle still brings a look forward.";

/// The hourly scratch sweep. It names what it deletes, because "sweep" beside a
/// folder full of the user's files must not read as a sweep of the files.
pub const PACED_SENTENCE_SWEEP_PACED: &str =
    "keeper deletes transfer scratch this folder will never use again, on this cadence, while it \
     is running.";

/// A notes vault's cadence, and the fact that makes it different from every
/// task row above it: `keeper-syncd` never runs this. The daemon has no vault
/// pacer at all, so a person who quits the app to let the service take over
/// would be quietly leaving their notes uncommitted.
pub const PACED_SENTENCE_NOTES_PACED: &str = "the app commits this vault after the quiet window and pushes within the deadline. Only the running app paces it — keeper-syncd never does.";

/// A vault the registry does not hold, which is the honest answer whenever the
/// vault folder could not be resolved when the registry was last built.
///
/// It names the remedy because the remedy is not obvious and is not "wait": the
/// registry is rebuilt at launch and when a vault is flagged or unflagged, and
/// by nothing else — so a drive that comes back mid-session does **not**
/// re-register on its own. Saying "it resumes when the drive is back" would be
/// the comfortable sentence and the false one.
pub const PACED_SENTENCE_NOTES_UNREGISTERED: &str = "keeper has no vault registered for this folder, so nothing paces it: the vault folder could not be found when the registry was last built. The registry is rebuilt at launch, and when a vault is flagged or unflagged — not when a drive comes back.";

/// Shared by all three kinds, because the reason is the folder's and not the
/// work's: `tick` skips both the sweep and `tick_profile` for a profile that is
/// not enabled, so one paused folder silences everything projected from it.
/// This sentence therefore beats governance — a paused folder is paused.
pub const PACED_SENTENCE_PAUSED: &str =
    "this folder is paused, so nothing here is paced and no cadence is in force.";

/// Appended to the scan and sweep sentences for a folder on removable media:
/// both are work that finds nothing while the drive is away, and the honest
/// thing to attach the fact to is the work that would have looked. Never
/// appended to a paused row, where the sentence already says nothing is paced
/// and a second reason would read as a second cause — nor to a notes row, whose
/// unregistered sentence already names the missing folder as the reason.
pub const PACED_SENTENCE_REMOVABLE_CLAUSE: &str =
    " The folder is on removable media, so nothing happens at all while the drive is away.";

/// A notes vault's two intervals, borrowed.
///
/// Its own struct rather than two `u64`s on [`PacedFolderFacts`] for the reason
/// that struct itself states, applied to the worst case in this module: these
/// are two adjacent same-typed numbers, so a call site could swap them without a
/// type error — and swapping them says the vault is pushed after two seconds and
/// committed after thirty, which is exactly backwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacedNotesFacts {
    /// Quiet time after the last edit before the note is committed.
    pub commit_idle_ms: u64,
    /// How long a commit may sit locally before it is pushed.
    pub push_interval_ms: u64,
    /// Whether the vault registry actually holds this vault.
    ///
    /// **A configured vault and a paced vault are two different facts**, and
    /// this is the second one. `notes_vault::register_one` returns nothing when
    /// the vault root cannot be canonicalized — the drive is away, the folder
    /// moved — and an unregistered vault is reached by no cadence tick and no
    /// flush. Projecting the *configuration* alone would tell somebody their
    /// notes are being committed and pushed while nothing at all is doing it,
    /// which is the over-claim [`PacedWorkVm`]'s missing keys exist to prevent.
    pub registered: bool,
}

/// Exactly the facts [`paced_work`] needs about one folder, borrowed.
///
/// [`TaskHostFacts`]'s shape and its reasons: a named-field struct rather than
/// positional arguments so two same-typed fields cannot be swapped silently
/// (`profile_id` with `profile`, `scan_interval_ms` with `sweep_interval_ms`),
/// and borrowed because the caller is projecting a profile list it already
/// holds. Every field is a fact the **caller** establishes: this function does
/// no I/O and asks no engine anything, which is what keeps the projection pure
/// and this crate free of `keeper-sync` (AD-40).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacedFolderFacts<'a> {
    /// The profile id, which every projected row's key is built from.
    pub profile_id: &'a str,
    /// The folder's human name.
    pub profile: &'a str,
    /// The profile's enabled flag. `false` silences every row for this folder.
    pub enabled: bool,
    /// Whether the folder lives on removable media.
    pub removable: bool,
    /// The scan's real interval — `effective_poll_interval_ms()`, never the
    /// stored column: a profile may hold a zero there and the engine floors it,
    /// so projecting the stored value would advertise a cadence of nothing.
    pub scan_interval_ms: u64,
    /// Whether a scheduled Sync task has taken this folder's paced backstop.
    ///
    /// A failed governance read is **not** a governance state: the caller passes
    /// `false` there, and the row then prints its poll interval — which is true,
    /// because the paced poll genuinely still runs when nothing was found to
    /// have taken it.
    pub scan_governed: bool,
    /// The scratch sweep's interval — the engine's `SWEEP_EVERY_MS`, passed in
    /// rather than mirrored here so there is no second place for it to disagree.
    pub sweep_interval_ms: u64,
    /// The vault's cadence, `None` when this folder holds no vault — which is
    /// the whole of "no notes row for that folder".
    pub notes: Option<PacedNotesFacts>,
}

/// Project every paced thing on this host from the folder list.
///
/// Pure, total and allocating only its answer: no clock is registered, no
/// due-gate is consulted and nothing is written (AD-142). Each folder yields a
/// scan row and a sweep row, plus a notes row when it holds a vault, in that
/// order — so the rows for one folder arrive together.
///
/// The gates, in the order they are applied and for the reasons the sentences
/// state:
///
/// 1. `!enabled` ⇒ [`PacedWorkStanding::Paused`] with no cadence, on **every**
///    row of that folder. This beats governance deliberately: `tick` skips a
///    disabled profile before it reaches any of this work, so a governed-looking
///    scan row on a paused folder would name the wrong reason for the same
///    silence.
/// 2. An enabled folder whose scan is governed ⇒ [`PacedWorkStanding::Governed`]
///    with no cadence, on the scan row alone. The sweep and the notes rows are
///    untouched: a Sync task takes the *scan's* backstop and nothing else.
/// 3. An enabled folder whose vault is not registered ⇒
///    [`PacedWorkStanding::Unregistered`] with no cadence, on the notes row
///    alone. The scan and the sweep do not go through the vault registry.
/// 4. Otherwise [`PacedWorkStanding::Paced`], with the cadence phrase.
///
/// The invariant *`cadence.is_some()` iff `standing == Paced`* is a property of
/// this function, asserted over every row it returns rather than only stated:
/// `PacedWorkVm`'s two fields are independent and public, so a fourth kind added
/// later can break the pairing, and the phrase it would put on screen —
/// *"paused, about every 15 seconds"* — is exactly the one this class exists to
/// make impossible. The assertion is `debug_assert!` because a release build
/// must not lose the whole view over a wording fault, and every builder below is
/// total: there is no input that reaches it.
pub fn paced_work(folders: &[PacedFolderFacts<'_>]) -> Vec<PacedWorkVm> {
    // Three rows per folder is the ceiling, and the common shape on a machine
    // whose folders hold vaults.
    let mut rows = Vec::with_capacity(folders.len() * 3);
    for folder in folders {
        rows.push(paced_scan_row(folder));
        rows.push(paced_sweep_row(folder));
        if let Some(notes) = folder.notes {
            rows.push(paced_notes_row(folder, notes));
        }
    }
    debug_assert!(
        rows.iter()
            .all(|row| { row.cadence.is_some() == (row.standing == PacedWorkStanding::Paced) }),
        "a projected row carried a cadence without a paced standing, or a paced \
         standing without a cadence"
    );
    rows
}

/// The scan row: the one row governance and removable media can reach.
fn paced_scan_row(folder: &PacedFolderFacts<'_>) -> PacedWorkVm {
    let (standing, cadence, mut sentence) = if !folder.enabled {
        paced_paused()
    } else if folder.scan_governed {
        (
            PacedWorkStanding::Governed,
            None,
            PACED_SENTENCE_SCAN_GOVERNED.to_owned(),
        )
    } else {
        (
            PacedWorkStanding::Paced,
            Some(format!(
                "about every {}",
                duration_words(folder.scan_interval_ms)
            )),
            PACED_SENTENCE_SCAN_PACED.to_owned(),
        )
    };
    // Only where it adds something: a paused row already says nothing is paced.
    if folder.removable && standing != PacedWorkStanding::Paused {
        sentence.push_str(PACED_SENTENCE_REMOVABLE_CLAUSE);
    }
    PacedWorkVm {
        id: format!("scan:{}", folder.profile_id),
        kind: PacedWorkKind::Scan,
        profile_id: folder.profile_id.to_owned(),
        profile: folder.profile.to_owned(),
        standing,
        cadence,
        sentence,
    }
}

/// The scratch sweep row. Governance cannot reach it: a scheduled Sync task
/// takes the scan's backstop, and the sweep keeps its own hour.
fn paced_sweep_row(folder: &PacedFolderFacts<'_>) -> PacedWorkVm {
    let (standing, cadence, mut sentence) = if folder.enabled {
        (
            PacedWorkStanding::Paced,
            Some(format!(
                "every {}",
                duration_words(folder.sweep_interval_ms)
            )),
            PACED_SENTENCE_SWEEP_PACED.to_owned(),
        )
    } else {
        paced_paused()
    };
    // The same clause the scan row carries, for the same reason: with the drive
    // away the sweep's own `read_dir` finds nothing, so an unhedged *"keeper
    // deletes transfer scratch on this cadence"* claims a deletion that is not
    // happening. Suppressed on a paused row, where the sentence already says
    // nothing is paced.
    if folder.removable && standing != PacedWorkStanding::Paused {
        sentence.push_str(PACED_SENTENCE_REMOVABLE_CLAUSE);
    }
    PacedWorkVm {
        id: format!("sweep:{}", folder.profile_id),
        kind: PacedWorkKind::ScratchSweep,
        profile_id: folder.profile_id.to_owned(),
        profile: folder.profile.to_owned(),
        standing,
        cadence,
        sentence,
    }
}

/// The notes row, which exists only for a folder that holds a vault.
///
/// Three answers, and the order matters. A paused folder comes first because
/// pausing is the reason somebody would recognise; an unregistered vault comes
/// second because it is a fact about the folder rather than about the pacer; and
/// only a registered vault on an enabled folder is paced.
fn paced_notes_row(folder: &PacedFolderFacts<'_>, notes: PacedNotesFacts) -> PacedWorkVm {
    let (standing, cadence, sentence) = if !folder.enabled {
        paced_paused()
    } else if !notes.registered {
        (
            PacedWorkStanding::Unregistered,
            None,
            PACED_SENTENCE_NOTES_UNREGISTERED.to_owned(),
        )
    } else {
        (
            PacedWorkStanding::Paced,
            Some(format!(
                "committed after {} of quiet, pushed within {}",
                duration_words(notes.commit_idle_ms),
                duration_words(notes.push_interval_ms)
            )),
            PACED_SENTENCE_NOTES_PACED.to_owned(),
        )
    };
    PacedWorkVm {
        id: format!("notes:{}", folder.profile_id),
        kind: PacedWorkKind::NotesCadence,
        profile_id: folder.profile_id.to_owned(),
        profile: folder.profile.to_owned(),
        standing,
        cadence,
        sentence,
    }
}

/// The paused triple, in one place so the three row builders cannot disagree
/// about it — and so *paused implies no cadence* is written down once.
fn paced_paused() -> (PacedWorkStanding, Option<String>, String) {
    (
        PacedWorkStanding::Paused,
        None,
        PACED_SENTENCE_PAUSED.to_owned(),
    )
}

/// A millisecond interval in the coarsest unit that divides it exactly.
///
/// Coarsest-exact rather than always-seconds because these intervals are read
/// as a promise about frequency: *every 3600 seconds* is the same number as
/// *every 1 hour* and reads as a machine talking. Exactness is what keeps it
/// honest — 90 minutes is not "1 hour", so it stays "90 minutes" rather than
/// being rounded into a unit it does not fill.
///
/// Seconds are the one rounded case, because `MIN_POLL_INTERVAL_MS` and the
/// notes cadence are seconds-scale and a stored 1_500 is genuinely "2 seconds"
/// to a reader. Below a second nothing is rounded and the raw number is shown:
/// no shipped default is sub-second, so such a value is somebody's deliberate
/// experiment and rounding it to "0 seconds" or "1 second" would hide it.
fn duration_words(ms: u64) -> String {
    const SECOND: u64 = 1_000;
    const MINUTE: u64 = 60 * SECOND;
    const HOUR: u64 = 60 * MINUTE;

    if ms >= HOUR && ms.is_multiple_of(HOUR) {
        return plural_words(ms / HOUR, "hour");
    }
    if ms >= MINUTE && ms.is_multiple_of(MINUTE) {
        return plural_words(ms / MINUTE, "minute");
    }
    if ms < SECOND {
        return format!("{ms} ms");
    }
    // Round rather than truncate: 1_900 ms is nearer two seconds than one, and
    // truncating would under-state every cadence that is not a whole second.
    // `saturating_add` because a stored interval is a `u64` a config file
    // supplies: `ms + 500` overflows within 500 of `u64::MAX`, which panics a
    // debug build and wraps a release one into an absurdly small cadence. No
    // reachable interval is anywhere near it, and a panic in a projection that
    // exists to be honest is worse than the two characters that prevent it.
    plural_words(ms.saturating_add(SECOND / 2) / SECOND, "second")
}

/// `1 hour` / `2 hours`. English's own rule, in one place so no phrase above
/// grows an "(s)".
fn plural_words(count: u64, unit: &str) -> String {
    if count == 1 {
        format!("1 {unit}")
    } else {
        format!("{count} {unit}s")
    }
}

/// What one written schedule fires at, or the refusal it earns (Story 59.7,
/// FR-368).
///
/// The answer to *when does this actually run*, asked by the task form while
/// somebody is still typing, and answered by the only implementation of the
/// dialect there is: `keeper_sync::tasks::preview_schedule` runs the same
/// `TaskSchedule::parse` the write door runs and the same `next_due_after` the
/// tick runs. The browser gains no cron parser, and the reason is not tidiness
/// — a second implementation would drift, and its first symptom would be a
/// preview that disagreed with the engine about when a task runs, which is a
/// lie about the future told with a straight face. `recording_path_preview` is
/// the precedent and states the same rule: both the clock and the renderer
/// belong to Rust.
///
/// This type is the wire shape only; it holds no dialect knowledge, because
/// this crate is deliberately `keeper-sync`-free (AD-40) and could not call the
/// parser even if it wanted to. The shell composes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TaskSchedulePreviewVm {
    /// The expression this answer is **about**, echoed exactly as it was asked.
    ///
    /// On the wire for one reason, and it is a correctness one: a caller that
    /// asks on every keystroke has replies that can land out of order, so a slow
    /// answer about `0 3 * * ` can arrive after the answer about `0 3 * * *` and
    /// paint a refusal over text the box no longer holds. The caller compares
    /// this against what it currently holds and renders nothing when they
    /// differ. Without the echo that guard cannot be written at all.
    pub expression: String,
    /// keeper's own refusal, verbatim, `null` when the expression parsed.
    ///
    /// The same sentence the save would show, from the same `Display` the save's
    /// error travels on — so a person meets one wording rather than a preview's
    /// paraphrase and then the real thing. It is a **refusal**, not a fault:
    /// asking about half-typed text is normal, so this arrives as data on a
    /// successful read rather than as a failed command, and nothing about it
    /// blocks a save. The write door still decides, still refuses, and still
    /// quotes what was typed.
    pub refusal: Option<String>,
    /// The next instants this schedule fires at: ms since the Unix epoch,
    /// soonest first.
    ///
    /// Empty when [`Self::refusal`] is set. Each instant is computed from the one
    /// before it, so this is the schedule's own cadence rather than one answer
    /// repeated — and it may be **shorter** than the number asked for, because
    /// the search window is finite. A surface renders however many arrived and
    /// claims no count of its own.
    #[ts(type = "Array<number>")]
    pub instants: Vec<i64>,
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
            profile_unreadable: false,
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

    /// The persistent-lie case this story's review found (finding 3): a task
    /// scoped to a folder that is *there* and syncing, whose profile row this
    /// build cannot deserialize. `db::list_profiles` skips such a row, so
    /// `profile` is `None` exactly as it is for a deleted folder — and telling
    /// the owner their folder is gone sends them to delete a task that is fine.
    #[test]
    fn a_task_whose_folder_row_is_unreadable_says_so_instead_of_folder_gone() {
        let host = task_host(
            TaskHostFacts {
                profile_id: Some("unreadable"),
                profile: None,
                profile_unreadable: true,
                ..scheduled()
            },
            DaemonPresence::Runs,
        );
        assert_eq!(
            host.kind,
            TaskHostKind::Unhosted,
            "still unhosted — the engine reads the same skipped list, so nothing runs it"
        );
        assert_eq!(host.sentence, HOST_SENTENCE_UNHOSTED);
        assert_eq!(
            host.reason.as_deref(),
            Some(UNHOSTED_FOLDER_UNREADABLE),
            "but the reason names a fault to fix, not a folder to forget"
        );
        assert_ne!(
            UNHOSTED_FOLDER_UNREADABLE, UNHOSTED_FOLDER_GONE,
            "the two must never collapse into one sentence"
        );
    }

    /// The flag decides nothing on its own: it is only consulted when the
    /// profile could not be resolved at all. A folder that resolves is hosted
    /// however the flag is set, so a caller that sets it too eagerly cannot
    /// invent an unhosted verdict.
    #[test]
    fn the_unreadable_flag_is_only_consulted_when_the_folder_did_not_resolve() {
        let resolved = task_host(
            TaskHostFacts {
                profile_id: Some("p1"),
                profile: Some("media"),
                profile_unreadable: true,
                ..scheduled()
            },
            DaemonPresence::Absent,
        );
        assert_eq!(resolved.kind, TaskHostKind::App);
        assert_eq!(resolved.reason, None);

        // And a host-wide task names no folder, so neither reason can reach it.
        let host_wide = task_host(
            TaskHostFacts {
                profile_unreadable: true,
                ..scheduled()
            },
            DaemonPresence::Absent,
        );
        assert_eq!(host_wide.kind, TaskHostKind::App);
        assert_eq!(host_wide.reason, None);
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

    /// Gate 6, which **no production caller can reach**: `db::decode_task` files
    /// a row whose `mode` will not read under `TaskListing.unknown` before
    /// anything projects it, so the facts below can only be built by hand — and
    /// this test is the hand that builds them. Kept because the alternative to a
    /// total function is a future in-crate caller falling through to a *hosted*
    /// verdict, which is the over-claim AD-137 forbids by name.
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

    /// Lingering absent: the daemon is still the host, and the sentence stops
    /// promising the one thing a non-lingering box cannot deliver.
    ///
    /// The **real strings** are asserted rather than a substring or the kind,
    /// because the defect this closes was a sentence, not a verdict: a reword
    /// that put *"logged in or not"* back on this branch would be the original
    /// over-claim restored, and a `kind`-only assertion would not notice.
    #[test]
    fn a_linux_box_whose_enabled_unit_does_not_linger_says_the_schedule_stops_at_logout() {
        let host = task_host(scheduled(), DaemonPresence::RunsUntilLogout);
        assert_eq!(host.kind, TaskHostKind::Daemon);
        assert_eq!(
            host.sentence,
            "the keeper-syncd unit on this machine runs this while you are logged in — \
             lingering is off, so its schedule stops when your session ends"
        );
        assert_eq!(host.sentence, HOST_SENTENCE_DAEMON_UNTIL_LOGOUT);
        assert_eq!(host.reason, None);
    }

    /// The two daemon sentences are one kind and two promises, and the promise
    /// is the part that must not leak across.
    #[test]
    fn the_two_daemon_sentences_differ_and_only_the_lingering_one_claims_post_logout() {
        let lingering = task_host(scheduled(), DaemonPresence::Runs);
        let session_only = task_host(scheduled(), DaemonPresence::RunsUntilLogout);

        assert_eq!(lingering.kind, session_only.kind);
        assert_ne!(
            lingering.sentence, session_only.sentence,
            "one sentence for both would be the over-claim on one of the two boxes"
        );

        // The exact phrase the intent contract's I/O matrix quotes, and the one
        // a non-lingering machine may never be told.
        assert!(
            lingering.sentence.contains("logged in or not"),
            "the lingering box keeps AD-137's promise verbatim: {}",
            lingering.sentence
        );
        assert!(
            !session_only.sentence.contains("logged in or not"),
            "a unit that dies at logout must not claim to outlive it: {}",
            session_only.sentence
        );
        assert!(
            session_only.sentence.contains("logged in"),
            "under-claiming is fine; saying nothing about the limit is not: {}",
            session_only.sentence
        );
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
            task_host(scheduled(), DaemonPresence::RunsUntilLogout),
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

    /// The stock shape every `daemon_presence` test below mutates: a lingering
    /// unit sharing the app's own directory.
    fn lingering_unit(app: &Path) -> DaemonHostFacts<'_> {
        DaemonHostFacts {
            unit_enabled: true,
            lingering: true,
            daemon_data_dir: Some(app),
            app_data_dir: app,
        }
    }

    #[test]
    fn a_unit_that_is_not_enabled_is_absent() {
        let app = dir("/home/dev/.local/share/dev.tgorka.keeper");
        // Even pointing at the very same directory, and even lingering: not
        // enabled is not a host.
        assert_eq!(
            daemon_presence(DaemonHostFacts {
                unit_enabled: false,
                ..lingering_unit(&app)
            }),
            DaemonPresence::Absent
        );
    }

    /// The non-Linux case, and the one where the lingering question does not
    /// arise at all.
    ///
    /// `daemon_presence_here` is `Absent` by construction off Linux
    /// (`keeper/src/sync_ipc.rs`), so no Mac ever reaches a daemon sentence.
    /// Asserted with `lingering: true` **and** matching directories, which is
    /// the most credit-worthy input this function can be handed: if the gate
    /// order ever inverted, this is the test that catches a Mac being told a
    /// systemd unit runs its tasks.
    #[test]
    fn a_host_with_no_unit_reads_app_whatever_the_lingering_fact_says() {
        let app = dir("/Users/dev/Library/Application Support/dev.tgorka.keeper");
        let presence = daemon_presence(DaemonHostFacts {
            unit_enabled: false,
            ..lingering_unit(&app)
        });
        assert_eq!(presence, DaemonPresence::Absent);

        let host = task_host(scheduled(), presence);
        assert_eq!(host.kind, TaskHostKind::App);
        assert_eq!(
            host.sentence,
            "keeper runs this — only while keeper is running"
        );
        assert_eq!(host.sentence, HOST_SENTENCE_APP);
    }

    #[test]
    fn an_enabled_lingering_unit_reading_the_same_data_dir_runs_tasks() {
        let app = dir("/home/dev/.local/share/dev.tgorka.keeper");
        assert_eq!(daemon_presence(lingering_unit(&app)), DaemonPresence::Runs);
    }

    /// The finding this pass closes: one bit apart from the case above, and a
    /// different promise on screen.
    #[test]
    fn an_enabled_unit_that_does_not_linger_runs_only_until_logout() {
        let app = dir("/home/dev/.local/share/dev.tgorka.keeper");
        assert_eq!(
            daemon_presence(DaemonHostFacts {
                lingering: false,
                ..lingering_unit(&app)
            }),
            DaemonPresence::RunsUntilLogout
        );
    }

    #[test]
    fn an_enabled_unit_reading_the_stock_daemon_dir_cannot_see_this_database() {
        // The stock Linux pairing, and the reason this function exists.
        let app = dir("/home/dev/.local/share/dev.tgorka.keeper");
        let daemon = dir("/home/dev/.local/share/keeper-sync");
        // Asserted for both lingering answers: a unit that cannot see the row
        // is irrelevant however long it lives, so lingering must not promote it.
        for lingering in [true, false] {
            assert_eq!(
                daemon_presence(DaemonHostFacts {
                    lingering,
                    daemon_data_dir: Some(&daemon),
                    ..lingering_unit(&app)
                }),
                DaemonPresence::OtherDataDir,
                "lingering={lingering}"
            );
        }
    }

    #[test]
    fn an_enabled_unit_whose_data_dir_cannot_be_resolved_is_not_credited() {
        let app = dir("/home/dev/.local/share/dev.tgorka.keeper");
        assert_eq!(
            daemon_presence(DaemonHostFacts {
                daemon_data_dir: None,
                ..lingering_unit(&app)
            }),
            DaemonPresence::OtherDataDir
        );
    }

    #[test]
    fn the_stock_linux_pairing_makes_the_app_the_host_of_a_scheduled_task() {
        // The two functions composed, which is how the shell calls them.
        let app = dir("/home/dev/.local/share/dev.tgorka.keeper");
        let daemon = dir("/home/dev/.local/share/keeper-sync");
        let presence = daemon_presence(DaemonHostFacts {
            daemon_data_dir: Some(&daemon),
            ..lingering_unit(&app)
        });
        let host = task_host(scheduled(), presence);
        assert_eq!(host.kind, TaskHostKind::App);
        assert_eq!(host.sentence, HOST_SENTENCE_APP_OTHER_DATA_DIR);
    }

    /// The marker path is logind's own, spelled exactly.
    #[test]
    fn the_linger_marker_is_the_file_logind_stats_for_that_user() {
        assert_eq!(LINGER_DIR, "/var/lib/systemd/linger");
        assert_eq!(
            linger_marker_path("dev"),
            Some(PathBuf::from("/var/lib/systemd/linger/dev"))
        );
    }

    /// A name that would make the probe answer *lingering* for a user who does
    /// not linger is refused instead.
    ///
    /// `Path::join("")` and `join(".")` both yield [`LINGER_DIR`] itself, which
    /// exists on every systemd box — so an empty or dotted `$USER` would turn a
    /// present directory into a false promise. `..` walks out of it, and a name
    /// with a `/` names some other file entirely. This is the one direction the
    /// module may never fail in, so it is a refusal rather than a sanitisation.
    #[test]
    fn a_user_name_that_could_name_the_directory_itself_is_refused() {
        for hostile in ["", ".", "..", "/", "dev/../root", "../root", "a/b"] {
            assert_eq!(
                linger_marker_path(hostile),
                None,
                "{hostile:?} must not become a linger probe"
            );
        }
        assert_eq!(linger_marker_path("de\0v"), None, "a NUL cannot be stat'ed");
    }

    /// On a machine with no systemd at all the probe is `false`, not an error.
    ///
    /// [`Path::exists`] returns `bool` and folds every `ENOENT` on the way down
    /// — no `/var/lib/systemd`, no `linger/`, no file — into the same answer, so
    /// a container or a non-systemd distribution reads *not lingering* rather
    /// than propagating a failure the caller would have to invent a verdict for.
    /// That answer is also unreachable in production: `systemctl --user
    /// is-enabled` fails on such a box, so [`DaemonPresence::Absent`] is
    /// returned before lingering is consulted.
    #[test]
    fn probing_a_user_who_does_not_linger_answers_false_rather_than_failing() {
        let path =
            linger_marker_path("keeper-linger-probe-no-such-user").expect("a plain name is usable");
        assert!(
            !path.exists(),
            "{} must not exist; the probe's negative answer is a bool, not an error",
            path.display()
        );
    }

    // -----------------------------------------------------------------------
    // The paced-work projection (Story 58.7)
    // -----------------------------------------------------------------------

    /// An enabled, non-removable folder with no vault, on the engine's real
    /// intervals — mutated per test by struct update, `scheduled()`'s idiom.
    fn folder() -> PacedFolderFacts<'static> {
        PacedFolderFacts {
            profile_id: "p1",
            profile: "keeper",
            enabled: true,
            removable: false,
            scan_interval_ms: 15_000,
            scan_governed: false,
            sweep_interval_ms: 3_600_000,
            notes: None,
        }
    }

    /// The vault cadence the profile defaults ship with, on a vault the registry
    /// actually holds — the only shape that is paced.
    fn vault() -> PacedNotesFacts {
        PacedNotesFacts {
            commit_idle_ms: 2_000,
            push_interval_ms: 30_000,
            registered: true,
        }
    }

    fn row_of(rows: &[PacedWorkVm], kind: PacedWorkKind) -> &PacedWorkVm {
        rows.iter()
            .find(|row| row.kind == kind)
            .unwrap_or_else(|| panic!("no {kind:?} row in {rows:?}"))
    }

    #[test]
    fn a_folder_with_no_vault_yields_a_scan_row_and_a_sweep_row_and_nothing_else() {
        let rows = paced_work(&[folder()]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].kind, PacedWorkKind::Scan);
        assert_eq!(rows[0].id, "scan:p1");
        assert_eq!(rows[1].kind, PacedWorkKind::ScratchSweep);
        assert_eq!(rows[1].id, "sweep:p1");
        for row in &rows {
            assert_eq!(row.profile_id, "p1");
            assert_eq!(row.profile, "keeper");
            assert_eq!(row.standing, PacedWorkStanding::Paced);
        }
    }

    #[test]
    fn a_folder_that_holds_a_vault_yields_the_notes_row_too() {
        let rows = paced_work(&[PacedFolderFacts {
            notes: Some(vault()),
            ..folder()
        }]);
        assert_eq!(rows.len(), 3);
        let notes = row_of(&rows, PacedWorkKind::NotesCadence);
        assert_eq!(notes.id, "notes:p1");
        assert_eq!(
            notes.cadence.as_deref(),
            Some("committed after 2 seconds of quiet, pushed within 30 seconds")
        );
        assert_eq!(notes.sentence, PACED_SENTENCE_NOTES_PACED);
        // The fact that separates it from every task row above it in the view.
        assert!(notes.sentence.contains("keeper-syncd never does"));
    }

    #[test]
    fn the_scan_row_reads_the_effective_interval_and_hedges_it() {
        let rows = paced_work(&[folder()]);
        let scan = row_of(&rows, PacedWorkKind::Scan);
        assert_eq!(scan.cadence.as_deref(), Some("about every 15 seconds"));
        assert_eq!(scan.sentence, PACED_SENTENCE_SCAN_PACED);
    }

    /// The DW-116 row: a profile whose stored `poll_interval_ms` is zero.
    ///
    /// The caller passes `effective_poll_interval_ms()`, so what reaches here is
    /// the floor and never the stored zero — a cadence of "0 ms" would advertise
    /// a scan that never stops.
    #[test]
    fn a_floored_poll_interval_projects_the_floor_and_never_the_stored_zero() {
        let rows = paced_work(&[PacedFolderFacts {
            scan_interval_ms: 2_000,
            ..folder()
        }]);
        let scan = row_of(&rows, PacedWorkKind::Scan);
        assert_eq!(scan.cadence.as_deref(), Some("about every 2 seconds"));
    }

    #[test]
    fn the_sweep_row_reads_the_engines_hour() {
        let rows = paced_work(&[folder()]);
        let sweep = row_of(&rows, PacedWorkKind::ScratchSweep);
        assert_eq!(sweep.cadence.as_deref(), Some("every 1 hour"));
        assert_eq!(sweep.sentence, PACED_SENTENCE_SWEEP_PACED);
        // It names what it deletes: "sweep" beside a folder of the user's own
        // files must not read as a sweep of the files.
        assert!(sweep.sentence.contains("transfer scratch"));
    }

    /// A paused folder silences everything projected from it, and paused beats
    /// governed: `tick` skips a disabled profile before it reaches any of this.
    #[test]
    fn a_paused_folder_advertises_no_cadence_on_any_of_its_rows() {
        let rows = paced_work(&[PacedFolderFacts {
            enabled: false,
            scan_governed: true,
            removable: true,
            notes: Some(vault()),
            ..folder()
        }]);
        assert_eq!(rows.len(), 3);
        for row in &rows {
            assert_eq!(row.standing, PacedWorkStanding::Paused, "{:?}", row.kind);
            assert_eq!(row.cadence, None, "{:?}", row.kind);
            assert_eq!(row.sentence, PACED_SENTENCE_PAUSED, "{:?}", row.kind);
        }
    }

    /// Story 58.8's stand-down, and the row beside it that must NOT move.
    #[test]
    fn a_governed_scan_row_stands_down_while_the_sweep_and_notes_rows_do_not() {
        let rows = paced_work(&[PacedFolderFacts {
            scan_governed: true,
            notes: Some(vault()),
            ..folder()
        }]);
        let scan = row_of(&rows, PacedWorkKind::Scan);
        assert_eq!(scan.standing, PacedWorkStanding::Governed);
        assert_eq!(scan.cadence, None);
        assert_eq!(scan.sentence, PACED_SENTENCE_SCAN_GOVERNED);
        // Governance takes the scan's backstop and nothing else.
        for kind in [PacedWorkKind::ScratchSweep, PacedWorkKind::NotesCadence] {
            let row = row_of(&rows, kind);
            assert_eq!(row.standing, PacedWorkStanding::Paced, "{kind:?}");
            assert!(row.cadence.is_some(), "{kind:?}");
        }
    }

    /// A governance read that failed is not a governance state: the caller
    /// passes `false` and the row prints its interval, which is true — the paced
    /// poll genuinely still runs when nothing was found to have taken it.
    #[test]
    fn a_folder_no_task_governs_keeps_its_paced_backstop() {
        let rows = paced_work(&[folder()]);
        let scan = row_of(&rows, PacedWorkKind::Scan);
        assert_eq!(scan.standing, PacedWorkStanding::Paced);
        assert_eq!(scan.cadence.as_deref(), Some("about every 15 seconds"));
    }

    /// The clause rides the two rows whose work would have *looked* at the
    /// folder, and not the notes row.
    ///
    /// The sweep earned it after review: `sweep_scratch_if_due` does not check
    /// `volume_ready`, so with the drive away its `read_dir` finds nothing and
    /// deletes nothing — an unhedged *"keeper deletes transfer scratch on this
    /// cadence"* was claiming work that was not happening. The notes row is still
    /// excluded, because a vault on an absent drive is unregistered and its own
    /// sentence already names the missing folder as the reason.
    #[test]
    fn a_removable_folder_says_so_on_the_rows_that_would_have_looked() {
        let rows = paced_work(&[PacedFolderFacts {
            removable: true,
            notes: Some(vault()),
            ..folder()
        }]);
        assert_eq!(
            row_of(&rows, PacedWorkKind::Scan).sentence,
            format!("{PACED_SENTENCE_SCAN_PACED}{PACED_SENTENCE_REMOVABLE_CLAUSE}")
        );
        assert_eq!(
            row_of(&rows, PacedWorkKind::ScratchSweep).sentence,
            format!("{PACED_SENTENCE_SWEEP_PACED}{PACED_SENTENCE_REMOVABLE_CLAUSE}")
        );
        assert!(
            !row_of(&rows, PacedWorkKind::NotesCadence)
                .sentence
                .contains("removable media"),
            "the notes row names the folder it cannot find, not the drive"
        );
        // It rides governance too: the look is still what finds nothing.
        let governed = paced_work(&[PacedFolderFacts {
            removable: true,
            scan_governed: true,
            ..folder()
        }]);
        assert!(row_of(&governed, PacedWorkKind::Scan)
            .sentence
            .contains("removable media"));
        // And a paused folder keeps its one reason: pausing, not the drive.
        let paused = paced_work(&[PacedFolderFacts {
            removable: true,
            enabled: false,
            ..folder()
        }]);
        for row in &paused {
            assert_eq!(row.sentence, PACED_SENTENCE_PAUSED, "{:?}", row.kind);
        }
    }

    /// The over-claim the registration fact exists to prevent: a configured
    /// vault whose folder the registry could not resolve is reached by no
    /// cadence tick and no flush, so it must not advertise one.
    #[test]
    fn an_unregistered_vault_advertises_no_cadence_and_names_the_registry() {
        let rows = paced_work(&[PacedFolderFacts {
            notes: Some(PacedNotesFacts {
                registered: false,
                ..vault()
            }),
            ..folder()
        }]);
        let notes = row_of(&rows, PacedWorkKind::NotesCadence);
        assert_eq!(notes.standing, PacedWorkStanding::Unregistered);
        assert_eq!(notes.cadence, None);
        assert_eq!(notes.sentence, PACED_SENTENCE_NOTES_UNREGISTERED);
        // Neither of the other two goes through the vault registry.
        for kind in [PacedWorkKind::Scan, PacedWorkKind::ScratchSweep] {
            assert_eq!(
                row_of(&rows, kind).standing,
                PacedWorkStanding::Paced,
                "{kind:?}"
            );
        }
    }

    /// Pausing outranks registration: a paused folder gets one reason, and it is
    /// the one somebody would recognise.
    #[test]
    fn a_paused_folder_with_an_unregistered_vault_reads_as_paused() {
        let rows = paced_work(&[PacedFolderFacts {
            enabled: false,
            notes: Some(PacedNotesFacts {
                registered: false,
                ..vault()
            }),
            ..folder()
        }]);
        let notes = row_of(&rows, PacedWorkKind::NotesCadence);
        assert_eq!(notes.standing, PacedWorkStanding::Paused);
        assert_eq!(notes.sentence, PACED_SENTENCE_PAUSED);
    }

    /// An interval within rounding distance of `u64::MAX` used to panic a debug
    /// build in `ms + 500` and wrap a release one to an absurdly small cadence.
    /// Nothing shipped reaches it; a projection that exists to be honest simply
    /// must not be able to panic on a number a config file can hold.
    #[test]
    fn an_absurd_interval_saturates_instead_of_overflowing() {
        let rows = paced_work(&[PacedFolderFacts {
            scan_interval_ms: u64::MAX,
            ..folder()
        }]);
        let scan = row_of(&rows, PacedWorkKind::Scan);
        // `u64::MAX` saturates rather than wrapping, so the rounding half is
        // absorbed and the number is the truncated one. Either reading is
        // nonsense to a person; the point is that neither is a panic.
        assert_eq!(
            scan.cadence.as_deref(),
            Some("about every 18446744073709551 seconds")
        );
    }

    /// The invariant, over every shape this projection can produce. It is the
    /// one property no TypeScript branch may be trusted with: *"paused, about
    /// every 15 seconds"* has to be impossible to build.
    #[test]
    fn a_cadence_is_present_exactly_when_the_standing_is_paced() {
        let mut rows = Vec::new();
        for enabled in [true, false] {
            for governed in [true, false] {
                for removable in [true, false] {
                    // Every vault shape, including the unregistered one the
                    // fourth standing exists for.
                    for notes in [
                        Some(vault()),
                        Some(PacedNotesFacts {
                            registered: false,
                            ..vault()
                        }),
                        None,
                    ] {
                        rows.extend(paced_work(&[PacedFolderFacts {
                            enabled,
                            scan_governed: governed,
                            removable,
                            notes,
                            ..folder()
                        }]));
                    }
                }
            }
        }
        assert!(!rows.is_empty());
        // Every standing this build can emit really did occur above, or the
        // sweep would be asserting the invariant over a subset of the space
        // while reading as if it covered all of it.
        for standing in [
            PacedWorkStanding::Paced,
            PacedWorkStanding::Paused,
            PacedWorkStanding::Governed,
            PacedWorkStanding::Unregistered,
        ] {
            assert!(
                rows.iter().any(|row| row.standing == standing),
                "{standing:?} never occurred in the sweep"
            );
        }
        for row in &rows {
            assert_eq!(
                row.cadence.is_some(),
                row.standing == PacedWorkStanding::Paced,
                "{row:?}"
            );
        }
    }

    #[test]
    fn several_folders_project_their_rows_together_and_keyed_by_id() {
        let rows = paced_work(&[
            folder(),
            PacedFolderFacts {
                profile_id: "p2",
                profile: "vault",
                notes: Some(vault()),
                ..folder()
            },
        ]);
        let ids: Vec<&str> = rows.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(
            ids,
            ["scan:p1", "sweep:p1", "scan:p2", "sweep:p2", "notes:p2"]
        );
    }

    #[test]
    fn no_folders_project_no_rows() {
        assert!(paced_work(&[]).is_empty());
    }

    /// The coarsest-exact rule at its boundaries, including the one rounded case
    /// and the sub-second one that is deliberately not rounded.
    #[test]
    fn duration_words_reads_in_the_coarsest_unit_that_divides_exactly() {
        assert_eq!(duration_words(3_600_000), "1 hour");
        assert_eq!(duration_words(7_200_000), "2 hours");
        // 90 minutes does not fill an hour twice, so it stays minutes.
        assert_eq!(duration_words(5_400_000), "90 minutes");
        assert_eq!(duration_words(60_000), "1 minute");
        assert_eq!(duration_words(900_000), "15 minutes");
        assert_eq!(duration_words(1_000), "1 second");
        assert_eq!(duration_words(15_000), "15 seconds");
        // Rounded, not truncated.
        assert_eq!(duration_words(1_500), "2 seconds");
        assert_eq!(duration_words(1_400), "1 second");
        assert_eq!(duration_words(59_500), "60 seconds");
        // Below a second nothing is rounded away.
        assert_eq!(duration_words(999), "999 ms");
        assert_eq!(duration_words(1), "1 ms");
        assert_eq!(duration_words(0), "0 ms");
    }

    /// The structural half of the under-claim: the wire object has exactly seven
    /// keys, and none of them could carry a next run or a last run.
    ///
    /// Asserted on the emitted JSON rather than on the Rust type, because that is
    /// what the view consumes — and asserted as an exact set rather than by
    /// checking two names are absent, so a *third* over-claiming key added later
    /// fails here too.
    #[test]
    fn the_paced_wire_shape_cannot_claim_a_next_run_or_a_last_run() {
        let rows = paced_work(&[PacedFolderFacts {
            notes: Some(vault()),
            ..folder()
        }]);
        for row in &rows {
            let json = serde_json::to_value(row).expect("a PacedWorkVm serializes");
            let object = json.as_object().expect("it serializes as an object");
            let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
            keys.sort_unstable();
            assert_eq!(
                keys,
                [
                    "cadence",
                    "id",
                    "kind",
                    "profile",
                    "profileId",
                    "sentence",
                    "standing",
                ]
            );
        }
    }

    /// The spellings the frontend switches on, pinned: `camelCase` on the enums
    /// means `scratchSweep` and `notesCadence`, not `ScratchSweep`.
    #[test]
    fn the_paced_enums_go_on_the_wire_in_camel_case() {
        let kinds = [
            (PacedWorkKind::Scan, "scan"),
            (PacedWorkKind::ScratchSweep, "scratchSweep"),
            (PacedWorkKind::NotesCadence, "notesCadence"),
        ];
        for (kind, spelling) in kinds {
            assert_eq!(
                serde_json::to_value(kind).expect("a kind serializes"),
                serde_json::Value::String(spelling.to_owned())
            );
        }
        let standings = [
            (PacedWorkStanding::Paced, "paced"),
            (PacedWorkStanding::Paused, "paused"),
            (PacedWorkStanding::Governed, "governed"),
        ];
        for (standing, spelling) in standings {
            assert_eq!(
                serde_json::to_value(standing).expect("a standing serializes"),
                serde_json::Value::String(spelling.to_owned())
            );
        }
    }
}
