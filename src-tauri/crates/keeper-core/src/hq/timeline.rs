//! An opened session (AD-424; FR-848, NFR-127): its messages as the window
//! holds them, newest months first and widened month by month as the person
//! scrolls back, each message with its turn state (R465: *waiting* until the
//! server has it, *sent*, *taken* once the agent's turn names it — never a
//! read receipt) and an agent's ask (R-HQ-28); its approvals; the answer
//! still being written, an ended answer's details, thinking and tool steps
//! as far as this viewer chose and may see them ([`live`]); the session's
//! tools and whether it read outside content.
//!
//! An archived session holds no rows here. Opening it reads its newest page
//! through the server's registered `sessionPage` query, read-only, page by
//! page and month by month on scroll-back; a row this device still holds in
//! its outbox is shown beside them. While it is open its attachments' rows
//! are held too (`sessions::plan`), so its files show as chips and open
//! through the same row check as an active session's — their bytes only
//! when a chip is opened.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use ts_rs::TS;

use crate::hq::approvals::{self, AgentApprovalStateVm, AgentApprovalVm, Undecidable, WaitingFor};
use crate::hq::attachments::{self, AgentAttachmentVm};
use crate::hq::drives::AgentSessionDrivesVm;
use crate::hq::instances::{self, AgentSessionAnsweringVm};
use crate::hq::live::{
    self, AgentLiveTurnVm, AgentMessageDetailsVm, AgentSessionToolVm, AgentToolStepVm,
    AgentViewPrefsVm, Extras, Live,
};
use crate::hq::mention::{self, AgentChildSessionVm};
use crate::hq::port::ReplicaError;
use crate::hq::sessions::{
    earlier_months, int, is_ask, is_heard, json, mark, month, newest_months, opt_text, page_key,
    previous_month, read, reads_trace, text, truthy, window, Source,
};
#[path = "turn_folds.rs"]
mod turn_folds;

/// The registered `sessionPage` query's operation id, as `syncular/`'s
/// generated `syncular.queries.ts` names it.
pub const SESSION_PAGE: &str =
    "sha256:9e32b5abb163f2d4dbacb19817c20004771f4b2a9016c304706c023c4f70dff8/sessionPage";

/// Messages per remote page: the newest page an archived session opens on.
pub const PAGE_SIZE: usize = 50;

/// The largest number SQLite and JSON agree on: the first page's cursor.
const NEWEST: i64 = 9_007_199_254_740_991;

/// How many empty months a fetch looks back through before it stops for
/// this page.
const MAX_EMPTY_MONTHS: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AgentMessageRoleVm {
    Person,
    Agent,
}

/// A person's message, by its turn (R465).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AgentMessageStateVm {
    /// Written here; the server does not have it yet.
    Waiting,
    /// The server has it.
    Sent,
    /// The agent's turn names it.
    Taken,
    /// The server refused the commit that carried it (an attachment's
    /// refusal, [`AgentAttachmentVm::sentence`]); nothing of it reached the
    /// session.
    Refused,
}

/// An agent's question to the person (R-HQ-28).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentSessionAskVm {
    /// The answers it offers; empty for a free answer.
    pub options: Vec<String>,
    /// No later message of the person answered it yet.
    pub open: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentMessageVm {
    pub id: String,
    pub role: AgentMessageRoleVm,
    pub author_actor: String,
    /// Written by this device's person (`author_actor` is the source's
    /// actor), from any of their devices: drawn as their own bubble. Another
    /// person's message in the same session is not (R-HQ-86).
    pub mine: bool,
    pub author_name: String,
    pub body: String,
    #[ts(type = "number")]
    pub created_at_ms: i64,
    /// This device's messages only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub state: Option<AgentMessageStateVm>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub ask: Option<AgentSessionAskVm>,
    /// An agent answer's details, in the `verbose` view mode only. Its
    /// turn's thinking and tool calls are the turn's fold's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub details: Option<AgentMessageDetailsVm>,
    /// The files attached to it, in the order they were attached.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<AgentAttachmentVm>>", optional)]
    pub attachments: Vec<AgentAttachmentVm>,
}

/// Ordered reading column. References resolve against `messages`, `folds`,
/// and `live`; grouping and ordering are never inferred by the renderer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum AgentTimelineItemVm {
    Message {
        message_id: String,
    },
    Fold {
        turn_id: String,
    },
    LiveAnswer {
        turn_id: String,
    },
    /// 109.24: a thread this person started from here with an `@mention`,
    /// where it was started; resolves against `children` by `thread_id`
    /// (its first line's id, never reused).
    Child {
        thread_id: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AgentTurnStatusVm {
    Running,
    Done,
    Parked,
    /// The provider refused to answer, or tool-only work ended refused.
    /// Never a stop, an interruption or a failure.
    Refused,
    Interrupted,
    Waiting,
    /// A person's Stop (109.19): asked for, or the turn closed `cancelled`.
    Stopped,
    /// The stream broke or the provider/runtime failed (`finish: failed`).
    Failed,
}

/// One compact, always-visible outcome above the answer, not a tool card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentTurnFailureVm {
    pub step_id: String,
    pub sentence: String,
}

