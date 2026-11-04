//! Pinned GraphQLite 995a8b9 schema, created and validated by Keeper.
//!
//! Upstream `extension.c:730` ignores `create_schema`'s result, so the builder runs
//! these statements itself inside its checked host transaction before registration
//! (upstream's later `IF NOT EXISTS` DDL is then a no-op), and both roles validate the
//! complete resulting `sqlite_schema` against this list. The strings are verbatim
//! copies of `cypher_schema.c`; a unit test compares them with the vendored source.
use crate::engine::Result;
use rusqlite::Connection;

pub const VERSION: i64 = 1;

/// `cypher_schema_create_tables` then `cypher_schema_create_indexes` order.
pub const GRAPH: [&str; 29] = [
    "CREATE TABLE IF NOT EXISTS nodes (  id INTEGER PRIMARY KEY AUTOINCREMENT)",
    "CREATE TABLE IF NOT EXISTS edges (  id INTEGER PRIMARY KEY AUTOINCREMENT,  source_id INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,  target_id INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,  type TEXT NOT NULL)",
    "CREATE TABLE IF NOT EXISTS property_keys (  id INTEGER PRIMARY KEY AUTOINCREMENT,  key TEXT UNIQUE NOT NULL)",
    "CREATE TABLE IF NOT EXISTS node_labels (  node_id INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,  label TEXT NOT NULL,  PRIMARY KEY (node_id, label))",
    "CREATE TABLE IF NOT EXISTS node_props_int (  node_id INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,  key_id INTEGER NOT NULL REFERENCES property_keys(id),  value INTEGER NOT NULL,  PRIMARY KEY (node_id, key_id))",
    "CREATE TABLE IF NOT EXISTS node_props_text (  node_id INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,  key_id INTEGER NOT NULL REFERENCES property_keys(id),  value TEXT NOT NULL,  PRIMARY KEY (node_id, key_id))",
    "CREATE TABLE IF NOT EXISTS node_props_real (  node_id INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,  key_id INTEGER NOT NULL REFERENCES property_keys(id),  value REAL NOT NULL,  PRIMARY KEY (node_id, key_id))",
    "CREATE TABLE IF NOT EXISTS node_props_bool (  node_id INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,  key_id INTEGER NOT NULL REFERENCES property_keys(id),  value INTEGER NOT NULL CHECK (value IN (0, 1)),  PRIMARY KEY (node_id, key_id))",
    "CREATE TABLE IF NOT EXISTS edge_props_int (  edge_id INTEGER NOT NULL REFERENCES edges(id) ON DELETE CASCADE,  key_id INTEGER NOT NULL REFERENCES property_keys(id),  value INTEGER NOT NULL,  PRIMARY KEY (edge_id, key_id))",
    "CREATE TABLE IF NOT EXISTS edge_props_text (  edge_id INTEGER NOT NULL REFERENCES edges(id) ON DELETE CASCADE,  key_id INTEGER NOT NULL REFERENCES property_keys(id),  value TEXT NOT NULL,  PRIMARY KEY (edge_id, key_id))",
    "CREATE TABLE IF NOT EXISTS edge_props_real (  edge_id INTEGER NOT NULL REFERENCES edges(id) ON DELETE CASCADE,  key_id INTEGER NOT NULL REFERENCES property_keys(id),  value REAL NOT NULL,  PRIMARY KEY (edge_id, key_id))",
    "CREATE TABLE IF NOT EXISTS edge_props_bool (  edge_id INTEGER NOT NULL REFERENCES edges(id) ON DELETE CASCADE,  key_id INTEGER NOT NULL REFERENCES property_keys(id),  value INTEGER NOT NULL CHECK (value IN (0, 1)),  PRIMARY KEY (edge_id, key_id))",
    "CREATE TABLE IF NOT EXISTS node_props_json (  node_id INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,  key_id INTEGER NOT NULL REFERENCES property_keys(id),  value TEXT NOT NULL CHECK (json_valid(value)),  PRIMARY KEY (node_id, key_id))",
    "CREATE TABLE IF NOT EXISTS edge_props_json (  edge_id INTEGER NOT NULL REFERENCES edges(id) ON DELETE CASCADE,  key_id INTEGER NOT NULL REFERENCES property_keys(id),  value TEXT NOT NULL CHECK (json_valid(value)),  PRIMARY KEY (edge_id, key_id))",
    "CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source_id, type)",
    "CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target_id, type)",
    "CREATE INDEX IF NOT EXISTS idx_edges_type ON edges(type)",
    "CREATE INDEX IF NOT EXISTS idx_node_labels_label ON node_labels(label, node_id)",
    "CREATE INDEX IF NOT EXISTS idx_property_keys_key ON property_keys(key)",
    "CREATE INDEX IF NOT EXISTS idx_node_props_int_key_value ON node_props_int(key_id, value, node_id)",
    "CREATE INDEX IF NOT EXISTS idx_node_props_text_key_value ON node_props_text(key_id, value, node_id)",
    "CREATE INDEX IF NOT EXISTS idx_node_props_real_key_value ON node_props_real(key_id, value, node_id)",
    "CREATE INDEX IF NOT EXISTS idx_node_props_bool_key_value ON node_props_bool(key_id, value, node_id)",
    "CREATE INDEX IF NOT EXISTS idx_edge_props_int_key_value ON edge_props_int(key_id, value, edge_id)",
    "CREATE INDEX IF NOT EXISTS idx_edge_props_text_key_value ON edge_props_text(key_id, value, edge_id)",
    "CREATE INDEX IF NOT EXISTS idx_edge_props_real_key_value ON edge_props_real(key_id, value, edge_id)",
    "CREATE INDEX IF NOT EXISTS idx_edge_props_bool_key_value ON edge_props_bool(key_id, value, edge_id)",
    "CREATE INDEX IF NOT EXISTS idx_node_props_json_key_value ON node_props_json(key_id, node_id)",
    "CREATE INDEX IF NOT EXISTS idx_edge_props_json_key_value ON edge_props_json(key_id, edge_id)",
];
/// Keeper's own accepted-history and manifest tables, created only by the builder.
pub const KEEPER: [&str; 4] = [
    "CREATE TABLE _kg_entities(id TEXT PRIMARY KEY, entity_type TEXT NOT NULL, body TEXT)",
    "CREATE TABLE _kg_events(sequence INTEGER PRIMARY KEY, digest TEXT NOT NULL, delta TEXT NOT NULL)",
    "CREATE TABLE _kg_manifest(body TEXT NOT NULL)",
    "CREATE TABLE _kg_native(id TEXT PRIMARY KEY, native_id INTEGER NOT NULL)",
];
const SEQUENCE: &str = "CREATE TABLE sqlite_sequence(name,seq)";
const STATISTICS: &str = "CREATE TABLE sqlite_stat1(tbl,idx,stat)";
/// libsqlite3-sys 0.38.2 `bundled` compiles with SQLITE_ENABLE_STAT4 (its build.rs:164).
const STATISTICS4: &str = "CREATE TABLE sqlite_stat4(tbl,idx,neq,nlt,ndlt,sample)";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Builder after registration: pinned graph schema only.
    Graph,
    /// A published generation: graph schema plus every Keeper table.
    Published,
}

