//! A question spoken to one of the person's agents, and its answer followed
//! (AD-384, AD-411; story 108.2, R-HQ-67, R-HQ-68).
//!
//! *Speak to* names an agent of the databases this device holds
//! (`bots.voice_target = agent:<agent>`, [`agent_target`]); the list it is
//! chosen from is every agent of every held database ([`targets`]). Before
//! 108.2 the value named one of the person's proxy rooms on Matrix
//! (`agent:!room…`); such a value is read as no choice, once, with a notice
//! ([`is_old_room_target`], UX-DR-E108-1).
//!
//! - **Where the question goes.** The agent's main conversation: its newest
//!   session that is not archived in any held database — the one launch and
//!   the docks open (106.5) — or, with none, a new session in the database
//!   the person last chose for it (else the first that names it)
//!   ([`main_session`]). The words the voice turn heard are a person
//!   `messages` row there, written into the replica's outbox like any typed
//!   message ([`Hq::spoken_send`]); recognition never left the device, and
//!   no audio or partial transcript is written anywhere.
//! - **Which answer.** An agent's turn names the person's message it
//!   answers: its `turn_id` is that message's id (`turn_chunks` while it
//!   streams, the agent's `messages` row once it is whole). Another
//!   question's answer, or the same words asked from another device, names
//!   another message.
//! - **What is heard.** The turn's text as the viewer folds it (106.3's
//!   [`crate::hq::live`]) goes to an [`AnswerFollower`], which hands out each
//!   closed sentence once; the agent's row, when it lands, completes it.
//! - **What is not waited for forever.** No answer within
//!   [`ANSWER_START_WAIT`] of the send, or nothing new for
//!   [`ANSWER_QUIET_WAIT`] once it answers, or an instance that stopped
//!   without ending the turn, ends the turn with a sentence naming the agent.
//! - **Stopping.** The stop phrase stops speech on this device; the agent's
//!   turn is not cancelled (R44). Where the person stopped is a person
//!   `messages` row whose metadata is `{"kind": "heard", …}` and exactly the
//!   `heard_until` object the contract names ([`HeardMark::metadata`],
//!   [`Hq::spoken_heard`]): the claim holder copies it into
//!   `sessions.heard_until` and cuts the answer there in the next turn's
//!   context (R-HQ-68). The Agents window draws no heard row
//!   ([`crate::hq::sessions::is_heard`]).
//!
//! The shell's `hq_ipc` drives the follow on its runtime and tells the voice
//! turn each [`SpokenStep`]; no `voice*.rs` file reaches the network.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::agents::log::HeardReason;
use crate::hq::live::AgentLiveTurnVm;
use crate::hq::port::RowOp;
use crate::hq::sessions::{
    self, authored, json as json_column, now_ms, object, page_key, read, text, AgentSessionRef,
    Source,
};
use crate::hq::{Hq, HqError};
use crate::vm::VoiceAgentTargetVm;
use crate::voice::backchannel::Utterance;
use crate::voice::speech::{AnswerFollower, Sentence};
use crate::voice::{Effect, TurnEvent, TurnState};

/// The `kind` of a person's heard row's metadata (R-HQ-68).
pub const HEARD_KIND: &str = "heard";

/// How `bots.voice_target` names an agent: this prefix and its actor id.
pub const AGENT_TARGET_PREFIX: &str = "agent:";

/// How long a spoken question waits for its answer to start.
pub const ANSWER_START_WAIT: Duration = Duration::from_secs(60);

/// How long an answer that started may bring nothing new before the turn
/// gives up on it.
pub const ANSWER_QUIET_WAIT: Duration = Duration::from_secs(120);

/// How long a spoken question waits for the heard mark written before it
/// (Q10). The mark is best-effort and the question is what the person is
/// waiting on: past this the question goes, and the mark, still writing,
/// cuts the turns after it.
pub const HEARD_MARK_WAIT: Duration = Duration::from_secs(10);

