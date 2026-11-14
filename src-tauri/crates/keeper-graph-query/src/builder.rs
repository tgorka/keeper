//! The `build` role: replay one verified accepted prefix into a new
//! generation inside one checked host transaction.
//!
//! Input (`keeper_graph_protocol::worker`): one manifest frame, then exactly
//! `accepted_head` event frames in order, then nothing. Each event's after-
//! images replace its entities whole, so a removed property, a changed
//! field or a lifecycle change never survives from an earlier image; a
//! membership `remove` clears its row. An entity keeps its type for good, a
//! `create` needs an entity that does not exist yet (a membership may be
//! created again after its removal) and every other operation one that
//! does. The frames' chained digest must end at the manifest's
//! `prefix_digest`. The engine tables are then projected from the final
//! entities only: active nodes and every tag as nodes, active edges whose
//! endpoints are both active, and each membership of an active node as a
//! `HAS_TAG` relationship to its tag; independent edge identities stay
//! independent. Contents are replayed for typed inspection and never
//! projected; a node's reserved `$` members are never property symbols,
//! and its `$assertion` fields are projected under their own names
//! (projection version 2). A `review` operation (schema 8) is checked and
//! skipped: a review is never an entity of a generation, and a reversal's
//! node and edge operations replace their entities' images like any other,
//! so a cleared or restored `merged_into_id` and restored endpoints are
//! re-projected from the final images (projection version 3).

use crate::engine::Result;
use keeper_graph_protocol::symbol::{encode, SymbolKind};
use keeper_graph_protocol::value::{MAX_NUMBER, RESERVED_MEMBERS};
use keeper_graph_protocol::vocabulary::{
    EntityType, Operation, ASSERTION_FIELDS, EDGE_FIELDS, NODE_FIELDS, TAG_FIELDS,
};
use keeper_graph_protocol::worker::{
    chain, EventFrame, Manifest, GENESIS, MAX_ENTITIES, MAX_EVENT_BYTES, MAX_EXPORT_BYTES,
    MAX_MANIFEST_BYTES, MAX_ROW_BYTES,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{Map, Value};
use std::io::{BufRead, Read};

/// One newline-terminated frame of at most `limit` bytes, newline excluded.
pub fn frame<R: BufRead>(reader: &mut R, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    Read::take(reader, limit as u64 + 1).read_until(b'\n', &mut bytes)?;
    if bytes.pop() != Some(b'\n') || bytes.is_empty() || bytes.len() > limit {
        return Err("bounded frame refused".into());
    }
    Ok(bytes)
}

fn entity_type(kind: EntityType) -> Result<&'static str> {
    Ok(match kind {
        EntityType::Node => "node",
        EntityType::Edge => "edge",
        EntityType::Tag => "tag",
        EntityType::NodeTag => "node_tag",
        EntityType::Content => "content",
        EntityType::Review => return Err("a review is never replayed".into()),
    })
}

pub fn build<R: BufRead>(connection: Connection, reader: &mut R) -> Result<Manifest> {
    let first = frame(reader, MAX_MANIFEST_BYTES)?;
    let manifest: Manifest = serde_json::from_slice(&first)?;
    manifest.validate()?;
    connection.execute_batch("PRAGMA max_page_count=524288")?;
    for sql in &crate::schema::KEEPER[..3] {
        connection.execute_batch(sql)?;
    }
    let mut total = first.len() as u64 + 1;
    let mut digest = GENESIS.to_owned();
    for sequence in 1..=manifest.accepted_head {
        let bytes = frame(reader, MAX_EVENT_BYTES)?;
        total += bytes.len() as u64 + 1;
        if total > MAX_EXPORT_BYTES {
            return Err("export byte limit".into());
        }
        let event: EventFrame = serde_json::from_slice(&bytes)?;
        check(&event, sequence)?;
        digest = chain(&digest, sequence, &event.delta_digest);
        connection.execute_batch("SAVEPOINT accepted_event")?;
        for op in event
            .ops
            .iter()
            .filter(|op| op.entity_type != EntityType::Review)
        {
            replay(&connection, op)?;
        }
        connection.execute(
            "INSERT INTO _kg_events(sequence,digest,delta) VALUES (?1,?2,?3)",
            params![
                i64::try_from(sequence)?,
                event.delta_digest,
                std::str::from_utf8(&bytes)?
            ],
        )?;
        connection.execute_batch("RELEASE accepted_event")?;
    }
    if digest != manifest.prefix_digest {
        return Err("accepted prefix digest mismatch".into());
    }
    let mut trailing = [0u8; 1];
    if reader.read(&mut trailing)? != 0 {
        return Err("export trailing data".into());
    }
    let entities: i64 =
        connection.query_row("SELECT count(*) FROM _kg_entities", [], |r| r.get(0))?;
    if entities > i64::try_from(MAX_ENTITIES)? {
        return Err("projection entity limit".into());
    }
    project(&connection)?;
    connection.execute(
        "INSERT INTO _kg_manifest(body) VALUES (?1)",
        [serde_json::to_string(&manifest)?],
    )?;
    connection.execute_batch("ANALYZE")?;
    crate::schema::verify(&connection, crate::schema::Phase::Published)?;
    connection.execute_batch("COMMIT")?;
    let mode: String = connection.query_row("PRAGMA journal_mode=DELETE", [], |r| r.get(0))?;
    if mode != "delete" {
        return Err("journal transition refused".into());
    }
    connection.close().map_err(|(_, e)| e)?;
    Ok(manifest)
}

