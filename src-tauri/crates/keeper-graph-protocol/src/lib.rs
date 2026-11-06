//! The knowledge graph's shared contract, version [`CONTRACT_VERSION`].
//!
//! What a native client and the server must compute identically: the fixed
//! vocabulary ([`vocabulary`]), the reversible query symbols ([`symbol`]),
//! canonical JSON values and typed references ([`value`]), request digests
//! ([`request`]), the `graph.apply` payload ([`change`]) and what it does to
//! a graph ([`candidate`]), event-operation digests ([`delta`]), the scope
//! and key values of the graph tables ([`scope`]) and the reads' shapes
//! ([`wire`]). `syncular/knowledge-graph.vectors.json` pins every rule here;
//! keeper-hq's TypeScript validator reproduces the same vectors.
//!
//! This crate owns no storage, transport or native code, and accepts nothing:
//! acceptance is the server's graph command.

pub mod candidate;
pub mod change;
pub mod delta;
pub mod request;
pub mod scope;
pub mod symbol;
pub mod value;
pub mod vocabulary;
pub mod wire;

use std::fmt;

/// The graph contract's version: a request envelope's `version`.
pub const CONTRACT_VERSION: u32 = 1;
/// The first Syncular schema version that holds the graph tables.
pub const SCHEMA_VERSION: u32 = 6;
/// The query profile whose symbols [`symbol`] maps.
pub const PROFILE: &str = "KG-Cypher/1";

/// Why the contract refuses an input: the stable error codes of the
/// contract's §7 that a contract check can produce.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    /// A contract version, schema version or wire shape this build does not
    /// implement.
    UnsupportedVersion,
    /// A value, name or operation the contract does not admit.
    InvalidChange,
    /// A size, count or nesting bound exceeded.
    ChangeLimit,
}

impl Refusal {
    /// The stable error code.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::UnsupportedVersion => "unsupported_version",
            Self::InvalidChange => "invalid_change",
            Self::ChangeLimit => "change_limit",
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for Refusal {}

/// Lowercase hex of a SHA-256 digest.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}

/// A ULID in its canonical Crockford base32 text: 26 characters, first at
/// most `7`, no lowercase.
#[must_use]
pub fn is_ulid(s: &str) -> bool {
    s.len() == 26
        && s.as_bytes()[0] <= b'7'
        && s.bytes()
            .all(|b| b"0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(&b))
}

/// A revision or graph sequence: a canonical non-negative decimal integer
/// (`0`, or no leading zero) of at most 20 digits.
#[must_use]
pub fn is_decimal(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 20
        && s.bytes().all(|b| b.is_ascii_digit())
        && (s == "0" || !s.starts_with('0'))
}
