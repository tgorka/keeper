//! Building a chat request, and driving its stream to a reduced answer.
//!
//! # The shape of this file
//!
//! Three things, in order: the request model and how it becomes JSON
//! ([`build_body`]), the reassembly state machine that turns fragmented deltas
//! back into one message ([`Reassembler`]), and the driver that runs a real
//! socket into a sink of [`ChatEvent`]s ([`stream_chat`]).
//!
//! There is deliberately **no** `if kind == Ollama` anywhere below. Every
//! per-provider divergence is read once from [`crate::bots::quirks`], which is
//! the decision the epic made before the first story started.
//!
//! # Where the failures go, and why the split is where it is
//!
//! [`stream_chat`] returns `Err` only for a failure that happened **before any
//! stream byte existed** — a refused connection, a 401, a 404, a 500. Once the
//! endpoint has streamed anything, a later failure returns `Ok(outcome)` whose
//! `finish_reason` is [`FinishReason::Failed`] or [`FinishReason::Cancelled`]
//! and whose partial `content` is intact, and the typed cause is handed to the
//! sink as [`ChatEvent::Failed`].
//!
//! That is not a convenience. A partial reply is a real record — the epic says
//! cancellation "leaves a partial assistant row persisted and marked partial,
//! never a silently discarded reply" — and a `Result` that carries only the
//! error would force the caller to throw the reply away to learn why it
//! stopped. The same line is also the retry boundary (see [`stream_chat`]), so
//! the two rules are one rule.

use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};

use crate::bots::error::BotsError;
use crate::bots::http::authorize;
use crate::bots::quirks::{quirks, ImagePartShape, Support};
use crate::bots::sse::{SseFrame, SseFramer};
use crate::bots::{Endpoint, ProviderKind};

/// The path every chat completion goes to, on both kinds.
pub const CHAT_PATH: &str = "/v1/chat/completions";

/// How this endpoint is named in an error. Method plus path, never a URL:
/// the base URL is user-typed and may not appear in a log line.
const CHAT_ENDPOINT: &str = "POST /v1/chat/completions";

/// The largest body keeper will read while looking for an error message.
///
/// A failing response is read in bounded chunks and stopped here rather than
/// with `Response::text`, which would read whatever the peer chose to send.
const MAX_ERROR_BODY_BYTES: usize = 8 * 1024;

// ---------------------------------------------------------------------------
// Request model
// ---------------------------------------------------------------------------

/// Who is speaking in a message.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Role {
    /// Instructions. Chat Completions accepts only text parts on this role.
    System,
    /// The person. The default: a message with no stated role is the user's.
    #[default]
    User,
    /// The model's own previous turn, replayed verbatim so `tool_call_id`
    /// linkage survives a multi-turn tool loop (research §2.3).
    Assistant,
    /// The result of one tool call, answering one `tool_call_id`.
    Tool,
}

impl Role {
    /// The wire spelling.
    pub fn as_wire(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
        }
    }
}

/// One piece of a message's content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentPart {
    /// Text.
    Text(String),
    /// An image, as an `http(s)` URL or a `data:` URI. Which of the two a
    /// provider accepts is a quirk row, not a decision made here.
    Image {
        /// The `http(s)` URL or `data:` URI.
        url: String,
        /// OpenAI's `detail` hint, where the caller set one.
        detail: Option<String>,
    },
}

/// One message in the conversation sent to the model.
#[derive(Debug, Clone, Default)]
pub struct ChatMessage {
    /// Who is speaking.
    pub role: Role,
    /// The content parts, in order.
    pub content: Vec<ContentPart>,
    /// For [`Role::Tool`], the call this message answers.
    pub tool_call_id: Option<String>,
    /// For [`Role::Assistant`], the tool calls that turn made — replayed
    /// verbatim, because dropping them breaks the `tool_call_id` linkage the
    /// next request depends on (research §2.3).
    pub tool_calls: Vec<ToolCall>,
}

impl ChatMessage {
    /// A plain-text message.
    pub fn text(role: Role, body: impl Into<String>) -> Self {
        Self {
            role,
            content: vec![ContentPart::Text(body.into())],
            tool_call_id: None,
            tool_calls: Vec::new(),
        }
    }
}

/// A tool keeper offers the model.
#[derive(Debug, Clone)]
pub struct ToolSpec {
    /// The function name the model will call.
    pub name: String,
    /// What it does, in the model's own context window.
    pub description: String,
    /// A JSON Schema object for the arguments.
    pub parameters: Value,
}

/// How hard the model is pushed toward calling a tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolChoice {
    /// The model decides. The wire default when tools are present.
    Auto,
    /// The model must not call a tool.
    None,
    /// The model must call at least one tool.
    Required,
    /// The model must call this one.
    Function(String),
}

/// Everything a chat completion needs, before the wire shape is decided.
#[derive(Debug, Clone, Default)]
pub struct ChatRequest {
    /// The model or alias to ask for.
    pub model: String,
    /// The conversation.
    pub messages: Vec<ChatMessage>,
    /// Tools on offer, if any.
    pub tools: Vec<ToolSpec>,
    /// Tool pressure, if the caller set any.
    pub tool_choice: Option<ToolChoice>,
    /// Sampling temperature, if the caller set one.
    pub temperature: Option<f64>,
    /// Completion-length ceiling, if the caller set one.
    pub max_tokens: Option<u32>,
}

