//! One agent turn served without a room.
//!
//! A host whose conversation is not a Matrix room — rows in a database, a
//! socket, a test — opens a turn from a person's message and streams it into
//! its own [`TurnSink`]. The turn runs the loop an agent's room turn runs
//! (`run_tool_loop_gated`), bounded by the agent's own `[limits]`, over the
//! tool host the caller built for the session; nothing here reads a drive,
//! writes a session log or names a room. What the host keeps of the turn is
//! what its sink makes of the events: `Opened`, `FirstToken` once, each
//! `Delta` and `Reasoning` as it arrives, `ToolCall` as the model names a
//! tool, `ToolResult` as each call completes, and one `Closed` with the
//! answer's metadata.
//!
//! Before a round's calls run, the sink hears each of them through
//! [`TurnSink::tool_started`], with the wire id its `ToolResult` will carry,
//! so a host can record a call as started before it has any effect.
//!
//! An error a turn ends with never carries the provider's key or a URL's
//! credentials: a gateway that echoes them in its body is redacted before
//! the reason reaches the sink or a log.
//!
//! An approval is the tool host's to ask: a host that parks a call on a
//! person asks its [`crate::ports::ApprovalPort`] inside `ToolHost::run`.

use std::time::{Duration, Instant};

use keeper_core::agents::home::Limits;
use keeper_core::bots::chat::{
    self, CancelSignal, ChatEvent, ChatOptions, ChatRequest, TokenUsage, ToolCall as WireToolCall,
};
use keeper_core::bots::error::BotsError;
use keeper_core::bots::tools::{
    self, ToolCall, ToolHost, ToolLoop, ToolLoopEvent, ToolLoopOptions, ToolOutcome,
};
use keeper_core::bots::{http, Endpoint};
use keeper_core::vm::{BotMessageVm, BotSessionVm, BotStreamEvent, BotToolCallVm};

use crate::drive::finish_word;
use crate::ports::{TurnEnd, TurnSink};
use crate::turn::now_ms;

/// The person's message a turn answers, as the host holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonMessage {
    /// The host's id for it.
    pub id: String,
    /// Its position in the conversation, as the host counts.
    pub seq: i64,
    /// What the person wrote.
    pub text: String,
    /// When it was written, ms since the Unix epoch (UTC).
    pub created_ms: i64,
}

/// One turn of an agent's session, resolved before it runs.
pub struct ServedTurn {
    /// The conversation, as the host names it.
    pub session_id: String,
    /// Its title.
    pub title: String,
    /// The agent answering.
    pub agent_id: String,
    /// The provider its model is reached through.
    pub provider_id: String,
    /// The answer's id: `Opened`'s subscription id and the assistant row's.
    pub turn_id: String,
    /// The message this turn answers.
    pub person: PersonMessage,
    /// Where the model is, with its credential.
    pub endpoint: Endpoint,
    /// The silence budget for the provider.
    pub read_timeout: Duration,
    /// The request: the system prompt, the conversation ending in the
    /// person's message, and the tools on offer.
    pub request: ChatRequest,
    /// The agent's `[limits]`: its rounds and its token budget per turn.
    pub limits: Limits,
    /// The drive profile an unqualified drive path means; empty where the
    /// host mounts no drive.
    pub default_profile_id: String,
}

/// What a stopped answer's row and pane carry.
pub const STOPPED_REASON: &str = "Stopped. What had arrived is kept.";

/// What an answer whose stream broke carries.
pub const BROKE_REASON: &str = "The answer stopped before it finished.";

/// What replaces a secret in an error's text.
pub const REDACTED: &str = "[redacted]";

/// `text` without `endpoint`'s token or the credentials of its base URL.
pub fn redact(text: &str, endpoint: &Endpoint) -> String {
    let mut secrets: Vec<String> = endpoint.token.iter().cloned().collect();
    if let Ok(url) = url::Url::parse(&endpoint.base_url) {
        secrets.extend(
            [url.username(), url.password().unwrap_or_default()]
                .into_iter()
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
        );
    }
    let mut out = text.to_owned();
    for secret in secrets.iter().filter(|s| s.len() >= 4) {
        out = out.replace(secret.as_str(), REDACTED);
    }
    out
}

/// The host a turn's calls run through, announcing each round's calls to
/// the sink before the first of them runs.
struct Announced<'a> {
    inner: &'a dyn ToolHost,
    sink: &'a dyn TurnSink,
}

impl ToolHost for Announced<'_> {
    fn run(&self, call: &ToolCall) -> Result<ToolOutcome, BotsError> {
        self.inner.run(call)
    }

    fn run_named(&self, wire: &WireToolCall) -> Option<ToolOutcome> {
        self.inner.run_named(wire)
    }

    fn prepare_round(&self, calls: &[WireToolCall]) {
        for call in calls {
            self.sink.tool_started(call);
        }
        self.inner.prepare_round(calls);
    }
}

