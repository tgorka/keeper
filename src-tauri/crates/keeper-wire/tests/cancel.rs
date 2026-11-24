//! 123.3-AC14 H1, H11: cancellation, terminal-frame uniqueness, command
//! place retention after cancel, and pre-start command suppression.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use common::{eventually, id, limits, serve, serve_with, Counts, Fixture, Gate, Served};
use keeper_wire::Hooks;
use serde_json::{json, Value};

fn serve_with_start_gate() -> (Served, Gate) {
    let start_gate = Gate::new();
    let gate = start_gate.clone();
    let hooks = Hooks {
        start_gate: Some(Arc::new(move || {
            let g = gate.clone();
            Box::pin(async move { g.wait().await })
        })),
        ..Default::default()
    };
    let fixture = Fixture::new(64);
    fixture.gate.open();
    let served = serve_with(fixture, limits(), hooks);
    (served, start_gate)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancel_stream_and_query_leaves_sibling_and_unknown_cancel_harmless() {
    let served = serve(limits(), Hooks::default());
    let mut raw = served.raw().await;

    let stream_id = id();
    raw.watch(&stream_id, "fx.stream_held", json!({})).await;
    let first_event = raw.frame().await.expect("first stream event");
    assert_eq!(first_event["type"], "event", "{first_event}");
    assert_eq!(first_event["call_id"], stream_id);
    assert_eq!(first_event["seq"], 1);

    let held_query = id();
    raw.call(&held_query, "fx.held", json!({"which": "cancelled"}))
        .await;
    let sibling = id();
    raw.call(&sibling, "fx.held", json!({"which": "kept"}))
        .await;

    assert!(
        eventually(Duration::from_secs(2), || {
            Counts::get(&served.fixture.counts.calls) == 3
                && served.server().open_streams() == 1
                && served.server().calls_in_flight() == 3
        })
        .await,
        "stream and both held queries should be in flight"
    );

    // Cancel the stream: end with CANCELLED and open_streams returns to 0 within 1 s.
    raw.cancel(&stream_id).await;
    let mut others = Vec::new();
    let stream_end = raw.terminal(&stream_id, &mut others).await;
    assert_eq!(stream_end["type"], "end", "{stream_end}");
    assert_eq!(stream_end["error"]["code"], "CANCELLED", "{stream_end}");
    assert!(others.is_empty(), "unexpected frames: {others:?}");
    assert!(
        eventually(Duration::from_secs(1), || served.server().open_streams()
            == 0)
        .await,
        "open_streams did not drop to 0 within 1 s"
    );
    assert!(
        eventually(Duration::from_secs(1), || {
            Counts::get(&served.fixture.counts.drops) >= 1
        })
        .await,
        "stream handler was not dropped"
    );

    // Cancel the held query: reply CANCELLED and fixture drops increments.
    let drops_before = Counts::get(&served.fixture.counts.drops);
    raw.cancel(&held_query).await;
    let query_reply = raw.terminal(&held_query, &mut others).await;
    assert_eq!(query_reply["type"], "reply", "{query_reply}");
    assert_eq!(query_reply["error"]["code"], "CANCELLED", "{query_reply}");
    assert!(others.is_empty(), "unexpected frames: {others:?}");
    assert!(
        eventually(Duration::from_secs(1), || {
            Counts::get(&served.fixture.counts.drops) > drops_before
        })
        .await,
        "held query handler was not dropped on cancel"
    );

    // Sibling call answers untouched once the gate opens.
    served.fixture.gate.open();
    let sibling_reply = raw.terminal(&sibling, &mut others).await;
    assert_eq!(sibling_reply["type"], "reply", "{sibling_reply}");
    assert!(sibling_reply.get("error").is_none(), "{sibling_reply}");
    assert_eq!(sibling_reply["output"], json!({"which": "kept"}));
    assert!(others.is_empty(), "unexpected frames: {others:?}");

    // Cancel for an id not in flight is a no-op and the connection continues.
    let unknown = id();
    raw.cancel(&unknown).await;
    let echo = id();
    raw.call(&echo, "fx.echo", json!({"ok": true})).await;
    let echo_reply = raw.terminal(&echo, &mut others).await;
    assert_eq!(echo_reply["type"], "reply", "{echo_reply}");
    assert_eq!(echo_reply["output"], json!({"ok": true}));
    assert!(
        others.is_empty(),
        "unexpected frames after unknown cancel: {others:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancel_started_command_returns_request_id_and_keeps_running() {
    let served = serve(limits(), Hooks::default());
    let mut raw = served.raw().await;

    let cmd = id();
    raw.call(&cmd, "fx.cmd", json!({"op": "write"})).await;
    assert!(
        eventually(Duration::from_secs(2), || {
            Counts::get(&served.fixture.counts.commands_started) == 1
        })
        .await,
        "command did not start"
    );
    assert_eq!(Counts::get(&served.fixture.counts.commands_finished), 0);

    raw.cancel(&cmd).await;
    let mut others = Vec::new();
    let reply = raw.terminal(&cmd, &mut others).await;
    assert_eq!(reply["type"], "reply", "{reply}");
    assert_eq!(reply["error"]["code"], "CANCELLED", "{reply}");
    assert_eq!(reply["error"]["request_id"], cmd, "{reply}");
    assert!(others.is_empty(), "unexpected frames: {others:?}");
    assert_eq!(Counts::get(&served.fixture.counts.commands_finished), 0);

    served.fixture.gate.open();
    assert!(
        eventually(Duration::from_secs(2), || {
            Counts::get(&served.fixture.counts.commands_finished) == 1
        })
        .await,
        "started command did not finish after cancel"
    );

    // No second terminal frame is emitted when the command handler returns.
    let echo = id();
    raw.call(&echo, "fx.echo", json!({"done": true})).await;
    let echo_reply = raw.terminal(&echo, &mut others).await;
    assert_eq!(echo_reply["output"], json!({"done": true}));
    assert!(others.is_empty(), "unexpected frames: {others:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancel_after_answer_decided_yields_single_answer_frame() {
    let served = serve(limits(), Hooks::default());
    let mut raw = served.raw().await;

    let mut ids = Vec::with_capacity(8);
    for _ in 0..8 {
        let call_id = id();
        raw.call(&call_id, "fx.big", json!({"bytes": 300_000}))
            .await;
        ids.push(call_id);
    }

    assert!(
        eventually(Duration::from_secs(5), || {
            Counts::get(&served.fixture.counts.calls) == 8
        })
        .await,
        "all 8 fx.big handlers did not run"
    );
    // Let all 8 immediate handlers finish serializing and queueing their terminal replies.
    tokio::time::sleep(Duration::from_millis(100)).await;

    for call_id in &ids {
        raw.cancel(call_id).await;
    }

    // Send a trailing echo so draining to it also catches any duplicate frames.
    let sentinel = id();
    raw.call(&sentinel, "fx.echo", json!({"sentinel": true}))
        .await;

    let mut frames = Vec::new();
    let sentinel_reply = raw.terminal(&sentinel, &mut frames).await;
    assert_eq!(sentinel_reply["output"], json!({"sentinel": true}));

    let expected: HashSet<&str> = ids.iter().map(String::as_str).collect();
    let mut seen: HashMap<String, Value> = HashMap::new();
    for frame in frames {
        let call_id = frame["call_id"].as_str().unwrap().to_owned();
        assert!(
            seen.insert(call_id.clone(), frame).is_none(),
            "duplicate frame for {call_id}"
        );
    }

    assert_eq!(seen.len(), 8, "expected 8 replies, got {}", seen.len());
    for call_id in expected {
        let reply = seen.get(call_id).expect("missing reply for call");
        assert_eq!(reply["type"], "reply", "{reply}");
        assert!(
            reply.get("error").is_none(),
            "got error instead of answer: {reply}"
        );
        let s = reply["output"]["s"].as_str().expect("output.s string");
        assert_eq!(s.len(), 300_000);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelled_commands_keep_their_places_until_handlers_return() {
    let mut lim = limits();
    lim.per_connection = 64;
    let served = serve(lim, Hooks::default());
    let mut raw = served.raw().await;

    let mut ids = Vec::with_capacity(64);
    for _ in 0..64 {
        let call_id = id();
        raw.call(&call_id, "fx.cmd", json!({})).await;
        ids.push(call_id);
    }

    assert!(
        eventually(Duration::from_secs(5), || {
            Counts::get(&served.fixture.counts.commands_started) == 64
        })
        .await,
        "all 64 commands did not start"
    );

    for call_id in &ids {
        raw.cancel(call_id).await;
    }

    let expected: HashSet<String> = ids.iter().cloned().collect();
    let mut cancelled = HashSet::new();
    while cancelled.len() < 64 {
        let frame = raw.frame().await.expect("cancelled reply");
        assert_eq!(frame["type"], "reply", "{frame}");
        assert_eq!(frame["error"]["code"], "CANCELLED", "{frame}");
        let call_id = frame["call_id"].as_str().unwrap().to_owned();
        assert_eq!(frame["error"]["request_id"], call_id, "{frame}");
        assert!(expected.contains(&call_id), "unexpected call_id {call_id}");
        assert!(cancelled.insert(call_id), "duplicate cancel reply");
    }

    // All 64 places are still held by the running command handlers.
    let overflow = id();
    raw.call(&overflow, "fx.echo", json!({})).await;
    let mut others = Vec::new();
    let overflow_reply = raw.terminal(&overflow, &mut others).await;
    assert_eq!(overflow_reply["type"], "reply", "{overflow_reply}");
    assert_eq!(
        overflow_reply["error"]["code"], "RESOURCE_EXHAUSTED",
        "{overflow_reply}"
    );
    assert!(others.is_empty(), "unexpected frames: {others:?}");

    // Once the handlers return, all places are released and a new call succeeds.
    served.fixture.gate.open();
    assert!(
        eventually(Duration::from_secs(5), || {
            Counts::get(&served.fixture.counts.commands_finished) == 64
                && served.server().calls_in_flight() == 0
        })
        .await,
        "commands did not finish and release their places"
    );

    let after = id();
    raw.call(&after, "fx.echo", json!({"recovered": true}))
        .await;
    let after_reply = raw.terminal(&after, &mut others).await;
    assert_eq!(after_reply["type"], "reply", "{after_reply}");
    assert_eq!(after_reply["output"], json!({"recovered": true}));
    assert!(others.is_empty(), "unexpected frames: {others:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn command_cancelled_before_start_never_runs() {
    let (served, start_gate) = serve_with_start_gate();
    let mut raw = served.raw().await;

    let cmd = id();
    raw.call(&cmd, "fx.cmd", json!({})).await;
    assert!(
        eventually(Duration::from_secs(2), || {
            served.server().calls_in_flight() == 1
        })
        .await,
        "command did not take its place at the start gate"
    );

    raw.cancel(&cmd).await;
    let mut others = Vec::new();
    let reply = raw.terminal(&cmd, &mut others).await;
    assert_eq!(reply["type"], "reply", "{reply}");
    assert_eq!(reply["error"]["code"], "CANCELLED", "{reply}");
    assert!(reply["error"].get("request_id").is_none(), "{reply}");
    assert!(others.is_empty(), "unexpected frames: {others:?}");

    start_gate.open();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(Counts::get(&served.fixture.counts.commands_started), 0);
    assert_eq!(Counts::get(&served.fixture.counts.calls), 0);
    assert!(
        eventually(Duration::from_secs(1), || {
            served.server().calls_in_flight() == 0
        })
        .await,
        "place was not released"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn command_connection_dropped_before_start_never_runs() {
    let (served, start_gate) = serve_with_start_gate();
    let mut raw = served.raw().await;

    let cmd = id();
    raw.call(&cmd, "fx.cmd", json!({})).await;
    assert!(
        eventually(Duration::from_secs(2), || {
            served.server().calls_in_flight() == 1
        })
        .await,
        "command did not take its place at the start gate"
    );

    drop(raw);
    // Give the server a moment to observe EOF before opening the start gate.
    let _ = eventually(Duration::from_millis(500), || {
        served.server().calls_in_flight() == 0
    })
    .await;

    start_gate.open();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(Counts::get(&served.fixture.counts.commands_started), 0);
    assert_eq!(Counts::get(&served.fixture.counts.calls), 0);
    assert!(
        eventually(Duration::from_secs(1), || {
            served.server().calls_in_flight() == 0
        })
        .await,
        "place was not released after disconnect"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn command_deadline_before_start_never_runs() {
    let (served, start_gate) = serve_with_start_gate();
    let mut raw = served.raw().await;

    let cmd = id();
    raw.send(&json!({
        "type": "call",
        "call_id": cmd,
        "operation": "fx.cmd",
        "input": {},
        "deadline_ms": 100
    }))
    .await;
    assert!(
        eventually(Duration::from_secs(2), || {
            served.server().calls_in_flight() == 1
        })
        .await,
        "command did not take its place at the start gate"
    );

    let mut others = Vec::new();
    let reply = raw.terminal(&cmd, &mut others).await;
    assert_eq!(reply["type"], "reply", "{reply}");
    assert_eq!(reply["error"]["code"], "DEADLINE_EXCEEDED", "{reply}");
    assert!(reply["error"].get("request_id").is_none(), "{reply}");
    assert!(others.is_empty(), "unexpected frames: {others:?}");

    start_gate.open();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(Counts::get(&served.fixture.counts.commands_started), 0);
    assert_eq!(Counts::get(&served.fixture.counts.calls), 0);
    assert!(
        eventually(Duration::from_secs(1), || {
            served.server().calls_in_flight() == 0
        })
        .await,
        "place was not released after deadline"
    );
}
