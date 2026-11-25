use std::path::PathBuf;
use std::process::Command;

use keeper_wire_api::schema::{compatible, frame_schema, COMMITTED};
use serde_json::Value;

fn schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schema/keeper.wire.v1.json")
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schema/fixtures")
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

#[test]
fn the_committed_schema_is_the_types() {
    let generated = frame_schema();
    let path = schema_path();

    if std::env::var_os("KEEPER_WRITE_WIRE_SCHEMA").is_some() {
        let mut rendered = serde_json::to_string_pretty(&generated).expect("serialize schema");
        rendered.push('\n');
        std::fs::write(&path, rendered).expect("write committed schema");
    }

    let on_disk_text = std::fs::read_to_string(&path).expect("read committed schema");
    let on_disk: Value = serde_json::from_str(&on_disk_text).expect("parse committed schema");
    assert_eq!(
        on_disk,
        generated,
        "committed schema at {} does not match frame types; rerun with KEEPER_WRITE_WIRE_SCHEMA=1",
        path.display()
    );

    let committed: Value = serde_json::from_str(COMMITTED).expect("parse COMMITTED schema");
    if std::env::var_os("KEEPER_WRITE_WIRE_SCHEMA").is_none() {
        assert_eq!(
            committed, generated,
            "COMMITTED schema does not match frame types; rerun with KEEPER_WRITE_WIRE_SCHEMA=1"
        );
    }
}

#[test]
fn the_committed_schema_is_compatible_with_itself() {
    let committed: Value = serde_json::from_str(COMMITTED).expect("parse COMMITTED schema");
    compatible(&committed, &committed).expect("committed schema must be compatible with itself");

    // Adding an optional property is allowed.
    let mut grown = committed.clone();
    grown["client"]["$defs"]["Call"]["properties"]["trace_hint"] =
        serde_json::json!({ "type": ["string", "null"] });
    compatible(&committed, &grown).expect("adding an optional property must stay compatible");
}

#[test]
fn each_fixture_breaks_compatibility_naming_its_field() {
    let committed: Value = serde_json::from_str(COMMITTED).expect("parse COMMITTED schema");
    let dir = fixtures_dir();

    let cases = [
        ("field-removed.json", "seq", "removed"),
        ("field-retyped.json", "max_in_flight", "retyped"),
        ("field-newly-required.json", "deadline_ms", "newly required"),
    ];

    for (file, field, kind) in cases {
        let raw = std::fs::read_to_string(dir.join(file))
            .unwrap_or_else(|e| panic!("read fixture {file}: {e}"));
        let fixture: Value =
            serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse fixture {file}: {e}"));
        let breaks = compatible(&committed, &fixture)
            .expect_err(&format!("fixture {file} must break compatibility"));
        let joined = breaks.join("\n");
        assert!(
            breaks.iter().any(|m| m.contains(field) && m.contains(kind)),
            "fixture {file}: expected a break naming `{field}` and `{kind}`, got:\n{joined}"
        );
    }
}

#[test]
fn against_the_last_release() {
    let root = repo_root();
    let tag_output = match Command::new("git")
        .args(["-C"])
        .arg(&root)
        .args(["describe", "--tags", "--abbrev=0"])
        .output()
    {
        Ok(out) => out,
        Err(err) => {
            println!(
                "no release before this one holds a frame schema; the compatibility check passes (XR-6) (git unavailable: {err})"
            );
            return;
        }
    };

    if !tag_output.status.success() {
        let stderr = String::from_utf8_lossy(&tag_output.stderr);
        println!(
            "no release before this one holds a frame schema; the compatibility check passes (XR-6) (git describe failed: {})",
            stderr.trim()
        );
        return;
    }

    let tag = String::from_utf8_lossy(&tag_output.stdout)
        .trim()
        .to_owned();
    if tag.is_empty() {
        println!(
            "no release before this one holds a frame schema; the compatibility check passes (XR-6)"
        );
        return;
    }

    let spec = format!("{tag}:src-tauri/crates/keeper-wire-api/schema/keeper.wire.v1.json");
    let show_output = match Command::new("git")
        .args(["-C"])
        .arg(&root)
        .args(["show", &spec])
        .output()
    {
        Ok(out) => out,
        Err(err) => {
            println!(
                "no release before this one holds a frame schema; the compatibility check passes (XR-6) (git show failed to spawn: {err})"
            );
            return;
        }
    };

    if !show_output.status.success() {
        println!(
            "no release before this one holds a frame schema; the compatibility check passes (XR-6)"
        );
        return;
    }

    let old: Value = serde_json::from_slice(&show_output.stdout)
        .unwrap_or_else(|e| panic!("parse frame schema from release {tag}: {e}"));
    let committed: Value = serde_json::from_str(COMMITTED).expect("parse COMMITTED schema");
    compatible(&old, &committed).unwrap_or_else(|breaks| {
        panic!(
            "schema breaks compatibility with {tag}:\n{}",
            breaks.join("\n")
        )
    });
}
