//! The contract's closed word sets, `syncular/vocabulary.json`, compiled
//! into keeper (schema 4). The server reads the same file, so the two sides
//! cannot disagree on them:
//!
//! - [`efforts`]: the reasoning efforts a session's settings may name
//!   (`session_settings.effort`, `agent_profiles.efforts`, V16);
//! - [`grant_tiers`]: the tiers a standing grant may cover (`standing_grants
//!   .max_tier`, V8, V15) — never `T4`, which another device decides every
//!   time;
//! - [`handle`]: a session's handle, three words drawn from the file's lists
//!   by the SHA-256 of the session id (the contract's *Session handles*).
//!   A handle is display only: never a key, a scope value or a lookup path;
//! - [`person_agent_key`]: the key of one person's standing grants about one
//!   agent (`standing_grants.person_agent_key`, the `person_agent` scope),
//!   checked against the file's `personAgentKey.examples`.

use std::sync::LazyLock;

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The contract's word sets, as the repository holds them.
const VOCABULARY: &str = include_str!("../../../../../syncular/vocabulary.json");

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Vocabulary {
    efforts: Vec<String>,
    grant_tiers: Vec<String>,
    handle: HandleWords,
    person_agent_key: PersonAgentWords,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersonAgentWords {
    examples: Vec<PersonAgentExample>,
}

/// A person, an agent, and the person-agent key the contract says they have.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersonAgentExample {
    pub person: String,
    pub agent: String,
    pub key: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HandleWords {
    hash: String,
    adjectives: Vec<String>,
    nouns: Vec<String>,
    verbs: Vec<String>,
    examples: Vec<HandleExample>,
}

/// A session id and the handle the contract says it has.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HandleExample {
    pub session_id: String,
    pub handle: String,
}

/// The file, parsed once. Malformed, every reader below answers nothing
/// (and `vocabulary_tests` fails the build's tests).
static PARSED: LazyLock<Option<Vocabulary>> =
    LazyLock::new(|| serde_json::from_str(VOCABULARY).ok());

/// The reasoning efforts a session may name, in the contract's order.
pub fn efforts() -> &'static [String] {
    PARSED.as_ref().map_or(&[], |v| v.efforts.as_slice())
}

/// Whether `effort` is one the contract names.
pub fn is_effort(effort: &str) -> bool {
    efforts().iter().any(|known| known == effort)
}

/// The tiers a standing grant may cover, in the contract's order.
pub fn grant_tiers() -> &'static [String] {
    PARSED.as_ref().map_or(&[], |v| v.grant_tiers.as_slice())
}

/// The contract's examples: each session id with the handle it must have.
pub fn handle_examples() -> &'static [HandleExample] {
    PARSED
        .as_ref()
        .map_or(&[], |v| v.handle.examples.as_slice())
}

/// The contract's examples: each (person, agent) with its person-agent key.
pub fn person_agent_examples() -> &'static [PersonAgentExample] {
    PARSED
        .as_ref()
        .map_or(&[], |v| v.person_agent_key.examples.as_slice())
}

/// The person-agent key of `person` and `agent`: `<n>:<person>/<agent>`, `n`
/// the UTF-8 byte length of `person` in decimal. The length says where the
/// person ends, so no two (person, agent) pairs share a key, whatever the ids
/// hold — `tgorka` with `a/b` is `6:tgorka/a/b`, `tgorka/a` with `b` is
/// `8:tgorka/a/b`.
pub fn person_agent_key(person: &str, agent: &str) -> String {
    format!("{}:{person}/{agent}", person.len())
}

/// The word `digest[at..at + 2]` picks from `words`: a big-endian unsigned
/// 16-bit number modulo the list's length.
fn pick<'a>(words: &'a [String], digest: &[u8], at: usize) -> Option<&'a str> {
    let high = *digest.get(at)?;
    let low = *digest.get(at + 1)?;
    let index = usize::from(u16::from_be_bytes([high, low])) % words.len().max(1);
    words.get(index).map(String::as_str)
}

/// A session's handle, e.g. `noble-hedgehog-struts`: the adjective, the noun
/// and the verb picked by bytes 0–1, 2–3 and 4–5 of the SHA-256 of the
/// session id's UTF-8 bytes. `None` only if the compiled file is malformed.
pub fn handle(session_id: &str) -> Option<String> {
    let words = &PARSED.as_ref()?.handle;
    if words.hash != "sha256" {
        return None;
    }
    let digest = Sha256::digest(session_id.as_bytes());
    let adjective = pick(&words.adjectives, &digest, 0)?;
    let noun = pick(&words.nouns, &digest, 2)?;
    let verb = pick(&words.verbs, &digest, 4)?;
    Some(format!("{adjective}-{noun}-{verb}"))
}

#[cfg(test)]
#[path = "vocabulary_tests.rs"]
mod tests;
