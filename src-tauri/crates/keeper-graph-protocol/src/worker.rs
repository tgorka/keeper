//! The frames between a native consumer and its separate graph worker
//! (contract §5 *Execution boundary*, AD-11/AD-12).
//!
//! The worker is a single-use process with three roles: `build` replays a
//! verified accepted prefix into a new generation, `query` answers one
//! admitted `KG-Cypher/1` query from one immutable generation, and
//! `inspect` answers one typed node, edge or content lookup from one. The
//! consumer verified the prefix (every event's operation count, ordinals,
//! delta digest and before-images, `crate::delta`); the builder receives
//! only its result:
//!
//! 1. one [`Manifest`] frame, naming the prefix's length `accepted_head`
//!    (N) and its chained digest [`Manifest::prefix_digest`];
//! 2. exactly N [`EventFrame`]s, positions 1..=N in order, each carrying
//!    the event's header and its operations' after-images only;
//! 3. nothing else.
//!
//! Every frame is one line: its JCS bytes and `\n`, at most
//! [`MAX_EVENT_BYTES`] before the newline. The builder refuses a frame out
//! of order, of the wrong shape or size, an operation count that disagrees,
//! an entity whose type changes, a chain that does not end at the
//! manifest's digest, or trailing input; nothing is published then.
//!
//! [`LINUX_WORKER_GLIBC_TUNABLES`] is part of this contract: the consumer
//! sets it, and nothing else but `GQL_STMT_CACHE=0`, on every Linux worker,
//! and the worker refuses any other environment.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::delta::{Op, MAX_OPERATIONS, MAX_OPERATION_BYTES};
use crate::profile::{AdmittedQuery, MAX_PARAMETER_BYTES, MAX_SOURCE_BYTES};
use crate::value::canonical;
use crate::vocabulary::{EntityType, Operation};
use crate::wire::image_ok;
use crate::{is_decimal, is_ulid, sha256_hex, Refusal, SCHEMA_VERSION};

/// The engine every generation is built and queried with: GraphQLite
/// v0.12.0 at its pinned commit, source-built into the worker.
pub const ENGINE: &str = "graphqlite-0.12.0-995a8b9bc25cdd86d4c2b50ff7ff87f0eb236629";
/// The projection format a generation holds: its schema, symbol mapping and
/// active-only view. A generation of another version is rebuilt. Version 2
/// replays `content` operations (kept for typed inspection, never projected
/// as query symbols) and exposes the six assertion fields as structural
/// keys of a node binding.
pub const PROJECTION_VERSION: u32 = 2;
/// The most canonical bytes one frame may have, newline excluded.
pub const MAX_EVENT_BYTES: usize = 2 * 1024 * 1024;
/// The most canonical bytes one operation's after-image may have.
pub const MAX_ROW_BYTES: usize = MAX_OPERATION_BYTES;
/// The most bytes a whole export may have, every frame and newline counted.
pub const MAX_EXPORT_BYTES: u64 = 1024 * 1024 * 1024;
/// The longest accepted prefix one generation may hold (an R1 limit: a
/// longer retained history is `projection_unavailable`).
pub const MAX_EVENTS: u64 = 100_000;
/// The most entities one generation may hold.
pub const MAX_ENTITIES: u64 = 1_000_000;
/// The most bytes a manifest frame may have.
pub const MAX_MANIFEST_BYTES: usize = 8 * 1024;
/// The most bytes an inspection frame may have.
pub const MAX_INSPECT_FRAME_BYTES: usize = 8 * 1024;
/// The chained prefix digest of the empty prefix.
pub const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// The fixed, non-secret allocator configuration of every Linux worker
/// (OD-A, approved delta r6): no mmapped chunks, no tcache, a trim
/// threshold above the worker's 512 MiB `RLIMIT_AS`, and one arena.
pub const LINUX_WORKER_GLIBC_TUNABLES: &str = "glibc.malloc.mmap_max=0:glibc.malloc.tcache_count=0:glibc.malloc.trim_threshold=536870912:glibc.malloc.arena_max=1";

/// A worker's exit status: the charged allocation budget is exhausted.
pub const EXIT_BUDGET: i32 = 72;
/// A worker's exit status: a thread, callback or registration
/// precondition was violated.
pub const EXIT_PRECONDITION: i32 = 73;
/// A worker's exit status: startup, confinement, the input boundary or a
/// checked state refused.
pub const EXIT_REFUSED: i32 = 74;
/// A worker's exit status: the result's row or byte bound was reached.
pub const EXIT_LIMIT: i32 = 75;
/// The fixed line a worker writes to stdout and stderr before exiting
/// [`EXIT_REFUSED`] when its execution class refuses, followed by one of
/// [`EXECUTION_CLASS_LABELS`].
pub const EXECUTION_CLASS_REFUSED: &str = "execution class refused: ";
/// Why a worker's execution class refused (contract delta §4.4 step 8).
pub const EXECUTION_CLASS_LABELS: [&str; 5] = ["no_new_privs", "auxv", "at_secure", "uid", "gid"];
/// The fixed line a worker writes before exiting [`EXIT_REFUSED`] when its
/// own page size or signal stack size is outside the admitted class.
pub const PLATFORM_REFUSED: &str = "platform prerequisite refused";