/// Serve `turn` into `sink`, every tool call through `host`, until it ends
/// or `stop` fires; returns how it ended, as [`TurnSink::ended`] was told.
pub async fn serve_turn(
    turn: ServedTurn,
    host: &dyn ToolHost,
    sink: &dyn TurnSink,
    stop: CancelSignal,
) -> TurnEnd {
    let started = Instant::now();
    let opened_ms = now_ms();
    sink.event(BotStreamEvent::Opened {
        subscription_id: turn.turn_id.clone(),
        session: Box::new(BotSessionVm {
            id: turn.session_id.clone(),
            bot_id: turn.agent_id.clone(),
            provider_id: turn.provider_id.clone(),
            title: turn.title.clone(),
            created_ms: turn.person.created_ms,
            updated_ms: opened_ms,
            archived: false,
            remote_session_id: None,
            remote_last_active_ms: None,
            remote_source: None,
        }),
        user: Box::new(message(
            &turn,
            &turn.person.id,
            turn.person.seq,
            "user",
            &turn.person.text,
            turn.person.created_ms,
        )),
        assistant: Box::new(BotMessageVm {
            partial: true,
            ..message(
                &turn,
                &turn.turn_id,
                turn.person.seq + 1,
                "assistant",
                "",
                opened_ms,
            )
        }),
    });

    let client = match http::client(turn.read_timeout) {
        Ok(client) => client,
        Err(error) => {
            return close(
                &turn,
                sink,
                Ending::Error(redact(&error.to_string(), &turn.endpoint)),
                Tally::default(),
                opened_ms,
                started,
            )
        }
    };
    sink.request_sent();
    let announced = Announced { inner: host, sink };
    let context = ToolLoop {
        client: &client,
        endpoint: &turn.endpoint,
        host: &announced,
        default_profile_id: &turn.default_profile_id,
    };

    // The turn's own record, across rounds: a later round's prose starts on
    // its own paragraph, and its tokens count against the same budget.
    let tally = std::sync::Mutex::new(Tally::default());
    let lock = || tally.lock().unwrap_or_else(|p| p.into_inner());
    let mut events = |event: ToolLoopEvent| match event {
        ToolLoopEvent::Chat(ChatEvent::FirstToken { after_ms }) => {
            let first = {
                let mut tally = lock();
                let first = tally.ttft_ms.is_none();
                tally.ttft_ms.get_or_insert(after_ms);
                first
            };
            if first {
                sink.event(BotStreamEvent::FirstToken { after_ms });
            }
        }
        ToolLoopEvent::Chat(ChatEvent::ContentDelta(text)) => {
            lock().content.push_str(&text);
            sink.answer_text(&text);
            sink.event(BotStreamEvent::Delta { text });
        }
        ToolLoopEvent::Chat(ChatEvent::ReasoningDelta(text)) => {
            sink.event(BotStreamEvent::Reasoning { text });
        }
        ToolLoopEvent::Chat(ChatEvent::ToolCallDelta {
            name: Some(name), ..
        }) => {
            sink.event(BotStreamEvent::ToolCall { name });
        }
        // A usage frame is the round's count so far, not an increment: the
        // latest one stands for the round (as the room turn's `round_usage`).
        ToolLoopEvent::Chat(ChatEvent::Usage(usage)) => {
            lock().round_usage = Some(usage);
        }
        ToolLoopEvent::Chat(ChatEvent::Failed { error }) => {
            let error = redact(&error.to_string(), &turn.endpoint);
            tracing::warn!(%error, "agents: a served turn's stream failed after it had produced bytes");
        }
        ToolLoopEvent::RoundStarted { round, .. } => {
            let separate = {
                let mut tally = lock();
                let separate =
                    round > 0 && !tally.content.is_empty() && !tally.content.ends_with('\n');
                if separate {
                    tally.content.push_str("\n\n");
                }
                separate
            };
            if separate {
                sink.event(BotStreamEvent::Delta {
                    text: "\n\n".to_owned(),
                });
            }
        }
        ToolLoopEvent::Chat(_)
        | ToolLoopEvent::ToolStarted { .. }
        | ToolLoopEvent::ToolFinished { .. }
        | ToolLoopEvent::GrantDenied { .. }
        | ToolLoopEvent::RoundsExhausted { .. } => {}
    };
    let mut report =
        |record: &tools::ToolCallRecord, wire: &chat::ToolCall, outcome: &tools::ToolOutcome| {
            lock().calls += 1;
            sink.event(BotStreamEvent::ToolResult {
                call: Box::new(BotToolCallVm::compose(
                    record,
                    &tools::arguments_text(wire),
                    Some(outcome),
                )),
            });
        };
    // The agent's token budget is the room turn's (R111): a round past it
    // is not sent.
    let mut gate = |_round: usize| {
        let mut tally = lock();
        tally.fold_round();
        let spent = u64::from(tally.prompt_tokens.unwrap_or(0))
            + u64::from(tally.completion_tokens.unwrap_or(0));
        if keeper_core::agents::helper::spent(turn.limits.tokens_per_turn, spent) {
            return Err(BotsError::Tool {
                detail: keeper_core::agents::helper::TURN_SPENT.to_owned(),
            });
        }
        Ok(())
    };
    let result = tools::run_tool_loop_gated(
        &context,
        &turn.request,
        &ChatOptions {
            read_timeout: turn.read_timeout,
            ..ChatOptions::default()
        },
        &ToolLoopOptions {
            max_rounds: usize::try_from(turn.limits.rounds_per_turn)
                .unwrap_or(tools::MAX_TOOL_ROUNDS)
                .max(1),
            ..ToolLoopOptions::default()
        },
        stop.clone(),
        &mut events,
        &mut report,
        &mut gate,
    )
    .await;
    let mut tally = std::mem::take(&mut *lock());
    tally.fold_round();
    let ending = match result {
        Ok(_) if stop.is_cancelled() => Ending::Stopped,
        Ok(ran) => Ending::Finished(ran.final_outcome),
        Err(error) => Ending::Error(redact(&error.to_string(), &turn.endpoint)),
    };
    close(&turn, sink, ending, tally, opened_ms, started)
}

