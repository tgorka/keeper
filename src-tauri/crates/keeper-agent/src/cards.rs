//! Cards on the board, as an agent's host writes and runs them (stories
//! 92.2 and 92.3, AD-386, AD-387).
//!
//! Two writers, both through the journaled executor:
//!
//! - **The host's run writer** ([`write_run`]): `run:` and `last_run:`, by
//!   the session's claim holder, on a transition only, as a guarded write
//!   on the bytes it read — re-read and tried once more when the card's
//!   length changed under it (a person's move, a pull), then reported.
//! - **An agent's session tools** (R50): `card_update` sets a card's
//!   `status`, `order`, `assignee` and `host`, and `session_write` writes a
//!   file of the agent's own session. Both are served from the agent's own
//!   host through [`ToolHost::run_named`] — never a ⌘9 bot's (R38) — and both
//!   pass what they write through [`card::stamp_agent_write`], the one door
//!   every agent write of a card goes through (R52). They are the only door:
//!   an agent host's `drive_write` and `drive_edit` never reach the sessions
//!   zone (R51).
//!
//! An agent writes cards of the session its turn runs in, whose claim its
//! host holds (NFR-120): a card in another session or another drive is out of
//! reach by construction.
//!
//! **A card that runs on a schedule** (92.3) lives alone in a
//! `kind = scheduled` session of its assignee (Q9, R57). The holder of that
//! session's claim runs it: [`due`] says when, over keeper-sync's
//! [`TaskSchedule`] — never a `TaskKind` row (AD-387, R9) — and [`begin`]
//! writes `run: running` and `last_run:` under the claim before the turn.
//!
//! [`ToolHost::run_named`]: keeper_core::bots::tools::ToolHost::run_named

use std::path::{Path, PathBuf};

use keeper_core::agents::card::{self, CardAgent, Field, Run};
use keeper_core::agents::index::{read_card_files, read_prefix};
use keeper_core::agents::knowledge::{self, KnowledgeRefusal};
use keeper_core::agents::label::{check_sink, Readers, Sink, SinkVerdict};
use keeper_core::agents::log::{HostSlug, RunBody, RunState};
use keeper_core::agents::session::{SessionAgent, SessionKind};
use keeper_core::bots::chat::{ToolCall as WireToolCall, ToolSpec};
use keeper_core::bots::tools::ToolOutcome;
use keeper_core::notes::frontmatter::{FieldValue, Frontmatter};
use keeper_core::notes::order::set_order_in;
use keeper_core::sessions::files;
use keeper_core::sessions::model::{classify, SessionStatus};
use keeper_core::sessions::plan::{Plan, PlanStep};
use keeper_core::sessions::pool::{read_one, PoolFile};
use keeper_core::sessions::shape::{KindTag, TaskStatus};
use keeper_core::sessions::tasks::TASK_STATUS_KEY;
use keeper_sync::browse;
use keeper_sync::tasks::TaskSchedule;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::delegate::{Delegator, TurnView};
use crate::sessions::exec::{self, ExecError};
use crate::sessions::lock::ZoneLock;
use crate::sessions::verbs::VerbError;
use crate::sessions::write::{in_session, landing, session_write_with, NO_CLAIM};

/// The tool that edits a card's keys.
pub const CARD_UPDATE: &str = "card_update";
/// The tool that writes a file of the agent's own session.
pub const SESSION_WRITE: &str = "session_write";

/// What refuses `run` and `last_run`.
pub const HOST_WRITES: &str = "is written by the host that runs the card, never by a tool.";
/// What refuses `scheduled_by` and `integrity`.
pub const KEEPER_WRITES: &str =
    "is written by keeper: it marks what an agent wrote, and only a person removes it.";

/// Whether `name` is `card_update` or `session_write`.
pub fn is_card_tool(name: &str) -> bool {
    name == CARD_UPDATE || name == SESSION_WRITE
}

/// The specs a turn is offered: each tool `[tools].allow` names.
pub fn specs(allow: &[String]) -> Vec<ToolSpec> {
    let allowed = |name: &str| allow.iter().any(|allowed| allowed == name);
    let mut specs = Vec::new();
    if allowed(CARD_UPDATE) {
        specs.push(ToolSpec {
            name: CARD_UPDATE.to_owned(),
            description: "Change a card of this session (a file tagged task): its status (in-preparation, todo, done or deferred), order, assignee or host. A schedule or a workflow needs a person. run, last_run, scheduled_by and integrity are keeper's.".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "card": {"type": "string", "description": "The card's path in this session, e.g. brief.md."},
                    "fields": {
                        "type": "object",
                        "description": "The keys to set.",
                        "properties": {
                            "status": {"type": "string", "enum": keeper_core::sessions::shape::STATUSES.map(TaskStatus::as_str)},
                            "order": {"type": "number"},
                            "assignee": {"type": "string"},
                            "host": {"type": "string"},
                            "schedule": {"type": "string"},
                            "workflow": {"type": "string"}
                        }
                    }
                },
                "required": ["card", "fields"],
                "additionalProperties": false
            }),
        });
    }
    if allowed(SESSION_WRITE) {
        specs.push(ToolSpec {
            name: SESSION_WRITE.to_owned(),
            description: "Write a file into this session, replacing it if it is there: markdown, csv or json anywhere in the session (a card is markdown tagged task); finished output under artifacts/, which also takes yaml, yml, toml, txt and html; anything under workspace/. keeper's own files (log/, approvals/, agent.toml, README.md, AGENTS.md) are not yours to write.".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "The file's path in this session, or from the drive's root as bmad_config's and bmad_render's write locations name it."},
                    "content": {"type": "string"}
                },
                "required": ["path", "content"],
                "additionalProperties": false
            }),
        });
    }
    specs
}

/// Set the host's keys of the card at `rel` in the session at `session`
/// (zone-relative): `run:`, and `last_run:` when given. Whether it wrote:
/// a card that already says both is left as it is. `may_write` is the
/// session's claim, asked right before the write (R120).
pub fn write_run(
    zone: &Path,
    session: &str,
    rel: &str,
    run: Run,
    last_run: Option<&str>,
    may_write: &dyn Fn() -> bool,
) -> Result<bool, VerbError> {
    let held = exec::hold(zone)?;
    rewrite(
        &held,
        session,
        rel,
        "card-run",
        |text| Ok(card::set_host_keys(text, run, last_run)),
        may_write,
        &mut |_| {},
    )
}

/// [`write_run`] of `run: blocked` alone, saying the card's exact bytes
/// before and after the write — what a park re-pins by (R178); `None` when
/// the card said so already.
pub fn block_run(
    zone: &Path,
    session: &str,
    rel: &str,
    may_write: &dyn Fn() -> bool,
) -> Result<Option<(String, String)>, VerbError> {
    let seen = std::cell::RefCell::new(None);
    let held = exec::hold(zone)?;
    let wrote = rewrite(
        &held,
        session,
        rel,
        "card-run",
        |text| {
            let next = card::set_host_keys(text, Run::Blocked, None);
            *seen.borrow_mut() = next.clone().map(|next| (text.to_owned(), next));
            Ok(next)
        },
        may_write,
        &mut |_| {},
    )?;
    Ok(if wrote { seen.into_inner() } else { None })
}

/// Rewrite one file of a session under the zone's hold, where it lands
/// through the session fence ([`landing`]): `compose` makes the new bytes
/// from the current ones (`None`: nothing to write), and the write is
/// guarded on the exact bytes read (R120). A file that changed by the time
/// the write ran is read and composed once more; a second change is
/// reported. `may_write` is asked right before each write: without the
/// session's claim nothing is written. `between` runs after each read —
/// where another writer's change lands.
fn rewrite(
    held: &ZoneLock,
    session: &str,
    rel: &str,
    verb: &str,
    compose: impl Fn(&str) -> Result<Option<String>, String>,
    may_write: &dyn Fn() -> bool,
    between: &mut dyn FnMut(&Path),
) -> Result<bool, VerbError> {
    let landed = landing(held.zone(), session, rel)?;
    files::check_rewritable(&landed).map_err(|refusal| VerbError::Refused(refusal.to_string()))?;
    let file = browse::lexical_join(held.zone(), &format!("{session}/{landed}"))
        .map_err(|refusal| VerbError::Refused(refusal.to_string()))?;
    let mut attempts = 0;
    loop {
        attempts += 1;
        let text = std::fs::read_to_string(&file)
            .map_err(|error| VerbError::Refused(format!("{rel} could not be read: {error}")))?;
        let Some(content) = compose(&text).map_err(VerbError::Refused)? else {
            return Ok(false);
        };
        between(&file);
        if !may_write() {
            return Err(VerbError::Refused(NO_CLAIM.to_owned()));
        }
        let plan = Plan {
            verb: verb.to_owned(),
            session: session.to_owned(),
            steps: vec![PlanStep::guarded(
                format!("{session}/{landed}"),
                &text,
                content,
            )],
        };
        match exec::run_held(plan, held) {
            Ok(()) => return Ok(true),
            Err(ExecError::Refused(_))
                if attempts == 1 && std::fs::read_to_string(&file).is_ok_and(|now| now != text) => {
            }
            Err(error) => return Err(error.into()),
        }
    }
}

/// Whether a scheduled card runs now (AD-387, R58).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Due {
    /// The card has no `schedule:`.
    Unscheduled,
    /// Its `schedule:` or its `last_run:` does not read: it never runs, and
    /// the board shows the key as unreadable.
    Unreadable,
    /// An agent wrote its schedule and no person allowed it (Q16): it never
    /// runs, and the board shows who scheduled it.
    Unticked,
    /// Its next window after `last_run` is still to come.
    NotDue,
    /// It runs now, in the window starting at `window_ms` (epoch ms): the
    /// latest instant the schedule fires at or before now, so the windows a
    /// host missed while away run once, as this one (DW-376).
    Due { window_ms: i64 },
}

/// Whether `card` runs at `now_ms`, its schedule read at the local offset
/// `utc_offset_minutes` (keeper-sync's dialect: `@daily`, five-field cron,
/// `every <n><unit>` no oftener than a minute).
///
/// Due ⇔ the schedule's first window after `last_run` is at or before now
/// (R58); a card that never ran is due at once. The window it runs is the
/// latest one at or before now, written as `last_run` and named in the
/// claim: `next_due_after` is strictly after, so the card is not due again
/// until the next window. An `every` card that never ran takes the minute
/// it is first seen in, the same on every host that sees it then.
pub fn due(card: &CardAgent, now_ms: i64, utc_offset_minutes: i32) -> Due {
    let Some(raw) = card.schedule.as_deref() else {
        return Due::Unscheduled;
    };
    if card.marked() {
        return Due::Unticked;
    }
    let Ok(schedule) = TaskSchedule::parse(raw) else {
        return Due::Unreadable;
    };
    let last = match &card.last_run {
        None => None,
        Some(Field::Read(at)) => Some(at.timestamp_millis()),
        Some(Field::Unreadable(_)) => return Due::Unreadable,
    };
    let window = match (schedule, last) {
        (TaskSchedule::Every { interval_ms }, Some(last)) => {
            let missed = now_ms.saturating_sub(last) / interval_ms;
            (missed >= 1).then(|| last + missed * interval_ms)
        }
        (TaskSchedule::Every { .. }, None) => Some(now_ms - now_ms.rem_euclid(60_000)),
        (TaskSchedule::Cron(_), last) => {
            // A card that never ran: the latest window at or before now,
            // as if it had run just before that one.
            let since = last.unwrap_or(now_ms.saturating_sub(MAX_LOOKBACK_MS));
            schedule
                .next_due_after(since, utc_offset_minutes)
                .filter(|first| *first <= now_ms)
                .map(|first| latest_fire(&schedule, first, now_ms, utc_offset_minutes))
        }
    };
    window.map_or(Due::NotDue, |window_ms| Due::Due { window_ms })
}

