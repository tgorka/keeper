//! Running a turn (FR-372, FR-373, FR-374): the tool loop, the partial row
//! and the stream.
//!
//! The lifecycle is a **string subscription id**: [`spawn_turn`] registers the
//! producer under it, and [`stop`] is idempotent — stopping an id that already
//! finished is a no-op, so a racing unmount is not an error. Stop is
//! *cooperative* rather than an abort: it fires
//! `keeper_core::bots::chat::CancelHandle`, so the driver unwinds through its
//! own cancel path and writes the partial row, where a bare
//! `JoinHandle::abort` would drop the answer on the floor mid-write.

use std::collections::HashMap;
use std::sync::Arc;

use keeper_core::bots::chat::{self, CancelHandle, ChatEvent, ChatOptions};
use keeper_core::bots::context_files::ContextBundle;
use keeper_core::bots::http;
use keeper_core::bots::session;
use keeper_core::bots::tools::{self, ToolLoop, ToolLoopEvent, ToolLoopOptions};
use keeper_core::vm::{BotContextBundleVm, BotMessageVm, BotStreamEvent, BotToolCallVm};

use crate::ports::{TurnEnd, TurnSink};
use crate::turn::{now_ms, OpenedTurn, Turn};

/// How many bytes of a growing answer may sit in memory before the partial row
/// on disk is rewritten.
///
/// Not a timer, and not every delta. Every delta would be one `UPDATE` per
/// token — a write per twenty bytes, on the database the account registry
/// shares. A timer would be a second clock in a process whose AD-62 already
/// refuses one. A byte threshold spends a bounded number of writes per answer
/// and bounds what a crash loses to the last half-kilobyte, which is a sentence
/// rather than an answer.
pub const FLUSH_BYTES: usize = 512;

/// One answer being streamed. Dropping it aborts the task, which is the
/// backstop; [`stop`] uses `cancel` instead.
struct LiveStream {
    cancel: CancelHandle,
    task: tokio::task::JoinHandle<()>,
    /// The conversation this answer is arriving into, so [`owns_turn`] can
    /// tell that this process holds the turn.
    session_id: String,
}

impl Drop for LiveStream {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Answers currently streaming, keyed by subscription id.
///
/// Several may run at once — a person who starts an answer, switches
/// conversation and starts another has two, and killing the first would be
/// keeper deciding their question was stale.
fn streams() -> std::sync::MutexGuard<'static, HashMap<String, LiveStream>> {
    static STREAMS: std::sync::LazyLock<std::sync::Mutex<HashMap<String, LiveStream>>> =
        std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));
    STREAMS
        .lock()
        // A poisoned lock means a driver panicked mid-answer. The map holds
        // cancel handles and join handles and nothing else, so there is no torn
        // state to protect and refusing every later send would be the worse
        // failure.
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Whether this process is streaming a turn of `session_id` right now
/// (Story 63.7). A device that has the stream does not read the transcript
/// underneath it.
pub fn owns_turn(session_id: &str) -> bool {
    streams().values().any(|live| live.session_id == session_id)
}

/// Stop a streaming answer by subscription id. Idempotent.
pub fn stop(subscription_id: &str) {
    if let Some(live) = streams().get(subscription_id) {
        live.cancel.cancel();
    }
}

/// Send `Opened`, then the context, then spawn the driver and register it;
/// returns the subscription id.
///
/// Must be called inside a tokio runtime.
pub fn spawn_turn(opened: OpenedTurn, sink: Arc<dyn TurnSink>) -> String {
    let OpenedTurn {
        turn,
        opened,
        context,
        subscription_id,
        ..
    } = opened;
    sink.event(opened);
    // After `Opened`, so the pane has the row the disclosure belongs to.
    emit_context(sink.as_ref(), context.as_ref());
    let (cancel, signal) = chat::cancellation();
    let session_id = turn.session_id.clone();
    let retire = subscription_id.clone();
    let task = tokio::spawn(async move {
        drive(turn, signal, sink).await;
        // Retire self. `remove` drops the `LiveStream`, whose `Drop` aborts a
        // task that has already finished — which aborts nothing.
        streams().remove(&retire);
    });
    streams().insert(
        subscription_id.clone(),
        LiveStream {
            cancel,
            task,
            session_id,
        },
    );
    subscription_id
}

