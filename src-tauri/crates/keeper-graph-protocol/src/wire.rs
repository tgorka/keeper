//! Synthetic accepted-prefix export and bounded worker frames for the isolated proof.
use crate::{Refusal, MAX_NUMBER};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
pub const MAX_EVENT_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_ROW_BYTES: usize = 128 * 1024;
pub const MAX_EXPORT_BYTES: u64 = 1024 * 1024 * 1024;
pub const MAX_EVENTS: u64 = 100_000;
pub const MAX_ENTITIES: u64 = 1_000_000;
pub const ENGINE: &str = "graphqlite-0.12.0-995a8b9bc25cdd86d4c2b50ff7ff87f0eb236629";
/// Fixed, non-secret allocator configuration the supervisor sets on every Linux worker
/// child (after `env_clear`, before exec); the worker refuses any other value. No mmapped
/// chunks (no page rounding, no persistent shrink after failed mremap), no tcache, and
/// (P1′) a trim threshold above the worker's 512 MiB `RLIMIT_AS`, so the main-arena heap
/// is never trimmed while that ceiling is in force.
pub const LINUX_WORKER_GLIBC_TUNABLES: &str =
    "glibc.malloc.mmap_max=0:glibc.malloc.tcache_count=0:glibc.malloc.trim_threshold=536870912";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub domain: String,
    pub authority_epoch: String,
    pub accepted_head: u64,
    pub sync_checkpoint: String,
    pub schema_version: u32,
    pub projection_version: u32,
    pub engine: String,
}
impl Manifest {
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.domain.is_empty()
            || self.domain.len() > 128
            || self.accepted_head > MAX_EVENTS
            || self.accepted_head == 0
            || self.schema_version != 1
            || self.projection_version != 1
            || self.engine != ENGINE
            || !decimal(&self.authority_epoch)
            || !decimal(&self.sync_checkpoint)
        {
            return Err(Refusal::QueryUnsupported);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub sequence: u64,
    pub operations: Vec<Operation>,
    pub digest: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    pub ordinal: u32,
    pub entity_type: EntityType,
    pub entity_id: String,
    pub operation: Change,
    pub before: Option<Value>,
    pub after: Option<Value>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EntityType {
    Node,
    Edge,
    Tag,
    NodeTag,
}
impl EntityType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Node => "node",
            Self::Edge => "edge",
            Self::Tag => "tag",
            Self::NodeTag => "node_tag",
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    Put,
    Remove,
}
impl Event {
    pub fn validate(&self, expected: u64) -> Result<(), Refusal> {
        if self.sequence != expected || self.operations.is_empty() || self.operations.len() > 128 {
            return Err(Refusal::QueryUnsupported);
        }
        for (ordinal, op) in self.operations.iter().enumerate() {
            if op.ordinal as usize != ordinal
                || !ulid(&op.entity_id)
                || (op.after.is_none() && op.entity_type != EntityType::NodeTag)
                || (op.operation == Change::Remove) != op.after.is_none()
            {
                return Err(Refusal::QueryUnsupported);
            }
            for value in [&op.before, &op.after].into_iter().flatten() {
                validate_value(value, 0)?;
                if serde_jcs::to_vec(value)
                    .map_err(|_| Refusal::QueryUnsupported)?
                    .len()
                    > MAX_ROW_BYTES
                {
                    return Err(Refusal::QueryLimit);
                }
            }
        }
        if operation_digest(&self.operations)? != self.digest {
            return Err(Refusal::QueryUnsupported);
        }
        Ok(())
    }
}
/// Event digest: SHA-256 of the JCS bytes of the operation array.
///
/// The bytes are produced by serializing the parsed operations straight through JCS
/// (no intermediate `serde_json::Value` copy; JCS canonicalizes member order).
/// Acceptance and typed-refusal precedence are unchanged: the whole-array tree
/// validation that `canonical_bytes` applied to `to_value(operations)` is reproduced
/// exactly, in the same order, by [`validate_operation_tree`], and the final
/// encoded-size cap still applies.
pub fn operation_digest(operations: &[Operation]) -> Result<String, Refusal> {
    if operations.len() > 128 {
        return Err(Refusal::QueryLimit);
    }
    validate_operation_tree(operations)?;
    let bytes = serde_jcs::to_vec(operations).map_err(|_| Refusal::QueryUnsupported)?;
    if bytes.len() > MAX_EVENT_BYTES {
        return Err(Refusal::QueryLimit);
    }
    let digest = Sha256::digest(bytes);
    use std::fmt::Write;
    let mut out = String::with_capacity(64);
    for byte in digest {
        write!(out, "{byte:02x}").map_err(|_| Refusal::QueryUnsupported)?;
    }
    Ok(out)
}
/// `validate_value(&to_value(operations), 0)` without building that tree, visiting what
/// that tree visits in the same order, so the first refusal is the same. The array is
/// depth 0 (≤ 8192 elements, in order). Each operation object is depth 1: six members
/// whose keys are short, visited in `serde_json::Map` (BTreeMap) key order — `after`,
/// `before`, `entity_id`, `entity_type`, `operation`, `ordinal` — each value at depth 2.
/// `after`/`before` recurse (absent is `null`, accepted); `entity_id` meets the
/// 65536-byte string limit; `entity_type`/`operation` are short fixed strings and
/// `ordinal` a u32 (finite, below MAX_NUMBER), none of which can be refused.
pub fn validate_operation_tree(operations: &[Operation]) -> Result<(), Refusal> {
    if operations.len() > 8192 {
        return Err(Refusal::QueryLimit);
    }
    for op in operations {
        for value in [&op.after, &op.before].into_iter().flatten() {
            validate_value(value, 2)?;
        }
        if op.entity_id.len() > 65536 {
            return Err(Refusal::QueryLimit);
        }
    }
    Ok(())
}
pub fn canonical_bytes(value: &Value) -> Result<Vec<u8>, Refusal> {
    validate_value(value, 0)?;
    serde_jcs::to_vec(value).map_err(|_| Refusal::QueryUnsupported)
}
pub fn validate_value(value: &Value, depth: usize) -> Result<(), Refusal> {
    if depth > 16 {
        return Err(Refusal::QueryLimit);
    }
    match value {
        Value::Number(n) => {
            let f = n.as_f64().ok_or(Refusal::QueryUnsupported)?;
            if !f.is_finite() || f.abs() > MAX_NUMBER {
                return Err(Refusal::QueryUnsupported);
            }
        }
        Value::String(s) if s.len() > 65536 => return Err(Refusal::QueryLimit),
        Value::Array(values) => {
            if values.len() > 8192 {
                return Err(Refusal::QueryLimit);
            }
            for v in values {
                validate_value(v, depth + 1)?;
            }
        }
        Value::Object(values) => {
            if values.len() > 8192 {
                return Err(Refusal::QueryLimit);
            }
            for (k, v) in values {
                if k.len() > 8192 {
                    return Err(Refusal::QueryLimit);
                }
                validate_value(v, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}
pub fn ulid(s: &str) -> bool {
    s.len() == 26
        && s.as_bytes()[0] <= b'7'
        && s.bytes()
            .all(|b| b"0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(&b))
}
fn decimal(s: &str) -> bool {
    !s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit())
}

/// Upper bound of one encoded query frame, newline included. Parameters travel as
/// an embedded JSON object, not a re-escaped string, so their normalized bytes
/// (<= MAX_PARAMETER_BYTES after admission) appear exactly once. Admitted source is
/// ASCII without quotes or backslashes; JSON escaping at most doubles its only
/// escapable bytes (\t \n \x0C \r whitespace). 64 bytes cover keys and punctuation.
pub const MAX_QUERY_FRAME_BYTES: usize =
    2 * crate::MAX_SOURCE_BYTES + crate::MAX_PARAMETER_BYTES + 64;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryRequest {
    pub source: String,
    pub parameters: Value,
}
impl QueryRequest {
    /// Encodes an admitted query; refusal here would mean the bound above is wrong.
    pub fn encode(query: &crate::AdmittedQuery, source: &str) -> Result<Vec<u8>, Refusal> {
        let request = Self {
            source: source.to_owned(),
            parameters: serde_json::from_str(&query.parameters_json()?)
                .map_err(|_| Refusal::QueryUnsupported)?,
        };
        let mut bytes = serde_json::to_vec(&request).map_err(|_| Refusal::QueryUnsupported)?;
        bytes.push(b'\n');
        if bytes.len() > MAX_QUERY_FRAME_BYTES {
            return Err(Refusal::QueryLimit);
        }
        Ok(bytes)
    }
    pub fn parameters_bytes(&self) -> Result<Vec<u8>, Refusal> {
        serde_json::to_vec(&self.parameters).map_err(|_| Refusal::QueryUnsupported)
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InspectRequest {
    pub id: String,
    pub include_archived: bool,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub manifest: Manifest,
    pub pid: u32,
    pub allocated_bytes: usize,
    /// Highest charged allocation at any point in the worker (Cap statistics).
    pub peak_allocated_bytes: usize,
    /// Worker's own peak resident set (getrusage), measured not enforced.
    pub peak_rss_bytes: u64,
    /// `VmData` admitted at the input boundary B0 (Linux; 0 elsewhere).
    pub boundary_vm_data_bytes: u64,
    /// `VmPeak` read at B0: an observation, never an allocation oracle.
    pub boundary_vm_peak_bytes: u64,
    pub rows: Vec<Value>,
}
