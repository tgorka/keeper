//! The common request envelope of every graph command (contract §1).
//!
//! `{"version":1,"request_id":<ULID>,"context_id":<ULID>|null,"payload":{…}}`,
//! nothing else. A human device sends `context_id: null`; an agent sends the
//! server-issued context it was admitted under. The server's digest of a
//! request is the SHA-256 of the JCS bytes of
//! `{"context_id","descriptor","payload","version"}`: the request id, actor,
//! client and domain are its lookup key, not part of it.

use serde_json::{json, Value};

use crate::value::{canonical, parse as parse_value};
use crate::{is_ulid, Refusal, CONTRACT_VERSION};

/// The most canonical bytes a request's `payload` may have.
pub const MAX_PAYLOAD_BYTES: usize = 1024 * 1024;
/// The most bytes an encoded envelope may have before it is decoded at all.
pub const MAX_ENVELOPE_BYTES: usize = 2 * MAX_PAYLOAD_BYTES;

/// A decoded, checked request envelope.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub request_id: String,
    pub context_id: Option<String>,
    pub payload: Value,
}

/// Decode and check an envelope. A `version` other than
/// [`CONTRACT_VERSION`] refuses `unsupported_version` before anything else
/// is looked at.
pub fn parse(bytes: &[u8]) -> Result<Request, Refusal> {
    if bytes.len() > MAX_ENVELOPE_BYTES {
        return Err(Refusal::ChangeLimit);
    }
    let envelope = parse_value(bytes)?;
    let Some(fields) = envelope.as_object() else {
        return Err(Refusal::InvalidChange);
    };
    match fields.get("version").and_then(Value::as_u64) {
        Some(version) if version == u64::from(CONTRACT_VERSION) => {}
        Some(_) => return Err(Refusal::UnsupportedVersion),
        None => return Err(Refusal::InvalidChange),
    }
    if fields.len() != 4 {
        return Err(Refusal::InvalidChange);
    }
    let request_id = match fields.get("request_id").and_then(Value::as_str) {
        Some(id) if is_ulid(id) => id.to_owned(),
        _ => return Err(Refusal::InvalidChange),
    };
    let context_id = match fields.get("context_id") {
        Some(Value::Null) => None,
        Some(Value::String(id)) if is_ulid(id) => Some(id.clone()),
        _ => return Err(Refusal::InvalidChange),
    };
    let payload = match fields.get("payload") {
        Some(payload @ Value::Object(_)) => payload.clone(),
        _ => return Err(Refusal::InvalidChange),
    };
    if canonical(&payload)?.len() > MAX_PAYLOAD_BYTES {
        return Err(Refusal::ChangeLimit);
    }
    Ok(Request {
        request_id,
        context_id,
        payload,
    })
}

/// A graph command descriptor: `graph.` followed by a lowercase name.
#[must_use]
pub fn is_descriptor(descriptor: &str) -> bool {
    descriptor.len() <= 64
        && descriptor.strip_prefix("graph.").is_some_and(|name| {
            name.starts_with(|c: char| c.is_ascii_lowercase())
                && name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
        })
}

/// The server's digest of a request sent to `descriptor`.
pub fn digest(descriptor: &str, request: &Request) -> Result<String, Refusal> {
    if !is_descriptor(descriptor) {
        return Err(Refusal::InvalidChange);
    }
    crate::value::digest(&json!({
        "version": CONTRACT_VERSION,
        "descriptor": descriptor,
        "context_id": request.context_id,
        "payload": request.payload,
    }))
}
