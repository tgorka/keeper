//! A live answer (AD-423; FR-849, FR-850): a turn still being written,
//! folded from its committed `turn_chunks` and the claim holder's presence
//! tail, and what an ended turn shows beside its message — its details, its
//! thinking and its tool steps — as far as this viewer chose to see them
//! and may read them; and the session's tool switches (R-HQ-10).
//!
//! The chunks and the tail reach this device by two paths with no order
//! between them, and presence keeps nothing once its peer leaves, so this
//! device keeps every tail it has seen ([`Live`]) and folds by the viewer
//! rule (`syncular/README.md` § The live answer): the chunks contiguous from
//! seq 0, then the latest tail whose `from_seq` is the seq after them, until
//! the turn's `messages` row replaces both. The reasoning folds the same way
//! from seq 1 and its compacted row replaces it. A member who does not read
//! the trace is sent no reasoning, tool call or reasoning tail (D-34); rows
//! and tails this device still holds from before the flag was withdrawn are
//! neither drawn nor, for the tails, kept ([`Live::forget_trace`]).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::hq::port::{Peer, RowOp};
use crate::hq::sessions::{
    authored, int, json, names, object, opt_text, read, text, truthy, Source,
};

/// How long a claim's row may stay unchanged, as this device has watched
/// it, before its holder is taken for gone: the runtime's lease (104.5),
/// three renewals missed.
pub const LEASE: Duration = Duration::from_secs(30);

/// How far behind this device's clock a claim's own `renewed_at_ms` may be
/// before its holder is taken for gone at once — for a session opened long
/// after its instance stopped. Four leases, so the two hosts' clocks may
/// disagree by a minute and a half before it says so early.
const STALE_MS: i64 = 120_000;

/// How often an open session with an answer still arriving is read again,
/// so an answer whose instance stopped says so with nothing else changing.
pub const RECHECK: Duration = Duration::from_secs(2);

/// What this viewer chose to see — a person's settings, the same on every
/// device (R-HQ-31), a settings file winning.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentViewPrefsVm {
    /// The agent's thinking, where the person reads the trace.
    pub show_reasoning: bool,
    /// An answer's model, tokens and timings.
    pub show_details: bool,
}

/// A change of [`AgentViewPrefsVm`]: only what is present is written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct AgentViewPrefsPatch {
    pub show_reasoning: Option<bool>,
    pub show_details: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AgentToolStepStatusVm {
    Started,
    Done,
    Refused,
    Interrupted,
    Parked,
}

impl AgentToolStepStatusVm {
    pub fn label(self) -> &'static str {
        match self {
            Self::Started => "Started",
            Self::Done => "Finished",
            Self::Refused => "Refused",
            Self::Interrupted => "Interrupted",
            Self::Parked => "Paused",
        }
    }
}

/// One tool call of a turn, as its `tool_calls` row says (readers only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentToolStepVm {
    pub id: String,
    pub name: String,
    #[serde(default)]
    #[ts(as = "Option<String>", optional)]
    pub status_label: String,
    pub status: AgentToolStepStatusVm,
    /// The outcome, or the refusal's sentence, once it ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub result: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub started_at_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub ended_at_ms: Option<i64>,
}

/// A turn still being written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentLiveTurnVm {
    pub turn_id: String,
    /// What is written so far: always a prefix of the answer.
    pub text: String,
    /// The tool being called now, by name (the session's tail carries it to
    /// every member).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tool: Option<String>,
    /// The thinking so far, for a reader who chose to see it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reasoning: Option<String>,
    /// Its tool calls; empty for a member who does not read the trace.
    pub steps: Vec<AgentToolStepVm>,
    /// The instance writing it stopped without ending it: what is shown is
    /// all there will be.
    pub stopped: bool,
}

/// What an answer's provider reported. A value it did not report is
/// absent, never 0.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentMessageDetailsVm {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub prompt_tokens: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub completion_tokens: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub total_tokens: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub ttft_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub duration_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub finish: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error: Option<String>,
}

/// A tool the agent offers in a session, and whether it is on there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentSessionToolVm {
    pub name: String,
    pub on: bool,
}

