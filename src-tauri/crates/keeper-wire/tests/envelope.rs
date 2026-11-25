//! 123.3-AC5: the envelope over a real socket.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::Duration;

use common::{id, limits, serve, Raw};
use keeper_wire::Hooks;
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hello_call_watch_and_describe_answer() {
    let served = serve(limits(), Hooks::default());
    let mut raw = Raw::connect(&served.socket()).await;
    let hello = raw.hello().await;
    assert_eq!(hello["wire"], "keeper.wire.v1");
    assert_eq!(hello["app_version"], "0.0.0-fixture");
    assert_eq!(hello["catalogue_digest"], "fixture-digest");
    assert_eq!(hello["max_line_bytes"], 4 * 1024 * 1024);
    assert_eq!(hello["max_in_flight"], 64);

    let call = id();
    raw.call(&call, "fx.echo", json!({"a": 1})).await;
    let reply = raw.terminal(&call, &mut Vec::new()).await;
    assert_eq!(reply["type"], "reply");
    assert_eq!(reply["output"], json!({"a": 1}));

    let stream = id();
    raw.watch(&stream, "fx.stream", json!({"count": 3})).await;
    let mut events = Vec::new();
    let end = raw.terminal(&stream, &mut events).await;
    assert_eq!(end["type"], "end");
    assert!(end.get("error").is_none(), "{end}");
    let seqs: Vec<u64> = events.iter().map(|e| e["seq"].as_u64().unwrap()).collect();
    assert_eq!(seqs, [1, 2, 3]);

    let describe = id();
    raw.send(&json!({"type": "describe", "call_id": describe}))
        .await;
    let reply = raw.terminal(&describe, &mut Vec::new()).await;
    let output = &reply["output"];
    assert_eq!(output["envelope"], "keeper.wire.v1");
    assert_eq!(output["catalogue_digest"], "fixture-digest");
    assert!(output["operations"].as_array().unwrap().len() >= 5);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_call_past_its_deadline_answers_deadline_exceeded() {
    let served = serve(limits(), Hooks::default());
    let mut raw = served.raw().await;
    let call = id();
    raw.send(&json!({"type": "call", "call_id": call, "operation": "fx.held", "input": {}, "deadline_ms": 100}))
        .await;
    let reply = raw.terminal(&call, &mut Vec::new()).await;
    assert_eq!(reply["error"]["code"], "DEADLINE_EXCEEDED", "{reply}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_seventeenth_connection_is_refused() {
    let served = serve(limits(), Hooks::default());
    let mut held = Vec::new();
    for _ in 0..16 {
        held.push(served.raw().await);
    }
    let mut extra = Raw::connect(&served.socket()).await;
    let frames = extra.until_closed().await;
    assert_eq!(frames.len(), 1, "{frames:?}");
    assert_eq!(frames[0]["type"], "close");
    assert_eq!(frames[0]["error"]["code"], "RESOURCE_EXHAUSTED");
    // Once one goes, a new connection is served.
    drop(held.pop());
    tokio::time::sleep(Duration::from_millis(100)).await;
    let _again = served.raw().await;
}

/// m1: the module's text names a path; the reply carries keeper's fixed
/// sentence only.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_error_reply_carries_the_fixed_sentence_not_the_path() {
    let served = serve(limits(), Hooks::default());
    let mut raw = served.raw().await;
    let call = id();
    raw.call(&call, "fx.fail_path", json!({})).await;
    let reply = raw.terminal(&call, &mut Vec::new()).await;
    let error = &reply["error"];
    assert_eq!(error["code"], "UNAVAILABLE");
    let text = reply.to_string();
    assert!(!text.contains("secret-person"), "{text}");
    assert!(!text.contains("keeper.db"), "{text}");
    assert_eq!(
        error["message"],
        keeper_wire_api::sentence(keeper_wire_api::Code::Unavailable, "storage_unavailable")
    );
}
