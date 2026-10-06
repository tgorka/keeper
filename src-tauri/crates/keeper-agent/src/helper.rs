//! `helper` (story 94.4, AD-399): one context-free, read-only model call
//! inside an agent's turn — BMAD's subagent, and how a review layer runs.
//!
//! A helper is told the session's frame without soul or core memory, its
//! lens, the brief and its inputs — nothing of the conversation — and is
//! offered only the reads its session is offered ([`helper::TOOLS`]). Each
//! call it makes goes through the session's own host: the session's grants,
//! tiers and audit rows, the session's label; anything but a read is
//! answered with [`helper::REFUSAL`]. Its model, the lens's bot or the
//! agent's, is checked as a sink of the session's label before every round
//! (S-04), and the turn's token budget — and a delegated session's or a
//! workflow's run's own — before its launch and its every round (R111). It
//! owns nothing and is never addressed again: what it did
//! is buffered as [`Step`]s, written under its `tool_call` by the turn's
//! reporter and kept out of every replay, and its answer re-enters the turn
//! as data.
//!
//! The helpers of one round run side by side, but none past a call that
//! has not run yet: the agent's host is told the round from
//! `ToolHost::prepare_round` and, at a round's helper, launches it with
//! the helpers right after it, answering each call from what its helper
//! came to (R110). A call that parks therefore leaves every later helper
//! unlaunched, a call of the rest of its round (R203).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use keeper_core::agents::delegation::BoundReached;
use keeper_core::agents::helper::{self, HelperCall, Lens};
use keeper_core::agents::label::{check_sink, Label, Sink, SinkVerdict};
use keeper_core::agents::log::{AssistantBody, ToolCallBody, ToolResultBody, Usage};
use keeper_core::bots::chat::{
    self, CancelSignal, ChatEvent, ChatMessage, ChatOptions, ChatRequest, FinishReason, Role,
    ToolSpec,
};
use keeper_core::bots::error::BotsError;
use keeper_core::bots::grant::Effect;
use keeper_core::bots::http;
use keeper_core::bots::store::ProviderRow;
use keeper_core::bots::tools::{
    self, ToolCall, ToolCallRecord, ToolHost, ToolLoop, ToolLoopEvent, ToolLoopOptions, ToolName,
    ToolOutcome,
};
use keeper_sync::SyncProfile;
use serde_json::Value;

use crate::agent::{result_word, AgentDeps, ROUND_FINISH};
use crate::drive::finish_word;
use crate::host::UNATTENDED_REFUSAL;
use crate::turn::{endpoint_of, read_timeout_of};

/// The session a helper works for, as its calls reach it.
pub(crate) trait Parent: Sync {
    /// The session's own host: every call a helper makes runs through it.
    fn host(&self) -> &dyn ToolHost;
    /// The tier call `id` was classified at, for its line.
    fn tier_of(&self, id: &str) -> u8;
    /// What the call `record` just read, each with its label and path.
    fn reads(&self, record: &ToolCallRecord, outcome: &ToolOutcome) -> Vec<(Label, String)>;
    /// Step `id` of the helper call `helper`, `tool` at `at` (a drive and
    /// path, for a drive verb) with `args`, refused as no read: its one
    /// audit row, classified where `tool` has a row of the tier table.
    fn refused(
        &self,
        helper: &str,
        id: &str,
        tool: &str,
        at: Option<(&str, &str)>,
        args: &Value,
        reason: &str,
    );
}

/// One thing a helper did, as its line under the helper's `tool_call`.
pub(crate) enum Step {
    /// A round of its model.
    Round(AssistantBody),
    /// A call it made, and what answered it.
    Call(ToolCallBody, ToolResultBody),
}

/// What one `helper` call came to.
pub(crate) struct HelperRun {
    pub outcome: ToolOutcome,
    pub steps: Vec<Step>,
    /// What it read — the lens's customization included — each with its
    /// label and path: all of it joins the session's label.
    pub reads: Vec<(Label, String)>,
}

impl HelperRun {
    pub(crate) fn refused(reason: impl Into<String>, reads: Vec<(Label, String)>) -> HelperRun {
        HelperRun {
            outcome: ToolOutcome::Refused {
                reason: reason.into(),
            },
            steps: Vec::new(),
            reads,
        }
    }
}

