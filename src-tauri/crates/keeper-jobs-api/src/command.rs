//! The `jobs.*` commands: descriptors, KG1's request envelope, the payloads,
//! the contract's own bound check, the request digest and the receipt.
//!
//! `params` is `{"version": 1, "request_id": <ULID>, "context_id": <ULID> |
//! null, "payload": {…}}` and nothing else (KG1 contract §1). The digest is
//! the SHA-256 of the JCS bytes of `{"version", "descriptor", "context_id",
//! "payload"}`; a request id once bound to a digest stays bound, and a lost
//! answer is resolved by asking again under the same id.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::model::{AttemptId, BlobRef, Effect, JobRef, RunsOn};
use crate::{canonical, is_capability, is_kind, is_ulid, Limits, Refusal, CONTRACT_VERSION};

/// A `jobs.*` command.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Descriptor {
    #[serde(rename = "jobs.submit")]
    Submit,
    #[serde(rename = "jobs.cancel")]
    Cancel,
    #[serde(rename = "jobs.claim")]
    Claim,
    #[serde(rename = "jobs.renew")]
    Renew,
    #[serde(rename = "jobs.release")]
    Release,
    #[serde(rename = "jobs.progress")]
    Progress,
    #[serde(rename = "jobs.complete")]
    Complete,
    #[serde(rename = "jobs.fail")]
    Fail,
    #[serde(rename = "jobs.declare_capability")]
    DeclareCapability,
}

impl Descriptor {
    /// Every descriptor, in the contract's order.
    pub const ALL: [Self; 9] = [
        Self::Submit,
        Self::Cancel,
        Self::Claim,
        Self::Renew,
        Self::Release,
        Self::Progress,
        Self::Complete,
        Self::Fail,
        Self::DeclareCapability,
    ];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Submit => "jobs.submit",
            Self::Cancel => "jobs.cancel",
            Self::Claim => "jobs.claim",
            Self::Renew => "jobs.renew",
            Self::Release => "jobs.release",
            Self::Progress => "jobs.progress",
            Self::Complete => "jobs.complete",
            Self::Fail => "jobs.fail",
            Self::DeclareCapability => "jobs.declare_capability",
        }
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.as_str() == text)
    }
}

/// A job's inline input: the references it works on and its kind's small
/// parameters (an object the contract does not read).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobInput {
    pub refs: Vec<JobRef>,
    pub params: serde_json::Map<String, Value>,
}

/// `jobs.submit`: a person, an agent or a tool asks for a job.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Submit {
    pub job_id: String,
    pub kind: String,
    #[serde(default)]
    pub input: Option<JobInput>,
    #[serde(default)]
    pub input_blob: Option<BlobRef>,
    pub runs_on: RunsOn,
    pub max_attempts: u32,
    #[serde(default)]
    pub not_before_ms: Option<u64>,
}

/// `jobs.cancel`: the submitter ends a job that has not ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cancel {
    pub job_id: String,
}

/// `jobs.claim`: a worker acquires a job for `lease_ms`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    pub job_id: String,
    pub lease_ms: u64,
}

/// `jobs.renew` and `jobs.release`: the current attempt keeps or gives back its claim.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttemptRef {
    pub job_id: String,
    pub attempt_id: AttemptId,
}

/// `jobs.progress`: a bounded note and, once, the job's plan, bound to the
/// input it was made from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Progress {
    pub job_id: String,
    pub attempt_id: AttemptId,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub plan_blob: Option<BlobRef>,
    /// The job's `input_digest` the plan was made from.
    pub input_digest: String,
}

/// `jobs.complete`: the current attempt ends the job `succeeded`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Complete {
    pub job_id: String,
    pub attempt_id: AttemptId,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub result_blob: Option<BlobRef>,
    pub effects: Vec<Effect>,
}

/// `jobs.fail`: the current attempt reports a failure. A retryable one
/// with attempts left queues the job again; otherwise it ends `failed`
/// (not retryable) or `dead` (no attempt left).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fail {
    pub job_id: String,
    pub attempt_id: AttemptId,
    pub error_code: String,
    pub sentence: String,
    pub retryable: bool,
    #[serde(default)]
    pub effects: Vec<Effect>,
}

/// `jobs.declare_capability`: a client says what it can run (122.5).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclareCapability {
    pub capabilities: Vec<String>,
}

/// The most capabilities one client may declare.
pub const MAX_CAPABILITIES: usize = 32;

