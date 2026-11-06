//! The graph's registered reads: their params and the rows they answer
//! with (contract §2, §9.1).
//!
//! Every read's params carry `version` ([`crate::CONTRACT_VERSION`]) and
//! `context_id` (`null` for a person's device, the admitted context for an
//! agent); a refusal is `operation.forbidden` with a stable code, and a
//! caller without read authority is refused alike whatever exists.
//!
//! | read | params | rows |
//! | --- | --- | --- |
//! | [`OUTCOME`] | `request_id` | `[]` or one `kg_receipts` row of the caller's own client scope |
//! | [`GET_NODE`] | `id`, `include_archived` | `[]` or `[{<node image>, "tags": [<text>…]}]` |
//! | [`GET_EDGE`] | `id`, `include_archived` | `[]` or `[<edge image>]` |
//! | [`HISTORY`] | `after`, `limit`, `filter` | `[{"events": [{"event": <header>, "ops": [<op>…]}], "scanned_through": <decimal>}]` |
//!
//! Without `include_archived` a node is answered only while active and an
//! edge only while it and both its endpoints are. Images are the shapes of
//! [`crate::candidate`]; tags are canonical texts in ascending order.
//!
//! **History** pages the retained events, not the transport log: every
//! event in `(after, scanned_through]` that matches `filter` is in
//! `events`, ascending, each with all of its operations; `scanned_through`
//! is the last graph position the server examined, matching or not, so a
//! filtered or empty page still moves the cursor and a later matching event
//! is never skipped. A page holds at most `limit` events, examines at most
//! [`MAX_HISTORY_SCAN`] positions and at most [`MAX_HISTORY_PAGE_BYTES`] of
//! canonical event bytes, always at least one whole event when one matches.
//! `filter` holds any of `entity_id` (an operation's entity), `actor_id`
//! and `origin_class`, each matching exactly.
//!
//! The native decoders hold the server to these shapes: an image outside
//! its entity's rules, or a page outside the bounds above, is refused as
//! an answer this build does not read, never presented as a typed result.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::delta::{self, Op};
use crate::value::{canonical, check_properties};
use crate::vocabulary::{stored_edge_type, stored_node_kind, EntityType, OriginClass};
use crate::{is_decimal, is_ulid, Refusal, CONTRACT_VERSION};

pub const OUTCOME: &str = "graph.outcome";
pub const GET_NODE: &str = "graph.get_node";
pub const GET_EDGE: &str = "graph.get_edge";
pub const HISTORY: &str = "graph.history";

/// The most events one history page may ask for.
pub const MAX_HISTORY_LIMIT: u32 = 100;
/// The most graph positions one history page examines.
pub const MAX_HISTORY_SCAN: u32 = 1_000;
/// The most canonical bytes of events one history page holds, beyond its
/// first event.
pub const MAX_HISTORY_PAGE_BYTES: usize = 4 * 1024 * 1024;

/// [`OUTCOME`]'s params.
#[must_use]
pub fn outcome_params(context_id: Option<&str>, request_id: &str) -> Value {
    json!({ "version": CONTRACT_VERSION, "context_id": context_id, "request_id": request_id })
}

/// [`GET_NODE`]'s or [`GET_EDGE`]'s params.
#[must_use]
pub fn get_params(context_id: Option<&str>, id: &str, include_archived: bool) -> Value {
    json!({
        "version": CONTRACT_VERSION, "context_id": context_id,
        "id": id, "include_archived": include_archived,
    })
}

/// Which events a history page answers with.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryFilter {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_class: Option<OriginClass>,
}

/// [`HISTORY`]'s params: events after graph position `after` (`"0"` for the
/// first), at most `limit` of them.
pub fn history_params(
    context_id: Option<&str>,
    after: &str,
    limit: u32,
    filter: &HistoryFilter,
) -> Result<Value, Refusal> {
    if !is_decimal(after) || !(1..=MAX_HISTORY_LIMIT).contains(&limit) {
        return Err(Refusal::InvalidChange);
    }
    Ok(json!({
        "version": CONTRACT_VERSION, "context_id": context_id,
        "after": after, "limit": limit, "filter": filter,
    }))
}

