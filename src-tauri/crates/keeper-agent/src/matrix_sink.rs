//! A streamed answer in a room: one anchor, then edits of it (AD-373, R18).
//!
//! The answer is not streamed token by token. The host sends an anchor (`…`)
//! as soon as the request reaches it, then edits that one event with the
//! whole text so far, no closer than [`MIN_EDIT_GAP`] apart, and a final edit
//! carrying the whole answer. A homeserver that asks the sender to wait
//! (`M_LIMIT_EXCEEDED`) is waited for, and then the whole text goes out once:
//! there is never a backlog of stale edits to drain. The final edit is
//! retried, with the same transaction id, until the homeserver accepts it, so
//! a reader never keeps a half answer.
//!
//! Every send goes through an [`EditPort`], so the pacing is tested against a
//! fake port on tokio's paused clock; [`RoomPort`] is the real one.
//!
//! A send that carries what the session read asks its [`RoomGate`] first,
//! at every attempt: the room's members as they are then, against the
//! label as it is then (R168). Narrowed, an answer's edits stop and a
//! status says only [`NARROWED_STATUS`] (R64).

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use keeper_core::agents::events::{
    self, RunState, StatusContent, TurnRef, CONTENT_VERSION, FINAL_CUT_BYTES, STATUS, TURN,
};
use keeper_core::agents::matrix::{AgentClient, AgentMatrixError};
use keeper_core::agents::redact::redact_secrets;
use keeper_core::agents::room::room_members;
use keeper_core::vm::BotStreamEvent;
use matrix_sdk::ruma::{EventId, OwnedEventId, OwnedRoomId, OwnedTransactionId, TransactionId};
use serde_json::{json, Value};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{sleep, sleep_until, Instant};

use crate::delegate::MembersFuture;
use crate::ports::TurnSink;
use crate::sinks::{RoomGate, NARROWED_STATUS};

/// No two edits of one anchor are closer than this (R18, NFR-113).
pub const MIN_EDIT_GAP: Duration = Duration::from_millis(400);

/// What an anchor says before the first edit.
pub const PLACEHOLDER: &str = "…";

/// How long one send may take before it counts as failed.
const SEND_TIMEOUT: Duration = Duration::from_secs(10);

/// The first and the longest wait between two attempts of a send that must
/// arrive.
const BACKOFF_FIRST: Duration = Duration::from_millis(250);
const BACKOFF_MAX: Duration = Duration::from_secs(8);

/// The wait after a 429 that named no `retry_after_ms`.
const RATE_LIMIT_DEFAULT: Duration = Duration::from_secs(1);

/// A boxed send future.
pub type SendFuture<'a> =
    Pin<Box<dyn Future<Output = Result<OwnedEventId, AgentMatrixError>> + Send + 'a>>;

/// Where a session's events go: one timeline send, once, under `txn`.
pub trait EditPort: Send + Sync {
    /// Send `content` as an `event_type` event. A retry passes the same
    /// `txn`, so the homeserver keeps one copy.
    fn send<'a>(
        &'a self,
        event_type: &'a str,
        content: Value,
        txn: OwnedTransactionId,
    ) -> SendFuture<'a>;
    /// Who is in the room or invited to it now, any power (R160); an
    /// error when that cannot be read.
    fn members(&self) -> MembersFuture<'_>;
}

/// The real port: one copy's client in one room.
pub struct RoomPort {
    client: AgentClient,
    room: OwnedRoomId,
}

impl RoomPort {
    pub fn new(client: AgentClient, room: OwnedRoomId) -> RoomPort {
        RoomPort { client, room }
    }
}

impl EditPort for RoomPort {
    fn send<'a>(
        &'a self,
        event_type: &'a str,
        content: Value,
        txn: OwnedTransactionId,
    ) -> SendFuture<'a> {
        Box::pin(async move {
            match tokio::time::timeout(
                SEND_TIMEOUT,
                self.client
                    .send(&self.room, event_type, content, Some(&txn)),
            )
            .await
            {
                Ok(sent) => sent,
                Err(_) => Err(AgentMatrixError::Network(format!(
                    "no answer within {} s",
                    SEND_TIMEOUT.as_secs()
                ))),
            }
        })
    }

    fn members(&self) -> MembersFuture<'_> {
        Box::pin(async move {
            let room = self
                .client
                .client()
                .get_room(&self.room)
                .ok_or_else(|| "this copy is not in the room".to_owned())?;
            tokio::time::timeout(SEND_TIMEOUT, room_members(&room))
                .await
                .ok()
                .flatten()
                .ok_or_else(|| "the room's members could not be read".to_owned())
        })
    }
}