/// How far back a cron card that never ran looks for its first window: the
/// horizon keeper-sync's own search covers, eight years and two days, for
/// the sparsest schedule its dialect accepts — `0 0 29 2 *`, whose windows
/// are eight years apart across 2100. A year would leave such a card
/// `NotDue` until its next window instead of running its latest one.
const MAX_LOOKBACK_MS: i64 = (366 * 8 + 2) * 24 * 60 * 60_000;

/// The latest instant `schedule` fires at or before `now_ms`, given that it
/// fires at `first` ≤ `now_ms`. The search is bounded: it looks back over
/// spans doubling from the dialect's one minute until one holds a fire,
/// then walks forward only across that span, so a host away for a year
/// asks the schedule a few dozen times, not once per missed window.
pub(crate) fn latest_fire(schedule: &TaskSchedule, first: i64, now_ms: i64, offset: i32) -> i64 {
    let mut span = keeper_sync::tasks::MIN_SCHEDULE_INTERVAL_MS;
    loop {
        let from = now_ms.saturating_sub(span).max(first);
        if let Some(mut fire) = schedule
            .next_due_after(from, offset)
            .filter(|at| *at <= now_ms)
        {
            while let Some(next) = schedule
                .next_due_after(fire, offset)
                .filter(|at| *at <= now_ms)
            {
                fire = next;
            }
            return fire;
        }
        if from == first {
            return first;
        }
        span = span.saturating_mul(2);
    }
}

/// A card of a scheduled session, read when the host rescans its zone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledCard {
    /// Session-relative.
    pub rel: String,
    pub card: CardAgent,
}

/// The cards carrying `schedule:` of one session, as far as a bounded read
/// established them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScheduledScan {
    pub cards: Vec<ScheduledCard>,
    /// Why the read could not establish every such card — a folder or a
    /// file that did not read, the walk's or the read's budget spent. While
    /// it could not, none runs: a card it missed may be a second one (R57).
    pub incomplete: Option<String>,
}

/// The most bytes the scan reads of one file. A longer one whose
/// frontmatter reads whole and names no schedule is no scheduled card; any
/// other leaves the scan incomplete.
const SCAN_FILE_BYTES: u64 = 64 * 1024;
/// The most bytes the scan reads of one session.
const SCAN_SESSION_BYTES: u64 = 1024 * 1024;

/// The cards carrying `schedule:` of the session `session` at `dir`,
/// wherever the board's pool reads a card, by keeper-core's bounded read
/// ([`read_card_files`]): at the host's rescan, and again under the claim
/// before a window begins.
pub fn scheduled_cards(session: &str, dir: &Path) -> ScheduledScan {
    let read = read_card_files(session, dir, SCAN_FILE_BYTES, SCAN_SESSION_BYTES);
    let mut problems = read.problems;
    let mut cards = Vec::new();
    for file in read.files {
        if !file.whole {
            let (fm, body_at) = Frontmatter::parse(&file.text);
            let may_name = if body_at > 0 {
                fm.count(card::SCHEDULE) > 0
            } else {
                file.text.starts_with("---")
            };
            if may_name {
                problems.push(format!(
                    "{session}/{}: more than {SCAN_FILE_BYTES} bytes, past what is read for a schedule.",
                    file.rel
                ));
            }
            continue;
        }
        if let Some(card) = scheduled_of(&file.rel, &file.text) {
            cards.push(ScheduledCard {
                rel: file.rel,
                card,
            });
        }
    }
    ScheduledScan {
        cards,
        incomplete: problems.into_iter().next(),
    }
}

/// `text`'s agent keys, when it is a card carrying `schedule:`.
fn scheduled_of(rel: &str, text: &str) -> Option<CardAgent> {
    if read_one(PoolFile { rel, text }).kind != Some(KindTag::Task) {
        return None;
    }
    CardAgent::of_text(text).filter(|card| card.schedule.is_some())
}

/// The card at `rel` of the session folder `dir`, read now by a bounded
/// read: `None` once it is no card carrying `schedule:`. The holder decides
/// a window by it, never by its rescan's copy (R163).
pub fn read_scheduled(dir: &Path, rel: &str) -> Result<Option<CardAgent>, String> {
    let path = rel
        .split('/')
        .fold(dir.to_path_buf(), |path, part| path.join(part));
    let (text, _, whole) =
        read_prefix(&path, SCAN_FILE_BYTES).map_err(|error| format!("{rel}: {error}"))?;
    if !whole {
        return Err(format!(
            "{rel}: more than {SCAN_FILE_BYTES} bytes, past what is read for a schedule"
        ));
    }
    Ok(scheduled_of(rel, &text))
}

/// The scheduled card `session` runs, from its folder's scan (Q9, R57): a
/// scheduled card runs only in a session of its assignee, which holds it
/// alone — its `host:` is that session's pin. `Ok(None)`: the session has
/// none. `Err`: the sentence saying why the one it holds does not run
/// there, or why the scan cannot tell it is alone. `session` is `None` for
/// a person's session, which has no `agent.toml`.
pub fn session_schedule<'c>(
    session: Option<&SessionAgent>,
    scan: &'c ScheduledScan,
) -> Result<Option<&'c ScheduledCard>, String> {
    if let Some(problem) = &scan.incomplete {
        return Err(format!(
            "the session's cards do not all read, so none runs on a schedule: {problem}"
        ));
    }
    let cards = &scan.cards;
    let Some(first) = cards.first() else {
        return Ok(None);
    };
    let assignee = match &first.card.assignee {
        Some(Field::Read(id)) => id.as_str(),
        Some(Field::Unreadable(raw)) => raw.as_str(),
        None => "its assignee",
    };
    let belongs = matches!(
        (&first.card.assignee, session),
        (Some(Field::Read(id)), Some(session)) if *id == session.agent
    );
    if !belongs {
        return Err(format!("runs only in {assignee}'s session"));
    }
    if session.is_some_and(|session| session.kind != SessionKind::Scheduled) || cards.len() > 1 {
        return Err(format!(
            "runs only alone in a scheduled session of {assignee}"
        ));
    }
    if let Some(Field::Unreadable(raw)) = &first.card.host {
        return Err(format!("its host: {raw} is not a host's name"));
    }
    Ok(Some(first))
}

/// What a scheduled session's host asks its worker to do about the card,
/// under the session's claim (R56–R58, R163, R164). It travels to the
/// worker as the content of a scheduled arrival.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum Scheduled {
    /// Run the card's window `window` (RFC 3339), which the claim names, as
    /// the host's clock found it due at `now_ms` read at the offset
    /// `utc_offset_minutes`: the worker judges the card it reads under the
    /// claim by the same instant.
    Run {
        card: String,
        window: String,
        now_ms: i64,
        utc_offset_minutes: i32,
    },
    /// No host can run it now: the card says `run: waiting`, the `run`
    /// line what it waits for (Q8).
    Wait { card: String, waiting: String },
    /// This host holds the session after `host`, whose claim named `window`:
    /// whether that window ran, or how a run the card still says is running
    /// ended, is not known here (Q18, S-25, R163, R164).
    TakenOver {
        card: String,
        window: Option<String>,
        host: String,
    },
}

impl Scheduled {
    /// The card it is about, session-relative.
    pub fn card(&self) -> &str {
        match self {
            Scheduled::Run { card, .. }
            | Scheduled::Wait { card, .. }
            | Scheduled::TakenOver { card, .. } => card,
        }
    }
}

/// Who begins a window: the session's agent, on this host.
#[derive(Debug, Clone, Copy)]
pub struct Holder<'h> {
    pub agent: &'h SessionAgent,
    pub host: &'h str,
}

/// What [`begin`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Begun {
    /// Nothing: the sentence says why.
    Nothing(&'static str),
    /// The card says what this `run` line says; no turn runs.
    Said(RunBody),
    /// `run: running` and `last_run:` are written: a turn runs with the
    /// card's body as its brief, read at the card's integrity.
    Run { brief: String, untrusted: bool },
}

/// A window the card's `last_run` has reached already.
pub const WINDOW_RAN: &str = "the card's last_run has reached this window already";
/// A card whose schedule no person allowed yet.
pub const SCHEDULE_UNTICKED: &str = "an agent's schedule runs only once a person allows it";
/// A card that waits already.
pub const WAITS_ALREADY: &str = "the card says it waits already";
/// A card that is no longer this session's one scheduled card.
pub const NOT_ITS_SCHEDULE: &str =
    "the card no longer runs alone on a schedule in this session of its assignee";
/// A card pinned to another host since the window was found.
pub const PINNED_ELSEWHERE: &str = "the card is pinned to another host";
/// A card whose schedule or `last_run` no longer makes this window due.
pub const NOT_DUE: &str = "the card as it reads now is not due in this window";

/// The `run` line of a window another host may have run.
pub fn effect_unknown(host: &str) -> String {
    format!("ran on {host}, effect unknown")
}

/// Whether a claim's window `window` is unsettled on `card` (R163, R164):
/// its `last_run` is before the window, absent or unreadable — the window
/// may have run with nothing written here — or the card still says
/// `run: running`, a run nobody finishes now.
pub fn unsettled(card: &CardAgent, window: Option<&str>) -> bool {
    matches!(card.run, Some(Field::Read(Run::Running)))
        || window.is_some_and(|window| behind(card, window))
}

/// Whether `card`'s `last_run` is before `window`, absent or unreadable. A
/// window that is no instant is none.
fn behind(card: &CardAgent, window: &str) -> bool {
    let Ok(window) = chrono::DateTime::parse_from_rfc3339(window) else {
        return false;
    };
    !matches!(&card.last_run, Some(Field::Read(at)) if *at >= window)
}

/// Why the card's `text`, read under the claim, does not run `window` as
/// judged at `now_ms` and `offset`: what the host's clock found at its read
/// is found again on these bytes — still a task, still a person's or an
/// allowed schedule, still its agent's and pinned nowhere else, and still
/// due in exactly this window.
fn not_runnable(
    rel: &str,
    text: &str,
    holder: &Holder<'_>,
    window: &str,
    now_ms: i64,
    offset: i32,
) -> Option<&'static str> {
    let Some(card) = scheduled_of(rel, text) else {
        return Some(NOT_ITS_SCHEDULE);
    };
    if card.marked() {
        return Some(SCHEDULE_UNTICKED);
    }
    if !matches!(&card.assignee, Some(Field::Read(id)) if *id == holder.agent.agent) {
        return Some(NOT_ITS_SCHEDULE);
    }
    match &card.host {
        Some(Field::Read(pin)) if pin.as_str() == holder.host => {}
        None => {}
        Some(_) => return Some(PINNED_ELSEWHERE),
    }
    let Ok(at) = chrono::DateTime::parse_from_rfc3339(window) else {
        return Some(NOT_DUE);
    };
    if !behind(&card, window) {
        return Some(WINDOW_RAN);
    }
    match due(&card, now_ms, offset) {
        Due::Due { window_ms } if window_ms == at.timestamp_millis() => None,
        _ => Some(NOT_DUE),
    }
}

