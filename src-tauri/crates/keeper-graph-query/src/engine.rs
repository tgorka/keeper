use keeper_graph_protocol::{admit, AdmittedQuery, MAX_RESULT_BYTES, PROFILE};
use rusqlite::{Connection, OpenFlags};
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
pub type Result<T, E = Box<dyn std::error::Error>> = std::result::Result<T, E>;

pub fn start(path: &Path, writable: bool) -> Result<Connection> {
    crate::allocation::mark_main();
    // Fail closed if the parent passed anything but the fixed non-secret setting.
    if std::env::vars_os().any(|(key, _)| key != "GQL_STMT_CACHE") {
        return Err("worker environment must be cleared".into());
    }
    crate::confinement::close_inherited()?;
    crate::confinement::limit_resources()?;
    #[cfg(target_os = "linux")]
    crate::confinement::restrict(path, writable)?;
    // On macOS the parent installs the constant sandbox-exec profile before exec.
    // There is no environment flag pretending to prove that OS boundary.
    crate::allocation::install_sqlite();
    let connection = if writable {
        Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?
    } else {
        Connection::open_with_flags(
            immutable_uri(path)?,
            OpenFlags::SQLITE_OPEN_READ_ONLY
                | OpenFlags::SQLITE_OPEN_URI
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?
    };
    connection.set_limit(rusqlite::limits::Limit::SQLITE_LIMIT_WORKER_THREADS, 0)?;
    connection.execute_batch("PRAGMA mmap_size=0; PRAGMA temp_store=MEMORY; PRAGMA threads=0;")?;
    if writable {
        let mode: String = connection.query_row("PRAGMA journal_mode=MEMORY", [], |r| r.get(0))?;
        if mode != "memory" {
            return Err("memory journal refused".into());
        }
        connection.execute_batch("PRAGMA locking_mode=EXCLUSIVE; BEGIN IMMEDIATE;")?;
    } else {
        connection.execute_batch("PRAGMA query_only=ON;")?;
    }
    crate::native::register(&connection);
    verify_schema(&connection)?;
    Ok(connection)
}
fn immutable_uri(path: &Path) -> Result<String> {
    use std::fmt::Write;
    let path = path.to_str().ok_or("non-UTF8 projection path")?;
    let mut uri = String::from("file:");
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            uri.push(char::from(b));
        } else {
            write!(uri, "%{b:02X}")?;
        }
    }
    uri.push_str("?mode=ro&immutable=1");
    Ok(uri)
}
pub fn verify_schema(connection: &Connection) -> Result<()> {
    let version: i64 = connection.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version != 1 {
        return Err("projection schema version".into());
    }
    for table in [
        "nodes",
        "edges",
        "property_keys",
        "node_labels",
        "node_props_int",
        "node_props_real",
        "node_props_text",
        "node_props_bool",
        "node_props_json",
        "edge_props_int",
        "edge_props_real",
        "edge_props_text",
        "edge_props_bool",
        "edge_props_json",
    ] {
        let sql: String = connection.query_row(
            "SELECT sql FROM sqlite_schema WHERE type='table' AND name=?1",
            [table],
            |r| r.get(0),
        )?;
        if sql.to_ascii_uppercase().contains("VIRTUAL") {
            return Err("unexpected virtual schema".into());
        }
    }
    let unexpected: i64 = connection.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE type IN ('trigger','view')",
        [],
        |r| r.get(0),
    )?;
    if unexpected != 0 {
        return Err("unexpected projection executable schema".into());
    }
    Ok(())
}

pub struct Reader {
    connection: Connection,
    denied: Arc<AtomicBool>,
}
impl Reader {
    pub fn new(connection: Connection) -> Result<Self> {
        // Fixed trusted startup before caller input; no writes are allowed by the
        // immutable descriptor/query_only. Upstream lazy initialization runs here.
        if std::env::var("GQL_STMT_CACHE").as_deref() != Ok("0") {
            return Err("statement cache must be disabled".into());
        }
        {
            let mut stmt = connection.prepare(
                "SELECT row FROM cypher_rows('MATCH (n) RETURN count(*) AS total LIMIT 1', '{}')",
            )?;
            let mut rows = stmt.query([])?;
            while rows.next()?.is_some() {}
        }
        let denied = Arc::new(AtomicBool::new(false));
        let refusal = denied.clone();
        connection.authorizer(Some(move |context: rusqlite::hooks::AuthContext<'_>| {
            use rusqlite::hooks::{AuthAction, Authorization};
            let allowed = match context.action {
                AuthAction::Select | AuthAction::Recursive => true,
                AuthAction::Read { table_name, .. } => matches!(
                    table_name,
                    "nodes"
                        | "edges"
                        | "property_keys"
                        | "node_labels"
                        | "node_props_int"
                        | "node_props_real"
                        | "node_props_text"
                        | "node_props_bool"
                        | "node_props_json"
                        | "edge_props_int"
                        | "edge_props_real"
                        | "edge_props_text"
                        | "edge_props_bool"
                        | "edge_props_json"
                        | "cypher_rows"
                        | "sqlite_master"
                        | "sqlite_schema"
                ),
                AuthAction::Function { function_name } => matches!(
                    function_name,
                    "count"
                        | "coalesce"
                        | "json_extract"
                        | "json_type"
                        | "json"
                        | "typeof"
                        | "_gql_bool"
                        | "_gql_bool_str"
                        | "_gql_typetag"
                        | "_gql_retag"
                        | "_gql_cmp"
                        | "_gql_order_rank"
                        | "_gql_order_key"
                        | "_gql_order_cmp"
                ),
                _ => false,
            };
            if allowed {
                Authorization::Allow
            } else {
                refusal.store(true, Ordering::Relaxed);
                Authorization::Deny
            }
        }))?;
        Ok(Self { connection, denied })
    }
    pub fn query(&self, query: &AdmittedQuery) -> Result<Vec<serde_json::Value>> {
        self.denied.store(false, Ordering::Relaxed);
        let mut stmt = self
            .connection
            .prepare("SELECT row FROM cypher_rows(?1, ?2)")?;
        let text = query.engine_text();
        let params = query.parameters_json()?;
        let mut rows = stmt.query([text, params])?;
        let mut output = Vec::new();
        let mut bytes = 0;
        while let Some(row) = rows.next()? {
            let raw = row.get_ref(0)?.as_str()?;
            bytes += raw.len();
            if bytes > MAX_RESULT_BYTES || output.len() == query.limit() {
                return Err("query_limit".into());
            }
            let values: serde_json::Map<String, serde_json::Value> = serde_json::from_str(raw)?;
            let mut normalized = serde_json::Map::new();
            for (i, alias) in query.aliases().iter().enumerate() {
                let value = values
                    .get(&format!("a{i}"))
                    .ok_or("native result column mismatch")?;
                let scalar: keeper_graph_protocol::Scalar = serde_json::from_value(value.clone())?;
                normalized.insert(alias.clone(), serde_json::to_value(scalar)?);
            }
            output.push(serde_json::Value::Object(normalized));
        }
        // Upstream may ignore an internal preparation error and return empty rows.
        // A denied nested statement always invalidates the whole result.
        if self.denied.load(Ordering::Relaxed) {
            return Err("native internal authorization failure".into());
        }
        Ok(output)
    }
    pub fn admit_and_query(
        &self,
        source: &str,
        parameters: &[u8],
    ) -> Result<Vec<serde_json::Value>> {
        self.query(&admit(PROFILE, source, parameters)?)
    }
    pub fn connection(&self) -> &Connection {
        &self.connection
    }
}