/// `text` cut to at most `max` bytes on a `char` boundary.
fn prefix(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// The final message for an answer longer than [`FINAL_CUT_BYTES`]: its
/// first [`FINAL_CUT_BYTES`] and the sentence naming the artifact; `None`
/// when the answer fits.
pub fn cut(answer: &str, artifact: &str) -> Option<String> {
    (answer.len() > FINAL_CUT_BYTES).then(|| {
        format!(
            "{}\n\nThe full answer is in {artifact}",
            prefix(answer, FINAL_CUT_BYTES)
        )
    })
}

/// What the room is sent for an answer longer than [`FINAL_CUT_BYTES`] when
/// its artifact could not be written: its first [`FINAL_CUT_BYTES`] and where
/// the whole answer is, which is the session's log.
pub fn cut_to_log(answer: &str) -> String {
    format!(
        "{}\n\n(the full answer is in this session's log)",
        prefix(answer, FINAL_CUT_BYTES)
    )
}

/// An answer's anchor: the placeholder, the turn it belongs to and the
/// person's message `question` it answers.
pub fn anchor_content(session: &str, user_line: &str, question: &EventId) -> Value {
    json!({
        "msgtype": "m.text",
        "body": PLACEHOLDER,
        TURN: TurnRef {
            session: session.to_owned(),
            line: user_line.to_owned(),
            question: Some(question.to_owned()),
        },
    })
}

/// A plain message from the host, outside any answer.
pub fn notice_content(text: &str) -> Value {
    json!({ "msgtype": "m.text", "body": text })
}

/// Send `content` until the homeserver accepts it, never sooner than
/// `not_before`, with one transaction id across every attempt: a 429 is
/// waited out, any other failure backs off. Returns the event and when it
/// was accepted.
pub async fn deliver(
    port: &dyn EditPort,
    event_type: &str,
    content: Value,
    not_before: Instant,
) -> (OwnedEventId, Instant) {
    let (event, at, _) =
        deliver_gated(port, event_type, None, &|_| content.clone(), not_before).await;
    (event, at)
}

/// [`deliver`], with each attempt's content made by `build` from whether
/// `gate` finds the session narrowed below its room at that attempt (R168,
/// R169): a retry never sends what an earlier attempt was made from.
/// Returns the event, when it was accepted, and whether it was narrowed.
pub async fn deliver_gated(
    port: &dyn EditPort,
    event_type: &str,
    gate: Option<&RoomGate>,
    build: &(dyn Fn(bool) -> Value + Sync),
    not_before: Instant,
) -> (OwnedEventId, Instant, bool) {
    let sent = attempt(
        port,
        event_type,
        gate,
        &|narrowed| Ok::<_, std::convert::Infallible>(build(narrowed)),
        not_before,
    )
    .await;
    match sent {
        Ok(sent) => sent,
        Err(never) => match never {},
    }
}

/// [`deliver_gated`] of what may not be sent at all under a narrowed room:
/// `build` answers `None` for that, and nothing more is attempted.
pub async fn deliver_unless_narrowed(
    port: &dyn EditPort,
    event_type: &str,
    gate: &RoomGate,
    build: &(dyn Fn(bool) -> Option<Value> + Sync),
    not_before: Instant,
) -> Option<(OwnedEventId, Instant)> {
    attempt(
        port,
        event_type,
        Some(gate),
        &|narrowed| build(narrowed).ok_or(()),
        not_before,
    )
    .await
    .ok()
    .map(|(event, at, _)| (event, at))
}

/// The attempts of one send that must arrive: each asks `gate`, then
/// `build` for its content — or for why it is not sent.
async fn attempt<E>(
    port: &dyn EditPort,
    event_type: &str,
    gate: Option<&RoomGate>,
    build: &(dyn Fn(bool) -> Result<Value, E> + Sync),
    not_before: Instant,
) -> Result<(OwnedEventId, Instant, bool), E> {
    let txn = TransactionId::new();
    let mut not_before = not_before;
    let mut backoff = BACKOFF_FIRST;
    loop {
        sleep_until(not_before).await;
        let narrowed = match gate {
            Some(gate) => gate.narrowed().await,
            None => false,
        };
        match port.send(event_type, build(narrowed)?, txn.clone()).await {
            Ok(event) => return Ok((event, Instant::now(), narrowed)),
            Err(AgentMatrixError::RateLimited { retry_after_ms }) => {
                let wait = retry_after_ms.map_or(RATE_LIMIT_DEFAULT, Duration::from_millis);
                tracing::info!(wait_ms = wait.as_millis() as u64, %event_type, "agents: the homeserver asked to wait");
                not_before = Instant::now() + wait.max(MIN_EDIT_GAP);
            }
            Err(error) => {
                tracing::warn!(%error, %event_type, "agents: a send that must arrive failed; retrying");
                not_before = Instant::now() + backoff;
                backoff = (backoff * 2).min(BACKOFF_MAX);
            }
        }
    }
}

/// Makes an edit's content from the anchor, the text and whether the
/// session is narrowed below its room now; `None` sends nothing.
type EditContent = Arc<dyn Fn(&OwnedEventId, &str, bool) -> Option<Value> + Send + Sync>;

/// What the pacer last did.
#[derive(Debug, Clone)]
struct Paced {
    last_send: Instant,
    edits: usize,
}

/// The text so far, and whether the turn is over.
#[derive(Debug, Clone, Default)]
struct Progress {
    text: String,
    finished: bool,
}

/// Edit `anchor` with the newest progress, no closer than [`MIN_EDIT_GAP`],
/// until the progress says the turn is over. `content` makes an edit from
/// the anchor, the text and whether `gate` finds the session narrowed
/// below its room at that edit; `shown` is what the room already has. The
/// narrowing is part of what was sent, so a change of it alone is an edit
/// (R169), and every attempt, a retry too, is made from it as it is then.
#[allow(clippy::too_many_arguments)]
async fn pace(
    port: Arc<dyn EditPort>,
    event_type: &'static str,
    anchor: OwnedEventId,
    content: EditContent,
    gate: Option<Arc<RoomGate>>,
    shown: (String, bool),
    mut last_send: Instant,
    mut progress: watch::Receiver<Progress>,
) -> Paced {
    let mut edits = 0;
    let mut sent = shown;
    // After a 429 the whole text goes out once, whether or not it grew
    // during the wait.
    let mut owed = false;
    loop {
        if progress.borrow().finished {
            break;
        }
        if !owed && progress.changed().await.is_err() {
            break;
        }
        sleep_until(last_send + MIN_EDIT_GAP).await;
        let text = {
            let now = progress.borrow_and_update();
            if now.finished {
                break;
            }
            now.text.clone()
        };
        let narrowed = match &gate {
            Some(gate) => gate.narrowed().await,
            None => false,
        };
        // Narrowed, no text is shown, so a new count is no new edit.
        let now = if narrowed {
            (String::new(), true)
        } else {
            (text, false)
        };
        if now == sent {
            owed = false;
            continue;
        }
        let Some(edit) = content(&anchor, &now.0, narrowed) else {
            sent = now;
            owed = false;
            continue;
        };
        match port.send(event_type, edit, TransactionId::new()).await {
            Ok(_) => {
                last_send = Instant::now();
                sent = now;
                edits += 1;
                owed = false;
            }
            Err(AgentMatrixError::RateLimited { retry_after_ms }) => {
                let wait = retry_after_ms.map_or(RATE_LIMIT_DEFAULT, Duration::from_millis);
                tracing::info!(
                    wait_ms = wait.as_millis() as u64,
                    "agents: edits wait for the homeserver"
                );
                sleep(wait).await;
                // Measured from the wait's end, so the edit after a 429 is
                // the one owed rather than one squeezed in early.
                last_send = Instant::now() - MIN_EDIT_GAP;
                owed = true;
            }
            Err(error) => {
                tracing::warn!(%error, "agents: an edit was not sent; the next carries its text");
                last_send = Instant::now();
                owed = true;
            }
        }
    }
    Paced { last_send, edits }
}

/// How a streamed answer ended in the room: made only by
/// [`MatrixSink::finish`], once the homeserver accepted the final edit.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Delivered {
    /// The final edit's event.
    pub final_event: OwnedEventId,
    /// When the homeserver accepted it.
    pub accepted_at: Instant,
    /// How many edits went out before it.
    pub edits: usize,
    /// Whether the room was found narrowed below the label at the accepted
    /// attempt, so the final edit said only the narrowed sentence.
    pub narrowed: bool,
}