/// Turn a [`ChatRequest`] into the JSON body for one provider kind.
///
/// This is where the quirk table is spent. Two rows change the body and one
/// refuses it outright:
///
/// * `tool_choice` is dropped for a kind that ignores it, rather than sent and
///   silently discarded — the difference matters because keeper can then say
///   so in the UI instead of the user wondering why `required` did nothing;
/// * an image part takes the shape that kind documents;
/// * a remote `http(s)` image URL sent to a kind that will not fetch one is a
///   named refusal ([`BotsError::Unsupported`]) before a byte goes out, not a
///   silently dropped image the model is then asked about.
pub fn build_body(kind: ProviderKind, request: &ChatRequest) -> Result<Value, BotsError> {
    let quirks = quirks(kind);

    let mut messages = Vec::with_capacity(request.messages.len());
    for message in &request.messages {
        messages.push(build_message(kind, message)?);
    }

    let mut body = Map::new();
    body.insert("model".to_owned(), json!(request.model));
    body.insert("messages".to_owned(), Value::Array(messages));
    body.insert("stream".to_owned(), json!(true));
    // Asked for on both kinds. Ollama documents it as supported; Hermes does
    // not document it either way, and the rule for an unread capability is to
    // ask and report the answer rather than to assume a `false`.
    body.insert(
        "stream_options".to_owned(),
        json!({ "include_usage": true }),
    );

    if !request.tools.is_empty() {
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters,
                    }
                })
            })
            .collect();
        body.insert("tools".to_owned(), Value::Array(tools));

        if let Some(choice) = &request.tool_choice {
            if quirks.tool_choice.permitted() {
                body.insert("tool_choice".to_owned(), tool_choice_value(choice));
            }
        }
    }

    if let Some(temperature) = request.temperature {
        if let Some(number) = serde_json::Number::from_f64(temperature) {
            body.insert("temperature".to_owned(), Value::Number(number));
        }
    }
    if let Some(max_tokens) = request.max_tokens {
        body.insert("max_tokens".to_owned(), json!(max_tokens));
    }

    Ok(Value::Object(body))
}

/// Whether a `tool_choice` set on this request will actually reach the model.
///
/// The surface needs this to avoid an affordance that lies (AD-27): a
/// `required` toggle that a provider silently ignores is worse than no toggle.
pub fn honours_tool_choice(kind: ProviderKind) -> bool {
    quirks(kind).tool_choice.permitted()
}

fn tool_choice_value(choice: &ToolChoice) -> Value {
    match choice {
        ToolChoice::Auto => json!("auto"),
        ToolChoice::None => json!("none"),
        ToolChoice::Required => json!("required"),
        ToolChoice::Function(name) => json!({
            "type": "function",
            "function": { "name": name }
        }),
    }
}