// ---------------------------------------------------------------------------
// The fold
// ---------------------------------------------------------------------------

/// One stream's tails of one turn, by `from_seq`, as this viewer saw them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tails(BTreeMap<i64, String>);

impl Tails {
    /// A tail arrived. A tail only grows under its `from_seq`, so a copy
    /// the held one already extends is an older one arriving late.
    pub fn see(&mut self, from_seq: i64, tail: &str) {
        let held = self.0.entry(from_seq).or_default();
        if held.len() > tail.len() && held.starts_with(tail) {
            return;
        }
        if held != tail {
            held.clear();
            held.push_str(tail);
        }
    }
}

/// The text of `pieces` (`(seq, text)`, in any order) contiguous from
/// `first`, followed by the tail kept for the seq after the last of them.
pub fn fold<'a>(
    pieces: impl IntoIterator<Item = (i64, &'a str)>,
    first: i64,
    tails: &Tails,
) -> String {
    let mut sorted: Vec<(i64, &str)> = pieces.into_iter().collect();
    sorted.sort_unstable_by_key(|(seq, _)| *seq);
    let mut out = String::new();
    let mut next = first;
    for (seq, piece) in sorted {
        if seq != next {
            break;
        }
        out.push_str(piece);
        next += 1;
    }
    if let Some(tail) = tails.0.get(&next) {
        out.push_str(tail);
    }
    out
}

/// What this viewer has seen of one turn's presence.
#[derive(Debug, Clone, Default)]
struct Turn {
    text: Tails,
    reasoning: Tails,
    tool: Option<String>,
    /// The instance whose document named the turn.
    client: Option<String>,
    /// A document of the turn is on its scope now.
    present: bool,
    /// When this viewer last saw a document of the turn.
    seen: Option<Instant>,
}

/// A session's `claims` row, as far as telling a holder that stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    pub holder_client: String,
    pub epoch: i64,
    pub renewed_at_ms: i64,
    pub released: bool,
}

pub fn claim(source: &Source, session_id: &str) -> Option<Claim> {
    let rows = read(
        source,
        "SELECT holder_client, epoch, renewed_at_ms, released FROM claims WHERE session_id = ?1",
        &[Value::from(session_id)],
    );
    let row = rows.first()?;
    Some(Claim {
        holder_client: text(row, "holder_client").to_owned(),
        epoch: int(row, "epoch").unwrap_or(0),
        renewed_at_ms: int(row, "renewed_at_ms").unwrap_or(0),
        released: truthy(row, "released"),
    })
}

/// Every tail an opened session's viewer has seen, kept through a presence
/// leave, and how long its claim has stood still.
#[derive(Debug, Clone, Default)]
pub struct Live {
    turns: HashMap<String, Turn>,
    /// When this device first saw each turn being written — by a document
    /// or by its chunks alone: how long a turn with no claim to judge it by
    /// has gone without a document, when it never had one.
    first: HashMap<String, Instant>,
    claim: Option<(Claim, Instant)>,
    /// The claim's holder is gone ([`LEASE`], [`STALE_MS`]) or let it go.
    lapsed: bool,
    /// The latest observation.
    at: Option<Instant>,
}

fn doc_turn(doc: &Value) -> Option<(&str, i64)> {
    Some((doc.get("turn")?.as_str()?, doc.get("from_seq")?.as_i64()?))
}