/// The sentence a spoken turn is refused with when the agent chosen under
/// *Speak to* is in none of the databases this device holds.
pub const AGENT_GONE_SENTENCE: &str =
    "The agent chosen under Speak to is in none of the agents' databases this device holds: choose again under Speak to.";

/// The notice shown once when *Speak to* named one of the agents' old
/// Matrix rooms (UX-DR-E108-1): the choice is read as unset.
pub const OLD_ROOM_TARGET_NOTICE: &str =
    "Speak to named a conversation of your assistant on Matrix, which keeper no longer uses. Choose an agent under Speak to.";

/// The stored value that names `agent`.
pub fn agent_target(agent: &str) -> String {
    format!("{AGENT_TARGET_PREFIX}{agent}")
}

/// The agent a stored value names, when it names one: never one of the
/// agents' old Matrix rooms (`agent:!room…`).
pub fn target_agent(chosen: &str) -> Option<&str> {
    chosen
        .trim()
        .strip_prefix(AGENT_TARGET_PREFIX)
        .map(str::trim)
        .filter(|agent| !agent.is_empty() && !agent.starts_with('!'))
}

/// Whether a stored value names one of the agents' old Matrix rooms, as
/// *Speak to* stored them before 108.2.
pub fn is_old_room_target(chosen: &str) -> bool {
    chosen
        .trim()
        .strip_prefix(AGENT_TARGET_PREFIX)
        .is_some_and(|rest| rest.trim().starts_with('!'))
}

/// *Speak to*'s agents: every agent of every held database, by name, each
/// with the value that chooses it.
pub fn targets(choices: &[sessions::AgentChoiceVm]) -> Vec<VoiceAgentTargetVm> {
    choices
        .iter()
        .map(|choice| VoiceAgentTargetVm {
            target: agent_target(&choice.agent_actor),
            agent: choice.agent_actor.clone(),
            name: choice.agent_name.clone(),
        })
        .collect()
}

/// Where a spoken question to an agent goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MainSession {
    /// The agent's newest session that is not archived.
    Existing(AgentSessionRef),
    /// None yet: a new session in this database.
    New { database: String },
}

/// The agent's main conversation over `sources`, or where a new one goes;
/// `None` when no held database names the agent.
pub fn main_session(
    sources: &[Source],
    agent: &str,
    last: &HashMap<String, String>,
    now: i64,
) -> Option<MainSession> {
    let choice = sessions::choices(sources, last)
        .into_iter()
        .find(|choice| choice.agent_actor == agent)?;
    if let Some(row) = sessions::list(sources, now)
        .into_iter()
        .find(|row| row.agent_actor == agent && !row.archived)
    {
        return Some(MainSession::Existing(AgentSessionRef {
            database: row.database,
            session_id: row.session_id,
        }));
    }
    let database = choice
        .database
        .or_else(|| choice.databases.first().cloned())?;
    Some(MainSession::New { database })
}

/// A spoken question written: where, its message id, and the agent's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpokenAsk {
    pub key: AgentSessionRef,
    pub question: String,
    pub agent_name: String,
}

/// What the voice turn is told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpokenStep {
    /// The answer's first words arrived, `after_ms` after the question was
    /// written.
    FirstText { after_ms: u64 },
    /// The answer to the question began: where the person stops listening
    /// is marked against it ([`HeardMark`]).
    Anchored,
    /// One closed sentence of the answer, in order, once, with its end in
    /// the answer's text as it read when the sentence closed.
    Sentence(Sentence),
    /// The answer is whole: the words after its last sentence, if any.
    Complete(Option<Sentence>),
    /// No answer, or no more of one: the sentence the turn ends on.
    Failed(String),
}

/// The agent's row for the question's turn, as far as speaking it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnswerRow {
    pub body: String,
    /// The runtime wrote an error for the turn (`metadata.error`).
    pub failed: bool,
}

