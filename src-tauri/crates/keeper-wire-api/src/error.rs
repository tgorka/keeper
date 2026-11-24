//! keeper's fixed sentences (MA/AD-3 *Errors*, m1). A wire caller's message is
//! chosen here from the code and the module's error kind, never copied from
//! the module's own text, which can carry an address or a path: that text is
//! logged, never sent.

use crate::{Code, WireError};

/// The fixed sentence for `code` and the module's error `kind`. An unknown
/// kind gets the code's own sentence; nothing of `kind` is ever returned.
#[must_use]
pub fn sentence(code: Code, kind: &str) -> &'static str {
    match (code, kind) {
        (Code::ResourceExhausted, "too_large") => "the answer is too large; ask for a page",
        (Code::ResourceExhausted, "fell_behind") => "you fell behind; open the stream again",
        (Code::ResourceExhausted, "line_too_long") => "the line is longer than the limit",
        (Code::ResourceExhausted, "connections") => "too many connections to keeper's socket",
        (Code::Unavailable, "switched_off") => "keeper's wire was switched off",
        (Code::Unavailable, "connection_ended") => {
            "the connection ended before the answer; ask again with the same id"
        }
        (Code::Unavailable, "projection_unavailable") => {
            "the graph's projection is not available here"
        }
        (Code::InvalidArgument, "version") => "no envelope version in common",
        (Code::InvalidArgument, "not_hello") => "the first line must be hello",
        (Code::InvalidArgument, "second_hello") => "hello was already said",
        (Code::InvalidArgument, "call_id_in_flight") => "that call_id is in flight",
        (Code::InvalidArgument, "framing") => "the line is not a frame of keeper.wire.v1",
        (Code::Cancelled, "not_started") => "cancelled; nothing was done",
        (Code::Cancelled, "started") => "cancelled; the command runs to its end",
        (Code::DeadlineExceeded, "not_started") => "the deadline passed; nothing was done",
        (Code::DeadlineExceeded, "started") => {
            "the deadline passed; the command runs to its end; ask again with its id"
        }
        (Code::NotFound, _) => "no such operation, domain or entity",
        (Code::InvalidArgument, _) => "the input or the frame is not valid",
        (Code::Refused, _) => "this device's settings refuse it",
        (Code::Unavailable, _) => "not available now",
        (Code::Conflict, _) => "a precondition moved",
        (Code::DeadlineExceeded, _) => "the deadline passed",
        (Code::Cancelled, _) => "cancelled",
        (Code::ResourceExhausted, _) => "too many calls in flight",
        (Code::Internal, _) => "keeper failed",
    }
}

impl WireError {
    /// An error with keeper's fixed sentence for `code` and `kind`.
    #[must_use]
    pub fn fixed(code: Code, kind: &str) -> Self {
        Self {
            code,
            message: sentence(code, kind).to_owned(),
            retryable: matches!(
                code,
                Code::Unavailable | Code::DeadlineExceeded | Code::ResourceExhausted
            ),
            field: None,
            request_id: None,
        }
    }

    #[must_use]
    pub fn with_field(mut self, field: impl Into<String>) -> Self {
        self.field = Some(field.into());
        self
    }

    #[must_use]
    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }

    #[must_use]
    pub fn retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }

    /// What a client reports for a call whose connection ended before its
    /// answer, whatever a `close` frame said (MA/AD-3 *Errors*).
    #[must_use]
    pub fn connection_ended(call_id: &str) -> Self {
        Self::fixed(Code::Unavailable, "connection_ended")
            .retryable(true)
            .with_request_id(call_id)
    }
}
