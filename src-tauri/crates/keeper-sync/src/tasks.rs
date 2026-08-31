//! Named, scheduled, recorded housekeeping work — the vocabulary, the schedule
//! dialect and the pure due-gate (AD-135, AD-136).
//!
//! This module holds two rules and nothing else. **A schedule is refused where
//! it is written**: an expression keeper cannot parse — including one that
//! parses to an instant that never arrives — is a [`SyncError::Config`] at save
//! time with the expression quoted, because a schedule nobody can parse is a
//! schedule that would silently never run and nobody notices the absence of
//! housekeeping. **The decision is pure**: [`decide`] takes a [`TaskState`], an
//! `Option<&TaskSchedule>` and an `i64` clock, so the state machine is asserted
//! against literal integers rather than against a real clock — AD-136 evaluates
//! it as a due-gate on the 1 Hz tick each host already runs, and AD-62 forbids
//! this module from owning a thread, an interval or a timer of its own.

use crate::error::{Result, SyncError};

/// Fastest a task may be scheduled to fire.
///
/// Below a minute the scheduler spends more time waking than working — the
/// reason `keeper-syncd`'s `MIN_POLL_INTERVAL_MS` exists. Only
/// `every <n><unit>` can reach this floor: a 5-field cron expression's finest
/// resolution **is** one minute, so it satisfies the floor by construction and
/// needs no check. That is stated here so nobody later "fixes" the parser by
/// adding one.
///
/// Refused rather than clamped, diverging from
/// [`crate::profile::MIN_POLL_INTERVAL_MS`] on purpose: `tasks.schedule` is a
/// brand-new field, so no stored row can carry a legacy zero-by-omission, which
/// means every out-of-range value here is one a person typed and deserves to be
/// told about.
pub const MIN_SCHEDULE_INTERVAL_MS: i64 = 60_000;

/// Slowest a task may be scheduled to fire.
///
/// The floor's mirror image, and it exists for the same reason the floor does:
/// `every 100000000d` parses, multiplies without overflowing, and arms a window
/// in the year 275 000 — a task that reports itself enabled and never fires,
/// which is exactly the shape [`TaskSchedule::parse`] refuses two branches later
/// for `0 0 30 2 *`. A schedule slower than a year is a calendar pattern rather
/// than an interval, and the cron half of the dialect expresses those exactly
/// (`0 0 1 1 *`), so nothing is lost by refusing here.
const MAX_SCHEDULE_INTERVAL_MS: i64 = 366 * 24 * 60 * 60 * 1_000;

/// How long an open window may sit unserved before keeper concludes **nobody
/// was home** (Story 58.4, FR-356, AD-139).
///
/// The policy's detection boundary, and the only thing that separates *"a window
/// this host has not reached yet"* from *"a window nobody was here to serve"*.
/// [`TaskMissedPolicy::RunNow`] does not consult it; the other two act only past
/// it, so a window that opens while a host is present is served normally under
/// **all three** settings. That is what makes `on_missed` a policy about *missed*
/// windows rather than a general re-timing of the schedule — the owner's own
/// qualifier, *"w takiej sytuacji"*.
///
/// **Why `skip` cannot do without it.** The due-gate runs on a 1 Hz tick, so
/// with no grace a `skip` task's window would be abandoned within a second of
/// opening, on every window, forever: a task that reports itself enabled and
/// scheduled while nothing ever runs, which is the one shape this whole feature
/// exists to close (`Engine::arm_task_window`'s load-bearing `warn`).
///
/// Fifteen minutes: long enough that a host that is present serves its own
/// window well inside it (the tick is one second, and the only thing that can
/// hold it up that far is a sync pass — in which case nobody *did* serve the
/// window), and short enough that a `@daily` task missed overnight is still
/// noticed on the day it was missed.
pub const TASK_MISSED_GRACE_MS: i64 = 15 * 60_000;

/// How long [`TaskMissedPolicy::Delay`] holds a missed window back, measured
/// **from the instant a host noticed it** (Story 58.4, FR-356, AD-139).
///
/// A separate number from [`TASK_MISSED_GRACE_MS`] because it answers a separate
/// question — that one is *when do we conclude nobody was home*, this one is
/// *how long do we then wait* — and neither is derived from the other.
///
/// **The anchor is the noticing, not the window**, and that is the whole
/// correctness of this setting. Anchoring on `next_due_ms` — the first draft of
/// this story, and the literal reading of AD-139's arithmetic — makes the option
/// vanish in precisely the scenario the owner described: an hourly task, a host
/// back two hours late, so `next_due_ms + delay` is already an hour in the past
/// and `delay` serves the window immediately, identically to `run_now`. Two of
/// his three options would have been one option.
///
/// It costs **no second column** (AD-139's rule is intact): the postponement is
/// written into `next_due_ms` itself, through the same forward-only
/// compare-and-set a skip uses, so it is still exactly one stored instant and
/// AD-138's no-enumeration rule holds by construction. Persisting it rather than
/// recomputing it also means a restart *inside* the delay respects the delay,
/// which a `decide`-side wait could not have offered.
///
/// Thirty minutes, and longer than the grace on purpose: the case this setting
/// is for is a machine that has just come back, where the minutes after the
/// grace elapses are the busiest — a boot, a login, a mail client and a browser
/// all waking at once — and housekeeping over a git remote is exactly the work
/// that should not join that.
///
/// **The default rather than the only value** (Story 59.6, FR-366). A task may
/// carry its own `missed_delay_ms`, and an absent one means exactly this
/// constant — so every row written before that column existed keeps meaning what
/// it meant. [`effective_missed_delay_ms`] is the single place that resolution
/// happens, and [`validate_missed_delay_ms`] is the single place an override is
/// refused; the number below stays the one a person gets by not choosing.
pub const TASK_MISSED_DELAY_MS: i64 = 30 * 60_000;

/// Milliseconds in one minute — the resolution of the whole cron dialect.
const MS_PER_MINUTE: i64 = 60_000;
/// Milliseconds in one day, the unit [`CronSpec::next_due_after`] walks in.
const MS_PER_DAY: i64 = 24 * 60 * MS_PER_MINUTE;
/// Minutes in one day, the inner scan's bound.
const MINUTES_PER_DAY: i64 = 24 * 60;

/// How many days forward a cron search looks before giving up.
///
/// Eight years plus two days, sized by the sparsest expression the grammar can
/// express: `0 0 29 2 *`. Consecutive 29 Februaries are normally 1 461 days
/// apart, but 2100 is not a leap year, so between 2096 and 2104 the gap is
/// 2 922 days. A four-year window would have made that one schedule resolve to
/// nothing for eight years — enabled, scheduled and silent, which is the failure
/// this whole module exists to refuse.
///
/// It costs nothing to be this wide. The loop exits on the first matching day,
/// so an ordinary schedule takes one or two iterations and only a Feb-29 pattern
/// ever walks far; and the walk is integer arithmetic with no allocation.
const SEARCH_DAYS: i64 = 366 * 8 + 2;

/// The most days each month can have, indexed by month number, February at 29
/// because leap years exist.
///
/// Read only by [`CronSpec::matches_any_date`], which answers "could this
/// pattern ever name a real date" in constant time instead of by walking a
/// calendar.
const MAX_DAYS_IN_MONTH: [u32; 13] = [0, 31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

/// What kind of work a task performs — a closed vocabulary of keeper's own
/// verbs, never a shell string.
///
/// Closed on purpose, and forward-compatible because of it: a stored `kind`
/// this build has never heard of is **skipped and listed as unknown, never
/// fatal** (NFR-43), which is what lets one host write a task an older binary
/// on the other host cannot run.
///
/// **There is no `update` variant and there must never be one.** `docs/sync.md`
/// refuses unattended replacement of the keeper binary, and a task kind is the
/// one place that refusal could be quietly undone — a schedule that installs
/// software is exactly the thing the anti-timer stance forbids. Since
/// `from_stored` returns `None` for `"update"`, a hand-written row naming it is
/// skipped like any other unknown kind rather than honoured.
///
/// **Why the vocabulary stays closed, stated once here because this is where a
/// third variant proves it can grow.** A kind names a verb *keeper* owns, so
/// every kind is code that already exists in this workspace, already has its
/// own refusals, and already answers to the same reviewer. A shell string
/// would name a verb nobody in this tree wrote: the daemon's egress is
/// disclosed in `docs/egress.md` and diffed against the previous tag by the
/// release workflow, and a user command can reach any host on the internet
/// while that diff shows nothing whatsoever. There is also no task timeout —
/// only the one-hour lease — and no stdout capture, so an arbitrary command
/// that hangs would hold a lease for an hour and report a line nobody wrote.
/// `ARCHITECTURE-SCHEDULED-TASKS.md`'s `## Deferred` entry is therefore
/// **still deferred**, and adding a variant here does not touch it: the price
/// of a closed vocabulary is that somebody must write the arm, which is
/// exactly the price being kept.
///
/// `Sync` is the kind that exists first because its effect is already real and
/// already safe: `sync --once` is documented as the cron entry point, and
/// `Engine::sync_once` opens by taking the same per-profile reservation the
/// host's own tick takes. That makes NFR-42's "a task never holds a git index
/// concurrently with its host's sync pass" a structural fact about the code
/// rather than a promise a reviewer has to trust.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind {
    /// Sync one profile's folder once, through the engine's ordinary one-shot
    /// pass and its reservation.
    Sync,
    /// One release sweep over the named folder, or over every enabled folder
    /// when the task is host-wide (Story 57.4, FR-349, FR-350).
    ///
    /// `Engine::release_expired` is the whole implementation — the same body
    /// the success edge of a sync runs — so every Epic 56 refusal, both AD-131
    /// clocks, the pin, 56.17's per-file deadline and both budgets apply to it
    /// identically. **A task is not a privileged caller**; what a task trigger
    /// changes is only *when* the pass may look.
    ///
    /// **Why this could not be the first kind.** Story 57.1's Design Notes
    /// settle it: the sweep carries its own hourly `release_is_due` **look**
    /// gate, so a task's schedule would not have controlled it — a nightly
    /// release task would have fired at 03:00 and been declined by an interval
    /// that knows nothing about schedules. Threading a triggered-run bypass
    /// through that gate, together with the off/manual/scheduled mode this row
    /// now imposes on the success edge too, *is* Story 57.4. `Sync` needed none
    /// of it, which is why "a due task really runs" could be asserted a wave
    /// earlier without a stub.
    Release,
    /// One verification pass over the named folder, or over every enabled
    /// folder when the task is host-wide (Story 59.9).
    ///
    /// `Engine::verify` is the whole implementation, unchanged and un-widened:
    /// the same body `keeper-syncd verify` runs, with the same four free
    /// excuses AD-129 requires before an absent object is called normal. What
    /// a task adds is a **schedule and a memory** — the one thing a check
    /// most needs, because a check nobody remembers running is
    /// indistinguishable from a check that stopped running, which is the
    /// sentence `verify`'s own `virtual` count already exists to answer.
    ///
    /// **Why this kind needed no new machinery, and why that is the test it
    /// had to pass.** It reads: no worktree file is written, no object is
    /// added to the store, and it opens the repository through
    /// `git::repo::open_read_only` precisely so it does not do the ordinary
    /// door's housekeeping — *"a check that repairs what it is checking is not
    /// a check"*. So it takes **no reservation** and needs none, which also
    /// means `TaskOutcome::Busy` is unreachable for this kind: a check that
    /// stood aside while the host synced would report nothing on exactly the
    /// folders that are moving. It asks **no network** either — the remote
    /// half of `verify` is `--remote`, one batch round trip per object, and a
    /// nightly task over NFR-41's ten-thousand-path fixture is the last place
    /// that belongs.
    ///
    /// It is also the kind whose `detail` line is worth reading on a run that
    /// went fine: *"1000 paths checked, 0 bad, 1000 virtual in 1 folders"* is
    /// the answer to a question `sync` and `release` cannot be asked.
    Verify,
}

