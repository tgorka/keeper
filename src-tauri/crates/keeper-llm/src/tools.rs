//! The tool loop: a completion that may call tools, driven to an answer.
//!
//! The loop decides how many rounds a turn may run, how many calls one round
//! may run, how every `tool_call_id` is answered, and when a turn stops. It
//! decides nothing about what a tool *is*: a [`CallHost`] runs each wire call
//! and says, in its own vocabulary, what happened and what the model is told.
//! keeper's drive verbs are one such host (`keeper_core::bots::tools`); an
//! agent's tools are another.

use crate::chat::{
    stream_chat, CancelSignal, ChatEvent, ChatMessage, ChatOptions, ChatOutcome, ChatRequest,
    ContentPart, FinishReason, Role, ToolCall as WireToolCall,
};
use crate::error::BotsError;
use crate::Endpoint;

/// How many completions in one turn may end in tool calls before keeper stops
/// offering tools.
///
/// Eight rounds is a real multi-step task — find the file, read it, read its
/// neighbour, write the answer — and it is also the point past which a model
/// that is looping is looping. Exhaustion is not a hang and not an error: the
/// loop answers every outstanding call with [`ROUNDS_EXHAUSTED`] and runs one
/// final completion **with no tools offered**, so the turn ends in prose and
/// the transcript keeps every `tool_call_id` answered.
pub const MAX_TOOL_ROUNDS: usize = 8;

/// How many tool calls in one round keeper will run.
///
/// Parallel calls in one round are supported and useful — reading four files at
/// once is one round instead of four. Eight is the ceiling; the rest are
/// answered with [`TOO_MANY_CALLS`] so the model can ask again rather than
/// silently losing them.
pub const MAX_CALLS_PER_ROUND: usize = 8;

/// What an outstanding call is told when the round budget is spent.
pub const ROUNDS_EXHAUSTED: &str =
    "keeper stopped running tools for this turn after its round budget. Answer with what you \
     already have, and ask again if you need more.";

/// What a call past [`MAX_CALLS_PER_ROUND`] is told.
pub const TOO_MANY_CALLS: &str =
    "keeper runs a limited number of tool calls per round and this one was past it. Ask for it \
     again in your next turn.";

/// What a call of a round Stop ended before it ran is told: nothing of a
/// stopped turn runs after the Stop.
pub const STOPPED: &str = "keeper stopped this turn before this call ran.";

/// The two bounds on one turn's tool use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolLoopOptions {
    /// How many completions may end in tool calls. See [`MAX_TOOL_ROUNDS`].
    pub max_rounds: usize,
    /// How many calls one round may run. See [`MAX_CALLS_PER_ROUND`].
    pub max_calls_per_round: usize,
}

impl Default for ToolLoopOptions {
    fn default() -> Self {
        Self {
            max_rounds: MAX_TOOL_ROUNDS,
            max_calls_per_round: MAX_CALLS_PER_ROUND,
        }
    }
}

/// What the caller is told while a tool loop runs. `N` is the host's name for
/// a tool it knows ([`CallHost::Name`]).
#[derive(Debug)]
pub enum ToolLoopEvent<N> {
    /// Something the underlying completion said. Forwarded unchanged, so a
    /// surface renders a tool-using turn exactly as it renders a plain one.
    Chat(ChatEvent),
    /// A completion is about to be sent. `round` is 0-based; the last one may
    /// be the toolless round the exhaustion path runs.
    RoundStarted {
        /// Which round.
        round: usize,
        /// Whether tools are on offer in it.
        tools_offered: bool,
    },
    /// A tool is about to run.
    ToolStarted {
        /// The wire call id.
        id: String,
        /// Which tool, or `None` when the model named one the host does not
        /// know by name.
        name: Option<N>,
        /// What it names, for a log line a person reads (FR-388).
        display_path: Option<String>,
    },
    /// A tool finished, and this is what the model was told.
    ToolFinished {
        /// The wire call id.
        id: String,
        /// The rendered result, verbatim — the same bytes the model got.
        result: String,
    },
    /// A grant refused a call. Raised **as well as** being answered to the
    /// model, because a denial is a fact about the user's permissions.
    GrantDenied {
        /// The wire call id.
        id: String,
        /// The grant layer's own sentence.
        reason: String,
    },
    /// The round budget ran out. Every outstanding call was answered and one
    /// final toolless completion follows, so the turn ends in prose.
    RoundsExhausted {
        /// How many rounds ran.
        rounds: usize,
    },
}