/// An inspected node: its image and its tags' canonical texts.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeView {
    pub image: Map<String, Value>,
    pub tags: Vec<String>,
}

const NODE_IMAGE: [&str; 10] = [
    "id",
    "kind",
    "title",
    "description",
    "description_source_id",
    "properties",
    "lifecycle",
    "merged_into_id",
    "revision",
    "lifecycle_revision",
];
const EDGE_IMAGE: [&str; 7] = [
    "id",
    "from_id",
    "to_id",
    "type",
    "properties",
    "lifecycle",
    "revision",
];

/// One row at most; anything else is a server this build does not read.
fn single(rows: Vec<Map<String, Value>>) -> Result<Option<Map<String, Value>>, Refusal> {
    let mut rows = rows.into_iter();
    match (rows.next(), rows.next()) {
        (row, None) => Ok(row),
        _ => Err(Refusal::UnsupportedVersion),
    }
}

fn ulid_or_null(image: &Map<String, Value>, field: &str) -> bool {
    match image.get(field) {
        Some(Value::Null) => true,
        Some(Value::String(id)) => is_ulid(id),
        _ => false,
    }
}

fn positive(image: &Map<String, Value>, field: &str) -> bool {
    image
        .get(field)
        .and_then(Value::as_str)
        .is_some_and(|r| is_decimal(r) && r != "0")
}

fn lifecycle(image: &Map<String, Value>) -> bool {
    matches!(
        image.get("lifecycle").and_then(Value::as_str),
        Some("active" | "archived")
    )
}

fn properties(image: &Map<String, Value>) -> bool {
    image
        .get("properties")
        .is_some_and(|p| check_properties(p).is_ok())
}

/// Whether `image` is node `id` as the contract holds one: exactly the
/// node fields, each of its type and within its rules.
fn node_ok(image: &Map<String, Value>, id: &str) -> bool {
    image.len() == NODE_IMAGE.len()
        && NODE_IMAGE.iter().all(|field| image.contains_key(*field))
        && image.get("id").and_then(Value::as_str) == Some(id)
        && image
            .get("kind")
            .and_then(Value::as_str)
            .is_some_and(|kind| stored_node_kind(kind).is_ok())
        && image
            .get("title")
            .and_then(Value::as_str)
            .is_some_and(|title| !title.is_empty())
        && matches!(
            image.get("description"),
            Some(Value::Null | Value::String(_))
        )
        && ulid_or_null(image, "description_source_id")
        && ulid_or_null(image, "merged_into_id")
        && properties(image)
        && lifecycle(image)
        && positive(image, "revision")
        && positive(image, "lifecycle_revision")
}

/// Whether `image` is edge `id` as the contract holds one.
fn edge_ok(image: &Map<String, Value>, id: &str) -> bool {
    image.len() == EDGE_IMAGE.len()
        && EDGE_IMAGE.iter().all(|field| image.contains_key(*field))
        && image.get("id").and_then(Value::as_str) == Some(id)
        && ["from_id", "to_id"]
            .iter()
            .all(|end| image.get(*end).and_then(Value::as_str).is_some_and(is_ulid))
        && image
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|kind| stored_edge_type(kind).is_ok())
        && properties(image)
        && lifecycle(image)
        && positive(image, "revision")
}

