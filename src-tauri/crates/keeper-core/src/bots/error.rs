//! The one error type the whole `bots` module speaks (Story 61.2).
//!
//! # Two rules this file exists to enforce
//!
//! **A credential never reaches an error string.** A bot provider is
//! bearer-authenticated, and the base URL is typed by the user, so every
//! diagnostic in this module runs the risk of carrying either. So:
//!
//! * the `Authorization` header value is marked sensitive at the point it is
//!   built ([`crate::bots::http::authorization`]), which is what stops the
//!   `http` stack's own `Debug`/`Display` from printing it;
//! * a `reqwest::Error` is **never** stored as a `source`. Its `Display`
//!   includes the request URL, which for a user-supplied base URL is exactly
//!   the string this epic promises not to log. [`BotsError::transport`] is the
//!   only sanctioned way in, and it calls `reqwest::Error::without_url` first.
//! * a variant that names a request names it as method + path
//!   (`"POST /v1/chat/completions"`), never as an absolute URL.
//!
//! **A caller can tell the failures apart without reading prose.** 401, 404,
//! 500, a silence timeout, a truncated stream and a malformed body are six
//! different situations with six different honest sentences in the UI, so they
//! are six shapes here rather than one `String`.

use thiserror::Error;

/// How much of a failing response body may appear in an error message.
///
/// An endpoint that answers 500 usually says something useful; an endpoint
/// that answers 500 with a megabyte of HTML must not put a megabyte into a log
/// line. The body of an error response is also the one place a badly-written
/// gateway might echo the request back, so this is a cap on the blast radius
/// as much as on the length.
pub const MAX_ERROR_DETAIL_BYTES: usize = 512;

/// Every way a bot request can fail.
#[derive(Debug, Error)]
pub enum BotsError {
    /// The shared `reqwest::Client` could not be built. Not reachable from a
    /// user action — a broken TLS backend or an impossible builder policy.
    #[error("could not build the HTTP client: {0}")]
    ClientBuild(String),

    /// Connect, TLS, read or stream failure. `detail` has been through
    /// [`BotsError::transport`], so it carries no URL.
    ///
    /// `retryable` is decided at construction because it cannot be recovered
    /// from the string afterwards: a connect failure produced no bytes and may
    /// be retried, a mid-body read failure did produce bytes and may not (see
    /// [`crate::bots::chat`]).
    #[error("could not reach the endpoint: {detail}")]
    Transport {
        /// A URL-free description of the transport failure.
        detail: String,
        /// Whether this failure happened before any response byte existed.
        retryable: bool,
    },

    /// An established connection went quiet for longer than the read timeout.
    ///
    /// Distinct from [`BotsError::Transport`] because the sentence a user
    /// needs is different: nothing is wrong with the network, the model simply
    /// stopped answering.
    #[error("the endpoint went silent for {after_ms} ms")]
    Timeout {
        /// The silence budget that expired, in milliseconds.
        after_ms: u64,
    },

    /// The endpoint answered with a status keeper did not ask for.
    ///
    /// `endpoint` is method + path only. `detail` is a bounded, credential-free
    /// slice of the response body — see [`MAX_ERROR_DETAIL_BYTES`].
    #[error("{endpoint} returned HTTP {status}: {detail}")]
    Status {
        /// The HTTP status code.
        status: u16,
        /// Method and path, e.g. `POST /v1/chat/completions`.
        endpoint: String,
        /// A bounded excerpt of the response body, or `no body`.
        detail: String,
    },

    /// A body that should have been JSON was not, or was not the shape the
    /// wire promises.
    #[error("{endpoint} returned a body keeper could not read: {detail}")]
    Parse {
        /// Method and path, e.g. `GET /v1/models`.
        endpoint: String,
        /// What failed, without the body itself.
        detail: String,
    },

