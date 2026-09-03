//! Where the two back ends disagree, as data (Story 61.2).
//!
//! # The decision this file is
//!
//! The epic picked one wire — `POST /v1/chat/completions` with `stream: true`
//! — for both provider kinds, and wrote down that the price is paid here:
//! *"Those become rows in a per-kind quirk table, not a second client."* So
//! [`chat`](crate::bots::chat) contains **no** `if kind == Ollama` branch. It
//! reads [`quirks`] once and shapes the request from the answer, which is the
//! difference between adding a third provider being a table row and being an
//! archaeology exercise.
//!
//! # A capability keeper could not read is `unknown`, never `false`
//!
//! Several of these rows are [`Support::Unknown`], and that is the honest
//! answer rather than a gap. Hermes' api-server documentation enumerates the
//! Chat Completions fields it honours and the ones it ignores, and
//! `tool_choice` is in neither list; recording that as "not supported" would
//! make keeper hide an affordance on the strength of something nobody read.
//! Unknown behaves as "allowed, and say so if it fails", which is the rule the
//! rest of the epic uses for a model's vision flag.
//!
//! Every row below carries the source it was read from.

use crate::bots::ProviderKind;

/// A three-valued answer, because two values would force a lie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    /// Read from the provider's own documentation or source.
    Yes,
    /// Read from the provider's own documentation or source as *not*
    /// supported.
    No,
    /// Not stated by any source keeper's research read. Treated as permitted:
    /// keeper sends the field and reports what came back.
    Unknown,
}

impl Support {
    /// Whether keeper may send the thing this row describes.
    ///
    /// [`Support::Unknown`] is permitted on purpose — see the module docs.
    pub fn permitted(self) -> bool {
        !matches!(self, Self::No)
    }
}

/// How a provider wants an image content part shaped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImagePartShape {
    /// OpenAI's documented shape: `{"type":"image_url","image_url":{"url":…}}`.
    Object,
    /// Ollama's documented shape: the same part with `image_url` as a bare
    /// string rather than an object.
    BareString,
}

/// Everything [`crate::bots::chat`] needs to know about one provider kind.
#[derive(Debug, Clone, Copy)]
pub struct Quirks {
    /// Whether `tool_choice` reaches the model.
    ///
    /// * Ollama — **No**. Its OpenAI-compatibility matrix marks `tool_choice`
    ///   unsupported, alongside `logit_bias`, `user` and `n`: "you cannot
    ///   force or forbid a tool" (research §5.1, §5.4.3, from
    ///   `docs/api/openai-compatibility.mdx`). Sending it is not an error, it
    ///   is silently ignored — which is worse, so keeper drops it and can say
    ///   why.
    /// * Hermes — **Unknown**. Its "honoured vs ignored" table for
    ///   `/v1/chat/completions` lists `messages`, `model`, `provider`,
    ///   `model_options`, `stream` and the system-message layering, and does
    ///   not mention `tool_choice` either way (research §2.4). Note the
    ///   related and *documented* asymmetry: Hermes executes tools
    ///   server-side and replays them as completed, never handing the client a
    ///   pending call (research §2.7), so the field is unlikely to matter
    ///   there at all.
    pub tool_choice: Support,

    /// Whether an `http(s)` image URL is fetched by the endpoint.
    ///
    /// * Ollama — **No**. The compatibility matrix is explicit: "Base64
    ///   encoded image ✔ / Image URL ✘", and the vision documentation passes a
    ///   `data:` URI (research §5.1, §7). keeper refuses the part rather than
    ///   sending one that will be dropped.
    /// * Hermes — **Yes**. "Remote `http(s)` and `data:image/...` both
    ///   supported" on Chat Completions (research §2.5).
    pub remote_image_url: Support,

    /// The shape of an image content part.
    ///
    /// * Ollama — [`ImagePartShape::BareString`]: every example in
    ///   `docs/api/openai-compatibility.mdx` passes `"image_url":
    ///   "data:image/png;base64,…"`, a string and not an object (research §7).
    ///   Whether it *also* accepts the object form was not verified in
    ///   `server/routes.go` (research §14), so keeper sends the shape that is
    ///   documented.
    /// * Hermes — [`ImagePartShape::Object`]: `{"type":"image_url",
    ///   "image_url":{"url":…,"detail":"high"}}` (research §2.5).
    pub image_part: ImagePartShape,

