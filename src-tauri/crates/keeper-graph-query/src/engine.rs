use keeper_graph_protocol::{admit, AdmittedQuery, MAX_RESULT_BYTES, PROFILE};
use rusqlite::{Connection, OpenFlags};
use std::path::Path;
use std::sync::{Arc, Mutex};
pub type Result<T, E = Box<dyn std::error::Error>> = std::result::Result<T, E>;

/// The worker's single entry. Claims allocation-callback permission for the calling
/// thread exactly once per process (a second claim from any thread exits 73).
pub fn start(path: &Path, writable: bool) -> Result<Connection> {
    crate::allocation::enter();
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
        // Checked pinned DDL inside the host transaction; upstream's own unchecked
        // creation during registration then finds every object already present.
        crate::schema::create(&connection)?;
    } else {
        connection.execute_batch("PRAGMA query_only=ON;")?;
    }
    crate::native::register(&connection);
    crate::schema::verify(
        &connection,
        if writable {
            crate::schema::Phase::Graph
        } else {
            crate::schema::Phase::Published
        },
    )?;
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
/// Exact `(table, column)` reads the native engine performs for the admitted profile,
/// derived with `native_probe read-trace` over the full positive corpus (oracle,
/// boundary, limits, benchmark and memory queries). `("nodes", "")` is the rowid
/// read behind count(*). Re-derive and review if the engine or profile changes.
/// Anything else, including any column of an unlisted table, is denied.
const READS: &[(&str, &str)] = &[
    ("cypher_rows", "row"),
    ("edge_props_bool", "edge_id"),
    ("edge_props_bool", "key_id"),
    ("edge_props_bool", "value"),
    ("edge_props_int", "edge_id"),
    ("edge_props_int", "key_id"),
    ("edge_props_int", "value"),
    ("edge_props_json", "edge_id"),
    ("edge_props_json", "key_id"),
    ("edge_props_json", "value"),
    ("edge_props_real", "edge_id"),
    ("edge_props_real", "key_id"),
    ("edge_props_real", "value"),
    ("edge_props_text", "edge_id"),
    ("edge_props_text", "key_id"),
    ("edge_props_text", "value"),
    ("edges", "id"),
    ("edges", "source_id"),
    ("edges", "target_id"),
    ("edges", "type"),
    ("node_labels", "label"),
    ("node_labels", "node_id"),
    ("node_props_bool", "key_id"),
    ("node_props_bool", "node_id"),
    ("node_props_bool", "value"),
    ("node_props_int", "key_id"),
    ("node_props_int", "node_id"),
    ("node_props_int", "value"),
    ("node_props_json", "key_id"),
    ("node_props_json", "node_id"),
    ("node_props_json", "value"),
    ("node_props_real", "key_id"),
    ("node_props_real", "node_id"),
    ("node_props_real", "value"),
    ("node_props_text", "key_id"),
    ("node_props_text", "node_id"),
    ("node_props_text", "value"),
    ("nodes", ""),
    ("nodes", "id"),
    ("property_keys", "id"),
    ("property_keys", "key"),
];
/// Exact functions the generated SQL calls; every other function is denied.
const FUNCTIONS: &[&str] = &[
    "_gql_bool",
    "_gql_bool_str",
    "_gql_order_cmp",
    "_gql_order_key",
    "_gql_order_rank",
    "_gql_typetag",
    "coalesce",
    "count",
    "json",
];
/// Fixed trusted warm-up: completes GraphQLite's lazy executor/schema initialization
/// before any caller input, with immutable/read-only/query_only already in force.
const WARMUP: &str = "MATCH (n) RETURN count(*) AS total LIMIT 1";

#[cfg(feature = "proof-executables")]
pub(crate) fn policy() -> (
    &'static [(&'static str, &'static str)],
    &'static [&'static str],
) {
    (READS, FUNCTIONS)
}