/// One turn's work, separate from its terminal answer. Lead-ins reference
/// the original messages so attachments and author identity are retained.
/// The requests its parked calls raised are read with it: decided ones in
/// its body, folded with its work; pending ones that are not in the banner
/// always visible, like `failures`, and never inside a closed fold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentTurnFoldVm {
    pub turn_id: String,
    pub lead_in_ids: Vec<String>,
    pub reasoning: Option<String>,
    pub steps: Vec<AgentToolStepVm>,
    pub failures: Vec<AgentTurnFailureVm>,
    pub status: AgentTurnStatusVm,
    pub status_label: String,
    pub summary: String,
    #[ts(type = "number | null")]
    pub duration_ms: Option<i64>,
    /// Its requests that were decided (or expired), oldest first: inside the
    /// fold's body, so they fold with the turn.
    pub approvals: Vec<AgentApprovalVm>,
    /// Its requests still pending that the banner does not hold — they wait
    /// for someone else, or for another of this person's devices — oldest
    /// first; always visible, outside the body.
    pub waiting: Vec<AgentApprovalVm>,
}

/// The requests a message sent now would decline, held above the composer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentApprovalBannerVm {
    /// Oldest first; empty when every request waiting for this person needs
    /// another of their devices (DW-E109-27).
    pub approvals: Vec<AgentApprovalVm>,
    /// What sending a message does to them, in plain words, and how many
    /// requests wait for another device instead.
    pub sentence: String,
}

/// The banner over `approvals`' requests a message would decline, and the
/// rest, for the reading column. A T4 request this device cannot decide
/// waits for another of the person's devices, or for the agent's host to be
/// known: a message does not decline it, so it stays in the reading column
/// and the banner says, in one more sentence each, how many wait that way
/// (DW-E109-27, R109T-04).
pub fn banner(
    approvals: Vec<AgentApprovalVm>,
) -> (Option<AgentApprovalBannerVm>, Vec<AgentApprovalVm>) {
    let count = |why: Undecidable| {
        approvals
            .iter()
            .filter(|approval| approval.undecidable == Some(why))
            .count()
    };
    let (elsewhere, unknown) = (
        count(Undecidable::AnotherDevice),
        count(Undecidable::HostUnknown),
    );
    let (waiting, rest): (Vec<_>, Vec<_>) = approvals
        .into_iter()
        .partition(|approval| approval.declined_by_send);
    let declines = match waiting.len() {
        0 => None,
        1 => Some(
            "Sending a message declines this request, even a message that says yes. Use Allow \
             to let it run."
                .to_owned(),
        ),
        n => Some(format!(
            "Sending a message declines these {n} requests, even a message that says yes. \
             Use Allow to let one run."
        )),
    };
    let another = match elsewhere {
        0 => None,
        1 => Some(
            "1 request needs a decision from another device; a message here leaves it waiting."
                .to_owned(),
        ),
        n => Some(format!(
            "{n} requests need a decision from another device; a message here leaves them \
             waiting."
        )),
    };
    let host = match unknown {
        0 => None,
        1 => Some(
            "1 request can't be decided until the agent's host is known; a message here leaves \
             it waiting."
                .to_owned(),
        ),
        n => Some(format!(
            "{n} requests can't be decided until the agent's host is known; a message here \
             leaves them waiting."
        )),
    };
    let sentence: Vec<String> = declines.into_iter().chain(another).chain(host).collect();
    let banner = (!sentence.is_empty()).then(|| AgentApprovalBannerVm {
        sentence: sentence.join(" "),
        approvals: waiting,
    });
    (banner, rest)
}

/// Why an untrusted session's commands ask (D-34): it read a web page, a
/// broker's answer or other outside content, so every consequential action
/// waits for a person (owner ruling 2026-10-27).
pub const UNTRUSTED: &str =
    "This chat read outside content, such as a web page, so its commands need your approval.";

/// The honest way out of an untrusted chat: `/clean` starts a fresh one with
/// the same agent, which starts trusted (Q109-17 b) and carries only the
/// ticks this person may still tick (`drives::carry`, A109-06), while the
/// mark on this one never lowers (D-34, V13).
pub const FRESH_CHAT: &str =
    "Start a fresh chat: starts trusted, keeps the drives you can still tick";

