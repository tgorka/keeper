//! Embeds the build-time worker digest and runtime ledger: the driver
//! refuses to spawn a worker without them (no search path, no fallback).
//! `KEEPER_GRAPH_WORKER_SHA256` is read by `option_env!`; the ledger is the
//! file `KEEPER_GRAPH_RUNTIME_LEDGER` names, copied into `OUT_DIR`.
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-env-changed=KEEPER_GRAPH_RUNTIME_LEDGER");
    println!("cargo:rerun-if-env-changed=KEEPER_GRAPH_WORKER_SHA256");
    let out = Path::new(&std::env::var("OUT_DIR")?).join("ledger.rs");
    let body = match std::env::var("KEEPER_GRAPH_RUNTIME_LEDGER") {
        Ok(path) => {
            println!("cargo:rerun-if-changed={path}");
            let text = std::fs::read_to_string(&path)?;
            format!("pub const LEDGER: Option<&str> = Some({text:?});\n")
        }
        Err(_) => "pub const LEDGER: Option<&str> = None;\n".to_owned(),
    };
    std::fs::write(out, body)?;
    Ok(())
}