/// One spoken question's answer, read from the session's rows and the
/// live turns (pure).
#[derive(Debug)]
pub struct SpokenWatch {
    question: String,
    agent_name: String,
    follower: AnswerFollower,
    asked: Instant,
    /// When the answer last brought something new; `None` before it began.
    moved: Option<Instant>,
    text_len: usize,
    first_text: bool,
    done: bool,
}

impl SpokenWatch {
    /// A watch for the question `question` (its message id) asked of
    /// `agent_name` at `now`.
    pub fn new(question: String, agent_name: String, now: Instant) -> Self {
        SpokenWatch {
            question,
            agent_name,
            follower: AnswerFollower::new(),
            asked: now,
            moved: None,
            text_len: 0,
            first_text: false,
            done: false,
        }
    }

    /// The question's message id: what its answer's turn names.
    pub fn question(&self) -> &str {
        &self.question
    }

    /// Whether the answer is whole, or the turn ended without one.
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// What the session shows now: its live turns and the agent's row for
    /// the question's turn, if it landed.
    pub fn see(
        &mut self,
        live: &[AgentLiveTurnVm],
        answer: Option<&AnswerRow>,
        now: Instant,
    ) -> Vec<SpokenStep> {
        if self.done {
            return Vec::new();
        }
        let mut steps = Vec::new();
        if let Some(answer) = answer {
            self.grow(&answer.body, now, &mut steps);
            self.done = true;
            if answer.failed {
                steps.push(SpokenStep::Failed(format!(
                    "{} could not answer.",
                    self.agent_name
                )));
            } else {
                steps.push(SpokenStep::Complete(self.follower.finish()));
            }
            return steps;
        }
        let Some(turn) = live.iter().find(|turn| turn.turn_id == self.question) else {
            return steps;
        };
        self.grow(&turn.text, now, &mut steps);
        if turn.stopped {
            self.done = true;
            steps.push(SpokenStep::Failed(format!(
                "{} stopped answering.",
                self.agent_name
            )));
        }
        steps
    }

    fn grow(&mut self, text: &str, now: Instant, steps: &mut Vec<SpokenStep>) {
        if self.moved.is_none() {
            self.moved = Some(now);
            steps.push(SpokenStep::Anchored);
        }
        if text.trim().is_empty() {
            return;
        }
        if !self.first_text {
            self.first_text = true;
            let after = now.saturating_duration_since(self.asked);
            steps.push(SpokenStep::FirstText {
                after_ms: u64::try_from(after.as_millis()).unwrap_or(u64::MAX),
            });
        }
        if text.len() != self.text_len {
            self.text_len = text.len();
            self.moved = Some(now);
        }
        steps.extend(
            self.follower
                .edit(&self.question, text)
                .into_iter()
                .map(SpokenStep::Sentence),
        );
    }

    /// When the turn gives up waiting, if nothing comes first.
    pub fn deadline(&self) -> Option<Instant> {
        if self.done {
            return None;
        }
        Some(match self.moved {
            None => self.asked + ANSWER_START_WAIT,
            Some(moved) => moved + ANSWER_QUIET_WAIT,
        })
    }

    /// The deadline passed at `now`: the sentence the turn ends on.
    pub fn expire(&mut self, now: Instant) -> Option<SpokenStep> {
        if self.deadline().is_none_or(|at| now < at) {
            return None;
        }
        self.done = true;
        let name = &self.agent_name;
        Some(SpokenStep::Failed(if self.moved.is_some() {
            format!("{name} stopped answering.")
        } else {
            format!("{name} has not answered: no instance of it may be running right now.")
        }))
    }
}

/// How far the person heard one spoken agent answer (AD-411; pure): the
/// session, the question its answer's turn names, the end of each sentence
/// handed to the synthesiser in the order it was handed, and how many of
/// them the synthesiser read to their end. A sentence it was cut off in
/// does not count (Q3): an agent told the person heard less repeats a
/// little; one told they heard more skips what they never heard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeardMark {
    key: AgentSessionRef,
    question: String,
    answered: bool,
    ends: Vec<usize>,
    finished: usize,
}

