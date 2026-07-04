//! Platform port trait (AD-24).
//!
//! `keeper-core` reaches the OS only through this port. The concrete
//! implementation lives in the `keeper` shell crate, keeping the hexagon free
//! of any platform- or tauri-specific dependency. Story 1.1 wires the data-dir
//! port end-to-end; the remaining ports (keychain, notifier, sidecar) are
//! declared here and return [`CoreError::Unsupported`] from the shell impl for
//! now — honest and non-panicking — until later stories fill them.

use std::path::PathBuf;

use crate::error::CoreError;

/// Injected platform capabilities the core depends on.
///
/// Implementations must be `Send + Sync` so they can be shared across the
/// account-supervision tasks introduced in later stories.
pub trait Platform: Send + Sync {
    /// Root directory for keeper's own persistent data (databases, account
    /// stores). Fully wired in Story 1.1.
    fn data_dir(&self) -> Result<PathBuf, CoreError>;

    /// Store a secret (e.g. an access token) in the OS keychain under `key`.
    /// Wired in Story 1.3 via the shell's `DesktopPlatform` (macOS Keychain,
    /// service `dev.tgorka.keeper`, backed by `keyring`).
    fn keychain_set(&self, key: &str, value: &str) -> Result<(), CoreError>;

    /// Retrieve a secret previously stored under `key`, if present.
    /// Wired in Story 1.3 via the shell's `DesktopPlatform` (macOS Keychain).
    fn keychain_get(&self, key: &str) -> Result<Option<String>, CoreError>;

    /// Delete a secret stored under `key`.
    /// Wired in Story 1.3 via the shell's `DesktopPlatform` (macOS Keychain).
    fn keychain_delete(&self, key: &str) -> Result<(), CoreError>;

    /// Post a desktop notification (title + body).
    /// Not wired in Story 1.1 — returns [`CoreError::Unsupported`].
    fn notify(&self, title: &str, body: &str) -> Result<(), CoreError>;

    /// Resolve the path to a bundled sidecar binary by logical `name`.
    /// Not wired in Story 1.1 — returns [`CoreError::Unsupported`].
    fn sidecar_path(&self, name: &str) -> Result<PathBuf, CoreError>;
}