impl TaskKind {
    /// Stable on-disk spelling, kept separate from any serde representation so
    /// a UI-facing rename can never invalidate stored rows.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sync => "sync",
            Self::Release => "release",
            Self::Verify => "verify",
        }
    }

    /// Parse the *stored* spelling. `None` is the forward-compatibility door,
    /// not an error: the caller skips the row and reports it as unknown.
    pub fn from_stored(value: &str) -> Option<Self> {
        match value {
            "sync" => Some(Self::Sync),
            "release" => Some(Self::Release),
            "verify" => Some(Self::Verify),
            _ => None,
        }
    }
}

/// Who may trigger a task, which is a different question from whether the row
/// is live.
///
/// Only [`Self::Scheduled`] is ever *due*; [`Self::Manual`] runs solely through
/// an explicit run-now, and [`Self::Off`] refuses even that. Both this and
/// `enabled` exist per AD-135 because they answer different questions, so the
/// gate reads both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskMode {
    /// Nothing may run this task, not even a human asking directly.
    Off,
    /// Runs only when somebody asks; the schedule is remembered, not obeyed.
    Manual,
    /// Runs on its schedule, on whichever host's tick sees it due first.
    Scheduled,
}

impl TaskMode {
    /// Stable on-disk spelling, kept separate from any serde representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Manual => "manual",
            Self::Scheduled => "scheduled",
        }
    }

    /// Parse the *stored* spelling; `None` means this build cannot read the row
    /// and must skip it rather than guess a mode that might run something.
    pub fn from_stored(value: &str) -> Option<Self> {
        match value {
            "off" => Some(Self::Off),
            "manual" => Some(Self::Manual),
            "scheduled" => Some(Self::Scheduled),
            _ => None,
        }
    }
}

/// What to do about a window that fell due while nobody was home (Story 58.4,
/// FR-356, AD-139).
///
/// Two of these three already exist in the tree, unnamed and unselectable, which
/// is the reason a policy is worth having rather than a behaviour worth
/// inventing. [`Self::RunNow`] is what an ordinary restart does: nothing
/// rewrites the row, so the stored past window fires on the next tick.
/// [`Self::Skip`] is what [`crate::db::upsert_task`]'s three service edges do —
/// they clear `next_due_ms` precisely so a stale window cannot fire. So today
/// the operator gets one or the other depending on which door the row last came
/// through; this makes the choice explicit and per task.
///
/// **No setting may enumerate more than one missed window** (AD-138, NFR-44).
/// That is not tidiness: [`TaskKind::Release`] deletes local content, so N
/// catch-up sweeps are N deletion passes at instants nobody chose. It holds by
/// construction here — `next_due_ms` is one `i64`, not a queue, so "overdue by
/// one window" and "overdue by two hundred" are the same state and nothing in
/// [`decide`] can tell them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TaskMissedPolicy {
    /// Serve the window on the first tick that sees it open.
    ///
    /// The default, and the default **because it reproduces today's restart
    /// behaviour**, so no existing install changes meaning on upgrade. It is
    /// also precisely systemd's `Persistent=true` semantics in-process, which
    /// the shipped timer already words the same way: *"A trigger missed while
    /// the machine was off or asleep fires once when it comes back … Once, not
    /// once per missed day"*.
    #[default]
    RunNow,
    /// Hold a missed window back, and serve it **once**,
    /// [`TASK_MISSED_DELAY_MS`] after a host noticed it.
    ///
    /// Not a floor on how soon, and not a re-timing of the schedule: a window
    /// that opens while a host is present is served normally, because this policy
    /// is about a window that was *missed*. Only past [`TASK_MISSED_GRACE_MS`]
    /// does it act, and then it acts by **writing** — see [`Action::Postpone`] —
    /// so the wait is a persisted instant rather than a decision retaken on every
    /// tick.
    ///
    /// Two things follow from that, and both are the reason the anchor moved.
    /// `db::claim_task`'s `next_due_ms <= now` condition correctly **fails** for
    /// the length of the delay, so the run is held back by the arbiter rather
    /// than beside it. And a restart inside the delay respects the delay, because
    /// the instant is on the row rather than in a process that has just started.
    Delay,
    /// Abandon a window nobody served and arm the next one.
    ///
    /// Only once it has been open for [`TASK_MISSED_GRACE_MS`]: a host that is
    /// present serves its own window, and `skip` is about the window a host that
    /// was absent left behind. It **must re-arm** — see [`Action::Skip`].
    Skip,
}

impl TaskMissedPolicy {
    /// Stable on-disk spelling, kept separate from any serde representation for
    /// [`TaskKind::as_str`]'s reason.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RunNow => "run_now",
            Self::Delay => "delay",
            Self::Skip => "skip",
        }
    }

    /// Parse the *stored* spelling; `None` for a policy a newer keeper wrote, so
    /// the row is skipped and listed rather than run under a policy this build
    /// guessed (NFR-43). Guessing is the dangerous direction: reading an unknown
    /// spelling as `run_now` would run a window its author asked to skip.
    pub fn from_stored(value: &str) -> Option<Self> {
        match value {
            "run_now" => Some(Self::RunNow),
            "delay" => Some(Self::Delay),
            "skip" => Some(Self::Skip),
            _ => None,
        }
    }
}

/// Who asked for a run through the explicit run verb (Story 58.6, FR-358).
///
/// The engine's own `TaskTrigger` is private, and deliberately: `Scheduled` is
/// the due-gate's to pass and nobody outside may claim to be it. This is the
/// half a caller *may* choose, and it exists because **a timer is not a person**.
///
/// Nothing about the process can tell the two apart. `keeper-syncd-tasks@.service`
/// runs the same binary with the same argv shape somebody would type, and a
/// person may equally run the verb from a script — so an environment probe, a
/// parent-process check or a TTY test would all be heuristics that are wrong
/// silently, on the one box where the defect this distinction fixes exists. Only
/// the caller knows, so the caller says.
///
/// A type rather than a `bool`, at every call site, for the reason this tree
/// already states about two adjacent booleans: `run_task_now(id, true)` is a call
/// nobody can read and anybody can invert.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TaskRunDriver {
    /// A person, or the app's Run now button, or a script somebody wrote.
    ///
    /// Asking for a run **now**, which is not asking whether one is due: the
    /// claim demands no open window, and that is the whole point of it.
    #[default]
    Person,
    /// A scheduled external driver — Story 57.7's `Persistent=true` timer —
    /// wearing a manual verb.
    ///
    /// It gets the *window* discipline a scheduled run gets whenever an
    /// in-process host is pacing the same task, because otherwise one missed
    /// window yields two runs: the timer's request bypasses `db::claim_task`'s
    /// window condition while the daemon's next tick claims the same past window
    /// independently. When nothing in-process paces the task, this timer **is**
    /// the schedule and claims like a request.
    Timer,
}

