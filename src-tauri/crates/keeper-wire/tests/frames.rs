//! 123.3-AC14 H7, H8, H9 (wire half), H10: line limits, malformed frames,
//! field validation, version negotiation, and call_id lifecycle.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::Duration;

use common::{eventually, id, limits, serve, Counts, Raw};
use keeper_wire::Hooks;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::unix::OwnedReadHalf;

async fn read_json_line(read: &mut BufReader<OwnedReadHalf>) -> Option<Value> {
    let mut line = Vec::new();
    let n = tokio::time::timeout(Duration::from_secs(5), read.read_until(b'\n', &mut line))
        .await
        .expect("a line within 5s")
        .unwrap();
    if n == 0 {
        return None;
    }
    assert_eq!(line.last(), Some(&b'\n'), "a torn line from the server");
    Some(serde_json::from_slice(&line).unwrap())
}

// --- H7: max_line_bytes ---

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn h7_exact_max_line_bytes_is_answered() {
    let mut lim = limits();
    lim.max_line_bytes = 4096;
    let served = serve(lim, Hooks::default());
    let mut raw = served.raw().await;

    let call_id = id();
    let base = json!({
        "type": "call",
        "call_id": call_id,
        "operation": "fx.big",
        "input": {"bytes": 1, "pad": ""},
    });
    let base_len = serde_json::to_vec(&base).unwrap().len() + 1; // +1 for '\n'
    assert!(base_len < 4096, "base frame unexpectedly large: {base_len}");
    let pad = "x".repeat(4096 - base_len);
    let frame = json!({
        "type": "call",
        "call_id": call_id,
        "operation": "fx.big",
        "input": {"bytes": 1, "pad": pad},
    });
    assert_eq!(serde_json::to_vec(&frame).unwrap().len() + 1, 4096);

    raw.send(&frame).await;
    let reply = raw.terminal(&call_id, &mut Vec::new()).await;
    assert_eq!(reply["type"], "reply", "{reply}");
    assert_eq!(reply["output"], json!({"s": "x"}), "{reply}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn h7_one_byte_over_without_newline_closes_resource_exhausted() {
    let mut lim = limits();
    lim.max_line_bytes = 4096;
    let served = serve(lim, Hooks::default());
    let mut raw = served.raw().await;

    let calls_before = Counts::get(&served.fixture.counts.calls);
    raw.send_bytes(&vec![b'a'; 4097]).await;

    let frames = raw.until_closed().await;
    assert_eq!(frames.len(), 1, "{frames:?}");
    assert_eq!(frames[0]["type"], "close", "{frames:?}");
    assert_eq!(
        frames[0]["error"]["code"], "RESOURCE_EXHAUSTED",
        "{frames:?}"
    );
    assert_eq!(Counts::get(&served.fixture.counts.calls), calls_before);
    assert!(
        served.server().longest_line_held() <= 4096,
        "longest_line_held exceeded 4096: {}",
        served.server().longest_line_held()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn h7_oversized_reply_returns_resource_exhausted_and_connection_continues() {
    let mut lim = limits();
    lim.max_line_bytes = 4096;
    let served = serve(lim, Hooks::default());
    let mut raw = served.raw().await;

    let big_id = id();
    raw.call(&big_id, "fx.big", json!({"bytes": 8000})).await;
    let reply = raw.terminal(&big_id, &mut Vec::new()).await;
    assert_eq!(reply["type"], "reply", "{reply}");
    assert_eq!(reply["error"]["code"], "RESOURCE_EXHAUSTED", "{reply}");

    let echo_id = id();
    raw.call(&echo_id, "fx.echo", json!({"alive": true})).await;
    let echo_reply = raw.terminal(&echo_id, &mut Vec::new()).await;
    assert_eq!(echo_reply["type"], "reply", "{echo_reply}");
    assert_eq!(echo_reply["output"], json!({"alive": true}), "{echo_reply}");
}

// --- H8: malformed frames and field errors ---

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn h8_malformed_lines_close_invalid_argument_without_disrupting_other_connection() {
    let served = serve(limits(), Hooks::default());
    let mut healthy = served.raw().await;

    let cases: [(&str, bool, Vec<u8>); 6] = [
        ("invalid_utf8", true, b"\xff\xfe\n".to_vec()),
        ("not_json", true, b"not json\n".to_vec()),
        ("json_array", true, b"[1,2]\n".to_vec()),
        (
            "bad_call_id",
            true,
            format!(
                "{}\n",
                json!({"type": "call", "call_id": "abc", "operation": "fx.echo", "input": {}})
            )
            .into_bytes(),
        ),
        (
            "call_before_hello",
            false,
            format!(
                "{}\n",
                json!({"type": "call", "call_id": id(), "operation": "fx.echo", "input": {}})
            )
            .into_bytes(),
        ),
        (
            "second_hello",
            true,
            format!(
                "{}\n",
                json!({"type": "hello", "wire": ["keeper.wire.v1"], "client": "test/1"})
            )
            .into_bytes(),
        ),
    ];

    for (name, do_hello, payload) in cases {
        let mut bad = if do_hello {
            served.raw().await
        } else {
            Raw::connect(&served.socket()).await
        };
        bad.send_bytes(&payload).await;
        let frames = bad.until_closed().await;
        assert_eq!(frames.len(), 1, "case {name}: {frames:?}");
        assert_eq!(frames[0]["type"], "close", "case {name}: {frames:?}");
        assert_eq!(
            frames[0]["error"]["code"], "INVALID_ARGUMENT",
            "case {name}: {frames:?}"
        );

        let echo_id = id();
        healthy
            .call(&echo_id, "fx.echo", json!({"after": name}))
            .await;
        let reply = healthy.terminal(&echo_id, &mut Vec::new()).await;
        assert_eq!(reply["type"], "reply", "case {name}: {reply}");
        assert_eq!(
            reply["output"],
            json!({"after": name}),
            "case {name}: {reply}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn h8_frame_field_errors_reply_invalid_argument_and_connection_continues() {
    let served = serve(limits(), Hooks::default());
    let mut raw = served.raw().await;

    // Unknown type -> /type
    let bogus_id = id();
    raw.send(&json!({"type": "bogus", "call_id": bogus_id}))
        .await;
    let reply = raw.terminal(&bogus_id, &mut Vec::new()).await;
    assert_eq!(reply["type"], "reply", "{reply}");
    assert_eq!(reply["error"]["code"], "INVALID_ARGUMENT", "{reply}");
    assert_eq!(reply["error"]["field"], "/type", "{reply}");

    let echo1 = id();
    raw.call(&echo1, "fx.echo", json!({"step": 1})).await;
    let r1 = raw.terminal(&echo1, &mut Vec::new()).await;
    assert_eq!(r1["output"], json!({"step": 1}), "{r1}");

    // Call with no operation -> /operation
    let no_op_id = id();
    raw.send(&json!({"type": "call", "call_id": no_op_id, "input": {}}))
        .await;
    let reply = raw.terminal(&no_op_id, &mut Vec::new()).await;
    assert_eq!(reply["type"], "reply", "{reply}");
    assert_eq!(reply["error"]["code"], "INVALID_ARGUMENT", "{reply}");
    assert_eq!(reply["error"]["field"], "/operation", "{reply}");

    let echo2 = id();
    raw.call(&echo2, "fx.echo", json!({"step": 2})).await;
    let r2 = raw.terminal(&echo2, &mut Vec::new()).await;
    assert_eq!(r2["output"], json!({"step": 2}), "{r2}");

    // Call with unknown door "web" -> /door, never dispatched as cli
    let calls_before = Counts::get(&served.fixture.counts.calls);
    let bad_door_id = id();
    raw.send(&json!({
        "type": "call",
        "call_id": bad_door_id,
        "operation": "fx.echo",
        "input": {"door_test": true},
        "door": "web",
    }))
    .await;
    let reply = raw.terminal(&bad_door_id, &mut Vec::new()).await;
    assert_eq!(reply["type"], "reply", "{reply}");
    assert_eq!(reply["error"]["code"], "INVALID_ARGUMENT", "{reply}");
    assert_eq!(reply["error"]["field"], "/door", "{reply}");
    assert_eq!(
        Counts::get(&served.fixture.counts.calls),
        calls_before,
        "unknown door must not be dispatched"
    );

    let echo3 = id();
    raw.call(&echo3, "fx.echo", json!({"step": 3})).await;
    let r3 = raw.terminal(&echo3, &mut Vec::new()).await;
    assert_eq!(r3["output"], json!({"step": 3}), "{r3}");
}

// --- H9 (wire half): version negotiation & unknown fields ---

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn h9_unsupported_wire_version_closes_with_v1_and_skips_pipelined_call() {
    let served = serve(limits(), Hooks::default());
    let mut raw = Raw::connect(&served.socket()).await;

    // Both lines in one write: the call is behind the hello on the wire.
    let pipelined = format!(
        "{}\n{}\n",
        json!({"type": "hello", "wire": ["keeper.wire.v9"], "client": "test/1"}),
        json!({"type": "call", "call_id": id(), "operation": "fx.echo", "input": {"should_not_run": true}})
    );
    raw.send_bytes(pipelined.as_bytes()).await;

    let frames = raw.until_closed().await;
    assert_eq!(frames.len(), 1, "{frames:?}");
    assert_eq!(frames[0]["type"], "close", "{frames:?}");
    let offered = frames[0]["wire"]
        .as_array()
        .expect("close frame wire array");
    assert!(
        offered.contains(&json!("keeper.wire.v1")),
        "close wire does not advertise keeper.wire.v1: {frames:?}"
    );
    assert_eq!(Counts::get(&served.fixture.counts.calls), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn h9_version_negotiation_and_unknown_fields_are_ignored() {
    let served = serve(limits(), Hooks::default());

    // Negotiate keeper.wire.v1 out of ["keeper.wire.v9", "keeper.wire.v1"]
    let mut neg = Raw::connect(&served.socket()).await;
    neg.send(&json!({
        "type": "hello",
        "wire": ["keeper.wire.v9", "keeper.wire.v1"],
        "client": "test/1",
    }))
    .await;
    let hello = neg.frame().await.expect("server hello");
    assert_eq!(hello["type"], "hello", "{hello}");
    assert_eq!(hello["wire"], "keeper.wire.v1", "{hello}");

    // Unknown fields in hello and call are ignored
    let mut ext = Raw::connect(&served.socket()).await;
    ext.send(&json!({
        "type": "hello",
        "wire": ["keeper.wire.v1"],
        "client": "test/1",
        "x_new": 1,
    }))
    .await;
    let hello = ext.frame().await.expect("server hello");
    assert_eq!(hello["type"], "hello", "{hello}");
    assert_eq!(hello["wire"], "keeper.wire.v1", "{hello}");

    let call_id = id();
    ext.send(&json!({
        "type": "call",
        "call_id": call_id,
        "operation": "fx.echo",
        "input": {"ok": 42},
        "x_new": {},
    }))
    .await;
    let reply = ext.terminal(&call_id, &mut Vec::new()).await;
    assert_eq!(reply["type"], "reply", "{reply}");
    assert_eq!(reply["output"], json!({"ok": 42}), "{reply}");
}

// --- H10: call_id lifecycle ---

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn h10a_reusing_in_flight_call_id_closes_invalid_argument_without_reply() {
    type Frame = fn(&str) -> Value;
    let cases: [(&str, Frame); 3] = [
        (
            "second_fx_held",
            |cid| json!({"type": "call", "call_id": cid, "operation": "fx.held", "input": {}}),
        ),
        ("bogus_type", |cid| json!({"type": "bogus", "call_id": cid})),
        (
            "call_naming_stream",
            |cid| json!({"type": "call", "call_id": cid, "operation": "fx.stream", "input": {}}),
        ),
    ];

    for (name, second_frame) in cases {
        let served = serve(limits(), Hooks::default());
        let mut raw = served.raw().await;

        let in_flight = id();
        raw.call(&in_flight, "fx.held", json!({})).await;
        assert!(
            eventually(Duration::from_secs(2), || {
                served.server().calls_in_flight() == 1
            })
            .await,
            "case {name}: fx.held did not enter flight"
        );

        raw.send(&second_frame(&in_flight)).await;
        let frames = raw.until_closed().await;
        assert_eq!(frames.len(), 1, "case {name}: {frames:?}");
        assert_eq!(frames[0]["type"], "close", "case {name}: {frames:?}");
        assert_eq!(
            frames[0]["error"]["code"], "INVALID_ARGUMENT",
            "case {name}: {frames:?}"
        );
        assert_eq!(
            frames[0]["error"]["field"], "/call_id",
            "case {name}: {frames:?}"
        );
        assert!(
            !frames
                .iter()
                .any(|f| f.get("call_id").and_then(Value::as_str) == Some(in_flight.as_str())),
            "case {name}: reply written for {in_flight} before close: {frames:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn h10b_reuse_same_call_id_across_1000_rounds() {
    let served = serve(limits(), Hooks::default());
    let mut raw = served.raw().await;
    let call_id = id();

    for i in 0..1000u64 {
        raw.call(&call_id, "fx.echo", json!({"round": i})).await;
        let reply = raw.terminal(&call_id, &mut Vec::new()).await;
        assert_eq!(reply["type"], "reply", "round {i}: {reply}");
        assert_eq!(reply["output"], json!({"round": i}), "round {i}: {reply}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn h10c_cancel_then_immediate_reuse_answers_cancelled_then_echo() {
    let served = serve(limits(), Hooks::default());
    let mut raw = served.raw().await;
    let x = id();

    raw.call(&x, "fx.held", json!({"first": true})).await;
    assert!(
        eventually(Duration::from_secs(2), || {
            served.server().calls_in_flight() == 1
        })
        .await,
        "fx.held did not enter flight"
    );

    raw.cancel(&x).await;
    raw.call(&x, "fx.echo", json!({"second": true})).await;

    let first = read_json_line(&mut raw.read)
        .await
        .expect("cancelled reply for X");
    assert_eq!(first["type"], "reply", "{first}");
    assert_eq!(first["call_id"], x, "{first}");
    assert_eq!(first["error"]["code"], "CANCELLED", "{first}");

    let second = read_json_line(&mut raw.read)
        .await
        .expect("echo reply for X");
    assert_eq!(second["type"], "reply", "{second}");
    assert_eq!(second["call_id"], x, "{second}");
    assert_eq!(second["output"], json!({"second": true}), "{second}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn h10d_same_call_id_on_two_connections_both_run_and_answer() {
    let served = serve(limits(), Hooks::default());
    let mut c1 = served.raw().await;
    let mut c2 = served.raw().await;
    let shared = id();

    c1.call(&shared, "fx.held", json!({"conn": 1})).await;
    c2.call(&shared, "fx.held", json!({"conn": 2})).await;

    assert!(
        eventually(Duration::from_secs(2), || {
            served.server().calls_in_flight() == 2
        })
        .await,
        "expected 2 calls in flight, got {}",
        served.server().calls_in_flight()
    );

    served.fixture.gate.open();

    let r1 = c1.terminal(&shared, &mut Vec::new()).await;
    let r2 = c2.terminal(&shared, &mut Vec::new()).await;
    assert_eq!(r1["type"], "reply", "{r1}");
    assert_eq!(r1["output"], json!({"conn": 1}), "{r1}");
    assert_eq!(r2["type"], "reply", "{r2}");
    assert_eq!(r2["output"], json!({"conn": 2}), "{r2}");
}