    /// Whether the context window can be set over the `/v1` layer.
    ///
    /// * Ollama — **No**, and this is the sharpest cost of the one-wire
    ///   decision. "The OpenAI API does not have a way of setting the context
    ///   size for a model"; `num_ctx` needs the native `POST /api/chat`
    ///   (research §5.4.5). The native dialect is deferred (DW-210), so a
    ///   surface that offers a context-window control for Ollama would be an
    ///   affordance that lies.
    /// * Hermes — **No**. Its request-scoped `model_options` object carries
    ///   exactly `reasoning_effort` and `service_tier` (research §2.4); no
    ///   context-window knob exists on this door.
    pub context_window_over_v1: Support,

    /// Whether the stream ends with the literal `data: [DONE]` sentinel.
    ///
    /// * Ollama — **Yes**. `middleware/openai.go:157` writes
    ///   `data: [DONE]\n\n` (research §5.4.1).
    /// * Hermes — **Yes**. "Terminator `data: [DONE]`" (research §2.6).
    ///
    /// Both being `Yes` is precisely why [`crate::bots::chat`] may treat a
    /// stream that ends without it as truncated rather than as merely
    /// finished.
    pub done_sentinel: Support,

    /// Whether `stream_options.include_usage` produces a usage chunk.
    ///
    /// * Ollama — **Yes**. `stream_options.include_usage` is a ticked row on
    ///   the compatibility matrix (research §5.1).
    /// * Hermes — **Unknown**. `usage.{prompt_tokens,completion_tokens,
    ///   total_tokens}` is documented on the *non-streaming* response
    ///   (research §2.4) and the streaming section does not mention a usage
    ///   frame (research §2.6). keeper therefore asks for it and records
    ///   absence as absence — Story 61.8's rule that a missing number renders
    ///   as missing and never as zero.
    pub stream_usage: Support,

    /// The `delta` fields this kind may put reasoning tokens in.
    ///
    /// Neither is an OpenAI Chat Completions field (research §1.2); both are
    /// provider extensions, which is exactly why they are listed here rather
    /// than hard-coded in the parser.
    ///
    /// * Ollama — `reasoning`. `openai/openai.go:43-48` puts `reasoning` on
    ///   the streaming delta (research §6.3), and R3 records the same
    ///   behaviour from a second implementation: "Some providers (e.g.,
    ///   Ollama) emit reasoning in `delta.reasoning` and send empty content"
    ///   (research §1.4).
    /// * Hermes — both `reasoning_content` and `reasoning`, because Hermes is
    ///   a gateway in front of other providers (research §2.4, model
    ///   precedence) and the field that arrives is the upstream's, not
    ///   Hermes'. `reasoning_content` is the DeepSeek-style spelling (research
    ///   §1.4).
    pub reasoning_fields: &'static [&'static str],

    /// The interval at which the server sends an SSE keep-alive comment, where
    /// it documents one.
    ///
    /// This is the calibration input for the read timeout: a silence budget
    /// below the server's own keep-alive kills healthy streams on a schedule.
    ///
    /// * Hermes — 30 s (`CHAT_COMPLETIONS_SSE_KEEPALIVE_SECONDS = 30.0`,
    ///   research §2.3).
    /// * Ollama — none documented; `middleware/openai.go` writes frames as the
    ///   model produces them (research §5.4.1).
    pub keepalive_secs: Option<u64>,

    /// Whether this kind multiplexes named events onto the chat stream.
    ///
    /// * Hermes — **Yes**: `event: hermes.tool.progress` frames are
    ///   interleaved with `chat.completion.chunk` frames, and "progress never
    ///   appears in `delta.content`" (research §2.6). The framer parses the
    ///   `event:` field and ignores it, so this row is documentation of *why*
    ///   that path exists rather than a switch.
    /// * Ollama — **No**.
    pub named_events: Support,

    /// Whether this kind keeps a server-side conversation keeper can name
    /// and read back (Epic 63, AD-176).
    ///
    /// * Hermes — **Unknown** until asked. A Hermes new enough advertises
    ///   `session_*` feature flags and `session_continuity_header` on
    ///   `GET /v1/capabilities`, and then `GET /api/sessions` is a real
    ///   `last_active`-ordered list and `GET /api/sessions/{id}/messages` a
    ///   real history read (verified against `NousResearch/hermes-agent`
    ///   `main`, 2026-09-03). An older one answers nothing there, and
    ///   [`crate::bots::remote::probe_capabilities`] is what turns this row
    ///   into a definite answer per endpoint.
    /// * Ollama — **No**. There is no server-side conversation at all, so no
    ///   probe is sent and everything stays in `keeper.db`.
    pub server_sessions: Support,
}

