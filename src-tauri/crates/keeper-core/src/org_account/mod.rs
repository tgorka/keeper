//! An optional organisation account (Epic 82): one OIDC sign-in whose token is a
//! general credential, plus a per-person config repository synced to every
//! device.
//!
//! Named `org_account` because [`crate::account`] is the Matrix account
//! supervisor. Everything here is platform-free: the descriptor and its store
//! ([`descriptor`]), the claim rules ([`claims`]), the repository layout plans
//! ([`layout`]), the synced preferences ([`settings_sync`]) and offers
//! ([`manifest`]), the status the UI renders ([`state`]), and the OIDC client
//! ([`oidc`], [`session`], [`loopback`]). The shell joins these to
//! `keeper-sync`, which moves the git bytes; this module never depends on it
//! (AD-40, AD-312).

pub mod claims;
pub mod descriptor;
pub mod device_state;
pub mod layout;
pub mod manifest;
pub mod settings_sync;
pub mod state;

pub mod loopback;
pub mod oidc;
pub mod session;

/// Why an account operation did not happen, sorted by what the person can do
/// about it. Every message is a finished, secret-free sentence.
#[derive(Debug, thiserror::Error)]
pub enum AccountError {
    /// The grant is dead, a refresh was rejected, or there is no session.
    #[error("{0}")]
    NeedsSignIn(String),
    /// Network, DNS, TLS or a timeout.
    #[error("{0}")]
    Unreachable(String),
    /// Policy: a missing required role, an unsafe username, a `sub` or
    /// username mismatch, a bad descriptor.
    #[error("{0}")]
    Refused(String),
    #[error("sign-in was cancelled")]
    Cancelled,
    #[error("{0}")]
    Internal(String),
}
