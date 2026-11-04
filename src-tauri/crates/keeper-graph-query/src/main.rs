#[cfg(any(target_os = "linux", target_os = "macos"))]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    use keeper_graph_protocol::wire::{InspectRequest, Manifest, QueryRequest, Response};
    use keeper_graph_query::{allocation, builder, engine};
    let mut args = std::env::args_os().skip(1);
    let role = args.next().ok_or("worker role missing")?;
    let path = std::path::PathBuf::from(args.next().ok_or("worker generation missing")?);
    if args.next().is_some() {
        return Err("worker extra arguments".into());
    }
    let writable = role == "build";
    if !writable && role != "query" && role != "inspect" {
        return Err("worker role unsupported".into());
    }
    let connection = engine::start(&path, writable)?;
    let mut input = std::io::BufReader::new(std::io::stdin());
    let response = if writable {
        let manifest = builder::build(connection, &mut input)?;
        Response {
            manifest,
            pid: std::process::id(),
            allocated_bytes: allocation::allocated(),
            rows: Vec::new(),
        }
    } else {
        let manifest: Manifest = serde_json::from_str(&connection.query_row::<String, _, _>(
            "SELECT body FROM _kg_manifest",
            [],
            |r| r.get(0),
        )?)?;
        manifest.validate()?;
        let rows = if role == "inspect" {
            use rusqlite::OptionalExtension;
            let request: InspectRequest = serde_json::from_slice(&builder::frame(&mut input, 8192)?)?;
            if !keeper_graph_protocol::wire::ulid(&request.id) { return Err("invalid inspection id".into()); }
            let body: Option<String> = connection.query_row(
                "SELECT body FROM _kg_entities WHERE id=?1 AND body IS NOT NULL AND (?2 OR coalesce(json_extract(body,'$.lifecycle'),'active')='active')",
                rusqlite::params![request.id, request.include_archived], |row|row.get(0)).optional()?;
            body.map(|body|serde_json::from_str(&body)).transpose()?.into_iter().collect()
        } else {
            let reader = engine::Reader::new(connection)?;
            let bytes = builder::frame(&mut input, 8192 + 65536 + 8192)?;
            let request: QueryRequest = serde_json::from_slice(&bytes)?;
            reader.admit_and_query(&request.source, request.parameters.as_bytes())?
        };
        Response {
            manifest,
            pid: std::process::id(),
            allocated_bytes: allocation::allocated(),
            rows,
        }
    };
    let bytes = serde_json::to_vec(&response)?;
    if bytes.len() > keeper_graph_protocol::MAX_RESULT_BYTES {
        return Err("query_limit".into());
    }
    use std::io::Write;
    std::io::stdout().write_all(&bytes)?;
    Ok(())
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn main() {
    if run().is_err() {
        std::process::exit(74);
    }
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn main() {
    std::process::exit(74);
}