/// What the loop left for the close.
#[derive(Default)]
struct Tally {
    content: String,
    ttft_ms: Option<u64>,
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
    total_tokens: Option<u32>,
    /// The round under way's latest usage frame.
    round_usage: Option<TokenUsage>,
    calls: usize,
}

impl Tally {
    /// Count the round that just ended once, from its latest usage frame.
    fn fold_round(&mut self) {
        let Some(usage) = self.round_usage.take() else {
            return;
        };
        let add = |sum: &mut Option<u32>, n: Option<u32>| {
            if let Some(n) = n {
                *sum = Some(sum.unwrap_or(0).saturating_add(n));
            }
        };
        add(&mut self.prompt_tokens, usage.prompt_tokens);
        add(&mut self.completion_tokens, usage.completion_tokens);
        add(&mut self.total_tokens, usage.total_tokens);
    }
}

enum Ending {
    Finished(chat::ChatOutcome),
    Stopped,
    Error(String),
}

/// One message of the turn as the stream's view model shows it.
fn message(
    turn: &ServedTurn,
    id: &str,
    seq: i64,
    role: &str,
    content: &str,
    created_ms: i64,
) -> BotMessageVm {
    BotMessageVm {
        id: id.to_owned(),
        session_id: turn.session_id.clone(),
        seq,
        role: role.to_owned(),
        content: content.to_owned(),
        model: None,
        provider_id: None,
        prompt_tokens: None,
        completion_tokens: None,
        total_tokens: None,
        ttft_ms: None,
        duration_ms: None,
        finish_reason: None,
        request_id: None,
        tool_call_count: 0,
        partial: false,
        created_ms,
    }
}

/// Send the one `Closed`, then tell the sink how the turn ended.
fn close(
    turn: &ServedTurn,
    sink: &dyn TurnSink,
    ending: Ending,
    tally: Tally,
    opened_ms: i64,
    started: Instant,
) -> TurnEnd {
    let (outcome, reason, end) = match ending {
        Ending::Finished(outcome) if outcome.finish_reason == chat::FinishReason::Failed => (
            Some(outcome),
            Some(BROKE_REASON.to_owned()),
            TurnEnd::Failed(BROKE_REASON.to_owned()),
        ),
        Ending::Finished(outcome) => (Some(outcome), None, TurnEnd::Complete),
        Ending::Stopped => (None, Some(STOPPED_REASON.to_owned()), TurnEnd::Stopped),
        Ending::Error(reason) => (None, Some(reason.clone()), TurnEnd::Failed(reason)),
    };
    let tokens = |n: Option<u32>| n.map(i64::from);
    let closed = BotMessageVm {
        model: Some(
            outcome
                .as_ref()
                .and_then(|outcome| outcome.model.clone())
                .unwrap_or_else(|| turn.request.model.clone()),
        ),
        provider_id: Some(turn.provider_id.clone()),
        prompt_tokens: tokens(tally.prompt_tokens),
        completion_tokens: tokens(tally.completion_tokens),
        total_tokens: tokens(tally.total_tokens),
        ttft_ms: tally
            .ttft_ms
            .map(|ms| i64::try_from(ms).unwrap_or(i64::MAX)),
        duration_ms: Some(i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX)),
        finish_reason: Some(match (&outcome, &end) {
            (Some(outcome), _) => finish_word(&outcome.finish_reason),
            (None, TurnEnd::Stopped) => finish_word(&chat::FinishReason::Cancelled),
            (None, _) => finish_word(&chat::FinishReason::Failed),
        }),
        request_id: outcome
            .as_ref()
            .and_then(|outcome| outcome.response_id.clone()),
        tool_call_count: i64::try_from(tally.calls).unwrap_or(i64::MAX),
        partial: reason.is_some(),
        ..message(
            turn,
            &turn.turn_id,
            turn.person.seq + 1,
            "assistant",
            &tally.content,
            opened_ms,
        )
    };
    sink.event(BotStreamEvent::Closed {
        message: Box::new(closed),
        reason,
    });
    sink.ended(end.clone());
    end
}