impl Live {
    /// Read the peers present on the session's key and on its trace keys,
    /// and its claim. Documents of no turn (another kind of presence) are
    /// not the answer's.
    pub fn observe(
        &mut self,
        session: &[Peer],
        trace: &[Peer],
        claim: Option<Claim>,
        now: Instant,
        now_ms: i64,
    ) {
        self.at = Some(now);
        // A document that left takes its tool with it: a present one names
        // it again in this same pass.
        for turn in self.turns.values_mut() {
            turn.present = false;
            turn.tool = None;
        }
        for peer in session {
            let Some((id, from_seq)) = doc_turn(&peer.doc) else {
                continue;
            };
            let Some(tail) = peer.doc.get("text").and_then(Value::as_str) else {
                continue;
            };
            self.first.entry(id.to_owned()).or_insert(now);
            let turn = self.turns.entry(id.to_owned()).or_default();
            turn.text.see(from_seq, tail);
            turn.tool = peer
                .doc
                .get("tool")
                .and_then(Value::as_str)
                .map(str::to_owned);
            turn.client = Some(peer.client_id.clone());
            turn.present = true;
            turn.seen = Some(now);
        }
        for peer in trace {
            let Some((id, from_seq)) = doc_turn(&peer.doc) else {
                continue;
            };
            let Some(tail) = peer.doc.get("reasoning").and_then(Value::as_str) else {
                continue;
            };
            self.first.entry(id.to_owned()).or_insert(now);
            let turn = self.turns.entry(id.to_owned()).or_default();
            turn.reasoning.see(from_seq, tail);
            turn.client.get_or_insert_with(|| peer.client_id.clone());
            turn.present = true;
            turn.seen = Some(now);
        }
        self.claim = match (self.claim.take(), claim) {
            (Some((held, since)), Some(now_claim)) if held == now_claim => Some((held, since)),
            (_, Some(now_claim)) => Some((now_claim, now)),
            (_, None) => None,
        };
        self.lapsed = self.claim.as_ref().is_some_and(|(claim, since)| {
            claim.released
                || now.saturating_duration_since(*since) >= LEASE
                || now_ms - claim.renewed_at_ms >= STALE_MS
        });
    }

    /// Whether `claim` — the session's row now — has lapsed as this device
    /// has watched it: released, unchanged a whole [`LEASE`], or renewed
    /// [`STALE_MS`] ago by its own clock. A claim not watched yet is judged
    /// by its own time alone.
    pub fn claim_lapsed(&self, claim: &Claim, now_ms: i64) -> bool {
        match &self.claim {
            Some((held, _)) if held == claim => self.lapsed,
            _ => claim.released || now_ms - claim.renewed_at_ms >= STALE_MS,
        }
    }

    /// How long until the claim watched now lapses with nothing changing:
    /// when a header naming its holder must be drawn again. `None` with no
    /// claim, or one released, without a holder or lapsed already.
    pub fn lapses_in(&self, now: Instant, now_ms: i64) -> Option<Duration> {
        let (claim, since) = self.claim.as_ref()?;
        if self.lapsed || claim.released || claim.holder_client.is_empty() {
            return None;
        }
        let watched = LEASE.saturating_sub(now.saturating_duration_since(*since));
        let stale = Duration::from_millis(
            u64::try_from(claim.renewed_at_ms + STALE_MS - now_ms).unwrap_or(0),
        );
        Some(watched.min(stale))
    }

    /// The viewer no longer reads the trace (the flag withdrawn, or no
    /// longer a member): every reasoning tail kept is dropped, so none is
    /// held, let alone shown, past that moment.
    pub fn forget_trace(&mut self) {
        for turn in self.turns.values_mut() {
            turn.reasoning = Tails::default();
        }
    }

    /// Settle what `seen` — the snapshot a view was drawn from — tells of
    /// the turns being written. A turn it held that the view no longer
    /// shows has ended (a view shows every held turn but those with their
    /// message), so it is forgotten. A turn it never held was observed
    /// later, by another stream of the session: not this view's to forget.
    /// A turn the view shows that this device had not seen yet — its
    /// chunks arrived before any document — is first seen as of `seen`.
    pub fn settle(&mut self, seen: &Live, live: &[AgentLiveTurnVm]) {
        let shown = |id: &String| live.iter().any(|turn| turn.turn_id == *id);
        self.turns
            .retain(|id, _| !seen.turns.contains_key(id) || shown(id));
        self.first
            .retain(|id, _| !seen.first.contains_key(id) || shown(id));
        if let Some(at) = seen.at {
            for turn in live {
                self.first.entry(turn.turn_id.clone()).or_insert(at);
            }
        }
    }