/// Executes every pinned statement and the version stamp, each checked.
pub fn create(connection: &Connection) -> Result<()> {
    for sql in GRAPH {
        connection.execute_batch(sql)?;
    }
    connection.execute_batch(&format!("PRAGMA user_version={VERSION}"))?;
    Ok(())
}

/// The complete `sqlite_schema` must equal the pinned objects: no missing, extra,
/// altered, trigger, view or virtual object. SQLite records CREATE text without the
/// `IF NOT EXISTS` clause.
pub fn verify(connection: &Connection, phase: Phase) -> Result<()> {
    let version: i64 = connection.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version != VERSION {
        return Err("projection schema version".into());
    }
    let mut expected: Vec<(String, String)> = GRAPH
        .iter()
        .map(|sql| {
            let sql = sql
                .replace("CREATE TABLE IF NOT EXISTS ", "CREATE TABLE ")
                .replace("CREATE INDEX IF NOT EXISTS ", "CREATE INDEX ");
            (object_name(&sql).to_owned(), sql)
        })
        .collect();
    expected.push(("sqlite_sequence".into(), SEQUENCE.into()));
    if phase == Phase::Published {
        expected.extend(
            KEEPER
                .iter()
                .map(|sql| (object_name(sql).to_owned(), (*sql).to_owned())),
        );
    }
    let mut statement =
        connection.prepare("SELECT type, name, tbl_name, sql FROM sqlite_schema")?;
    let mut rows = statement.query([])?;
    let mut seen = Vec::new();
    while let Some(row) = rows.next()? {
        let kind: String = row.get(0)?;
        let name: String = row.get(1)?;
        let table: String = row.get(2)?;
        let sql: Option<String> = row.get(3)?;
        match sql {
            // Implicit PRIMARY KEY/UNIQUE indexes of pinned tables carry no SQL.
            None if kind == "index"
                && name.starts_with(&format!("sqlite_autoindex_{table}_"))
                && expected.iter().any(|(n, _)| *n == table) => {}
            Some(sql)
                if (name == "sqlite_stat1" && sql == STATISTICS)
                    || (name == "sqlite_stat4" && sql == STATISTICS4) => {}
            // P6b: a repeated expected name is refused before it is retained, so `seen`
            // never exceeds the fixed expected set.
            Some(sql) if expected.iter().any(|(n, s)| *n == name && *s == sql) => {
                if seen.contains(&name) {
                    return Err("repeated projection schema object".into());
                }
                seen.push(name)
            }
            _ => return Err("unexpected projection schema object".into()),
        }
    }
    if seen.len() != expected.len() {
        return Err("incomplete projection schema".into());
    }
    Ok(())
}

