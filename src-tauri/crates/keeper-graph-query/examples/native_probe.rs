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
    let writable = mode == "build" || mode == "oom";
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
        "native-file" => {
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
        _ => return Err("unknown proof mode".into()),
    }
    Ok(())
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn main() {
    std::process::exit(74);
}