    /// Whether the instance writing turn `id` has stopped: its document has
    /// left and its claim lapsed, or went to another instance. With no
    /// claim row to judge by (not yet replicated, or gone), a turn whose
    /// document has been away a whole lease — or that has shown none for a
    /// lease since this device first saw it — is taken for stopped too, so
    /// no answer waits without an end. A turn seen for the first time now
    /// has had no time to show anything.
    fn stopped(&self, id: &str, author: Option<&str>) -> bool {
        let turn = self.turns.get(id);
        if turn.is_some_and(|turn| turn.present) {
            return false;
        }
        let Some((claim, _)) = &self.claim else {
            let quiet_since = turn
                .and_then(|turn| turn.seen)
                .or_else(|| self.first.get(id).copied());
            return match (quiet_since, self.at) {
                (Some(since), Some(at)) => at.saturating_duration_since(since) >= LEASE,
                _ => false,
            };
        };
        let writer = turn.and_then(|turn| turn.client.as_deref()).or(author);
        self.lapsed || writer.is_some_and(|writer| writer != claim.holder_client)
    }
}

// ---------------------------------------------------------------------------
// Reading a session's turns
// ---------------------------------------------------------------------------

/// What a session's view needs from the rows beside its messages.
pub struct Extras {
    /// Tool steps by turn; empty for a member who does not read the trace.
    pub steps: HashMap<String, Vec<AgentToolStepVm>>,
    /// Reasoning rows by turn: `(seq, compacted, text)`; empty unless shown.
    pub reasoning: HashMap<String, Vec<(i64, bool, String)>>,
    /// A reader who chose to see the thinking: only then is any drawn.
    pub thinking: bool,
}

pub fn extras(
    source: &Source,
    session_id: &str,
    reads_trace: bool,
    show_reasoning: bool,
) -> Extras {
    let mut steps: HashMap<String, Vec<AgentToolStepVm>> = HashMap::new();
    let mut reasoning: HashMap<String, Vec<(i64, bool, String)>> = HashMap::new();
    if reads_trace {
        for row in read(
            source,
            "SELECT id, turn_id, name, status, result, started_at_ms, ended_at_ms FROM tool_calls \
             WHERE session_id = ?1 ORDER BY created_at_ms, id",
            &[Value::from(session_id)],
        ) {
            let (Some(id), Some(turn)) = (opt_text(&row, "id"), opt_text(&row, "turn_id")) else {
                continue;
            };
            let status = match text(&row, "status") {
                "done" => AgentToolStepStatusVm::Done,
                "refused" => AgentToolStepStatusVm::Refused,
                "interrupted" => AgentToolStepStatusVm::Interrupted,
                "parked" => AgentToolStepStatusVm::Parked,
                _ => AgentToolStepStatusVm::Started,
            };
            steps.entry(turn).or_default().push(AgentToolStepVm {
                id,
                name: text(&row, "name").to_owned(),
                status_label: status.label().to_owned(),
                status,
                result: row.get("result").and_then(Value::as_str).map(str::to_owned),
                started_at_ms: int(&row, "started_at_ms"),
                ended_at_ms: int(&row, "ended_at_ms"),
            });
        }
        if show_reasoning {
            for row in read(
                source,
                "SELECT turn_id, seq, compacted, text FROM reasoning WHERE session_id = ?1",
                &[Value::from(session_id)],
            ) {
                let Some(turn) = opt_text(&row, "turn_id") else {
                    continue;
                };
                reasoning.entry(turn).or_default().push((
                    int(&row, "seq").unwrap_or(-1),
                    truthy(&row, "compacted"),
                    text(&row, "text").to_owned(),
                ));
            }
        }
    }
    Extras {
        steps,
        reasoning,
        thinking: reads_trace && show_reasoning,
    }
}

/// A turn's thinking: its compacted row once it has one, otherwise its
/// rows from seq 1 and the tail after them.
fn thinking(rows: Option<&Vec<(i64, bool, String)>>, tails: &Tails) -> String {
    let rows = rows.map(Vec::as_slice).unwrap_or_default();
    if let Some((_, _, all)) = rows.iter().find(|(_, compacted, _)| *compacted) {
        return all.clone();
    }
    fold(
        rows.iter().map(|(seq, _, text)| (*seq, text.as_str())),
        1,
        tails,
    )
}