/// A checked payload.
#[derive(Clone, Debug, PartialEq)]
pub enum Payload {
    Submit(Submit),
    Cancel(Cancel),
    Claim(Claim),
    Renew(AttemptRef),
    Release(AttemptRef),
    Progress(Progress),
    Complete(Complete),
    Fail(Fail),
    DeclareCapability(DeclareCapability),
}

impl Payload {
    /// The job the payload names; `None` for a capability declaration.
    #[must_use]
    pub fn job_id(&self) -> Option<&str> {
        match self {
            Self::Submit(p) => Some(&p.job_id),
            Self::Cancel(p) => Some(&p.job_id),
            Self::Claim(p) => Some(&p.job_id),
            Self::Renew(p) | Self::Release(p) => Some(&p.job_id),
            Self::Progress(p) => Some(&p.job_id),
            Self::Complete(p) => Some(&p.job_id),
            Self::Fail(p) => Some(&p.job_id),
            Self::DeclareCapability(_) => None,
        }
    }
}

/// A decoded, checked command.
#[derive(Clone, Debug, PartialEq)]
pub struct Command {
    pub descriptor: Descriptor,
    pub request_id: String,
    pub context_id: Option<String>,
    /// The payload as sent, for the digest.
    pub raw_payload: Value,
    pub payload: Payload,
}

impl Command {
    /// The server's digest of this request.
    pub fn digest(&self) -> Result<String, Refusal> {
        crate::digest(&json!({
            "version": CONTRACT_VERSION,
            "descriptor": self.descriptor.as_str(),
            "context_id": self.context_id,
            "payload": self.raw_payload,
        }))
    }
}

/// Decodes `bytes` as the envelope of a `descriptor` command and checks it
/// against the contract's shape and `limits`. The order is fixed, so every
/// implementation answers the same code: the encoded size, then the
/// envelope (`unsupported_version` before anything else in it), then the
/// payload's shape, then each bound.
pub fn check(descriptor: Descriptor, bytes: &[u8], limits: &Limits) -> Result<Command, Refusal> {
    if bytes.len() as u64 > limits.row_max_bytes {
        return Err(Refusal::RowTooLarge);
    }
    let envelope: Value = serde_json::from_slice(bytes).map_err(|_| Refusal::InvalidRequest)?;
    let Some(fields) = envelope.as_object() else {
        return Err(Refusal::InvalidRequest);
    };
    match fields.get("version").and_then(Value::as_u64) {
        Some(version) if version == u64::from(CONTRACT_VERSION) => {}
        Some(_) => return Err(Refusal::UnsupportedVersion),
        None => return Err(Refusal::InvalidRequest),
    }
    if fields.len() != 4 {
        return Err(Refusal::InvalidRequest);
    }
    let request_id = match fields.get("request_id").and_then(Value::as_str) {
        Some(id) if is_ulid(id) => id.to_owned(),
        _ => return Err(Refusal::InvalidRequest),
    };
    let context_id = match fields.get("context_id") {
        Some(Value::Null) => None,
        Some(Value::String(id)) if is_ulid(id) => Some(id.clone()),
        _ => return Err(Refusal::InvalidRequest),
    };
    let raw_payload = match fields.get("payload") {
        Some(payload @ Value::Object(_)) => payload.clone(),
        _ => return Err(Refusal::InvalidRequest),
    };
    let payload = decode(descriptor, &raw_payload)?;
    bounds(&payload, limits)?;
    Ok(Command {
        descriptor,
        request_id,
        context_id,
        raw_payload,
        payload,
    })
}

fn decode(descriptor: Descriptor, payload: &Value) -> Result<Payload, Refusal> {
    fn de<T: serde::de::DeserializeOwned>(payload: &Value) -> Result<T, Refusal> {
        serde_json::from_value(payload.clone()).map_err(|_| Refusal::InvalidRequest)
    }
    Ok(match descriptor {
        Descriptor::Submit => Payload::Submit(de(payload)?),
        Descriptor::Cancel => Payload::Cancel(de(payload)?),
        Descriptor::Claim => Payload::Claim(de(payload)?),
        Descriptor::Renew => Payload::Renew(de(payload)?),
        Descriptor::Release => Payload::Release(de(payload)?),
        Descriptor::Progress => Payload::Progress(de(payload)?),
        Descriptor::Complete => Payload::Complete(de(payload)?),
        Descriptor::Fail => Payload::Fail(de(payload)?),
        Descriptor::DeclareCapability => Payload::DeclareCapability(de(payload)?),
    })
}

fn inline_len(value: &Value) -> Result<u64, Refusal> {
    Ok(canonical(value)?.len() as u64)
}