impl HeardMark {
    /// A mark for the question `question` asked in session `key`.
    pub fn new(key: AgentSessionRef, question: String) -> Self {
        HeardMark {
            key,
            question,
            answered: false,
            ends: Vec::new(),
            finished: 0,
        }
    }

    /// The session the question was asked in.
    pub fn key(&self) -> &AgentSessionRef {
        &self.key
    }

    /// The answer to the question began.
    pub fn anchored(&mut self) {
        self.answered = true;
    }

    /// A sentence ending at `end` in the answer's text was handed to the
    /// synthesiser, after every one before it.
    pub fn handed(&mut self, end: usize) {
        self.ends.push(end);
    }

    /// The synthesiser read one more of the handed sentences to its end.
    pub fn spoken(&mut self) {
        self.finished = (self.finished + 1).min(self.ends.len());
    }

    /// The heard row's metadata for `reason` (R-HQ-68): `{"kind": "heard"}`
    /// and the `heard_until` object — the answer (by the question's id,
    /// which its turn names), where what was heard ends in it in Unicode
    /// scalar values, how many sentences were finished aloud, and why speech
    /// stopped. `None` while the answer has not begun: nothing of it was
    /// heard, and there is nothing to mark against.
    pub fn metadata(&self, reason: HeardReason) -> Option<Value> {
        if !self.answered {
            return None;
        }
        let heard_until = match self.finished {
            0 => 0,
            n => self.ends.get(n - 1).copied().unwrap_or(0),
        };
        Some(json!({
            "kind": HEARD_KIND,
            "message": self.question,
            "heard_until": u64::try_from(heard_until).unwrap_or(u64::MAX),
            "sentence": u32::try_from(self.finished).unwrap_or(u32::MAX),
            "reason": reason,
        }))
    }

    /// Whether a voice turn's move — from `before` on `event` as the table
    /// was handed it, handing out `effects` — stopped an answer the person
    /// was hearing, and why: the stop phrase, a stop by hand, or speech
    /// that stopped the answer with nothing to ask is `stop`, a question
    /// asked over the answer `barge_in`. An answer heard to its end, or one
    /// a failure ended, is marked nowhere.
    pub fn reason(
        before: &TurnState,
        event: &TurnEvent,
        effects: &[Effect],
    ) -> Option<HeardReason> {
        if !matches!(before, TurnState::Speaking | TurnState::Paused { .. })
            || !effects.contains(&Effect::StopSpeaking)
        {
            return None;
        }
        match event {
            TurnEvent::StopHeard | TurnEvent::Abandoned => Some(HeardReason::Stop),
            TurnEvent::Resolved(Utterance::Question(text)) if !text.trim().is_empty() => {
                Some(HeardReason::BargeIn)
            }
            TurnEvent::Resolved(Utterance::Question(_)) => Some(HeardReason::Stop),
            _ => None,
        }
    }
}

/// Which question's answer a device is following: at most one, the voice
/// turn's current question's (pure). Two sends may finish in either order,
/// and a send may finish after the person stopped or asked again; only the
/// current question's follower is kept.
#[derive(Debug)]
pub struct FollowSlot<H> {
    held: Option<(u64, H)>,
}

impl<H> Default for FollowSlot<H> {
    fn default() -> Self {
        FollowSlot::new()
    }
}

impl<H> FollowSlot<H> {
    /// An empty slot.
    pub const fn new() -> Self {
        FollowSlot { held: None }
    }

    /// A follower for `question` is ready and the voice turn's question is
    /// `current`. Returns the follower to stop: the one it replaces, or this
    /// one itself when its question is no longer the turn's.
    pub fn install(&mut self, question: u64, current: Option<u64>, follower: H) -> Option<H> {
        if current != Some(question) {
            return Some(follower);
        }
        self.held
            .replace((question, follower))
            .map(|(_, earlier)| earlier)
    }

