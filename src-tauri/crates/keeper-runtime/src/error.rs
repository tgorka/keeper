//! Errors returned when a binary's module composition fails the start check.

use std::fmt;

use keeper_runtime_api::{Effect, OpError};

/// Why [`crate::Runtime::build`] refused to start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartError {
    DuplicateModule {
        module: String,
    },
    DuplicateOperation {
        module: String,
        id: String,
    },
    BadOperationId {
        module: String,
        id: String,
    },
    ForeignOperation {
        module: String,
        id: String,
    },
    UnboundOperation {
        module: String,
        id: String,
    },
    UnknownBinding {
        module: String,
        id: String,
    },
    DuplicateBinding {
        module: String,
        id: String,
    },
    ClassMismatch {
        module: String,
        id: String,
        declared: Effect,
        bound: Effect,
    },
    BadProposalForm {
        module: String,
        id: String,
        reason: String,
    },
    BadWireRule {
        module: String,
        id: String,
        pointer: String,
    },
    MissingRequired {
        module: String,
        lacks: String,
    },
    RequirementCycle {
        path: Vec<String>,
    },
    StartFailed {
        module: String,
        error: OpError,
    },
}

impl StartError {
    /// Stable snake_case error code.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::DuplicateModule { .. } => "duplicate_module",
            Self::DuplicateOperation { .. } => "duplicate_operation",
            Self::BadOperationId { .. } => "bad_operation_id",
            Self::ForeignOperation { .. } => "foreign_operation",
            Self::UnboundOperation { .. } => "unbound_operation",
            Self::UnknownBinding { .. } => "unknown_binding",
            Self::DuplicateBinding { .. } => "duplicate_binding",
            Self::ClassMismatch { .. } => "class_mismatch",
            Self::BadProposalForm { .. } => "bad_proposal_form",
            Self::BadWireRule { .. } => "bad_wire_rule",
            Self::MissingRequired { .. } => "missing_required",
            Self::RequirementCycle { .. } => "requirement_cycle",
            Self::StartFailed { .. } => "start_failed",
        }
    }
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateModule { module } => write!(f, "duplicate module '{module}'"),
            Self::DuplicateOperation { module, id } => {
                write!(f, "module '{module}' declares duplicate operation '{id}'")
            }
            Self::BadOperationId { module, id } => write!(
                f,
                "module '{module}' declares invalid operation id '{id}' (expected '<module>.<verb>')"
            ),
            Self::ForeignOperation { module, id } => {
                write!(f, "module '{module}' declares foreign operation '{id}'")
            }
            Self::UnboundOperation { module, id } => {
                write!(f, "module '{module}' has no handler for operation '{id}'")
            }
            Self::UnknownBinding { module, id } => {
                write!(f, "module '{module}' bound handler for unknown operation '{id}'")
            }
            Self::DuplicateBinding { module, id } => {
                write!(f, "module '{module}' bound duplicate handler for operation '{id}'")
            }
            Self::ClassMismatch {
                module,
                id,
                declared,
                bound,
            } => write!(
                f,
                "module '{module}' operation '{id}' declares effect '{}' but bound handler is '{}'",
                declared.as_str(),
                bound.as_str()
            ),
            Self::BadProposalForm { module, id, reason } => {
                write!(f, "module '{module}' operation '{id}' has invalid proposal form: {reason}")
            }
            Self::BadWireRule {
                module,
                id,
                pointer,
            } => write!(
                f,
                "module '{module}' operation '{id}' has wire rule pointer '{pointer}' not in input schema"
            ),
            Self::MissingRequired { module, lacks } => {
                write!(f, "{module} needs {lacks}, which this build does not include")
            }
            Self::RequirementCycle { path } => {
                write!(f, "requirement cycle: {}", path.join(" -> "))
            }
            Self::StartFailed { module, error } => {
                write!(f, "module '{module}' failed to start: {error}")
            }
        }
    }
}

impl std::error::Error for StartError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::StartFailed { error, .. } => Some(error),
            _ => None,
        }
    }
}
