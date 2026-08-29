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
}

impl TaskKind {
    /// Stable on-disk spelling, kept separate from any serde representation so
    /// a UI-facing rename can never invalidate stored rows.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sync => "sync",
        }
    }

    /// Parse the *stored* spelling. `None` is the forward-compatibility door,
    /// not an error: the caller skips the row and reports it as unknown.
    pub fn from_stored(value: &str) -> Option<Self> {
        match value {
            "sync" => Some(Self::Sync),
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

/// How one run ended.
///
/// Three of the five are deliberately **not** failures, and keeping them apart
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
        Some(at) if now_ms >= at => Action::Run,
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
        for value in ["update", "release", "Sync", "", "teleport"] {
            assert_eq!(
                TaskKind::from_stored(value),
                None,
                "{value:?} is not a kind this build knows"
            );
        }
        assert_eq!(TaskKind::from_stored("sync"), Some(TaskKind::Sync));
    }

    /// The on-disk spellings are the compatibility surface, so every one of
    /// them must survive a round trip through the reader that parses it.
    #[test]
    fn every_stored_spelling_round_trips() {
        // One variant today, so this is an assertion rather than a loop; the
        // loop returns when a second kind lands.
        assert_eq!(
            TaskKind::from_stored(TaskKind::Sync.as_str()),
            Some(TaskKind::Sync)
        );
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
}