    /// The turn's question is now `current`: a follower for any other one is
    /// returned, to stop.
    pub fn keep_only(&mut self, current: Option<u64>) -> Option<H> {
        match &self.held {
            Some((question, _)) if Some(*question) == current => None,
            _ => self.held.take().map(|(_, follower)| follower),
        }
    }
}

/// The agent's row for `question`'s turn in `session_id`, if it landed.
fn answer_row(source: &Source, session_id: &str, question: &str) -> Option<AnswerRow> {
    let rows = read(
        source,
        "SELECT body, metadata FROM messages WHERE session_id = ?1 AND role = 'agent' \
         AND turn_id = ?2 ORDER BY created_at_ms DESC, id DESC LIMIT 1",
        &[Value::from(session_id), Value::from(question)],
    );
    let row = rows.first()?;
    Some(AnswerRow {
        body: text(row, "body").to_owned(),
        failed: json_column(row, "metadata")
            .get("error")
            .is_some_and(|error| !error.is_null()),
    })
}

/// A person message row `id` in `session_id`, written `at`, with `metadata`.
fn person_row(source: &Source, session_id: &str, id: &str, body: &str, metadata: Value, at: i64) -> RowOp {
    RowOp::Upsert {
        table: "messages".to_owned(),
        values: authored(
            source,
            object(json!({
                "id": id,
                "session_id": session_id,
                "page_key": page_key(session_id, &sessions::month(at)),
                "turn_id": null,
                "role": "person",
                "body": body,
                "body_blob": null,
                "metadata": metadata,
                "created_at_ms": at,
            })),
        ),
        base_version: None,
    }
}

impl Hq {
    /// *Speak to*'s agents (AD-384, 108.2): every agent of every database
    /// this device holds.
    pub fn voice_agent_targets(&self) -> Result<Vec<VoiceAgentTargetVm>, HqError> {
        Ok(targets(&self.choices()?))
    }

    /// Write what the voice turn heard into `agent`'s main conversation
    /// ([`main_session`]) as the person's own message, and answer where it
    /// went. Refused, with [`AGENT_GONE_SENTENCE`], when no held database
    /// names the agent; an empty question is never written.
    pub fn spoken_send(&self, agent: &str, body: &str) -> Result<SpokenAsk, HqError> {
        if body.trim().is_empty() {
            return Err(HqError::Sentence("Nothing was heard to send.".to_owned()));
        }
        let sources = self.sources();
        let last = sessions::last_choices(&self.data_dir()?);
        let agent_name = sessions::choices(&sources, &last)
            .into_iter()
            .find(|choice| choice.agent_actor == agent)
            .map(|choice| choice.agent_name)
            .ok_or_else(|| HqError::Sentence(AGENT_GONE_SENTENCE.to_owned()))?;
        let question = ulid::Ulid::new().to_string();
        let key = match main_session(&sources, agent, &last, now_ms())
            .ok_or_else(|| HqError::Sentence(AGENT_GONE_SENTENCE.to_owned()))?
        {
            MainSession::Existing(key) => {
                let source = self.source(&key.database)?;
                let at = now_ms();
                let message = person_row(&source, &key.session_id, &question, body, Value::Null, at);
                let activity = crate::hq::send::activity(&source, &key.session_id, at);
                source
                    .replica
                    .mutate(vec![message, activity])
                    .map_err(|error| HqError::Sentence(error.to_string()))?;
                source.replica.sync_now();
                key
            }
            MainSession::New { database } => {
                let source = self.source(&database)?;
                let session_id = sessions::create_with_first(&source, agent, body, &question)
                    .map_err(|error| HqError::Sentence(error.to_string()))?;
                AgentSessionRef {
                    database,
                    session_id,
                }
            }
        };
        self.touch();
        Ok(SpokenAsk {
            key,
            question,
            agent_name,
        })
    }