/// An ended turn's thinking, for a reader who chose to see it.
pub fn ended_thinking(extras: &Extras, turn_id: &str) -> Option<String> {
    let rows = extras.reasoning.get(turn_id)?;
    Some(thinking(Some(rows), &Tails::default())).filter(|text| !text.is_empty())
}

/// Runtime Closed rows always include `finish`, even when the provider
/// reported no finish reason. A persisted lead-in has no such marker.
pub(super) fn is_terminal(metadata: &Value) -> bool {
    metadata.get("finish").is_some()
}

/// The turns of `session_id` still being written, oldest first chunk
/// first (presence-only turns, with no chunk yet, last). Only a terminal
/// agent row suppresses a turn through `ended`.
pub(super) fn turns(
    source: &Source,
    session_id: &str,
    live: &Live,
    ended: &HashSet<String>,
    extras: &Extras,
) -> Vec<AgentLiveTurnVm> {
    struct Chunks {
        pieces: Vec<(i64, String)>,
        author: Option<String>,
        first_ms: i64,
    }
    let mut chunks: HashMap<String, Chunks> = HashMap::new();
    for row in read(
        source,
        "SELECT turn_id, seq, text, author_client, created_at_ms FROM turn_chunks \
         WHERE session_id = ?1",
        &[Value::from(session_id)],
    ) {
        let Some(turn) = opt_text(&row, "turn_id") else {
            continue;
        };
        let at = int(&row, "created_at_ms").unwrap_or(i64::MAX);
        let entry = chunks.entry(turn).or_insert_with(|| Chunks {
            pieces: Vec::new(),
            author: None,
            first_ms: at,
        });
        entry.first_ms = entry.first_ms.min(at);
        entry.author = entry
            .author
            .take()
            .or_else(|| opt_text(&row, "author_client"));
        if let Some(seq) = int(&row, "seq") {
            entry.pieces.push((seq, text(&row, "text").to_owned()));
        }
    }
    let mut ids: Vec<(i64, &str)> = chunks
        .iter()
        .map(|(id, c)| (c.first_ms, id.as_str()))
        .chain(
            live.turns
                .keys()
                .filter(|id| !chunks.contains_key(*id))
                .map(|id| (i64::MAX, id.as_str())),
        )
        .filter(|(_, id)| !ended.contains(*id))
        .collect();
    ids.sort_unstable();
    let empty = Tails::default();
    ids.into_iter()
        .map(|(_, id)| {
            let turn = live.turns.get(id);
            let held = chunks.get(id);
            let text = fold(
                held.iter()
                    .flat_map(|c| c.pieces.iter().map(|(seq, text)| (*seq, text.as_str()))),
                0,
                turn.map_or(&empty, |turn| &turn.text),
            );
            let reasoning = extras
                .thinking
                .then(|| {
                    thinking(
                        extras.reasoning.get(id),
                        turn.map_or(&empty, |t| &t.reasoning),
                    )
                })
                .filter(|text| !text.is_empty());
            AgentLiveTurnVm {
                turn_id: id.to_owned(),
                text,
                tool: turn.and_then(|turn| turn.tool.clone()),
                reasoning,
                steps: extras.steps.get(id).cloned().unwrap_or_default(),
                stopped: live.stopped(id, held.and_then(|c| c.author.as_deref())),
            }
        })
        .collect()
}

