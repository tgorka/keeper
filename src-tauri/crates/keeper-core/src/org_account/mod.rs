//! An optional organisation account (Epic 82): one OIDC sign-in whose token is a
//! general credential, plus a per-person config repository synced to every
//! device.
//!
//! Named `org_account` because [`crate::account`] is the Matrix account
//! supervisor. Everything here is platform-free: the descriptor and its store
//! ([`descriptor`]), the claim rules ([`claims`]), the repository layout plans
//! ([`layout`]), the synced preferences ([`settings_sync`]) and offers
//! ([`manifest`]), the status the UI renders ([`state`]), the setup link's QR
//! code ([`setup_qr`]), the OIDC client ([`oidc`], [`session`],
//! [`loopback`]), and the boundary a sign-in or sync run is cancelled at
//! ([`attempt`]). The shell joins these to `keeper-sync`, which moves the git
//! bytes; this module never depends on it (AD-40, AD-312).

pub mod claims;
pub mod descriptor;
pub mod device_state;
pub mod layout;
pub mod log_upload;
pub mod manifest;
pub mod settings_sync;
pub mod setup_qr;
pub mod state;

pub mod attempt;
pub mod loopback;
pub mod oidc;
pub mod session;

/// Why an account operation did not happen, sorted by what the person can do
/// about it. Every message is a finished, secret-free sentence.
///
/// A browser sign-in ends four ways besides success (Epic 100, R381): the
/// person closed it ([`AccountError::Cancelled`], quiet), the identity
/// provider refused it ([`AccountError::Denied`]), no answer came back in
/// time ([`AccountError::TimedOut`]), or the round trip broke
/// ([`AccountError::Interrupted`]). The last three are problems the person
/// is told about, by the account's name ([`state::problem_of`]).
#[derive(Debug, thiserror::Error)]
pub enum AccountError {
    /// The grant is dead, a refresh was rejected, or there is no session.
    #[error("{0}")]
    NeedsSignIn(String),
    /// Network, DNS, TLS or a timeout.
    #[error("{0}")]
    Unreachable(String),
    /// A sign-in's server did not answer within `oauth::REACH_TIMEOUT`
    /// before the browser opened; the sentence names its `host[:port]`
    /// (`oauth::no_answer`) and is what the person reads.
    #[error("{0}")]
    NoAnswer(String),
    /// Policy: a missing required role, an unsafe username, a `sub` or
    /// username mismatch, a bad descriptor.
    #[error("{0}")]
    Refused(String),
    /// The person closed the sign-in before it finished.
    #[error("sign-in was cancelled")]
    Cancelled,
    /// The identity provider answered `error=access_denied`: this account
    /// may not sign in to this application. `reason` is the provider's own
    /// `error_description`, bounded and on one line, when it sent one.
    #[error("{host} refused the sign-in.")]
    Denied {
        host: String,
        reason: Option<String>,
    },
    /// No answer came back from the browser before the flow's wait ran out.
    #[error("The sign-in timed out before the browser answered.")]
    TimedOut,
    /// The browser round trip ended without an answer: the sign-in sheet
    /// could not be shown, or failed, or the flow was dropped. The string is
    /// the platform's reason.
    #[error("The sign-in did not come back from the browser ({0}).")]
    Interrupted(String),
    #[error("{0}")]
    Internal(String),
}

impl AccountError {
    /// The closed name of this error's kind, for a log line: never a
    /// sentence that could carry a host's words, a token or a URL.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::NeedsSignIn(_) => "needs_sign_in",
            Self::Unreachable(_) => "unreachable",
            Self::NoAnswer(_) => "no_answer",
            Self::Refused(_) => "refused",
            Self::Cancelled => "closed",
            Self::Denied { .. } => "denied",
            Self::TimedOut => "timed_out",
            Self::Interrupted(_) => "interrupted",
            Self::Internal(_) => "internal",
        }
    }
}
