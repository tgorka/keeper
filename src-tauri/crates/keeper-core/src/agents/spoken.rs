//! A question spoken to the person's own agent, and its answer followed
//! (AD-384, ruling R31).
//!
//! The words the voice turn heard go into one of the person's proxy rooms
//! as their own message (`account::spoken_send`, the send gate's third
//! trigger). The answer comes back the way every agent answer does
//! (AD-373): one message from the agent carrying `dev.keeper.agent.turn` —
//! the anchor — then edits replacing it with the whole text so far, and the
//! session's status going `idle` once the final edit is sent.
//! [`SpokenWatch`] reads the room's events in the order the room holds
//! them and says what the voice turn hears.
//!
//! - **Which answer (Q6).** The anchor whose `dev.keeper.agent.turn` names
//!   the question's own event — learned from the send queue when the send
//!   lands — from the room's agent, at an agent's power and not the person.
//!   Another question's answer, queued before this one or asked in the same
//!   words from another device, names another event.
//! - **What is heard.** Each edit of that anchor goes to an
//!   [`AnswerFollower`], which hands out each closed sentence once.
//! - **When it is whole.** The turn's own status says so: the first
//!   `running` after the anchor names the session, host and claim epoch
//!   answering, and only `idle` or `done` from that same one completes the
//!   answer. `blocked` (a claim lost), `waiting`, a status from another copy
//!   or one older than the anchor completes nothing.
//! - **Late keys.** The room's event cache is followed, not live event
//!   handlers: an event that could not be decrypted when it arrived is
//!   replaced in the cache once its key comes (SDK 0.18's redecryptor), and
//!   [`CacheFeed`] hands each event out once, in the room's order, holding
//!   back behind an agent's event still encrypted.
//! - **What is not waited for forever.** No anchor within
//!   [`ANSWER_START_WAIT`] of the send, or nothing from the turn for
//!   [`ANSWER_QUIET_WAIT`] once it answers, fails the turn with a sentence
//!   naming the agent.
//! - **Stopping (R44).** No event cancels an agent's turn: the stop phrase
//!   stops speech on this device (the voice turn's own rule) and the agent
//!   finishes its answer in the room. The next question drops the watch —
//!   [`FollowSlot`] keeps only the current question's.
//!
//! [`SpokenAnswer`] is the watch over one room's event cache.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use matrix_sdk::deserialized_responses::TimelineEvent;
use matrix_sdk::event_cache::{
    EventCacheDropHandles, RoomEventCache, RoomEventCacheSubscriber, RoomEventCacheUpdate,
};
use matrix_sdk::ruma::{MilliSecondsSinceUnixEpoch, OwnedTransactionId, OwnedUserId, UserId};
use matrix_sdk::send_queue::{LocalEcho, LocalEchoContent, RoomSendQueueUpdate, SendHandle};
use matrix_sdk::Room;
use matrix_sdk_ui::eyeball_im::{Vector, VectorDiff};
use serde_json::Value;
use tokio::sync::broadcast;

use crate::agents::events::{RunState, StatusContent, TurnRef, STATUS, TURN};
use crate::agents::room::holds_agent_power;
use crate::error::CoreError;
use crate::voice::speech::AnswerFollower;

/// How long a spoken question waits for its answer to start.
pub const ANSWER_START_WAIT: Duration = Duration::from_secs(60);

/// How long an answer that started may be silent before the turn gives up
/// on it. A tool call that runs long still edits the status, which counts.
pub const ANSWER_QUIET_WAIT: Duration = Duration::from_secs(120);

/// What the voice turn is told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpokenStep {
    /// The answer's first words arrived, `after_ms` after the question was
    /// handed to the send queue.
    FirstText { after_ms: u64 },
    /// One closed sentence of the answer, in order, once.
    Sentence(String),
    /// The answer is whole: the words after its last sentence, empty when
    /// there are none.
    Complete(String),
    /// No answer, or no more of one: the sentence the turn ends on.
    Failed(String),
}

/// The status stream that answers: whose `running` came first after the
/// anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Answering {
    session: String,
    host: String,
    epoch: u64,
}