/// Run one turn — every completion of it, tool rounds included — into the
/// sink and into the store.
///
/// Every write to the partial row happens here, so there is exactly one writer
/// per answer and the flush policy is stated once. The turn is
/// `run_tool_loop_reporting` whether or not tools were offered: with an empty
/// `tools` array it is one completion, so there is one code path. A finished
/// call becomes a [`BotStreamEvent::ToolResult`] **as it completes**, and its
/// audit row was written before its effect (NFR-47).
async fn drive(mut turn: Turn, signal: chat::CancelSignal, sink: Arc<dyn TurnSink>) {
    let client = match http::client(turn.read_timeout) {
        Ok(client) => client,
        Err(error) => {
            close_failed(&turn, sink.as_ref(), &error.to_string());
            return;
        }
    };
    let options = ChatOptions {
        read_timeout: turn.read_timeout,
        ..ChatOptions::default()
    };
    sink.request_sent();

    // The host is the port's to build: the signal an approval waits on exists
    // only here.
    let profiles = std::mem::take(&mut turn.profiles);
    let host = turn.drive.host(turn.host_ids(), profiles, signal.clone());
    let context = ToolLoop {
        client: &client,
        endpoint: &turn.endpoint,
        host: host.as_ref(),
        default_profile_id: &turn.default_profile_id,
    };

    // Accumulated in the sink and flushed by byte count — see `FLUSH_BYTES`.
    // Across rounds, not per round: the text a model wrote before it called a
    // tool and the text it wrote after are one answer on screen, so they are
    // one answer in the row.
    let mut content = String::new();
    let mut unflushed = 0usize;
    let mut events = |event: ToolLoopEvent| match event {
        ToolLoopEvent::Chat(ChatEvent::FirstToken { after_ms }) => {
            sink.event(BotStreamEvent::FirstToken { after_ms });
        }
        ToolLoopEvent::Chat(ChatEvent::ContentDelta(text)) => {
            unflushed += text.len();
            content.push_str(&text);
            sink.answer_text(&text);
            sink.event(BotStreamEvent::Delta { text });
            if unflushed >= FLUSH_BYTES {
                unflushed = 0;
                // A failed flush is not fatal to the answer on screen: the row
                // stays partial, which is exactly what it is.
                if let Err(error) =
                    session::set_message_content(&turn.dir, &turn.assistant_id, &content)
                {
                    tracing::warn!(%error, "bots: could not flush a partial answer");
                }
            }
        }
        ToolLoopEvent::Chat(ChatEvent::ReasoningDelta(text)) => {
            sink.event(BotStreamEvent::Reasoning { text });
        }
        ToolLoopEvent::Chat(ChatEvent::ToolCallDelta { name, .. }) => {
            // The name, as the fragments arrive: the row itself follows from
            // the reporter once the call has run.
            if let Some(name) = name {
                sink.event(BotStreamEvent::ToolCall { name });
            }
        }
        // Usage and the finish reason are written once, by the close below,
        // from the outcome.
        ToolLoopEvent::Chat(ChatEvent::Usage(_) | ChatEvent::Finished { .. }) => {}
        ToolLoopEvent::Chat(ChatEvent::Failed { error }) => {
            tracing::warn!(%error, "bots: a stream failed after it had produced bytes");
        }
        // A later round's prose starts on its own paragraph, on screen and in
        // the row alike. The separator is not model prose, so it never reaches
        // `answer_text`.
        ToolLoopEvent::RoundStarted { round, .. } => {
            if round > 0 && !content.is_empty() && !content.ends_with('\n') {
                content.push_str("\n\n");
                sink.event(BotStreamEvent::Delta {
                    text: "\n\n".to_owned(),
                });
            }
        }
        // The row carries the refusal and `grantDenied`; nothing here needs a
        // second wording.
        ToolLoopEvent::ToolStarted { .. }
        | ToolLoopEvent::ToolFinished { .. }
        | ToolLoopEvent::GrantDenied { .. }
        | ToolLoopEvent::RoundsExhausted { .. } => {}
    };
    let mut report =
        |record: &tools::ToolCallRecord, wire: &chat::ToolCall, outcome: &tools::ToolOutcome| {
            sink.event(BotStreamEvent::ToolResult {
                call: Box::new(BotToolCallVm::compose(
                    record,
                    &tools::arguments_text(wire),
                    Some(outcome),
                )),
            });
        };

    let outcome = tools::run_tool_loop_reporting(
        &context,
        &turn.request,
        &options,
        &ToolLoopOptions::default(),
        signal.clone(),
        &mut events,
        &mut report,
    )
    .await;

    match outcome {
        Ok(ran) => close(
            &turn,
            sink.as_ref(),
            &ran.final_outcome,
            &content,
            ran.calls.len(),
            signal.is_cancelled(),
        ),
        // No stream byte ever existed, so there is nothing to keep. The row
        // stays — marked partial, with the reason — rather than vanishing.
        Err(error) => close_failed(&turn, sink.as_ref(), &error.to_string()),
    }
}