/// The reviewed 110.1 worker value admission (inventory round 10,
/// invariant 5), unchanged: nesting at most 16, every number finite and
/// within ±2^53−1, every string at most 65,536 bytes, every array or object
/// at most 8,192 entries and every key at most 8,192 bytes. A value outside
/// it refuses the whole export (`projection_unavailable` at the consumer):
/// an R1 limit, even where the server's own rules accepted the value.
pub fn validate_value(value: &Value, depth: usize) -> Result<()> {
    if depth > 16 {
        return Err("value nesting limit".into());
    }
    match value {
        Value::Number(n) => {
            let f = n.as_f64().ok_or("invalid number")?;
            if !f.is_finite() || f.abs() > MAX_NUMBER {
                return Err("number range".into());
            }
        }
        Value::String(s) if s.len() > 65536 => return Err("string byte limit".into()),
        Value::Array(values) => {
            if values.len() > 8192 {
                return Err("array length limit".into());
            }
            for v in values {
                validate_value(v, depth + 1)?;
            }
        }
        Value::Object(values) => {
            if values.len() > 8192 {
                return Err("object size limit".into());
            }
            for (k, v) in values {
                if k.len() > 8192 {
                    return Err("key byte limit".into());
                }
                validate_value(v, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// The canonical (JCS) bytes of an admitted value, serialized from the
/// borrowed tree as the reviewed worker did: no normalized copy.
pub fn canonical_bytes(value: &Value) -> Result<Vec<u8>> {
    validate_value(value, 0)?;
    Ok(serde_jcs::to_vec(value)?)
}

/// One event frame's shape at position `expected`, checked in the worker
/// without the shared value code (no normalized copy): the position, a
/// ULID event and entities, a digest, 1..=128 operations agreeing with
/// `op_count` in ordinal order, schema 8 entity types, the revision and
/// after-image each operation implies, each after-image admitted by
/// [`validate_value`] within [`MAX_ROW_BYTES`] canonical bytes and of its
/// entity's shape (a review's: a `create` at revision 1 of the review
/// image).
fn check(event: &EventFrame, expected: u64) -> Result<()> {
    let count = event.ops.len();
    if event.graph_seq != expected
        || !keeper_graph_protocol::is_ulid(&event.event_id)
        || !keeper_graph_protocol::worker::is_hex_digest(&event.delta_digest)
        || count == 0
        || count > 128
        || usize::try_from(event.op_count).ok() != Some(count)
    {
        return Err("event frame refused".into());
    }
    for (ordinal, op) in event.ops.iter().enumerate() {
        if usize::try_from(op.ordinal).ok() != Some(ordinal)
            || !keeper_graph_protocol::is_ulid(&op.entity_id)
        {
            return Err("operation refused".into());
        }
        let membership = match op.entity_type {
            EntityType::NodeTag => true,
            EntityType::Node | EntityType::Edge | EntityType::Tag | EntityType::Content => false,
            EntityType::Review => {
                review(op)?;
                continue;
            }
        };
        let revised = match &op.after_revision {
            Some(revision) => !membership && keeper_graph_protocol::is_decimal(revision),
            None => membership,
        };
        match (&op.operation, &op.after_value) {
            (Operation::Remove, None) if membership && revised => {}
            (Operation::Remove, _) | (_, None) => return Err("operation image refused".into()),
            (_, Some(after)) => {
                if !revised {
                    return Err("operation revision refused".into());
                }
                validate_value(after, 0)?;
                if serde_jcs::to_vec(after)?.len() > MAX_ROW_BYTES {
                    return Err("after-image byte limit".into());
                }
                validate_entity(op.entity_type, &op.entity_id, after)?;
            }
        }
    }
    Ok(())
}

fn validate_entity(kind: EntityType, id: &str, value: &Value) -> Result<()> {
    let object = value.as_object().ok_or("entity must be object")?;
    if object.get("id").and_then(Value::as_str) != Some(id) {
        return Err("canonical identity mismatch".into());
    }
    if let Some(properties) = object.get("properties") {
        let properties = properties.as_object().ok_or("properties must be object")?;
        if serde_json::to_vec(properties)?.len() > 65536 {
            return Err("property byte limit".into());
        }
        for (key, value) in properties {
            // A node's reserved members are stored, never symbols.
            if !(kind == EntityType::Node && RESERVED_MEMBERS.contains(&key.as_str())) {
                encode(SymbolKind::Property, key)?;
            }
            property_depth(value, 0)?;
        }
    }
    match kind {
        EntityType::Node | EntityType::Edge => {
            if !matches!(text(object, "lifecycle")?, "active" | "archived") {
                return Err("invalid lifecycle".into());
            }
            if !keeper_graph_protocol::is_decimal(text(object, "revision")?) {
                return Err("invalid revision".into());
            }
            if kind == EntityType::Node {
                mapped(SymbolKind::Node, text(object, "kind")?)?;
            } else {
                mapped(SymbolKind::Edge, text(object, "type")?)?;
                for key in ["from_id", "to_id"] {
                    if !keeper_graph_protocol::is_ulid(text(object, key)?) {
                        return Err("invalid endpoint".into());
                    }
                }
            }
        }
        EntityType::Tag => {
            text(object, "text")?;
        }
        EntityType::NodeTag => {
            for key in ["node_id", "tag_id"] {
                if !keeper_graph_protocol::is_ulid(text(object, key)?) {
                    return Err("invalid membership endpoint".into());
                }
            }
        }
        EntityType::Content => {
            if !keeper_graph_protocol::is_ulid(text(object, "owner_node_id")?)
                || !keeper_graph_protocol::is_decimal(text(object, "revision")?)
                || !object.get("body").is_some_and(Value::is_object)
            {
                return Err("invalid content".into());
            }
        }
        EntityType::Review => return Err("a review is checked as an operation".into()),
    }
    Ok(())
}

/// A review operation's shape: a `create` at revision 1 whose after-image
/// is a review image within [`validate_value`].
fn review(op: &keeper_graph_protocol::worker::FrameOp) -> Result<()> {
    let image = match (&op.operation, &op.after_revision, &op.after_value) {
        (Operation::Create, Some(revision), Some(Value::Object(image))) if revision == "1" => image,
        _ => return Err("review operation refused".into()),
    };
    validate_value(op.after_value.as_ref().unwrap_or(&Value::Null), 0)?;
    if !keeper_graph_protocol::review::image_ok(&op.entity_id, image) {
        return Err("review image refused".into());
    }
    Ok(())
}

fn property_depth(value: &Value, depth: usize) -> Result<()> {
    if depth > 8 {
        return Err("property nesting limit".into());
    }
    match value {
        Value::Array(a) => {
            for v in a {
                property_depth(v, depth + 1)?;
            }
        }
        Value::Object(o) => {
            for v in o.values() {
                property_depth(v, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Apply one checked operation's after-image to `_kg_entities`.
fn replay(connection: &Connection, op: &keeper_graph_protocol::worker::FrameOp) -> Result<()> {
    let kind = entity_type(op.entity_type)?;
    let previous: Option<(String, Option<String>)> = connection
        .query_row(
            "SELECT entity_type, body FROM _kg_entities WHERE id=?1",
            [&op.entity_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let live = match &previous {
        Some((held, body)) if held == kind => body.is_some(),
        Some(_) => return Err("accepted entity changed type".into()),
        None => false,
    };
    let expected_live = op.operation != Operation::Create;
    if live != expected_live {
        return Err("accepted operation on an entity in the wrong state".into());
    }
    let after = match &op.after_value {
        Some(value) => Some(String::from_utf8(canonical_bytes(value)?)?),
        None => None,
    };
    connection.execute(
        "INSERT INTO _kg_entities(id,entity_type,body) VALUES (?1,?2,?3) \
         ON CONFLICT(id) DO UPDATE SET body=excluded.body",
        params![op.entity_id, kind, after],
    )?;
    Ok(())
}

fn text<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a str> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing structural field {key}").into())
}

/// The engine label or relationship type of a kind or type.
fn mapped(kind: SymbolKind, name: &str) -> Result<String> {
    Ok(encode(kind, name)?)
}

fn active(object: &Map<String, Value>) -> Result<bool> {
    Ok(text(object, "lifecycle")? == "active")
}

fn project(connection: &Connection) -> Result<()> {
    connection.execute_batch(crate::schema::KEEPER[3])?;
    let mut stmt = connection.prepare(
        "SELECT id,entity_type,body FROM _kg_entities WHERE body IS NOT NULL \
         AND entity_type IN ('node','tag') ORDER BY id",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let kind: String = row.get(1)?;
        let body: String = row.get(2)?;
        let object: Map<String, Value> = serde_json::from_str(&body)?;
        let (label, fields): (String, &[&str]) = if kind == "tag" {
            ("Tag".into(), &TAG_FIELDS)
        } else if active(&object)? {
            (
                mapped(SymbolKind::Node, text(&object, "kind")?)?,
                &NODE_FIELDS,
            )
        } else {
            // Archived nodes keep their canonical entity for typed inspection
            // but never enter the active view.
            continue;
        };
        connection.execute("INSERT INTO nodes DEFAULT VALUES", [])?;
        let native = connection.last_insert_rowid();
        connection.execute(
            "INSERT INTO _kg_native(id,native_id) VALUES (?1,?2)",
            params![id, native],
        )?;
        connection.execute(
            "INSERT INTO node_labels(node_id,label) VALUES (?1,?2)",
            params![native, label],
        )?;
        properties(connection, false, native, &object, fields)?;
    }
    drop(rows);
    drop(stmt);
    let mut stmt = connection.prepare(
        "SELECT entity_type,body FROM _kg_entities WHERE body IS NOT NULL \
         AND entity_type IN ('edge','node_tag') ORDER BY id",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let kind: String = row.get(0)?;
        let body: String = row.get(1)?;
        let object: Map<String, Value> = serde_json::from_str(&body)?;
        let (from, to, relation, edge) = if kind == "edge" {
            if !active(&object)? {
                continue;
            }
            (
                text(&object, "from_id")?.to_owned(),
                text(&object, "to_id")?.to_owned(),
                mapped(SymbolKind::Edge, text(&object, "type")?)?,
                object.clone(),
            )
        } else {
            let from = text(&object, "node_id")?.to_owned();
            let to = text(&object, "tag_id")?.to_owned();
            let mut edge = Map::new();
            edge.insert(
                "id".into(),
                object.get("id").cloned().unwrap_or(Value::Null),
            );
            edge.insert("from_id".into(), Value::String(from.clone()));
            edge.insert("to_id".into(), Value::String(to.clone()));
            edge.insert("type".into(), Value::String("HAS_TAG".into()));
            edge.insert("lifecycle".into(), Value::String("active".into()));
            (from, to, "HAS_TAG".into(), edge)
        };
        // Every endpoint is an accepted entity of the right type, active or
        // not; a dangling one is a corrupt prefix.
        let (from_type, to_type) = if kind == "edge" {
            ("node", "node")
        } else {
            ("node", "tag")
        };
        for (endpoint, wanted) in [(&from, from_type), (&to, to_type)] {
            let exists: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM _kg_entities WHERE id=?1 AND entity_type=?2 \
                 AND body IS NOT NULL)",
                params![endpoint, wanted],
                |r| r.get(0),
            )?;
            if !exists {
                return Err("dangling accepted endpoint".into());
            }
        }
        // Only edges between projected (active) nodes enter the view.
        let endpoints: Option<(i64, i64)> = connection
            .query_row(
                "SELECT a.native_id,b.native_id FROM _kg_native a,_kg_native b \
                 WHERE a.id=?1 AND b.id=?2",
                params![from, to],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((from, to)) = endpoints {
            connection.execute(
                "INSERT INTO edges(source_id,target_id,type) VALUES (?1,?2,?3)",
                params![from, to, relation],
            )?;
            properties(
                connection,
                true,
                connection.last_insert_rowid(),
                &edge,
                &EDGE_FIELDS,
            )?;
        }
    }
    Ok(())
}

/// The structural `fields` of `object` under their names and its custom
/// `properties` under their `P_` symbols. Nothing else of an image (a
/// tag's label or revision) enters the view. A node's reserved `$` members
/// never become symbols; its `$assertion` fields are projected under their
/// own names, the structural keys `KG-Cypher/1` admits for a node.
fn properties(
    connection: &Connection,
    edge: bool,
    id: i64,
    object: &Map<String, Value>,
    fields: &[&str],
) -> Result<()> {
    for field in fields {
        if let Some(value) = object.get(*field) {
            put_property(connection, edge, id, field, value)?;
        }
    }
    let Some(custom) = object.get("properties") else {
        return Ok(());
    };
    let custom = custom.as_object().ok_or("properties must be object")?;
    for (key, value) in custom {
        if !edge && RESERVED_MEMBERS.contains(&key.as_str()) {
            continue;
        }
        put_property(
            connection,
            edge,
            id,
            &encode(SymbolKind::Property, key)?,
            value,
        )?;
    }
    if edge {
        return Ok(());
    }
    if let Some(assertion) = custom.get(keeper_graph_protocol::assertion::MEMBER) {
        let assertion = assertion.as_object().ok_or("assertion must be object")?;
        for field in ASSERTION_FIELDS {
            if let Some(value) = assertion.get(field) {
                put_property(connection, false, id, field, value)?;
            }
        }
    }
    Ok(())
}

fn put_property(
    connection: &Connection,
    edge: bool,
    id: i64,
    key: &str,
    value: &Value,
) -> Result<()> {
    // Cypher treats missing and explicit null alike. Canonical after-images
    // keep the distinction for typed inspection; a JSON text "null" is not
    // SQL NULL.
    if value.is_null() {
        return Ok(());
    }
    connection.execute(
        "INSERT OR IGNORE INTO property_keys(key) VALUES (?1)",
        [key],
    )?;
    let key_id: i64 =
        connection.query_row("SELECT id FROM property_keys WHERE key=?1", [key], |r| {
            r.get(0)
        })?;
    let (suffix, sql_value) = match value {
        Value::Bool(b) => ("bool", rusqlite::types::Value::Integer(i64::from(*b))),
        Value::Number(n) => {
            let f = n.as_f64().ok_or("invalid number")?;
            if f.fract() == 0.0 {
                ("int", rusqlite::types::Value::Integer(f as i64))
            } else {
                ("real", rusqlite::types::Value::Real(f))
            }
        }
        Value::String(s) => ("text", rusqlite::types::Value::Text(s.clone())),
        _ => (
            "json",
            rusqlite::types::Value::Text(String::from_utf8(canonical_bytes(value)?)?),
        ),
    };
    let (prefix, field) = if edge {
        ("edge", "edge_id")
    } else {
        ("node", "node_id")
    };
    // Table and column names are only the fixed variants above; all data is
    // bound.
    connection.execute(
        &format!("INSERT INTO {prefix}_props_{suffix}({field},key_id,value) VALUES (?1,?2,?3)"),
        params![id, key_id, sql_value],
    )?;
    Ok(())
}

/// The `inspect` role over a published generation: the typed node, edge or
/// content lookup, with the server's `graph.get_node` / `graph.get_edge` /
/// `graph.get_content` rules.
pub fn inspect(
    connection: &Connection,
    request: &keeper_graph_protocol::worker::InspectRequest,
) -> Result<Vec<Value>> {
    use keeper_graph_protocol::worker::InspectEntity;
    let kind = match request.entity {
        InspectEntity::Node => "node",
        InspectEntity::Edge => "edge",
        InspectEntity::Content => "content",
    };
    let body = |id: &str, kind: &str| -> Result<Option<Map<String, Value>>> {
        let body: Option<String> = connection
            .query_row(
                "SELECT body FROM _kg_entities WHERE id=?1 AND entity_type=?2 AND body IS NOT NULL",
                params![id, kind],
                |row| row.get(0),
            )
            .optional()?;
        Ok(body.map(|b| serde_json::from_str(&b)).transpose()?)
    };
    let Some(mut image) = body(&request.id, kind)? else {
        return Ok(Vec::new());
    };
    if !request.include_archived {
        // What decides visibility: the entity, an edge's endpoints, and a
        // content's owner (it has no lifecycle of its own).
        let mut ends = Vec::new();
        let ends_of: &[&str] = match request.entity {
            InspectEntity::Node => {
                ends.push(image.clone());
                &[]
            }
            InspectEntity::Edge => {
                ends.push(image.clone());
                &["from_id", "to_id"]
            }
            InspectEntity::Content => &["owner_node_id"],
        };
        for end in ends_of {
            ends.push(body(text(&image, end)?, "node")?.ok_or("dangling accepted endpoint")?);
        }
        for entity in &ends {
            if !active(entity)? {
                return Ok(Vec::new());
            }
        }
    }
    if request.entity == InspectEntity::Node {
        let mut stmt = connection.prepare(
            "SELECT json_extract(t.body,'$.text') FROM _kg_entities m JOIN _kg_entities t \
             ON t.id = json_extract(m.body,'$.tag_id') AND t.entity_type='tag' AND t.body IS NOT NULL \
             WHERE m.entity_type='node_tag' AND m.body IS NOT NULL \
             AND json_extract(m.body,'$.node_id') = ?1",
        )?;
        let mut tags = stmt
            .query_map([&request.id], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        tags.sort();
        tags.dedup();
        image.insert(
            "tags".into(),
            Value::Array(tags.into_iter().map(Value::String).collect()),
        );
    }
    Ok(vec![Value::Object(image)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use keeper_graph_protocol::worker::FrameOp;
    use serde_json::json;

    fn frame(title: &str) -> EventFrame {
        EventFrame {
            graph_seq: 1,
            event_id: "01JK0000000000000000000001".into(),
            op_count: 1,
            delta_digest: "ab".repeat(32),
            ops: vec![FrameOp {
                ordinal: 0,
                entity_type: EntityType::Node,
                entity_id: "01JK0000000000000000000002".into(),
                operation: Operation::Create,
                after_revision: Some("1".into()),
                after_value: Some(json!({
                    "id": "01JK0000000000000000000002", "kind": "Concept", "title": title,
                    "description": null, "description_source_id": null, "properties": {},
                    "lifecycle": "active", "merged_into_id": null, "revision": "1",
                    "lifecycle_revision": "1"
                })),
            }],
        }
    }

    /// B6/R6: the reviewed value admission holds unchanged — a string of
    /// 65,536 bytes is admitted, one byte more refuses the frame.
    #[test]
    fn the_reviewed_value_limits_hold_at_ingestion() {
        assert!(check(&frame(&"x".repeat(65_536)), 1).is_ok());
        assert_eq!(
            check(&frame(&"x".repeat(65_537)), 1)
                .err()
                .map(|e| e.to_string()),
            Some("string byte limit".to_owned())
        );
        let mut wide = json!({});
        for i in 0..8_193 {
            wide[format!("k{i}")] = json!(0);
        }
        assert!(validate_value(&wide, 0).is_err());
        assert!(validate_value(&json!({"x".repeat(8_193): 0}), 0).is_err());
        let mut deep = json!(0);
        for _ in 0..17 {
            deep = json!([deep]);
        }
        assert!(validate_value(&deep, 0).is_err());
        assert!(validate_value(&json!(9_007_199_254_740_992u64), 0).is_err());
        assert_eq!(
            canonical_bytes(&json!({"b": 1.0, "a": [true, null]})).expect("jcs"),
            br#"{"a":[true,null],"b":1}"#.to_vec()
        );
    }

    #[test]
    fn a_frame_out_of_shape_is_refused() {
        assert!(check(&frame("t"), 2).is_err());
        let mut f = frame("t");
        f.op_count = 2;
        assert!(check(&f, 1).is_err());
        let mut f = frame("t");
        f.ops[0].after_value = Some(json!({"id": "01JK0000000000000000000009"}));
        assert!(check(&f, 1).is_err());
        let mut f = frame("t");
        f.ops[0].entity_type = EntityType::Review;
        assert!(check(&f, 1).is_err());
        // A node image under a content op is not a content image.
        let mut f = frame("t");
        f.ops[0].entity_type = EntityType::Content;
        assert!(check(&f, 1).is_err());
    }
}