/// Act on `scheduled` for the card of the session at `session` (zone
/// relative), as `holder`, reading the card again under the claim
/// (`may_write`, asked right before the write): the bytes it writes on
/// decide, never what the host read before.
///
/// - `Run`: a card that is still the session's one scheduled card, its
///   agent's, pinned nowhere else, a person's or an allowed schedule, and
///   due in exactly this window gets `run: running` and `last_run` = the
///   window, and its turn runs.
/// - `Wait`: `run: waiting`, when the card does not say so already.
/// - `TakenOver`: a card on which the previous claim's window is unsettled
///   ([`unsettled`]) gets `run: review`, and `last_run` = that window when
///   it was before it, its line saying the effect is unknown; no turn runs.
pub fn begin(
    zone: &Path,
    session: &str,
    holder: &Holder<'_>,
    scheduled: &Scheduled,
    may_write: &dyn Fn() -> bool,
) -> Result<Begun, VerbError> {
    let skipped = std::cell::Cell::new(None);
    let body = std::cell::RefCell::new(None);
    let held = exec::hold(zone)?;
    if let Scheduled::Run { card, .. } = scheduled {
        let dir = browse::lexical_join(zone, session)
            .map_err(|refusal| VerbError::Refused(refusal.to_string()))?;
        let scan = scheduled_cards(session, &dir);
        if !session_schedule(Some(holder.agent), &scan)
            .is_ok_and(|found| found.is_some_and(|found| found.rel == *card))
        {
            return Ok(Begun::Nothing(NOT_ITS_SCHEDULE));
        }
    }
    let wrote = rewrite(
        &held,
        session,
        scheduled.card(),
        "card-run",
        |text| {
            let (run, last_run) = match scheduled {
                Scheduled::Run {
                    card,
                    window,
                    now_ms,
                    utc_offset_minutes,
                } => {
                    if let Some(why) =
                        not_runnable(card, text, holder, window, *now_ms, *utc_offset_minutes)
                    {
                        skipped.set(Some(why));
                        return Ok(None);
                    }
                    let (_, body_at) = Frontmatter::parse(text);
                    *body.borrow_mut() = Some((
                        text.get(body_at..).unwrap_or_default().trim().to_owned(),
                        card::marked_untrusted(text),
                    ));
                    (Run::Running, Some(window.as_str()))
                }
                Scheduled::Wait { .. } => (Run::Waiting, None),
                Scheduled::TakenOver { window, .. } => {
                    let window = window.as_deref();
                    let Some(keys) =
                        CardAgent::of_text(text).filter(|keys| unsettled(keys, window))
                    else {
                        skipped.set(Some(WINDOW_RAN));
                        return Ok(None);
                    };
                    (Run::Review, window.filter(|window| behind(&keys, window)))
                }
            };
            Ok(card::set_host_keys(text, run, last_run))
        },
        may_write,
        &mut |_| {},
    )?;
    if !wrote {
        return Ok(Begun::Nothing(skipped.get().unwrap_or(match scheduled {
            Scheduled::Wait { .. } => WAITS_ALREADY,
            _ => WINDOW_RAN,
        })));
    }
    Ok(match scheduled {
        Scheduled::Run { .. } => {
            let (brief, untrusted) = body.into_inner().unwrap_or_default();
            Begun::Run { brief, untrusted }
        }
        Scheduled::Wait { waiting, .. } => Begun::Said(RunBody {
            state: RunState::Waiting,
            detail: Some(waiting.clone()),
            step: None,
        }),
        Scheduled::TakenOver { host, .. } => Begun::Said(RunBody {
            state: RunState::Review,
            detail: Some(effect_unknown(host)),
            step: None,
        }),
    })
}

/// Whether a session file is markdown, which the stamp keeps: a card is
/// one, and so is a note an agent may retag as one.
fn is_markdown(landed: &str) -> bool {
    let name = landed
        .rsplit('/')
        .next()
        .unwrap_or(landed)
        .to_ascii_lowercase();
    name.ends_with(".md") || name.ends_with(".markdown")
}

/// One turn's `card_update` and `session_write`.
pub struct CardTools<'t> {
    pub from: Delegator,
    /// The readers of the home drive the session is in: the audience of
    /// every write these tools make.
    pub drive_readers: Readers,
    pub view: &'t dyn TurnView,
    pub allow: &'t [String],
    /// `agent:<agent>@<host>`: what a knowledge note this turn writes is
    /// generated by ([`knowledge::agent_actor`]).
    pub signer: String,
    /// The home drive's root, where its OKF type registry is.
    pub drive_root: PathBuf,
}

fn refused(reason: impl Into<String>) -> Option<ToolOutcome> {
    Some(ToolOutcome::Refused {
        reason: reason.into(),
    })
}

/// The keys `card_update` sets, checked and turned into frontmatter values,
/// or the sentence refusing the first one it may not.
fn checked_fields(fields: &Map<String, Value>) -> Result<Vec<(&str, FieldValue)>, String> {
    let text = |key: &str, value: &Value| {
        value
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| format!("{key} is a non-empty string."))
    };
    let mut out = Vec::new();
    for (key, value) in fields {
        let set = match key.as_str() {
            "status" => {
                let word = text(key, value)?;
                let status = TaskStatus::parse(&word).ok_or_else(|| {
                    format!("{word} is not one of the board's columns: in-preparation, todo, done or deferred. An agent's run is not a column.")
                })?;
                FieldValue::Str(status.as_str().to_owned())
            }
            "order" => match value.as_f64().filter(|n| n.is_finite()) {
                Some(n) => FieldValue::Num(n),
                None => return Err("order is a number.".to_owned()),
            },
            card::ASSIGNEE => {
                let id = text(key, value)?;
                if !keeper_core::agents::home::is_agent_id(&id) {
                    return Err(format!("{id} is not an agent id: lower-case letters, digits and dashes."));
                }
                FieldValue::Str(id)
            }
            card::HOST => {
                let slug = text(key, value)?;
                HostSlug::new(&slug).map_err(|_| format!("{slug} is not a host's name: lower-case letters, digits and dashes."))?;
                FieldValue::Str(slug)
            }
            card::SCHEDULE => {
                let schedule = text(key, value)?;
                TaskSchedule::parse(&schedule).map_err(|refusal| refusal.to_string())?;
                FieldValue::Str(schedule)
            }
            card::WORKFLOW => {
                text(key, value)?;
                continue;
            }
            card::RUN | card::LAST_RUN => return Err(format!("{key} {HOST_WRITES}")),
            card::SCHEDULED_BY | card::INTEGRITY => return Err(format!("{key} {KEEPER_WRITES}")),
            card::ALLOWED_BY => {
                return Err(format!("{key} is written when a person allows a schedule."))
            }
            card::REQUESTED_BY => {
                return Err("requested_by is set when a card is made; card_update changes a card that exists.".to_owned())
            }
            other => {
                return Err(format!(
                    "card_update sets status, order, assignee, host, schedule and workflow; {other} is none of them."
                ))
            }
        };
        out.push((key.as_str(), set));
    }
    Ok(out)
}

impl CardTools<'_> {
    /// Run `wire` when it is `card_update` or `session_write`; `None` for any
    /// other name.
    pub fn run(&self, wire: &WireToolCall) -> Option<ToolOutcome> {
        if !is_card_tool(&wire.name) {
            return None;
        }
        if !self.allow.contains(&wire.name) {
            return refused(format!("{} is not one of this agent's tools.", wire.name));
        }
        let args = wire.arguments.as_ref();
        let text = |key: &str| args.and_then(|args| args[key].as_str());
        if let SinkVerdict::Block { reason, .. } = check_sink(
            &self.view.label(),
            &Sink::DriveWrite {
                drive_readers: self.drive_readers.clone(),
            },
        ) {
            return refused(reason);
        }
        match wire.name.as_str() {
            CARD_UPDATE => {
                // The card it names is the file it changes and the file its
                // audit row names: no other key may name another.
                if let Some(other) = args.and_then(Value::as_object).and_then(|keys| {
                    keys.keys()
                        .find(|key| !["card", "fields"].contains(&key.as_str()))
                }) {
                    return refused(format!(
                        "card_update takes \"card\" and \"fields\"; {other} is neither."
                    ));
                }
                let (Some(rel), Some(fields)) = (
                    text("card"),
                    args.and_then(|args| args["fields"].as_object()),
                ) else {
                    return refused("card_update needs \"card\" and \"fields\" arguments.");
                };
                self.update(in_session(&self.session_dir(), rel), fields)
            }
            _ => {
                let (Some(rel), Some(content)) = (text("path"), text("content")) else {
                    return refused("session_write needs \"path\" and \"content\" arguments.");
                };
                self.write(in_session(&self.session_dir(), rel), content)
            }
        }
    }

    /// The session's folder, drive-relative: the spelling of a path the
    /// tools also take a session's file by ([`in_session`]).
    pub fn session_dir(&self) -> String {
        format!("{}/{}", self.from.subfolder, self.from.session)
    }

    fn update(&self, rel: &str, fields: &Map<String, Value>) -> Option<ToolOutcome> {
        let fields = match checked_fields(fields) {
            Ok(fields) if fields.is_empty() => {
                return refused("card_update was given no key to set.")
            }
            Ok(fields) => fields,
            Err(sentence) => return refused(sentence),
        };
        let integrity = self.view.label().integrity;
        let compose = |text: &str| {
            let read = read_one(PoolFile { rel, text });
            if read.kind != Some(KindTag::Task) {
                return Err(format!(
                    "{rel} is not a card: a card is a file tagged task."
                ));
            }
            let mut out = text.to_owned();
            for (key, value) in &fields {
                out = match (*key, value) {
                    ("order", FieldValue::Num(n)) => set_order_in(&out, *n),
                    ("status", value) => Frontmatter::set_in(&out, TASK_STATUS_KEY, value.clone()),
                    (key, value) => {
                        Frontmatter::set_after_in(&out, &card::KEYS, key, value.clone())
                    }
                };
            }
            let out = card::stamp_agent_write(Some(text), &out, &self.from.user, integrity);
            Ok((out != text).then_some(out))
        };
        let wrote = exec::hold(&self.from.zone)
            .map_err(VerbError::from)
            .and_then(|held| {
                rewrite(
                    &held,
                    &self.from.session,
                    rel,
                    "card-update",
                    compose,
                    &|| self.view.may_write(),
                    &mut |_| {},
                )
            });
        match wrote {
            Ok(true) => Some(ToolOutcome::Answered {
                text: format!("Updated {rel}."),
            }),
            Ok(false) => Some(ToolOutcome::Answered {
                text: format!("{rel} already says that; nothing was written."),
            }),
            Err(VerbError::Refused(sentence)) => refused(sentence),
            Err(error) => refused(format!("{rel} could not be updated: {error}")),
        }
    }

    fn write(&self, rel: &str, content: &str) -> Option<ToolOutcome> {
        let integrity = self.view.label().integrity;
        // Every markdown file, card or not: a note retagged later carries
        // what the stamp put on it (R119); a knowledge note then carries
        // what the host says of it (R140).
        let written = session_write_with(
            &self.from.zone,
            &self.from.session,
            rel,
            &|| self.view.may_write(),
            |landed, old| {
                let text = if is_markdown(landed) {
                    card::stamp_agent_write(old, content, &self.from.user, integrity)
                } else {
                    content.to_owned()
                };
                if knowledge::is_note(landed) {
                    self.harvested(landed, &text)
                        .map_err(|refusal| refusal.to_string())
                } else {
                    Ok(text)
                }
            },
        );
        match written {
            Ok(()) => Some(ToolOutcome::Answered {
                text: format!("Wrote {rel}."),
            }),
            Err(error) => refused(error.to_string()),
        }
    }

    /// `text`, a note landing at the session-relative `landed` under
    /// `artifacts/knowledge/`, as the host stores it (R140): stamped by
    /// [`knowledge::stamp`] against the closed session its folder names —
    /// found under the zone's `archive/` — and the drive's registry, every
    /// source a regular file of that session where its path says, through
    /// no link: one leading into another session of the zone is not its.
    fn harvested(&self, landed: &str, text: &str) -> Result<String, KnowledgeRefusal> {
        let slug = knowledge::source_slug(landed).ok_or(KnowledgeRefusal::NoFolder)?;
        let source_session = crate::sessions::scan::session_dirs(&self.from.zone)
            .into_iter()
            .filter(|dir| matches!(classify(dir), Some(SessionStatus::Archived(_))))
            .find(|dir| dir.rsplit('/').next() == Some(slug))
            .ok_or_else(|| KnowledgeRefusal::NoSuchSession {
                slug: slug.to_owned(),
            })?;
        let types = browse::resolve(&self.drive_root, knowledge::REGISTRY)
            .ok()
            .flatten()
            .and_then(|registry| std::fs::read_to_string(registry).ok())
            .map(|text| knowledge::registry_types(&text))
            .unwrap_or_default();
        let at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let stamped = knowledge::stamp(
            text,
            &knowledge::Harvest {
                actor: &self.signer,
                at: &at,
                types: &types,
                sessions_subfolder: &self.from.subfolder,
                source_session: &source_session,
            },
        )?;
        for (resource, file) in &stamped.sources {
            let there = browse::landing(&self.from.zone, file)
                .is_ok_and(|landed| landed.iter().map(String::as_str).eq(file.split('/')))
                && browse::lexical_join(&self.from.zone, file)
                    .is_ok_and(|path| std::fs::metadata(path).is_ok_and(|meta| meta.is_file()));
            if !there {
                return Err(KnowledgeRefusal::SourceMissing {
                    resource: resource.clone(),
                    session: source_session,
                });
            }
        }
        Ok(stamped.text)
    }
}

