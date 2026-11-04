//! Standalone proof executable, not a mode of the shipped worker.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use keeper_graph_query::{allocation, builder, engine, native};
    use rusqlite::Connection;
    use std::io::Write;
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).ok_or("probe mode required")?;
    match mode.as_str() {
        "wrong-thread" | "live-statement" | "wrong-thread-native" => {
            keeper_graph_query::proof::enter();
            allocation::install_sqlite();
            if mode == "wrong-thread" {
                let result = std::thread::spawn(Connection::open_in_memory).join();
                return Err(format!("wrong-thread callback survived: {result:?}").into());
            }
            let connection = Connection::open_in_memory()?;
            if mode == "live-statement" {
                let _statement = connection.prepare("SELECT 1")?;
                native::register(&connection);
                return Err("live-statement registration survived".into());
            }
            // Registered on main; the native GraphQLite/SQLite path then runs elsewhere.
            native::register(&connection);
            let result = std::thread::spawn(move || {
                connection.query_row(
                    "SELECT row FROM cypher_rows('MATCH (n) RETURN count(*) AS t LIMIT 1','{}')",
                    [],
                    |r| r.get::<_, String>(0),
                )
            })
            .join();
            return Err(format!("wrong-thread native path survived: {result:?}").into());
        }
        "repeat-entry" => {
            keeper_graph_query::proof::enter();
            keeper_graph_query::proof::enter();
            return Err("repeated entry survived".into());
        }
        "concurrent-entry" => {
            keeper_graph_query::proof::enter();
            let result = std::thread::spawn(keeper_graph_query::proof::enter).join();
            return Err(format!("second-thread entry survived: {result:?}").into());
        }
        _ => {}
    }
    let path = std::path::Path::new(args.get(2).ok_or("projection path required")?);
    let writable = matches!(
        mode.as_str(),
        "build"
            | "oom"
            | "native-file-build"
            | "journal-create"
            | "canonical-write-build"
            | "build-fault"
    );
    let connection = engine::start(path, writable)?;
    match mode.as_str() {
        "thread-after-confine" => {
            // Linux: seccomp KillProcess on clone/clone3 ends the whole process (SIGSYS).
            // macOS has no thread filter; the new thread's first callback exits 73.
            let result = std::thread::spawn(Connection::open_in_memory).join();
            return Err(format!("thread after confinement survived: {result:?}").into());
        }
        "build-fault" => {
            // Real SQLite failures at the builder's transaction boundaries, injected
            // through rusqlite's safe hooks; the export arrives on stdin.
            use rusqlite::hooks::{AuthAction, AuthContext, Authorization, TransactionOperation};
            let point = args.get(3).ok_or("fault point")?.clone();
            let target = point.clone();
            connection.authorizer(Some(move |c: AuthContext<'_>| {
                let deny = match (target.as_str(), c.action) {
                    (
                        "savepoint",
                        AuthAction::Savepoint {
                            operation: TransactionOperation::Begin,
                            ..
                        },
                    )
                    | (
                        "release",
                        AuthAction::Savepoint {
                            operation: TransactionOperation::Release,
                            ..
                        },
                    ) => true,
                    ("projection-write", AuthAction::Insert { table_name }) => {
                        table_name == "node_props_text"
                    }
                    _ => false,
                };
                if deny {
                    Authorization::Deny
                } else {
                    Authorization::Allow
                }
            }))?;
            if point == "commit" {
                connection.commit_hook(Some(|| true))?;
            }
            let mut input = std::io::BufReader::new(std::io::stdin());
            let result = builder::build(connection, &mut input);
            writeln!(
                std::io::stdout(),
                "{}",
                serde_json::json!({"point":point,"refused":result.is_err(),"error":result.err().map(|e| e.to_string())})
            )?;
        }
        "startup-transition" => {
            let reader = engine::Reader::new(connection)?;
            let closed = reader.close();
            writeln!(
                std::io::stdout(),
                "{}",
                serde_json::json!({"no_retained_statement":closed.is_ok(),"error":closed.err().map(|e| e.to_string())})
            )?;
        }
        "retained-reprepare" => {
            // R3 on the real GraphQLite path. With GQL_STMT_CACHE=0 the executor runs
            // each MATCH through cypher_executor_helper_stmt, which keeps the prepared
            // statement keyed by its generated SQL and resets it on every exit
            // (executor_match.c:151-209, cypher_executor.c:223-240, :885). The warm-up
            // here is exactly the admitted query's engine text, so that retained, reset
            // statement is the one the query reuses. Under the reduced policy (its
            // node_props_text.value read removed) the reuse must re-prepare, be denied
            // and fail the whole query; under the full policy it answers.
            let policy = args.get(3).ok_or("policy full|reduced")?.clone();
            let source = "MATCH (n:Concept) WHERE n.title = $t RETURN n.title AS t LIMIT 1";
            let parameters = br#"{"t":"same title"}"#;
            let admitted =
                keeper_graph_protocol::admit(keeper_graph_protocol::PROFILE, source, parameters)?;
            let (reads, functions) = keeper_graph_query::proof::production_policy();
            let reads: Vec<_> = reads
                .iter()
                .copied()
                .filter(|read| policy == "full" || *read != ("node_props_text", "value"))
                .collect();
            let reader = keeper_graph_query::proof::reader_with_policy(
                connection,
                &admitted.engine_text(),
                &admitted.parameters_json()?,
                reads,
                functions.to_vec(),
            )?;
            let result = reader.query(&admitted);
            writeln!(
                std::io::stdout(),
                "{}",
                serde_json::json!({
                    "policy": policy,
                    "rows": result.as_ref().ok().map(Vec::len),
                    "error": result.err().map(|e| e.to_string()),
                })
            )?;
        }
        "read-trace" => {
            // Diagnostic only, in this separate executable: replace the worker's exact
            // authorizer with one that allows and records, then run the admitted
            // corpus from stdin to derive the reviewed (table, column) read set.
            use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
            use std::collections::BTreeSet;
            use std::io::BufRead;
            use std::sync::{Arc, Mutex};
            let reader = engine::Reader::new(connection)?;
            let seen = Arc::new(Mutex::new(BTreeSet::new()));
            let record = seen.clone();
            reader
                .connection()
                .authorizer(Some(move |c: AuthContext<'_>| {
                    let entry = match c.action {
                        AuthAction::Read {
                            table_name,
                            column_name,
                        } => format!("read {table_name}.{column_name}"),
                        AuthAction::Function { function_name } => {
                            format!("function {function_name}")
                        }
                        other => format!("{other:?}"),
                    };
                    if let Ok(mut s) = record.lock() {
                        s.insert(entry);
                    }
                    Authorization::Allow
                }))?;
            for line in std::io::stdin().lock().lines() {
                let request: serde_json::Value = serde_json::from_str(&line?)?;
                let source = request["source"].as_str().ok_or("source")?;
                let parameters = serde_json::to_vec(&request["parameters"])?;
                reader.admit_and_query(source, &parameters)?;
            }
            let seen: Vec<String> = seen.lock().map_err(|_| "lock")?.iter().cloned().collect();
            serde_json::to_writer(std::io::stdout(), &seen)?;
        }
        "build" => {
            // Fixture generation mirrors the published layout the query role verifies.
            for sql in keeper_graph_query::schema::KEEPER {
                connection.execute_batch(sql)?;
            }
            connection.execute_batch("INSERT INTO property_keys(key) VALUES ('id'), ('title');
                INSERT INTO nodes(id) VALUES (1),(2),(3);
                INSERT INTO node_labels(node_id,label) VALUES (1,'Concept'),(2,'Concept'),(3,'Concept');
                INSERT INTO node_props_text(node_id,key_id,value) SELECT id,1,printf('%026d',id) FROM nodes;
                INSERT INTO node_props_text(node_id,key_id,value) SELECT id,2,'same title' FROM nodes;
                INSERT INTO edges(id,source_id,target_id,type) VALUES (1,1,2,'contains'),(2,1,2,'contains');
                INSERT INTO edge_props_text(edge_id,key_id,value) SELECT id,1,printf('%026d',id+10) FROM edges;
                COMMIT;")?;
            let _: String = connection.query_row("PRAGMA journal_mode=DELETE", [], |r| r.get(0))?;
            drop(connection);
            writeln!(
                std::io::stdout(),
                "{{\"built\":true,\"pid\":{}}}",
                std::process::id()
            )?;
        }
        "query" => {
            let reader = engine::Reader::new(connection)?;
            let source = args.get(3).ok_or("query source")?;
            let params = args.get(4).map(String::as_bytes).unwrap_or(b"{}");
            let rows = reader.admit_and_query(source, params)?;
            serde_json::to_writer(
                std::io::stdout(),
                &serde_json::json!({"pid":std::process::id(),"allocated":allocation::allocated(),"rows":rows}),
            )?;
        }
        "oom" => {
            let _: String =
                connection.query_row("SELECT hex(zeroblob(200000000))", [], |r| r.get(0))?;
            return Err("allocation over budget survived".into());
        }
        "native-file" | "native-file-build" => {
            let sentinel = args.get(3).ok_or("sentinel path")?;
            if sentinel.contains('\'') {
                return Err("invalid host fixture path".into());
            }
            let cypher = format!("LOAD CSV FROM '{sentinel}' AS row RETURN row LIMIT 1");
            let value: String =
                connection.query_row("SELECT cypher(?1)", [cypher], |r| r.get(0))?;
            writeln!(std::io::stdout(), "{value}")?;
        }
        "network" => {
            let address = args.get(3).ok_or("listener")?;
            if std::net::TcpStream::connect(address).is_ok() {
                return Err("network access succeeded".into());
            }
            writeln!(std::io::stdout(), "{{\"network_denied\":true}}")?;
        }
        "canonical-write" | "canonical-write-build" => {
            let canonical = args.get(3).ok_or("canonical path")?;
            let read = std::fs::read(canonical);
            let file = std::fs::OpenOptions::new().append(true).open(canonical);
            let sqlite = Connection::open(canonical).and_then(|c| {
                c.execute_batch(
                    "CREATE TABLE IF NOT EXISTS stolen(x); INSERT INTO stolen VALUES (1);",
                )
            });
            writeln!(
                std::io::stdout(),
                "{}",
                serde_json::json!({"read_denied":read.is_err(),"file_open_denied":file.is_err(),"sqlite_write_denied":sqlite.is_err()})
            )?;
        }
        "authz" => {
            let reader = engine::Reader::new(connection)?;
            let denied = |code: rusqlite::ffi::Error, message: Option<&str>| {
                // Statement-level denials surface as SQLITE_AUTH. SQLite reports a
                // denied SQLITE_FUNCTION as SQLITE_ERROR with this fixed text,
                // distinct from "no such function" / "wrong number of arguments".
                code.code == rusqlite::ErrorCode::AuthorizationForStatementDenied
                    || message.is_some_and(|m| m.starts_with("not authorized to use function: "))
            };
            let classify = |sql: &str| match reader.connection().prepare(sql) {
                Ok(_) => "allowed".to_owned(),
                Err(rusqlite::Error::SqliteFailure(e, message))
                    if denied(e, message.as_deref()) =>
                {
                    "auth".to_owned()
                }
                Err(rusqlite::Error::SqlInputError { error, ref msg, .. })
                    if denied(error, Some(msg)) =>
                {
                    "auth".to_owned()
                }
                Err(other) => format!("other: {other}"),
            };
            let mut outcomes = serde_json::Map::new();
            for sql in [
                "SELECT regexp('a','a')",
                "SELECT cypher('MATCH (n) RETURN count(*)')",
                "SELECT gql_load_graph()",
                "SELECT gql_unload_graph()",
                "SELECT gql_reload_graph()",
                "SELECT gql_graph_loaded()",
                "SELECT _gql_personalized_pagerank(1,0.85,10)",
                "SELECT _gql_label_propagation(1)",
                "SELECT abs(1)",
                "PRAGMA user_version",
                "PRAGMA mmap_size=1000000",
                "ATTACH DATABASE ':memory:' AS other",
                "DETACH DATABASE other",
                "CREATE TEMP TABLE t(x)",
                "INSERT INTO nodes DEFAULT VALUES",
                "SELECT body FROM _kg_entities",
                "SELECT body FROM _kg_manifest",
                "SELECT sql FROM sqlite_schema",
            ] {
                outcomes.insert(sql.to_owned(), serde_json::Value::String(classify(sql)));
            }
            // Controls: the classifier must not count unrelated errors as denials,
            // and the fixed parent-facing statement must remain preparable.
            let controls = serde_json::json!({
                "missing_function": classify("SELECT kg_no_such_function(1)"),
                "syntax_error": classify("SELEC 1"),
                "fixed_row_statement": classify("SELECT row FROM cypher_rows(?1, ?2)"),
            });
            writeln!(
                std::io::stdout(),
                "{}",
                serde_json::json!({"denied":outcomes,"controls":controls})
            )?;
        }
        "fd-probe" => {
            let mut closed = serde_json::Map::new();
            for fd in args[3..].iter() {
                let fd: i32 = fd.parse()?;
                let reuse = nix::unistd::close(fd);
                closed.insert(
                    fd.to_string(),
                    serde_json::Value::Bool(reuse == Err(nix::errno::Errno::EBADF)),
                );
            }
            writeln!(std::io::stdout(), "{}", serde_json::json!({"ebadf":closed}))?;
        }
        "journal-create" => {
            // Leave MEMORY journal mode; a rollback journal now needs a sidecar create.
            let created = connection.execute_batch(
                "COMMIT; PRAGMA locking_mode=NORMAL; PRAGMA journal_mode=DELETE; BEGIN; INSERT INTO nodes DEFAULT VALUES; COMMIT;",
            );
            let sidecar = std::path::PathBuf::from(format!("{}-journal", path.display())).exists();
            writeln!(
                std::io::stdout(),
                "{}",
                serde_json::json!({"denied":created.is_err(),"sidecar_exists":sidecar})
            )?;
        }
        _ => return Err("unknown proof mode".into()),
    }
    Ok(())
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn main() {
    std::process::exit(74);
}
