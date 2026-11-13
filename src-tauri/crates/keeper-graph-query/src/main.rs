#[cfg(any(target_os = "linux", target_os = "macos"))]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    use keeper_graph_protocol::worker::{
        InspectRequest, Manifest, QueryRequest, Response, MAX_INSPECT_FRAME_BYTES,
        MAX_QUERY_FRAME_BYTES,
    };
    use keeper_graph_query::{allocation, boundary, builder, engine};
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
    let (manifest, rows) = if writable {
        (builder::build(connection, &mut input)?, Vec::new())
    } else {
        let manifest: Manifest = serde_json::from_str(&connection.query_row::<String, _, _>(
            "SELECT body FROM _kg_manifest",
            [],
            |r| r.get(0),
        )?)?;
        manifest.validate()?;
        let rows = if role == "inspect" {
            let request: InspectRequest =
                serde_json::from_slice(&builder::frame(&mut input, MAX_INSPECT_FRAME_BYTES)?)?;
            if !keeper_graph_protocol::is_ulid(&request.id) {
                return Err("invalid inspection id".into());
            }
            builder::inspect(&connection, &request)?
        } else {
            let reader = engine::Reader::new(connection)?;
            let request: QueryRequest =
                serde_json::from_slice(&builder::frame(&mut input, MAX_QUERY_FRAME_BYTES)?)?;
            reader.admit_and_query(&request.source, &request.parameters_bytes()?)?
        };
        (manifest, rows)
    };
    let response = Response {
        manifest,
        pid: std::process::id(),
        allocated_bytes: allocation::allocated(),
        peak_allocated_bytes: allocation::peak_allocated(),
        peak_rss_bytes: peak_rss()?,
        boundary_vm_data_bytes: boundary::recorded().0,
        boundary_vm_peak_bytes: boundary::recorded().1,
        rows,
    };
    let bytes = serde_json::to_vec(&response)?;
    if bytes.len() > keeper_graph_protocol::profile::MAX_RESULT_BYTES {
        return Err("query_limit".into());
    }
    use std::io::Write;
    std::io::stdout().write_all(&bytes)?;
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn peak_rss() -> Result<u64, Box<dyn std::error::Error>> {
    use nix::sys::resource::{getrusage, UsageWho};
    let max = u64::try_from(getrusage(UsageWho::RUSAGE_SELF)?.max_rss())?;
    // Linux reports KiB, macOS bytes.
    Ok(if cfg!(target_os = "linux") {
        max * 1024
    } else {
        max
    })
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn main() {
    use keeper_graph_protocol::worker::{EXIT_LIMIT, EXIT_REFUSED};
    keeper_graph_query::engine::abort_on_panic();
    match run() {
        Ok(()) => {}
        // 75: bounded result/row limit; 72 stays reserved for allocation budget exits.
        Err(error) if error.to_string() == "query_limit" => std::process::exit(EXIT_LIMIT),
        Err(_) => std::process::exit(EXIT_REFUSED),
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn main() {
    std::process::exit(74);
}