impl Answering {
    fn of(status: &StatusContent) -> Answering {
        Answering {
            session: status.session.clone(),
            host: status.host.clone(),
            epoch: status.epoch,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Stage {
    /// The question is on its way; its event id is not known yet. What the
    /// room delivers meanwhile waits for it.
    Sent(Vec<(Value, bool)>),
    /// The question is the event with this id; no answer yet.
    Asked(String),
    /// Following this anchor, sent at `since` (the room's clock).
    Following {
        anchor: String,
        since: u64,
        answering: Option<Answering>,
    },
    Done,
}

/// One spoken question's answer, read from its room's events (pure).
#[derive(Debug)]
pub struct SpokenWatch {
    me: OwnedUserId,
    agent: OwnedUserId,
    agent_name: String,
    stage: Stage,
    follower: AnswerFollower,
    sent_at: Instant,
    heard_at: Instant,
    answering: bool,
}

impl SpokenWatch {
    /// A watch for a question `me` hands to the send queue at `now`, in a
    /// room whose agent is `agent`, called `agent_name` in what the person
    /// is told.
    pub fn new(me: OwnedUserId, agent: OwnedUserId, agent_name: String, now: Instant) -> Self {
        SpokenWatch {
            me,
            agent,
            agent_name,
            stage: Stage::Sent(Vec::new()),
            follower: AnswerFollower::new(),
            sent_at: now,
            heard_at: now,
            answering: false,
        }
    }

    /// The agent's name, as the person is told it.
    pub fn agent_name(&self) -> &str {
        &self.agent_name
    }

    /// Whether the answer is complete or failed.
    pub fn is_done(&self) -> bool {
        self.stage == Stage::Done
    }

    /// Whether the question's event id is still to come.
    pub fn awaits_question(&self) -> bool {
        matches!(self.stage, Stage::Sent(_))
    }

    /// The question landed as `question`: what the room delivered before
    /// that is read now, in its order.
    pub fn asked(&mut self, question: &str, now: Instant) -> Vec<SpokenStep> {
        let Stage::Sent(early) =
            std::mem::replace(&mut self.stage, Stage::Asked(question.to_owned()))
        else {
            return Vec::new();
        };
        let mut steps = Vec::new();
        for (event, power) in early {
            steps.extend(self.see(&event, power, now));
        }
        steps
    }

    /// The send queue gave the question up: the turn ends on a sentence.
    pub fn not_sent(&mut self) -> Option<SpokenStep> {
        if !self.awaits_question() {
            return None;
        }
        self.stage = Stage::Done;
        Some(SpokenStep::Failed(format!(
            "Your question did not reach {}.",
            self.agent_name
        )))
    }

    /// Read one event of the room, as JSON with `type`, `sender`,
    /// `event_id`, `origin_server_ts` and `content`; `agent_power` says
    /// whether its sender holds an agent's power in the room.
    pub fn see(&mut self, event: &Value, agent_power: bool, now: Instant) -> Vec<SpokenStep> {
        let sender = event["sender"].as_str();
        let content = &event["content"];
        let relation = &content["m.relates_to"];
        let at = event["origin_server_ts"].as_u64().unwrap_or(0);
        let from_agent =
            agent_power && sender == Some(self.agent.as_str()) && self.agent != self.me;
        if !from_agent {
            return Vec::new();
        }
        match (event["type"].as_str(), &mut self.stage) {
            (_, Stage::Sent(early)) => {
                early.push((event.clone(), agent_power));
                Vec::new()
            }
            (Some("m.room.message"), Stage::Asked(question)) if relation.is_null() => {
                let answers = serde_json::from_value::<TurnRef>(content[TURN].clone())
                    .ok()
                    .and_then(|turn| turn.question)
                    .is_some_and(|asked| asked.as_str() == question.as_str());
                if let (true, Some(anchor)) = (answers, event["event_id"].as_str()) {
                    self.stage = Stage::Following {
                        anchor: anchor.to_owned(),
                        since: at,
                        answering: None,
                    };
                    self.heard_at = now;
                }
                Vec::new()
            }
            (Some("m.room.message"), Stage::Following { anchor, .. })
                if relation["rel_type"] == "m.replace"
                    && relation["event_id"].as_str() == Some(anchor.as_str()) =>
            {
                let Some(text) = content["m.new_content"]["body"].as_str() else {
                    return Vec::new();
                };
                let anchor = anchor.clone();
                self.heard_at = now;
                let mut steps = Vec::new();
                if !self.answering && !text.trim().is_empty() {
                    self.answering = true;
                    let after = now.saturating_duration_since(self.sent_at);
                    steps.push(SpokenStep::FirstText {
                        after_ms: u64::try_from(after.as_millis()).unwrap_or(u64::MAX),
                    });
                }
                steps.extend(
                    self.follower
                        .edit(&anchor, text)
                        .into_iter()
                        .map(SpokenStep::Sentence),
                );
                steps
            }
            (
                Some(STATUS),
                Stage::Following {
                    since, answering, ..
                },
            ) => {
                let Ok(status) = serde_json::from_value::<StatusContent>(content.clone()) else {
                    return Vec::new();
                };
                // A status older than the anchor is an earlier turn's.
                if status.agent != self.agent || at < *since {
                    return Vec::new();
                }
                let this = Answering::of(&status);
                let ours = answering.as_ref() == Some(&this);
                match status.run {
                    RunState::Running if answering.is_none() || ours => {
                        *answering = Some(this);
                        self.heard_at = now;
                        Vec::new()
                    }
                    RunState::Idle | RunState::Done if ours => {
                        self.stage = Stage::Done;
                        vec![SpokenStep::Complete(
                            self.follower.finish().unwrap_or_default(),
                        )]
                    }
                    // `blocked` (its claim lost), `waiting`, another copy's
                    // word, or an end before this turn ran: not the answer's
                    // end, and not a sign of it — the quiet deadline runs.
                    _ => Vec::new(),
                }
            }
            _ => Vec::new(),
        }
    }

    /// When the watch gives up unless the agent says something first.
    pub fn deadline(&self) -> Option<Instant> {
        match self.stage {
            Stage::Sent(_) | Stage::Asked(_) => Some(self.sent_at + ANSWER_START_WAIT),
            Stage::Following { .. } => Some(self.heard_at + ANSWER_QUIET_WAIT),
            Stage::Done => None,
        }
    }

    /// `now` is past the deadline: the turn ends on a sentence.
    pub fn expire(&mut self, now: Instant) -> Option<SpokenStep> {
        let deadline = self.deadline()?;
        if now < deadline {
            return None;
        }
        let started = matches!(self.stage, Stage::Following { .. });
        self.stage = Stage::Done;
        let name = &self.agent_name;
        Some(SpokenStep::Failed(if started {
            format!("{name} stopped answering.")
        } else {
            format!("{name} has not answered: no copy of it may be running right now.")
        }))
    }
}

/// The room's events as its event cache holds them, handed out once each,
/// in the cache's order (pure).
///
/// What the cache held when the watch began is the baseline: it and
/// anything before it — a page loaded backwards later — is never handed
/// out. An event of the agent still encrypted holds back everything after
/// it until its key comes and the cache replaces it ([`VectorDiff::Set`]);
/// anyone else's is passed over, since nobody else's word is the answer.
#[derive(Debug)]
pub struct CacheFeed {
    agent: OwnedUserId,
    events: Vector<Value>,
    baseline: HashSet<String>,
    /// The newest baseline event's time, for a cache emptied and refilled
    /// since (a gappy sync), where no baseline event is left to stand after.
    since: u64,
    handed: HashSet<String>,
}

impl CacheFeed {
    /// A feed over a cache holding `initial`, for `agent`'s answer.
    pub fn new(agent: OwnedUserId, initial: Vec<Value>) -> CacheFeed {
        let baseline: HashSet<String> = initial.iter().filter_map(event_id).collect();
        let since = initial
            .iter()
            .filter_map(|event| event["origin_server_ts"].as_u64())
            .max()
            .unwrap_or(0);
        CacheFeed {
            agent,
            events: initial.into_iter().collect(),
            baseline,
            since,
            handed: HashSet::new(),
        }
    }

    /// Apply the cache's `diffs`; the events now readable that were not
    /// handed out before, in order.
    pub fn apply(&mut self, diffs: Vec<VectorDiff<Value>>) -> Vec<Value> {
        for diff in diffs {
            diff.apply(&mut self.events);
        }
        let after = self
            .events
            .iter()
            .rposition(|event| event_id(event).is_some_and(|id| self.baseline.contains(&id)));
        let start = after.map_or(0, |at| at + 1);
        let mut fresh = Vec::new();
        for event in self.events.iter().skip(start) {
            let Some(id) = event_id(event) else {
                continue;
            };
            if self.handed.contains(&id) || self.baseline.contains(&id) {
                continue;
            }
            if after.is_none() && event["origin_server_ts"].as_u64().unwrap_or(0) < self.since {
                continue;
            }
            if event["type"] == "m.room.encrypted" {
                if event["sender"].as_str() == Some(self.agent.as_str()) {
                    break;
                }
                continue;
            }
            self.handed.insert(id);
            fresh.push(event.clone());
        }
        fresh
    }
}

fn event_id(event: &Value) -> Option<String> {
    event["event_id"].as_str().map(ToOwned::to_owned)
}

/// One cached event as JSON: decrypted where it was, else as it arrived.
fn event_value(event: &TimelineEvent) -> Value {
    event.raw().deserialize_as::<Value>().unwrap_or(Value::Null)
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
}

impl<H> FollowSlot<H> {
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

/// A spoken question's answer as its room delivers it: the watch, fed from
/// the room's event cache in the cache's order, and told the question's
/// event id by the send queue.
pub struct SpokenAnswer {
    room: Room,
    watch: SpokenWatch,
    feed: CacheFeed,
    cache: RoomEventCache,
    updates: RoomEventCacheSubscriber,
    queue: broadcast::Receiver<RoomSendQueueUpdate>,
    /// The question as it went into the send queue: when, and its words.
    asking: Option<(MilliSecondsSinceUnixEpoch, String)>,
    /// The question's transaction, once the queue's local echo named it.
    transaction: Option<OwnedTransactionId>,
    queued: VecDeque<SpokenStep>,
    _handles: Arc<EventCacheDropHandles>,
}

impl SpokenAnswer {
    /// Watch `room` for the answer to a question its own user is about to
    /// send to `agent`. Taken before the send, so nothing of the answer
    /// falls between the cache's baseline and the send.
    pub async fn watch(room: &Room, agent: OwnedUserId) -> Result<SpokenAnswer, CoreError> {
        let unwatched =
            |error: String| CoreError::Internal(format!("the answer cannot be followed: {error}"));
        let agent_name = match room.get_member_no_sync(&agent).await {
            Ok(Some(member)) => member.name().to_owned(),
            _ => agent.localpart().to_owned(),
        };
        // Cheap and idempotent: the app's accounts subscribe at activation.
        room.client()
            .event_cache()
            .subscribe()
            .map_err(|e| unwatched(e.to_string()))?;
        let (cache, handles) = room
            .event_cache()
            .await
            .map_err(|e| unwatched(e.to_string()))?;
        let (initial, updates) = cache
            .subscribe()
            .await
            .map_err(|e| unwatched(e.to_string()))?;
        let (_, queue) = room
            .send_queue()
            .subscribe()
            .await
            .map_err(|e| unwatched(e.to_string()))?;
        Ok(SpokenAnswer {
            room: room.clone(),
            watch: SpokenWatch::new(
                room.own_user_id().to_owned(),
                agent.clone(),
                agent_name,
                Instant::now(),
            ),
            feed: CacheFeed::new(agent, initial.iter().map(event_value).collect()),
            cache,
            updates,
            queue,
            asking: None,
            transaction: None,
            queued: VecDeque::new(),
            _handles: handles,
        })
    }

    /// The question `text` went into the send queue as `handle`. The queue
    /// announced it as a local echo while it was taken, the same handle's
    /// creation time and the same words — how its transaction is known: the
    /// SDK's handle keeps the transaction id to itself.
    pub fn sent(&mut self, handle: &SendHandle, text: &str) {
        self.asking = Some((handle.created_at, text.to_owned()));
    }

    /// The agent's name, as the person is told it.
    pub fn agent_name(&self) -> &str {
        self.watch.agent_name()
    }

    /// The next thing the voice turn hears; `None` once the answer is
    /// complete or failed and everything was told.
    pub async fn next(&mut self) -> Option<SpokenStep> {
        loop {
            if let Some(step) = self.queued.pop_front() {
                return Some(step);
            }
            let deadline = self.watch.deadline()?;
            let awaits = self.watch.awaits_question() && self.asking.is_some();
            tokio::select! {
                update = self.queue.recv(), if awaits => self.sending(update),
                update = self.updates.recv() => {
                    let diffs = match update {
                        Ok(RoomEventCacheUpdate::UpdateTimelineEvents(update)) => update
                            .diffs
                            .into_iter()
                            .map(|diff| diff.map(|event| event_value(&event)))
                            .collect(),
                        Ok(_) => continue,
                        // Missed updates: read the cache again, whole.
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            let Ok(events) = self.cache.events().await else {
                                continue;
                            };
                            vec![VectorDiff::Reset {
                                values: events.iter().map(event_value).collect(),
                            }]
                        }
                        Err(broadcast::error::RecvError::Closed) => return None,
                    };
                    let fresh = self.feed.apply(diffs);
                    self.read(fresh).await;
                }
                () = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => {
                    self.queued.extend(self.watch.expire(Instant::now()));
                }
            }
        }
    }

    /// What the send queue said of the question.
    fn sending(&mut self, update: Result<RoomSendQueueUpdate, broadcast::error::RecvError>) {
        let ours = self.transaction.clone();
        let ours = |transaction: &OwnedTransactionId| ours.as_ref() == Some(transaction);
        match update {
            Ok(RoomSendQueueUpdate::NewLocalEvent(LocalEcho {
                transaction_id,
                content:
                    LocalEchoContent::Event {
                        serialized_event,
                        send_handle,
                        ..
                    },
            })) if self.transaction.is_none() => {
                let body = serialized_event
                    .raw()
                    .0
                    .get_field::<String>("body")
                    .ok()
                    .flatten();
                let echo = (send_handle.created_at, body.unwrap_or_default());
                if self.asking.as_ref() == Some(&echo) {
                    self.transaction = Some(transaction_id);
                }
            }
            Ok(RoomSendQueueUpdate::SentEvent {
                transaction_id,
                event_id,
            }) if ours(&transaction_id) => {
                let steps = self.watch.asked(event_id.as_str(), Instant::now());
                self.queued.extend(steps);
            }
            Ok(
                RoomSendQueueUpdate::SendError {
                    transaction_id,
                    is_recoverable: false,
                    ..
                }
                | RoomSendQueueUpdate::CancelledLocalEvent { transaction_id },
            ) if ours(&transaction_id) => {
                self.queued.extend(self.watch.not_sent());
            }
            Err(broadcast::error::RecvError::Closed) => {
                self.queued.extend(self.watch.not_sent());
            }
            _ => {}
        }
    }

    /// Read `events`, in order, with whether each sender holds an agent's
    /// power in the room now.
    async fn read(&mut self, events: Vec<Value>) {
        if events.is_empty() {
            return;
        }
        let levels = self.room.power_levels().await.ok();
        for event in events {
            let power = match event["sender"].as_str().map(UserId::parse) {
                Some(Ok(user)) => holds_agent_power(levels.as_ref(), &user),
                _ => false,
            };
            let steps = self.watch.see(&event, power, Instant::now());
            self.queued.extend(steps);
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::voice::{Turn, TurnEvent, VoicePlatform};

    const ME: &str = "@tgorka:example.org";
    const NIXI: &str = "@nixi:example.org";

    fn user(id: &str) -> OwnedUserId {
        OwnedUserId::try_from(id).expect("user")
    }

    fn watch(now: Instant) -> SpokenWatch {
        SpokenWatch::new(user(ME), user(NIXI), "Nixi".to_owned(), now)
    }

    fn message(id: &str, sender: &str, ts: u64, content: Value) -> Value {
        json!({"type": "m.room.message", "event_id": id, "sender": sender,
               "origin_server_ts": ts, "content": content})
    }

    fn question(id: &str, sender: &str, ts: u64) -> Value {
        message(
            id,
            sender,
            ts,
            json!({"msgtype": "m.text", "body": "what colour is the sky"}),
        )
    }

    /// An answer's anchor, answering the question `asks`.
    fn anchor(id: &str, sender: &str, asks: &str, ts: u64) -> Value {
        message(
            id,
            sender,
            ts,
            json!({"msgtype": "m.text", "body": "…",
                   "dev.keeper.agent.turn": {"session": "main", "line": "01L", "question": asks}}),
        )
    }

    fn edit(target: &str, sender: &str, text: &str, ts: u64) -> Value {
        message(
            &format!("$e{ts}"),
            sender,
            ts,
            json!({"msgtype": "m.text", "body": format!("* {text}"),
                   "m.new_content": {"msgtype": "m.text", "body": text},
                   "m.relates_to": {"rel_type": "m.replace", "event_id": target}}),
        )
    }

    /// A status from `sender` naming `agent`, from `host` under `epoch`.
    fn status(sender: &str, agent: &str, run: &str, host: &str, epoch: u64, ts: u64) -> Value {
        json!({"type": STATUS, "event_id": format!("$s{ts}"), "sender": sender,
               "origin_server_ts": ts, "content": {
            "v": 1, "session": "60-sessions/main", "kind": "main", "title": "Nixi",
            "agent": agent, "host": host, "epoch": epoch, "run": run}})
    }

    fn turn_status(run: &str, ts: u64) -> Value {
        status(NIXI, NIXI, run, "electra", 1, ts)
    }

    fn sentences(steps: &[SpokenStep]) -> Vec<String> {
        steps
            .iter()
            .filter_map(|step| match step {
                SpokenStep::Sentence(sentence) => Some(sentence.clone()),
                _ => None,
            })
            .collect()
    }

    fn see_all(watch: &mut SpokenWatch, events: &[Value], now: Instant) -> Vec<SpokenStep> {
        events
            .iter()
            .flat_map(|event| watch.see(event, true, now))
            .collect()
    }

    /// The answer is the anchor naming this question's event: its edits
    /// are spoken sentence by sentence, and the turn's `idle` completes it
    /// with its tail.
    #[test]
    fn the_answer_to_this_question_is_followed_to_its_end() {
        let start = Instant::now();
        let mut watch = watch(start);
        assert!(watch.asked("$q", start).is_empty());
        assert!(watch.see(&question("$q", ME, 10), true, start).is_empty());
        assert!(see_all(
            &mut watch,
            &[anchor("$a", NIXI, "$q", 11), turn_status("running", 12)],
            start
        )
        .is_empty());
        let later = start + Duration::from_millis(1500);
        assert_eq!(
            watch.see(&edit("$a", NIXI, "The sky is blue. It", 13), true, later),
            vec![
                SpokenStep::FirstText { after_ms: 1500 },
                SpokenStep::Sentence("The sky is blue.".to_owned())
            ]
        );
        let steps = watch.see(
            &edit("$a", NIXI, "The sky is blue. It is clear today.", 14),
            true,
            later,
        );
        assert!(steps.is_empty(), "{steps:?}");
        assert_eq!(
            watch.see(&turn_status("idle", 15), true, later),
            vec![SpokenStep::Complete("It is clear today.".to_owned())]
        );
        assert!(watch.is_done());
        assert_eq!(watch.deadline(), None);
        assert!(watch
            .see(
                &edit("$a", NIXI, "The sky is blue. More. ", 16),
                true,
                later
            )
            .is_empty());
    }

    /// The host serializes a room's turns: `$q1` and `$q2` are queued, and
    /// `$a1` — the first anchor after `$q2` — answers `$q1`. Followed for
    /// `$q2`, `$a1` and its edits are not heard; `$a2` is.
    #[test]
    fn an_older_questions_answer_is_not_this_ones() {
        let start = Instant::now();
        let mut watch = watch(start);
        watch.asked("$q2", start);
        let room = [
            question("$q1", ME, 1),
            question("$q2", ME, 2),
            anchor("$a1", NIXI, "$q1", 3),
            turn_status("running", 4),
            edit("$a1", NIXI, "Answer one. Done", 5),
            turn_status("idle", 6),
        ];
        assert!(see_all(&mut watch, &room, start).is_empty());
        assert!(!watch.is_done());
        let steps = see_all(
            &mut watch,
            &[
                anchor("$a2", NIXI, "$q2", 7),
                status(NIXI, NIXI, "running", "electra", 1, 8),
                edit("$a2", NIXI, "Answer two. Done", 9),
                turn_status("idle", 10),
            ],
            start,
        );
        assert_eq!(sentences(&steps), ["Answer two."]);
        assert_eq!(steps.last(), Some(&SpokenStep::Complete("Done".to_owned())));
    }

    /// The same words asked from the phone and the Mac are two questions:
    /// each device follows the anchor naming its own event, whichever came
    /// first in the room.
    #[test]
    fn the_same_words_from_two_devices_are_two_questions() {
        let start = Instant::now();
        let mut phone = watch(start);
        let mut mac = watch(start);
        phone.asked("$from-phone", start);
        mac.asked("$from-mac", start);
        let room = [
            question("$from-mac", ME, 1),
            question("$from-phone", ME, 2),
            anchor("$for-mac", NIXI, "$from-mac", 3),
            turn_status("running", 4),
            edit("$for-mac", NIXI, "Mac. Answer", 5),
            turn_status("idle", 6),
            anchor("$for-phone", NIXI, "$from-phone", 7),
            turn_status("running", 8),
            edit("$for-phone", NIXI, "Phone. Answer", 9),
            turn_status("idle", 10),
        ];
        assert_eq!(sentences(&see_all(&mut phone, &room, start)), ["Phone."]);
        assert_eq!(sentences(&see_all(&mut mac, &room, start)), ["Mac."]);
        assert!(phone.is_done() && mac.is_done());
    }

    /// The room may deliver the answer before the send queue reports the
    /// question's event id: what came is kept and read, in order, once the
    /// id is known.
    #[test]
    fn what_arrives_before_the_send_lands_is_read_once_it_does() {
        let start = Instant::now();
        let mut watch = watch(start);
        let early = see_all(
            &mut watch,
            &[
                anchor("$a", NIXI, "$q", 3),
                turn_status("running", 4),
                edit("$a", NIXI, "Early. Words", 5),
            ],
            start,
        );
        assert!(early.is_empty());
        assert_eq!(sentences(&watch.asked("$q", start)), ["Early."]);
        assert_eq!(
            watch.see(&turn_status("idle", 6), true, start),
            vec![SpokenStep::Complete("Words".to_owned())]
        );
        let mut lost = self::watch(start);
        assert_eq!(
            lost.not_sent(),
            Some(SpokenStep::Failed(
                "Your question did not reach Nixi.".to_owned()
            ))
        );
        assert!(lost.is_done());
    }

    /// Only the room's agent, at an agent's power, answers: a person's
    /// marked message, someone else's edit or status, or the agent's own
    /// user without power is not the answer.
    #[test]
    fn only_the_rooms_agent_answers() {
        let start = Instant::now();
        let mut watch = watch(start);
        watch.asked("$q", start);
        watch.see(&anchor("$forged", ME, "$q", 1), true, start);
        watch.see(&anchor("$weak", NIXI, "$q", 1), false, start);
        watch.see(&anchor("$other", "@tola:example.org", "$q", 1), true, start);
        watch.see(&anchor("$a", NIXI, "$q", 2), true, start);
        watch.see(&turn_status("running", 3), true, start);
        assert!(
            sentences(&watch.see(&edit("$a", ME, "Forged. Text. ", 4), true, start)).is_empty()
        );
        assert!(
            sentences(&watch.see(&edit("$a", NIXI, "Weak. Text. ", 4), false, start)).is_empty()
        );
        let tola = "@tola:example.org";
        assert!(watch
            .see(&status(tola, tola, "idle", "electra", 1, 5), true, start)
            .is_empty());
        // A status naming another agent, sent by the room's agent, is not
        // this session's.
        assert!(watch
            .see(&status(NIXI, tola, "idle", "electra", 1, 5), true, start)
            .is_empty());
        assert!(!watch.is_done());
        assert_eq!(
            sentences(&watch.see(&edit("$a", NIXI, "Yes. Really", 6), true, start)),
            ["Yes."]
        );
        assert_eq!(
            watch.see(&turn_status("idle", 7), true, start),
            vec![SpokenStep::Complete("Really".to_owned())]
        );
    }

    /// Only the answering turn's own end completes the answer: `blocked`
    /// after its claim was lost, `waiting`, another copy's `idle`, an
    /// `idle` older than the anchor, or an `idle` before the turn said it
    /// was running completes nothing, and the edits still being sent are
    /// heard.
    #[test]
    fn only_the_answering_turns_end_completes_the_answer() {
        let start = Instant::now();
        let mut watch = watch(start);
        watch.asked("$q", start);
        let steps = see_all(
            &mut watch,
            &[
                anchor("$a", NIXI, "$q", 20),
                // The previous turn's end, read late from the cache.
                turn_status("idle", 19),
                // An end before this turn said it runs.
                status(NIXI, NIXI, "idle", "electra", 1, 21),
            ],
            start,
        );
        assert!(steps.is_empty() && !watch.is_done(), "{steps:?}");
        watch.see(&turn_status("running", 22), true, start);
        let steps = see_all(
            &mut watch,
            &[
                edit("$a", NIXI, "One. Two", 23),
                // electra lost its claim: it says `blocked` and drains.
                turn_status("blocked", 24),
                status(NIXI, NIXI, "waiting", "electra", 1, 25),
                // Another copy, under the claim it took over.
                status(NIXI, NIXI, "idle", "hesperia", 2, 26),
                status(NIXI, NIXI, "running", "hesperia", 2, 27),
                status(NIXI, NIXI, "idle", "electra", 2, 28),
                // This copy's end of an earlier turn, older than the anchor,
                // read late (a page loaded backwards).
                turn_status("idle", 19),
                edit("$a", NIXI, "One. Two. Three", 29),
            ],
            start,
        );
        assert_eq!(sentences(&steps), ["One.", "Two."]);
        assert!(!watch.is_done());
        assert_eq!(
            watch.see(&turn_status("idle", 30), true, start),
            vec![SpokenStep::Complete("Three".to_owned())]
        );
    }

    /// A turn whose claim was lost and never ends is not waited for
    /// forever: its `blocked` does not count as the turn speaking, and the
    /// quiet deadline ends it on a sentence.
    #[test]
    fn a_turn_that_lost_its_claim_ends_on_the_quiet_deadline() {
        let start = Instant::now();
        let mut watch = watch(start);
        watch.asked("$q", start);
        watch.see(&anchor("$a", NIXI, "$q", 1), true, start);
        watch.see(&turn_status("running", 2), true, start);
        let spoke = start + Duration::from_secs(5);
        watch.see(&edit("$a", NIXI, "Half. An", 3), true, spoke);
        let blocked = start + Duration::from_secs(30);
        watch.see(&turn_status("blocked", 4), true, blocked);
        assert_eq!(watch.deadline(), Some(spoke + ANSWER_QUIET_WAIT));
        assert_eq!(
            watch.expire(spoke + ANSWER_QUIET_WAIT),
            Some(SpokenStep::Failed("Nixi stopped answering.".to_owned()))
        );
    }

    /// No answer within a minute, or silence for two once it started, ends
    /// the turn on a sentence naming the agent; the turn's own `running`
    /// moves the silence's deadline.
    #[test]
    fn a_question_nobody_answers_ends_on_a_sentence() {
        let start = Instant::now();
        let mut watch = watch(start);
        watch.asked("$q", start);
        assert_eq!(watch.deadline(), Some(start + ANSWER_START_WAIT));
        assert_eq!(watch.expire(start + Duration::from_secs(59)), None);
        let Some(SpokenStep::Failed(why)) = watch.expire(start + ANSWER_START_WAIT) else {
            panic!("the turn ends");
        };
        assert!(why.starts_with("Nixi has not answered"), "{why}");
        assert!(watch.is_done());

        let mut watch = self::watch(start);
        watch.asked("$q", start);
        watch.see(
            &anchor("$a", NIXI, "$q", 1),
            true,
            start + Duration::from_secs(50),
        );
        let busy = start + Duration::from_secs(150);
        watch.see(&turn_status("running", 2), true, busy);
        assert_eq!(watch.deadline(), Some(busy + ANSWER_QUIET_WAIT));
        assert_eq!(watch.expire(busy + Duration::from_secs(119)), None);
        assert_eq!(
            watch.expire(busy + ANSWER_QUIET_WAIT),
            Some(SpokenStep::Failed("Nixi stopped answering.".to_owned()))
        );
    }

    fn encrypted(id: &str, sender: &str, ts: u64) -> Value {
        json!({"type": "m.room.encrypted", "event_id": id, "sender": sender,
               "origin_server_ts": ts, "content": {"algorithm": "m.megolm.v1.aes-sha2"}})
    }

    fn ids(events: &[Value]) -> Vec<&str> {
        events
            .iter()
            .filter_map(|event| event["event_id"].as_str())
            .collect()
    }

    /// The anchor could not be decrypted when it arrived; the cache
    /// replaces it once its key comes. Nothing after it is handed out until
    /// then, and then everything is, once, in the room's order — so the
    /// watch follows the answer whose key came late to its end.
    #[test]
    fn an_answer_decrypted_late_is_read_in_the_rooms_order() {
        let start = Instant::now();
        let older = question("$older", ME, 1);
        let mut feed = CacheFeed::new(user(NIXI), vec![older.clone()]);
        let mut watch = watch(start);
        watch.asked("$q", start);

        let fresh = feed.apply(vec![VectorDiff::Append {
            values: [
                question("$q", ME, 10),
                encrypted("$a", NIXI, 11),
                encrypted("$marta", "@marta:example.org", 12),
                turn_status("running", 13),
                edit("$a", NIXI, "Late. Key", 14),
                turn_status("idle", 15),
            ]
            .into_iter()
            .collect(),
        }]);
        assert_eq!(ids(&fresh), ["$q"]);
        assert!(see_all(&mut watch, &fresh, start).is_empty());

        // The redecryptor replaces the event where it stands.
        let fresh = feed.apply(vec![VectorDiff::Set {
            index: 2,
            value: anchor("$a", NIXI, "$q", 11),
        }]);
        assert_eq!(ids(&fresh), ["$a", "$s13", "$e14", "$s15"]);
        let steps = see_all(&mut watch, &fresh, start);
        assert_eq!(sentences(&steps), ["Late."]);
        assert_eq!(steps.last(), Some(&SpokenStep::Complete("Key".to_owned())));

        // Handed out once: the cache moving one again hands out nothing,
        // and neither does an older page loaded in front; Marta's message,
        // decrypted after all, is handed out (the watch passes it over).
        let fresh = feed.apply(vec![
            VectorDiff::Set {
                index: 3,
                value: question("$marta", "@marta:example.org", 12),
            },
            VectorDiff::PushFront {
                value: anchor("$ancient", NIXI, "$q", 0),
            },
            VectorDiff::Remove { index: 5 },
            VectorDiff::Insert {
                index: 5,
                value: turn_status("running", 13),
            },
        ]);
        assert_eq!(ids(&fresh), ["$marta"]);
        assert!(see_all(&mut watch, &fresh, start).is_empty());
    }

    /// The cache emptied and refilled (a gappy sync): with no baseline
    /// event left to stand after, nothing older than the baseline is handed
    /// out, and the newer events are.
    #[test]
    fn a_refilled_cache_hands_out_only_what_is_newer() {
        let mut feed = CacheFeed::new(user(NIXI), vec![question("$older", ME, 100)]);
        let fresh = feed.apply(vec![VectorDiff::Reset {
            values: [encrypted("$stale", NIXI, 50), anchor("$a", NIXI, "$q", 120)]
                .into_iter()
                .collect(),
        }]);
        assert_eq!(ids(&fresh), ["$a"]);
    }

    fn heard(turn: &mut Turn, text: &str) -> u64 {
        turn.apply(TurnEvent::WakeMatched);
        turn.apply(TurnEvent::FinalHeard(text.to_owned()));
        turn.question().expect("a question is out")
    }

    /// Question A's send is slow; the person stops and asks B, whose send
    /// finishes first. A's finishing later neither takes B's place nor
    /// moves B's turn; only B's follower is kept.
    #[test]
    fn two_sends_finishing_in_reverse_order_leave_the_newer_question_its_answer() {
        let mut turn = Turn::new(VoicePlatform::MACOS);
        let mut slot = FollowSlot::default();
        let a = heard(&mut turn, "first question");
        assert!(turn.owns_answer(a));
        turn.apply(TurnEvent::Abandoned);
        assert!(!turn.owns_answer(a), "stopping drops the question");
        let b = heard(&mut turn, "second question");
        assert_ne!(a, b);

        assert_eq!(slot.keep_only(turn.question()), None);
        assert_eq!(slot.install(b, turn.question(), "B"), None);
        assert_eq!(slot.install(a, turn.question(), "A"), Some("A"));
        assert!(!turn.owns_answer(a));
        assert!(turn.owns_answer(b));
        assert_eq!(slot.keep_only(turn.question()), None, "B is kept");

        // A's late callbacks are not the turn's; B's are.
        turn.apply(TurnEvent::Sent);
        assert!(turn.owns_answer(b) && !turn.owns_answer(a));
    }

    /// A was going to the agent; the person asked B of a provider bot, which
    /// installs no follower. A's follower, ready after, is refused, and a
    /// barge-in or a stop while A still prepares leaves A nothing.
    #[test]
    fn a_question_replaced_while_it_was_sent_has_no_answer_to_follow() {
        let mut turn = Turn::new(VoicePlatform::MACOS);
        let mut slot: FollowSlot<&str> = FollowSlot::default();
        let a = heard(&mut turn, "ask the agent");
        turn.apply(TurnEvent::Abandoned);
        let bot = heard(&mut turn, "ask the bot");
        assert_eq!(slot.keep_only(turn.question()), None);
        assert_eq!(slot.install(a, turn.question(), "A"), Some("A"));
        assert!(!turn.owns_answer(a) && turn.owns_answer(bot));

        let mut turn = Turn::new(VoicePlatform::MACOS);
        let a = heard(&mut turn, "ask the agent");
        turn.apply(TurnEvent::Abandoned);
        assert_eq!(turn.question(), None);
        assert_eq!(slot.install(a, turn.question(), "A"), Some("A"));

        // An installed follower is dropped once its question is not the
        // turn's: answered and spoken, then a barge-in asks again.
        let mut turn = Turn::new(VoicePlatform::MACOS);
        let a = heard(&mut turn, "ask the agent");
        assert_eq!(slot.install(a, turn.question(), "A"), None);
        turn.apply(TurnEvent::AnswerSentence("Hello.".to_owned()));
        assert!(turn.owns_answer(a), "reading the answer is still A's");
        turn.apply(TurnEvent::SpeechDetected("wait".to_owned()));
        assert!(!turn.owns_answer(a));
        assert_eq!(slot.keep_only(turn.question()), Some("A"));
    }
}