/// Whether `image` is the after-image of entity `id` of `entity_type` as
/// the contract writes one: a node `{id, kind, title, description,
/// description_source_id, properties, lifecycle, merged_into_id, revision,
/// lifecycle_revision}`, an edge `{id, from_id, to_id, type, properties,
/// lifecycle, revision}`, a tag `{id, text, label, revision}` or a
/// membership `{id, node_id, tag_id}`, each field within its rules. Schema
/// 6 holds no `content` or `review` image.
#[must_use]
pub fn image_ok(entity_type: EntityType, id: &str, image: &Map<String, Value>) -> bool {
    match entity_type {
        EntityType::Node => node_ok(image, id),
        EntityType::Edge => edge_ok(image, id),
        EntityType::Tag => {
            image.len() == 4
                && image.get("id").and_then(Value::as_str) == Some(id)
                && image
                    .get("text")
                    .and_then(Value::as_str)
                    .is_some_and(|text| !text.is_empty() && !text.chars().any(char::is_control))
                && matches!(image.get("label"), Some(Value::Null | Value::String(_)))
                && positive(image, "revision")
        }
        EntityType::NodeTag => {
            image.len() == 3
                && image.get("id").and_then(Value::as_str) == Some(id)
                && ["node_id", "tag_id"]
                    .iter()
                    .all(|end| image.get(*end).and_then(Value::as_str).is_some_and(is_ulid))
        }
        EntityType::Content | EntityType::Review => false,
    }
}

/// Decode [`GET_NODE`]'s answer for node `id`. An image outside the node
/// shape, or tags that are not distinct non-empty texts in ascending order,
/// refuse `unsupported_version` rather than read as an inspected node.
pub fn decode_node(id: &str, rows: Vec<Map<String, Value>>) -> Result<Option<NodeView>, Refusal> {
    let Some(mut row) = single(rows)? else {
        return Ok(None);
    };
    let tags = match row.remove("tags") {
        Some(Value::Array(tags)) => tags
            .into_iter()
            .map(|tag| match tag {
                Value::String(tag) if !tag.is_empty() => Ok(tag),
                _ => Err(Refusal::UnsupportedVersion),
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(Refusal::UnsupportedVersion),
    };
    if !node_ok(&row, id) || tags.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(Refusal::UnsupportedVersion);
    }
    Ok(Some(NodeView { image: row, tags }))
}

/// Decode [`GET_EDGE`]'s answer for edge `id`; an image outside the edge
/// shape refuses `unsupported_version`.
pub fn decode_edge(
    id: &str,
    rows: Vec<Map<String, Value>>,
) -> Result<Option<Map<String, Value>>, Refusal> {
    match single(rows)? {
        None => Ok(None),
        Some(row) if edge_ok(&row, id) => Ok(Some(row)),
        Some(_) => Err(Refusal::UnsupportedVersion),
    }
}

/// An accepted event's header: its `kg_events` row without the scope and
/// author columns, JSON columns decoded.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventHeader {
    pub id: String,
    pub graph_seq: String,
    pub descriptor: String,
    pub request_id: String,
    pub request_digest: String,
    pub actor_id: String,
    pub client_id: String,
    pub origin_class: OriginClass,
    pub origin_actor: Option<String>,
    /// `null` for a person's event; for an agent's, root first,
    /// `[{"session_id", "agent_actor"}…]` down to the acting session.
    pub origin_chain: Option<Value>,
    pub context_id: Option<String>,
    /// `{"basis": "direct"}` for a person's own write; an agent's
    /// `{"basis": "approval", "approval_id", "grant_id", "origin"}`.
    pub approval_basis: Option<Value>,
    pub reverses_event_id: Option<String>,
    pub event_time_ms: Option<u64>,
    pub recorded_at_ms: u64,
    pub op_count: u32,
    pub delta_digest: String,
}

/// One whole event.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub event: EventHeader,
    pub ops: Vec<Op>,
}

impl Event {
    /// Whether the event is one complete group: a ULID id, a positive
    /// graph sequence, `op_count` operations whose digest is
    /// `delta_digest`.
    pub fn check(&self) -> Result<(), Refusal> {
        let header = &self.event;
        let complete = is_ulid(&header.id)
            && is_decimal(&header.graph_seq)
            && header.graph_seq != "0"
            && usize::try_from(header.op_count).ok() == Some(self.ops.len());
        if complete && delta::digest(&self.ops)? == header.delta_digest {
            Ok(())
        } else {
            Err(Refusal::UnsupportedVersion)
        }
    }