/// A helper call ready to launch: everything checked that can be before
/// its model is reached.
pub(crate) struct Launch {
    /// The `helper` call's wire id.
    pub id: String,
    pub call: HelperCall,
    pub lens: Option<Lens>,
    /// The provider its model is reached through, and the model.
    pub row: ProviderRow,
    pub target: String,
    /// The session's label, joined with what finding the lens read.
    pub label: Label,
    /// The turn's spend when its helpers launched: what every helper's
    /// first request is checked against (R111).
    pub spend: u64,
    /// A delegated session's or a workflow's run's own budget when its
    /// helpers launched: what it had spent, and its limit (Q12, R214).
    pub session: Option<(u64, u64)>,
    /// What finding the lens read.
    pub reads: Vec<(Label, String)>,
}

/// One turn's helpers.
pub(crate) struct Helpers<'t> {
    pub deps: &'t AgentDeps,
    /// The mounted drives, where a read's label is read.
    pub profiles: Vec<SyncProfile>,
    /// The profile a path names when its call names none.
    pub default_profile_id: String,
    /// The session frame as a helper is told it: no soul, no memory.
    pub frame: String,
    /// The reads a helper is offered, of the turn's offer.
    pub offer: Vec<ToolSpec>,
    pub stop: CancelSignal,
    /// While a round's helpers run, a call that needs a person is refused,
    /// never parked: a helper holds no turn to wait in.
    pub running: AtomicBool,
    /// What each `helper` call of the turn came to, by call id, until its
    /// `tool_call` line takes its steps.
    runs: Mutex<HashMap<String, HelperRun>>,
    /// The round under way, as `prepare_round` was told it.
    round: Mutex<Vec<chat::ToolCall>>,
    /// The tokens the helpers launched together have spent so far, not yet
    /// in the log: every round of each, counted once, is checked before
    /// any later request of any of them (R203).
    pub spent: AtomicU64,
}

impl<'t> Helpers<'t> {
    pub(crate) fn new(
        deps: &'t AgentDeps,
        profiles: Vec<SyncProfile>,
        default_profile_id: String,
        frame: String,
        offer: Vec<ToolSpec>,
        stop: CancelSignal,
    ) -> Helpers<'t> {
        Helpers {
            deps,
            profiles,
            default_profile_id,
            frame,
            offer,
            stop,
            running: AtomicBool::new(false),
            runs: Mutex::new(HashMap::new()),
            round: Mutex::new(Vec::new()),
            spent: AtomicU64::new(0),
        }
    }

    fn runs(&self) -> std::sync::MutexGuard<'_, HashMap<String, HelperRun>> {
        self.runs.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Whether call `id` has run.
    pub(crate) fn ran(&self, id: &str) -> bool {
        self.runs().contains_key(id)
    }

    pub(crate) fn keep(&self, id: String, run: HelperRun) {
        self.runs().insert(id, run);
    }

    /// What call `id` answers.
    pub(crate) fn outcome(&self, id: &str) -> Option<ToolOutcome> {
        self.runs().get(id).map(|run| run.outcome.clone())
    }

    /// Call `id`'s steps and reads, for its `tool_call` line.
    pub(crate) fn take(&self, id: &str) -> Option<HelperRun> {
        self.runs().remove(id)
    }

    /// The round about to run.
    pub(crate) fn prepare(&self, calls: &[chat::ToolCall]) {
        *self.round.lock().unwrap_or_else(|p| p.into_inner()) = calls.to_vec();
    }

    /// The helpers to launch at call `id`: it and the helpers of its round
    /// right after it, up to the round's next other call — which may park
    /// — or its end. Just `id` when no round holds it (a parked round's
    /// later call, run on resume).
    pub(crate) fn batch(&self, wire: &chat::ToolCall) -> Vec<chat::ToolCall> {
        let round = self.round.lock().unwrap_or_else(|p| p.into_inner());
        let Some(at) = round.iter().position(|call| call.id == wire.id) else {
            return vec![wire.clone()];
        };
        round[at..]
            .iter()
            .take_while(|call| call.name == helper::HELPER)
            .filter(|call| !self.ran(&call.id))
            .cloned()
            .collect()
    }

    /// The model `launch` reaches, as the provider row's kind says.
    pub(crate) fn is_local(row: &ProviderRow) -> bool {
        keeper_core::agents::home::serves_local_models(row.provider.kind)
    }
}

/// The session's host as a helper may use it: its reads and `skill_view`,
/// nothing else.
struct Reads<'a> {
    parent: &'a dyn Parent,
    /// The `helper` call it reads for.
    helper: &'a str,
}

fn refused(reason: &str) -> ToolOutcome {
    ToolOutcome::Refused {
        reason: reason.to_owned(),
    }
}

/// A call that would wait for a person is refused: a helper never parks.
fn unparked(outcome: ToolOutcome) -> ToolOutcome {
    match outcome {
        ToolOutcome::Parked { .. } => refused(UNATTENDED_REFUSAL),
        outcome => outcome,
    }
}

