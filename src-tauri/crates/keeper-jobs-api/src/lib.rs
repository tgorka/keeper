//! The jobs contract (story 122.1), version [`CONTRACT_VERSION`].
//!
//! A job is a row of the domain it belongs to (`jobs`), with one claim
//! (`job_claims`) the server fences, and a client's declaration of what it
//! can run (`job_capabilities`). Both change only through the server's
//! `jobs.*` commands ([`command`]), each under KG1's request envelope.
//! A kind is a namespaced string this contract never enumerates: kinds are
//! closed in keeper-hq's registry, and the shape here is open to every kind
//! (DF/AD-4). A plan's content is the kind's and is opaque here.
//!
//! What a client and the server compute identically lives here: the types
//! ([`model`]), the page keys and the move of an ended job to its month
//! ([`page`]), the envelope, its bounds and digest ([`command`]), the
//! refusal codes ([`refusal`]) and the reference judgement of a command
//! against a job and its claim, including when an attempt has lapsed
//! ([`transition`]). `syncular/jobs.vectors.json` pins every rule; keeper-hq's
//! server tests read the same vectors. The server is the judge: this crate
//! stores, sends and accepts nothing.

pub mod command;
pub mod model;
pub mod page;
pub mod refusal;
pub mod transition;

pub use refusal::Refusal;

/// The envelope version every `jobs.*` command carries.
pub const CONTRACT_VERSION: u32 = 1;

/// The contract's bounds, `syncular/limits.json`, compiled in.
const LIMITS_JSON: &str = include_str!("../../../../syncular/limits.json");

/// The generic job bounds of `syncular/limits.json`. keeper-hq compiles the
/// same file into `hq-limits`; no module writes these numbers again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The most bytes an encoded command envelope (and so a row it writes) may have.
    pub row_max_bytes: u64,
    /// The most canonical bytes an inline `input` or `result` may have.
    pub inline_max_bytes: u64,
    /// The most bytes of a progress note, or of a failure's sentence.
    pub note_max_bytes: u64,
    /// The most references one job's input may name.
    pub refs_max: u64,
    /// The most effects one completion or failure may report.
    pub effects_max: u64,
    /// The most bytes a plan blob may have.
    pub plan_max_bytes: u64,
    /// The most jobs not yet ended one domain may hold.
    pub open_max_per_domain: u64,
    /// The highest `max_attempts` a job may ask for.
    pub attempts_max: u64,
    /// The shortest lease a claim may ask for.
    pub lease_min_ms: u64,
    /// The longest lease a claim may ask for.
    pub lease_max_ms: u64,
}

/// The bounds this build was compiled with. A malformed `limits.json` fails the build.
pub const LIMITS: Limits = Limits {
    row_max_bytes: contract_number(LIMITS_JSON, b"\"jobRowMaxBytes\""),
    inline_max_bytes: contract_number(LIMITS_JSON, b"\"jobInlineMaxBytes\""),
    note_max_bytes: contract_number(LIMITS_JSON, b"\"jobNoteMaxBytes\""),
    refs_max: contract_number(LIMITS_JSON, b"\"jobRefsMax\""),
    effects_max: contract_number(LIMITS_JSON, b"\"jobEffectsMax\""),
    plan_max_bytes: contract_number(LIMITS_JSON, b"\"jobPlanMaxBytes\""),
    open_max_per_domain: contract_number(LIMITS_JSON, b"\"jobOpenMaxPerDomain\""),
    attempts_max: contract_number(LIMITS_JSON, b"\"jobAttemptsMax\""),
    lease_min_ms: contract_number(LIMITS_JSON, b"\"jobLeaseMinMs\""),
    lease_max_ms: contract_number(LIMITS_JSON, b"\"jobLeaseMaxMs\""),
};

/// The whole number `key` (with its quotes) names in the JSON `text`.
const fn contract_number(text: &str, key: &[u8]) -> u64 {
    let text = text.as_bytes();
    let mut at = 0;
    while at + key.len() <= text.len() {
        let mut matched = 0;
        while matched < key.len() && text[at + matched] == key[matched] {
            matched += 1;
        }
        if matched == key.len() {
            let mut i = at + matched;
            while i < text.len() && matches!(text[i], b' ' | b'\t' | b'\r' | b'\n' | b':') {
                i += 1;
            }
            let mut value: u64 = 0;
            let mut digits = 0;
            while i < text.len() && text[i].is_ascii_digit() {
                value = value * 10 + (text[i] - b'0') as u64;
                i += 1;
                digits += 1;
            }
            assert!(digits > 0, "syncular/limits.json: the key holds no number");
            return value;
        }
        at += 1;
    }
    panic!("syncular/limits.json does not name the key")
}

/// A ULID in Crockford base32, upper case, as the server mints them.
#[must_use]
pub fn is_ulid(s: &str) -> bool {
    s.len() == 26
        && s.as_bytes()[0] <= b'7'
        && s.bytes()
            .all(|b| b"0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(&b))
}

/// A lowercase name: `[a-z][a-z0-9_-]*`, at most `max` bytes.
fn is_name(s: &str, max: usize) -> bool {
    !s.is_empty()
        && s.len() <= max
        && s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}
/// A kind: a namespaced name, `<namespace>.<name>` with any further
/// `.<name>` segments (`files.extract`), every segment a lowercase name, at
/// most 64 bytes in all. The contract checks the grammar only; which kinds
/// exist is the server's registry (DF/AD-4).
#[must_use]
pub fn is_kind(s: &str) -> bool {
    s.len() <= 64 && s.split('.').count() >= 2 && s.split('.').all(|part| is_name(part, 64))
}

/// A capability a client declares and a `node:<capability>` job asks for:
/// a lowercase name of at most 32 bytes.
#[must_use]
pub fn is_capability(s: &str) -> bool {
    is_name(s, 32)
}

/// The SHA-256 of a value's RFC 8785 (JCS) bytes, `sha256:<hex>`.
pub fn digest(value: &serde_json::Value) -> Result<String, Refusal> {
    use sha2::{Digest, Sha256};
    let bytes = canonical(value)?;
    let hash = Sha256::digest(&bytes);
    let mut out = String::with_capacity(71);
    out.push_str("sha256:");
    for byte in hash {
        out.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        out.push(char::from(b"0123456789abcdef"[usize::from(byte & 15)]));
    }
    Ok(out)
}

/// A value's RFC 8785 (JCS) bytes.
pub fn canonical(value: &serde_json::Value) -> Result<Vec<u8>, Refusal> {
    serde_jcs::to_vec(value).map_err(|_| Refusal::InvalidRequest)
}