/// One answer's sink: deltas in, paced edits of its anchor out.
pub struct MatrixSink {
    port: Arc<dyn EditPort>,
    anchor: OwnedEventId,
    text: Mutex<String>,
    progress: watch::Sender<Progress>,
    pacer: Mutex<Option<JoinHandle<Paced>>>,
    /// The room's boundary: once narrowed, no more of the text reaches it.
    gate: Option<Arc<RoomGate>>,
}

impl MatrixSink {
    /// Start pacing edits of `anchor`, which the homeserver accepted at
    /// `anchor_at`, each one past `gate` when there is one.
    pub fn start(
        port: Arc<dyn EditPort>,
        anchor: OwnedEventId,
        anchor_at: Instant,
        gate: Option<Arc<RoomGate>>,
    ) -> MatrixSink {
        let (progress, receiver) = watch::channel(Progress::default());
        // An edit carries the log's redaction (S-17): the room never shows
        // a secret the log does not. Narrowed, the text stops (S-16).
        let content: EditContent = Arc::new(|anchor, text, narrowed| {
            (!narrowed).then(|| events::edit_content(anchor, &redact_secrets(text).text))
        });
        let pacer = tokio::spawn(pace(
            Arc::clone(&port),
            "m.room.message",
            anchor.clone(),
            content,
            gate.clone(),
            (String::new(), false),
            anchor_at,
            receiver,
        ));
        MatrixSink {
            port,
            anchor,
            text: Mutex::new(String::new()),
            progress,
            pacer: Mutex::new(Some(pacer)),
            gate,
        }
    }