/// How one run ended — or, for two of the seven, why there was no run at all.
///
/// Five of the seven are deliberately **not** failures, and keeping them apart
/// is what stops a scheduled task crying wolf once an hour.
///
/// [`Self::Busy`] records that the target was already in use when the task came
/// due, which is NFR-42's one-operation-per-folder rule working rather than
/// something to notify a user about. [`Self::Deferred`] records that the
/// conditions for the work were not met — an unplugged drive above all, which
/// this tree already settles by name: *"an unplugged volume is absence, never
/// failure"* (AD-48), and `SyncError::MediaAbsent` is
/// `Retriability::Deferred` for exactly that reason. [`Self::Abandoned`] is
/// written *by the next host* when it reclaims an expired lease, so a killed
/// process leaves a closed run rather than a wedged row.
///
/// [`Self::Declined`] and [`Self::Postponed`] are the odd two and the newest
/// (Story 58.5), and the thing that makes them odd is worth stating: **every
/// other variant is written by a host that took the lease**. Five of the seven
/// therefore assert that a host was present and reached the task, which is
/// precisely what is *not* true of a window that fell due while nobody was home.
/// The pair are not interchangeable either: a declined window will **never** be
/// served, a postponed one **will** be, later — so a surface that conflated them
/// would tell somebody their housekeeping had been dropped when it had only been
/// held back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskOutcome {
    /// The work ran and did what it was asked to.
    Ok,
    /// The work could not start because its target was already in use.
    Busy,
    /// The work did not run because a condition it waits on was not met.
    Deferred,
    /// The work ran and failed; `detail` carries the reason.
    Failed,
    /// The run was never closed by the host that started it.
    Abandoned,
    /// There was no run: the task's missed-window policy declined this window
    /// and armed the next one (Story 58.5, FR-357, AD-140).
    ///
    /// The one variant written **without a lease**, because nothing ran and
    /// nothing needed serializing. It exists because none of the five above can
    /// carry the fact, and their own doc comments settle it: `Busy` needs a
    /// target that was in use, `Deferred` needs a condition that was waited on,
    /// `Abandoned` needs a host that started a run, and `Ok` and `Failed` both
    /// assert the work ran.
    ///
    /// **`Deferred` in particular must not be reused for this.**
    /// `Engine::next_task_window` consumes `Deferred` to retry within
    /// `TASK_RETRY_MS`, so it means *"try again very soon"* — the exact opposite
    /// of *"this window is abandoned"*, and overloading it would silently turn
    /// `on_missed = skip` into `on_missed = retry in a minute`. For the mirror
    /// reason this variant stays **out** of that retry group: a decline has
    /// already moved the window forward, so treating it as a run that did not
    /// happen would rewind it and re-decide the same window a minute later.
    ///
    /// Recorded rather than logged because a policy nobody can see the effect of
    /// is the invisible-non-execution shape this feature exists to close: before
    /// this variant a declined window left no row anywhere, and the Tasks view's
    /// *last run* went stale for a reason it could not show.
    Declined,
    /// There was no run *yet*: the task's missed-window policy held this window
    /// back, and it is armed for later (Story 58.5, FR-357, AD-140).
    ///
    /// [`Self::Declined`]'s twin and its opposite. Both are written without a
    /// lease, both record a decision rather than a run, and both exist so that
    /// `on_missed`'s two non-default settings are visible rather than silent. The
    /// difference is the one a reader most needs: this window **is** going to be
    /// served, at the instant `detail` names.
    ///
    /// Kept separate from [`Self::Deferred`] for the reason that variant's own
    /// doc gives — `Deferred` is consumed by `Engine::next_task_window` to retry
    /// within `TASK_RETRY_MS`, which would collapse a thirty-minute postponement
    /// into a one-minute one — and separate from [`Self::Declined`] because
    /// *"held back"* and *"dropped"* are different answers to *"will my nightly
    /// sweep happen"*.
    Postponed,
}

impl TaskOutcome {
    /// Stable on-disk spelling, kept separate from any serde representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Busy => "busy",
            Self::Deferred => "deferred",
            Self::Failed => "failed",
            Self::Abandoned => "abandoned",
            Self::Declined => "declined",
            Self::Postponed => "postponed",
        }
    }

    /// Parse the *stored* spelling; `None` for anything a newer keeper wrote,
    /// so history a newer binary recorded reads as unknown rather than fatal.
    pub fn from_stored(value: &str) -> Option<Self> {
        match value {
            "ok" => Some(Self::Ok),
            "busy" => Some(Self::Busy),
            "deferred" => Some(Self::Deferred),
            "failed" => Some(Self::Failed),
            "abandoned" => Some(Self::Abandoned),
            "declined" => Some(Self::Declined),
            "postponed" => Some(Self::Postponed),
            _ => None,
        }
    }
}

/// A parsed 5-field cron expression, as five bitmasks plus the two flags
/// vixie's day rule needs.
///
/// Bitmasks rather than sets because the whole thing is `Copy` and lives in a
/// `TaskSchedule` the engine passes by value on every tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CronSpec {
    /// Bits 0..=59: minutes of the hour this expression matches.
    minutes: u64,
    /// Bits 0..=23: hours of the day.
    hours: u32,
    /// Bits 1..=31: days of the month.
    days_of_month: u32,
    /// Bits 1..=12: months of the year.
    months: u16,
    /// Bits 0..=6, Sunday first: days of the week.
    days_of_week: u8,
    /// The day-of-month field did not begin with `*`.
    day_of_month_restricted: bool,
    /// The day-of-week field did not begin with `*`.
    day_of_week_restricted: bool,
}

/// When a task is due.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskSchedule {
    /// Fires `interval_ms` after the **end of the previous run**, not after a
    /// fixed origin.
    ///
    /// Stated precisely because it is a choice and it drifts: a task whose pass
    /// takes ninety seconds on an `every 1m` schedule fires about every two and
    /// a half minutes, not every minute. The alternative — a fixed origin — makes
    /// a task that overran come due the instant it finished, which for
    /// housekeeping over a git repository is a worse answer than drift.
    Every {
        /// Between [`MIN_SCHEDULE_INTERVAL_MS`] and
        /// [`MAX_SCHEDULE_INTERVAL_MS`]; the parser refuses either side.
        interval_ms: i64,
    },
    /// Fires on a local wall-clock pattern.
    Cron(CronSpec),
}

/// The pure state a due-gate needs, lifted out of the stored row so [`decide`]
/// can be tested without a database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskState {
    /// Whether the row is live at all.
    pub enabled: bool,
    /// Who may trigger it.
    pub mode: TaskMode,
    /// When it next comes due; `None` means keeper has never armed it.
    pub next_due_ms: Option<i64>,
    /// When the current holder's claim expires; `None` means unclaimed.
    pub lease_until_ms: Option<i64>,
    /// What to do about a window that fell due while nobody was home.
    ///
    /// Read here rather than passed alongside because it is a property of the
    /// row, exactly as `mode` is. Acting on it is [`decide`]'s, and for the two
    /// non-default settings the action is a **write** rather than a wait: see
    /// [`Action::Postpone`] and [`Action::Skip`].
    pub on_missed: TaskMissedPolicy,
}

/// What the host should do about one task on this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Nothing, and no state changes.
    None,
    /// Compute and store the next due instant. Nothing runs.
    Arm,
    /// Claim the lease and run the work.
    Run,
    /// Abandon the open window and arm the next one. Nothing runs.
    ///
    /// The variant `Action` could not do without once a policy may decline a
    /// window (Story 58.4). [`Self::None`] cannot express it: the past window
    /// would stay standing, so the next tick would decide about it again,
    /// forever. And the write cannot be `db::arm_task`, which is `WHERE
    /// next_due_ms IS NULL` because first sight can only happen once — a skip
    /// needs its own forward-only write.
    ///
    /// `Engine::run_due_tasks`' match is exhaustive with no `_` arm, so adding
    /// this **forces** every host to decide what it does rather than inherit
    /// silence.
    Skip,
    /// Hold the open window back to a later instant. Nothing runs, yet.
    ///
    /// [`TaskMissedPolicy::Delay`]'s action, and it is an action rather than a
    /// wait for one reason: the delay has to be anchored on the instant a host
    /// **noticed** the missed window, and the only place that instant can be kept
    /// without a second column is `next_due_ms` itself. So the host writes
    /// `now_ms + `[`TASK_MISSED_DELAY_MS`] through the same forward-only
    /// compare-and-set [`Self::Skip`] uses.
    ///
    /// The window it writes is in the **future**, which is what distinguishes
    /// this from [`Self::Skip`]: the run still happens, once, later. And because
    /// the instant is stored, `db::claim_task`'s window condition holds the run
    /// back on every host for the whole delay — including a host that restarts
    /// inside it.
    ///
    /// It cannot loop. A postponed window arrives fresh — nought late — so
    /// [`decide`] answers [`Self::Run`] rather than postponing again; only a host
    /// that went away *again* for longer than the grace can postpone a second
    /// time, which is the same fact about the same absence and still one stored
    /// instant.
    Postpone,
}

impl TaskSchedule {
    /// Parse one written schedule, or refuse it.
    ///
    /// Refusal, never coercion (AD-136 part 2): this runs where the expression
    /// is *written*, so a person is present to be told. The three refusals are
    /// each built by one closure, in the manner of
    /// [`crate::profile`]'s quiet-window validator, so the wording cannot drift
    /// between the branches that reach it.
    ///
    /// Matched against a trimmed, ASCII-lowercased copy so `@Daily` works, but
    /// every refusal quotes the *original* trimmed text — telling somebody
    /// their lowercased input was rejected sends them looking for a typo they
    /// did not make.
    pub fn parse(expression: &str) -> Result<Self> {
        let original = expression.trim();
        let lowered = original.to_ascii_lowercase();
        let malformed = || {
            SyncError::Config(format!(
                "task schedule must be a 5-field cron expression \
                 (minute hour day-of-month month day-of-week), one of @hourly, \
                 @daily or @weekly, or every <n><unit> with unit \
                 s/m/h/d, got {original:?}"
            ))
        };
        let below_floor = || {
            SyncError::Config(format!(
                "task schedule must not fire more often than once a minute \
                 ({MIN_SCHEDULE_INTERVAL_MS} ms), got {original:?}"
            ))
        };
        let above_ceiling = || {
            SyncError::Config(format!(
                "task schedule must not fire less often than once a year \
                 ({MAX_SCHEDULE_INTERVAL_MS} ms) — write a calendar pattern \
                 instead, got {original:?}"
            ))
        };
        let never = || {
            SyncError::Config(format!(
                "task schedule matches no instant, got {original:?}"
            ))
        };

        let cron_text: &str = if let Some(word) = lowered.strip_prefix('@') {
            // Aliases desugar to cron rather than to intervals. `@daily` as
            // "86 400 000 ms after whenever it was armed" would drift a nightly
            // sweep to whatever time the host last restarted, and nightly would
            // stop meaning night.
            match word {
                "hourly" => "0 * * * *",
                "daily" => "0 0 * * *",
                "weekly" => "0 0 * * 0",
                _ => return Err(malformed()),
            }
        } else if let Some(rest) = lowered.strip_prefix("every") {
            // The keyword has to be a word: `everything` is not an interval,
            // and a bare `every` has no number to read.
            let rest = rest
                .strip_prefix(|c: char| c.is_whitespace())
                .ok_or_else(malformed)?
                .trim();
            let split = rest
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(rest.len());
            let (count, unit) = rest.split_at(split);
            // No emptiness guard: an absent unit falls through the unit match
            // and an absent count fails to parse, so both already reach
            // `malformed` by the shortest route there is.
            let unit = unit.trim_start();
            // Seconds are in the grammar and then refused by the floor, so
            // `every 30s` is told about the one-minute floor rather than about
            // a unit keeper supposedly does not understand.
            let unit_ms: i64 = match unit {
                "s" | "sec" | "secs" | "second" | "seconds" => 1_000,
                "m" | "min" | "mins" | "minute" | "minutes" => MS_PER_MINUTE,
                "h" | "hr" | "hrs" | "hour" | "hours" => 60 * MS_PER_MINUTE,
                "d" | "day" | "days" => MS_PER_DAY,
                _ => return Err(malformed()),
            };
            // A count keeper cannot represent, or a product that overflows, is
            // malformed rather than below the floor: the floor's message would
            // describe the opposite of what happened.
            let count: i64 = count.parse().map_err(|_| malformed())?;
            let interval_ms = count.checked_mul(unit_ms).ok_or_else(malformed)?;
            if interval_ms < MIN_SCHEDULE_INTERVAL_MS {
                return Err(below_floor());
            }
            if interval_ms > MAX_SCHEDULE_INTERVAL_MS {
                return Err(above_ceiling());
            }
            return Ok(Self::Every { interval_ms });
        } else {
            &lowered
        };

        let spec = CronSpec::parse(cron_text).ok_or_else(malformed)?;
        // A cron expression can parse cleanly and still name a date that does
        // not exist (`0 0 30 2 *`). AD-136 calls that out by hand: a schedule
        // that parses to "never" while reporting itself enabled is the
        // invisible-failure shape, so it is refused where it is written.
        //
        // Answered in constant time rather than by walking a calendar. This
        // runs on every read of every task row — `decode_task` parses each
        // stored schedule on every tick — so a day-walk here would have made a
        // Feb-29 pattern re-derive a save-time fact eight hundred iterations at
        // a time, once a second, forever.
        if !spec.matches_any_date() {
            return Err(never());
        }
        Ok(Self::Cron(spec))
    }