    /// The stream broke its own contract: it ended without `[DONE]` or a
    /// finish reason, or a single SSE frame exceeded its cap.
    ///
    /// Kept apart from [`BotsError::Transport`] on purpose. A socket that died
    /// is the network's fault; a stream that ended cleanly halfway through a
    /// sentence is the endpoint's, and only one of the two may be retried.
    #[error("the stream ended badly: {detail}")]
    Protocol {
        /// What the stream did instead of finishing.
        detail: String,
    },

    /// The endpoint reported an error inside the stream, as an `{"error": …}`
    /// frame rather than as a status code.
    ///
    /// Swallowing this is the failure R3 §1.3(g) names: a masked error frame
    /// followed by `[DONE]` commits a failed turn to history as a successful
    /// zero-usage completion.
    #[error("the endpoint reported an error{}: {message}", match code {
        Some(code) => format!(" ({code})"),
        None => String::new(),
    })]
    Provider {
        /// The provider's own machine-readable code, where it sent one.
        code: Option<String>,
        /// The provider's message.
        message: String,
    },

    /// The request asks for something this provider kind cannot do, and the
    /// quirk table ([`crate::bots::quirks`]) says so before a byte is sent.
    #[error("this provider cannot do that: {detail}")]
    Unsupported {
        /// Which capability, and which kind lacks it.
        detail: String,
    },

    /// The stored credential cannot be sent as an HTTP header — a pasted key
    /// with a newline or a non-ASCII character in it.
    ///
    /// A named refusal, never a silently dropped `Authorization` header: an
    /// unauthenticated request to a bearer endpoint fails as 401 and sends the
    /// user looking for the wrong bug. `detail` names the shape of the
    /// problem and never the credential.
    #[error("the stored credential cannot be sent as a header: {detail}")]
    Credential {
        /// What is wrong with the value, never the value.
        detail: String,
    },

    /// The base URL a provider row holds is not one keeper will send to.
    #[error(transparent)]
    BaseUrl(#[from] crate::bots::url::BaseUrlError),

    /// A filesystem tool call failed for a reason that is about the drive
    /// rather than about the endpoint (Story 61.11).
    ///
    /// The tool loop turns this into a `role: "tool"` message the model reads
    /// and carries on, because a model that asked for a file that is not there
    /// should get to ask for a different one — a stream failure would end the
    /// turn over a mistake the model could have fixed itself. `detail` is
    /// whichever layer refused, rendered verbatim: `keeper-sync`'s containment
    /// sentence, the write router's, or the OS's own words.
    #[error("{detail}")]
    Tool {
        /// The refusing layer's sentence, carried across and not paraphrased.
        detail: String,
    },

    /// A grant refused a tool call (Story 61.10, Story 61.11, FR-386).
    ///
    /// Separate from [`Self::Tool`] for one reason and it is not tidiness: a
    /// denial is a fact about the *user's* permissions, so the loop raises it
    /// to the user as well as answering it to the model. Every other tool
    /// failure is the model's business alone. `reason` is one of
    /// `keeper_core::bots::grant`'s `DENY_*` constants, quoted verbatim so the
    /// pane, the audit log and this error say the same words.
    #[error("{reason}")]
    GrantDenied {
        /// The grant layer's own sentence.
        reason: String,
    },
}

impl BotsError {
    /// The only sanctioned way to turn a `reqwest::Error` into a `BotsError`.
    ///
    /// `reqwest::Error`'s `Display` embeds the request URL, and a bot
    /// provider's base URL is typed by the user — `without_url` is therefore
    /// not a nicety, it is the redaction. It takes `self`, which is why this
    /// consumes the error and why the idiom at a call site is
    /// `.map_err(BotsError::transport)?`. The `Authorization` header is
    /// already excluded by its sensitive flag
    /// ([`crate::bots::http::authorization`]).
    pub fn transport(err: reqwest::Error) -> Self {
        // A failure that happened while connecting produced no response byte,
        // so the request may be sent again; anything later may not, because
        // half a completion has already been billed and cannot be resumed
        // (R3 §7.3). `is_timeout` here is reqwest's own connect timeout.
        let retryable = err.is_connect() || err.is_timeout();
        Self::Transport {
            detail: describe(err.without_url()),
            retryable,
        }
    }

