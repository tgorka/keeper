//! An operation's error: one of nine codes, the module's stable reason and
//! a detail.

use std::fmt;

/// The error codes every door maps to (gRPC's names, where they share one).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    NotFound,
    InvalidArgument,
    Refused,
    Unavailable,
    Conflict,
    DeadlineExceeded,
    Cancelled,
    ResourceExhausted,
    Internal,
}

impl ErrorCode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotFound => "NOT_FOUND",
            Self::InvalidArgument => "INVALID_ARGUMENT",
            Self::Refused => "REFUSED",
            Self::Unavailable => "UNAVAILABLE",
            Self::Conflict => "CONFLICT",
            Self::DeadlineExceeded => "DEADLINE_EXCEEDED",
            Self::Cancelled => "CANCELLED",
            Self::ResourceExhausted => "RESOURCE_EXHAUSTED",
            Self::Internal => "INTERNAL",
        }
    }
}

/// An operation's error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpError {
    pub code: ErrorCode,
    /// The module's stable reason (e.g. `projection_unavailable`), or the
    /// field an `INVALID_ARGUMENT` names.
    pub reason: String,
    pub detail: String,
    /// Whether the same call may succeed later.
    pub retryable: bool,
}

impl OpError {
    #[must_use]
    pub fn new(code: ErrorCode, reason: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            code,
            reason: reason.into(),
            detail: detail.into(),
            retryable: matches!(
                code,
                ErrorCode::Unavailable | ErrorCode::DeadlineExceeded | ErrorCode::ResourceExhausted
            ),
        }
    }

    /// `INVALID_ARGUMENT`, naming `field` (a JSON path such as `target.domain`).
    #[must_use]
    pub fn invalid_argument(field: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidArgument, field, detail)
    }

    #[must_use]
    pub fn retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }
}

impl fmt::Display for OpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.code.as_str(), self.reason, self.detail)
    }
}

impl std::error::Error for OpError {}
