//! Proof-only stand-in for the worker executable; never shipped or selectable by a
//! caller. The supervisor treats it exactly like the real worker. Its behaviour is
//! chosen by its own file name (`fake-<behaviour>`), because the supervisor passes
//! only the fixed `<role> <path>` arguments.
//!
//! `build` publishes a header-only file and echoes the manifest so a generation
//! exists; `query` then misbehaves: malformed / oversized / wrong-pid output, a hang
//! that never reads its request, or an abort.
use keeper_graph_protocol::wire::{Manifest, Response};
use std::io::{BufRead, Read, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let name = std::path::Path::new(args.first().ok_or("argv0")?)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("name")?
        .to_owned();
    let behaviour = name.strip_prefix("fake-").ok_or("fake worker name")?;
    let role = args.get(1).ok_or("role")?;
    let path = args.get(2).ok_or("path")?;
    let mut stdout = std::io::stdout();
    if role == "build" {
        let mut first = String::new();
        std::io::stdin().lock().read_line(&mut first)?;
        let manifest: Manifest = serde_json::from_str(first.trim_end())?;
        std::io::copy(&mut std::io::stdin(), &mut std::io::sink())?;
        let mut header = b"SQLite format 3\0".to_vec();
        header.extend_from_slice(&[16, 0, 1, 1]);
        header.resize(100, 0);
        std::fs::write(path, header)?;
        return Ok(serde_json::to_writer(
            &mut stdout,
            &response(manifest, std::process::id()),
        )?);
    }
    match behaviour {
        "malformed" => {
            std::io::stdin().read_to_end(&mut Vec::new())?;
            stdout.write_all(b"{\"manifest\":")?;
        }
        "oversized" => {
            let chunk = vec![b'x'; 64 * 1024];
            for _ in 0..40 {
                stdout.write_all(&chunk)?;
            }
        }
        "wrong-pid" => {
            std::io::stdin().read_to_end(&mut Vec::new())?;
            let manifest = Manifest {
                domain: "fake".into(),
                authority_epoch: "1".into(),
                accepted_head: 1,
                sync_checkpoint: "1".into(),
                schema_version: 1,
                projection_version: 1,
                engine: keeper_graph_protocol::wire::ENGINE.into(),
            };
            serde_json::to_writer(&mut stdout, &response(manifest, std::process::id() + 1))?;
        }
        "hang" => std::thread::sleep(std::time::Duration::from_secs(30)),
        "abort" => std::process::abort(),
        _ => return Err("unknown fake behaviour".into()),
    }
    Ok(())
}

fn response(manifest: Manifest, pid: u32) -> Response {
    Response {
        manifest,
        pid,
        allocated_bytes: 0,
        peak_allocated_bytes: 0,
        peak_rss_bytes: 0,
        rows: Vec::new(),
    }
}