    /// Write where the person stopped hearing a spoken answer (R-HQ-68): a
    /// person `messages` row in `key` with `metadata`
    /// ([`HeardMark::metadata`]) and no words. The claim holder copies it
    /// into `sessions.heard_until`; nothing else reads it as a message.
    pub fn spoken_heard(&self, key: &AgentSessionRef, metadata: Value) -> Result<(), HqError> {
        if metadata.get("kind").and_then(Value::as_str) != Some(HEARD_KIND) {
            return Err(HqError::Sentence("That is no heard mark.".to_owned()));
        }
        let source = self.source(&key.database)?;
        let at = now_ms();
        let id = ulid::Ulid::new().to_string();
        source
            .replica
            .mutate(vec![person_row(&source, &key.session_id, &id, "", metadata, at)])
            .map_err(|error| HqError::Sentence(error.to_string()))?;
        source.replica.sync_now();
        self.touch();
        Ok(())
    }

    /// Follow the answer to `ask`: the session is held open while the
    /// returned follower lives, so its live turns are read.
    pub fn spoken_answer(self: &Arc<Self>, ask: &SpokenAsk) -> Result<SpokenAnswer, HqError> {
        self.open_session(&ask.key)?;
        Ok(SpokenAnswer {
            hq: Arc::clone(self),
            key: ask.key.clone(),
            watch: SpokenWatch::new(ask.question.clone(), ask.agent_name.clone(), Instant::now()),
            queued: VecDeque::new(),
            changes: self.changes(),
            live: self.live_changes(),
        })
    }
}

/// A spoken question's answer as the session shows it: the watch, fed from
/// the session's view whenever the replicas or a live tail move.
pub struct SpokenAnswer {
    hq: Arc<Hq>,
    key: AgentSessionRef,
    watch: SpokenWatch,
    queued: VecDeque<SpokenStep>,
    changes: tokio::sync::watch::Receiver<u64>,
    live: tokio::sync::watch::Receiver<u64>,
}

impl SpokenAnswer {
    /// The agent's name.
    pub fn agent_name(&self) -> &str {
        &self.watch.agent_name
    }

    /// The next step the voice turn hears; `None` once the answer is whole
    /// or the turn ended without one.
    pub async fn next(&mut self) -> Option<SpokenStep> {
        loop {
            if let Some(step) = self.queued.pop_front() {
                return Some(step);
            }
            if self.watch.is_done() {
                return None;
            }
            let hq = Arc::clone(&self.hq);
            let key = self.key.clone();
            let question = self.watch.question().to_owned();
            let read = tokio::task::spawn_blocking(move || {
                let view = hq.session_view(&key).ok();
                let answer = hq
                    .source(&key.database)
                    .ok()
                    .and_then(|source| answer_row(&source, &key.session_id, &question));
                (view.map(|view| view.live).unwrap_or_default(), answer)
            })
            .await;
            let (live, answer) = read.unwrap_or_default();
            let now = Instant::now();
            self.queued
                .extend(self.watch.see(&live, answer.as_ref(), now));
            if !self.queued.is_empty() || self.watch.is_done() {
                continue;
            }
            let deadline = self.watch.deadline().map(tokio::time::Instant::from_std);
            let recheck = tokio::time::sleep(crate::hq::live::RECHECK);
            tokio::select! {
                _ = self.changes.changed() => {}
                _ = self.live.changed() => {}
                () = recheck => {}
                () = async {
                    match deadline {
                        Some(at) => tokio::time::sleep_until(at).await,
                        None => std::future::pending().await,
                    }
                } => {
                    if let Some(step) = self.watch.expire(Instant::now()) {
                        self.queued.push_back(step);
                    }
                }
            }
        }
    }
}

impl Drop for SpokenAnswer {
    fn drop(&mut self) {
        self.hq.close_session(&self.key);
    }
}

#[cfg(test)]
#[path = "spoken_tests.rs"]
mod tests;