impl ToolHost for Reads<'_> {
    fn run(&self, call: &ToolCall) -> Result<ToolOutcome, BotsError> {
        if call.name.effect() == Effect::Write {
            self.parent.refused(
                self.helper,
                &call.id,
                call.name.as_wire(),
                Some((&call.target.profile_id, &call.target.subpath)),
                &Value::Null,
                helper::REFUSAL,
            );
            return Ok(refused(helper::REFUSAL));
        }
        self.parent.host().run(call).map(unparked)
    }

    fn run_named(&self, wire: &chat::ToolCall) -> Option<ToolOutcome> {
        match ToolName::from_wire(&wire.name) {
            Some(_) => None,
            None if wire.name == keeper_core::agents::workflow::SKILL_VIEW => {
                self.parent.host().run_named(wire).map(unparked)
            }
            None => {
                self.parent.refused(
                    self.helper,
                    &wire.id,
                    &wire.name,
                    None,
                    wire.arguments.as_ref().unwrap_or(&Value::Null),
                    helper::REFUSAL,
                );
                Some(refused(helper::REFUSAL))
            }
        }
    }
}

/// A round of the helper's model, as its line.
fn round(text: String, finish: String, usage: Usage, model: &str) -> Step {
    Step::Round(AssistantBody {
        text,
        model: model.to_owned(),
        finish,
        usage,
        ttft_ms: None,
        duration_ms: 0,
        anchor_event: None,
    })
}

fn tokens(usage: Usage) -> u64 {
    u64::from(usage.prompt.unwrap_or(0)) + u64::from(usage.completion.unwrap_or(0))
}

/// What a running helper keeps between its loop's callbacks.
struct State {
    label: Label,
    text: String,
    usage: Usage,
    /// Whether the round under way has its line.
    logged: bool,
    steps: Vec<Step>,
    reads: Vec<(Label, String)>,
    /// Why its gate stopped it.
    stopped: Option<String>,
}