    /// A status failure, with the body excerpt bounded and the endpoint named
    /// as method + path.
    pub fn status(status: u16, endpoint: impl Into<String>, body: &str) -> Self {
        Self::Status {
            status,
            endpoint: endpoint.into(),
            detail: excerpt(body),
        }
    }

    /// Whether sending this request again could plausibly succeed.
    ///
    /// Deliberately narrow, and deliberately not the whole answer: the caller
    /// must ALSO know that the failed attempt produced no stream bytes before
    /// it acts on this. See [`crate::bots::chat::stream_chat`].
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Transport { retryable, .. } => *retryable,
            // A timed-out request is the documented transport-retry case:
            // "wait a few seconds and retry" (R3 §7.1). This is precisely why
            // the no-bytes half of the rule is load-bearing rather than
            // decorative — a stream that stalled HALFWAY is also a timeout,
            // and that one must never be sent again.
            Self::Timeout { .. } => true,
            // 429 is the documented back-off case and 5xx the documented
            // "retry after a brief wait" case (R3 §7.1). 4xx otherwise is a
            // request keeper got wrong, and sending it again just repeats it.
            Self::Status { status, .. } => *status == 429 || (500..600).contains(status),
            _ => false,
        }
    }
}

/// A `reqwest::Error` rendered without its URL, with a bounded source chain.
fn describe(err: reqwest::Error) -> String {
    let mut text = err.to_string();
    let mut source = std::error::Error::source(&err);
    // Two levels is enough to name the real cause (`error sending request` ->
    // `tcp connect error` -> `Connection refused`) without walking a chain
    // whose length a hostile endpoint's TLS certificate could influence.
    for _ in 0..2 {
        let Some(inner) = source else { break };
        text.push_str(": ");
        text.push_str(&inner.to_string());
        source = inner.source();
    }
    excerpt(&text)
}

/// Bound a string at [`MAX_ERROR_DETAIL_BYTES`], on a character boundary.
fn excerpt(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return "no body".to_owned();
    }
    if trimmed.len() <= MAX_ERROR_DETAIL_BYTES {
        return trimmed.to_owned();
    }
    let mut end = MAX_ERROR_DETAIL_BYTES;
    while end > 0 && !trimmed.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… (truncated)", &trimmed[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excerpt_bounds_a_long_body_on_a_char_boundary() {
        let body = "é".repeat(MAX_ERROR_DETAIL_BYTES);
        let out = excerpt(&body);
        assert!(out.ends_with("… (truncated)"));
        assert!(out.len() < body.len());
    }

    #[test]
    fn excerpt_names_an_empty_body_rather_than_printing_nothing() {
        assert_eq!(excerpt("   "), "no body");
    }

    #[test]
    fn a_401_display_names_neither_the_token_nor_the_header() {
        let err = BotsError::status(401, "POST /v1/chat/completions", "{\"error\":\"bad key\"}");
        let text = err.to_string();
        assert!(text.contains("401"));
        assert!(!text.to_ascii_lowercase().contains("authorization"));
        assert!(!text.contains("sk-secret"));
    }

    #[test]
    fn only_429_and_5xx_are_retryable_statuses() {
        assert!(BotsError::status(429, "POST /x", "").is_retryable());
        assert!(BotsError::status(503, "POST /x", "").is_retryable());
        assert!(!BotsError::status(401, "POST /x", "").is_retryable());
        assert!(!BotsError::status(404, "POST /x", "").is_retryable());
        assert!(!BotsError::Protocol {
            detail: "cut".to_owned()
        }
        .is_retryable());
    }
}
