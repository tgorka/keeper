use crate::engine::Result;
use keeper_graph_protocol::wire::{
    canonical_bytes, EntityType, Event, Manifest, MAX_ENTITIES, MAX_EVENT_BYTES, MAX_EXPORT_BYTES,
};
use keeper_graph_protocol::{custom_property, encode_symbol, SymbolKind};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{Map, Value};
use std::io::{BufRead, Read};

pub fn frame<R: BufRead>(reader: &mut R, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    Read::take(reader, limit as u64 + 1).read_until(b'\n', &mut bytes)?;
    if bytes.is_empty() || bytes.len() > limit || bytes.pop() != Some(b'\n') {
        return Err("bounded frame refused".into());
    }
    Ok(bytes)
}
pub fn build<R: BufRead>(connection: Connection, reader: &mut R) -> Result<Manifest> {
    let first = frame(reader, 8192)?;
    let manifest: Manifest = serde_json::from_slice(&first)?;
    manifest.validate()?;
    connection.execute_batch("PRAGMA max_page_count=524288")?;
    for sql in &crate::schema::KEEPER[..3] {
        connection.execute_batch(sql)?;
    }
    let mut total = first.len() as u64;
    for sequence in 1..=manifest.accepted_head {
        let bytes = frame(reader, MAX_EVENT_BYTES)?;
        total += bytes.len() as u64 + 1;
        if total > MAX_EXPORT_BYTES {
            return Err("export byte limit".into());
        }
        let event: Event = serde_json::from_slice(&bytes)?;
        event.validate(sequence)?;
        connection.execute_batch("SAVEPOINT accepted_event")?;
        for op in &event.operations {
            let previous: Option<(String, Option<String>)> = connection
                .query_row(
                    "SELECT entity_type, body FROM _kg_entities WHERE id=?1",
                    [&op.entity_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            let expected = op.before.as_ref().map(canonical_bytes).transpose()?;
            if previous
                .as_ref()
                .map(|p| p.0.as_str())
                .is_some_and(|kind| kind != op.entity_type.as_str())
                || previous
                    .as_ref()
                    .and_then(|p| p.1.as_deref())
                    .map(str::as_bytes)
                    != expected.as_deref()
            {
                return Err("accepted before-image mismatch".into());
            }
            if let Some(after) = &op.after {
                validate_entity(op.entity_type, &op.entity_id, after)?;
            }
            let after = op.after.as_ref().map(canonical_bytes).transpose()?;
            let after = after.map(String::from_utf8).transpose()?;
            connection.execute("INSERT INTO _kg_entities(id,entity_type,body) VALUES (?1,?2,?3) ON CONFLICT(id) DO UPDATE SET body=excluded.body",
                params![op.entity_id,op.entity_type.as_str(),after])?;
        }
        connection.execute(
            "INSERT INTO _kg_events(sequence,digest,delta) VALUES (?1,?2,?3)",
            params![
                i64::try_from(sequence)?,
                event.digest,
                std::str::from_utf8(&bytes)?
            ],
        )?;
        connection.execute_batch("RELEASE accepted_event")?;
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
            custom_property(key)?;
            property_depth(value, 0)?;
        }
    }
    match kind {
        EntityType::Node | EntityType::Edge => {
            if !matches!(text(object, "lifecycle")?, "active" | "archived") {
                return Err("invalid lifecycle".into());
            }
            let revision = text(object, "revision")?;
            if revision.is_empty()
                || revision.len() > 20
                || !revision.bytes().all(|b| b.is_ascii_digit())
            {
                return Err("invalid revision".into());
            }
            if kind == EntityType::Node {
                mapped(SymbolKind::Node, text(object, "kind")?)?;
            } else {
                mapped(SymbolKind::Edge, text(object, "type")?)?;
                for key in ["from_id", "to_id"] {
                    if !keeper_graph_protocol::wire::ulid(text(object, key)?) {
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
                if !keeper_graph_protocol::wire::ulid(text(object, key)?) {
                    return Err("invalid membership endpoint".into());
                }
            }
        }
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
fn text<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a str> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing structural field {key}").into())
}
fn mapped(kind: SymbolKind, name: &str) -> Result<String> {
    if (kind == SymbolKind::Node
        && matches!(
            name,
            "Concept" | "Collection" | "Resource" | "Tag" | "Guidance"
        ))
        || (kind == SymbolKind::Edge
            && matches!(name, "HAS_TAG" | "contains" | "guidance_scope" | "sources"))
    {
        Ok(name.into())
    } else {
        Ok(encode_symbol(kind, name)?)
    }
}
fn project(connection: &Connection) -> Result<()> {
    connection.execute_batch(crate::schema::KEEPER[3])?;
    let mut stmt=connection.prepare("SELECT id,entity_type,body FROM _kg_entities WHERE body IS NOT NULL AND (entity_type='tag' OR (entity_type='node' AND json_extract(body,'$.lifecycle')='active')) ORDER BY id")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let kind: String = row.get(1)?;
        let body: String = row.get(2)?;
        let object: Map<String, Value> = serde_json::from_str(&body)?;
        connection.execute("INSERT INTO nodes DEFAULT VALUES", [])?;
        let native = connection.last_insert_rowid();
        connection.execute(
            "INSERT INTO _kg_native(id,native_id) VALUES (?1,?2)",
            params![id, native],
        )?;
        let label = if kind == "tag" {
            "Tag".into()
        } else {
            mapped(SymbolKind::Node, text(&object, "kind")?)?
        };
        connection.execute(
            "INSERT INTO node_labels(node_id,label) VALUES (?1,?2)",
            params![native, label],
        )?;
        properties(connection, false, native, &object)?;
    }
    drop(rows);
    drop(stmt);
    let mut stmt=connection.prepare("SELECT entity_type,body FROM _kg_entities WHERE body IS NOT NULL AND entity_type IN ('edge','node_tag') ORDER BY id")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let kind: String = row.get(0)?;
        let body: String = row.get(1)?;
        let mut object: Map<String, Value> = serde_json::from_str(&body)?;
        let (from, to, relation) = if kind == "edge" {
            if text(&object, "lifecycle")? != "active" {
                continue;
            }
            (
                text(&object, "from_id")?.to_owned(),
                text(&object, "to_id")?.to_owned(),
                mapped(SymbolKind::Edge, text(&object, "type")?)?,
            )
        } else {
            let from = text(&object, "node_id")?.to_owned();
            let to = text(&object, "tag_id")?.to_owned();
            object.insert("from_id".into(), Value::String(from.clone()));
            object.insert("to_id".into(), Value::String(to.clone()));
            object.insert("type".into(), Value::String("HAS_TAG".into()));
            object.insert("lifecycle".into(), Value::String("active".into()));
            (from, to, "HAS_TAG".into())
        };
        // Archived endpoints retain canonical entities but never enter active traversal.
        for endpoint in [&from, &to] {
            let exists:bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM _kg_entities WHERE id=?1 AND entity_type IN ('node','tag') AND body IS NOT NULL)",[endpoint],|r|r.get(0))?;
            if !exists {
                return Err("dangling accepted endpoint".into());
            }
        }
        let endpoints:Option<(i64,i64)>=connection.query_row("SELECT a.native_id,b.native_id FROM _kg_native a,_kg_native b WHERE a.id=?1 AND b.id=?2",params![from,to],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        if let Some((from, to)) = endpoints {
            connection.execute(
                "INSERT INTO edges(source_id,target_id,type) VALUES (?1,?2,?3)",
                params![from, to, relation],
            )?;
            properties(connection, true, connection.last_insert_rowid(), &object)?;
        }
    }
    Ok(())
}
fn properties(
    connection: &Connection,
    edge: bool,
    id: i64,
    object: &Map<String, Value>,
) -> Result<()> {
    for (key, value) in object {
        if key == "properties" {
            for (key, value) in value.as_object().ok_or("properties must be object")? {
                put_property(connection, edge, id, &custom_property(key)?, value)?;
            }
        } else {
            put_property(connection, edge, id, key, value)?;
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
    // Cypher treats missing and explicit null alike. Canonical after-images keep
    // the distinction for typed inspection; a JSON text "null" is not SQL NULL.
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
    // Table/column names are solely the fixed variants above; all data is bound.
    connection.execute(
        &format!("INSERT INTO {prefix}_props_{suffix}({field},key_id,value) VALUES (?1,?2,?3)"),
        params![id, key_id, sql_value],
    )?;
    Ok(())
}