    /// The event's graph position.
    #[must_use]
    pub fn position(&self) -> u128 {
        self.event.graph_seq.parse().unwrap_or(0)
    }
}

/// One history page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryPage {
    pub events: Vec<Event>,
    pub scanned_through: String,
}

/// The most canonical bytes one event header may have: an event row, like
/// every graph row, is at most [`delta::MAX_OPERATION_BYTES`].
pub const MAX_EVENT_HEADER_BYTES: usize = delta::MAX_OPERATION_BYTES;

/// One raw event of a page, checked before it is decoded: `{event, ops}`
/// and nothing else, a header within [`MAX_EVENT_HEADER_BYTES`], 1 to
/// [`delta::MAX_OPERATIONS`] operations each within
/// [`delta::MAX_OPERATION_BYTES`] and together within
/// [`delta::MAX_DELTA_BYTES`]. Its size for the page budget: the canonical
/// bytes of the whole entry, `{"event":…,"ops":…}` wrapper included, as the
/// server counts it.
fn raw_event(event: &Value) -> Result<usize, Refusal> {
    let refuse = || Refusal::UnsupportedVersion;
    let fields = event.as_object().ok_or_else(refuse)?;
    let (Some(header), Some(Value::Array(ops)), 2) =
        (fields.get("event"), fields.get("ops"), fields.len())
    else {
        return Err(refuse());
    };
    if !header.is_object() || ops.is_empty() || ops.len() > delta::MAX_OPERATIONS {
        return Err(refuse());
    }
    if canonical(header).map_err(|_| refuse())?.len() > MAX_EVENT_HEADER_BYTES {
        return Err(refuse());
    }
    let mut delta = 2 + ops.len() - 1;
    for op in ops {
        let op = canonical(op).map_err(|_| refuse())?.len();
        delta += op;
        if op > delta::MAX_OPERATION_BYTES || delta > delta::MAX_DELTA_BYTES {
            return Err(refuse());
        }
    }
    Ok(canonical(event).map_err(|_| refuse())?.len())
}

/// Decode [`HISTORY`]'s answer to a page of at most `limit` events after
/// `after`, refusing (`unsupported_version`) anything the page bounds rule
/// out before any event is decoded: more than `limit` events, more than
/// [`MAX_HISTORY_SCAN`] positions scanned, any event — the first included —
/// outside its row and group bounds (a header within
/// [`MAX_EVENT_HEADER_BYTES`], 1 to 128 operations within their row and
/// delta bounds), or more than
/// [`MAX_HISTORY_PAGE_BYTES`] of canonical event bytes beyond the first
/// event (the one bound the first whole event is exempt from). Then every
/// event must be whole and checked, positions ascending within
/// `(after, scanned_through]`.
pub fn decode_history(
    after: &str,
    limit: u32,
    rows: Vec<Map<String, Value>>,
) -> Result<HistoryPage, Refusal> {
    let row = single(rows)?.ok_or(Refusal::UnsupportedVersion)?;
    let parse = |s: &str| s.parse::<u128>().ok().filter(|_| is_decimal(s));
    let through = row
        .get("scanned_through")
        .and_then(Value::as_str)
        .and_then(parse);
    let (Some(start), Some(through)) = (parse(after), through) else {
        return Err(Refusal::UnsupportedVersion);
    };
    let events = match row.get("events") {
        Some(Value::Array(events)) => events,
        _ => return Err(Refusal::UnsupportedVersion),
    };
    let limit = usize::try_from(limit.min(MAX_HISTORY_LIMIT)).unwrap_or(0);
    if through < start || through - start > u128::from(MAX_HISTORY_SCAN) || events.len() > limit {
        return Err(Refusal::UnsupportedVersion);
    }
    let mut bytes = 0usize;
    for (index, event) in events.iter().enumerate() {
        let size = raw_event(event)?;
        if index > 0 {
            bytes += size;
            if bytes > MAX_HISTORY_PAGE_BYTES {
                return Err(Refusal::UnsupportedVersion);
            }
        }
    }
    let page: HistoryPage =
        serde_json::from_value(Value::Object(row)).map_err(|_| Refusal::UnsupportedVersion)?;
    let mut last = start;
    for event in &page.events {
        event.check()?;
        let at = event.position();
        if at <= last || at > through {
            return Err(Refusal::UnsupportedVersion);
        }
        last = at;
    }
    Ok(page)
}