/// Run `launch` to its answer: a nested tool loop over the session's reads,
/// on the lens's bot or the agent's.
pub(crate) async fn run(helpers: &Helpers<'_>, parent: &dyn Parent, launch: Launch) -> HelperRun {
    let Launch {
        id,
        call,
        lens,
        row,
        target,
        label,
        spend,
        session,
        reads,
    } = launch;
    // A Stop while the credential resolves ends the helper there.
    let mut stop = helpers.stop.clone();
    let endpoint = tokio::select! {
        biased;
        () = stop.cancelled() => return HelperRun::refused(helper::STOPPED, reads),
        endpoint = endpoint_of(&helpers.deps.env, &row, Some(&target)) => endpoint,
    };
    let endpoint = match endpoint {
        Ok(endpoint) => endpoint,
        Err(error) => return HelperRun::refused(error.to_string(), reads),
    };
    let read_timeout = read_timeout_of(&row);
    let client = match http::client(read_timeout) {
        Ok(client) => client,
        Err(error) => return HelperRun::refused(error.to_string(), reads),
    };
    let request = ChatRequest {
        model: target.clone(),
        messages: vec![
            ChatMessage::text(
                Role::System,
                helper::system_message(&helpers.frame, lens.as_ref()),
            ),
            ChatMessage::text(Role::User, helper::brief_message(&call)),
        ],
        tools: helpers.offer.clone(),
        ..ChatRequest::default()
    };
    let local = Helpers::is_local(&row);
    let budget = helpers.deps.home.config.limits.tokens_per_turn;
    let host = Reads {
        parent,
        helper: &id,
    };
    let tool_loop = ToolLoop {
        client: &client,
        endpoint: &endpoint,
        host: &host,
        default_profile_id: &helpers.default_profile_id,
    };
    let state = Mutex::new(State {
        label,
        text: String::new(),
        usage: Usage::default(),
        logged: false,
        steps: Vec::new(),
        reads,
        stopped: None,
    });
    // Each round of each helper counted once, as it ends.
    let count = |usage: Usage| helpers.spent.fetch_add(tokens(usage), Ordering::SeqCst);
    let lock = || state.lock().unwrap_or_else(|p| p.into_inner());
    let mut events = |event: ToolLoopEvent| match event {
        ToolLoopEvent::RoundStarted { .. } => {
            let mut state = lock();
            state.text.clear();
            state.usage = Usage::default();
            state.logged = false;
        }
        ToolLoopEvent::Chat(ChatEvent::Usage(usage)) => {
            lock().usage = Usage {
                prompt: usage.prompt_tokens,
                completion: usage.completion_tokens,
            };
        }
        ToolLoopEvent::Chat(ChatEvent::ContentDelta(text)) => lock().text.push_str(&text),
        _ => {}
    };
    let mut report = |record: &ToolCallRecord, wire: &chat::ToolCall, outcome: &ToolOutcome| {
        let read = parent.reads(record, outcome);
        let mut state = lock();
        if !state.logged {
            let (text, usage) = (state.text.clone(), state.usage);
            count(usage);
            state
                .steps
                .push(round(text, ROUND_FINISH.to_owned(), usage, &target));
            state.logged = true;
        }
        for (read_label, _) in &read {
            state.label = state.label.join(read_label);
        }
        let result_label = read
            .iter()
            .map(|(label, _)| label.clone())
            .reduce(|joined, label| joined.join(&label))
            .unwrap_or_else(|| state.label.clone());
        let (outcome_word, truncated) = result_word(outcome);
        state.steps.push(Step::Call(
            ToolCallBody {
                call_id: wire.id.clone(),
                tool: wire.name.clone(),
                args: wire.arguments_raw.clone(),
                tier: parent.tier_of(&wire.id),
                grant_id: None,
            },
            ToolResultBody {
                call_id: wire.id.clone(),
                outcome: outcome_word,
                content: tools::render_result(outcome),
                truncated,
                label: result_label,
            },
        ));
        state.reads.extend(read);
    };
    // Before every round: its model is a sink of what it has read. Its
    // first request is checked against the spend the helpers launched at
    // (R111: launched together, none waits on another's tokens); every
    // later one against that spend and every round any of them has ended
    // since (R203). A delegated session's or a workflow's run's own budget
    // is counted the same way, and first, as the run's round gate checks
    // it first: a helper never spends past the bound its run's next round
    // would stop at, and says the bound that round would (R214, R215).
    let mut gate = |round: usize| {
        let mut state = lock();
        let since = if round == 0 {
            0
        } else {
            helpers.spent.load(Ordering::SeqCst)
        };
        if let Some((spent, limit)) = session.filter(|(spent, limit)| spent + since >= *limit) {
            let spent = spent + since;
            state.stopped = Some(BoundReached::Tokens { spent, limit }.sentence());
        } else if helper::spent(budget, spend + since) {
            state.stopped = Some(helper::TURN_SPENT.to_owned());
        } else if let SinkVerdict::Block { .. } = check_sink(&state.label, &Sink::Model { local }) {
            state.stopped = Some(keeper_core::agents::label::LOCAL_ONLY_SINK.to_owned());
        }
        match &state.stopped {
            Some(reason) => Err(BotsError::Tool {
                detail: reason.clone(),
            }),
            None => Ok(()),
        }
    };
    let ran = tools::run_tool_loop_gated(
        &tool_loop,
        &request,
        &ChatOptions {
            read_timeout,
            ..ChatOptions::default()
        },
        &ToolLoopOptions {
            max_rounds: usize::try_from(helpers.deps.home.config.limits.rounds_per_turn)
                .unwrap_or(tools::MAX_TOOL_ROUNDS)
                .max(1),
            ..ToolLoopOptions::default()
        },
        helpers.stop.clone(),
        &mut events,
        &mut report,
        &mut gate,
    )
    .await;
    let mut state = state.into_inner().unwrap_or_else(|p| p.into_inner());
    let outcome = match ran {
        Ok(done) => {
            // The round the loop ended on — answered, failed, or stopped
            // once any of it arrived — has its line and its tokens.
            let final_outcome = done.final_outcome;
            let usage = final_outcome.usage.as_ref();
            let usage = Usage {
                prompt: usage.and_then(|u| u.prompt_tokens),
                completion: usage.and_then(|u| u.completion_tokens),
            };
            let arrived = final_outcome.finish_reason != FinishReason::Cancelled
                || !final_outcome.content.is_empty()
                || usage != Usage::default();
            if arrived {
                count(usage);
                state.steps.push(round(
                    final_outcome.content.clone(),
                    finish_word(&final_outcome.finish_reason),
                    usage,
                    &target,
                ));
            }
            match final_outcome.finish_reason {
                FinishReason::Failed => refused("The helper's model failed before it answered."),
                FinishReason::Cancelled => refused(helper::STOPPED),
                _ => ToolOutcome::Answered {
                    text: helper::result_text(
                        lens.as_ref().map(|lens| lens.id.as_str()),
                        &final_outcome.content,
                    ),
                },
            }
        }
        Err(error) => match state.stopped {
            Some(reason) => refused(&reason),
            None => ToolOutcome::Refused {
                reason: error.to_string(),
            },
        },
    };
    HelperRun {
        outcome,
        steps: state.steps,
        reads: state.reads,
    }
}
