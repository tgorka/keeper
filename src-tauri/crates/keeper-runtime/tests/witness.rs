//! The class witness and the local caller as compile-fail tests (123.2
//! AC11; DR-9 b, MA-G9, MA-G14): code outside keeper-runtime cannot make a
//! `Caller::Local` or a `decide` witness, and a `read` handler cannot call
//! the graph facade's write method. Each pins the compiler's error code,
//! never its text; a clean control per setup proves the harness compiles.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates dir")
        .to_path_buf()
}

fn workspace_dir() -> PathBuf {
    crates_dir()
        .parent()
        .expect("src-tauri workspace dir")
        .to_path_buf()
}

fn repo_root() -> PathBuf {
    workspace_dir().parent().expect("repo root").to_path_buf()
}

struct CheckOutcome {
    success: bool,
    codes: Vec<String>,
    rendered: String,
    stderr: String,
}

/// Check `lib_rs` as crate `witness-probe-<name>`. Each probe has its own
/// package name: cargo identifies a workspace root's unit by name, not by
/// path, so two probes of one name in the shared target directory would
/// read each other's results.
fn run_probe(name: &str, deps_toml: &str, lib_rs: &str) -> CheckOutcome {
    let dir = tempfile::tempdir().expect("tempdir");
    let ws = workspace_dir();
    let root = repo_root();
    let target_dir = ws.join("target").join("witness-check");

    let tools_tao = root.join("tools").join("tao");
    let tools_matrix = root.join("tools").join("matrix-sdk-sqlite");
    let tools_syncular = root.join("tools").join("syncular-client");

    let manifest = format!(
        r#"[workspace]

[package]
name = "witness-probe-{name}"
version = "0.1.0"
edition = "2021"

[dependencies]
{deps_toml}

[patch.crates-io]
tao = {{ path = "{}" }}
matrix-sdk-sqlite = {{ path = "{}" }}
syncular-client = {{ path = "{}" }}
"#,
        tools_tao.display(),
        tools_matrix.display(),
        tools_syncular.display(),
    );

    std::fs::write(dir.path().join("Cargo.toml"), manifest).expect("write Cargo.toml");
    std::fs::create_dir_all(dir.path().join("src")).expect("create src dir");
    std::fs::write(dir.path().join("src").join("lib.rs"), lib_rs).expect("write src/lib.rs");
    std::fs::copy(ws.join("Cargo.lock"), dir.path().join("Cargo.lock")).expect("copy Cargo.lock");

    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args(["check", "--offline", "--message-format=json"])
        .current_dir(dir.path())
        .env("CARGO_TARGET_DIR", &target_dir)
        .output()
        .expect("run cargo check");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let mut codes = Vec::new();
    let mut rendered = String::new();

    for line in stdout.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value.get("reason").and_then(Value::as_str) != Some("compiler-message") {
            continue;
        }
        let Some(message) = value.get("message") else {
            continue;
        };
        if message.get("level").and_then(Value::as_str) == Some("error") {
            if let Some(code) = message
                .get("code")
                .and_then(|c| c.get("code"))
                .and_then(Value::as_str)
            {
                codes.push(code.to_string());
            }
            if let Some(r) = message.get("rendered").and_then(Value::as_str) {
                rendered.push_str(r);
                rendered.push('\n');
            }
        }
    }

    CheckOutcome {
        success: output.status.success(),
        codes,
        rendered,
        stderr,
    }
}

fn runtime_api_deps() -> String {
    let crates = crates_dir();
    format!(
        "keeper-runtime-api = {{ path = \"{}\" }}",
        crates.join("keeper-runtime-api").display()
    )
}

fn graph_deps() -> String {
    let crates = crates_dir();
    format!(
        "keeper-runtime-api = {{ path = \"{}\" }}\nkeeper-graph-api = {{ path = \"{}\" }}\nkeeper-graph = {{ path = \"{}\", features = [\"module\"] }}",
        crates.join("keeper-runtime-api").display(),
        crates.join("keeper-graph-api").display(),
        crates.join("keeper-graph").display(),
    )
}