fn object_name(sql: &str) -> &str {
    sql.split(['(', ' '])
        .find(|w| !w.is_empty() && !["CREATE", "TABLE", "INDEX"].contains(w))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The constants must be the vendored upstream statements, character for character.
    #[test]
    fn pinned_statements_match_vendored_upstream_source() {
        let source = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/vendor/graphqlite/src/backend/executor/cypher_schema.c"
        ))
        .expect("vendored schema source");
        // Concatenate adjacent C string literals of each `const char* CYPHER_SCHEMA_*`.
        let mut upstream = Vec::new();
        for definition in source.split("const char* CYPHER_SCHEMA_").skip(1) {
            let body = definition.split(';').next().unwrap_or_default();
            let text: String = body
                .split('"')
                .skip(1)
                .step_by(2)
                .collect::<Vec<_>>()
                .concat();
            upstream.push(text);
        }
        for sql in GRAPH {
            assert!(upstream.iter().any(|u| u == sql), "not upstream: {sql}");
        }
        assert_eq!(upstream.len(), GRAPH.len());
    }

    #[test]
    fn created_schema_verifies_and_any_deviation_is_refused() {
        let fresh = || {
            let c = Connection::open_in_memory().expect("memory");
            create(&c).expect("create");
            c
        };
        let connection = fresh();
        verify(&connection, Phase::Graph).expect("graph phase");
        assert!(
            verify(&connection, Phase::Published).is_err(),
            "missing keeper tables"
        );
        for sql in KEEPER {
            connection.execute_batch(sql).expect("keeper table");
        }
        connection.execute_batch("ANALYZE").expect("analyze");
        verify(&connection, Phase::Published).expect("published phase");
        for deviation in [
            "CREATE TABLE extra(x)",
            "CREATE VIEW v AS SELECT 1",
            "CREATE TRIGGER t AFTER INSERT ON nodes BEGIN SELECT 1; END",
            "DROP INDEX idx_edges_type",
            "CREATE INDEX idx_edges_type2 ON edges(type)",
            "PRAGMA user_version=2",
        ] {
            let connection = fresh();
            for sql in KEEPER {
                connection.execute_batch(sql).expect("keeper table");
            }
            connection.execute_batch(deviation).expect("deviation");
            assert!(
                verify(&connection, Phase::Published).is_err(),
                "{deviation}"
            );
        }
    }

    /// P6b: a second row carrying an expected object's exact name and SQL is refused as
    /// a repeat, not counted toward completeness.
    #[test]
    fn repeated_expected_schema_name_is_refused() {
        let connection = Connection::open_in_memory().expect("memory");
        create(&connection).expect("create");
        for sql in KEEPER {
            connection.execute_batch(sql).expect("keeper table");
        }
        connection.execute_batch("ANALYZE").expect("analyze");
        verify(&connection, Phase::Published).expect("published phase");
        let name = object_name(KEEPER[0]).to_owned();
        connection
            .execute_batch("PRAGMA writable_schema=ON")
            .expect("writable schema");
        connection
            .execute(
                "INSERT INTO sqlite_schema SELECT * FROM sqlite_schema WHERE name=?1",
                [&name],
            )
            .expect("duplicate row");
        let error = verify(&connection, Phase::Published).expect_err("repeat refused");
        assert_eq!(error.to_string(), "repeated projection schema object");
    }
}