/// Where a tool loop sends its events.
pub type ToolLoopSink<'a, N> = &'a mut (dyn FnMut(ToolLoopEvent<N>) + Send);

/// One call the loop ran, for the conversation record and the audit surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallRecord<N> {
    /// The wire call id.
    pub id: String,
    /// What the model asked for, verbatim — the tool name it used, whether or
    /// not it exists.
    pub requested_name: String,
    /// Which tool, when the host knows it by name.
    pub name: Option<N>,
    /// What it named, in the frame a person reads (FR-388: paths, not ids).
    pub display_path: Option<String>,
    /// Whether it was refused, and why.
    pub refusal: Option<String>,
    /// Whether the refusal was the grant's.
    pub grant_denied: bool,
}

/// What a whole turn turned out to be.
#[derive(Debug)]
pub struct ToolLoopOutcome<N> {
    /// The completion that ended the turn.
    pub final_outcome: ChatOutcome,
    /// Every message the loop appended to the conversation, in order: the
    /// assistant turns that called tools and the tool results that answered
    /// them. The caller persists these so a resumed conversation replays with
    /// every `tool_call_id` still answered.
    pub appended: Vec<ChatMessage>,
    /// How many completions were sent.
    pub rounds: usize,
    /// Whether the round budget ran out. An honest terminal state, not a hang:
    /// the final completion was run with no tools on offer.
    pub exhausted: bool,
    /// Every tool call, in order.
    pub calls: Vec<ToolCallRecord<N>>,
    /// The approval a call parked on, and the round's calls after it, not
    /// run (R73). `None` whenever no host call parked.
    pub parked: Option<(ulid::Ulid, Vec<WireToolCall>)>,
}

/// The impure half: what runs a wire call, in the host's own vocabulary.
///
/// The loop hands each call it runs to [`Self::run`] and gets back the record
/// and the host's outcome; [`Self::render`] is the text the model reads for
/// that outcome. A call the loop refuses itself — past the round budget, past
/// the per-round bound, or after a Stop — never reaches [`Self::run`]: the
/// loop asks [`Self::refused`] for the outcome that carries its sentence.
pub trait CallHost: Send + Sync {
    /// The host's name for a tool it knows.
    type Name: Send;
    /// What one call turned out to be.
    type Outcome;

    /// The tool `requested` names, when the host knows it.
    fn name(&self, requested: &str) -> Option<Self::Name>;

    /// Told a round's calls once, before the first of them runs — only
    /// those it will run.
    fn prepare_round(&self, _calls: &[WireToolCall]) {}

    /// Run one call, telling `sink` it started (and anything else it should
    /// hear, such as a grant's denial), and say what happened.
    fn run(
        &self,
        wire: &WireToolCall,
        sink: ToolLoopSink<'_, Self::Name>,
    ) -> (ToolCallRecord<Self::Name>, Self::Outcome);

    /// The outcome of a call the loop refused with `reason` before it ran.
    fn refused(&self, reason: &str) -> Self::Outcome;

    /// The text of the `role: "tool"` message `outcome` produces.
    fn render(&self, outcome: &Self::Outcome) -> String;

    /// The approval record `outcome` waits on, when the call parked: the loop
    /// returns right after it, the round's later calls handed back unrun.
    fn parked(&self, _outcome: &Self::Outcome) -> Option<ulid::Ulid> {
        None
    }
}

/// The things a tool loop needs that do not change between its rounds.
pub struct ToolLoop<'a, H: ?Sized> {
    /// The shared client, built by [`crate::http::client`].
    pub client: &'a reqwest::Client,
    /// Where to send, and with which credential.
    pub endpoint: &'a Endpoint,
    /// The impure half.
    pub host: &'a H,
}

/// Told about each call the moment it has run, with everything a row needs:
/// the record the loop keeps, the wire call the model sent (its `arguments`
/// are what a row shows verbatim) and the host's outcome — or the refusal the
/// loop answered with instead.
pub type ToolCallReporter<'a, N, O> =
    &'a mut (dyn FnMut(&ToolCallRecord<N>, &WireToolCall, &O) + Send);