fn build_message(kind: ProviderKind, message: &ChatMessage) -> Result<Value, BotsError> {
    let quirks = quirks(kind);
    let mut object = Map::new();
    object.insert("role".to_owned(), json!(message.role.as_wire()));

    let all_text = message
        .content
        .iter()
        .all(|part| matches!(part, ContentPart::Text(_)));

    // Chat Completions accepts only `text` parts on the system, developer and
    // tool roles (research §3.1, §4.1), and a plain string is the shape every
    // server has always accepted, so a text-only message is sent as a string.
    if all_text {
        let text = message
            .content
            .iter()
            .filter_map(|part| match part {
                ContentPart::Text(text) => Some(text.as_str()),
                ContentPart::Image { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        object.insert("content".to_owned(), json!(text));
    } else {
        if !matches!(message.role, Role::User) {
            return Err(BotsError::Unsupported {
                detail: format!(
                    "an image can only be attached to a user message, not to a {} message",
                    message.role.as_wire()
                ),
            });
        }
        let mut parts = Vec::with_capacity(message.content.len());
        for part in &message.content {
            parts.push(build_content_part(
                kind,
                quirks.remote_image_url,
                quirks.image_part,
                part,
            )?);
        }
        object.insert("content".to_owned(), Value::Array(parts));
    }

    if let Some(tool_call_id) = &message.tool_call_id {
        object.insert("tool_call_id".to_owned(), json!(tool_call_id));
    }
    if !message.tool_calls.is_empty() {
        let calls: Vec<Value> = message
            .tool_calls
            .iter()
            .map(|call| {
                json!({
                    "id": call.id,
                    "type": "function",
                    "function": { "name": call.name, "arguments": call.arguments_raw }
                })
            })
            .collect();
        object.insert("tool_calls".to_owned(), Value::Array(calls));
    }

    Ok(Value::Object(object))
}

fn build_content_part(
    kind: ProviderKind,
    remote_image_url: Support,
    image_part: ImagePartShape,
    part: &ContentPart,
) -> Result<Value, BotsError> {
    match part {
        ContentPart::Text(text) => Ok(json!({ "type": "text", "text": text })),
        ContentPart::Image { url, detail } => {
            let is_remote = url.starts_with("http://") || url.starts_with("https://");
            if is_remote && !remote_image_url.permitted() {
                return Err(BotsError::Unsupported {
                    detail: format!(
                        "{kind:?} does not fetch image URLs; attach the image itself instead"
                    ),
                });
            }
            match image_part {
                ImagePartShape::BareString => Ok(json!({ "type": "image_url", "image_url": url })),
                ImagePartShape::Object => {
                    let mut image_url = Map::new();
                    image_url.insert("url".to_owned(), json!(url));
                    if let Some(detail) = detail {
                        image_url.insert("detail".to_owned(), json!(detail));
                    }
                    Ok(json!({ "type": "image_url", "image_url": Value::Object(image_url) }))
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The reduced answer
// ---------------------------------------------------------------------------

/// Why the model stopped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum FinishReason {
    /// A natural stop or a stop sequence.
    Stop,
    /// The completion-length ceiling was reached.
    Length,
    /// The provider's content filter cut it.
    ContentFilter,
    /// The model called tools.
    ToolCalls,
    /// The user stopped it. Whatever had arrived is kept.
    Cancelled,
    /// The stream broke. `error` on the sink's [`ChatEvent::Failed`] says how.
    ///
    /// The default, deliberately: a [`ChatOutcome`] that nobody finished is a
    /// broken one, and defaulting to `Stop` would let a bug present a
    /// half-built record as a completed answer.
    #[default]
    Failed,
    /// A reason the wire invented. Kept verbatim rather than folded into
    /// `Stop`, because a metadata caption that prints the provider's own word
    /// is more use than one that prints keeper's guess.
    Other(String),
}

impl FinishReason {
    fn from_wire(value: &str) -> Self {
        match value {
            "stop" => Self::Stop,
            "length" => Self::Length,
            "content_filter" => Self::ContentFilter,
            "tool_calls" | "function_call" => Self::ToolCalls,
            other => Self::Other(other.to_owned()),
        }
    }
}

/// Token counts, where the endpoint reported them.
///
/// Every field is optional because an endpoint that omits `usage` is a fact
/// about the endpoint, and Story 61.8's rule is that an absent number renders
/// as absent and never as zero.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenUsage {
    /// Prompt tokens.
    pub prompt_tokens: Option<u32>,
    /// Completion tokens.
    pub completion_tokens: Option<u32>,
    /// Total tokens.
    pub total_tokens: Option<u32>,
}

/// One reassembled tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    /// The provider's call id, or a synthesised `call_{index}` when the wire
    /// omitted one (research §1.3l).
    pub id: String,
    /// The function name.
    pub name: String,
    /// The arguments exactly as they arrived, concatenated across fragments.
    ///
    /// Kept even when `arguments` parsed, because it is the only evidence when
    /// a parse fails and the model is the thing that got it wrong.
    pub arguments_raw: String,
    /// The parsed arguments, or `None` when the model did not produce valid
    /// JSON — which OpenAI's own schema warns is possible (research §1.2).
    pub arguments: Option<Value>,
}

/// What one completion turned out to be.
#[derive(Debug, Clone, Default)]
pub struct ChatOutcome {
    /// The assistant's text.
    pub content: String,
    /// The assistant's reasoning tokens, kept separate from `content` — a
    /// provider extension carried through as its own thing rather than mixed
    /// into the answer.
    pub reasoning: String,
    /// A refusal, where the model produced one.
    pub refusal: Option<String>,
    /// The reassembled tool calls, in the order their indices first appeared.
    pub tool_calls: Vec<ToolCall>,
    /// Why it stopped.
    pub finish_reason: FinishReason,
    /// Token counts, where reported.
    pub usage: Option<TokenUsage>,
    /// The model the server says answered — which may differ from the model
    /// requested (research §8.3).
    pub model: Option<String>,
    /// The provider's completion id.
    pub response_id: Option<String>,
    /// Milliseconds from request start to the first delta of any kind.
    pub first_token_ms: Option<u64>,
    /// Milliseconds from request start to the end of the stream.
    pub total_ms: u64,
}

/// What the caller is told as it happens.
#[derive(Debug)]
pub enum ChatEvent {
    /// The first delta of any kind arrived, this many ms after the request
    /// started. Emitted exactly once per completion.
    FirstToken {
        /// Milliseconds from request start.
        after_ms: u64,
    },
    /// A slice of the answer.
    ContentDelta(String),
    /// A slice of the model's reasoning, kept as its own kind so a surface can
    /// show or hide it without parsing the answer.
    ReasoningDelta(String),
    /// A fragment of one tool call, keyed by its wire index.
    ToolCallDelta {
        /// `delta.tool_calls[].index` — the identity of the call, not its
        /// position in this chunk's array.
        index: u32,
        /// The call id, on the fragment that carries it.
        id: Option<String>,
        /// The function name, on the fragment that carries it.
        name: Option<String>,
        /// This fragment's slice of the arguments string.
        arguments: String,
    },
    /// The usage chunk, when one arrived.
    Usage(TokenUsage),
    /// The stream ended.
    Finished {
        /// Why.
        reason: FinishReason,
    },
    /// The stream ended badly, after it had already produced bytes. The
    /// completion so far is still returned to the caller.
    Failed {
        /// The typed cause.
        error: BotsError,
    },
}

/// Where [`stream_chat`] sends events as they happen.
pub type ChatSink<'a> = &'a mut (dyn FnMut(ChatEvent) + Send);

// ---------------------------------------------------------------------------
// Reassembly
// ---------------------------------------------------------------------------

/// One tool call being built out of fragments.
#[derive(Debug, Default)]
struct PartialCall {
    id: String,
    name: String,
    arguments: String,
}

/// The delta state machine (research §1.3).
///
/// Kept separate from the driver so it can be exercised against a transcript
/// with no socket in sight, and so the driver has nothing in it but I/O.
#[derive(Debug)]
pub struct Reassembler {
    reasoning_fields: &'static [&'static str],
    content: String,
    reasoning: String,
    refusal: Option<String>,
    /// Keyed by `delta.tool_calls[].index`, in first-seen order.
    ///
    /// A `Vec` of pairs rather than a map: the wire promises neither dense nor
    /// ascending indices (research §14), the count is single digits, and an
    /// insertion-ordered association list keeps the reply's call order without
    /// pulling an index-map crate into the tree.
    calls: Vec<(u32, PartialCall)>,
    finish_reason: Option<FinishReason>,
    usage: Option<TokenUsage>,
    model: Option<String>,
    response_id: Option<String>,
    saw_valid_frame: bool,
    max_arguments_bytes: usize,
}

/// The largest tool-call `arguments` string keeper will accumulate.
///
/// A hostile endpoint can stream a single argument forever; the silence
/// timeout does not catch that because the bytes keep coming (research §10.6).
pub const MAX_ARGUMENTS_BYTES: usize = 1 << 20;

impl Reassembler {
    /// A machine that knows which delta fields this kind puts reasoning in.
    pub fn new(kind: ProviderKind) -> Self {
        Self {
            reasoning_fields: quirks(kind).reasoning_fields,
            content: String::new(),
            reasoning: String::new(),
            refusal: None,
            calls: Vec::new(),
            finish_reason: None,
            usage: None,
            model: None,
            response_id: None,
            saw_valid_frame: false,
            max_arguments_bytes: MAX_ARGUMENTS_BYTES,
        }
    }

    /// Whether any frame has been successfully applied.
    pub fn saw_valid_frame(&self) -> bool {
        self.saw_valid_frame
    }

    /// The finish reason the wire stated, if it stated one.
    pub fn wire_finish_reason(&self) -> Option<&FinishReason> {
        self.finish_reason.as_ref()
    }

    /// Apply one `data:` payload.
    ///
    /// The malformed-frame policy is a decision, not a detail (research
    /// §1.3i), and this is it, in three classes:
    ///
    /// * **not JSON at all** — error the stream. A frame keeper cannot read
    ///   may be the error frame, and continuing past it commits a failed turn
    ///   to history as a successful one.
    /// * **valid JSON that is not an object** — a gateway keep-alive `null` or
    ///   a bare scalar. Skipped, so forward compatibility does not cost a
    ///   broken stream.
    /// * **an object carrying `error`** — surfaced as
    ///   [`BotsError::Provider`], never swallowed.
    pub fn apply(
        &mut self,
        payload: &str,
        sink: &mut dyn FnMut(ChatEvent),
    ) -> Result<(), BotsError> {
        let value: Value = serde_json::from_str(payload).map_err(|err| BotsError::Parse {
            endpoint: CHAT_ENDPOINT.to_owned(),
            detail: format!("a stream frame was not JSON: {err}"),
        })?;
        let Some(object) = value.as_object() else {
            // Not modelable, and not necessarily wrong. Skip it.
            tracing::debug!("bots: skipping a non-object stream frame");
            return Ok(());
        };

        if let Some(error) = object.get("error") {
            return Err(provider_error(error));
        }

        self.saw_valid_frame = true;

        if self.response_id.is_none() {
            if let Some(id) = object.get("id").and_then(Value::as_str) {
                self.response_id = Some(id.to_owned());
            }
        }
        if self.model.is_none() {
            if let Some(model) = object.get("model").and_then(Value::as_str) {
                self.model = Some(model.to_owned());
            }
        }

        // The usage chunk arrives with `choices: []`, and every other chunk
        // carries `usage: null` when `include_usage` is on — so a null must
        // never overwrite a real one.
        if let Some(usage) = object.get("usage").and_then(Value::as_object) {
            let usage = TokenUsage {
                prompt_tokens: read_u32(usage.get("prompt_tokens")),
                completion_tokens: read_u32(usage.get("completion_tokens")),
                total_tokens: read_u32(usage.get("total_tokens")),
            };
            self.usage = Some(usage);
            sink(ChatEvent::Usage(usage));
        }

        let Some(choices) = object.get("choices").and_then(Value::as_array) else {
            return Ok(());
        };
        // Select the choice whose `index` is 0, not the array's first element:
        // with `n > 1` the candidates interleave and are told apart only by
        // that field, so taking position 0 would concatenate every candidate
        // into one garbled answer (research §1.3c).
        let Some(choice) = choices.iter().find(|choice| {
            choice
                .get("index")
                .and_then(Value::as_u64)
                .is_none_or(|index| index == 0)
        }) else {
            return Ok(());
        };

        if let Some(delta) = choice.get("delta").and_then(Value::as_object) {
            self.apply_delta(delta, sink)?;
        }

        // Read the finish reason AFTER the delta, never instead of it: a chunk
        // may carry payload and a finish reason at once, and a machine that
        // stops at the reason loses the last content or the whole tool call
        // (research §1.3d).
        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            self.finish_reason = Some(FinishReason::from_wire(reason));
        }

        Ok(())
    }

    fn apply_delta(
        &mut self,
        delta: &Map<String, Value>,
        sink: &mut dyn FnMut(ChatEvent),
    ) -> Result<(), BotsError> {
        if let Some(content) = delta.get("content").and_then(Value::as_str) {
            // An empty content delta is normal — the role-only opening chunk
            // and the final `{}` both produce one (research §1.3h).
            if !content.is_empty() {
                self.content.push_str(content);
                sink(ChatEvent::ContentDelta(content.to_owned()));
            }
        }
        for field in self.reasoning_fields {
            if let Some(text) = delta.get(*field).and_then(Value::as_str) {
                if !text.is_empty() {
                    self.reasoning.push_str(text);
                    sink(ChatEvent::ReasoningDelta(text.to_owned()));
                }
            }
        }
        if let Some(refusal) = delta.get("refusal").and_then(Value::as_str) {
            self.refusal
                .get_or_insert_with(String::new)
                .push_str(refusal);
        }

        let Some(fragments) = delta.get("tool_calls").and_then(Value::as_array) else {
            return Ok(());
        };
        // The wire type is an array and a chunk may carry several fragments;
        // reading only the first is a real defect in a shipped client
        // (research §1.3k).
        for fragment in fragments {
            let Some(fragment) = fragment.as_object() else {
                continue;
            };
            let index = fragment
                .get("index")
                .and_then(Value::as_u64)
                .unwrap_or_default() as u32;
            let id = fragment
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty());
            let function = fragment.get("function").and_then(Value::as_object);
            let name = function
                .and_then(|function| function.get("name"))
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty());
            let arguments = function
                .and_then(|function| function.get("arguments"))
                .and_then(Value::as_str)
                .unwrap_or_default();

            // Read before the slot borrow: `slot` holds `&mut *self`.
            let cap = self.max_arguments_bytes;
            let slot = self.slot(index);
            if let Some(id) = id {
                if slot.id.is_empty() {
                    slot.id = id.to_owned();
                }
            }
            if let Some(name) = name {
                if slot.name.is_empty() {
                    slot.name = name.to_owned();
                }
            }
            if !arguments.is_empty() {
                if slot.arguments.len() + arguments.len() > cap {
                    return Err(BotsError::Protocol {
                        detail: format!("a tool call's arguments exceeded {cap} bytes"),
                    });
                }
                // Accumulated as a STRING and parsed once at the end: a
                // fragment such as `{"ci` is not JSON, and parsing per
                // fragment is the classic way to lose a whole call (research
                // §1.3a).
                slot.arguments.push_str(arguments);
            }

            sink(ChatEvent::ToolCallDelta {
                index,
                id: id.map(str::to_owned),
                name: name.map(str::to_owned),
                arguments: arguments.to_owned(),
            });
        }
        Ok(())
    }

    /// The accumulator for one wire index, created in first-seen order.
    fn slot(&mut self, index: u32) -> &mut PartialCall {
        if let Some(position) = self.calls.iter().position(|(key, _)| *key == index) {
            return &mut self.calls[position].1;
        }
        self.calls.push((index, PartialCall::default()));
        let last = self.calls.len() - 1;
        &mut self.calls[last].1
    }

    /// Reduce to the answer, parsing every `arguments` string exactly once.
    pub fn finish(
        self,
        reason: FinishReason,
        first_token_ms: Option<u64>,
        total_ms: u64,
    ) -> ChatOutcome {
        let tool_calls = self
            .calls
            .into_iter()
            .map(|(index, call)| {
                let arguments = serde_json::from_str::<Value>(&call.arguments).ok();
                if arguments.is_none() && !call.arguments.is_empty() {
                    // Never dropped: an unparseable call is still provider
                    // output, and `arguments_raw` is the evidence.
                    tracing::warn!(
                        index,
                        "bots: a tool call's arguments were not valid JSON; keeping the raw string"
                    );
                }
                ToolCall {
                    id: if call.id.is_empty() {
                        format!("call_{index}")
                    } else {
                        call.id
                    },
                    name: call.name,
                    arguments_raw: call.arguments,
                    arguments,
                }
            })
            .collect();

        ChatOutcome {
            content: self.content,
            reasoning: self.reasoning,
            refusal: self.refusal,
            tool_calls,
            finish_reason: reason,
            usage: self.usage,
            model: self.model,
            response_id: self.response_id,
            first_token_ms,
            total_ms,
        }
    }
}

fn provider_error(error: &Value) -> BotsError {
    let (code, message) = match error {
        Value::Object(object) => (
            object
                .get("code")
                .and_then(Value::as_str)
                .map(str::to_owned),
            object
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("the endpoint sent an error with no message")
                .to_owned(),
        ),
        Value::String(message) => (None, message.clone()),
        other => (None, other.to_string()),
    };
    BotsError::Provider { code, message }
}

fn read_u32(value: Option<&Value>) -> Option<u32> {
    value
        .and_then(Value::as_u64)
        .and_then(|number| u32::try_from(number).ok())
}

// ---------------------------------------------------------------------------
// Cancellation
// ---------------------------------------------------------------------------

/// The stop button's end of a cancellation.
#[derive(Debug)]
pub struct CancelHandle {
    tx: tokio::sync::watch::Sender<bool>,
}

/// The stream's end of a cancellation.
#[derive(Debug, Clone)]
pub struct CancelSignal {
    rx: tokio::sync::watch::Receiver<bool>,
}

/// A fresh cancellation pair.
///
/// A watch channel rather than a bare flag because the driver must wake from a
/// *silent* socket: a flag is only read between chunks, so Stop on a stalled
/// stream would appear to do nothing until the silence budget expired.
pub fn cancellation() -> (CancelHandle, CancelSignal) {
    let (tx, rx) = tokio::sync::watch::channel(false);
    (CancelHandle { tx }, CancelSignal { rx })
}

impl CancelHandle {
    /// Stop the stream. Idempotent.
    pub fn cancel(&self) {
        let _ = self.tx.send(true);
    }
}

impl CancelSignal {
    /// Whether cancellation has already been requested.
    pub fn is_cancelled(&self) -> bool {
        *self.rx.borrow()
    }

    /// Resolves when cancellation is requested.
    async fn cancelled(&mut self) {
        loop {
            if *self.rx.borrow_and_update() {
                return;
            }
            if self.rx.changed().await.is_err() {
                // Every handle was dropped without cancelling, so this can
                // never fire. Park rather than resolve: resolving would abort
                // a perfectly healthy stream the moment its handle went out of
                // scope.
                std::future::pending::<()>().await;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The driver
// ---------------------------------------------------------------------------

/// The knobs a caller may turn per request.
#[derive(Debug, Clone)]
pub struct ChatOptions {
    /// The silence budget. See [`crate::bots::http`].
    pub read_timeout: Duration,
    /// How many times the request may be SENT, including the first. `1`
    /// disables retrying.
    pub max_attempts: u32,
    /// The base back-off between attempts.
    pub retry_backoff: Duration,
    /// A ceiling on the whole streamed body.
    pub max_stream_bytes: usize,
}

impl Default for ChatOptions {
    fn default() -> Self {
        Self {
            read_timeout: crate::bots::http::READ_TIMEOUT,
            // One retry. A desktop client talking to one endpoint the user
            // chose is not a fleet: the second attempt catches the dropped
            // connection and the momentary 503, and a third would only make a
            // wrong base URL take longer to say so.
            max_attempts: 2,
            retry_backoff: Duration::from_millis(500),
            // Sixteen mebibytes of a single answer is already far past what a
            // webview will render; a stream past it is a runaway, not a reply.
            max_stream_bytes: 16 * 1024 * 1024,
        }
    }
}

/// A failed attempt, and the two facts the retry decision needs.
struct AttemptFailure {
    error: BotsError,
    /// Whether the endpoint had already streamed anything when it failed.
    saw_stream_bytes: bool,
    /// The completion so far, when there is one worth keeping.
    partial: Option<ChatOutcome>,
    /// `Retry-After`, where the endpoint sent one.
    retry_after: Option<Duration>,
}

/// Send one chat completion and drive its stream to a [`ChatOutcome`].
///
/// # The retry rule, and why it is written this narrowly
///
/// A request is sent again only when **all** of these hold: it produced no
/// stream byte, its failure is a connect failure / 429 / 5xx, and an attempt
/// remains. `Retry-After` is honoured where the endpoint sends one — Hermes
/// sends it on its concurrency rejection (research §2.8).
///
/// A partially-streamed completion is **never** retried, for three reasons
/// that are all somebody else's scar tissue (research §7.3): Chat Completions
/// SSE has no resume token and no `Last-Event-ID` semantics, so there is
/// nothing to resume from; a re-sent request samples afresh, so splicing the
/// two halves produces text the model never wrote; and the tokens of the
/// failed attempt were generated and billed whether or not keeper kept them.
/// Retrying it would turn one truncated answer into two charges and one lie.
pub async fn stream_chat(
    client: &reqwest::Client,
    endpoint: &Endpoint,
    request: &ChatRequest,
    options: &ChatOptions,
    cancel: CancelSignal,
    sink: ChatSink<'_>,
) -> Result<ChatOutcome, BotsError> {
    let body = build_body(endpoint.kind, request)?;
    let url = endpoint.url(CHAT_PATH);
    let attempts = options.max_attempts.max(1);

    for attempt in 0..attempts {
        let failure = match attempt_stream(
            client,
            endpoint,
            &url,
            &body,
            options,
            cancel.clone(),
            sink,
        )
        .await
        {
            Ok(outcome) => return Ok(outcome),
            Err(failure) => failure,
        };

        let may_retry =
            !failure.saw_stream_bytes && failure.error.is_retryable() && attempt + 1 < attempts;

        if may_retry {
            let wait = failure
                .retry_after
                .unwrap_or_else(|| backoff(options.retry_backoff, attempt));
            tracing::warn!(
                attempt = attempt + 1,
                wait_ms = wait.as_millis() as u64,
                "bots: retrying a chat request that produced no bytes"
            );
            tokio::time::sleep(wait).await;
            continue;
        }

        return match failure.partial {
            // The endpoint had already answered. Keep what it said, hand the
            // caller the typed cause through the sink, and report the partial
            // record rather than throwing it away to return an error.
            Some(mut partial) => {
                sink(ChatEvent::Failed {
                    error: failure.error,
                });
                partial.finish_reason = FinishReason::Failed;
                sink(ChatEvent::Finished {
                    reason: partial.finish_reason.clone(),
                });
                Ok(partial)
            }
            None => Err(failure.error),
        };
    }

    // Unreachable: the loop either returns or continues, and `attempts >= 1`.
    Err(BotsError::Protocol {
        detail: "the request was never attempted".to_owned(),
    })
}

/// Exponential back-off, saturating.
///
/// No jitter: jitter exists to de-synchronise a fleet, and this is one desktop
/// client talking to one endpoint the user chose. Saturating arithmetic
/// because an overflow on a long-lived session is a real bug class (research
/// §7.2).
fn backoff(base: Duration, attempt: u32) -> Duration {
    let factor = 2u32.saturating_pow(attempt);
    base.saturating_mul(factor)
}

async fn attempt_stream(
    client: &reqwest::Client,
    endpoint: &Endpoint,
    url: &str,
    body: &Value,
    options: &ChatOptions,
    mut cancel: CancelSignal,
    sink: ChatSink<'_>,
) -> Result<ChatOutcome, Box<AttemptFailure>> {
    let started = Instant::now();

    let request = client.post(url).json(body);
    let request = authorize(request, endpoint.token.as_deref()).map_err(|error| {
        Box::new(AttemptFailure {
            error,
            saw_stream_bytes: false,
            partial: None,
            retry_after: None,
        })
    })?;

    let mut response = match request.send().await {
        Ok(response) => response,
        Err(err) => {
            return Err(Box::new(AttemptFailure {
                error: BotsError::transport(err),
                saw_stream_bytes: false,
                partial: None,
                retry_after: None,
            }))
        }
    };

    let status = response.status();
    if !status.is_success() {
        let retry_after = parse_retry_after(&response);
        let detail = read_bounded_body(&mut response).await;
        return Err(Box::new(AttemptFailure {
            error: BotsError::status(status.as_u16(), CHAT_ENDPOINT, &detail),
            saw_stream_bytes: false,
            partial: None,
            retry_after,
        }));
    }

    let mut framer = SseFramer::new();
    let mut machine = Reassembler::new(endpoint.kind);
    let mut first_token_ms: Option<u64> = None;
    let mut streamed_bytes: usize = 0;
    let mut saw_done = false;
    let mut cancelled = false;

    // The sink is wrapped so `FirstToken` is emitted exactly once, ahead of
    // whichever delta arrived first, without every emit site remembering to.
    macro_rules! emit {
        ($event:expr) => {{
            let event = $event;
            if first_token_ms.is_none()
                && matches!(
                    event,
                    ChatEvent::ContentDelta(_)
                        | ChatEvent::ReasoningDelta(_)
                        | ChatEvent::ToolCallDelta { .. }
                )
            {
                let after_ms = started.elapsed().as_millis() as u64;
                first_token_ms = Some(after_ms);
                sink(ChatEvent::FirstToken { after_ms });
            }
            sink(event);
        }};
    }

    'stream: loop {
        let chunk = tokio::select! {
            biased;
            () = cancel.cancelled() => {
                cancelled = true;
                break 'stream;
            }
            // No `tokio::time::timeout` here on purpose: the silence budget is
            // the CLIENT's `read_timeout`, which resets on every successful
            // read. A timeout wrapped around the whole stream — or around the
            // whole request — is the mistake `keeper-sync/src/http.rs:24-40`
            // documents, and a reasoning model that thinks for a minute would
            // trip it every time.
            chunk = response.chunk() => chunk,
        };

        match chunk {
            Ok(Some(bytes)) => {
                streamed_bytes += bytes.len();
                if streamed_bytes > options.max_stream_bytes {
                    return Err(Box::new(AttemptFailure {
                        error: BotsError::Protocol {
                            detail: format!(
                                "the stream exceeded {} bytes",
                                options.max_stream_bytes
                            ),
                        },
                        saw_stream_bytes: true,
                        partial: Some(machine.finish(
                            FinishReason::Failed,
                            first_token_ms,
                            started.elapsed().as_millis() as u64,
                        )),
                        retry_after: None,
                    }));
                }
                framer.push(&bytes);
            }
            Ok(None) => break 'stream,
            Err(err) => {
                let error = if err.is_timeout() {
                    BotsError::Timeout {
                        after_ms: options.read_timeout.as_millis() as u64,
                    }
                } else {
                    BotsError::transport(err)
                };
                let saw_stream_bytes = streamed_bytes > 0;
                return Err(Box::new(AttemptFailure {
                    error,
                    saw_stream_bytes,
                    partial: saw_stream_bytes.then(|| {
                        machine.finish(
                            FinishReason::Failed,
                            first_token_ms,
                            started.elapsed().as_millis() as u64,
                        )
                    }),
                    retry_after: None,
                }));
            }
        }

        loop {
            let frame = match framer.next_frame() {
                Ok(Some(frame)) => frame,
                Ok(None) => break,
                Err(error) => {
                    return Err(Box::new(AttemptFailure {
                        error,
                        saw_stream_bytes: true,
                        partial: Some(machine.finish(
                            FinishReason::Failed,
                            first_token_ms,
                            started.elapsed().as_millis() as u64,
                        )),
                        retry_after: None,
                    }))
                }
            };
            match frame {
                SseFrame::Done => {
                    saw_done = true;
                    break 'stream;
                }
                SseFrame::Data(payload) => {
                    if let Err(error) = machine.apply(&payload, &mut |event| emit!(event)) {
                        return Err(Box::new(AttemptFailure {
                            error,
                            saw_stream_bytes: true,
                            partial: Some(machine.finish(
                                FinishReason::Failed,
                                first_token_ms,
                                started.elapsed().as_millis() as u64,
                            )),
                            retry_after: None,
                        }));
                    }
                }
            }
        }
    }

    let total_ms = started.elapsed().as_millis() as u64;

    if cancelled {
        // A cancelled turn keeps everything that arrived. This is the whole
        // reason Stop is safe to press.
        let outcome = machine.finish(FinishReason::Cancelled, first_token_ms, total_ms);
        sink(ChatEvent::Finished {
            reason: FinishReason::Cancelled,
        });
        return Ok(outcome);
    }

    // Only `[DONE]` or a stated finish reason counts as the provider
    // completing the turn. A stream that reached EOF with neither was
    // truncated, and synthesising a terminal record would present a cut-off
    // turn as a successful, default-usage completion (research §1.3f).
    let completed = saw_done || machine.wire_finish_reason().is_some();
    if !completed {
        let pending = framer.pending_bytes();
        let detail = if machine.saw_valid_frame() {
            format!("the stream ended mid-message, with {pending} bytes of an unfinished frame")
        } else {
            "the stream closed before it sent anything keeper could read".to_owned()
        };
        let saw_stream_bytes = streamed_bytes > 0;
        return Err(Box::new(AttemptFailure {
            error: BotsError::Protocol { detail },
            saw_stream_bytes,
            partial: saw_stream_bytes
                .then(|| machine.finish(FinishReason::Failed, first_token_ms, total_ms)),
            retry_after: None,
        }));
    }

    let reason = machine
        .wire_finish_reason()
        .cloned()
        // `[DONE]` with no stated reason: xAI and others simply omit it
        // (research §1.3e), and the turn did complete.
        .unwrap_or(FinishReason::Stop);
    let outcome = machine.finish(reason.clone(), first_token_ms, total_ms);
    sink(ChatEvent::Finished { reason });
    Ok(outcome)
}

/// `Retry-After`, in the delta-seconds form both back ends use.
fn parse_retry_after(response: &reqwest::Response) -> Option<Duration> {
    response
        .headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

/// Read at most [`MAX_ERROR_BODY_BYTES`] of a failing response.
///
/// Never `Response::text`: that reads whatever the peer decided to send, and
/// the peer here is a URL the user typed.
async fn read_bounded_body(response: &mut reqwest::Response) -> String {
    let mut buffer: Vec<u8> = Vec::new();
    while buffer.len() < MAX_ERROR_BODY_BYTES {
        match response.chunk().await {
            Ok(Some(bytes)) => buffer.extend_from_slice(&bytes),
            Ok(None) | Err(_) => break,
        }
    }
    buffer.truncate(MAX_ERROR_BODY_BYTES);
    String::from_utf8_lossy(&buffer).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(kind: ProviderKind, payloads: &[&str]) -> ChatOutcome {
        let mut machine = Reassembler::new(kind);
        let mut sink = |_event: ChatEvent| {};
        for payload in payloads {
            machine
                .apply(payload, &mut sink)
                .expect("a modelable frame");
        }
        let reason = machine
            .wire_finish_reason()
            .cloned()
            .unwrap_or(FinishReason::Stop);
        machine.finish(reason, None, 0)
    }

    #[test]
    fn content_deltas_concatenate_in_order() {
        let outcome = drain(
            ProviderKind::Hermes,
            &[
                r#"{"id":"c1","model":"m","choices":[{"index":0,"delta":{"role":"assistant"}}]}"#,
                r#"{"choices":[{"index":0,"delta":{"content":"Hel"}}]}"#,
                r#"{"choices":[{"index":0,"delta":{"content":"lo"}}]}"#,
                r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
            ],
        );
        assert_eq!(outcome.content, "Hello");
        assert_eq!(outcome.finish_reason, FinishReason::Stop);
        assert_eq!(outcome.response_id.as_deref(), Some("c1"));
        assert_eq!(outcome.model.as_deref(), Some("m"));
    }

    #[test]
    fn a_chunk_carrying_payload_and_a_finish_reason_keeps_both() {
        // The genai/Ollama shape: a tool call and `finish_reason` in one frame.
        let outcome = drain(
            ProviderKind::Ollama,
            &[
                r#"{"choices":[{"index":0,"finish_reason":"tool_calls","delta":{"tool_calls":[
                 {"index":0,"id":"call_a","type":"function",
                  "function":{"name":"get_weather","arguments":"{\"city\":\"Paris\"}"}}]}}]}"#,
            ],
        );
        assert_eq!(outcome.finish_reason, FinishReason::ToolCalls);
        assert_eq!(outcome.tool_calls.len(), 1);
        assert_eq!(outcome.tool_calls[0].name, "get_weather");
        assert_eq!(
            outcome.tool_calls[0].arguments,
            Some(json!({"city": "Paris"}))
        );
    }

    #[test]
    fn tool_calls_are_keyed_by_index_not_by_array_position() {
        // Two calls, fragments interleaved, and the SECOND chunk lists them in
        // the opposite order. Keying by position would swap their arguments.
        let outcome = drain(
            ProviderKind::Hermes,
            &[
                r#"{"choices":[{"index":0,"delta":{"tool_calls":[
                     {"index":0,"id":"a","function":{"name":"alpha","arguments":"{\"x\":"}},
                     {"index":1,"id":"b","function":{"name":"beta","arguments":"{\"y\":"}}]}}]}"#,
                r#"{"choices":[{"index":0,"delta":{"tool_calls":[
                     {"index":1,"function":{"arguments":"2}"}},
                     {"index":0,"function":{"arguments":"1}"}}]}}]}"#,
                r#"{"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#,
            ],
        );
        assert_eq!(outcome.tool_calls.len(), 2);
        assert_eq!(outcome.tool_calls[0].id, "a");
        assert_eq!(outcome.tool_calls[0].arguments, Some(json!({"x": 1})));
        assert_eq!(outcome.tool_calls[1].id, "b");
        assert_eq!(outcome.tool_calls[1].arguments, Some(json!({"y": 2})));
    }

    #[test]
    fn out_of_order_and_gapped_indices_keep_first_seen_order() {
        let outcome = drain(
            ProviderKind::Hermes,
            &[
                r#"{"choices":[{"index":0,"delta":{"tool_calls":[
                     {"index":7,"id":"seven","function":{"name":"g","arguments":"{}"}}]}}]}"#,
                r#"{"choices":[{"index":0,"delta":{"tool_calls":[
                     {"index":2,"id":"two","function":{"name":"h","arguments":"{}"}}]}},
                   {"index":1,"delta":{"content":"other candidate"}}]}"#,
                r#"{"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#,
            ],
        );
        let ids: Vec<&str> = outcome
            .tool_calls
            .iter()
            .map(|call| call.id.as_str())
            .collect();
        assert_eq!(ids, vec!["seven", "two"]);
        // The `index: 1` candidate must not have leaked into the answer.
        assert_eq!(outcome.content, "");
    }

    #[test]
    fn an_unparseable_arguments_string_is_kept_raw_not_dropped() {
        let outcome = drain(
            ProviderKind::Hermes,
            &[r#"{"choices":[{"index":0,"delta":{"tool_calls":[
                     {"index":0,"id":"a","function":{"name":"f","arguments":"{\"ci"}}]},
                   "finish_reason":"length"}]}"#],
        );
        assert_eq!(outcome.tool_calls[0].arguments, None);
        assert_eq!(outcome.tool_calls[0].arguments_raw, "{\"ci");
        assert_eq!(outcome.finish_reason, FinishReason::Length);
    }

    #[test]
    fn a_tool_call_without_an_id_gets_a_stable_synthesised_one() {
        let outcome = drain(
            ProviderKind::Ollama,
            &[r#"{"choices":[{"index":0,"delta":{"tool_calls":[
                 {"index":3,"function":{"name":"f","arguments":"{}"}}]},"finish_reason":"tool_calls"}]}"#],
        );
        assert_eq!(outcome.tool_calls[0].id, "call_3");
    }

    #[test]
    fn reasoning_is_its_own_channel_and_never_joins_the_answer() {
        let outcome = drain(
            ProviderKind::Ollama,
            &[
                r#"{"choices":[{"index":0,"delta":{"reasoning":"thinking…","content":""}}]}"#,
                r#"{"choices":[{"index":0,"delta":{"content":"42"},"finish_reason":"stop"}]}"#,
            ],
        );
        assert_eq!(outcome.reasoning, "thinking…");
        assert_eq!(outcome.content, "42");
    }

    #[test]
    fn a_null_usage_never_overwrites_a_real_one() {
        let outcome = drain(
            ProviderKind::Hermes,
            &[
                r#"{"choices":[{"index":0,"delta":{"content":"x"}}],"usage":null}"#,
                r#"{"choices":[],"usage":{"prompt_tokens":3,"completion_tokens":1,"total_tokens":4}}"#,
                r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":null}"#,
            ],
        );
        assert_eq!(
            outcome.usage,
            Some(TokenUsage {
                prompt_tokens: Some(3),
                completion_tokens: Some(1),
                total_tokens: Some(4),
            })
        );
    }

    #[test]
    fn an_in_band_error_frame_is_surfaced_and_never_swallowed() {
        let mut machine = Reassembler::new(ProviderKind::Hermes);
        let err = machine
            .apply(
                r#"{"error":{"code":"rate_limit_exceeded","message":"slow down"}}"#,
                &mut |_| {},
            )
            .expect_err("an error frame must terminate the stream");
        assert!(matches!(
            err,
            BotsError::Provider { ref code, .. } if code.as_deref() == Some("rate_limit_exceeded")
        ));
    }

    #[test]
    fn a_non_object_frame_is_skipped_rather_than_erroring_the_stream() {
        let mut machine = Reassembler::new(ProviderKind::Hermes);
        machine.apply("null", &mut |_| {}).expect("skipped");
        assert!(!machine.saw_valid_frame());
    }

    #[test]
    fn a_frame_that_is_not_json_errors_the_stream() {
        let mut machine = Reassembler::new(ProviderKind::Hermes);
        assert!(matches!(
            machine.apply("{not json", &mut |_| {}),
            Err(BotsError::Parse { .. })
        ));
    }

    #[test]
    fn ollama_drops_tool_choice_and_hermes_keeps_it() {
        let request = ChatRequest {
            model: "m".to_owned(),
            messages: vec![ChatMessage::text(Role::User, "hi")],
            tools: vec![ToolSpec {
                name: "f".to_owned(),
                description: "d".to_owned(),
                parameters: json!({"type": "object", "properties": {}}),
            }],
            tool_choice: Some(ToolChoice::Required),
            ..ChatRequest::default()
        };
        let ollama = build_body(ProviderKind::Ollama, &request).expect("body");
        assert!(ollama.get("tool_choice").is_none());
        assert!(!honours_tool_choice(ProviderKind::Ollama));

        let hermes = build_body(ProviderKind::Hermes, &request).expect("body");
        assert_eq!(hermes.get("tool_choice"), Some(&json!("required")));
    }

    #[test]
    fn the_body_always_streams_and_always_asks_for_usage() {
        let request = ChatRequest {
            model: "m".to_owned(),
            messages: vec![ChatMessage::text(Role::User, "hi")],
            ..ChatRequest::default()
        };
        let body = build_body(ProviderKind::Hermes, &request).expect("body");
        assert_eq!(body.get("stream"), Some(&json!(true)));
        assert_eq!(
            body.get("stream_options"),
            Some(&json!({"include_usage": true}))
        );
    }

    #[test]
    fn an_image_takes_the_shape_its_provider_documents() {
        let request = ChatRequest {
            model: "m".to_owned(),
            messages: vec![ChatMessage {
                role: Role::User,
                content: vec![
                    ContentPart::Text("what is this".to_owned()),
                    ContentPart::Image {
                        url: "data:image/png;base64,AAAA".to_owned(),
                        detail: Some("high".to_owned()),
                    },
                ],
                ..ChatMessage::default()
            }],
            ..ChatRequest::default()
        };

        let hermes = build_body(ProviderKind::Hermes, &request).expect("body");
        let part = &hermes["messages"][0]["content"][1];
        assert_eq!(
            part["image_url"]["url"],
            json!("data:image/png;base64,AAAA")
        );
        assert_eq!(part["image_url"]["detail"], json!("high"));

        let ollama = build_body(ProviderKind::Ollama, &request).expect("body");
        let part = &ollama["messages"][0]["content"][1];
        assert_eq!(part["image_url"], json!("data:image/png;base64,AAAA"));
    }

    #[test]
    fn a_remote_image_url_is_refused_for_the_kind_that_will_not_fetch_one() {
        let request = ChatRequest {
            model: "m".to_owned(),
            messages: vec![ChatMessage {
                role: Role::User,
                content: vec![ContentPart::Image {
                    url: "https://example.com/cat.png".to_owned(),
                    detail: None,
                }],
                ..ChatMessage::default()
            }],
            ..ChatRequest::default()
        };
        assert!(matches!(
            build_body(ProviderKind::Ollama, &request),
            Err(BotsError::Unsupported { .. })
        ));
        assert!(build_body(ProviderKind::Hermes, &request).is_ok());
    }

    #[test]
    fn backoff_doubles_and_saturates() {
        let base = Duration::from_millis(500);
        assert_eq!(backoff(base, 0), Duration::from_millis(500));
        assert_eq!(backoff(base, 1), Duration::from_millis(1000));
        assert_eq!(backoff(base, 2), Duration::from_millis(2000));
        // No overflow panic on an absurd attempt count.
        assert!(backoff(base, 200) >= Duration::from_millis(500));
    }
}
