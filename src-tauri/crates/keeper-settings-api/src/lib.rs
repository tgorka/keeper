//! The settings contract: travel classes, record keys, credential guards, and
//! the pure decision rules for who may write a person's settings records and a
//! domain's shared configuration.
//!
//! `syncular/settings.travel.json`, `syncular/limits.json`, and
//! `syncular/settings.vectors.json` pin every rule here; keeper-hq's TypeScript
//! implements the same functions against the same vectors. This crate holds
//! types and pure rules only: it accepts, stores, and sends nothing.

pub mod adoption;
pub mod blob;
pub mod config;
pub mod credential;
pub mod record;
pub mod refusal;
pub mod scope;
pub mod travel;
pub mod write;

pub use refusal::Refusal;

use serde::{Deserialize, Serialize};

/// Compiled-in settings travel table from `syncular/settings.travel.json`.
pub const TRAVEL_JSON: &str = include_str!("../../../../syncular/settings.travel.json");

/// Compiled-in size bounds from `syncular/limits.json`.
pub const LIMITS_JSON: &str = include_str!("../../../../syncular/limits.json");

/// Byte limits shared between Rust and TypeScript for settings values and
/// uploaded blobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limits {
    /// Maximum UTF-8 byte length of an inline `settings_values.value` or
    /// `config_values.value` string before it must travel as a blob.
    pub setting_value_max_bytes: u64,
    /// Maximum byte length of an uploaded blob attachment.
    pub attachment_max_bytes: u64,
}

/// Parses the compiled-in [`LIMITS_JSON`] into [`Limits`].
///
/// Panics only if the compiled-in `syncular/limits.json` asset is malformed.
#[must_use]
#[allow(clippy::expect_used)]
pub fn limits() -> Limits {
    serde_json::from_str(LIMITS_JSON)
        .expect("compiled-in syncular/limits.json must parse as Limits")
}