/// What a generation is: the domain, the authority it was built under, the
/// accepted prefix it replays and the versions that read it. The consumer
/// writes it, the builder stores it, and every answer repeats it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub domain: String,
    /// The graph-read authority epoch the generation was built under.
    pub authority_epoch: String,
    /// The graph-read authority digest the generation was built under.
    pub authority_digest: String,
    /// N: the prefix holds events 1..=N.
    pub accepted_head: u64,
    /// [`chain`] over events 1..=N, from [`GENESIS`].
    pub prefix_digest: String,
    /// The replica's Syncular commit checkpoint when the prefix was read: a
    /// transport visibility bound, not graph order.
    pub sync_checkpoint: String,
    /// The Syncular schema version of the graph tables ([`SCHEMA_VERSION`]).
    pub contract_schema: u32,
    pub projection_version: u32,
    pub engine: String,
}

impl Manifest {
    /// Refuse a manifest this build does not read: another schema,
    /// projection or engine, a prefix too long, or a malformed field.
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.contract_schema != SCHEMA_VERSION
            || self.projection_version != PROJECTION_VERSION
            || self.engine != ENGINE
        {
            return Err(Refusal::UnsupportedVersion);
        }
        if self.domain.is_empty()
            || self.domain.len() > 128
            || self.domain.chars().any(char::is_control)
            || self.accepted_head > MAX_EVENTS
            || !is_decimal(&self.authority_epoch)
            || !is_hex_digest(&self.authority_digest)
            || !is_hex_digest(&self.prefix_digest)
            || !is_decimal(&self.sync_checkpoint)
        {
            return Err(Refusal::InvalidChange);
        }
        Ok(())
    }

    /// The manifest's frame: its canonical bytes and a newline.
    pub fn encode(&self) -> Result<Vec<u8>, Refusal> {
        self.validate()?;
        line(self, MAX_MANIFEST_BYTES)
    }
}

/// A lowercase SHA-256 hex digest.
#[must_use]
pub fn is_hex_digest(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The chained prefix digest after event `graph_seq` whose delta digest is
/// `delta_digest`: the SHA-256 hex of `previous`, `graph_seq` and
/// `delta_digest`, newline-separated. Events 1..=N chain from [`GENESIS`].
#[must_use]
pub fn chain(previous: &str, graph_seq: u64, delta_digest: &str) -> String {
    sha256_hex(format!("{previous}\n{graph_seq}\n{delta_digest}").as_bytes())
}

/// One operation of an exported event: its row without the before-image.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FrameOp {
    pub ordinal: u32,
    pub entity_type: EntityType,
    pub entity_id: String,
    pub operation: Operation,
    pub after_revision: Option<String>,
    pub after_value: Option<Value>,
}

/// One exported accepted event.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EventFrame {
    pub graph_seq: u64,
    pub event_id: String,
    pub op_count: u32,
    pub delta_digest: String,
    pub ops: Vec<FrameOp>,
}

impl EventFrame {
    /// The frame of an event the consumer verified: its header and the
    /// after-images of `ops` (ordinal order).
    pub fn new(graph_seq: u64, event_id: &str, delta_digest: &str, ops: &[Op]) -> Self {
        Self {
            graph_seq,
            event_id: event_id.to_owned(),
            op_count: u32::try_from(ops.len()).unwrap_or(u32::MAX),
            delta_digest: delta_digest.to_owned(),
            ops: ops
                .iter()
                .map(|op| FrameOp {
                    ordinal: op.ordinal,
                    entity_type: op.entity_type,
                    entity_id: op.entity_id.clone(),
                    operation: op.operation,
                    after_revision: op.after_revision.clone(),
                    after_value: op.after_value.clone(),
                })
                .collect(),
        }
    }

