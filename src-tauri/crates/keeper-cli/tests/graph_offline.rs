//! Nothing a person sees changes (123.2 AC2, the rung's half): every graph
//! verb, run as a `keeper-cli` process with 113.5's arguments, prints the
//! line 113.5's binary printed and exits with its code — recorded from
//! keeper `main` at df74c488 with no server reachable — and reaches the
//! graph through exactly one dispatch, by the runtime's counter.
//! The live half (the 17 verbs against keeper-hq on delectra, compared with
//! 113.5's recording) is the epic's E row.

#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const URL: &str = "http://127.0.0.1:9";
const NO_TOKEN: &str =
    r#"{"detail":"KEEPER_GRAPH_TOKEN is required","error":"unauthorised","ok":false}"#;

struct Run {
    stdout: String,
    code: Option<i32>,
    dispatches: Option<u64>,
}

fn scratch(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("keeper-cli-offline-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    dir
}

fn write(dir: &Path, name: &str, text: &str) -> String {
    let path = dir.join(name);
    std::fs::write(&path, text).expect("payload");
    path.to_string_lossy().into_owned()
}

fn run(state: &Path, token: bool, json: bool, verb: &[&str]) -> Run {
    let mut command = Command::new(env!("CARGO_BIN_EXE_keeper-cli"));
    if json {
        command.arg("--json");
    }
    command
        .args(["graph", "--state"])
        .arg(state)
        .args([
            "--url",
            URL,
            "--database",
            "work",
            "--actor",
            "ana",
            "--client",
            "dev1",
        ])
        .args(verb)
        .env("KEEPER_CLI_DISPATCHES", "1")
        .env_remove("KEEPER_GRAPH_TOKEN");
    if token {
        command.env("KEEPER_GRAPH_TOKEN", "t");
    }
    let out = command.output().expect("keeper-cli runs");
    let stderr = String::from_utf8_lossy(&out.stderr);
    let dispatches = stderr
        .lines()
        .find_map(|l| l.strip_prefix("keeper-runtime dispatches: "))
        .and_then(|n| n.trim().parse().ok());
    Run {
        stdout: String::from_utf8_lossy(&out.stdout).trim_end().to_owned(),
        code: out.status.code(),
        dispatches,
    }
}

/// All eighteen verbs, without a token: 113.5's refusal, byte for byte,
/// exit `2`, one dispatch each.
#[test]
fn every_verb_answers_as_before_through_one_dispatch() {
    let dir = scratch("no-token");
    let change = write(
        &dir,
        "c.json",
        r#"{"ops":[{"op":"create_node","id":"01JE0000000000000000000001","kind":"Concept","title":"N","description":null,"properties":{}}]}"#,
    );
    let event = "01JE0000000000000000000003";
    let verbs: [Vec<&str>; 18] = [
        vec!["apply", &change],
        vec!["enqueue", &change],
        vec!["annotate", "01JE0000000000000000000005", "--note", "n"],
        vec!["pending"],
        vec!["resume"],
        vec!["discard", "01JE0000000000000000000009"],
        vec!["sync"],
        vec!["query", "MATCH"],
        vec!["inspect", "node", "01JE0000000000000000000001"],
        vec!["history"],
        vec!["unreviewed"],
        vec!["propose", &change],
        vec!["decide", &change],
        vec!["proposal", "01JE0000000000000000000002"],
        vec!["acknowledge", event, "abc"],
        vec!["preview-reversal", event],
        vec!["reverse", event],
        vec!["status"],
    ];
    for verb in &verbs {
        let out = run(&dir.join("state"), false, true, verb);
        assert_eq!(out.stdout, NO_TOKEN, "{verb:?}");
        assert_eq!(out.code, Some(2), "{verb:?}");
        assert_eq!(out.dispatches, Some(1), "{verb:?}");
    }
    let human = run(&dir.join("state"), false, false, &["status"]);
    assert_eq!(
        human.stdout,
        "Failed: unauthorised (KEEPER_GRAPH_TOKEN is required)."
    );
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

/// With a token and no server: the offline verbs over one persistent
/// state directory, in order, as 113.5's binary answered them.
#[test]
fn offline_answers_are_unchanged_over_a_persistent_state() {
    let dir = scratch("token");
    let state = dir.join("state");
    let change = write(
        &dir,
        "c.json",
        r#"{"ops":[{"op":"create_node","id":"01JE0000000000000000000001","kind":"Concept","title":"N","description":null,"properties":{}}]}"#,
    );
    let empty = write(&dir, "bad.json", "{}");
    let array = write(&dir, "arr.json", "[]");
    let exact: Vec<(Vec<&str>, &str, i32)> = vec![(
        vec!["status"],
        r#"{"checkpoint":{"cursor":null,"scope":"pending"},"fence":null,"head":null,"ok":true,"queue":{},"runtime":"projection_unavailable: worker.path"}"#,
        0,
    )];
    for (verb, line, code) in &exact {
        let out = run(&state, true, true, verb);
        assert_eq!(
            (out.stdout.as_str(), out.code),
            (*line, Some(*code)),
            "{verb:?}"
        );
        assert_eq!(out.dispatches, Some(1), "{verb:?}");
    }
    // A fresh request id each time: compare the shape.
    let queued = run(&state, true, true, &["enqueue", &change]);
    let answer: Value = serde_json::from_str(&queued.stdout).expect("json");
    let id = answer["request_id"]
        .as_str()
        .expect("request id")
        .to_owned();
    assert_eq!(
        queued.stdout,
        format!(r#"{{"ok":true,"request_id":"{id}","state":"queued"}}"#)
    );
    let pending = run(&state, true, true, &["pending"]);
    let answer: Value = serde_json::from_str(&pending.stdout).expect("json");
    let age = answer["entries"][0]["age_ms"].as_u64().expect("age");
    assert_eq!(
        pending.stdout,
        format!(
            r#"{{"entries":[{{"age_ms":{age},"attempts":0,"cause":null,"request_id":"{id}","state":"queued"}}],"ok":true}}"#
        )
    );
    let after: Vec<(Vec<&str>, &str, i32)> = vec![
        (
            vec!["discard", "01JE0000000000000000000009"],
            r#"{"detail":"no queued request 01JE0000000000000000000009","error":"unknown_request","ok":false}"#,
            1,
        ),
        (
            vec!["status"],
            r#"{"checkpoint":{"cursor":null,"scope":"pending"},"fence":null,"head":null,"ok":true,"queue":{"queued":1},"runtime":"projection_unavailable: worker.path"}"#,
            0,
        ),
        (
            vec!["query", "MATCH"],
            r#"{"detail":"projection_unavailable: worker.path","error":"projection_unavailable","ok":false}"#,
            3,
        ),
        (
            vec!["inspect", "bogus", "x"],
            r#"{"detail":"entity is node, edge or content","error":"query_unsupported","ok":false}"#,
            2,
        ),
        (
            vec!["enqueue", &empty],
            r#"{"detail":"payload","error":"invalid_change","ok":false}"#,
            2,
        ),
        (
            vec!["propose", &empty],
            r#"{"detail":"payload","error":"invalid_change","ok":false}"#,
            2,
        ),
        (
            vec!["decide", &empty],
            r#"{"detail":"payload","error":"invalid_change","ok":false}"#,
            2,
        ),
        (
            vec!["acknowledge", "x", "y"],
            r#"{"detail":"target","error":"invalid_change","ok":false}"#,
            2,
        ),
    ];
    for (verb, line, code) in &after {
        let out = run(&state, true, true, verb);
        assert_eq!(
            (out.stdout.as_str(), out.code),
            (*line, Some(*code)),
            "{verb:?}"
        );
        assert_eq!(out.dispatches, Some(1), "{verb:?}");
    }
    // A payload that is no object is refused before anything is asked.
    let refused = run(&state, true, true, &["enqueue", &array]);
    assert_eq!(
        (refused.stdout.as_str(), refused.code),
        (
            r#"{"detail":"payload","error":"invalid_change","ok":false}"#,
            Some(2)
        )
    );
    let human = run(
        &state,
        true,
        false,
        &["discard", "01JE0000000000000000000009"],
    );
    assert_eq!(
        human.stdout,
        "Failed: unknown_request (no queued request 01JE0000000000000000000009)."
    );
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

/// One replica per `--database` (SR-5): two databases open, the verb
/// addresses the first; an `--actor` list that matches neither one nor
/// every database is a configuration error.
#[test]
fn the_database_flag_repeats() {
    let dir = scratch("repeat");
    let state = dir.join("state");
    let out = Command::new(env!("CARGO_BIN_EXE_keeper-cli"))
        .args(["--json", "graph", "--state"])
        .arg(&state)
        .args([
            "--url",
            URL,
            "--database",
            "work",
            "--database",
            "home",
            "--actor",
            "ana",
            "--client",
            "dev1",
            "status",
        ])
        .env("KEEPER_GRAPH_TOKEN", "t")
        .output()
        .expect("runs");
    let answer: Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(answer["ok"], true, "{answer}");
    assert!(state.join("work.sqlite").exists());
    assert!(state.join("home.sqlite").exists());
    let out = Command::new(env!("CARGO_BIN_EXE_keeper-cli"))
        .args(["--json", "graph", "--state"])
        .arg(&state)
        .args([
            "--url",
            URL,
            "--database",
            "work",
            "--database",
            "home",
            "--database",
            "x",
            "--actor",
            "ana",
            "--actor",
            "bo",
            "--client",
            "dev1",
            "status",
        ])
        .env("KEEPER_GRAPH_TOKEN", "t")
        .output()
        .expect("runs");
    let answer: Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(answer["error"], "invalid_arguments", "{answer}");
    assert_eq!(out.status.code(), Some(2));
    // A database named twice would open two replica owners on one file:
    // refused before anything is opened.
    let dup_state = dir.join("dup");
    let out = Command::new(env!("CARGO_BIN_EXE_keeper-cli"))
        .args(["--json", "graph", "--state"])
        .arg(&dup_state)
        .args([
            "--url",
            URL,
            "--database",
            "work",
            "--database",
            "work",
            "--actor",
            "ana",
            "--client",
            "dev1",
            "status",
        ])
        .env("KEEPER_GRAPH_TOKEN", "t")
        .output()
        .expect("runs");
    let answer: Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(answer["error"], "invalid_arguments", "{answer}");
    assert_eq!(out.status.code(), Some(2), "{answer}");
    assert!(!dup_state.join("work.sqlite").exists());
    std::fs::remove_dir_all(&dir).expect("cleanup");
}
