//! Standalone proof executable, not a mode of the shipped worker.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use keeper_graph_query::{allocation, engine, native};
    use rusqlite::Connection;
    use std::io::Write;
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).ok_or("probe mode required")?;
    if mode == "wrong-thread" || mode == "live-statement" {
        allocation::mark_main();
        allocation::install_sqlite();
        if mode == "wrong-thread" {
            let result = std::thread::spawn(Connection::open_in_memory).join();
            return Err(format!("wrong-thread callback survived: {result:?}").into());
        }
        let connection = Connection::open_in_memory()?;
        let _statement = connection.prepare("SELECT 1")?;
        native::register(&connection);
        return Err("live-statement registration survived".into());
    }
    let path = std::path::Path::new(args.get(2).ok_or("projection path required")?);
    let writable = matches!(
        mode.as_str(),
        "build" | "oom" | "native-file-build" | "journal-create" | "canonical-write-build"
    );
    let connection = engine::start(path, writable)?;
    match mode.as_str() {
        "build" => {
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
            let mut outcomes = serde_json::Map::new();
            for sql in [
                "SELECT regexp('a','a')",
                "SELECT cypher('MATCH (n) RETURN count(*)')",
                "SELECT gql_load_graph()",
                "SELECT gql_reload_graph()",
                "SELECT _gql_personalized_pagerank(1,0.85,10)",
                "SELECT _gql_label_propagation(1)",
                "PRAGMA user_version",
                "PRAGMA mmap_size=1000000",
                "ATTACH DATABASE ':memory:' AS escape",
                "CREATE TEMP TABLE t(x)",
                "INSERT INTO nodes DEFAULT VALUES",
                "SELECT load_extension('x')",
            ] {
                let denied = reader.connection().prepare(sql).is_err();
                outcomes.insert(sql.to_owned(), serde_json::Value::Bool(denied));
            }
            // Allowed read prepares; a denied nested statement must still fail typed.
            let allowed = reader
                .connection()
                .prepare("SELECT count(*) FROM nodes")
                .is_ok();
            writeln!(
                std::io::stdout(),
                "{}",
                serde_json::json!({"denied":outcomes,"plain_read_allowed":allowed})
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
