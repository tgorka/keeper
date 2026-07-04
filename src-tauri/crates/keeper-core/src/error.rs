//! Error root for the keeper hexagon (AD-21).
//!
//! Per-module `thiserror` enums roll up into the [`CoreError`] root. The Tauri
//! shell maps `CoreError` to the IPC `IpcError` envelope exactly once, in its
//! command layer — no module below the shell constructs an `IpcError` directly.

use thiserror::Error;

/// Errors originating in a [`crate::platform::Platform`] port implementation.
///
/// This is the first module-level enum rolling up into [`CoreError`]; later
/// stories add sibling enums (sync, send, store, …) that roll up the same way.
#[derive(Debug, Error)]
pub enum PlatformError {
    /// A platform capability is not available on this host / build.
    #[error("platform capability unsupported: {0}")]
    Unsupported(String),

    /// A required directory could not be resolved on this platform.
    #[error("could not resolve platform directory: {0}")]
    DirUnavailable(String),
}

/// The hexagon error root. Every fallible core operation surfaces one of these.
#[derive(Debug, Error)]
pub enum CoreError {
    /// A platform port failed or is unavailable.
    #[error(transparent)]
    Platform(#[from] PlatformError),

    /// A requested capability is not supported on this platform/build. Honest,
    /// non-panicking signal used by not-yet-wired [`crate::platform::Platform`]
    /// ports.
    #[error("unsupported: {0}")]
    Unsupported(String),

    /// An unexpected internal invariant violation.
    #[error("internal error: {0}")]
    Internal(String),
}
