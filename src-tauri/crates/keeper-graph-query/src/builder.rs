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
//! independent.

use crate::engine::Result;
use keeper_graph_protocol::symbol::{encode, SymbolKind};
use keeper_graph_protocol::value::canonical;
use keeper_graph_protocol::vocabulary::{
    EntityType, Operation, EDGE_FIELDS, NODE_FIELDS, TAG_FIELDS,
};
use keeper_graph_protocol::worker::{
    chain, EventFrame, Manifest, GENESIS, MAX_ENTITIES, MAX_EVENT_BYTES, MAX_EXPORT_BYTES,
    MAX_MANIFEST_BYTES,
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
        EntityType::Content | EntityType::Review => {
            return Err("entity type outside schema 6".into())
        }
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
        event.check(sequence)?;
        digest = chain(&digest, sequence, &event.delta_digest);
        connection.execute_batch("SAVEPOINT accepted_event")?;
        for op in &event.ops {
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
        Some(value) => Some(String::from_utf8(canonical(value)?)?),
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
/// tag's label or revision) enters the view.
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
    if let Some(custom) = object.get("properties") {
        for (key, value) in custom.as_object().ok_or("properties must be object")? {
            put_property(
                connection,
                edge,
                id,
                &encode(SymbolKind::Property, key)?,
                value,
            )?;
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
            rusqlite::types::Value::Text(String::from_utf8(canonical(value)?)?),
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

/// The `inspect` role over a published generation: the typed node or edge
/// lookup, with the server's `graph.get_node` / `graph.get_edge` rules.
pub fn inspect(
    connection: &Connection,
    request: &keeper_graph_protocol::worker::InspectRequest,
) -> Result<Vec<Value>> {
    use keeper_graph_protocol::worker::InspectEntity;
    let kind = match request.entity {
        InspectEntity::Node => "node",
        InspectEntity::Edge => "edge",
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
        let mut ends = vec![image.clone()];
        if request.entity == InspectEntity::Edge {
            for end in ["from_id", "to_id"] {
                ends.push(body(text(&image, end)?, "node")?.ok_or("dangling accepted endpoint")?);
            }
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