#[cfg(test)]
mod tests {
    use keeper_core::agents::delegation::CARD_FILE;
    use keeper_core::agents::home;
    use keeper_core::agents::label::{Integrity, Label};
    use keeper_core::agents::session::SessionKind;
    use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId};

    use super::*;
    use crate::delegate::Delegation;

    const SESSION: &str = "active/2026-10-04-tola";
    const CARD: &str = "---\ntags: [task]\ntitle: Sort the inbox\nstatus: todo\norder: 1\nassignee: tola\nrequested_by: \"@nixi:h\"\nrun: queued\n---\n\nSort what came in.\n";

    struct View(Label, std::sync::atomic::AtomicBool);

    impl TurnView for View {
        fn label(&self) -> Label {
            self.0.clone()
        }
        fn delegation(&self, _: &str) -> Option<Delegation> {
            None
        }
        fn handed(&self, _: &str) -> Option<Delegation> {
            None
        }
        fn may_write(&self) -> bool {
            self.1.load(std::sync::atomic::Ordering::SeqCst)
        }
        fn relay(&self, _: &str) -> Option<crate::ask::Relay> {
            None
        }
        fn ended(&self) -> bool {
            false
        }
        fn turn_spend(&self) -> u64 {
            0
        }
        fn session_budget(&self) -> Option<(u64, u64)> {
            None
        }
    }

    fn view(integrity: Integrity) -> View {
        View(label(integrity), std::sync::atomic::AtomicBool::new(true))
    }

    fn tgorka() -> OwnedUserId {
        OwnedUserId::try_from("@tgorka:h").expect("user")
    }

    fn label(integrity: Integrity) -> Label {
        Label {
            readers: Readers::Only([tgorka()].into_iter().collect()),
            integrity,
            local_only: false,
        }
    }

    /// A zone holding one delegated session with its card.
    fn zone() -> tempfile::TempDir {
        let zone = tempfile::tempdir().expect("zone");
        let session = zone.path().join(SESSION);
        std::fs::create_dir_all(&session).expect("session");
        std::fs::write(session.join(CARD_FILE), CARD).expect("card");
        zone
    }

    fn card_text(zone: &Path) -> String {
        std::fs::read_to_string(zone.join(SESSION).join(CARD_FILE)).expect("card")
    }

    fn tools<'t>(zone: &Path, view: &'t View, allow: &'t [String]) -> CardTools<'t> {
        CardTools {
            from: Delegator {
                user: OwnedUserId::try_from("@tola:h").expect("user"),
                drive: "tgdrive".to_owned(),
                id: "01J5AAAAAAAAAAAAAAAAAAAAAA".to_owned(),
                session: SESSION.to_owned(),
                room: OwnedRoomId::try_from("!r:h").expect("room"),
                kind: SessionKind::Delegated,
                requester: tgorka(),
                hop: 1,
                limits: home::Limits {
                    rounds_per_turn: 8,
                    tokens_per_turn: 0,
                    tokens_per_delegation: 200_000,
                    hop_limit: 3,
                    rounds_per_exchange: 3,
                    max_concurrent_sessions: 1,
                },
                zone: zone.to_owned(),
                subfolder: "60-sessions".to_owned(),
                chain: Vec::new(),
            },
            drive_readers: Readers::Only([tgorka()].into_iter().collect()),
            view,
            allow,
            signer: knowledge::agent_actor("tola-grey", "electra"),
            drive_root: zone.to_owned(),
        }
    }

    fn call(name: &str, args: Value) -> WireToolCall {
        WireToolCall {
            id: "c1".to_owned(),
            name: name.to_owned(),
            arguments_raw: args.to_string(),
            arguments: Some(args),
        }
    }

    fn allow() -> Vec<String> {
        vec![CARD_UPDATE.to_owned(), SESSION_WRITE.to_owned()]
    }

    fn refusal(outcome: Option<ToolOutcome>) -> String {
        match outcome {
            Some(ToolOutcome::Refused { reason }) => reason,
            other => panic!("not refused: {other:?}"),
        }
    }

    /// AC4: `run`, `last_run`, `scheduled_by` and `integrity` are each
    /// refused with their sentence, and the card's bytes stay as they were;
    /// a column is set as one key.
    #[test]
    fn card_update_refuses_the_keys_keeper_writes() {
        let zone = zone();
        let view = view(Integrity::Agent);
        let allow = allow();
        let tools = tools(zone.path(), &view, &allow);
        let update = |fields: Value| {
            tools.run(&call(
                CARD_UPDATE,
                json!({"card": CARD_FILE, "fields": fields}),
            ))
        };
        for (key, sentence) in [
            ("run", HOST_WRITES),
            ("last_run", HOST_WRITES),
            ("scheduled_by", KEEPER_WRITES),
            ("integrity", KEEPER_WRITES),
        ] {
            assert_eq!(
                refusal(update(json!({ key: "x" }))),
                format!("{key} {sentence}")
            );
        }
        assert!(refusal(update(json!({"requested_by": "@marta:h"}))).contains("requested_by"));
        assert!(refusal(update(json!({"status": "blocked"})))
            .contains("not one of the board's columns"));
        assert_eq!(card_text(zone.path()), CARD, "nothing was written");

        assert!(matches!(
            update(json!({"status": "done", "order": 2.5})),
            Some(ToolOutcome::Answered { .. })
        ));
        assert_eq!(
            card_text(zone.path()),
            CARD.replace("status: todo", "status: done")
                .replace("order: 1\n", "order: 2.5\n")
        );
    }

    /// R52: `card_update` passes the stamp too — a session at `untrusted`
    /// integrity marks the card it moves.
    #[test]
    fn a_card_update_from_outside_content_marks_the_card() {
        let zone = zone();
        let view = view(Integrity::Untrusted);
        let allow = allow();
        let tools = tools(zone.path(), &view, &allow);
        tools.run(&call(
            CARD_UPDATE,
            json!({"card": CARD_FILE, "fields": {"status": "done"}}),
        ));
        let text = card_text(zone.path());
        assert!(text.contains("status: done"), "{text}");
        assert_eq!(
            Frontmatter::parse(&text).0.as_string(card::INTEGRITY),
            Some("untrusted")
        );
    }

    /// AC5: a schedule is checked where it is written; one that does not
    /// read writes nothing. Whether a readable one may be set at all is its
    /// tier's (T3, AD-392), decided before this tool runs.
    #[test]
    fn the_schedule_is_checked_where_it_is_written() {
        let zone = zone();
        let view = view(Integrity::Agent);
        let allow = allow();
        let tools = tools(zone.path(), &view, &allow);
        let update = |fields: Value| {
            refusal(tools.run(&call(
                CARD_UPDATE,
                json!({"card": CARD_FILE, "fields": fields}),
            )))
        };
        assert!(update(json!({"schedule": "every 30s"})).contains("more often than once a minute"));
        assert!(update(json!({"schedule": "0 0 30 2 *"})).contains("matches no instant"));
        assert_eq!(card_text(zone.path()), CARD);
    }

    /// AC4, R120: the host writes `run:` only on a transition, one key, every
    /// other byte kept; a card changed under the write — even by an edit of
    /// the same length — is read again and written once more, and a second
    /// change is reported.
    #[test]
    fn the_host_writes_run_only_on_a_transition() {
        let zone = zone();
        assert!(
            !write_run(zone.path(), SESSION, CARD_FILE, Run::Queued, None, &|| true)
                .expect("queued")
        );
        assert_eq!(
            card_text(zone.path()),
            CARD,
            "running → running writes nothing"
        );
        assert!(
            write_run(zone.path(), SESSION, CARD_FILE, Run::Running, None, &|| {
                true
            })
            .expect("running")
        );
        assert_eq!(
            card_text(zone.path()),
            CARD.replace("run: queued", "run: running")
        );

        // A person moves the card between the read and the write.
        let held = exec::hold(zone.path()).expect("hold");
        let mut moves = 0;
        let wrote = rewrite(
            &held,
            SESSION,
            CARD_FILE,
            "card-run",
            |text| Ok(card::set_host_keys(text, Run::Review, None)),
            &|| true,
            &mut |file| {
                moves += 1;
                if moves == 1 {
                    let text = std::fs::read_to_string(file).expect("read");
                    std::fs::write(file, text.replace("status: todo", "status: done"))
                        .expect("move");
                }
            },
        )
        .expect("retried once");
        assert!(wrote);
        assert_eq!(moves, 2, "read again after the move");
        let text =
            std::fs::read_to_string(zone.path().join(SESSION).join(CARD_FILE)).expect("card");
        assert!(
            text.contains("status: done") && text.contains("run: review"),
            "{text}"
        );

        // A card that keeps changing is reported after the second try.
        let mut grows = 0;
        let error = rewrite(
            &held,
            SESSION,
            CARD_FILE,
            "card-run",
            |text| Ok(card::set_host_keys(text, Run::Failed, None)),
            &|| true,
            &mut |file| {
                grows += 1;
                let text = std::fs::read_to_string(file).expect("read");
                std::fs::write(file, format!("{text}more\n")).expect("grow");
            },
        )
        .expect_err("reported");
        assert_eq!(grows, 2);
        assert!(
            matches!(error, VerbError::Exec(ExecError::Refused(_))),
            "{error:?}"
        );
        drop(held);
        assert!(!card_text(zone.path()).contains("run: failed"));
    }

    /// AC10 (S-21): a `session_write` of a card that sets `schedule:` stores
    /// `scheduled_by: <the agent>` whatever it wrote there; a later rewrite
    /// dropping `scheduled_by` or `integrity` stores them again; a write that
    /// changes neither adds no mark; a session at `untrusted` integrity marks
    /// the card it writes.
    #[test]
    fn an_agents_schedule_is_stamped_and_never_unstamped_by_an_agent() {
        let zone = zone();
        let allow = allow();
        let agent = view(Integrity::Agent);
        let mine = tools(zone.path(), &agent, &allow);
        let write = |tools: &CardTools<'_>, rel: &str, content: &str| {
            tools.run(&call(
                SESSION_WRITE,
                json!({"path": rel, "content": content}),
            ))
        };
        let field = |rel: &str, key: &str| {
            let text = std::fs::read_to_string(zone.path().join(SESSION).join(rel)).expect("file");
            Frontmatter::parse(&text)
                .0
                .as_string(key)
                .map(str::to_owned)
        };
        let scheduled = CARD.replace(
            "run: queued\n",
            "schedule: \"@daily\"\nscheduled_by: \"@tgorka:h\"\n",
        );
        assert!(matches!(
            write(&mine, "cards/daily.md", &scheduled),
            Some(ToolOutcome::Answered { .. })
        ));
        assert_eq!(
            field("cards/daily.md", card::SCHEDULED_BY).as_deref(),
            Some("@tola:h")
        );

        let dropped = scheduled.replace("scheduled_by: \"@tgorka:h\"\n", "");
        write(&mine, "cards/daily.md", &dropped);
        assert_eq!(
            field("cards/daily.md", card::SCHEDULED_BY).as_deref(),
            Some("@tola:h")
        );

        write(&mine, "plain.md", CARD);
        assert_eq!(
            field("plain.md", card::SCHEDULED_BY),
            None,
            "no schedule, no mark"
        );

        let outside = view(Integrity::Untrusted);
        let untrusted = tools(zone.path(), &outside, &allow);
        write(&untrusted, "from-inbox.md", CARD);
        assert_eq!(
            field("from-inbox.md", card::INTEGRITY).as_deref(),
            Some("untrusted")
        );
        write(&mine, "from-inbox.md", CARD);
        assert_eq!(
            field("from-inbox.md", card::INTEGRITY).as_deref(),
            Some("untrusted"),
            "an agent's rewrite keeps the mark"
        );

        assert!(refusal(write(&mine, "log/x.md", CARD)).contains("keeper's own record"));
    }

    /// A tool `[tools].allow` does not name is refused, and nothing is
    /// written; a label beyond the drive's readers writes nothing.
    #[test]
    fn a_card_tool_needs_the_allow_and_the_drives_readers() {
        let zone = zone();
        let view = view(Integrity::Agent);
        let none: Vec<String> = Vec::new();
        let unlisted = tools(zone.path(), &view, &none);
        assert!(refusal(unlisted.run(&call(
            SESSION_WRITE,
            json!({"path": "x.md", "content": "x"})
        )))
        .contains("not one of this agent's tools"));

        let allow = allow();
        let mut narrow_drive = tools(zone.path(), &view, &allow);
        narrow_drive.drive_readers = Readers::Only(
            [tgorka(), OwnedUserId::try_from("@marta:h").expect("user")]
                .into_iter()
                .collect(),
        );
        refusal(narrow_drive.run(&call(
            SESSION_WRITE,
            json!({"path": "x.md", "content": "x"}),
        )));
        assert!(!zone.path().join(SESSION).join("x.md").exists());
    }

    /// R119 (R4-01): a folder link is followed to where it lands, and the
    /// fence is asked there — a link in `workspace/` back into the session
    /// cannot reach `agent.toml` or the log, and a link to another session's
    /// cards cannot carry a `card_update` into it.
    #[test]
    fn a_folder_link_never_carries_a_write_out_of_the_fence() {
        let zone = zone();
        let session = zone.path().join(SESSION);
        std::fs::write(session.join("agent.toml"), "kept\n").expect("agent.toml");
        std::fs::create_dir_all(session.join("workspace")).expect("workspace");
        std::os::unix::fs::symlink("..", session.join("workspace/back")).expect("link");
        let other = zone.path().join("active/2026-10-04-other/cards");
        std::fs::create_dir_all(&other).expect("other");
        std::fs::write(other.join("task.md"), CARD).expect("their card");
        std::os::unix::fs::symlink("../2026-10-04-other/cards", session.join("cards"))
            .expect("link");
        let view = view(Integrity::Agent);
        let allow = allow();
        let tools = tools(zone.path(), &view, &allow);
        for path in ["workspace/back/agent.toml", "workspace/back/log/x.md"] {
            refusal(tools.run(&call(
                SESSION_WRITE,
                json!({"path": path, "content": "taken"}),
            )));
        }
        assert_eq!(
            std::fs::read_to_string(session.join("agent.toml")).expect("read"),
            "kept\n"
        );
        assert!(!session.join("log").exists());
        refusal(tools.run(&call(
            CARD_UPDATE,
            json!({"card": "cards/task.md", "fields": {"status": "done"}}),
        )));
        refusal(tools.run(&call(
            SESSION_WRITE,
            json!({"path": "cards/task.md", "content": "taken"}),
        )));
        assert_eq!(
            std::fs::read_to_string(other.join("task.md")).expect("read"),
            CARD
        );
    }

    /// R119 (R4-02): keeper's own names are refused however they are
    /// spelled, as the Mac's volume reads them alike.
    #[test]
    fn keepers_names_are_refused_in_any_case() {
        let zone = zone();
        let view = view(Integrity::Agent);
        let allow = allow();
        let tools = tools(zone.path(), &view, &allow);
        for path in [
            "readme.md",
            "Agents.md",
            "Approvals/x.json",
            "Log/blobs/x.json",
            "LOG/x.md",
        ] {
            assert!(
                refusal(tools.run(&call(SESSION_WRITE, json!({"path": path, "content": "{}"}),)))
                    .contains("keeper's own record"),
                "{path}"
            );
        }
        let entries: Vec<_> = std::fs::read_dir(zone.path().join(SESSION))
            .expect("session")
            .flatten()
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(entries, [std::ffi::OsString::from(CARD_FILE)]);
    }

    /// R112: `session_write` puts BMAD's output kinds into `artifacts/` —
    /// `sprint-status.yaml`, a TOML, a text or an HTML file — and keeps
    /// the session's own pool to markdown, csv and json; and the memlog's
    /// dotted name stays `bmad_memlog`'s alone.
    #[test]
    fn session_write_takes_bmads_outputs_under_artifacts() {
        let zone = zone();
        let view = view(Integrity::Agent);
        let allow = allow();
        let tools = tools(zone.path(), &view, &allow);
        let write = |rel: &str| {
            tools.run(&call(
                SESSION_WRITE,
                json!({"path": rel, "content": "development_status: {}\n"}),
            ))
        };
        let session = zone.path().join(SESSION);
        for rel in [
            "artifacts/sprint-status.yaml",
            "artifacts/_bmad-output/tea-progress.yml",
            "artifacts/report.toml",
            "artifacts/notes.txt",
            "artifacts/site/index.html",
        ] {
            assert!(
                matches!(write(rel), Some(ToolOutcome::Answered { .. })),
                "{rel}"
            );
            assert!(session.join(rel).is_file(), "{rel}");
        }
        assert!(refusal(write("artifacts/shot.png")).contains("is none of those"));
        assert!(refusal(write("sprint-status.yaml")).contains("is none of those"));
        assert!(refusal(write("artifacts/run/.memlog.md")).contains("not a plain path"));
        assert!(!session.join("artifacts/shot.png").exists());
        assert!(!session.join("sprint-status.yaml").exists());
        assert!(!session.join("artifacts/run").exists());
    }

    /// R94R-05: a write location as `bmad_config` and `bmad_render` name it
    /// — from the drive's root, through this session's folder — is the
    /// session's file, Markdown and YAML alike: written where it names,
    /// never under a second copy of the session's path.
    #[test]
    fn session_write_takes_the_write_locations_bmad_names() {
        let zone = zone();
        let view = view(Integrity::Agent);
        let allow = allow();
        let tools = tools(zone.path(), &view, &allow);
        let session = zone.path().join(SESSION);
        for rel in [
            "artifacts/_bmad-output/implementation-artifacts/spec-x.md",
            "artifacts/_bmad-output/implementation-artifacts/sprint-status.yaml",
        ] {
            let named = format!("60-sessions/{SESSION}/{rel}");
            let outcome = tools.run(&call(
                SESSION_WRITE,
                json!({"path": named, "content": "status: done\n"}),
            ));
            assert!(
                matches!(outcome, Some(ToolOutcome::Answered { .. })),
                "{rel}: {outcome:?}"
            );
            assert!(session.join(rel).is_file(), "{rel}");
        }
        assert!(
            !session.join("60-sessions").exists(),
            "no second session path"
        );
    }

    /// R119 (R4-03, R4-04): through the tool, a rewrite cannot change the
    /// host's `run:`, and a note staged with a schedule and a forged
    /// `allowed_by:`, then retagged as a card, reaches the board stamped as
    /// the agent's and with no person's tick.
    #[test]
    fn the_tools_never_launder_a_card() {
        let zone = zone();
        let view = view(Integrity::Agent);
        let allow = allow();
        let tools = tools(zone.path(), &view, &allow);
        let write = |rel: &str, content: &str| {
            tools.run(&call(
                SESSION_WRITE,
                json!({"path": rel, "content": content}),
            ))
        };
        write(CARD_FILE, &CARD.replace("run: queued", "run: review"));
        assert_eq!(card_text(zone.path()), CARD, "run is the host's");

        let note = "---\ntags: [note]\ntitle: Daily\nschedule: \"@daily\"\nallowed_by: \"@tgorka:h\"\n---\n\nRun daily.\n";
        write("daily.md", note);
        let staged =
            std::fs::read_to_string(zone.path().join(SESSION).join("daily.md")).expect("note");
        write("daily.md", &staged.replace("[note]", "[task]"));
        let card =
            std::fs::read_to_string(zone.path().join(SESSION).join("daily.md")).expect("card");
        let (fm, _) = Frontmatter::parse(&card);
        assert_eq!(fm.as_string(card::SCHEDULED_BY), Some("@tola:h"), "{card}");
        assert_eq!(fm.count(card::ALLOWED_BY), 0, "{card}");

        // A card's mark survives being retagged a note, stripped there, and
        // retagged a card again.
        let marked = CARD.replace("run: queued\n", "integrity: untrusted\n");
        std::fs::write(zone.path().join(SESSION).join("marked.md"), &marked).expect("card");
        let note = marked.replace("[task]", "[note]");
        write("marked.md", &note);
        write("marked.md", &note.replace("integrity: untrusted\n", ""));
        write(
            "marked.md",
            &note
                .replace("integrity: untrusted\n", "")
                .replace("[note]", "[task]"),
        );
        let back =
            std::fs::read_to_string(zone.path().join(SESSION).join("marked.md")).expect("card");
        assert!(card::marked_untrusted(&back), "{back}");
    }

    /// R120 (R4-10): a write that waited for the zone while its host lost
    /// the session's claim writes nothing — the agent's tool and the host's
    /// run writer alike.
    #[test]
    fn a_write_without_the_claim_has_no_effect() {
        let zone = zone();
        let view = view(Integrity::Agent);
        let allow = allow();
        let held = exec::hold(zone.path()).expect("hold");
        std::thread::scope(|scope| {
            let waiting = scope.spawn(|| {
                tools(zone.path(), &view, &allow).run(&call(
                    CARD_UPDATE,
                    json!({"card": CARD_FILE, "fields": {"status": "done"}}),
                ))
            });
            std::thread::sleep(std::time::Duration::from_millis(200));
            view.1.store(false, std::sync::atomic::Ordering::SeqCst);
            drop(held);
            assert_eq!(refusal(waiting.join().expect("tool")), NO_CLAIM);
        });
        assert_eq!(card_text(zone.path()), CARD);
        let lost = write_run(zone.path(), SESSION, CARD_FILE, Run::Review, None, &|| {
            false
        });
        assert!(matches!(lost, Err(VerbError::Refused(sentence)) if sentence == NO_CLAIM));
        assert_eq!(card_text(zone.path()), CARD);
        let written =
            crate::sessions::write::session_write(zone.path(), SESSION, "x.md", "x\n", &|| false);
        assert!(matches!(written, Err(VerbError::Refused(sentence)) if sentence == NO_CLAIM));
        assert!(!zone.path().join(SESSION).join("x.md").exists());
    }

    /// R122 (R4-14): a person's move on one copy and the host's first
    /// `run:` on another merge line by line, through the `git merge -X
    /// theirs` keeper-sync's engine converges with, keeping both edits — on
    /// a card whose agent keys sit beside `status:`/`order:`, and on one
    /// with none. Had the two edits touched, `-X theirs` would have dropped
    /// one side's.
    #[test]
    fn a_move_and_a_run_merge_through_gits_own_merge() {
        use keeper_core::sessions::tasks::{compile_move, TaskFile};
        use keeper_sync::git::cli::MergeOutcome;
        use keeper_sync::git::GitCli;

        let git = |repo: &Path, args: &[&str]| {
            let status = std::process::Command::new("git")
                .current_dir(repo)
                .args([
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@h",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .output()
                .expect("git");
            assert!(status.status.success(), "git {args:?}: {status:?}");
        };
        for base in [
            "---\ntags: [task]\nassignee: tola-grey\nstatus: todo\norder: 1\n---\n\nBody.\n",
            "---\ntags: [task]\ntitle: Sort\nstatus: todo\norder: 1\n---\n\nBody.\n",
        ] {
            let repo = tempfile::tempdir().expect("repo");
            let repo = repo.path();
            git(repo, &["init", "-q", "-b", "main"]);
            std::fs::write(repo.join("card.md"), base).expect("card");
            git(repo, &["add", "card.md"]);
            git(repo, &["commit", "-q", "-m", "base"]);
            git(repo, &["checkout", "-q", "-b", "host"]);
            let ran = card::set_host_keys(base, Run::Running, Some("2026-10-04T09:00:00+02:00"))
                .expect("a transition");
            std::fs::write(repo.join("card.md"), &ran).expect("run");
            git(repo, &["commit", "-q", "-am", "run"]);
            git(repo, &["checkout", "-q", "main"]);
            let other = "---\ntags: [task]\nstatus: done\norder: 1\n---\n";
            let plan = compile_move(
                "active/s",
                "card.md",
                base,
                TaskStatus::Done,
                &[TaskFile {
                    rel: "other.md",
                    text: other,
                    order: 1.0,
                }],
                1,
            )
            .expect("a move");
            let Some(PlanStep::GuardedWrite { content: moved, .. }) = plan.steps.last() else {
                panic!("the moved card is written last");
            };
            std::fs::write(repo.join("card.md"), moved).expect("move");
            git(repo, &["commit", "-q", "-am", "move"]);

            let merged = GitCli::new("git".into())
                .merge_theirs(repo, "host", "merge")
                .expect("merge");
            assert_eq!(merged, MergeOutcome::Clean);
            let text = std::fs::read_to_string(repo.join("card.md")).expect("merged");
            let (fm, _) = Frontmatter::parse(&text);
            assert_eq!(fm.as_string(TASK_STATUS_KEY), Some("done"), "{text}");
            assert_eq!(fm.get("order"), Some(&FieldValue::Num(2.0)), "{text}");
            assert_eq!(fm.as_string(card::RUN), Some("running"), "{text}");
            assert!(fm.as_string(card::LAST_RUN).is_some(), "{text}");
        }
    }

    /// `text`'s instant, epoch ms.
    fn at(text: &str) -> i64 {
        chrono::DateTime::parse_from_rfc3339(text)
            .expect("an instant")
            .timestamp_millis()
    }

    /// A card for Nixi with `keys` among its frontmatter.
    fn scheduled(keys: &str) -> CardAgent {
        CardAgent::of_text(&format!(
            "---\ntags: [task]\ntitle: Sort\nstatus: todo\nassignee: nixi\n{keys}---\n\nSort.\n"
        ))
        .expect("agent keys")
    }

    /// `due` of a card with `keys` at `now`, at UTC.
    fn due_at(keys: &str, now: &str) -> Due {
        due(&scheduled(keys), at(now), 0)
    }

    fn window(text: &str) -> Due {
        Due::Due {
            window_ms: at(text),
        }
    }

    /// 92.3 AC1: keeper-sync's dialect decides, after `last_run`; a card that
    /// never ran is due at once in its latest window; an unreadable schedule
    /// (the 60 s floor included) or `last_run` never runs; an agent's
    /// schedule waits for a person's tick.
    #[test]
    fn a_card_is_due_by_keepers_own_dialect() {
        let daily = "schedule: \"@daily\"\nlast_run: \"2026-10-04T00:00:00Z\"\n";
        assert_eq!(due_at(daily, "2026-10-04T23:59:59Z"), Due::NotDue);
        assert_eq!(
            due_at(daily, "2026-10-05T00:00:00Z"),
            window("2026-10-05T00:00:00Z")
        );
        // Friday's run; nothing at the weekend; Monday at nine.
        let weekdays = "schedule: \"0 9 * * 1-5\"\nlast_run: \"2026-10-02T09:00:00Z\"\n";
        assert_eq!(due_at(weekdays, "2026-10-03T12:00:00Z"), Due::NotDue);
        assert_eq!(due_at(weekdays, "2026-10-05T08:59:00Z"), Due::NotDue);
        assert_eq!(
            due_at(weekdays, "2026-10-05T09:00:00Z"),
            window("2026-10-05T09:00:00Z")
        );
        let every = "schedule: every 2h\nlast_run: \"2026-10-05T09:00:00Z\"\n";
        assert_eq!(due_at(every, "2026-10-05T10:59:59Z"), Due::NotDue);
        assert_eq!(
            due_at(every, "2026-10-05T11:00:00Z"),
            window("2026-10-05T11:00:00Z")
        );
        // The offset is the machine's: nine o'clock at +02:00 is seven UTC.
        assert_eq!(
            due(
                &scheduled("schedule: \"0 9 * * *\"\nlast_run: \"2026-10-04T07:00:00Z\"\n"),
                at("2026-10-05T07:00:00Z"),
                120
            ),
            window("2026-10-05T07:00:00Z")
        );

        // Never ran: its latest window, or the minute an `every` is seen in.
        assert_eq!(
            due_at("schedule: \"@daily\"\n", "2026-10-05T09:30:00Z"),
            window("2026-10-05T00:00:00Z")
        );
        assert_eq!(
            due_at("schedule: every 2h\n", "2026-10-05T09:30:42Z"),
            window("2026-10-05T09:30:00Z")
        );

        assert_eq!(
            due_at("schedule: every 30s\n", "2026-10-05T09:30:00Z"),
            Due::Unreadable,
            "under the 60 s floor"
        );
        assert_eq!(
            due_at("schedule: every 1m\n", "2026-10-05T09:30:00Z"),
            window("2026-10-05T09:30:00Z")
        );
        assert_eq!(
            due_at("schedule: \"0 0 30 2 *\"\n", "2026-10-05T09:30:00Z"),
            Due::Unreadable
        );
        assert_eq!(
            due_at(
                "schedule: \"@daily\"\nlast_run: yesterday\n",
                "2026-10-05T09:30:00Z"
            ),
            Due::Unreadable
        );
        assert_eq!(
            due_at(
                "schedule: \"@daily\"\nscheduled_by: \"@tola:h\"\n",
                "2026-10-05T09:30:00Z"
            ),
            Due::Unticked
        );
        assert_eq!(
            due_at("run: queued\n", "2026-10-05T09:30:00Z"),
            Due::Unscheduled
        );
    }

    /// 92.3 AC4 (DW-376): an `@hourly` card whose host was away five hours
    /// runs once on return, in the latest window; its `last_run` then makes
    /// it due only at the next one. A year away is one window too.
    #[test]
    fn missed_windows_run_once_on_return() {
        let away = "schedule: \"@hourly\"\nlast_run: \"2026-10-05T04:00:00Z\"\n";
        assert_eq!(
            due_at(away, "2026-10-05T09:30:00Z"),
            window("2026-10-05T09:00:00Z")
        );
        let ran = "schedule: \"@hourly\"\nlast_run: \"2026-10-05T09:00:00Z\"\n";
        assert_eq!(due_at(ran, "2026-10-05T09:59:59Z"), Due::NotDue);
        assert_eq!(
            due_at(ran, "2026-10-05T10:00:00Z"),
            window("2026-10-05T10:00:00Z")
        );
        let every = "schedule: every 1h\nlast_run: \"2026-10-05T04:00:00Z\"\n";
        assert_eq!(
            due_at(every, "2026-10-05T09:30:00Z"),
            window("2026-10-05T09:00:00Z")
        );
        let year = "schedule: \"*/5 * * * *\"\nlast_run: \"2025-10-05T09:00:00Z\"\n";
        assert_eq!(
            due_at(year, "2026-10-05T09:32:10Z"),
            window("2026-10-05T09:30:00Z")
        );
        // Three windows inside the span the look-back finds: the latest.
        let thrice = "schedule: \"0,1,2 9 * * *\"\nlast_run: \"2026-10-04T09:02:00Z\"\n";
        assert_eq!(
            due_at(thrice, "2026-10-05T09:30:00Z"),
            window("2026-10-05T09:02:00Z")
        );
    }

    fn session_of(agent: &str, kind: SessionKind) -> SessionAgent {
        let decl = keeper_core::agents::drive::parse(
            "version = 1\nid = \"tgdrive\"\ntitle = \"tgdrive\"\nprincipal = \"tgorka\"\nowner = \"@tgorka:h\"\nreaders = [\"@tgorka:h\"]\n",
        )
        .expect("decl");
        SessionAgent {
            id: ulid::Ulid::new(),
            agent: agent.to_owned(),
            drive: "tgdrive".to_owned(),
            kind,
            title: "schedule".to_owned(),
            requested_by: tgorka(),
            parent: None,
            room: OwnedRoomId::try_from("!r:h").expect("room"),
            drives: vec!["tgdrive".to_owned()],
            label: Label::opening(&decl, Integrity::Owner),
            needs: None,
            pin: None,
            hop: 0,
            dispatch_chain: Vec::new(),
            limits: None,
            workflow: None,
            checkpoints: None,
            outputs: Vec::new(),
            created_at: chrono::Utc::now(),
        }
    }

    /// 92.3 AC5 (Q9, R57): a scheduled card runs only alone in a scheduled
    /// session of its assignee, and says why anywhere else; a scan that
    /// could not read every card says so, and runs none.
    #[test]
    fn a_scheduled_card_runs_only_in_its_assignees_session() {
        let card = |keys: &str| ScheduledCard {
            rel: "card.md".to_owned(),
            card: scheduled(&format!("schedule: \"@daily\"\n{keys}")),
        };
        let scan = |cards: Vec<ScheduledCard>| ScheduledScan {
            cards,
            incomplete: None,
        };
        let mine = session_of("nixi", SessionKind::Scheduled);
        let one = scan(vec![card("")]);
        assert_eq!(session_schedule(Some(&mine), &one), Ok(Some(&one.cards[0])));
        assert_eq!(session_schedule(Some(&mine), &scan(Vec::new())), Ok(None));
        let elsewhere = "runs only in nixi's session".to_owned();
        assert_eq!(
            session_schedule(Some(&session_of("tola", SessionKind::Scheduled)), &one),
            Err(elsewhere.clone())
        );
        assert_eq!(session_schedule(None, &one), Err(elsewhere));
        assert_eq!(
            session_schedule(Some(&mine), &scan(vec![card(""), card("")])),
            Err("runs only alone in a scheduled session of nixi".to_owned())
        );
        let pinned = scan(vec![card("host: Hesperia!\n")]);
        assert!(session_schedule(Some(&mine), &pinned)
            .expect_err("an unreadable pin")
            .contains("Hesperia!"));
        let partial = ScheduledScan {
            incomplete: Some("a/b.md: unreadable.".to_owned()),
            ..one.clone()
        };
        assert!(session_schedule(Some(&mine), &partial)
            .expect_err("a partial scan")
            .contains("a/b.md: unreadable."));
    }

    /// R57: the scan reads a bounded prefix of each file — a
    /// large note whose frontmatter names no schedule costs that prefix and
    /// hides nothing — and a set it could not read whole is never taken for
    /// the whole: a scheduled card too long to read, a file that does not
    /// read, or a second card past the walk's budget leaves it incomplete.
    #[test]
    fn a_scan_that_misses_a_card_is_never_taken_for_the_whole() {
        let dir = tempfile::tempdir().expect("session");
        let card =
            "---\ntags: [task]\ntitle: Sort\nassignee: nixi\nschedule: \"@daily\"\n---\n\nSort.\n";
        std::fs::write(dir.path().join("card.md"), card).expect("card");
        let mut note = "---\ntags: [note]\ntitle: Archive\n---\n\n".to_owned();
        note.push_str(&"x".repeat(5 * 1024 * 1024));
        std::fs::write(dir.path().join("archive.md"), &note).expect("note");
        let found = scheduled_cards("s", dir.path());
        assert_eq!(found.incomplete, None, "{found:?}");
        assert_eq!(found.cards.len(), 1);
        assert_eq!(found.cards[0].rel, "card.md");

        let long = format!("{}{}", card, "y".repeat(SCAN_FILE_BYTES as usize));
        std::fs::write(dir.path().join("long.md"), long).expect("long");
        let found = scheduled_cards("s", dir.path());
        assert!(
            found
                .incomplete
                .as_deref()
                .is_some_and(|why| why.contains("s/long.md")),
            "{found:?}"
        );
        std::fs::remove_file(dir.path().join("long.md")).expect("gone");

        std::fs::write(dir.path().join("bytes.md"), [0xff, 0xfe, 0x00]).expect("bytes");
        let found = scheduled_cards("s", dir.path());
        assert!(
            found
                .incomplete
                .as_deref()
                .is_some_and(|why| why.contains("s/bytes.md")),
            "{found:?}"
        );
        std::fs::remove_file(dir.path().join("bytes.md")).expect("gone");

        for n in 0..2_000 {
            std::fs::write(dir.path().join(format!("n{n:04}.md")), "").expect("note");
        }
        std::fs::write(dir.path().join("z-second.md"), card).expect("second");
        let found = scheduled_cards("s", dir.path());
        assert!(found.incomplete.is_some(), "{found:?}");
        assert!(
            session_schedule(Some(&session_of("nixi", SessionKind::Scheduled)), &found).is_err()
        );
    }

    /// R58: a leap-day card that never ran takes its latest window, by
    /// the horizon keeper-sync's dialect allows — four years back, or eight
    /// across 2100, which is no leap year.
    #[test]
    fn a_leap_day_card_that_never_ran_takes_its_latest_window() {
        let leap = "schedule: \"0 0 29 2 *\"\n";
        assert_eq!(
            due_at(leap, "2026-10-05T09:30:00Z"),
            window("2024-02-29T00:00:00Z")
        );
        assert_eq!(
            due_at(leap, "2103-06-01T00:00:00Z"),
            window("2096-02-29T00:00:00Z")
        );
        assert_eq!(
            due_at(
                "schedule: \"0 0 29 2 *\"\nlast_run: \"2096-02-29T00:00:00Z\"\n",
                "2103-06-01T00:00:00Z"
            ),
            Due::NotDue
        );
    }

    /// The session at [`SESSION`] with its card [`CARD_FILE`] as `text`.
    fn session_with(text: &str) -> tempfile::TempDir {
        let zone = tempfile::tempdir().expect("zone");
        let session = zone.path().join(SESSION);
        std::fs::create_dir_all(&session).expect("session");
        std::fs::write(session.join(CARD_FILE), text).expect("card");
        zone
    }

    const HOURLY: &str = "---\ntags: [task]\ntitle: Sort\nstatus: todo\nassignee: tola\nschedule: \"@hourly\"\nlast_run: \"2026-10-05T08:00:00Z\"\n---\n\nSort what came in.\n";

    /// Tola's run of `window` as the host's clock found it due at `now`.
    fn run_at(window: &str, now: &str) -> Scheduled {
        Scheduled::Run {
            card: CARD_FILE.to_owned(),
            window: window.to_owned(),
            now_ms: at(now),
            utc_offset_minutes: 0,
        }
    }

    /// `begin` as tola's session on hesperia, the claim held.
    fn begin_in(zone: &Path, scheduled: &Scheduled) -> Result<Begun, VerbError> {
        let agent = session_of("tola", SessionKind::Scheduled);
        let holder = Holder {
            agent: &agent,
            host: "hesperia",
        };
        begin(zone, SESSION, &holder, scheduled, &|| true)
    }

    fn card_of(zone: &Path) -> String {
        std::fs::read_to_string(zone.join(SESSION).join(CARD_FILE)).expect("card")
    }

    /// R56–R58, R163, R164 under the claim: a window runs once whatever the
    /// host's scan said, an agent's schedule never, a wait is said once, a
    /// taken-over window is settled when `last_run` is before it, a run the
    /// card still says is running is settled too, and nothing is written
    /// without the claim.
    #[test]
    fn a_window_is_begun_once_under_the_claim() {
        let zone = session_with(HOURLY);
        let run = || run_at("2026-10-05T09:00:00Z", "2026-10-05T09:30:00Z");
        let agent = session_of("tola", SessionKind::Scheduled);
        let holder = Holder {
            agent: &agent,
            host: "hesperia",
        };
        assert!(matches!(
            begin(zone.path(), SESSION, &holder, &run(), &|| false),
            Err(VerbError::Refused(sentence)) if sentence == NO_CLAIM
        ));
        assert_eq!(card_of(zone.path()), HOURLY);
        assert_eq!(
            begin_in(zone.path(), &run()).expect("run"),
            Begun::Run {
                brief: "Sort what came in.".to_owned(),
                untrusted: false
            }
        );
        let ran = CardAgent::of_text(&card_of(zone.path())).expect("keys");
        assert_eq!(ran.run, Some(Field::Read(Run::Running)));
        assert_eq!(due(&ran, at("2026-10-05T09:30:00Z"), 0), Due::NotDue);
        assert_eq!(
            begin_in(zone.path(), &run()).expect("again"),
            Begun::Nothing(WINDOW_RAN)
        );

        let taken = |window: &str| Scheduled::TakenOver {
            card: CARD_FILE.to_owned(),
            window: Some(window.to_owned()),
            host: "electra".to_owned(),
        };
        let unknown = Begun::Said(RunBody {
            state: RunState::Review,
            detail: Some("ran on electra, effect unknown".to_owned()),
            step: None,
        });
        // The window was written, its run never finished: it is settled,
        // `last_run` left as it is (R164).
        assert_eq!(
            begin_in(zone.path(), &taken("2026-10-05T09:00:00Z")).expect("crashed"),
            unknown
        );
        let settled = CardAgent::of_text(&card_of(zone.path())).expect("keys");
        assert_eq!(settled.run, Some(Field::Read(Run::Review)));
        assert!(!unsettled(&settled, Some("2026-10-05T09:00:00Z")));
        assert_eq!(
            begin_in(zone.path(), &taken("2026-10-05T09:00:00Z")).expect("settled"),
            Begun::Nothing(WINDOW_RAN)
        );
        std::fs::write(zone.path().join(SESSION).join(CARD_FILE), HOURLY).expect("card");
        assert_eq!(
            begin_in(zone.path(), &taken("2026-10-05T09:00:00Z")).expect("unknown"),
            unknown
        );
        let settled = CardAgent::of_text(&card_of(zone.path())).expect("keys");
        assert_eq!(settled.run, Some(Field::Read(Run::Review)));
        assert_eq!(due(&settled, at("2026-10-05T09:59:00Z"), 0), Due::NotDue);

        let wait = Scheduled::Wait {
            card: CARD_FILE.to_owned(),
            waiting: "hesperia — a live host".to_owned(),
        };
        assert!(
            matches!(begin_in(zone.path(), &wait), Ok(Begun::Said(body)) if body.state == RunState::Waiting)
        );
        assert_eq!(
            begin_in(zone.path(), &wait).expect("again"),
            Begun::Nothing(WAITS_ALREADY)
        );

        let marked = HOURLY.replace(
            "assignee: tola\n",
            "assignee: tola\nscheduled_by: \"@tola:h\"\n",
        );
        std::fs::write(zone.path().join(SESSION).join(CARD_FILE), &marked).expect("card");
        assert_eq!(
            begin_in(zone.path(), &run()).expect("unticked"),
            Begun::Nothing(SCHEDULE_UNTICKED)
        );
        assert_eq!(card_of(zone.path()), marked);
    }

    /// R163: what the host's clock found is found again on the
    /// bytes under the claim. An edit made while the run waited for the
    /// zone — the schedule gone, later or unreadable, `last_run` unreadable
    /// or reached by a run a minute before, the assignee changed, the card
    /// pinned elsewhere or untagged, a second scheduled card — runs nothing
    /// and writes nothing; an edit of the body alone runs the new body.
    #[test]
    fn a_run_is_judged_again_on_the_card_under_the_claim() {
        let run = || run_at("2026-10-05T09:00:00Z", "2026-10-05T09:30:00Z");
        for (edited, why) in [
            (
                HOURLY.replace("schedule: \"@hourly\"\n", ""),
                NOT_ITS_SCHEDULE,
            ),
            (HOURLY.replace("@hourly", "0 23 * * *"), NOT_DUE),
            (HOURLY.replace("@hourly", "every 10s"), NOT_DUE),
            (
                HOURLY.replace("\"2026-10-05T08:00:00Z\"", "yesterday"),
                NOT_DUE,
            ),
            (
                HOURLY.replace("assignee: tola", "assignee: nixi"),
                NOT_ITS_SCHEDULE,
            ),
            (
                HOURLY.replace("assignee: tola\n", "assignee: tola\nhost: electra\n"),
                PINNED_ELSEWHERE,
            ),
            (
                HOURLY.replace("tags: [task]", "tags: [note]"),
                NOT_ITS_SCHEDULE,
            ),
        ] {
            let zone = session_with(&edited);
            assert_eq!(
                begin_in(zone.path(), &run()).expect("judged"),
                Begun::Nothing(why),
                "{edited}"
            );
            assert_eq!(card_of(zone.path()), edited);
        }

        let zone = session_with(HOURLY);
        std::fs::write(zone.path().join(SESSION).join("second.md"), HOURLY).expect("second");
        assert_eq!(
            begin_in(zone.path(), &run()).expect("judged"),
            Begun::Nothing(NOT_ITS_SCHEDULE)
        );
        assert_eq!(card_of(zone.path()), HOURLY);

        // A run of 09:00 reaching the card at 10:30, when 10:00 is the
        // latest window, is no run of the window the card is due in.
        let zone = session_with(HOURLY);
        assert_eq!(
            begin_in(
                zone.path(),
                &run_at("2026-10-05T09:00:00Z", "2026-10-05T10:30:00Z")
            )
            .expect("judged"),
            Begun::Nothing(NOT_DUE)
        );
        assert_eq!(card_of(zone.path()), HOURLY);

        // An `every 2h` card first seen at 09:30:58 ran 09:30; at 09:31:00
        // a host whose scan still says it never ran finds 09:31 due.
        let every = HOURLY
            .replace("@hourly", "every 2h")
            .replace("2026-10-05T08:00:00Z", "2026-10-05T09:30:00Z");
        let zone = session_with(&every);
        assert_eq!(
            begin_in(
                zone.path(),
                &run_at("2026-10-05T09:31:00Z", "2026-10-05T09:31:00Z")
            )
            .expect("judged"),
            Begun::Nothing(NOT_DUE)
        );
        assert_eq!(card_of(zone.path()), every);

        let zone = session_with(&HOURLY.replace("Sort what came in.", "Sort it twice."));
        assert_eq!(
            begin_in(zone.path(), &run()).expect("run"),
            Begun::Run {
                brief: "Sort it twice.".to_owned(),
                untrusted: false
            }
        );
    }

    /// The closed session a harvest reads, zone-relative.
    const TAXES: &str = "archive/2026/2026-10-05-taxes";
    const NOTE_AT: &str = "artifacts/knowledge/2026-10-05-taxes/what-to-bring.md";

    /// A drive whose sessions zone holds her harvest session ([`SESSION`])
    /// and the closed session [`TAXES`] with an artifact and a log chunk,
    /// beside the drive's OKF type registry.
    fn harvest_drive() -> (tempfile::TempDir, std::path::PathBuf) {
        let drive = tempfile::tempdir().expect("drive");
        let zone = drive.path().join("60-sessions");
        std::fs::create_dir_all(zone.join(SESSION)).expect("harvest session");
        let taxes = zone.join(TAXES);
        std::fs::create_dir_all(taxes.join("artifacts")).expect("artifacts");
        std::fs::create_dir_all(taxes.join("log")).expect("log");
        std::fs::write(taxes.join("README.md"), "# Taxes\n").expect("readme");
        std::fs::write(taxes.join("artifacts/answer.md"), "PIT-37.\n").expect("answer");
        std::fs::write(taxes.join("log/2026-10-05.electra.1.jsonl"), "{}\n").expect("log");
        std::fs::create_dir_all(drive.path().join(".okf/registry")).expect("registry");
        std::fs::write(
            drive.path().join(knowledge::REGISTRY),
            "| `type` | what |\n|---|---|\n| `Note` | x |\n| `Reference` | y |\n",
        )
        .expect("types");
        (drive, zone)
    }

    fn candidate(resource: &str, head: &str) -> String {
        format!("---\ntype: Reference\ntitle: What to bring\n{head}sources:\n  - id: s1\n    resource: {resource}\n  - id: s2\n    resource: log/2026-10-05.electra.1.jsonl#01J5ZZZZZZZZZZZZZZZZZZZZZZ\n---\n\nThe PIT-37 and the receipts.\n")
    }

    const ANSWER: &str = "60-sessions/archive/2026/2026-10-05-taxes/artifacts/answer.md";

    /// 95.5 acceptance 1 (R140, through the `session_write` tool): in her
    /// harvest session a note whose model wrote `generated: {by:
    /// human:tgorka}` and `human_reviewed: true` is stored generated by
    /// `agent:tola-grey@electra`, unreviewed, with no `verified`; a write
    /// carrying `verified`, one citing a file of another session, and one
    /// citing a file the closed session does not have are refused and
    /// change nothing; a note of exactly 64 KiB as stored is stored and one
    /// byte more is refused with the sentence, nothing written.
    #[test]
    fn a_harvested_note_never_claims_a_person() {
        let (drive, zone) = harvest_drive();
        let view = view(Integrity::Agent);
        let allow = allow();
        let mut tools = tools(&zone, &view, &allow);
        tools.drive_root = drive.path().to_owned();
        let write = |path: &str, content: &str| {
            tools.run(&call(
                SESSION_WRITE,
                json!({"path": path, "content": content}),
            ))
        };
        let stored = |path: &str| std::fs::read_to_string(zone.join(SESSION).join(path)).ok();

        let forged = candidate(
            ANSWER,
            "generated: {by: human:tgorka, at: 2020-01-01}\nhuman_reviewed: true\n",
        );
        assert!(matches!(
            write(NOTE_AT, &forged),
            Some(ToolOutcome::Answered { .. })
        ));
        let note = stored(NOTE_AT).expect("stored");
        let (fm, _) = Frontmatter::parse(&note);
        let doc = keeper_core::notes::okf::read(&fm);
        assert_eq!(
            doc.generated.map(|generated| generated.by).as_deref(),
            Some("agent:tola-grey@electra")
        );
        assert_eq!(fm.as_bool(knowledge::HUMAN_REVIEWED), Some(false));
        assert_eq!(fm.count("verified"), 0);
        assert_eq!(doc.doc_type.as_deref(), Some("Reference"));

        let verified = candidate(
            ANSWER,
            "verified:\n  - by: human:tgorka\n    at: 2026-10-06\n",
        );
        assert_eq!(
            refusal(write(NOTE_AT, &verified)),
            KnowledgeRefusal::Verified.to_string()
        );
        let other = "60-sessions/archive/2026/2026-10-01-other/artifacts/answer.md";
        assert_eq!(
            refusal(write(NOTE_AT, &candidate(other, ""))),
            KnowledgeRefusal::SourceOutside {
                resource: other.to_owned(),
                session: TAXES.to_owned()
            }
            .to_string()
        );
        let absent = "60-sessions/archive/2026/2026-10-05-taxes/artifacts/gone.md";
        let missing = |resource: &str| {
            KnowledgeRefusal::SourceMissing {
                resource: resource.to_owned(),
                session: TAXES.to_owned(),
            }
            .to_string()
        };
        assert_eq!(
            refusal(write(NOTE_AT, &candidate(absent, ""))),
            missing(absent)
        );
        // R95K-10: a file of the closed session that is a link to another
        // session's file in the same zone is not the closed session's.
        #[cfg(unix)]
        {
            let elsewhere = zone.join("archive/2026/2026-10-01-other/artifacts");
            std::fs::create_dir_all(&elsewhere).expect("other session");
            std::fs::write(elsewhere.join("secret.md"), "theirs\n").expect("theirs");
            std::os::unix::fs::symlink(
                elsewhere.join("secret.md"),
                zone.join(TAXES).join("artifacts/linked.md"),
            )
            .expect("link");
            let linked = "60-sessions/archive/2026/2026-10-05-taxes/artifacts/linked.md";
            assert_eq!(
                refusal(write(NOTE_AT, &candidate(linked, ""))),
                missing(linked)
            );
        }
        // R95K2-05: an author spelt with YAML escapes is the person every
        // YAML reader reads in it.
        for (author, actor) in [
            ("\"\\u0068uman:tgorka\"", "human:tgorka"),
            ("\"\\x75ser:marta\"", "user:marta"),
        ] {
            let escaped = candidate(ANSWER, "").replace(
                "  - id: s2\n",
                &format!("    author: {author}\n  - id: s2\n"),
            );
            assert_eq!(
                refusal(write(NOTE_AT, &escaped)),
                KnowledgeRefusal::ClaimsPerson {
                    actor: actor.to_owned()
                }
                .to_string()
            );
        }
        assert_eq!(
            stored(NOTE_AT).as_deref(),
            Some(note.as_str()),
            "nothing was written"
        );
        assert_eq!(
            refusal(write(
                "artifacts/knowledge/2026-10-01-nowhere/x.md",
                &candidate(ANSWER, "")
            )),
            KnowledgeRefusal::NoSuchSession {
                slug: "2026-10-01-nowhere".to_owned()
            }
            .to_string()
        );

        let plain = candidate(ANSWER, "");
        let small = "artifacts/knowledge/2026-10-05-taxes/small.md";
        assert!(matches!(
            write(small, &plain),
            Some(ToolOutcome::Answered { .. })
        ));
        let base = stored(small).expect("stored").len();
        let padded = |extra: usize| format!("{plain}{}", "x".repeat(extra));
        let exact = "artifacts/knowledge/2026-10-05-taxes/exact.md";
        assert!(matches!(
            write(exact, &padded(knowledge::MAX_NOTE_BYTES - base)),
            Some(ToolOutcome::Answered { .. })
        ));
        assert_eq!(
            stored(exact).map(|text| text.len()),
            Some(knowledge::MAX_NOTE_BYTES)
        );
        let over = "artifacts/knowledge/2026-10-05-taxes/over.md";
        assert!(
            refusal(write(over, &padded(knowledge::MAX_NOTE_BYTES - base + 1)))
                .starts_with("a knowledge note holds at most 64 KiB")
        );
        assert_eq!(stored(over), None, "nothing was written");
    }

    /// Every file under `dir`, by relative path, with its bytes.
    fn tree(dir: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
        let mut out = std::collections::BTreeMap::new();
        let mut todo = vec![dir.to_owned()];
        while let Some(at) = todo.pop() {
            for entry in std::fs::read_dir(&at).expect("read_dir").flatten() {
                let path = entry.path();
                if path.is_dir() {
                    todo.push(path);
                } else {
                    let rel = path
                        .strip_prefix(dir)
                        .expect("inside")
                        .to_string_lossy()
                        .into_owned();
                    out.insert(rel, std::fs::read(&path).expect("read"));
                }
            }
        }
        out
    }

    /// 95.5 acceptance 2 (R26 Q2, §3 row 29): a harvest turn's write aimed
    /// at the closed session is refused — by `..`, or through a link in its
    /// own session — and spelt from the drive's root it lands in her own
    /// session; the closed session's files and log are byte-identical after.
    #[cfg(unix)]
    #[test]
    fn harvest_never_writes_the_closed_session() {
        let (drive, zone) = harvest_drive();
        let view = view(Integrity::Agent);
        let allow = allow();
        let mut tools = tools(&zone, &view, &allow);
        tools.drive_root = drive.path().to_owned();
        let before = tree(&zone.join(TAXES));
        let write = |path: &str| {
            tools.run(&call(
                SESSION_WRITE,
                json!({"path": path, "content": candidate(ANSWER, "")}),
            ))
        };

        assert!(
            refusal(write("../../archive/2026/2026-10-05-taxes/artifacts/x.md"))
                .contains("not a plain path inside the session")
        );
        std::fs::create_dir_all(zone.join(SESSION).join("artifacts/knowledge")).expect("dir");
        std::os::unix::fs::symlink(
            zone.join(TAXES),
            zone.join(SESSION)
                .join("artifacts/knowledge/2026-10-05-taxes"),
        )
        .expect("link");
        assert!(refusal(write(NOTE_AT)).contains("does not stay inside this session"));
        assert!(matches!(
            write("60-sessions/archive/2026/2026-10-05-taxes/notes.md"),
            Some(ToolOutcome::Answered { .. })
        ));
        assert!(zone
            .join(SESSION)
            .join("60-sessions/archive/2026/2026-10-05-taxes/notes.md")
            .is_file());
        assert_eq!(tree(&zone.join(TAXES)), before);
    }
}