    /// The next instant strictly after `now_ms` at which this schedule fires,
    /// in epoch milliseconds, or `None` for a pattern that has no such instant
    /// inside the search window.
    ///
    /// Strictly after, because a window that resolved to itself would re-run on
    /// every tick forever.
    ///
    /// `utc_offset_minutes` comes from
    /// [`crate::platform::SyncPlatform::utc_offset_minutes`], the crate's only
    /// zone authority, and it is read at *evaluation* time. That means there is
    /// no DST arithmetic in here to get wrong; the accepted cost is that a
    /// schedule crossing an offset change fires at the new offset's wall-clock
    /// time, which is the only thing a fixed offset can honestly promise.
    pub fn next_due_after(&self, now_ms: i64, utc_offset_minutes: i32) -> Option<i64> {
        match self {
            Self::Every { interval_ms } => Some(now_ms.saturating_add(*interval_ms)),
            Self::Cron(spec) => spec.next_due_after(now_ms, utc_offset_minutes),
        }
    }
}

impl CronSpec {
    /// Parse the five whitespace-separated fields, or `None`.
    ///
    /// Returns an `Option` rather than a `Result` so the one refusal message
    /// lives with the rest of the dialect in [`TaskSchedule::parse`] instead of
    /// being reworded here.
    fn parse(expression: &str) -> Option<Self> {
        let mut fields = expression.split_whitespace();
        let minute = fields.next()?;
        let hour = fields.next()?;
        let day_of_month = fields.next()?;
        let month = fields.next()?;
        let day_of_week = fields.next()?;
        if fields.next().is_some() {
            return None;
        }

        let (minutes, _) = parse_field(minute, 0, 59, false)?;
        let (hours, _) = parse_field(hour, 0, 23, false)?;
        let (days_of_month, dom_star) = parse_field(day_of_month, 1, 31, false)?;
        let (months, _) = parse_field(month, 1, 12, false)?;
        let (days_of_week, dow_star) = parse_field(day_of_week, 0, 7, true)?;

        Some(Self {
            minutes,
            hours: hours as u32,
            days_of_month: days_of_month as u32,
            months: months as u16,
            days_of_week: days_of_week as u8,
            day_of_month_restricted: !dom_star,
            day_of_week_restricted: !dow_star,
        })
    }

    /// The next matching instant strictly after `now_ms`.
    ///
    /// Searches by **day**, not by minute: a minute-by-minute walk of a
    /// four-year window is 2.1 million iterations, and the day is where all the
    /// interesting arithmetic lives anyway.
    fn next_due_after(&self, now_ms: i64, utc_offset_minutes: i32) -> Option<i64> {
        let offset_ms = i64::from(utc_offset_minutes) * MS_PER_MINUTE;
        let local_ms = now_ms.checked_add(offset_ms)?;
        // Euclidean division so a negative local instant still floors to the
        // day that contains it rather than truncating toward the epoch.
        let mut day = local_ms.div_euclid(MS_PER_DAY);
        let mut from_minute = local_ms.rem_euclid(MS_PER_DAY) / MS_PER_MINUTE + 1;

        for _ in 0..SEARCH_DAYS {
            if self.day_matches(day) {
                for minute in from_minute..MINUTES_PER_DAY {
                    if self.minute_matches(minute) {
                        let local = day
                            .checked_mul(MS_PER_DAY)?
                            .checked_add(minute * MS_PER_MINUTE)?;
                        return local.checked_sub(offset_ms);
                    }
                }
            }
            day = day.checked_add(1)?;
            from_minute = 0;
        }
        None
    }

    /// Whether this expression fires at `minute` of the local day.
    fn minute_matches(&self, minute: i64) -> bool {
        let hour = minute / 60;
        let within_hour = minute % 60;
        self.hours & (1u32 << hour) != 0 && self.minutes & (1u64 << within_hour) != 0
    }

    /// Whether any real calendar date can satisfy the month and day fields.
    ///
    /// Constant time, and exact. A pattern is unreachable only when the
    /// day-of-month field is the sole day constraint and no month it names has a
    /// day it names — `0 0 30 2 *`, `0 0 31 4 *`. If the day-of-month field
    /// began with `*` every day is a candidate; and if the day-of-week field is
    /// restricted too then vixie's rule ORs them, so a weekday alone already
    /// makes the pattern reachable. February counts 29 days here because leap
    /// years exist and `0 0 29 2 *` is a schedule a person may legitimately mean.
    fn matches_any_date(&self) -> bool {
        if !self.day_of_month_restricted || self.day_of_week_restricted {
            return true;
        }
        (1u32..=12).any(|month| {
            self.months & (1u16 << month) != 0
                && (1u32..=MAX_DAYS_IN_MONTH[month as usize])
                    .any(|day| self.days_of_month & (1u32 << day) != 0)
        })
    }

    /// Whether this expression fires on the local day `day` days after
    /// 1970-01-01.
    ///
    /// Implements **vixie-cron's day rule**, which is surprising and therefore
    /// worth stating: when *neither* day field begins with `*`, the day matches
    /// if either one does; when at least one begins with `*`, both must match.
    /// So `0 0 13 * 5` fires on every 13th *and* every Friday, while
    /// `0 0 13 * *` fires only on the 13th — and, the wart faithfully
    /// reproduced, `0 0 */2 * 5` fires on the Fridays that fall on an odd day of
    /// the month, because `*/2` counts as a star and so the fields are ANDed.
    fn day_matches(&self, day: i64) -> bool {
        let (_, month, day_of_month) = civil_from_days(day);
        if self.months & (1u16 << month) == 0 {
            return false;
        }
        let by_month_day = self.days_of_month & (1u32 << day_of_month) != 0;
        let by_weekday = self.days_of_week & (1u8 << weekday_from_days(day)) != 0;
        // Both masks are always consulted; only the connective changes. That is
        // vixie's shape verbatim, and it is why a bare `*` needs no special case:
        // its mask holds every value, so the `&&` reduces to the other field.
        if self.day_of_month_restricted && self.day_of_week_restricted {
            by_month_day || by_weekday
        } else {
            by_month_day && by_weekday
        }
    }
}

/// Parse one comma-separated cron field into a bitmask, and report whether it
/// began with `*`.
///
/// The star flag is what vixie's day rule reads, and it is what vixie itself
/// reads: the flag is set from the field's **first character**, so `*/2` is a
/// star and `1-31` is not — even though the range selects every day. Both halves
/// of that are surprising, and both are reproduced deliberately: this dialect
/// claims to be a 5-field cron expression, and a dialect that agrees with cron
/// on the easy fields and diverges on the day rule would be worse than one that
/// does not claim the name at all.
///
/// `fold_seven` accepts 7 as a second spelling of Sunday and folds it onto 0,
/// which is the one place the grammar has two names for one thing.
///
/// Month and weekday **names** (`JAN`, `MON`) are deliberately absent: the
/// grammar is small on purpose, and a name nobody parses is worse than a
/// refusal that says so.
fn parse_field(field: &str, min: u32, max: u32, fold_seven: bool) -> Option<(u64, bool)> {
    let mut mask = 0u64;
    let star = field.starts_with('*');
    for term in field.split(',') {
        // `A/S` is refused: a step over a single value is a contradiction, and
        // accepting it would mean guessing which of the two the writer meant.
        let (head, step) = match term.split_once('/') {
            Some((head, step)) => {
                let step: u32 = parse_number(step)?;
                // `*/0` is refused rather than read as `*/1`: a zero step
                // advances nothing, so the walk below would spin forever on the
                // first value in the range.
                if step == 0 {
                    return None;
                }
                (head, step)
            }
            None => (term, 1),
        };
        let (from, to) = if head == "*" {
            (min, max)
        } else if let Some((low, high)) = head.split_once('-') {
            let low = parse_number(low)?;
            let high = parse_number(high)?;
            // Never wrapped. `5-1` is a mistake, not "17:00 through 01:00", and
            // silently reinterpreting it would schedule twenty hours nobody
            // asked for.
            if low > high {
                return None;
            }
            (low, high)
        } else {
            let single = parse_number(head)?;
            if step != 1 {
                return None;
            }
            (single, single)
        };
        if from < min || to > max {
            return None;
        }
        let mut value = from;
        while value <= to {
            let bit = if fold_seven && value == 7 { 0 } else { value };
            mask |= 1u64 << bit;
            // Saturating, because a step is only bounded by `u32` and
            // `59 + u32::MAX` would panic in a debug build rather than end the
            // walk it obviously ends.
            value = value.saturating_add(step);
        }
    }
    Some((mask, star))
}