/// A session that read outside content, as its header's chip says it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentUntrustedVm {
    /// Why its commands ask ([`UNTRUSTED`]).
    pub sentence: String,
    /// The words of the action that runs `/clean` ([`FRESH_CHAT`]); absent
    /// on an archived session, where `/clean` refuses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub fresh_chat: Option<String>,
}

impl AgentUntrustedVm {
    fn of(archived: bool) -> Self {
        Self {
            sentence: UNTRUSTED.to_owned(),
            fresh_chat: (!archived).then(|| FRESH_CHAT.to_owned()),
        }
    }
}

/// What an opened session shows, whole, every time it changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentSessionViewVm {
    pub database: String,
    pub session_id: String,
    pub title: String,
    pub display_name: String,
    pub handle: String,
    pub agent_actor: String,
    pub agent_name: String,
    pub agent_mark: String,
    pub archived: bool,
    /// Read through the server, not held here: nothing can be written.
    pub read_only: bool,
    /// Oldest first.
    pub messages: Vec<AgentMessageVm>,
    /// The reading column, in order (109.13): what the window draws. Its
    /// entries name `messages`, `folds` and `live`.
    pub items: Vec<AgentTimelineItemVm>,
    /// One per turn, in the order the column names them.
    pub folds: Vec<AgentTurnFoldVm>,
    /// 109.24: the conversations this person started from this one with an
    /// `@mention`, oldest first, each with its agent's latest answer.
    #[serde(default)]
    #[ts(as = "Option<Vec<AgentChildSessionVm>>", optional)]
    pub children: Vec<AgentChildSessionVm>,
    /// 109.24 (R10924K-08): that older threads of the shown months were left
    /// out of `children`, and where they are; drawn above the first one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub children_note: Option<String>,
    /// Oldest first; empty for a member who does not read the trace. Only
    /// the requests whose turn this view does not show: the others are in
    /// their turn's fold, and the requests that wait for this person are in
    /// `banner`.
    pub approvals: Vec<AgentApprovalVm>,
    /// The requests waiting for this person, above the composer and out of
    /// the reading column (Q109-12 a); absent while none waits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub banner: Option<AgentApprovalBannerVm>,
    /// Earlier messages exist beyond what is shown.
    pub has_earlier: bool,
    /// An earlier page is on its way.
    pub loading_earlier: bool,
    /// The newest page is all here; until then it may still be filling.
    pub complete: bool,
    /// Why something is missing, as the person reads it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub note: Option<String>,
    /// This person may read the session's thinking and tool results.
    pub reads_trace: bool,
    /// The session has read outside content (`sessions.trust`, D-34): why its
    /// commands ask, and the way out; absent while trusted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub untrusted: Option<AgentUntrustedVm>,
    /// The answers still being written, oldest first; each one's work is its
    /// turn's fold.
    pub live: Vec<AgentLiveTurnVm>,
    /// The tools the agent offers here and whether each is on; empty while
    /// no instance has said what it offers.
    pub tools: Vec<AgentSessionToolVm>,
    /// What this viewer sees of agents' turns, as the settings resolve it
    /// now (a settings file wins): the state its switches show, whichever
    /// device or file last changed it.
    pub prefs: AgentViewPrefsVm,
    /// Which instance is answering and each instance's online state, from
    /// this session's database; absent while it lists no instance of the
    /// agent and no claim, and while the session is archived: no instance
    /// claims an archived session, so nobody answers it until it is
    /// unarchived — the read-only bar says so (R-HQ-86).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub answering: Option<AgentSessionAnsweringVm>,
    /// Last answer's context occupancy, for any member in every view mode,
    /// unknown before the first measured answer (owner ruling 2026-10-27).
    pub context: super::context::AgentContextVm,
    /// The model and effort the next answer uses, always: the composer's bar
    /// draws them as chips (owner ruling 2026-10-27).
    pub footer: crate::hq::settings::AgentSessionFooterVm,
    /// The drives the agent may read here and this person's ticks; absent
    /// where this person has no control for them (not a person member, an
    /// archived session, an agent that has not said what it reads). The
    /// window fills it from every database this device holds
    /// ([`crate::hq::drives::view`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub drives: Option<AgentSessionDrivesVm>,
    /// The composer's model and effort pickers (109.18): absent for an
    /// archived session, while its settings are still arriving, and for a
    /// reader who may not change them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub settings: Option<crate::hq::settings::AgentSessionPickerVm>,
}

/// The pages of an archived session read so far through the server.
#[derive(Debug, Clone, Default)]
pub struct RemotePages {
    /// Newest first, as the query answers.
    pub rows: Vec<Map<String, Value>>,
    /// The month the next page reads, and the cursor inside it.
    pub month: Option<String>,
    pub cursor: Option<(i64, String)>,
    /// Every month back to the session's creation has been read.
    pub exhausted: bool,
    /// A fetch is under way.
    pub loading: bool,
    /// At least one fetch has ended.
    pub fetched: bool,
    pub error: Option<String>,
    /// The fetch whose answer may land; any other answer is stale.
    pub token: u64,
}