    /// Add streamed text. Edits carry at most [`FINAL_CUT_BYTES`] of it: the
    /// rest reaches the room only through the final message's artifact.
    pub fn push(&self, delta: &str) {
        let mut text = self.text.lock().unwrap_or_else(|p| p.into_inner());
        text.push_str(delta);
        let shown = prefix(&text, FINAL_CUT_BYTES).to_owned();
        self.progress.send_modify(|progress| progress.text = shown);
    }

    /// The text streamed so far.
    pub fn text(&self) -> String {
        self.text.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// Stop the paced edits and deliver `final_text` as the final edit,
    /// retried until accepted — or `narrowed_text` at an attempt the gate
    /// finds the room narrowed below the label.
    pub async fn finish(&self, final_text: &str, narrowed_text: &str) -> Delivered {
        self.progress
            .send_modify(|progress| progress.finished = true);
        let pacer = self.pacer.lock().unwrap_or_else(|p| p.into_inner()).take();
        let paced = match pacer {
            Some(handle) => handle.await.unwrap_or_else(|_| Paced {
                last_send: Instant::now(),
                edits: 0,
            }),
            None => Paced {
                last_send: Instant::now(),
                edits: 0,
            },
        };
        let (final_event, accepted_at, narrowed) = deliver_gated(
            self.port.as_ref(),
            "m.room.message",
            self.gate.as_deref(),
            &|narrowed| {
                let text = if narrowed { narrowed_text } else { final_text };
                events::edit_content(&self.anchor, text)
            },
            paced.last_send + MIN_EDIT_GAP,
        )
        .await;
        Delivered {
            final_event,
            accepted_at,
            edits: paced.edits,
            narrowed,
        }
    }
}

impl TurnSink for MatrixSink {
    fn event(&self, event: BotStreamEvent) -> bool {
        if let BotStreamEvent::Delta { text } = event {
            self.push(&text);
        }
        true
    }
}

/// What a session's status anchor says while a turn runs: counts only,
/// never a path, a title or a heading (S-16).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ToolProgress {
    /// Files read so far this turn.
    pub reads: usize,
    /// Tool calls so far this turn.
    pub calls: usize,
}

impl ToolProgress {
    /// The status `detail`: "reading 3 files, 4 tool calls".
    pub fn detail(&self) -> String {
        let plural =
            |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        let calls = plural(self.calls, "tool call", "tool calls");
        if self.reads == 0 {
            calls
        } else {
            format!("reading {}, {calls}", plural(self.reads, "file", "files"))
        }
    }
}

/// The session's status anchor and its paced edits for one turn.
///
/// The status says `running` from the turn's start, whatever the turn does,
/// and `idle` only once [`StatusBoard::finish`] is handed the answer's
/// accepted final edit, so a device following the answer reads the status
/// leaving `running` as "the answer is whole" (AD-384). A board dropped
/// without that — the turn cancelled, or unwinding — stops its task where it
/// is and publishes nothing more: never an `idle` for an answer that did
/// not land. Every attempt asks the gate: narrowed below its room, the
/// status keeps `session` and says [`NARROWED_STATUS`] with no detail
/// (R64), and the suppression is audited (R65).
pub struct StatusBoard {
    progress: watch::Sender<Progress>,
    task: Mutex<Option<JoinHandle<Option<OwnedEventId>>>>,
    gate: Option<Arc<RoomGate>>,
}

