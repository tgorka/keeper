//! The stable codes a `jobs.*` command is refused with. The server answers
//! them as the `operation.forbidden` message (or a receipt's `code`); the
//! vectors pin each by its string.

use serde::{Deserialize, Serialize};

/// Why a command was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    /// The envelope or payload does not have the contract's shape.
    InvalidRequest,
    /// The envelope's `version` is not [`crate::CONTRACT_VERSION`].
    UnsupportedVersion,
    /// The same request id was already bound to another digest.
    IdempotencyMismatch,
    /// The encoded command is longer than `jobRowMaxBytes`.
    RowTooLarge,
    /// An inline `input` or `result` is longer than `jobInlineMaxBytes`.
    InlineTooLarge,
    /// A note or a failure's sentence is longer than `jobNoteMaxBytes`.
    NoteTooLarge,
    /// More references than `jobRefsMax`.
    TooManyRefs,
    /// More effects than `jobEffectsMax`.
    TooManyEffects,
    /// A plan blob larger than `jobPlanMaxBytes`.
    PlanTooLarge,
    /// A lease outside `jobLeaseMinMs..=jobLeaseMaxMs`.
    LeaseOutOfRange,
    /// `max_attempts` outside `1..=jobAttemptsMax`.
    AttemptsOutOfRange,
    /// The domain already holds `jobOpenMaxPerDomain` jobs not yet ended.
    TooManyOpen,
    /// A job with this id already exists.
    JobExists,
    /// No such job.
    UnknownJob,
    /// The job has ended; a terminal state never changes.
    Terminal,
    /// The job is held by a live attempt of another client.
    Conflict,
    /// The job is not due yet (`not_before_ms`).
    NotDue,
    /// Every attempt has been used.
    AttemptsExhausted,
    /// The command names an attempt that is not the job's current one.
    StaleAttempt,
    /// The job already has a plan; a second is refused.
    PlanExists,
    /// The plan is bound to input revisions the job no longer has.
    InputMoved,
    /// An effect names another job, or repeats an ordinal.
    InvalidEffect,
}

impl Refusal {
    /// The stable code.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::UnsupportedVersion => "unsupported_version",
            Self::IdempotencyMismatch => "idempotency_mismatch",
            Self::RowTooLarge => "row_too_large",
            Self::InlineTooLarge => "inline_too_large",
            Self::NoteTooLarge => "note_too_large",
            Self::TooManyRefs => "too_many_refs",
            Self::TooManyEffects => "too_many_effects",
            Self::PlanTooLarge => "plan_too_large",
            Self::LeaseOutOfRange => "lease_out_of_range",
            Self::AttemptsOutOfRange => "attempts_out_of_range",
            Self::TooManyOpen => "too_many_open",
            Self::JobExists => "job_exists",
            Self::UnknownJob => "unknown_job",
            Self::Terminal => "terminal",
            Self::Conflict => "conflict",
            Self::NotDue => "not_due",
            Self::AttemptsExhausted => "attempts_exhausted",
            Self::StaleAttempt => "stale_attempt",
            Self::PlanExists => "plan_exists",
            Self::InputMoved => "input_moved",
            Self::InvalidEffect => "invalid_effect",
        }
    }

    /// Every code, in declaration order.
    pub const ALL: [Self; 22] = [
        Self::InvalidRequest,
        Self::UnsupportedVersion,
        Self::IdempotencyMismatch,
        Self::RowTooLarge,
        Self::InlineTooLarge,
        Self::NoteTooLarge,
        Self::TooManyRefs,
        Self::TooManyEffects,
        Self::PlanTooLarge,
        Self::LeaseOutOfRange,
        Self::AttemptsOutOfRange,
        Self::TooManyOpen,
        Self::JobExists,
        Self::UnknownJob,
        Self::Terminal,
        Self::Conflict,
        Self::NotDue,
        Self::AttemptsExhausted,
        Self::StaleAttempt,
        Self::PlanExists,
        Self::InputMoved,
        Self::InvalidEffect,
    ];
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for Refusal {}