fn text_ok(text: &str, limits: &Limits) -> Result<(), Refusal> {
    if text.len() as u64 > limits.note_max_bytes {
        Err(Refusal::NoteTooLarge)
    } else {
        Ok(())
    }
}

fn blob_ok(blob: Option<&BlobRef>) -> Result<(), Refusal> {
    match blob {
        Some(blob) if !blob.is_valid() => Err(Refusal::InvalidRequest),
        _ => Ok(()),
    }
}

fn effects_ok(job_id: &str, effects: &[Effect], limits: &Limits) -> Result<(), Refusal> {
    if effects.len() as u64 > limits.effects_max {
        return Err(Refusal::TooManyEffects);
    }
    let mut seen = std::collections::BTreeSet::new();
    for effect in effects {
        if effect.id.job_id != job_id || !seen.insert(effect.id.ordinal) {
            return Err(Refusal::InvalidEffect);
        }
    }
    Ok(())
}

fn job_id_ok(job_id: &str) -> Result<(), Refusal> {
    if is_ulid(job_id) {
        Ok(())
    } else {
        Err(Refusal::InvalidRequest)
    }
}

/// The contract's bounds on a decoded payload (122.1-AC2).
fn bounds(payload: &Payload, limits: &Limits) -> Result<(), Refusal> {
    if let Some(job_id) = payload.job_id() {
        job_id_ok(job_id)?;
    }
    match payload {
        Payload::Submit(p) => {
            if !is_kind(&p.kind) || (p.input.is_some() && p.input_blob.is_some()) {
                return Err(Refusal::InvalidRequest);
            }
            blob_ok(p.input_blob.as_ref())?;
            if let Some(input) = &p.input {
                if input.refs.len() as u64 > limits.refs_max {
                    return Err(Refusal::TooManyRefs);
                }
                let value = serde_json::to_value(input).map_err(|_| Refusal::InvalidRequest)?;
                if inline_len(&value)? > limits.inline_max_bytes {
                    return Err(Refusal::InlineTooLarge);
                }
            }
            if p.max_attempts == 0 || u64::from(p.max_attempts) > limits.attempts_max {
                return Err(Refusal::AttemptsOutOfRange);
            }
        }
        Payload::Cancel(_) | Payload::Renew(_) | Payload::Release(_) => {}
        Payload::Claim(p) => {
            if !(limits.lease_min_ms..=limits.lease_max_ms).contains(&p.lease_ms) {
                return Err(Refusal::LeaseOutOfRange);
            }
        }
        Payload::Progress(p) => {
            if let Some(note) = &p.note {
                text_ok(note, limits)?;
            }
            blob_ok(p.plan_blob.as_ref())?;
            if p.plan_blob
                .as_ref()
                .is_some_and(|plan| plan.byte_length > limits.plan_max_bytes)
            {
                return Err(Refusal::PlanTooLarge);
            }
        }
        Payload::Complete(p) => {
            if p.result.is_some() && p.result_blob.is_some() {
                return Err(Refusal::InvalidRequest);
            }
            blob_ok(p.result_blob.as_ref())?;
            if let Some(result) = &p.result {
                if inline_len(result)? > limits.inline_max_bytes {
                    return Err(Refusal::InlineTooLarge);
                }
            }
            effects_ok(&p.job_id, &p.effects, limits)?;
        }
        Payload::Fail(p) => {
            if !p.error_code.is_empty()
                && p.error_code.len() <= 64
                && p.error_code
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
            {
                text_ok(&p.sentence, limits)?;
            } else {
                return Err(Refusal::InvalidRequest);
            }
            effects_ok(&p.job_id, &p.effects, limits)?;
        }
        Payload::DeclareCapability(p) => {
            if p.capabilities.len() > MAX_CAPABILITIES
                || !p.capabilities.iter().all(|c| is_capability(c))
            {
                return Err(Refusal::InvalidRequest);
            }
        }
    }
    Ok(())
}

/// What a command's receipt says.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// The command was judged and its commit applied.
    Accepted,
    /// Another live attempt holds the job (a `jobs.claim` lost the race).
    Conflict,
    /// The command was judged and refused; nothing was written.
    Rejected,
}

/// A command's receipt. `jobs.claim`'s accepted receipt carries the new
/// `attempt_id` and `epoch`; every accepted one the commit sequence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub descriptor: Descriptor,
    pub request_id: String,
    pub request_digest: String,
    pub outcome: Outcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<Refusal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_seq: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_id: Option<AttemptId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epoch: Option<u64>,
}