impl RemotePages {
    /// Start a fetch: its token, and the pages it reads from.
    pub fn begin(&mut self, token: u64) -> RemotePages {
        self.token = token;
        self.loading = true;
        self.clone()
    }

    /// Land a fetch's answer if it is the one this view waits for: rows
    /// already shown are not added twice, and the cursor moves only
    /// forward from the fetch that read it. `false` when it was stale.
    pub fn land(&mut self, token: u64, fetched: Result<Fetched, String>) -> bool {
        if !self.loading || token != self.token {
            return false;
        }
        self.loading = false;
        self.fetched = true;
        match fetched {
            Ok(page) => {
                let shown: HashSet<String> = self
                    .rows
                    .iter()
                    .map(|row| text(row, "id").to_owned())
                    .collect();
                self.rows.extend(
                    page.rows
                        .into_iter()
                        .filter(|row| !shown.contains(text(row, "id"))),
                );
                self.month = page.month;
                self.cursor = page.cursor;
                self.exhausted = page.exhausted;
                self.error = None;
            }
            Err(sentence) => self.error = Some(sentence),
        }
        true
    }
}

/// One opened session's state on this device.
#[derive(Debug, Clone, Default)]
pub struct Opened {
    /// The streams showing it.
    pub streams: usize,
    /// Months widened back beyond the newest page.
    pub widen: u32,
    pub remote: RemotePages,
    /// The tails seen of its answers still being written.
    pub live: Live,
    /// What this viewer chose to see.
    pub prefs: AgentViewPrefsVm,
}

/// A session's own row, as the view reads it.
#[derive(Debug, Clone)]
pub struct SessionRow {
    pub title: String,
    pub agent_actor: String,
    pub archived: bool,
    pub created_ms: i64,
    pub last_activity_ms: i64,
    /// `sessions.trust` is `untrusted`.
    pub untrusted: bool,
}

pub fn session_row(source: &Source, session_id: &str) -> Option<SessionRow> {
    let rows = read(
        source,
        "SELECT title, agent_actor, archived, created_at_ms, last_activity_ms, trust \
         FROM sessions WHERE id = ?1",
        &[Value::from(session_id)],
    );
    let row = rows.first()?;
    Some(SessionRow {
        title: text(row, "title").to_owned(),
        agent_actor: text(row, "agent_actor").to_owned(),
        archived: truthy(row, "archived"),
        created_ms: int(row, "created_at_ms").unwrap_or(0),
        last_activity_ms: int(row, "last_activity_ms").unwrap_or(0),
        untrusted: text(row, "trust") == "untrusted",
    })
}

const MESSAGES: &str = "SELECT id, role, body, metadata, created_at_ms, author_actor, \
     author_client, turn_id, _sync_version AS version FROM messages \
     WHERE session_id = ?1 ORDER BY created_at_ms, id";

