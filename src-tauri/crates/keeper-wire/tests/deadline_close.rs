//! 123.3-AC14 H2, H6: deadlines, ceilings, partial lines on EOF, and
//! connection close with in-flight queries, streams, and orphaned commands.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::{Duration, Instant};

use common::{eventually, id, limits, serve, Counts};
use keeper_wire::{Hooks, Limits};
use serde_json::json;
use tokio::io::AsyncWriteExt as _;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn h2_query_deadline_exceeded_drops_handler() {
    let lim = Limits {
        query_ceiling: Duration::from_millis(400),
        command_ceiling: Duration::from_millis(400),
        ..limits()
    };
    let served = serve(lim, Hooks::default());
    let mut raw = served.raw().await;

    let call = id();
    let start = Instant::now();
    raw.send(&json!({
        "type": "call",
        "call_id": call,
        "operation": "fx.held",
        "input": {},
        "deadline_ms": 100
    }))
    .await;

    let reply = raw.terminal(&call, &mut Vec::new()).await;
    let elapsed = start.elapsed();
    assert_eq!(reply["type"], "reply", "{reply}");
    assert_eq!(reply["error"]["code"], "DEADLINE_EXCEEDED", "{reply}");
    assert!(
        elapsed >= Duration::from_millis(90) && elapsed < Duration::from_millis(350),
        "expected DEADLINE_EXCEEDED within ~100-350 ms, got {elapsed:?}"
    );
    assert!(
        eventually(Duration::from_secs(1), || {
            Counts::get(&served.fixture.counts.drops) == 1
        })
        .await,
        "held query handler was not dropped after deadline"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn h2_query_ceiling_clamps_above_and_defaults_absent_deadline() {
    let lim = Limits {
        query_ceiling: Duration::from_millis(400),
        command_ceiling: Duration::from_millis(400),
        ..limits()
    };
    let served = serve(lim, Hooks::default());
    let mut raw = served.raw().await;

    let above = id();
    let start_above = Instant::now();
    raw.send(&json!({
        "type": "call",
        "call_id": above,
        "operation": "fx.held",
        "input": {},
        "deadline_ms": 60_000
    }))
    .await;
    let reply_above = raw.terminal(&above, &mut Vec::new()).await;
    let elapsed_above = start_above.elapsed();
    assert_eq!(reply_above["type"], "reply", "{reply_above}");
    assert_eq!(
        reply_above["error"]["code"], "DEADLINE_EXCEEDED",
        "{reply_above}"
    );
    assert!(
        elapsed_above >= Duration::from_millis(350) && elapsed_above < Duration::from_secs(5),
        "expected clamped ceiling deadline (>= 350 ms, << 60 s), got {elapsed_above:?}"
    );

    let absent = id();
    let start_absent = Instant::now();
    raw.call(&absent, "fx.held", json!({})).await;
    let reply_absent = raw.terminal(&absent, &mut Vec::new()).await;
    let elapsed_absent = start_absent.elapsed();
    assert_eq!(reply_absent["type"], "reply", "{reply_absent}");
    assert_eq!(
        reply_absent["error"]["code"], "DEADLINE_EXCEEDED",
        "{reply_absent}"
    );
    assert!(
        elapsed_absent >= Duration::from_millis(350) && elapsed_absent < Duration::from_secs(5),
        "expected default ceiling deadline (>= 350 ms, << 60 s), got {elapsed_absent:?}"
    );

    assert!(
        eventually(Duration::from_secs(1), || {
            Counts::get(&served.fixture.counts.drops) == 2
        })
        .await,
        "held query handlers were not dropped"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn h2_started_command_deadline_exceeded_keeps_running() {
    let lim = Limits {
        query_ceiling: Duration::from_millis(400),
        command_ceiling: Duration::from_millis(400),
        ..limits()
    };
    let served = serve(lim, Hooks::default());
    let mut raw = served.raw().await;

    let call = id();
    raw.send(&json!({
        "type": "call",
        "call_id": call,
        "operation": "fx.cmd",
        "input": {},
        "deadline_ms": 100
    }))
    .await;

    let reply = raw.terminal(&call, &mut Vec::new()).await;
    assert_eq!(reply["type"], "reply", "{reply}");
    assert_eq!(reply["error"]["code"], "DEADLINE_EXCEEDED", "{reply}");
    assert_eq!(reply["error"]["retryable"], true, "{reply}");
    assert_eq!(reply["error"]["request_id"], call, "{reply}");
    assert_eq!(Counts::get(&served.fixture.counts.commands_started), 1);
    assert_eq!(Counts::get(&served.fixture.counts.commands_finished), 0);

    served.fixture.gate.open();
    assert!(
        eventually(Duration::from_secs(2), || {
            Counts::get(&served.fixture.counts.commands_finished) == 1
        })
        .await,
        "started command did not finish after gate opened"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn h6a_half_call_line_then_eof_dispatches_nothing() {
    let served = serve(limits(), Hooks::default());
    let mut raw = served.raw().await;

    let call = id();
    let half = format!(r#"{{"type":"call","call_id":"{call}","oper"#);
    raw.send_bytes(half.as_bytes()).await;
    raw.write.shutdown().await.unwrap();

    let trailing = raw.drain_bytes().await;
    assert!(
        trailing.is_empty(),
        "expected no bytes before EOF, got {trailing:?}"
    );

    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(Counts::get(&served.fixture.counts.calls), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn h6b_whole_call_without_newline_then_eof_dispatches_nothing() {
    let served = serve(limits(), Hooks::default());
    let mut raw = served.raw().await;

    let call = id();
    let body = serde_json::to_vec(&json!({
        "type": "call",
        "call_id": call,
        "operation": "fx.echo",
        "input": {}
    }))
    .unwrap();
    raw.send_bytes(&body).await;
    raw.write.shutdown().await.unwrap();

    let trailing = raw.drain_bytes().await;
    assert!(
        trailing.is_empty(),
        "expected no bytes before EOF, got {trailing:?}"
    );

    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(Counts::get(&served.fixture.counts.calls), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn h6c_close_drops_query_and_stream_but_keeps_started_command() {
    let served = serve(limits(), Hooks::default());
    let mut raw = served.raw().await;

    let q = id();
    let s = id();
    let c = id();
    raw.call(&q, "fx.held", json!({})).await;
    raw.watch(&s, "fx.stream_held", json!({})).await;
    raw.call(&c, "fx.cmd", json!({})).await;

    // Read the first event from fx.stream_held so we know it is paused at the gate.
    let ev = raw.frame().await.expect("first stream_held event");
    assert_eq!(ev["type"], "event", "{ev}");
    assert_eq!(ev["call_id"], s, "{ev}");
    assert_eq!(ev["seq"], 1, "{ev}");

    assert!(
        eventually(Duration::from_secs(2), || {
            Counts::get(&served.fixture.counts.calls) == 3
                && Counts::get(&served.fixture.counts.commands_started) == 1
                && served.server().open_streams() == 1
        })
        .await,
        "expected query, stream, and started command in flight"
    );

    drop(raw);

    assert!(
        eventually(Duration::from_secs(1), || {
            Counts::get(&served.fixture.counts.drops) == 2 && served.server().open_streams() == 0
        })
        .await,
        "expected 2 drops and 0 open streams within 1 s, got drops={} open_streams={}",
        Counts::get(&served.fixture.counts.drops),
        served.server().open_streams()
    );
    assert_eq!(Counts::get(&served.fixture.counts.commands_finished), 0);

    served.fixture.gate.open();
    assert!(
        eventually(Duration::from_secs(2), || {
            Counts::get(&served.fixture.counts.commands_finished) == 1
        })
        .await,
        "started command did not finish after gate opened"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn h6d_orphaned_commands_keep_listener_places_until_finished() {
    let lim = Limits {
        listener_calls: 4,
        ..limits()
    };
    let served = serve(lim, Hooks::default());
    let mut raw1 = served.raw().await;

    for _ in 0..4 {
        let c = id();
        raw1.call(&c, "fx.cmd", json!({})).await;
    }
    assert!(
        eventually(Duration::from_secs(2), || {
            Counts::get(&served.fixture.counts.commands_started) == 4
        })
        .await,
        "expected 4 commands started"
    );

    drop(raw1);
    // Give the server connection task a moment to observe EOF and exit.
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(served.server().calls_in_flight(), 4);

    let mut raw2 = served.raw().await;
    let refused = id();
    raw2.call(&refused, "fx.echo", json!({"try": 1})).await;
    let reply = raw2.terminal(&refused, &mut Vec::new()).await;
    assert_eq!(reply["type"], "reply", "{reply}");
    assert_eq!(reply["error"]["code"], "RESOURCE_EXHAUSTED", "{reply}");

    served.fixture.gate.open();
    assert!(
        eventually(Duration::from_secs(2), || {
            Counts::get(&served.fixture.counts.commands_finished) == 4
                && served.server().calls_in_flight() == 0
        })
        .await,
        "expected 4 finished commands and 0 calls in flight, got finished={} in_flight={}",
        Counts::get(&served.fixture.counts.commands_finished),
        served.server().calls_in_flight()
    );

    let ok = id();
    raw2.call(&ok, "fx.echo", json!({"try": 2})).await;
    let reply = raw2.terminal(&ok, &mut Vec::new()).await;
    assert_eq!(reply["type"], "reply", "{reply}");
    assert_eq!(reply["output"], json!({"try": 2}), "{reply}");
}