/// Parse one bare decimal number, refusing anything with a sign, a space or a
/// name in it.
fn parse_number(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// Day of the week for a count of days since 1970-01-01, Sunday first.
///
/// The `+ 4` is not arbitrary: 1970-01-01 was a **Thursday**, which is index 4
/// when Sunday is 0. `rem_euclid` rather than `%` so a pre-epoch day still
/// lands in `0..=6`.
fn weekday_from_days(day: i64) -> u32 {
    (day + 4).rem_euclid(7) as u32
}

/// Civil date `(year, month, day)` from a count of days since 1970-01-01.
///
/// Howard Hinnant's `civil_from_days` (`chrono`-compatible calendar
/// algorithms), named so the algorithm is attributable rather than looking like
/// a pile of magic constants. It shifts the era to start on 1 March so the leap
/// day falls at the end of a year and needs no special case.
///
/// The intermediate ranges, since the divisors otherwise read as noise:
/// `day_of_era` is `[0, 146096]`, `year_of_era` is `[0, 399]`, `day_of_year` is
/// `[0, 365]`, `month_prime` is `[0, 11]` counted from March, and
/// `day_of_month` is `[1, 31]`.
fn civil_from_days(day: i64) -> (i64, u32, u32) {
    let shifted = day + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day_of_month = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    let year = if month <= 2 { year + 1 } else { year };
    (year, month as u32, day_of_month as u32)
}

/// Whether `id` is a spelling a task could ever have been stored under
/// (Story 57.3, FR-347).
///
/// **The single implementation of that rule**, and public because two callers
/// on opposite sides of the database need the same answer from it:
/// [`crate::db::upsert_task`], which refuses at the write door, and 57.3's
/// `keeper-syncd tasks` selector, which has to tell **"a spelling this keeper
/// could never have stored"** — malformed, refuse and quote it — apart from
/// **"well formed, but no such task"** — unknown, refuse and list what is
/// known. Those are two different sentences and two different pieces of advice
/// to the person at the prompt: a selector that could not distinguish them
/// would answer "no such task" to `tasks run "nightly "` and send somebody
/// looking for a row that was never the problem.
///
/// Pure — no `self`, no clock, no `Connection` — so the selector may ask it
/// before it opens a database, and so the rule is asserted against literal
/// strings. A second copy of it inside the CLI is the one duplication this
/// story cannot afford: two copies drift, and the drift would be silent in the
/// direction of accepting an id [`crate::db::upsert_task`] will not store.
///
/// The two rules and both messages are the ones the write door has carried
/// since Story 57.1, moved here verbatim rather than reworded — a stored
/// refusal's text is what a person greps for.
pub fn validate_id(id: &str) -> Result<()> {
    if id.trim().is_empty() {
        return Err(SyncError::Config("task id must not be empty".into()));
    }
    // Refused rather than trimmed: the id is a primary key, it is what 57.3's
    // CLI reads from argv and what `task_runs.task_id` joins on, and silently
    // accepting three spellings of one intended task is worse than saying so.
    if id.trim() != id {
        return Err(SyncError::Config(format!(
            "task id must not begin or end with whitespace, got {id:?}"
        )));
    }
    Ok(())
}

/// Whether `delay_ms` is a per-task missed-window delay a task could coherently
/// carry (Story 59.6, FR-366).
///
/// **The single implementation of that rule**, for [`validate_id`]'s reason and
/// with the same two callers on opposite sides of the database:
/// [`crate::db::upsert_task`] refuses at the write door, and `keeper-syncd tasks
/// set` converts a person's minutes before it ever opens a store. Pure, so both
/// may ask it, and so the two boundaries are asserted against literal integers.
///
/// `None` is always fine and always means [`TASK_MISSED_DELAY_MS`] — that is
/// what keeps a row written before this column existed meaning what it meant.
///
/// # The floor is the grace period, and it is not a taste
///
/// [`TASK_MISSED_GRACE_MS`] is *the interval that concludes nobody was home*:
/// nothing is a missed window until it has been open that long. A delay shorter
/// than it is therefore not a delay at all — it would be over before the window
/// it holds back was recognised as missed, so the very next tick would serve the
/// window and `delay` would be spelling `run_now` at a cost of one extra write
/// and one `postponed` run row per absence. Two settings, one behaviour, is
/// exactly the collapse 58.4's review moved the anchor to avoid.
///
/// # The ceiling is the schedule's ceiling, for the schedule's reason
///
/// A delay is stored by writing `next_due_ms` forward, so an enormous one is
/// indistinguishable from [`MAX_SCHEDULE_INTERVAL_MS`]'s own failure: a row that
/// reports itself enabled and scheduled while the instant it is waiting for is
/// past anybody's patience. `every 100000000d` is refused for that; `--missed-delay
/// 100000000` would arrive at the same place through the other door, so it meets
/// the same bound. Nothing is lost above it — a task that wants its housekeeping
/// a year later wants a different schedule, not a delay.
pub fn validate_missed_delay_ms(delay_ms: Option<i64>) -> Result<()> {
    let Some(delay_ms) = delay_ms else {
        return Ok(());
    };
    if delay_ms < TASK_MISSED_GRACE_MS {
        return Err(SyncError::Config(format!(
            "task missed-window delay must be at least the grace period \
             ({TASK_MISSED_GRACE_MS} ms), because the grace period is the interval \
             that concludes nobody was home — a shorter delay would elapse before \
             the window it holds back counted as missed, which is run_now wearing \
             delay's name, got {delay_ms} ms"
        )));
    }
    if delay_ms > MAX_SCHEDULE_INTERVAL_MS {
        return Err(SyncError::Config(format!(
            "task missed-window delay must not exceed a year \
             ({MAX_SCHEDULE_INTERVAL_MS} ms), the ceiling a schedule has and for the \
             same reason: the delay is stored as the instant the window is held \
             back to, so one that far ahead is a task that reports itself enabled \
             and scheduled while nothing ever runs, got {delay_ms} ms"
        )));
    }
    Ok(())
}

/// How long this task holds a missed window back: its own value, or the
/// constant (Story 59.6, FR-366).
///
/// **The one place the override is resolved**, and it is a named function rather
/// than an `unwrap_or` at the call site so that *"absent means the constant"* is
/// a rule with a home instead of a habit. There is exactly one production reader
/// of the answer — `Engine::move_task_window`, which turns it into the stored
/// instant — and a second one appearing anywhere is a second chance to read a
/// `None` as a zero.
///
/// It does not re-check the bounds, exactly as `db::get_task` does not re-check
/// [`validate_id`]: the bounds are a rule about what may be *written*, and this
/// is a read on a 1 Hz tick that has to answer. A value from a newer keeper is
/// therefore honoured as stored rather than clamped — clamping would hold a
/// window back to an instant nobody chose and leave no trace of having done it,
/// and `move_task_window` writes the instant it computes into a `detail` line a
/// person reads, so an unusual delay explains itself there.
pub fn effective_missed_delay_ms(delay_ms: Option<i64>) -> i64 {
    delay_ms.unwrap_or(TASK_MISSED_DELAY_MS)
}

/// The pure gate: what this host should do about one task, right now.
///
/// No `self`, no clock, no database and no allocation, so the state machine is
/// asserted against literal integers — the split `notes_vault::decide` already
/// makes, and the reason AD-136 can be tested without a timer.
///
/// [`Action::Arm`] on first sight is load-bearing: a nightly task created at
/// noon must run tonight, not at noon. A live lease yields [`Action::None`] on
/// every other host, and an *expired* one falls through to the due check, which
/// is what makes a dead holder's task reclaimable rather than wedged forever.
///
/// # The MISSED window is where the policy lives
///
/// The due test is still a scalar compare on one stored instant, so nothing here
/// counts elapsed windows and nothing here could produce N runs (AD-138).
/// [`TaskState::on_missed`] chooses only what happens to a window that is
/// **already open** *and* has been open for [`TASK_MISSED_GRACE_MS`] — long
/// enough that no host was here to serve it. Inside the grace all three settings
/// answer [`Action::Run`], which is what makes this a policy about *missed*
/// windows rather than a re-timing of the schedule.
///
/// [`TaskMissedPolicy::RunNow`]'s answer is textually the one this function has
/// always had, unconditionally, which is why it is the default and why an upgrade
/// changes no install's meaning. The other two answer with a **write** rather
/// than a wait — [`Action::Postpone`] and [`Action::Skip`] — because both need to
/// anchor on the instant a host *noticed*, and `next_due_ms` is the only place
/// that instant can live without a second column.
pub fn decide(state: &TaskState, schedule: Option<&TaskSchedule>, now_ms: i64) -> Action {
    if !state.enabled || state.mode != TaskMode::Scheduled {
        return Action::None;
    }
    if schedule.is_none() {
        return Action::None;
    }
    if state.lease_until_ms.is_some_and(|until| now_ms < until) {
        return Action::None;
    }
    match state.next_due_ms {
        None => Action::Arm,
        Some(at) if now_ms >= at => {
            // The instant at which a window nobody served stops counting as one
            // this host merely has not reached yet. Saturating because a stored
            // window from a newer keeper can be any `i64`.
            let missed = now_ms >= at.saturating_add(TASK_MISSED_GRACE_MS);
            if !missed {
                // A host is here and this is its own window. Every policy serves
                // it, including the two that would otherwise re-time a schedule
                // nobody asked them to re-time.
                return Action::Run;
            }
            match state.on_missed {
                // Today's behaviour, unchanged.
                TaskMissedPolicy::RunNow => Action::Run,
                // Held back to `now + TASK_MISSED_DELAY_MS`, which the host
                // writes: the anchor is this noticing, not the window, because a
                // window two hours old is already past any instant derived from
                // itself.
                TaskMissedPolicy::Delay => Action::Postpone,
                // Dropped, and the next natural window armed in its place.
                TaskMissedPolicy::Skip => Action::Skip,
            }
        }
        Some(_) => Action::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2024-01-01T00:00:00Z. 19 723 days since the epoch (a well-known value),
    /// and 19 723 × 86 400 000 = 1 704 067 200 000. A Monday: days-since-epoch
    /// 19 723, so `(19 723 + 4) % 7 == 1`, and 0 is Sunday.
    const JAN_1_2024_UTC: i64 = 1_704_067_200_000;
    const HOUR_MS: i64 = 3_600_000;
    const DAY_MS: i64 = 86_400_000;

    fn state(mode: TaskMode, next_due_ms: Option<i64>) -> TaskState {
        TaskState {
            enabled: true,
            mode,
            next_due_ms,
            lease_until_ms: None,
            on_missed: TaskMissedPolicy::RunNow,
        }
    }

    /// [`state`] under one of the two non-default policies, spelled as an
    /// override so the four cases below cannot drift in anything but the field
    /// they are about.
    fn state_with(
        mode: TaskMode,
        next_due_ms: Option<i64>,
        on_missed: TaskMissedPolicy,
    ) -> TaskState {
        TaskState {
            on_missed,
            ..state(mode, next_due_ms)
        }
    }

    fn cron(expression: &str) -> TaskSchedule {
        TaskSchedule::parse(expression).expect("expression is in the accepted dialect")
    }

    /// A schedule nobody can parse is a schedule that silently never runs, so
    /// the near misses matter as much as the accepted forms — and the accepted
    /// forms are asserted in the same test so a tightening of the grammar
    /// cannot quietly start refusing them.
    #[test]
    fn the_dialect_refuses_every_near_miss_and_accepts_every_boundary() {
        for expression in [
            "",
            "0 3 * *",
            "0 3 * * * *",
            "60 * * * *",
            "* 24 * * *",
            "* * 0 * *",
            "* * * 13 *",
            "* * * * 8",
            "5-1 * * * *",
            "*/0 * * * *",
            "0 0 * jan *",
            "0 0 * * mon",
            "@yearly",
            "@",
            "every",
            "every 5",
            "every 5x",
            "every -5m",
            "every 99999999999999999999d",
            // Multiplies without overflowing and then arms a window in the year
            // 275 000: a schedule nobody would ever see fire.
            "every 1000000000000d",
            "every 367d",
        ] {
            assert!(
                matches!(TaskSchedule::parse(expression), Err(SyncError::Config(_))),
                "{expression:?} must be refused"
            );
        }

        for expression in [
            "0 3 * * *",
            "*/15 * * * *",
            "0,30 1-5 * * 1-5",
            // 7 is Sunday too, folded onto 0.
            "* * * * 7",
            "@hourly",
            // Aliases are matched case-insensitively.
            "@Daily",
            "every 1m",
            "every 60s",
            "every 2 hours",
            // The ceiling's own boundary: a year is accepted, a day more is not.
            "every 366d",
        ] {
            assert!(
                TaskSchedule::parse(expression).is_ok(),
                "{expression:?} must be accepted"
            );
        }

        // A backwards range and an out-of-range value are *malformed*, and
        // specifically not "matches no instant". Both refusals reject `5-1` and
        // `60 * * * *`, and telling somebody their expression matches nothing
        // sends them to look at the calendar instead of at their typo.
        for expression in ["5-1 * * * *", "60 * * * *", "* 24 * * *", "* * * * 8"] {
            let error = TaskSchedule::parse(expression).expect_err("outside the field's range");
            assert!(
                error.to_string().contains("5-field cron expression"),
                "{expression:?} must be refused as malformed, got {error}"
            );
        }
    }

    /// FR-347 asks for the expression quoted, and a schedule contains spaces,
    /// so the quotation marks are what make its boundaries visible.
    #[test]
    fn a_refusal_quotes_the_expression_it_could_not_parse() {
        let error = TaskSchedule::parse("0 3 * *").expect_err("four fields is not the dialect");
        let rendered = error.to_string();
        assert!(
            rendered.contains("\"0 3 * *\""),
            "the refusal must quote the expression, got {rendered}"
        );
    }

    /// Below a minute the scheduler spends more time waking than working, and
    /// the message has to say so — seconds parse, then the floor refuses them,
    /// so the reader is told about the floor and not about their unit.
    #[test]
    fn a_sub_minute_schedule_is_refused_and_the_message_names_the_floor() {
        for expression in ["every 30s", "every 0m", "every 59s"] {
            let error = TaskSchedule::parse(expression).expect_err("below the floor");
            let rendered = error.to_string();
            assert!(
                matches!(error, SyncError::Config(_)),
                "{expression:?} must be a config refusal"
            );
            assert!(
                rendered.contains(&MIN_SCHEDULE_INTERVAL_MS.to_string()),
                "{expression:?} must be refused by a message naming the floor, got {rendered}"
            );
        }
    }

    /// The ceiling's mirror: an interval nobody would see fire is refused for
    /// the same reason `0 0 30 2 *` is, and the message points at the cron half
    /// of the dialect rather than leaving the reader with no way to say "yearly".
    #[test]
    fn a_schedule_slower_than_a_year_is_refused_and_the_message_offers_cron() {
        let error = TaskSchedule::parse("every 400d").expect_err("above the ceiling");
        let rendered = error.to_string();
        assert!(matches!(error, SyncError::Config(_)));
        assert!(
            rendered.contains("calendar pattern") && rendered.contains("\"every 400d\""),
            "the refusal must name the alternative and quote the input, got {rendered}"
        );
        assert_eq!(
            TaskSchedule::parse("0 0 1 1 *").map(|_| ()).ok(),
            Some(()),
            "and the alternative it offers really is in the dialect"
        );
    }

    /// The invisible-failure shape AD-136 names by hand: a schedule that parses
    /// to "never" while reporting itself enabled. Feb 29 is the boundary that
    /// proves the search window is wide enough to tell the two apart.
    #[test]
    fn a_cron_that_matches_no_instant_is_refused_and_the_leap_day_is_not() {
        for expression in ["0 0 30 2 *", "0 0 31 4 *"] {
            assert!(
                matches!(TaskSchedule::parse(expression), Err(SyncError::Config(_))),
                "{expression:?} matches no instant and must be refused"
            );
        }
        assert!(
            TaskSchedule::parse("0 0 29 2 *").is_ok(),
            "leap years exist, so Feb 29 is a real instant"
        );

        // The widest gap the grammar can actually ask for, and the one that
        // sizes SEARCH_DAYS: 1972-02-29 is day 789 since the epoch and
        // 1976-02-29 is day 2250, so the search has to reach 1 461 days
        // forward. 789 × 86 400 000 = 68 169 600 000 and
        // 2250 × 86 400 000 = 194 400 000 000.
        assert_eq!(
            cron("0 0 29 2 *").next_due_after(68_169_600_000, 0),
            Some(194_400_000_000),
            "four years of walking is what the window is sized for"
        );
    }

    /// Aliases desugar to cron, not to intervals: `@daily` as "86 400 000 ms
    /// after whenever it was armed" would drift a nightly sweep to whatever
    /// time the host last restarted, and nightly would stop meaning night.
    #[test]
    fn the_aliases_desugar_to_the_cron_they_claim() {
        assert_eq!(cron("@hourly"), cron("0 * * * *"));
        assert_eq!(cron("@daily"), cron("0 0 * * *"));
        assert_eq!(cron("@weekly"), cron("0 0 * * 0"));
    }

    #[test]
    fn next_due_after_lands_on_the_exact_minute() {
        // 2024-01-01T00:00:00Z + 3 h = 2024-01-01T03:00:00Z.
        assert_eq!(
            cron("0 3 * * *").next_due_after(JAN_1_2024_UTC, 0),
            Some(JAN_1_2024_UTC + 3 * HOUR_MS)
        );
        // The first quarter-hour after midnight is 00:15, not 00:00.
        assert_eq!(
            cron("*/15 * * * *").next_due_after(JAN_1_2024_UTC, 0),
            Some(JAN_1_2024_UTC + 900_000)
        );
        // One millisecond before 00:15 still resolves to 00:15.
        assert_eq!(
            cron("*/15 * * * *").next_due_after(JAN_1_2024_UTC + 899_999, 0),
            Some(JAN_1_2024_UTC + 900_000)
        );
    }

    /// A nightly task on a host two hours ahead of UTC fires at that host's
    /// midnight, not Greenwich's.
    ///
    /// The instants are chosen so the host is on a *different date* from UTC,
    /// which is the only shape that discriminates. At 00:00Z with a +2 h offset
    /// the two readings coincide by arithmetic accident — 24 h − 2 h is the same
    /// answer as 2 h → next-midnight → −2 h — so a test anchored there would
    /// pass over an implementation that never applied the offset at all.
    #[test]
    fn a_daily_schedule_fires_at_local_midnight_not_at_utc_midnight() {
        // 23:00Z is 01:00 on 2 January at UTC+2, so this host's midnight is
        // 23 hours away while UTC's is one: 3 January 00:00 local = 2 January
        // 22:00Z.
        assert_eq!(
            cron("@daily").next_due_after(JAN_1_2024_UTC + 23 * HOUR_MS, 120),
            Some(JAN_1_2024_UTC + 46 * HOUR_MS),
            "east of UTC the host is already on the next date"
        );
        // The same asymmetry the other way: 03:00Z is 22:00 on 31 December at
        // UTC-5, so the next local midnight is 1 January 00:00 local, which is
        // 1 January 05:00Z — not 2 January.
        assert_eq!(
            cron("@daily").next_due_after(JAN_1_2024_UTC + 3 * HOUR_MS, -300),
            Some(JAN_1_2024_UTC + 5 * HOUR_MS),
            "west of UTC the host is still on the previous date"
        );
    }

    /// Strictly after, never the same instant: an armed window that resolved to
    /// itself would re-run on every tick forever.
    #[test]
    fn next_due_after_is_strictly_after_the_instant_it_is_given() {
        let at_the_match = JAN_1_2024_UTC + 3 * HOUR_MS;
        assert_eq!(
            cron("0 3 * * *").next_due_after(at_the_match, 0),
            Some(at_the_match + DAY_MS)
        );
    }

    /// An interval counts from the arming instant, which is what makes it
    /// independent of any wall clock or zone.
    #[test]
    fn an_interval_schedule_fires_one_interval_after_it_was_armed() {
        assert_eq!(cron("every 5m").next_due_after(1_000, 0), Some(301_000));
        assert_eq!(
            cron("every 2 hours").next_due_after(1_000, 780),
            Some(1_000 + 2 * HOUR_MS),
            "an interval ignores the zone: there is no wall clock in it"
        );
    }

    /// vixie-cron's day rule, which is surprising enough to be worth pinning:
    /// with both day fields restricted the day matches if *either* does.
    #[test]
    fn when_both_day_fields_are_restricted_either_one_matches() {
        // 2024-01-01 is a Monday. Fridays: the 5th, 12th, 19th. The 13th is a
        // Saturday, and it matches anyway because day-of-month says so.
        let schedule = cron("0 0 13 * 5");
        let fifth = JAN_1_2024_UTC + 4 * DAY_MS;
        let twelfth = JAN_1_2024_UTC + 11 * DAY_MS;
        let thirteenth = JAN_1_2024_UTC + 12 * DAY_MS;
        assert_eq!(schedule.next_due_after(JAN_1_2024_UTC, 0), Some(fifth));
        assert_eq!(schedule.next_due_after(fifth, 0), Some(twelfth));
        assert_eq!(schedule.next_due_after(twelfth, 0), Some(thirteenth));
    }

    #[test]
    fn when_only_one_day_field_is_restricted_only_that_one_applies() {
        let thirteenth = JAN_1_2024_UTC + 12 * DAY_MS;
        assert_eq!(
            cron("0 0 13 * *").next_due_after(JAN_1_2024_UTC, 0),
            Some(thirteenth),
            "an unrestricted weekday must not add Fridays"
        );

        let fifth = JAN_1_2024_UTC + 4 * DAY_MS;
        let twelfth = JAN_1_2024_UTC + 11 * DAY_MS;
        let fridays = cron("0 0 * * 5");
        assert_eq!(fridays.next_due_after(JAN_1_2024_UTC, 0), Some(fifth));
        assert_eq!(
            fridays.next_due_after(fifth, 0),
            Some(twelfth),
            "an unrestricted day-of-month must not add the 13th"
        );
    }

    /// vixie's own wart, reproduced deliberately: the star flag comes from the
    /// field's FIRST CHARACTER, so `*/2` counts as a star and the two day fields
    /// are ANDed — while `1-31`, which selects every day, does not and they are
    /// ORed. This is the half of the day rule a from-scratch implementation gets
    /// wrong, and getting it wrong makes `0 0 */2 * 5` fire on about fifteen
    /// extra days a month.
    #[test]
    fn a_stepped_day_field_counts_as_a_star_the_way_vixie_counts_it() {
        // 2024-01-01 is a Monday, so January's Fridays are the 5th, 12th, 19th
        // and 26th. `*/2` selects the odd days, so only the 5th and 19th match
        // both — the 12th and 26th are even.
        let stepped = cron("0 0 */2 * 5");
        let fifth = JAN_1_2024_UTC + 4 * DAY_MS;
        let nineteenth = JAN_1_2024_UTC + 18 * DAY_MS;
        assert_eq!(stepped.next_due_after(JAN_1_2024_UTC, 0), Some(fifth));
        assert_eq!(
            stepped.next_due_after(fifth, 0),
            Some(nineteenth),
            "a stepped day-of-month is a star, so the fields AND and the 12th is skipped"
        );

        // The same shape written as a range is NOT a star, so the fields OR and
        // every listed day matches even when it is not a Friday.
        let ranged = cron("0 0 1-31 * 5");
        assert_eq!(
            ranged.next_due_after(JAN_1_2024_UTC, 0),
            Some(JAN_1_2024_UTC + DAY_MS),
            "a range selecting every day still ORs, so 2 January matches"
        );
    }

    /// The search window has to clear the non-leap century. 2100 is not a leap
    /// year, so between 2096 and 2104 consecutive 29 Februaries are 2 922 days
    /// apart: a four-year window resolved `0 0 29 2 *` to nothing there, and a
    /// schedule that resolves to nothing while reporting itself enabled is the
    /// failure this module exists to refuse.
    #[test]
    fn a_leap_day_schedule_still_resolves_across_the_non_leap_century() {
        // 1 March 2096, the day after that year's 29 February: days since the
        // epoch are 126 × 365 + 31 leap days + 31 + 29 = 46 081.
        let after_2096s_leap_day = 46_081 * DAY_MS;
        let next = cron("0 0 29 2 *")
            .next_due_after(after_2096s_leap_day, 0)
            .expect("29 February 2104 is a real instant");
        assert!(
            next - after_2096s_leap_day > 4 * 366 * DAY_MS,
            "the next leap day is more than four years away, which is exactly \
             why the window is eight"
        );
    }

    #[test]
    fn a_task_that_is_disabled_off_manual_or_unscheduled_is_never_due() {
        let schedule = cron("@daily");
        let due = Some(JAN_1_2024_UTC);

        let disabled = TaskState {
            enabled: false,
            ..state(TaskMode::Scheduled, due)
        };
        assert_eq!(
            decide(&disabled, Some(&schedule), JAN_1_2024_UTC),
            Action::None
        );

        for mode in [TaskMode::Off, TaskMode::Manual] {
            assert_eq!(
                decide(&state(mode, due), Some(&schedule), JAN_1_2024_UTC),
                Action::None,
                "{mode:?} is never due; only Scheduled is"
            );
        }

        assert_eq!(
            decide(&state(TaskMode::Scheduled, due), None, JAN_1_2024_UTC),
            Action::None,
            "a scheduled task with no parsable schedule has no window to open"
        );
    }

    /// A nightly task created at noon must run tonight, not at noon: first
    /// sight computes the window and runs nothing.
    #[test]
    fn a_task_seen_for_the_first_time_is_armed_and_never_run() {
        let schedule = cron("@daily");
        assert_eq!(
            decide(
                &state(TaskMode::Scheduled, None),
                Some(&schedule),
                JAN_1_2024_UTC
            ),
            Action::Arm
        );
    }

    #[test]
    fn the_window_opens_at_the_instant_and_not_one_millisecond_before() {
        let schedule = cron("@daily");
        let due = JAN_1_2024_UTC + 3 * HOUR_MS;
        let armed = state(TaskMode::Scheduled, Some(due));
        assert_eq!(decide(&armed, Some(&schedule), due - 1), Action::None);
        assert_eq!(decide(&armed, Some(&schedule), due), Action::Run);
    }

    /// The lease is what keeps a daemon and the app, both writing one
    /// `sync.db`, from running one task twice over one git index — and an
    /// expired lease is what keeps a killed host from wedging it forever.
    #[test]
    fn a_live_lease_holds_every_other_host_off_and_an_expired_one_does_not() {
        let schedule = cron("@daily");
        let due = JAN_1_2024_UTC;
        let leased = TaskState {
            lease_until_ms: Some(due + 30_000),
            ..state(TaskMode::Scheduled, Some(due))
        };
        assert_eq!(decide(&leased, Some(&schedule), due), Action::None);
        assert_eq!(
            decide(&leased, Some(&schedule), due + 29_999),
            Action::None,
            "the lease holds up to the instant before it expires"
        );
        assert_eq!(
            decide(&leased, Some(&schedule), due + 30_000),
            Action::Run,
            "at expiry the holder is presumed dead and the task is reclaimable"
        );
    }

    /// `update` is not a task kind and never will be; a hand-written row naming
    /// it must read as unknown, exactly like any other kind this build cannot
    /// run (NFR-43).
    #[test]
    fn an_unrecognised_stored_kind_including_update_is_not_a_kind_this_build_runs() {
        for value in [
            "update", "Sync", "", "teleport", "Release", "releases", "Verify", "verifies",
            // Story 59.9's own near-miss, and the one that matters most: the
            // deferred kind is an arbitrary command, so the spelling a
            // hand-written row would most plausibly try is a verb-looking
            // string this build does not own.
            "exec", "run",
        ] {
            assert_eq!(
                TaskKind::from_stored(value),
                None,
                "{value:?} is not a kind this build knows"
            );
        }
        assert_eq!(TaskKind::from_stored("sync"), Some(TaskKind::Sync));
        // Story 57.4's kind, asserted beside the refusals rather than in a test
        // of its own: the claim worth making is that adding a second kind did
        // not widen the vocabulary by anything else, and `update` in particular
        // is still nothing this build can name.
        assert_eq!(TaskKind::from_stored("release"), Some(TaskKind::Release));
        // Story 59.9's kind, asserted here for the same reason `release` is:
        // the claim is that a *third* variant widened the vocabulary by
        // exactly one word, and that `update` is still nothing this build can
        // name after it.
        assert_eq!(TaskKind::from_stored("verify"), Some(TaskKind::Verify));
    }

    /// The on-disk spellings are the compatibility surface, so every one of
    /// them must survive a round trip through the reader that parses it.
    #[test]
    fn every_stored_spelling_round_trips() {
        for kind in [TaskKind::Sync, TaskKind::Release, TaskKind::Verify] {
            assert_eq!(TaskKind::from_stored(kind.as_str()), Some(kind));
        }
        for mode in [TaskMode::Off, TaskMode::Manual, TaskMode::Scheduled] {
            assert_eq!(TaskMode::from_stored(mode.as_str()), Some(mode));
        }
        for outcome in [
            TaskOutcome::Ok,
            TaskOutcome::Busy,
            TaskOutcome::Deferred,
            TaskOutcome::Failed,
            TaskOutcome::Abandoned,
        ] {
            assert_eq!(TaskOutcome::from_stored(outcome.as_str()), Some(outcome));
        }
    }

    /// The id rule, at the one place that now holds it (Story 57.3).
    ///
    /// Asserted here rather than only through `db::upsert_task` because
    /// 57.3's CLI asks this function *before* it opens a database, to tell "a
    /// spelling this keeper could never have stored" apart from "well formed,
    /// but no such task" — two different sentences to the person at the prompt.
    /// An interior space is accepted deliberately: the rule is about the edges,
    /// and refusing more than the write door refuses would have the selector
    /// reject an id `upsert_task` is perfectly willing to store.
    #[test]
    fn the_id_rule_accepts_what_the_write_door_stores_and_refuses_what_it_will_not() {
        for id in ["nightly", "01JTASK", "release sweep"] {
            assert!(
                validate_id(id).is_ok(),
                "{id:?} is a spelling this keeper stores"
            );
        }
        for id in ["", "   ", "\t", " nightly", "nightly ", "nightly\n"] {
            let err = validate_id(id).expect_err("refused");
            assert!(
                matches!(err, SyncError::Config(_)),
                "{id:?} must be a typed configuration refusal, got {err:?}"
            );
        }
        // The two messages, verbatim, because 57.3's CLI prints them and a
        // refusal that does not quote the input leaves nothing to fix.
        assert_eq!(
            validate_id("").expect_err("refused").to_string(),
            "invalid sync configuration: task id must not be empty"
        );
        assert_eq!(
            validate_id("nightly ").expect_err("refused").to_string(),
            "invalid sync configuration: task id must not begin or end with whitespace, \
             got \"nightly \""
        );
    }

    /// **Every schedule the dev harness shows must be one this parser accepts**
    /// (Story 57.5's review, finding 12).
    ///
    /// `dev/mock-shell.ts` is the only place a developer on a Linux host can see
    /// the Tasks view at all, and its own header promises that "what a browser on
    /// Linux shows is what the real command would answer". Three fixtures read
    /// `@daily 03:00`, and this function strips the `@` and matches the *entire*
    /// remainder against `hourly|daily|weekly` — so no such row can exist in a
    /// real `sync.db` and the harness was teaching a syntax the dialect does not
    /// have. The dialect's way to say 03:00 daily is `0 3 * * *`.
    ///
    /// Asserted from the engine's own parser over the harness's own text, rather
    /// than by a second list of expected strings, because a second list is the
    /// thing that goes stale. Precedent: `keeper-syncd`'s tests read the shipped
    /// unit files and feed their `ExecStart` through clap.
    #[test]
    fn every_schedule_the_dev_harness_shows_is_one_this_dialect_accepts() {
        let harness = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../dev/mock-shell.ts")
            .canonicalize()
            .expect("dev/mock-shell.ts is in this repository");
        let source = std::fs::read_to_string(&harness).expect("read the harness");

        // `schedule: "…"`, which is how the fixtures spell it. A `null` schedule
        // and the passthrough `schedule: req.schedule` are not literals and are
        // not matched, which is correct: neither describes a dialect expression.
        let mut found = Vec::new();
        for rest in source.split("schedule: \"").skip(1) {
            let Some(literal) = rest.split('"').next() else {
                continue;
            };
            found.push(literal.to_owned());
        }

        // Before anything is asserted about the contents: a renamed field or a
        // reshaped fixture would otherwise make this test pass over nothing.
        assert!(
            found.len() >= 5,
            "the extraction found {} schedule literals in {}, which is too few to \
             be the fixture block — has the field been renamed?",
            found.len(),
            harness.display()
        );

        for literal in &found {
            let parsed = TaskSchedule::parse(literal);
            assert!(
                parsed.is_ok(),
                "the dev harness shows {literal:?}, which this dialect refuses: {}",
                parsed.expect_err("refused")
            );
        }
    }

    /// One case per setting, and the whole point is the **boundary**: at
    /// `next_due_ms + TASK_MISSED_GRACE_MS` the two non-default policies are
    /// exact complements, and either side of it neither is the same as
    /// `run_now`.
    ///
    /// Asserted against literal integers with no clock and no database, which is
    /// what the pure split is for — and named as the *pure half* deliberately:
    /// the risk in this story lives in the claim and in the store, and those are
    /// asserted in `db.rs` and `engine.rs`.
    #[test]
    fn each_missed_window_policy_decides_its_own_side_of_the_grace_boundary() {
        let due_at = JAN_1_2024_UTC;
        let hourly = cron("@hourly");
        let fresh = due_at + 1;
        let stale = due_at + TASK_MISSED_GRACE_MS;

        for (policy, at_fresh, at_stale) in [
            // Unconditional, and textually the arm this function has always
            // had: an upgrade changes no install's meaning.
            (TaskMissedPolicy::RunNow, Action::Run, Action::Run),
            // Inside the grace a host was here and serves its own window; past
            // it the window was nobody's and is held back to an instant this
            // host computes from NOW — see `TASK_MISSED_DELAY_MS` for why the
            // anchor cannot be the window.
            (TaskMissedPolicy::Delay, Action::Run, Action::Postpone),
            // The same boundary, the other answer: dropped and re-armed.
            (TaskMissedPolicy::Skip, Action::Run, Action::Skip),
        ] {
            assert_eq!(
                decide(
                    &state_with(TaskMode::Scheduled, Some(due_at), policy),
                    Some(&hourly),
                    fresh
                ),
                at_fresh,
                "{} one millisecond into an open window",
                policy.as_str()
            );
            assert_eq!(
                decide(
                    &state_with(TaskMode::Scheduled, Some(due_at), policy),
                    Some(&hourly),
                    stale
                ),
                at_stale,
                "{} at the grace boundary",
                policy.as_str()
            );
        }
    }

    /// AD-138, in the pure layer: the policy may govern the window and may never
    /// enumerate it. Two hundred windows of clock produce **one** decision under
    /// every setting, because there is nothing to enumerate — `next_due_ms` is
    /// one `i64`, so overdue-by-one and overdue-by-two-hundred are the same
    /// state and this function cannot tell them apart.
    #[test]
    fn no_policy_can_tell_one_missed_window_from_two_hundred() {
        let due_at = JAN_1_2024_UTC;
        let hourly = cron("@hourly");
        for policy in [
            TaskMissedPolicy::RunNow,
            TaskMissedPolicy::Delay,
            TaskMissedPolicy::Skip,
        ] {
            let one = decide(
                &state_with(TaskMode::Scheduled, Some(due_at), policy),
                Some(&hourly),
                due_at + HOUR_MS,
            );
            let many = decide(
                &state_with(TaskMode::Scheduled, Some(due_at), policy),
                Some(&hourly),
                due_at + 200 * HOUR_MS,
            );
            assert_eq!(
                one,
                many,
                "{} must answer the same thing about one missed window and two \
                 hundred, or something somewhere is counting them",
                policy.as_str()
            );
        }

        // And the answer is one run, or none — never a number that scales.
        assert_eq!(
            decide(
                &state_with(TaskMode::Scheduled, Some(due_at), TaskMissedPolicy::Skip),
                Some(&hourly),
                due_at + 200 * DAY_MS
            ),
            Action::Skip,
            "a task out of service for two hundred days drops one window, not two hundred"
        );
    }

    /// The stored vocabulary is a closed set and an unknown spelling is `None`,
    /// which is what routes the row to `decode_task`'s unknown path rather than
    /// to a policy this build guessed. Guessing is the dangerous direction:
    /// reading `"teleport"` as `run_now` would run a window its author asked to
    /// skip.
    #[test]
    fn a_missed_window_policy_round_trips_and_an_unknown_spelling_is_refused() {
        for policy in [
            TaskMissedPolicy::RunNow,
            TaskMissedPolicy::Delay,
            TaskMissedPolicy::Skip,
        ] {
            assert_eq!(TaskMissedPolicy::from_stored(policy.as_str()), Some(policy));
        }
        assert_eq!(
            TaskMissedPolicy::default(),
            TaskMissedPolicy::RunNow,
            "the default reproduces today's restart behaviour, which is the whole \
             reason it is the default"
        );
        for spelling in ["", "run now", "RUN_NOW", "runnow", "teleport", "none"] {
            assert_eq!(
                TaskMissedPolicy::from_stored(spelling),
                None,
                "{spelling:?} is not a policy this build may act on"
            );
        }
    }

    /// An absent override *is* the constant, and that is the whole compatibility
    /// claim of this column (Story 59.6, FR-366).
    ///
    /// Asserted as an equality against `TASK_MISSED_DELAY_MS` rather than against
    /// `30 * 60_000`, so changing the constant cannot make this test the thing
    /// that has to be edited — and so a future `unwrap_or(0)`, which is the one
    /// typo this function can contain, fails here rather than in a field install
    /// where `delay` would silently have become `run_now`.
    #[test]
    fn a_task_with_no_delay_of_its_own_waits_exactly_the_constant() {
        assert_eq!(effective_missed_delay_ms(None), TASK_MISSED_DELAY_MS);
        assert_ne!(
            effective_missed_delay_ms(None),
            0,
            "a zero default would make every delay task a run_now task, at the \
             cost of one extra write per absence"
        );
        // And an override is honoured verbatim: the floor is a write-door rule,
        // so nothing on the read path may quietly raise a stored value to it.
        for delay_ms in [
            TASK_MISSED_GRACE_MS,
            TASK_MISSED_DELAY_MS + 1,
            MAX_SCHEDULE_INTERVAL_MS,
        ] {
            assert_eq!(effective_missed_delay_ms(Some(delay_ms)), delay_ms);
        }
    }

    /// The two bounds, at the boundary rather than near it, and each refusal
    /// quotes the value (Story 59.6, FR-366).
    ///
    /// The floor is `TASK_MISSED_GRACE_MS` exactly — inclusive, because a delay
    /// equal to the grace is coherent: the window is recognised as missed and
    /// held back by the same interval again, which is a real, if impatient,
    /// answer. One millisecond below it is not, and that is the assertion that
    /// pins the inclusivity rather than leaving it to a reader of `<`.
    #[test]
    fn a_delay_shorter_than_the_grace_or_longer_than_a_year_is_refused() {
        for accepted in [
            None,
            Some(TASK_MISSED_GRACE_MS),
            Some(TASK_MISSED_DELAY_MS),
            Some(MAX_SCHEDULE_INTERVAL_MS),
        ] {
            assert!(
                validate_missed_delay_ms(accepted).is_ok(),
                "{accepted:?} is a delay a task may carry"
            );
        }

        for refused in [0, -1, 1, TASK_MISSED_GRACE_MS - 1, i64::MIN] {
            let err = validate_missed_delay_ms(Some(refused))
                .expect_err("shorter than the interval that concludes nobody was home");
            let message = err.to_string();
            assert!(
                message.contains("concludes nobody was home"),
                "the refusal must say why the grace period is the floor; got {message}"
            );
            assert!(
                message.contains(&refused.to_string()),
                "a refusal quotes the value it refused, as every other one in this \
                 module does; got {message}"
            );
        }

        for refused in [MAX_SCHEDULE_INTERVAL_MS + 1, i64::MAX] {
            let message = validate_missed_delay_ms(Some(refused))
                .expect_err("longer than a schedule may be")
                .to_string();
            assert!(
                message.contains("must not exceed a year"),
                "the refusal must name the ceiling it shares with the schedule; got {message}"
            );
            assert!(
                message.contains(&refused.to_string()),
                "a refusal quotes the value it refused; got {message}"
            );
        }
    }
}
