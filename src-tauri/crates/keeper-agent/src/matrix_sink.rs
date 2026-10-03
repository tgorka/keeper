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

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use keeper_core::agents::events::{
    self, RunState, StatusContent, TurnRef, CONTENT_VERSION, STATUS, TURN,
};
use keeper_core::agents::matrix::{AgentClient, AgentMatrixError};
use keeper_core::agents::redact::redact_secrets;
use keeper_core::vm::BotStreamEvent;
use matrix_sdk::ruma::{OwnedEventId, OwnedRoomId, OwnedTransactionId, TransactionId};
use serde_json::{json, Value};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{sleep, sleep_until, Instant};

use crate::ports::TurnSink;

/// No two edits of one anchor are closer than this (R18, NFR-113).
pub const MIN_EDIT_GAP: Duration = Duration::from_millis(400);

/// The longest final message, in bytes (R23). Longer answers are cut on a
/// `char` boundary and point to an artifact holding the whole text.
///
/// Measured, not chosen (90.5 acceptance 14): the largest text whose
/// encrypted final edit the Synapse test homeserver accepted was 47 061
/// bytes (2026-10-03; the server's 64 KiB event cap after Megolm and base64),
/// so R23's 60 KiB does not fit. This is that, less room for the artifact
/// sentence, rounded down to 1 KiB (`docs/agents.md` § Measured).
pub const FINAL_CUT_BYTES: usize = 45 * 1024;

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