/// The view of an opened session, and the newest message's time (what the
/// person has now read). Reads the replica and `opened`'s pages only.
pub fn view(
    source: &Source,
    session_id: &str,
    opened: &Opened,
    refusals: &approvals::Refusals,
    now_ms: i64,
) -> Result<(AgentSessionViewVm, Option<i64>), String> {
    let Some(session) = session_row(source, session_id) else {
        return Err("This conversation is not on this device.".to_owned());
    };
    let directory = crate::hq::sessions::directory(source);
    let agent_name = directory
        .get(&session.agent_actor)
        .cloned()
        .unwrap_or_else(|| session.agent_actor.clone());
    // The oldest month the view shows: where the threads started from it by
    // a mention are drawn from (109.24, R10924K-02). None while archived.
    let (mut rows, has_earlier, loading_earlier, complete, note, shown_from) = if session.archived {
        let local = read(source, MESSAGES, &[Value::from(session_id)]);
        let remote = &opened.remote;
        let held: HashSet<&str> = local.iter().map(|row| text(row, "id")).collect();
        let mut rows: Vec<Map<String, Value>> = remote
            .rows
            .iter()
            .filter(|row| !held.contains(text(row, "id")))
            .cloned()
            .collect();
        rows.extend(local.iter().cloned());
        rows.sort_by(|a, b| {
            int(a, "created_at_ms")
                .cmp(&int(b, "created_at_ms"))
                .then_with(|| text(a, "id").cmp(text(b, "id")))
        });
        (
            rows,
            !remote.exhausted,
            remote.loading && remote.fetched,
            remote.fetched,
            remote.error.clone(),
            None,
        )
    } else {
        let newest_months = newest_months(session.created_ms, session.last_activity_ms, now_ms);
        let newest: Vec<String> = newest_months
            .iter()
            .map(|m| page_key(session_id, m))
            .collect();
        let earlier = earlier_months(session.created_ms, session.last_activity_ms, opened.widen);
        let mut units = newest.clone();
        units.extend(earlier.iter().map(|m| page_key(session_id, m)));
        let (rows, incomplete) = source
            .replica
            .query_covered(MESSAGES, &[Value::from(session_id)], &window("messages"), &units)
            .unwrap_or_else(|error| {
                tracing::debug!(database = %source.database, %error, "an opened session could not be read");
                (Vec::new(), units.clone())
            });
        let first = month(session.created_ms);
        let oldest_shown = earlier
            .last()
            .cloned()
            .unwrap_or_else(|| month(session.last_activity_ms.max(session.created_ms)));
        let complete = !newest.iter().any(|unit| incomplete.contains(unit));
        let loading = earlier
            .iter()
            .any(|m| incomplete.contains(&page_key(session_id, m)));
        let shown_from = newest_months
            .iter()
            .chain(earlier.iter())
            .min()
            .cloned()
            .unwrap_or_else(|| oldest_shown.clone());
        (
            rows,
            oldest_shown > first,
            loading,
            complete,
            None,
            Some(shown_from),
        )
    };
    // A person's heard row is a mark for the claim holder, not a message
    // (R-HQ-68): never drawn.
    rows.retain(|row| text(row, "role") != "person" || !is_heard(&json(row, "metadata")));
    let taken = taken(source, session_id, &rows);
    let reads_trace = reads_trace(source, session_id);
    let extras = live::extras(source, session_id, reads_trace, opened.prefs.view_mode);
    let ended: HashSet<String> = rows
        .iter()
        .filter(|row| text(row, "role") == "agent" && live::is_terminal(&json(row, "metadata")))
        .filter_map(|row| opt_text(row, "turn_id"))
        .collect();
    let mut messages = messages(source, &mut rows, &directory, &taken, opened.prefs);
    let mut chips = attachments::chips(source, session_id);
    for message in &mut messages {
        message.attachments = chips.remove(&message.id).unwrap_or_default();
    }
    if !session.archived {
        refused(source, session_id, &directory, &mut messages);
    }
    let newest_ms = messages.last().map(|m| m.created_at_ms);
    let approvals = approvals::of_session(source, session_id, &directory, refusals, now_ms);
    // A turn whose request waits reads so on its fold line, naming whom it
    // waits for; this person first.
    let mut pending: HashMap<String, String> = HashMap::new();
    for approval in &approvals {
        let (Some(turn), Some(who)) = (&approval.turn_id, &approval.waiting_for) else {
            continue;
        };
        if *who == WaitingFor::You || !pending.contains_key(turn) {
            pending.insert(turn.clone(), who.label());
        }
    }
    let (banner, rest) = banner(approvals);
    // No instance claims an archived session (the runtime skips it), so any
    // header — "no instance holds it yet", "X takes it next" — would be false.
    let answering = if session.archived {
        None
    } else {
        instances::answering(
            source,
            session_id,
            &session.agent_actor,
            &agent_name,
            &opened.live,
            now_ms,
        )
    };
    let (display_name, handle) = crate::hq::sessions::SessionNames::read(source).presentation(
        session_id,
        &session.agent_actor,
        &session.title,
    );
    let live = live::turns(source, session_id, &opened.live, &ended, &extras);
    let stopping = stopping(source, session_id, &rows);
    let (mut items, mut folds) =
        turn_folds::project(&rows, &messages, &live, &extras, &pending, &stopping);
    let approvals = place(rest, &mut folds);
    let live = live.into_iter().map(|turn| turn.vm).collect();
    let (children, left_out) = match &shown_from {
        Some(since) => mention::children(source, session_id, since, &directory, opened.prefs),
        None => (Vec::new(), 0),
    };
    mention::place(&mut items, &messages, &children);
    let bar = crate::hq::settings::bar(source, session_id, &session.agent_actor, session.archived);
    Ok((
        AgentSessionViewVm {
            database: source.database.clone(),
            session_id: session_id.to_owned(),
            title: session.title,
            display_name,
            handle,
            agent_mark: mark(&agent_name),
            agent_name,
            archived: session.archived,
            read_only: session.archived,
            messages,
            items,
            folds,
            children,
            children_note: mention::left_out(left_out),
            approvals,
            banner,
            has_earlier,
            loading_earlier,
            complete,
            note,
            reads_trace,
            untrusted: session
                .untrusted
                .then(|| AgentUntrustedVm::of(session.archived)),
            live,
            tools: live::tools(source, session_id, &session.agent_actor),
            prefs: opened.prefs,
            answering,
            context: super::context::latest(
                &rows,
                super::context::Next {
                    model: &bar.footer.model,
                    chosen_at_ms: bar.chosen_at_ms,
                },
            ),
            drives: None,
            footer: bar.footer,
            settings: bar.picker,
            agent_actor: session.agent_actor,
        },
        newest_ms,
    ))
}

