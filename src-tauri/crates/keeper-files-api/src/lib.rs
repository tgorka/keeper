//! The files contract (story 116.1): a drive's tree as rows every device of
//! the domain replicates (`syncular/README.md`, *The drive*).
//!
//! - [`ids`]: entry ids, content ids, drive keys, page keys, the locator.
//! - [`names`]: what a name may be, its stored fold, its media type, its mode.
//! - [`wire`]: the request envelope and the commands' payloads.
//! - [`rows`]: the synced rows — entries, events, operations, the head.
//! - [`caller`]: who sends a command, and an agent's or a tool's file part.
//! - [`refusal`]: the closed set of refusal codes.
//! - [`view`]: what a client shows, each naming its data by `(server, domain)`.
//! - [`rules`]: the reference rules — [`rules::evaluate`] judges a command
//!   against a manifest exactly as the server must, so every implementation
//!   reproduces `syncular/files.vectors.json` through it.
//!
//! The server enforces; this crate stores and sends nothing.

pub mod caller;
pub mod ids;
pub mod names;
pub mod refusal;
pub mod rows;
pub mod rules;
pub mod view;
pub mod wire;

use serde::{Deserialize, Serialize};

/// Where a manifest's data lives: one domain (a database) of one server.
/// The contract names no server; a client names the one it was given.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DomainRef {
    pub server: String,
    pub domain: String,
}

/// The most operations one change is charged (the bound, decision 17).
pub const CHANGE_LIMIT: usize = 128;

/// The most UTF-8 bytes one name holds.
pub const MAX_NAME_BYTES: usize = 255;

/// The most levels a walk up `parent_id` climbs before it stops.
pub const MAX_DEPTH: usize = 256;

/// The drive name the contract reserves (121.2's logs drive), refused to
/// every sender of a `files.*` command.
pub const RESERVED_DRIVE: &str = ".keeper-logs";