    /// The frame's shape at position `expected`: the position, a ULID
    /// event, a digest, 1..=128 operations in ordinal order agreeing with
    /// `op_count`, and each operation's after-image as its operation
    /// implies — its entity's whole image ([`image_ok`]) for everything but
    /// a membership's removal, which has none. Schema 7 holds no `review`
    /// entity.
    pub fn check(&self, expected: u64) -> Result<(), Refusal> {
        let count = self.ops.len();
        if self.graph_seq != expected
            || !is_ulid(&self.event_id)
            || !is_hex_digest(&self.delta_digest)
            || count == 0
            || count > MAX_OPERATIONS
            || usize::try_from(self.op_count).ok() != Some(count)
        {
            return Err(Refusal::InvalidChange);
        }
        for (ordinal, op) in self.ops.iter().enumerate() {
            if usize::try_from(op.ordinal).ok() != Some(ordinal) || !is_ulid(&op.entity_id) {
                return Err(Refusal::InvalidChange);
            }
            if op.entity_type == EntityType::Review {
                return Err(Refusal::UnsupportedVersion);
            }
            let membership = op.entity_type == EntityType::NodeTag;
            let revised = match &op.after_revision {
                Some(revision) => !membership && is_decimal(revision),
                None => membership,
            };
            let image = match (&op.operation, &op.after_value) {
                (Operation::Remove, None) => membership,
                (Operation::Remove, Some(_)) | (_, None) => false,
                (_, Some(Value::Object(after))) => image_ok(op.entity_type, &op.entity_id, after),
                (_, Some(_)) => false,
            };
            if !revised || !image {
                return Err(Refusal::InvalidChange);
            }
            if let Some(after) = &op.after_value {
                if canonical(after)?.len() > MAX_ROW_BYTES {
                    return Err(Refusal::ChangeLimit);
                }
            }
        }
        Ok(())
    }

    /// The frame's line: its canonical bytes and a newline, at most
    /// [`MAX_EVENT_BYTES`] before the newline.
    pub fn encode(&self) -> Result<Vec<u8>, Refusal> {
        line(self, MAX_EVENT_BYTES)
    }
}

fn line<T: Serialize>(value: &T, limit: usize) -> Result<Vec<u8>, Refusal> {
    let mut bytes = serde_jcs::to_vec(value).map_err(|_| Refusal::InvalidChange)?;
    if bytes.len() > limit {
        return Err(Refusal::ChangeLimit);
    }
    bytes.push(b'\n');
    Ok(bytes)
}

/// The upper bound of one encoded query frame, newline included.
/// Parameters travel as an embedded JSON object, so their normalised bytes
/// (at most [`MAX_PARAMETER_BYTES`]) appear once; admitted source is ASCII
/// without quotes or backslashes, and JSON escaping at most doubles its
/// only escapable bytes (whitespace). 64 bytes cover keys and punctuation.
pub const MAX_QUERY_FRAME_BYTES: usize = 2 * MAX_SOURCE_BYTES + MAX_PARAMETER_BYTES + 64;

/// A `query` worker's one request.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryRequest {
    pub source: String,
    pub parameters: Value,
}

impl QueryRequest {
    /// The frame of an admitted query; a refusal here means the bound above
    /// is wrong.
    pub fn encode(query: &AdmittedQuery, source: &str) -> Result<Vec<u8>, Refusal> {
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

    /// The parameters' bytes, for the worker's own admission.
    pub fn parameters_bytes(&self) -> Result<Vec<u8>, Refusal> {
        serde_json::to_vec(&self.parameters).map_err(|_| Refusal::QueryUnsupported)
    }
}

/// What an `inspect` request looks up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectEntity {
    Node,
    Edge,
    Content,
}

/// An `inspect` worker's one request: the typed `graph.get_node` /
/// `graph.get_edge` / `graph.get_content` read over one generation. Without
/// `include_archived` a node is answered only while active, an edge only
/// while it and both its endpoints are, and a content only while its owner
/// is; a node's answer carries its tags' canonical texts, ascending.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InspectRequest {
    pub entity: InspectEntity,
    pub id: String,
    pub include_archived: bool,
}

impl InspectRequest {
    /// The request's frame.
    pub fn encode(&self) -> Result<Vec<u8>, Refusal> {
        if !is_ulid(&self.id) {
            return Err(Refusal::InvalidChange);
        }
        line(self, MAX_INSPECT_FRAME_BYTES)
    }
}

/// A worker's one answer. Every role repeats the generation's manifest.
/// The counters are measurements, never enforcement: the budget is
/// enforced inside the worker.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub manifest: Manifest,
    pub pid: u32,
    pub allocated_bytes: usize,
    /// The highest charged allocation at any point in the worker.
    pub peak_allocated_bytes: usize,
    /// The worker's own peak resident set (getrusage).
    pub peak_rss_bytes: u64,
    /// `VmData` admitted at the input boundary B0 (Linux; 0 elsewhere).
    pub boundary_vm_data_bytes: u64,
    /// `VmPeak` read at B0: an observation only.
    pub boundary_vm_peak_bytes: u64,
    pub rows: Vec<Value>,
}