/// What an answer's `metadata` says the provider reported; `None` when it
/// reported nothing.
pub fn details(metadata: &Value) -> Option<AgentMessageDetailsVm> {
    let usage = metadata.get("usage");
    let number = |value: Option<&Value>| value.and_then(Value::as_i64);
    let string = |value: Option<&Value>| {
        value
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    let details = AgentMessageDetailsVm {
        model: string(metadata.get("model")),
        prompt_tokens: number(usage.and_then(|u| u.get("prompt_tokens"))),
        completion_tokens: number(usage.and_then(|u| u.get("completion_tokens"))),
        total_tokens: number(usage.and_then(|u| u.get("total_tokens"))),
        ttft_ms: number(metadata.get("ttft_ms")),
        duration_ms: number(metadata.get("duration_ms")),
        finish: string(metadata.get("finish")),
        error: string(metadata.get("error")),
    };
    (details != AgentMessageDetailsVm::default()).then_some(details)
}

// ---------------------------------------------------------------------------
// Tool switches
// ---------------------------------------------------------------------------

/// The tools the session's agent offers: what the claim holder's profile
/// says (`agent_profiles.tools`); before any instance holds it, or while
/// the holder's row says nothing (an instance older than the column), what
/// every instance of the agent that says anything offers in common. Empty
/// while no instance has said.
pub fn offered(source: &Source, agent_actor: &str, holder: Option<&str>) -> Vec<String> {
    let profiles: Vec<(String, Option<Vec<String>>)> = read(
        source,
        "SELECT id, tools FROM agent_profiles WHERE actor_id = ?1 ORDER BY id",
        &[Value::from(agent_actor)],
    )
    .into_iter()
    .filter_map(|row| {
        let id = opt_text(&row, "id")?;
        let tools = json(&row, "tools").is_array().then(|| names(&row, "tools"));
        Some((id, tools))
    })
    .collect();
    if let Some((_, Some(tools))) = holder.and_then(|h| profiles.iter().find(|(id, _)| id == h)) {
        return tools.clone();
    }
    let mut said = profiles.into_iter().filter_map(|(_, tools)| tools);
    let Some(mut common) = said.next() else {
        return Vec::new();
    };
    for tools in said {
        common.retain(|tool| tools.contains(tool));
    }
    common
}

/// The tool names switched off in a session, and the row's version.
fn switched_off(source: &Source, session_id: &str) -> (Vec<String>, Option<i64>) {
    let rows = read(
        source,
        "SELECT tools_off, _sync_version AS version FROM session_tool_switches \
         WHERE session_id = ?1",
        &[Value::from(session_id)],
    );
    match rows.first() {
        Some(row) => (names(row, "tools_off"), int(row, "version")),
        None => (Vec::new(), None),
    }
}

/// The session's tools: each one the agent offers, on unless switched off.
/// A tool the agent does not offer is absent, never shown as off.
pub fn tools(source: &Source, session_id: &str, agent_actor: &str) -> Vec<AgentSessionToolVm> {
    let holder = claim(source, session_id).map(|claim| claim.holder_client);
    let (off, _) = switched_off(source, session_id);
    offered(source, agent_actor, holder.as_deref())
        .into_iter()
        .map(|name| AgentSessionToolVm {
            on: !off.contains(&name),
            name,
        })
        .collect()
}

/// Why a switch was not written, as the person reads it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct SwitchRefused(pub String);

/// Switch one offered tool off or back on for a session: the row names only
/// what is off (V12), so switching on removes the name and nothing can
/// offer a tool the agent does not. Written on the version read, so two
/// devices switching at once conflict rather than overwrite.
pub fn set_tool(
    source: &Source,
    session_id: &str,
    agent_actor: &str,
    name: &str,
    on: bool,
    now_ms: i64,
) -> Result<(), SwitchRefused> {
    let holder = claim(source, session_id).map(|claim| claim.holder_client);
    if !offered(source, agent_actor, holder.as_deref())
        .iter()
        .any(|tool| tool == name)
    {
        return Err(SwitchRefused(format!(
            "{name} is not one of the tools offered in this session."
        )));
    }
    let (mut off, version) = switched_off(source, session_id);
    let was_off = off.iter().any(|tool| tool == name);
    if was_off != on {
        return Ok(());
    }
    if on {
        off.retain(|tool| tool != name);
    } else {
        off.push(name.to_owned());
    }
    let row = RowOp::Upsert {
        table: "session_tool_switches".to_owned(),
        values: authored(
            source,
            object(serde_json::json!({
                "session_id": session_id,
                "tools_off": Value::from(off).to_string(),
                "updated_at_ms": now_ms,
            })),
        ),
        base_version: version.filter(|v| *v > 0),
    };
    source
        .replica
        .mutate(vec![row])
        .map_err(|error| SwitchRefused(error.to_string()))?;
    source.replica.sync_now();
    Ok(())
}

#[cfg(test)]
#[path = "live_tests.rs"]
mod tests;