/// Each request outside the banner read in its turn's fold: a decided one in
/// its body, a pending one always visible. What has no turn this view shows
/// — its call is not on this device, or its turn is not in the shown months —
/// is returned, oldest first, for the column's end.
fn place(rest: Vec<AgentApprovalVm>, folds: &mut [AgentTurnFoldVm]) -> Vec<AgentApprovalVm> {
    let mut unplaced = Vec::new();
    for approval in rest {
        let fold = approval
            .turn_id
            .as_deref()
            .and_then(|turn| folds.iter_mut().find(|fold| fold.turn_id == turn));
        match fold {
            Some(fold) if approval.state == AgentApprovalStateVm::Pending => {
                fold.waiting.push(approval)
            }
            Some(fold) => fold.approvals.push(approval),
            None => unplaced.push(approval),
        }
    }
    unplaced
}

/// The turns a person asked to stop (109.19), as the runtime's
/// `stop::targets` reads a stop row: `metadata.kind = "stop"` naming the
/// turn, written by one of the session's members whose directory kind is
/// `person`. A row in a person's role written by anyone else stops nothing.
fn stopping(source: &Source, session_id: &str, rows: &[Map<String, Value>]) -> HashSet<String> {
    let people: HashSet<String> = read(
        source,
        "SELECT m.actor_id FROM session_members m JOIN actors a ON a.id = m.actor_id \
         WHERE m.session_id = ?1 AND a.kind = 'person'",
        &[Value::from(session_id)],
    )
    .iter()
    .filter_map(|row| opt_text(row, "actor_id"))
    .collect();
    rows.iter()
        .filter(|row| text(row, "role") == "person" && people.contains(text(row, "author_actor")))
        .filter_map(|row| {
            let metadata = json(row, "metadata");
            (metadata.get("kind").and_then(Value::as_str) == Some("stop"))
                .then(|| {
                    metadata
                        .get("turn")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .flatten()
        })
        .collect()
}

/// The person's messages a turn names: by an agent message or a streamed
/// chunk whose `turn_id` is the message's id.
fn taken(source: &Source, session_id: &str, rows: &[Map<String, Value>]) -> HashSet<String> {
    let mut taken: HashSet<String> = rows
        .iter()
        .filter(|row| text(row, "role") == "agent")
        .filter_map(|row| opt_text(row, "turn_id"))
        .collect();
    taken.extend(
        read(
            source,
            "SELECT DISTINCT turn_id FROM turn_chunks WHERE session_id = ?1",
            &[Value::from(session_id)],
        )
        .iter()
        .filter_map(|row| opt_text(row, "turn_id")),
    );
    taken
}

fn messages(
    source: &Source,
    rows: &mut [Map<String, Value>],
    directory: &HashMap<String, String>,
    taken: &HashSet<String>,
    prefs: AgentViewPrefsVm,
) -> Vec<AgentMessageVm> {
    let last_person = rows.iter().rposition(|row| text(row, "role") == "person");
    rows.iter()
        .enumerate()
        .filter_map(|(at, row)| {
            let id = opt_text(row, "id")?;
            let author = text(row, "author_actor").to_owned();
            let role = if text(row, "role") == "agent" {
                AgentMessageRoleVm::Agent
            } else {
                AgentMessageRoleVm::Person
            };
            let state = (role == AgentMessageRoleVm::Person
                && text(row, "author_client") == source.client)
                .then(|| {
                    if int(row, "version").is_some_and(|v| v < 0) {
                        AgentMessageStateVm::Waiting
                    } else if taken.contains(&id) {
                        AgentMessageStateVm::Taken
                    } else {
                        AgentMessageStateVm::Sent
                    }
                });
            let metadata = json(row, "metadata");
            let answer = role == AgentMessageRoleVm::Agent;
            let ask = (role == AgentMessageRoleVm::Agent && is_ask(&metadata)).then(|| {
                AgentSessionAskVm {
                    options: metadata
                        .get("options")
                        .and_then(Value::as_array)
                        .map(|options| {
                            options
                                .iter()
                                .filter_map(|o| o.as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default(),
                    open: last_person.is_none_or(|person| person < at),
                }
            });
            Some(AgentMessageVm {
                id,
                role,
                author_name: directory
                    .get(&author)
                    .cloned()
                    .unwrap_or_else(|| author.clone()),
                mine: author == source.actor,
                author_actor: author,
                body: text(row, "body").to_owned(),
                created_at_ms: int(row, "created_at_ms").unwrap_or(0),
                state,
                ask,
                details: (answer && prefs.view_mode.shows_details())
                    .then(|| live::details(&metadata))
                    .flatten(),
                attachments: Vec::new(),
            })
        })
        .collect()
}

/// A thread's latest turn as a parent draws it inline (109.24): the person's
/// first line and the newest turn's own rows, projected as 109.13 projects a
/// session — never the thread's whole view (R10924K-02).
pub(crate) struct LatestTurn {
    pub prompt: Option<AgentMessageVm>,
    /// From the turn's fold on.
    pub items: Vec<AgentTimelineItemVm>,
    pub folds: Vec<AgentTurnFoldVm>,
    pub messages: Vec<AgentMessageVm>,
    pub live: Vec<AgentLiveTurnVm>,
}

/// The person's first line of `session_id`.
const FIRST_LINE: &str = "SELECT id, role, body, metadata, created_at_ms, author_actor, \
     author_client, turn_id, _sync_version AS version FROM messages \
     WHERE session_id = ?1 AND role = 'person' ORDER BY created_at_ms, id LIMIT 1";

/// The newest turn of `session_id` an agent wrote to, or is writing: the
/// newest of its newest agent row and its newest streamed chunk, each read
/// alone, so the read does not grow with the thread's history.
const NEWEST_TURN: &str = "SELECT turn_id FROM ( \
     SELECT * FROM (SELECT turn_id, created_at_ms FROM messages \
     WHERE session_id = ?1 AND role = 'agent' AND turn_id IS NOT NULL \
     ORDER BY created_at_ms DESC, id DESC LIMIT 1) \
     UNION ALL SELECT * FROM (SELECT turn_id, created_at_ms FROM turn_chunks \
     WHERE session_id = ?1 ORDER BY created_at_ms DESC, id DESC LIMIT 1)) \
     ORDER BY created_at_ms DESC LIMIT 1";

/// The agent's rows of one turn.
const TURN_ROWS: &str = "SELECT id, role, body, metadata, created_at_ms, author_actor, \
     author_client, turn_id, _sync_version AS version FROM messages \
     WHERE session_id = ?1 AND role = 'agent' AND turn_id = ?2 ORDER BY created_at_ms, id";

/// Whether an agent's turn names message `?2` of `session_id` (R465's
/// *taken*), asked of that one message.
const TAKEN: &str = "SELECT (EXISTS (SELECT 1 FROM messages WHERE session_id = ?1 \
     AND role = 'agent' AND turn_id = ?2) OR EXISTS (SELECT 1 FROM turn_chunks \
     WHERE session_id = ?1 AND turn_id = ?2)) AS taken";

/// What a parent draws of a thread (R10924K-05): its first line and its
/// newest turn, every read scoped to that line or that turn — the thread's
/// earlier turns, their tool calls, thinking and chunks are never read.
/// Thinking is not read at all: the parent does not draw it.
pub(crate) fn latest_turn(
    source: &Source,
    session_id: &str,
    directory: &HashMap<String, String>,
    prefs: AgentViewPrefsVm,
) -> LatestTurn {
    let first = read(source, FIRST_LINE, &[Value::from(session_id)]);
    let turn = read(source, NEWEST_TURN, &[Value::from(session_id)])
        .first()
        .and_then(|row| opt_text(row, "turn_id"));
    let extras = match &turn {
        Some(turn) => live::extras_of(
            source,
            session_id,
            Some(turn),
            reads_trace(source, session_id),
            false,
        ),
        None => Extras {
            steps: HashMap::new(),
            reasoning: HashMap::new(),
            thinking: false,
        },
    };
    let none = HashSet::new();
    let taken: HashSet<String> = first
        .iter()
        .filter_map(|row| opt_text(row, "id"))
        .filter(|id| {
            read(
                source,
                TAKEN,
                &[Value::from(session_id), Value::from(id.as_str())],
            )
            .first()
            .is_some_and(|row| truthy(row, "taken"))
        })
        .collect();
    let mut prompt_rows = first;
    let prompt = messages(source, &mut prompt_rows, directory, &taken, prefs)
        .into_iter()
        .next();
    let Some(turn) = turn else {
        return LatestTurn {
            prompt,
            items: Vec::new(),
            folds: Vec::new(),
            messages: Vec::new(),
            live: Vec::new(),
        };
    };
    let mut rows = read(
        source,
        TURN_ROWS,
        &[Value::from(session_id), Value::from(turn.as_str())],
    );
    let ended: HashSet<String> = rows
        .iter()
        .filter(|row| live::is_terminal(&json(row, "metadata")))
        .map(|_| turn.clone())
        .collect();
    let live = live::turns_of(
        source,
        session_id,
        Some(&turn),
        &Live::default(),
        &ended,
        &extras,
    );
    let messages = messages(source, &mut rows, directory, &none, prefs);
    let (items, folds) =
        turn_folds::project(&rows, &messages, &live, &extras, &HashMap::new(), &none);
    let live = live.into_iter().map(|turn| turn.vm).collect();
    let items: Vec<AgentTimelineItemVm> = items
        .iter()
        .rposition(|item| matches!(item, AgentTimelineItemVm::Fold { .. }))
        .map(|at| items[at..].to_vec())
        .unwrap_or_default();
    LatestTurn {
        prompt,
        items,
        folds,
        messages,
        live,
    }
}

/// This device's messages whose files the server refused, among the others
/// by time: the replica no longer holds them, their outcome journal does.
fn refused(
    source: &Source,
    session_id: &str,
    directory: &HashMap<String, String>,
    messages: &mut Vec<AgentMessageVm>,
) {
    for refused in attachments::refused_messages(source, session_id) {
        let Some(id) = opt_text(&refused.values, "id") else {
            continue;
        };
        if messages.iter().any(|message| message.id == id) {
            continue;
        }
        let message = AgentMessageVm {
            id,
            role: AgentMessageRoleVm::Person,
            author_name: directory
                .get(&source.actor)
                .cloned()
                .unwrap_or_else(|| source.actor.clone()),
            author_actor: source.actor.clone(),
            mine: true,
            body: text(&refused.values, "body").to_owned(),
            created_at_ms: int(&refused.values, "created_at_ms").unwrap_or(0),
            state: Some(AgentMessageStateVm::Refused),
            ask: None,
            details: None,
            attachments: refused.attachments,
        };
        let at = messages.partition_point(|shown| {
            (shown.created_at_ms, shown.id.as_str()) <= (message.created_at_ms, message.id.as_str())
        });
        messages.insert(at, message);
    }
}

/// What a remote fetch read: rows newest first and where the next page
/// starts.
#[derive(Debug, Clone, PartialEq)]
pub struct Fetched {
    pub rows: Vec<Map<String, Value>>,
    pub month: Option<String>,
    pub cursor: Option<(i64, String)>,
    pub exhausted: bool,
}

/// Read the next page of an archived session through `sessionPage`: from
/// `pages`' month and cursor (the newest month on the first fetch), moving
/// to an earlier month when one runs out, until a page has rows or the
/// session's creation month is passed. Blocking; one request per month read.
pub fn fetch_page(
    source: &Source,
    session_id: &str,
    session: &SessionRow,
    pages: &RemotePages,
) -> Result<Fetched, ReplicaError> {
    let first = month(session.created_ms);
    let mut at = pages
        .month
        .clone()
        .unwrap_or_else(|| month(session.last_activity_ms.max(session.created_ms)));
    let mut cursor = pages.cursor.clone();
    let mut rows = Vec::new();
    for _ in 0..MAX_EMPTY_MONTHS {
        let (before_ms, before_id) = cursor.clone().unwrap_or((NEWEST, String::new()));
        let page = source.replica.remote_query(
            SESSION_PAGE,
            json!({
                "pageKey": page_key(session_id, &at),
                "beforeMs": before_ms,
                "beforeId": before_id,
                "pageSize": PAGE_SIZE,
            }),
        )?;
        let full = page.len() >= PAGE_SIZE;
        if let Some(last) = page.last() {
            cursor = Some((
                int(last, "created_at_ms").unwrap_or(0),
                text(last, "id").to_owned(),
            ));
        }
        rows.extend(page);
        if full {
            return Ok(Fetched {
                rows,
                month: Some(at),
                cursor,
                exhausted: false,
            });
        }
        let exhausted = at <= first;
        match previous_month(&at) {
            Some(before) if !exhausted => {
                at = before;
                cursor = None;
            }
            _ => {
                return Ok(Fetched {
                    rows,
                    month: None,
                    cursor: None,
                    exhausted: true,
                })
            }
        }
        if !rows.is_empty() {
            return Ok(Fetched {
                rows,
                month: Some(at),
                cursor: None,
                exhausted: false,
            });
        }
    }
    Ok(Fetched {
        rows,
        month: Some(at),
        cursor,
        exhausted: false,
    })
}

/// The sentence an archived session shows when the server could not be
/// asked.
pub fn fetch_sentence(database: &str, error: &ReplicaError) -> String {
    match error {
        ReplicaError::NotSyncing => format!(
            "This conversation is archived on the server; keeper will fetch it when {database} is reachable."
        ),
        other => format!("keeper could not fetch this archived conversation from {database}: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_session_page_id_is_the_contracts() {
        let generated = include_str!("../../../../../syncular/syncular.queries.ts");
        assert!(
            generated.contains(&format!("id: '{SESSION_PAGE}'")),
            "syncular.queries.ts names another sessionPage id; update SESSION_PAGE"
        );
    }
}