#[test]
fn runtime_api_control_compiles_cleanly() {
    let out = run_probe(
        "runtime-api-control-compiles-cleanly",
        &runtime_api_deps(),
        r#"
use keeper_runtime_api::{Bindings, CallContext, Caller, Door, Read};

pub fn check_caller(caller: &Caller) -> bool {
    matches!(caller, Caller::Wire { door: Door::Cli })
}

pub fn bind(b: &mut Bindings) {
    b.read("demo.ping", |ctx: CallContext<Read>, _: ()| async move {
        let _ = ctx.witness();
        Ok(())
    });
}
"#,
    );
    assert!(
        out.success,
        "control must compile cleanly; codes={:?}\nrendered:\n{}\nstderr:\n{}",
        out.codes, out.rendered, out.stderr
    );
}

#[test]
fn cannot_construct_local_outside_runtime() {
    let out = run_probe(
        "cannot-construct-local-outside-runtime",
        &runtime_api_deps(),
        r#"
pub fn forge() -> keeper_runtime_api::Caller {
    keeper_runtime_api::Caller::Local(keeper_runtime_api::Local(()))
}
"#,
    );
    assert!(
        !out.success && out.codes.iter().any(|c| c == "E0423"),
        "expected E0423; success={} codes={:?}\nrendered:\n{}\nstderr:\n{}",
        out.success,
        out.codes,
        out.rendered,
        out.stderr
    );
}

#[test]
fn cannot_call_runtime_only_decide_without_feature() {
    let out = run_probe(
        "cannot-call-runtime-only-decide-without-feature",
        &runtime_api_deps(),
        r#"
pub fn forge(local: &keeper_runtime_api::Local) -> keeper_runtime_api::Decide {
    keeper_runtime_api::runtime_only::decide(local)
}
"#,
    );
    assert!(
        !out.success && out.codes.iter().any(|c| c == "E0433"),
        "expected E0433; success={} codes={:?}\nrendered:\n{}\nstderr:\n{}",
        out.success,
        out.codes,
        out.rendered,
        out.stderr
    );
}

#[test]
fn graph_read_control_compiles_cleanly() {
    let out = run_probe(
        "graph-read-control-compiles-cleanly",
        &graph_deps(),
        r#"
use std::sync::Arc;
use keeper_runtime_api::{Bindings, CallContext, Read};

pub fn bind(b: &mut Bindings, f: Arc<keeper_graph::module::GraphFacade>) {
    b.read("graph.status", move |ctx: CallContext<Read>, input: keeper_graph_api::TargetOnly| {
        let f = Arc::clone(&f);
        async move { f.status(ctx.witness(), input).await }
    });
}
"#,
    );
    assert!(
        out.success,
        "graph control must compile cleanly; codes={:?}\nrendered:\n{}\nstderr:\n{}",
        out.codes, out.rendered, out.stderr
    );
}

#[test]
fn read_handler_cannot_call_write_facade_method() {
    let out = run_probe(
        "read-handler-cannot-call-write-facade-method",
        &graph_deps(),
        r#"
use std::sync::Arc;
use keeper_runtime_api::{Bindings, CallContext, Read};

pub fn bind(b: &mut Bindings, f: Arc<keeper_graph::module::GraphFacade>) {
    b.read("graph.status", move |ctx: CallContext<Read>, input: keeper_graph_api::Targeted| {
        let f = Arc::clone(&f);
        async move { f.apply(ctx.witness(), input).await }
    });
}
"#,
    );
    assert!(
        !out.success && out.codes.iter().any(|c| c == "E0308"),
        "expected E0308; success={} codes={:?}\nrendered:\n{}\nstderr:\n{}",
        out.success,
        out.codes,
        out.rendered,
        out.stderr
    );
}