/// Asked before each round's request leaves, with the 0-based round. An
/// `Err` sends nothing more: the loop returns it, every call already run
/// having been reported.
pub type RoundGate<'a> = &'a mut (dyn FnMut(usize) -> Result<(), BotsError> + Send);

/// Drive a completion that may call tools, to an answer.
///
/// The shape of one round: send the completion; if it produced no tool calls,
/// that is the answer. Otherwise append the assistant turn **with its
/// `tool_calls` replayed verbatim** — dropping them breaks the `tool_call_id`
/// linkage the next request depends on — run each call through the host,
/// append one `role: "tool"` message per call, and re-enter.
///
/// `request.tools` is the caller's. The loop does not decide what is on offer;
/// it decides how many times.
pub async fn run_tool_loop<H: CallHost>(
    context: &ToolLoop<'_, H>,
    request: &ChatRequest,
    options: &ChatOptions,
    loop_options: &ToolLoopOptions,
    cancel: CancelSignal,
    sink: ToolLoopSink<'_, H::Name>,
) -> Result<ToolLoopOutcome<H::Name>, BotsError> {
    let mut unreported = |_: &ToolCallRecord<H::Name>, _: &WireToolCall, _: &H::Outcome| {};
    run_tool_loop_reporting(
        context,
        request,
        options,
        loop_options,
        cancel,
        sink,
        &mut unreported,
    )
    .await
}

/// [`run_tool_loop`], reporting every call **as it completes**.
///
/// Its own entry point rather than a field on [`ToolLoop`] or a variant of
/// [`ToolLoopEvent`], so a caller that wants only the outcome keeps the
/// narrower signature. The reporter is called once per call, after the
/// [`ToolLoopEvent::ToolFinished`] for it, and before the loop moves on — which
/// is what lets a crash mid-loop leave every completed call on record rather
/// than only the ones a finished turn would have returned.
pub async fn run_tool_loop_reporting<H: CallHost>(
    context: &ToolLoop<'_, H>,
    request: &ChatRequest,
    options: &ChatOptions,
    loop_options: &ToolLoopOptions,
    cancel: CancelSignal,
    sink: ToolLoopSink<'_, H::Name>,
    report: ToolCallReporter<'_, H::Name, H::Outcome>,
) -> Result<ToolLoopOutcome<H::Name>, BotsError> {
    let mut open = |_: usize| Ok(());
    run_tool_loop_gated(
        context,
        request,
        options,
        loop_options,
        cancel,
        sink,
        report,
        &mut open,
    )
    .await
}