/// Tell the pane what the model was told about the drive, where anything was
/// (Story 61.11, FR-391). Absent when no bundle was built.
fn emit_context(sink: &dyn TurnSink, context: Option<&ContextBundle>) {
    if let Some(bundle) = context {
        sink.event(BotStreamEvent::Context {
            bundle: Box::new(BotContextBundleVm::compose(bundle)),
        });
    }
}

/// Write the finished (or stopped) answer, emit the terminal event, then tell
/// the sink how it ended — after the row, so what a sink acts on is what is
/// stored.
///
/// `content` is the whole turn's prose as the sink accumulated it;
/// `tool_call_count` is every call the loop ran; `cancelled` is whether Stop
/// was pressed. The reason the row and the pane carry is worded here, once.
fn close(
    turn: &Turn,
    sink: &dyn TurnSink,
    outcome: &chat::ChatOutcome,
    content: &str,
    tool_call_count: usize,
    cancelled: bool,
) {
    let reason = match (&outcome.finish_reason, cancelled) {
        (_, true) => Some("Stopped. What had arrived is kept.".to_owned()),
        (chat::FinishReason::Failed, false) => {
            Some("The answer stopped before it finished.".to_owned())
        }
        _ => None,
    };
    let usage = outcome.usage.unwrap_or_default();
    let partial = reason.is_some();
    let finish_reason = finish_word(&outcome.finish_reason);
    let close = session::MessageClose {
        id: &turn.assistant_id,
        content,
        model: outcome.model.as_deref().or(Some(&turn.request.model)),
        prompt_tokens: usage.prompt_tokens.map(i64::from),
        completion_tokens: usage.completion_tokens.map(i64::from),
        total_tokens: usage.total_tokens.map(i64::from),
        ttft_ms: outcome
            .first_token_ms
            .map(|ms| i64::try_from(ms).unwrap_or(i64::MAX)),
        duration_ms: Some(i64::try_from(outcome.total_ms).unwrap_or(i64::MAX)),
        finish_reason: Some(&finish_reason),
        request_id: outcome.response_id.as_deref(),
        tool_call_count: i64::try_from(tool_call_count).unwrap_or(i64::MAX),
        partial,
    };
    if let Err(error) = session::close_message(&turn.dir, close) {
        tracing::warn!(%error, "bots: could not close an answer");
    }
    emit_closed(turn, sink, reason.clone());
    sink.ended(match reason {
        None => TurnEnd::Complete,
        Some(_) if cancelled => TurnEnd::Stopped,
        Some(reason) => TurnEnd::Failed(reason),
    });
}

/// Write a failure that produced no usable outcome, keeping the row partial.
fn close_failed(turn: &Turn, sink: &dyn TurnSink, reason: &str) {
    let close = session::MessageClose {
        id: &turn.assistant_id,
        content: "",
        model: Some(&turn.request.model),
        finish_reason: Some("failed"),
        partial: true,
        ..session::MessageClose::default()
    };
    if let Err(error) = session::close_message(&turn.dir, close) {
        tracing::warn!(%error, "bots: could not record a failed answer");
    }
    emit_closed(turn, sink, Some(reason.to_owned()));
    sink.ended(TurnEnd::Failed(reason.to_owned()));
}

/// Re-read the row and emit the one terminal event, so the surface renders what
/// was actually stored rather than what the producer believed it stored.
fn emit_closed(turn: &Turn, sink: &dyn TurnSink, reason: Option<String>) {
    session::touch_session(&turn.dir, &turn.session_id, now_ms()).ok();
    let stored = session::list_messages(&turn.dir, &turn.session_id)
        .ok()
        .and_then(|rows| {
            rows.into_iter()
                .find(|row| row.id == turn.assistant_id)
                .as_ref()
                .map(BotMessageVm::compose)
        });
    let Some(message) = stored else {
        // The row is gone — the conversation was deleted while the answer was
        // in flight. Nothing to report to a surface that no longer shows it.
        tracing::warn!(
            provider = %turn.provider_id,
            "bots: an answer finished into a conversation that no longer exists"
        );
        return;
    };
    sink.event(BotStreamEvent::Closed {
        message: Box::new(message),
        reason,
    });
}

/// The stored spelling of a finish reason: the provider's own word where it
/// invented one.
pub fn finish_word(reason: &chat::FinishReason) -> String {
    match reason {
        chat::FinishReason::Stop => "stop".to_owned(),
        chat::FinishReason::Length => "length".to_owned(),
        chat::FinishReason::ContentFilter => "content_filter".to_owned(),
        chat::FinishReason::ToolCalls => "tool_calls".to_owned(),
        chat::FinishReason::Cancelled => "cancelled".to_owned(),
        chat::FinishReason::Failed => "failed".to_owned(),
        chat::FinishReason::Other(word) => word.clone(),
    }
}