/// An answer's anchor: the placeholder and the turn it belongs to.
pub fn anchor_content(session: &str, user_line: &str) -> Value {
    json!({
        "msgtype": "m.text",
        "body": PLACEHOLDER,
        TURN: TurnRef { session: session.to_owned(), line: user_line.to_owned() },
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
    let txn = TransactionId::new();
    let mut not_before = not_before;
    let mut backoff = BACKOFF_FIRST;
    loop {
        sleep_until(not_before).await;
        match port.send(event_type, content.clone(), txn.clone()).await {
            Ok(event) => return (event, Instant::now()),
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

/// Makes an edit's content from the anchor and the text.
type EditContent = Arc<dyn Fn(&OwnedEventId, &str) -> Value + Send + Sync>;

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
/// the anchor and the text.
async fn pace(
    port: Arc<dyn EditPort>,
    event_type: &'static str,
    anchor: OwnedEventId,
    content: EditContent,
    mut last_send: Instant,
    mut progress: watch::Receiver<Progress>,
) -> Paced {
    let mut edits = 0;
    let mut sent = String::new();
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
        if text == sent {
            owed = false;
            continue;
        }
        match port
            .send(event_type, content(&anchor, &text), TransactionId::new())
            .await
        {
            Ok(_) => {
                last_send = Instant::now();
                sent = text;
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

/// How a streamed answer ended in the room.
#[derive(Debug, Clone)]
pub struct Delivered {
    /// The final edit's event.
    pub final_event: OwnedEventId,
    /// When the homeserver accepted it.
    pub accepted_at: Instant,
    /// How many edits went out before it.
    pub edits: usize,
}

/// One answer's sink: deltas in, paced edits of its anchor out.
pub struct MatrixSink {
    port: Arc<dyn EditPort>,
    anchor: OwnedEventId,
    text: Mutex<String>,
    progress: watch::Sender<Progress>,
    pacer: Mutex<Option<JoinHandle<Paced>>>,
    /// No more of the text reaches the room before the final edit.
    withheld: AtomicBool,
}

impl MatrixSink {
    /// Start pacing edits of `anchor`, which the homeserver accepted at
    /// `anchor_at`.
    pub fn start(port: Arc<dyn EditPort>, anchor: OwnedEventId, anchor_at: Instant) -> MatrixSink {
        let (progress, receiver) = watch::channel(Progress::default());
        // An edit carries the log's redaction (S-17): the room never shows
        // a secret the log does not.
        let content: EditContent =
            Arc::new(|anchor, text| events::edit_content(anchor, &redact_secrets(text).text));
        let pacer = tokio::spawn(pace(
            Arc::clone(&port),
            "m.room.message",
            anchor.clone(),
            content,
            anchor_at,
            receiver,
        ));
        MatrixSink {
            port,
            anchor,
            text: Mutex::new(String::new()),
            progress,
            pacer: Mutex::new(Some(pacer)),
            withheld: AtomicBool::new(false),
        }
    }

    /// Add streamed text. Edits carry at most [`FINAL_CUT_BYTES`] of it: the
    /// rest reaches the room only through the final message's artifact.
    pub fn push(&self, delta: &str) {
        let mut text = self.text.lock().unwrap_or_else(|p| p.into_inner());
        text.push_str(delta);
        if self.withheld.load(Ordering::Relaxed) {
            return;
        }
        let shown = prefix(&text, FINAL_CUT_BYTES).to_owned();
        self.progress.send_modify(|progress| progress.text = shown);
    }

    /// Stop sending the text as it grows: it is still kept, and the final
    /// edit is the only send left.
    pub fn withhold(&self) {
        self.withheld.store(true, Ordering::Relaxed);
    }

    /// The text streamed so far.
    pub fn text(&self) -> String {
        self.text.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// Stop the paced edits and deliver `final_text` as the final edit,
    /// retried until accepted.
    pub async fn finish(&self, final_text: &str) -> Delivered {
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
        let (final_event, accepted_at) = deliver(
            self.port.as_ref(),
            "m.room.message",
            events::edit_content(&self.anchor, final_text),
            paced.last_send + MIN_EDIT_GAP,
        )
        .await;
        Delivered {
            final_event,
            accepted_at,
            edits: paced.edits,
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
pub struct StatusBoard {
    progress: watch::Sender<Progress>,
    task: Mutex<Option<JoinHandle<Option<OwnedEventId>>>>,
}

impl StatusBoard {
    /// Start a board for one turn. `anchor` is the session's status anchor
    /// when it has one; the first progress creates it otherwise. `base` is
    /// the status every edit carries, `detail` and `anchor` aside.
    pub fn start(
        port: Arc<dyn EditPort>,
        anchor: Option<OwnedEventId>,
        base: StatusContent,
    ) -> StatusBoard {
        let (progress, mut receiver) = watch::channel(Progress::default());
        let task = tokio::spawn(async move {
            let status = move |anchor: Option<&OwnedEventId>, run: RunState, detail: &str| {
                let mut content = base.clone();
                content.v = CONTENT_VERSION;
                content.run = run;
                content.detail = Some(detail.to_owned());
                content.anchor = anchor.cloned();
                serde_json::to_value(content).unwrap_or(Value::Null)
            };
            let anchor = match anchor {
                Some(anchor) => anchor,
                None => {
                    if receiver.changed().await.is_err() || receiver.borrow().finished {
                        return None;
                    }
                    let detail = receiver.borrow_and_update().text.clone();
                    let (anchor, _) = deliver(
                        port.as_ref(),
                        STATUS,
                        status(None, RunState::Running, &detail),
                        Instant::now(),
                    )
                    .await;
                    anchor
                }
            };
            let status = Arc::new(status);
            let running = Arc::clone(&status);
            let content: EditContent =
                Arc::new(move |anchor, detail| running(Some(anchor), RunState::Running, detail));
            let paced = pace(
                Arc::clone(&port),
                STATUS,
                anchor.clone(),
                content,
                Instant::now(),
                receiver.clone(),
            )
            .await;
            // The turn is over: the status says so, with the last counts.
            let detail = receiver.borrow().text.clone();
            if !detail.is_empty() {
                deliver(
                    port.as_ref(),
                    STATUS,
                    status(Some(&anchor), RunState::Idle, &detail),
                    paced.last_send + MIN_EDIT_GAP,
                )
                .await;
            }
            Some(anchor)
        });
        StatusBoard {
            progress,
            task: Mutex::new(Some(task)),
        }
    }

    /// The turn's progress now.
    pub fn update(&self, progress: ToolProgress) {
        let detail = progress.detail();
        self.progress.send_modify(|now| now.text = detail);
    }

    /// Stop the board; the session's status anchor, when there is one.
    pub async fn finish(&self) -> Option<OwnedEventId> {
        self.progress.send_modify(|now| now.finished = true);
        let task = self.task.lock().unwrap_or_else(|p| p.into_inner()).take();
        match task {
            Some(task) => task.await.ok().flatten(),
            None => None,
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

    /// A port answering from a script, then accepting everything.
    struct FakePort {
        sent: Mutex<Vec<Sent>>,
        script: Mutex<Vec<Option<AgentMatrixError>>>,
    }

    impl FakePort {
        fn new(script: Vec<Option<AgentMatrixError>>) -> Arc<FakePort> {
            Arc::new(FakePort {
                sent: Mutex::new(Vec::new()),
                script: Mutex::new(script.into_iter().rev().collect()),
            })
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
        let sink = MatrixSink::start(port.clone(), anchor(), anchor_at);
        let mut answer = String::new();
        for n in 0..200 {
            sleep(Duration::from_millis(10)).await;
            let delta = format!("w{n} ");
            answer.push_str(&delta);
            sink.event(BotStreamEvent::Delta { text: delta });
        }
        let delivered = sink.finish(&answer).await;

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
        let sink = MatrixSink::start(port.clone(), anchor(), Instant::now());
        let mut answer = String::new();
        for n in 0..300 {
            sleep(Duration::from_millis(10)).await;
            let delta = format!("w{n} ");
            answer.push_str(&delta);
            sink.push(&delta);
        }
        sink.finish(&answer).await;

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
        let sink = MatrixSink::start(port.clone(), anchor(), Instant::now());
        let delivered = sink.finish("the whole answer").await;

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
        let sink = MatrixSink::start(port.clone(), anchor(), Instant::now());
        let answer = "ż".repeat(5000);
        sink.push(&answer);
        sleep(Duration::from_secs(1)).await;
        sink.finish(&answer).await;
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
}