/// [`run_tool_loop_reporting`], asking `gate` before every request.
///
/// The gate is how a caller stops a conversation from reaching a model it
/// may no longer reach: what a round sends includes every tool result so
/// far, so a result that narrowed what may leave must be able to stop the
/// next request, not only the next turn.
// One parameter per seam the loop has; a struct would only move the same
// names one line down.
#[allow(clippy::too_many_arguments)]
pub async fn run_tool_loop_gated<H: CallHost>(
    context: &ToolLoop<'_, H>,
    request: &ChatRequest,
    options: &ChatOptions,
    loop_options: &ToolLoopOptions,
    cancel: CancelSignal,
    sink: ToolLoopSink<'_, H::Name>,
    report: ToolCallReporter<'_, H::Name, H::Outcome>,
    gate: RoundGate<'_>,
) -> Result<ToolLoopOutcome<H::Name>, BotsError> {
    let ToolLoop {
        client,
        endpoint,
        host,
    } = *context;
    let mut messages = request.messages.clone();
    let mut appended: Vec<ChatMessage> = Vec::new();
    let mut calls: Vec<ToolCallRecord<H::Name>> = Vec::new();
    let mut rounds = 0usize;
    // At least one round, whatever the caller passed: a budget of zero would
    // make the whole turn a no-op with no answer to show.
    let budget = loop_options.max_rounds.max(1);

    loop {
        let tools_offered = rounds < budget;
        // A stopped turn sends no further request (R203).
        if cancel.is_cancelled() {
            return Ok(ToolLoopOutcome {
                final_outcome: ChatOutcome {
                    finish_reason: FinishReason::Cancelled,
                    ..ChatOutcome::default()
                },
                appended,
                rounds,
                exhausted: false,
                calls,
                parked: None,
            });
        }
        gate(rounds)?;
        sink(ToolLoopEvent::RoundStarted {
            round: rounds,
            tools_offered,
        });
        let attempt = ChatRequest {
            messages: messages.clone(),
            tools: if tools_offered {
                request.tools.clone()
            } else {
                Vec::new()
            },
            tool_choice: if tools_offered {
                request.tool_choice.clone()
            } else {
                None
            },
            ..request.clone()
        };

        let outcome = {
            let mut forward = |event: ChatEvent| sink(ToolLoopEvent::Chat(event));
            stream_chat(
                client,
                endpoint,
                &attempt,
                options,
                cancel.clone(),
                &mut forward,
            )
            .await?
        };
        rounds += 1;

        // A stream Stop cut runs none of its calls: what arrived is kept.
        if !tools_offered
            || outcome.tool_calls.is_empty()
            || outcome.finish_reason == FinishReason::Cancelled
        {
            return Ok(ToolLoopOutcome {
                final_outcome: outcome,
                appended,
                rounds,
                exhausted: !tools_offered,
                calls,
                parked: None,
            });
        }

        // The assistant turn, replayed verbatim so the linkage survives.
        let assistant = ChatMessage {
            role: Role::Assistant,
            content: if outcome.content.is_empty() {
                Vec::new()
            } else {
                vec![ContentPart::Text(outcome.content.clone())]
            },
            tool_call_id: None,
            tool_calls: outcome.tool_calls.clone(),
        };
        messages.push(assistant.clone());
        appended.push(assistant);

        let exhausted = rounds >= budget;
        if exhausted {
            sink(ToolLoopEvent::RoundsExhausted { rounds });
        }

        if !exhausted {
            let runs = outcome
                .tool_calls
                .len()
                .min(loop_options.max_calls_per_round);
            host.prepare_round(&outcome.tool_calls[..runs]);
        }
        let mut parked = None;
        for (index, wire) in outcome.tool_calls.iter().enumerate() {
            let refusal = if exhausted {
                // The budget is spent, but the call still has to be ANSWERED:
                // an unanswered `tool_call_id` is a transcript no provider will
                // accept on the next turn, and a turn that simply stopped here
                // would look to the user like a hang.
                Some(ROUNDS_EXHAUSTED)
            } else if cancel.is_cancelled() {
                Some(STOPPED)
            } else if index >= loop_options.max_calls_per_round {
                Some(TOO_MANY_CALLS)
            } else {
                None
            };
            let (record, ran) = match refusal {
                Some(reason) => (
                    ToolCallRecord {
                        id: wire.id.clone(),
                        requested_name: wire.name.clone(),
                        name: host.name(&wire.name),
                        display_path: None,
                        refusal: Some(reason.to_owned()),
                        grant_denied: false,
                    },
                    host.refused(reason),
                ),
                None => host.run(wire, sink),
            };

            let result = host.render(&ran);
            sink(ToolLoopEvent::ToolFinished {
                id: wire.id.clone(),
                result: result.clone(),
            });
            report(&record, wire, &ran);
            calls.push(record);
            if let Some(approval) = host.parked(&ran) {
                parked = Some((approval, index));
                break;
            }
            let message = ChatMessage {
                role: Role::Tool,
                content: vec![ContentPart::Text(result)],
                tool_call_id: Some(wire.id.clone()),
                tool_calls: Vec::new(),
            };
            messages.push(message.clone());
            appended.push(message);
        }
        if let Some((approval, index)) = parked {
            // The rest of the round runs only once a person decided.
            let rest = outcome.tool_calls[index + 1..].to_vec();
            return Ok(ToolLoopOutcome {
                final_outcome: outcome,
                appended,
                rounds,
                exhausted: false,
                calls,
                parked: Some((approval, rest)),
            });
        }
    }
}

/// Whether a completion ended by asking for tools — the caller's own check,
/// where it wants one without re-deriving the rule.
pub fn wants_tools(outcome: &ChatOutcome) -> bool {
    !outcome.tool_calls.is_empty() || outcome.finish_reason == FinishReason::ToolCalls
}