pub struct Reader {
    connection: Connection,
    denied: Arc<Mutex<Option<String>>>,
}
impl Reader {
    pub fn new(connection: Connection) -> Result<Self> {
        Self::with_policy(connection, WARMUP, "{}", READS.to_vec(), FUNCTIONS.to_vec())
    }
    /// Transition invariant (R3). The outer warm-up statement is fully stepped and
    /// finalized before the policy is installed. GraphQLite keeps the MATCH statement it
    /// generated as a helper statement keyed by SQL text (`GQL_STMT_CACHE=0` disables
    /// capture, `cypher_executor.c:885`) and resets it on every exit path
    /// (`executor_match.c:151-209`), so nothing is running at the transition; the
    /// schema manager's write slots are reached only by write paths outside the profile.
    /// Installing the authorizer expires every prepared statement
    /// (`sqlite3_set_authorizer` -> `sqlite3ExpirePreparedStatements`), so a retained
    /// statement re-prepares under this exact policy before it can run again, and any
    /// denial during that re-preparation is recorded and fails the whole query.
    pub(crate) fn with_policy(
        connection: Connection,
        warmup: &str,
        warmup_parameters: &str,
        reads: Vec<(&'static str, &'static str)>,
        functions: Vec<&'static str>,
    ) -> Result<Self> {
        if std::env::var("GQL_STMT_CACHE").as_deref() != Ok("0") {
            return Err("statement cache must be disabled".into());
        }
        {
            let mut stmt = connection.prepare("SELECT row FROM cypher_rows(?1, ?2)")?;
            let mut rows = stmt.query([warmup, warmup_parameters])?;
            while rows.next()?.is_some() {}
        }
        let denied = Arc::new(Mutex::new(None));
        let refusal = denied.clone();
        connection.authorizer(Some(move |context: rusqlite::hooks::AuthContext<'_>| {
            use rusqlite::hooks::{AuthAction, Authorization};
            let allowed = match context.action {
                AuthAction::Select | AuthAction::Recursive => true,
                AuthAction::Read {
                    table_name,
                    column_name,
                } => reads.contains(&(table_name, column_name)),
                AuthAction::Function { function_name } => functions.contains(&function_name),
                _ => false,
            };
            if allowed {
                Authorization::Allow
            } else {
                if let Ok(mut first) = refusal.lock() {
                    // Bounded: fixed-format action text, first refusal only.
                    first.get_or_insert_with(|| {
                        let mut text = format!("{:?}", context.action);
                        text.truncate(160);
                        text
                    });
                }
                Authorization::Deny
            }
        }))?;
        Ok(Self { connection, denied })
    }
    /// `sqlite3_close` (not close_v2) reports SQLITE_BUSY while GraphQLite still holds
    /// prepared slot statements; recorded as an observation, not a safety failure.
    pub fn close(self) -> Result<()> {
        self.connection.close().map_err(|(_, e)| e)?;
        Ok(())
    }
    pub fn query(&self, query: &AdmittedQuery) -> Result<Vec<serde_json::Value>> {
        if let Ok(mut first) = self.denied.lock() {
            *first = None;
        }
        let result = self.execute(query);
        match self.denied.lock().ok().and_then(|mut d| d.take()) {
            // Upstream may ignore an internal preparation error and return empty rows.
            // A denied nested statement always invalidates the whole result.
            Some(action) => Err(format!("native internal authorization failure: {action}").into()),
            None => result,
        }
    }
    fn execute(&self, query: &AdmittedQuery) -> Result<Vec<serde_json::Value>> {
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

#[cfg(test)]
mod tests {
    //! R3 transition invariant on the pinned rusqlite/SQLite, independent of GraphQLite:
    //! installing the final authorizer re-authorizes every *inactive* prepared
    //! statement before it runs again, while a *running* one may finish (advisory
    //! expiry), which is why `Reader::with_policy` drains its warm-up first.
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use rusqlite::Connection;

    fn fixture() -> Connection {
        let c = Connection::open_in_memory().expect("memory");
        c.execute_batch(
            "CREATE TABLE secret(x); CREATE TABLE open(y); INSERT INTO open VALUES (1),(2);",
        )
        .expect("fixture");
        c
    }
    fn final_policy(c: &Connection) {
        c.authorizer(Some(|ctx: AuthContext<'_>| match ctx.action {
            AuthAction::Select => Authorization::Allow,
            AuthAction::Read {
                table_name: "open", ..
            } => Authorization::Allow,
            _ => Authorization::Deny,
        }))
        .expect("authorizer");
    }

    #[test]
    fn inactive_statements_prepared_before_the_final_policy_are_reauthorized() {
        let c = fixture();
        let mut write = c.prepare("INSERT INTO secret VALUES (1)").expect("write");
        let mut hidden = c.prepare("SELECT x FROM secret").expect("hidden read");
        let mut allowed = c
            .prepare("SELECT y FROM open ORDER BY y")
            .expect("allowed read");
        final_policy(&c);
        let write_result = write.execute([]);
        let hidden_result = hidden.query_row([], |r| r.get::<_, i64>(0));
        let rows: Vec<i64> = allowed
            .query_map([], |r| r.get(0))
            .expect("allowed retained read")
            .collect::<Result<_, _>>()
            .expect("rows");
        drop((write, hidden, allowed));
        c.authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
            .expect("clear");
        let written: i64 = c
            .query_row("SELECT count(*) FROM secret", [], |r| r.get(0))
            .expect("count");
        assert!(
            write_result.is_err(),
            "privileged write ran: {write_result:?}"
        );
        assert!(hidden_result.is_err(), "denied read ran: {hidden_result:?}");
        assert_eq!(written, 0, "denied write had an effect");
        assert_eq!(rows, vec![1, 2]);
    }

    #[test]
    fn a_running_statement_finishes_under_the_old_policy_so_warmup_must_drain() {
        let c = fixture();
        let mut running = c.prepare("SELECT y FROM open ORDER BY y").expect("read");
        let mut rows = running.query([]).expect("query");
        let first: i64 = rows.next().expect("step").expect("row").get(0).expect("y");
        c.authorizer(Some(|_: AuthContext<'_>| Authorization::Deny))
            .expect("deny all");
        let second = rows.next().expect("advisory expiry lets it continue");
        assert_eq!(first, 1);
        assert_eq!(second.map(|r| r.get::<_, i64>(0).expect("y")), Some(2));
    }
}