/// The divergence table, keyed by provider kind.
///
/// `const fn` so a row is a compile-time constant at every call site and the
/// table cannot drift into being computed from runtime state.
pub const fn quirks(kind: ProviderKind) -> Quirks {
    match kind {
        ProviderKind::Hermes => Quirks {
            tool_choice: Support::Unknown,
            remote_image_url: Support::Yes,
            image_part: ImagePartShape::Object,
            context_window_over_v1: Support::No,
            done_sentinel: Support::Yes,
            stream_usage: Support::Unknown,
            reasoning_fields: &["reasoning_content", "reasoning"],
            keepalive_secs: Some(30),
            named_events: Support::Yes,
            server_sessions: Support::Unknown,
        },
        ProviderKind::Ollama => Quirks {
            tool_choice: Support::No,
            remote_image_url: Support::No,
            image_part: ImagePartShape::BareString,
            context_window_over_v1: Support::No,
            done_sentinel: Support::Yes,
            stream_usage: Support::Yes,
            reasoning_fields: &["reasoning"],
            keepalive_secs: None,
            named_events: Support::No,
            server_sessions: Support::No,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_is_permitted_and_no_is_not() {
        // The rule the whole epic turns on: keeper never hides a capability it
        // merely failed to read about.
        assert!(Support::Unknown.permitted());
        assert!(Support::Yes.permitted());
        assert!(!Support::No.permitted());
    }

    #[test]
    fn ollama_refuses_tool_choice_and_remote_images_and_hermes_does_not() {
        let ollama = quirks(ProviderKind::Ollama);
        assert_eq!(ollama.tool_choice, Support::No);
        assert_eq!(ollama.remote_image_url, Support::No);
        assert_eq!(ollama.image_part, ImagePartShape::BareString);

        let hermes = quirks(ProviderKind::Hermes);
        assert!(hermes.tool_choice.permitted());
        assert_eq!(hermes.remote_image_url, Support::Yes);
        assert_eq!(hermes.image_part, ImagePartShape::Object);
    }

    #[test]
    fn neither_kind_can_set_the_context_window_over_v1() {
        for kind in [ProviderKind::Hermes, ProviderKind::Ollama] {
            assert_eq!(quirks(kind).context_window_over_v1, Support::No);
        }
    }

    #[test]
    fn both_kinds_send_the_done_sentinel() {
        // The premise for treating a missing `[DONE]` as truncation.
        for kind in [ProviderKind::Hermes, ProviderKind::Ollama] {
            assert_eq!(quirks(kind).done_sentinel, Support::Yes);
        }
    }

    #[test]
    fn the_read_timeout_default_clears_every_documented_keepalive() {
        // A silence budget under the server's own keep-alive interval would
        // kill healthy idle streams on a schedule.
        let budget = crate::bots::http::READ_TIMEOUT.as_secs();
        for kind in [ProviderKind::Hermes, ProviderKind::Ollama] {
            if let Some(keepalive) = quirks(kind).keepalive_secs {
                assert!(
                    budget > keepalive,
                    "read timeout {budget}s must exceed the {keepalive}s server keep-alive"
                );
            }
        }
    }
}