/// A task that is aborted when its holder goes.
struct AbortOnDrop<T>(JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// `base` as a status saying `run` and `detail`, an edit of `anchor` when
/// there is one: under a narrowed room the fixed title and no detail (R64).
pub fn status_content(
    base: &StatusContent,
    anchor: Option<&OwnedEventId>,
    run: RunState,
    detail: Option<&str>,
    narrowed: bool,
) -> Value {
    let mut content = base.clone();
    content.v = CONTENT_VERSION;
    content.run = run;
    content.detail = detail.filter(|text| !text.is_empty()).map(str::to_owned);
    if narrowed {
        NARROWED_STATUS.clone_into(&mut content.title);
        content.detail = None;
    }
    content.anchor = anchor.cloned();
    serde_json::to_value(content).unwrap_or(Value::Null)
}

impl StatusBoard {
    /// Start a board for one turn: a `running` status at once, as an edit
    /// of `anchor` — the session's status anchor — or as the anchor itself
    /// when the session has none yet. `base` is the status every edit
    /// carries, `detail` and `anchor` aside; `gate` is the room's boundary.
    pub fn start(
        port: Arc<dyn EditPort>,
        anchor: Option<OwnedEventId>,
        base: StatusContent,
        gate: Option<Arc<RoomGate>>,
    ) -> StatusBoard {
        let (progress, receiver) = watch::channel(Progress::default());
        let board_gate = gate.clone();
        let task = tokio::spawn(async move {
            let audited = gate.clone();
            let status = move |anchor: Option<&OwnedEventId>, run, detail: &str, narrowed| {
                if narrowed {
                    if let Some(gate) = &audited {
                        gate.suppressed("status");
                    }
                }
                status_content(&base, anchor, run, Some(detail), narrowed)
            };
            let detail = receiver.borrow().text.clone();
            let (sent, started, narrowed) = deliver_gated(
                port.as_ref(),
                STATUS,
                gate.as_deref(),
                &|narrowed| status(anchor.as_ref(), RunState::Running, &detail, narrowed),
                Instant::now(),
            )
            .await;
            let anchor = anchor.unwrap_or(sent);
            let status = Arc::new(status);
            let running = Arc::clone(&status);
            let content: EditContent = Arc::new(move |anchor, detail, narrowed| {
                Some(running(Some(anchor), RunState::Running, detail, narrowed))
            });
            let paced = pace(
                Arc::clone(&port),
                STATUS,
                anchor.clone(),
                content,
                gate.clone(),
                (if narrowed { String::new() } else { detail }, narrowed),
                started,
                receiver.clone(),
            )
            .await;
            // The pacer also returns when the board's owner went without
            // finishing: only a finish says the answer landed.
            let (finished, detail) = {
                let now = receiver.borrow();
                (now.finished, now.text.clone())
            };
            if !finished {
                return None;
            }
            // The turn is over: the status says so, with the last counts.
            deliver_gated(
                port.as_ref(),
                STATUS,
                gate.as_deref(),
                &|narrowed| status(Some(&anchor), RunState::Idle, &detail, narrowed),
                paced.last_send + MIN_EDIT_GAP,
            )
            .await;
            Some(anchor)
        });
        StatusBoard {
            progress,
            task: Mutex::new(Some(task)),
            gate: board_gate,
        }
    }

    /// The turn's progress now.
    pub fn update(&self, progress: ToolProgress) {
        let detail = progress.detail();
        self.progress.send_modify(|now| now.text = detail);
    }

    /// The session's label is `label` now: the gate checks it from the next
    /// attempt on, and the pacer looks again even if no count changed.
    pub fn relabel(&self, label: &keeper_core::agents::label::Label) {
        if let Some(gate) = &self.gate {
            gate.set_label(label.clone());
        }
        self.progress.send_modify(|_| {});
    }

    /// The answer's final edit was accepted (`delivered`, which only
    /// [`MatrixSink::finish`] makes): the status goes `idle`. The session's
    /// status anchor, when there is one.
    pub async fn finish(&self, _delivered: &Delivered) -> Option<OwnedEventId> {
        self.progress.send_modify(|now| now.finished = true);
        let task = self.task.lock().unwrap_or_else(|p| p.into_inner()).take();
        match task {
            // Held so that a finish dropped mid-wait still stops the task.
            Some(task) => {
                let mut task = AbortOnDrop(task);
                (&mut task.0).await.ok().flatten()
            }
            None => None,
        }
    }
}

impl Drop for StatusBoard {
    fn drop(&mut self) {
        if let Some(task) = self.task.lock().unwrap_or_else(|p| p.into_inner()).take() {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One send the fake port saw.
    #[derive(Debug, Clone)]
    struct Sent {
        at: Instant,
        event_type: String,
        content: Value,
        txn: OwnedTransactionId,
        accepted: bool,
    }

    /// A port answering from a script, then accepting everything; its room
    /// holds `members`.
    struct FakePort {
        sent: Mutex<Vec<Sent>>,
        script: Mutex<Vec<Option<AgentMatrixError>>>,
        members: Mutex<std::collections::BTreeSet<matrix_sdk::ruma::OwnedUserId>>,
    }

    impl FakePort {
        fn new(script: Vec<Option<AgentMatrixError>>) -> Arc<FakePort> {
            Arc::new(FakePort {
                sent: Mutex::new(Vec::new()),
                script: Mutex::new(script.into_iter().rev().collect()),
                members: Mutex::default(),
            })
        }

        fn join(&self, user: &str) {
            self.members
                .lock()
                .expect("lock")
                .insert(matrix_sdk::ruma::OwnedUserId::try_from(user).expect("user"));
        }

        fn sent(&self) -> Vec<Sent> {
            self.sent.lock().expect("lock").clone()
        }
    }

    impl EditPort for FakePort {
        fn send<'a>(
            &'a self,
            event_type: &'a str,
            content: Value,
            txn: OwnedTransactionId,
        ) -> SendFuture<'a> {
            Box::pin(async move {
                let answer = self.script.lock().expect("lock").pop().flatten();
                let n = {
                    let mut sent = self.sent.lock().expect("lock");
                    sent.push(Sent {
                        at: Instant::now(),
                        event_type: event_type.to_owned(),
                        content,
                        txn,
                        accepted: answer.is_none(),
                    });
                    sent.len()
                };
                match answer {
                    Some(error) => Err(error),
                    None => Ok(OwnedEventId::try_from(format!("$e{n}:example.org")).expect("id")),
                }
            })
        }

        fn members(&self) -> MembersFuture<'_> {
            Box::pin(async move { Ok(self.members.lock().expect("lock").clone()) })
        }
    }

    /// A {tgorka} label's gate over `port`, its room holding tgorka.
    fn gate(port: &Arc<FakePort>) -> Arc<RoomGate> {
        use keeper_core::agents::label::{Integrity, Label, Readers};
        port.join("@tgorka:h");
        let tgorka = matrix_sdk::ruma::OwnedUserId::try_from("@tgorka:h").expect("user");
        Arc::new(RoomGate::new(
            Arc::clone(port) as Arc<dyn EditPort>,
            None,
            Vec::new(),
            Label {
                readers: Readers::Only([tgorka].into()),
                integrity: Integrity::Owner,
                local_only: false,
            },
            None,
        ))
    }

    fn statuses(port: &FakePort) -> Vec<Value> {
        port.sent()
            .into_iter()
            .filter(|s| s.event_type == STATUS && s.accepted)
            .map(|s| s.content)
            .collect()
    }

    /// R169: a room that widens past the label before any count was sent
    /// is a change of its own — the running status says the fixed sentence
    /// with no detail while the turn is still open, and a later count adds
    /// no detail back.
    #[tokio::test(start_paused = true)]
    async fn a_status_narrowed_before_any_count_says_so_while_the_turn_runs() {
        let port = FakePort::new(Vec::new());
        let gate = gate(&port);
        let board = StatusBoard::start(port.clone(), None, base(), Some(Arc::clone(&gate)));
        sleep(Duration::from_secs(1)).await;
        assert_eq!(statuses(&port).last().expect("running")["title"], "Nixi");
        port.join("@marta:h");
        board.relabel(&gate.label());
        sleep(Duration::from_secs(1)).await;
        let last = statuses(&port).last().cloned().expect("a status");
        assert_eq!(last["title"], NARROWED_STATUS, "{last}");
        assert_eq!(last["run"], "running");
        assert!(last.get("detail").is_none_or(Value::is_null), "{last}");
        board.update(ToolProgress { reads: 1, calls: 2 });
        sleep(Duration::from_secs(1)).await;
        for status in statuses(&port).iter().skip(1) {
            assert_eq!(status["title"], NARROWED_STATUS, "{status}");
        }
        drop(board);
    }

    /// R169: the first status, retried after a 429, is made at its retry:
    /// a room that widened during the wait gets the fixed sentence, never
    /// the title the first attempt was made with.
    #[tokio::test(start_paused = true)]
    async fn a_retried_status_is_made_from_the_room_as_it_is_then() {
        let port = FakePort::new(vec![Some(AgentMatrixError::RateLimited {
            retry_after_ms: Some(2000),
        })]);
        let gate = gate(&port);
        let board = StatusBoard::start(port.clone(), None, base(), Some(gate));
        sleep(Duration::from_millis(500)).await;
        assert_eq!(port.sent()[0].content["title"], "Nixi");
        port.join("@marta:h");
        sleep(Duration::from_secs(3)).await;
        let accepted = statuses(&port);
        assert_eq!(accepted.len(), 1, "{accepted:?}");
        assert_eq!(accepted[0]["title"], NARROWED_STATUS);
        drop(board);
    }

    fn anchor() -> OwnedEventId {
        OwnedEventId::try_from("$anchor:example.org").expect("id")
    }

    fn whole(sent: &Sent) -> String {
        sent.content["m.new_content"]["body"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    }

    /// 200 deltas over 2 s: at most 5 edits, each ≥ 400 ms after the anchor
    /// or the previous edit, then one final edit with the whole text.
    #[tokio::test(start_paused = true)]
    async fn edits_are_never_closer_than_400_ms() {
        let port = FakePort::new(Vec::new());
        let anchor_at = Instant::now();
        let sink = MatrixSink::start(port.clone(), anchor(), anchor_at, None);
        let mut answer = String::new();
        for n in 0..200 {
            sleep(Duration::from_millis(10)).await;
            let delta = format!("w{n} ");
            answer.push_str(&delta);
            sink.event(BotStreamEvent::Delta { text: delta });
        }
        let delivered = sink.finish(&answer, "").await;

        let sent = port.sent();
        let (edits, last) = sent.split_at(sent.len() - 1);
        assert!(edits.len() <= 5, "{} edits", edits.len());
        assert_eq!(delivered.edits, edits.len());
        let mut previous = anchor_at;
        for edit in sent.iter() {
            assert!(
                edit.at - previous >= MIN_EDIT_GAP,
                "{:?}",
                edit.at - previous
            );
            previous = edit.at;
        }
        assert_eq!(whole(&last[0]), answer);
    }

    /// A `RateLimited{2000}` delays the next edit by at least 2000 ms, and
    /// exactly one edit follows the wait before the ordinary pace resumes.
    #[tokio::test(start_paused = true)]
    async fn a_429_waits_retry_after_then_sends_the_whole_text_once() {
        let port = FakePort::new(vec![
            None,
            Some(AgentMatrixError::RateLimited {
                retry_after_ms: Some(2000),
            }),
        ]);
        let sink = MatrixSink::start(port.clone(), anchor(), Instant::now(), None);
        let mut answer = String::new();
        for n in 0..300 {
            sleep(Duration::from_millis(10)).await;
            let delta = format!("w{n} ");
            answer.push_str(&delta);
            sink.push(&delta);
        }
        sink.finish(&answer, "").await;

        let sent = port.sent();
        let limited = sent
            .iter()
            .position(|s| !s.accepted)
            .expect("the 429 was met");
        let after = &sent[limited + 1];
        assert!(
            after.at - sent[limited].at >= Duration::from_millis(2000),
            "{:?}",
            after.at - sent[limited].at
        );
        // The one owed edit carries everything streamed by then; the next
        // send is a full gap later, never a drained backlog.
        assert!(sent[limited + 2].at - after.at >= MIN_EDIT_GAP);
        assert!(whole(after).len() > whole(&sent[limited]).len());
    }

    /// The final edit survives a 429, a 502 and a timeout, under one
    /// transaction id, and carries the whole answer.
    #[tokio::test(start_paused = true)]
    async fn the_final_edit_is_retried_until_accepted_and_carries_the_whole_answer() {
        let port = FakePort::new(vec![
            Some(AgentMatrixError::RateLimited {
                retry_after_ms: Some(500),
            }),
            Some(AgentMatrixError::Other("HTTP 502".to_owned())),
            Some(AgentMatrixError::Network(
                "no answer within 10 s".to_owned(),
            )),
        ]);
        let sink = MatrixSink::start(port.clone(), anchor(), Instant::now(), None);
        let delivered = sink.finish("the whole answer", "").await;

        let sent = port.sent();
        assert_eq!(sent.len(), 4, "{sent:?}");
        assert!(sent.iter().all(|s| s.txn == sent[0].txn));
        assert!(sent[3].accepted);
        assert_eq!(whole(&sent[3]), "the whole answer");
        assert_eq!(delivered.final_event.as_str(), "$e4:example.org");
    }

    /// The edit's fallback `body` stays within 1 KiB however long the answer,
    /// while `m.new_content` carries all of it.
    #[tokio::test(start_paused = true)]
    async fn the_fallback_body_is_at_most_1_kib_and_the_new_content_is_whole() {
        let port = FakePort::new(Vec::new());
        let sink = MatrixSink::start(port.clone(), anchor(), Instant::now(), None);
        let answer = "ż".repeat(5000);
        sink.push(&answer);
        sleep(Duration::from_secs(1)).await;
        sink.finish(&answer, "").await;
        let sent = port.sent();
        assert_eq!(sent.len(), 2);
        for edit in &sent {
            assert_eq!(edit.event_type, "m.room.message");
            let body = edit.content["body"].as_str().expect("a body");
            assert!(body.len() <= events::FALLBACK_BODY_MAX, "{}", body.len());
            assert_eq!(whole(edit), answer);
            assert_eq!(edit.content["m.relates_to"]["rel_type"], "m.replace");
            assert_eq!(edit.content["m.relates_to"]["event_id"], anchor().as_str());
        }
    }

    #[test]
    fn a_long_answer_is_cut_on_a_char_boundary_with_the_artifact_named() {
        let answer = format!("a{}", "ż".repeat(FINAL_CUT_BYTES));
        let message = cut(&answer, "artifacts/answer-X.md").expect("cut");
        let (head, tail) = message
            .split_once("\n\nThe full answer is in ")
            .expect("the link");
        assert!(head.len() <= FINAL_CUT_BYTES && head.len() > FINAL_CUT_BYTES - 4);
        assert!(answer.starts_with(head));
        assert_eq!(tail, "artifacts/answer-X.md");
        assert_eq!(cut("short", "x"), None);
    }

    fn base() -> StatusContent {
        StatusContent {
            v: CONTENT_VERSION,
            session: "60-sessions/main".to_owned(),
            kind: keeper_core::agents::session::SessionKind::Main,
            title: "Nixi".to_owned(),
            agent: matrix_sdk::ruma::OwnedUserId::try_from("@nixi:example.org").expect("user"),
            host: "electra".to_owned(),
            epoch: 1,
            run: RunState::Running,
            detail: None,
            waiting: None,
            anchor: None,
        }
    }

    fn failing(n: usize) -> Vec<Option<AgentMatrixError>> {
        (0..n)
            .map(|_| Some(AgentMatrixError::Other("HTTP 502".to_owned())))
            .collect()
    }

    /// A turn cancelled while its answer's final edit is still being
    /// retried never shows the session `idle` — that would tell a device
    /// following the answer that a partial answer was whole — and nothing
    /// is sent once the turn is gone.
    #[tokio::test(start_paused = true)]
    async fn a_turn_cancelled_before_its_final_edit_never_says_idle() {
        // The board's `running` lands; every send after it fails for a
        // while, then the homeserver accepts again.
        let mut script = vec![None];
        script.extend(failing(40));
        let port = FakePort::new(script);
        let sink = MatrixSink::start(port.clone(), anchor(), Instant::now(), None);
        let board = StatusBoard::start(port.clone(), None, base(), None);
        sink.push("Half an answer");
        let turn = async move {
            let delivered = sink.finish("Half an answer", "").await;
            board.finish(&delivered).await
        };
        assert!(
            tokio::time::timeout(Duration::from_secs(5), turn)
                .await
                .is_err(),
            "the final edit is still being retried"
        );
        let at_cancel = port.sent().len();
        sleep(Duration::from_secs(600)).await;
        let sent = port.sent();
        assert_eq!(sent.len(), at_cancel, "{sent:?}");
        let runs: Vec<&str> = sent
            .iter()
            .filter(|s| s.event_type == STATUS)
            .filter_map(|s| s.content["run"].as_str())
            .collect();
        assert_eq!(runs, ["running"]);
    }

    /// A board dropped while its first status is still being retried stops
    /// retrying: its task does not outlive the turn that owned it.
    #[tokio::test(start_paused = true)]
    async fn a_dropped_board_stops_its_task() {
        let port = FakePort::new(failing(200));
        let board = StatusBoard::start(port.clone(), None, base(), None);
        sleep(Duration::from_secs(3)).await;
        assert!(!port.sent().is_empty());
        drop(board);
        let at_drop = port.sent().len();
        sleep(Duration::from_secs(600)).await;
        assert_eq!(port.sent().len(), at_drop);
    }
}