/// The graph-read context operations a native reader runs (contract §9.3;
/// keeper-hq `server/src/graph/operations.ts`): the agent path admits a
/// domain into its context and refreshes it before every delivery; the
/// person's path reads the domain's read status. A refusal is
/// `operation.forbidden` with a stable code, collapsed for a caller who may
/// not read: none of them says whether something exists.
pub const ADMIT_READ: &str = "graph.admit_read";
pub const REFRESH_CONTEXT: &str = "graph.refresh_context";
pub const READ_STATUS: &str = "graph.read_status";

/// [`ADMIT_READ`]'s params: add `domain` to `context`'s sources.
#[must_use]
pub fn admit_read_params(context: &str, domain: &str) -> Value {
    json!({ "version": CONTRACT_VERSION, "context": context, "domain": domain })
}

/// [`REFRESH_CONTEXT`]'s params.
#[must_use]
pub fn refresh_context_params(context: &str) -> Value {
    json!({ "version": CONTRACT_VERSION, "context": context })
}

/// [`READ_STATUS`]'s params: the person's current read authority over
/// `domain`.
#[must_use]
pub fn read_status_params(domain: &str) -> Value {
    json!({ "version": CONTRACT_VERSION, "domain": domain })
}

/// The graph-read authority a reader holds over one domain: the server's
/// authority epoch and the `(domain, principal)` authority digest. A
/// generation is built under one and served only while the reader's fresh
/// authority is the same.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Authority {
    pub epoch: String,
    pub digest: String,
}

impl Authority {
    fn valid(&self) -> bool {
        is_decimal(&self.epoch) && crate::worker::is_hex_digest(&self.digest)
    }
}

/// A context row: what [`ADMIT_READ`] and [`REFRESH_CONTEXT`] answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextRow {
    pub context: String,
    pub session: String,
    pub permitted: Vec<String>,
    pub sources: Vec<String>,
    pub expires_at_ms: u64,
    pub authority: Authority,
}

/// [`READ_STATUS`]'s row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadStatus {
    pub domain: String,
    pub authority: Authority,
}

fn domains_ok(domains: &[String]) -> bool {
    domains
        .iter()
        .all(|d| !d.is_empty() && d.len() <= 128 && !d.chars().any(char::is_control))
}

/// Decode a context operation's answer for `context`: exactly one closed
/// row naming that context, a ULID, with valid domains and authority.
pub fn decode_context(context: &str, rows: Vec<Map<String, Value>>) -> Result<ContextRow, Refusal> {
    let row = single(rows)?.ok_or(Refusal::UnsupportedVersion)?;
    let row: ContextRow =
        serde_json::from_value(Value::Object(row)).map_err(|_| Refusal::UnsupportedVersion)?;
    if row.context != context
        || !is_ulid(&row.context)
        || row.session.is_empty()
        || !domains_ok(&row.permitted)
        || !domains_ok(&row.sources)
        || !row.authority.valid()
    {
        return Err(Refusal::UnsupportedVersion);
    }
    Ok(row)
}

/// Decode [`READ_STATUS`]'s answer for `domain`.
pub fn decode_read_status(
    domain: &str,
    rows: Vec<Map<String, Value>>,
) -> Result<ReadStatus, Refusal> {
    let row = single(rows)?.ok_or(Refusal::UnsupportedVersion)?;
    let row: ReadStatus =
        serde_json::from_value(Value::Object(row)).map_err(|_| Refusal::UnsupportedVersion)?;
    if row.domain != domain || !row.authority.valid() {
        return Err(Refusal::UnsupportedVersion);
    }
    Ok(row)
}
