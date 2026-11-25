//! 123.3-AC14 H3: stream backpressure, writer stall, and unadmitted refusal bound.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::Duration;

use common::{eventually, id, limits, serve, Counts};
use keeper_wire::Hooks;
use serde_json::json;
use tokio::io::AsyncReadExt as _;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stream_bound_drops_handler_and_ends_resource_exhausted() {
    let mut lim = limits();
    lim.stall = Duration::from_secs(60);
    lim.stream_events = 32;
    lim.stream_bytes = 1 << 20;
    let served = serve(lim.clone(), Hooks::default());
    let mut raw = served.raw().await;

    let stream_id = id();
    raw.watch(&stream_id, "fx.stream", json!({"pad": 2000}))
        .await;

    for expected_seq in 1..=3u64 {
        let frame = raw.frame().await.expect("initial stream event");
        assert_eq!(frame["type"], "event", "{frame}");
        assert_eq!(frame["call_id"], stream_id);
        assert_eq!(frame["seq"], expected_seq);
    }

    // Stop reading until the server drops the stream handler under backpressure.
    assert!(
        eventually(Duration::from_secs(5), || {
            Counts::get(&served.fixture.counts.drops) == 1
        })
        .await,
        "stream handler was not dropped within 5 s (drops={}, produced={})",
        Counts::get(&served.fixture.counts.drops),
        Counts::get(&served.fixture.counts.produced),
    );

    let mut last_seq_read: u64 = 3;
    let end = loop {
        let frame = raw.frame().await.expect("frame before stream end");
        assert_eq!(frame["call_id"], stream_id, "{frame}");
        match frame["type"].as_str() {
            Some("event") => {
                last_seq_read = frame["seq"].as_u64().unwrap();
            }
            Some("end") => break frame,
            other => panic!("unexpected frame type {other:?}: {frame}"),
        }
    };
    assert_eq!(end["error"]["code"], "RESOURCE_EXHAUSTED", "{end}");

    assert!(
        raw.frame_within(Duration::from_millis(300)).await.is_err(),
        "expected no further frames and connection to stay open"
    );

    let produced = Counts::get(&served.fixture.counts.produced) as u64;
    assert!(
        produced <= last_seq_read + lim.stream_events as u64 + 2,
        "produced {produced} exceeded last_seq_read ({last_seq_read}) + stream_events ({}) + 2",
        lim.stream_events,
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stalled_and_trickling_clients_close_without_blocking_others() {
    let mut lim = limits();
    lim.stall = Duration::from_millis(500);
    let served = serve(lim.clone(), Hooks::default());

    let mut client_a = served.raw().await;
    let mut client_b = served.raw().await;

    for _ in 0..8 {
        client_a
            .call(&id(), "fx.big", json!({"bytes": 400_000}))
            .await;
    }
    assert!(
        eventually(Duration::from_secs(2), || {
            Counts::get(&served.fixture.counts.calls) >= 8
        })
        .await,
        "client A calls were not dispatched"
    );

    // Connection B stays responsive while A is stalled.
    let b_id = id();
    client_b.call(&b_id, "fx.echo", json!({"ok": true})).await;
    let b_reply = client_b.terminal(&b_id, &mut Vec::new()).await;
    assert_eq!(b_reply["type"], "reply", "{b_reply}");
    assert_eq!(b_reply["output"], json!({"ok": true}));
    drop(client_b);

    // Wait for A's stalled calls to be dropped when the server closes A.
    assert!(
        eventually(Duration::from_secs(3), || {
            served.server().calls_in_flight() == 0
        })
        .await,
        "stalled client A calls remained in flight ({})",
        served.server().calls_in_flight(),
    );

    // Drain A's socket and assert EOF arrives within 3 s.
    let eof_a = tokio::time::timeout(Duration::from_secs(3), async {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match client_a.read.read(&mut buf).await {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
        }
    })
    .await;
    assert!(eof_a.is_ok(), "client A did not reach EOF within 3 s");
    drop(client_a);

    // Client C reads one byte every 100 ms; per-frame age still exceeds 500 ms.
    let mut client_c = served.raw().await;
    let calls_before_c = Counts::get(&served.fixture.counts.calls);
    for _ in 0..8 {
        client_c
            .call(&id(), "fx.big", json!({"bytes": 400_000}))
            .await;
    }
    assert!(
        eventually(Duration::from_secs(2), || {
            Counts::get(&served.fixture.counts.calls) >= calls_before_c + 8
        })
        .await,
        "client C calls were not dispatched"
    );

    let mut byte = [0u8; 1];
    let trickle_deadline = tokio::time::Instant::now() + Duration::from_secs(4);
    let mut closed_during_trickle = false;
    loop {
        if tokio::time::Instant::now() >= trickle_deadline {
            break;
        }
        match tokio::time::timeout(
            Duration::from_millis(500),
            client_c.read.read_exact(&mut byte),
        )
        .await
        {
            Ok(Ok(_)) => {}
            Ok(Err(_)) => {
                closed_during_trickle = true;
                break;
            }
            Err(_) => panic!("client C timed out waiting for a single byte"),
        }
        if served.server().calls_in_flight() == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    assert!(
        served.server().calls_in_flight() == 0,
        "client C was not closed by stall timer while trickling 1 byte / 100 ms (in_flight={})",
        served.server().calls_in_flight(),
    );

    if !closed_during_trickle {
        let eof_c = tokio::time::timeout(Duration::from_secs(3), async {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match client_c.read.read(&mut buf).await {
                    Ok(0) | Err(_) => return,
                    Ok(_) => {}
                }
            }
        })
        .await;
        assert!(eof_c.is_ok(), "client C did not reach EOF within 3 s");
    }
    drop(client_c);

    assert!(
        eventually(Duration::from_secs(3), || {
            served.server().calls_in_flight() == 0
        })
        .await,
        "calls_in_flight did not return to 0"
    );
    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut fresh = Vec::with_capacity(lim.connections);
    for idx in 0..lim.connections {
        let mut conn = served.raw().await;
        let cid = id();
        conn.call(&cid, "fx.echo", json!({"conn": idx})).await;
        let reply = conn.terminal(&cid, &mut Vec::new()).await;
        assert_eq!(reply["type"], "reply", "{reply}");
        assert_eq!(reply["output"], json!({"conn": idx}));
        fresh.push(conn);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unadmitted_refusal_queue_stops_reading_from_client() {
    let mut lim = limits();
    lim.unadmitted = 4;
    lim.stall = Duration::from_secs(60);
    let served = serve(lim, Hooks::default());
    let mut raw = served.raw().await;

    // Fill the server-to-client socket buffer so refusal replies cannot drain.
    for _ in 0..8 {
        raw.call(&id(), "fx.big", json!({"bytes": 400_000})).await;
    }
    assert!(
        eventually(Duration::from_secs(3), || {
            Counts::get(&served.fixture.counts.calls) >= 8
        })
        .await,
        "initial fx.big calls were not dispatched"
    );
    let calls_before = Counts::get(&served.fixture.counts.calls);

    // Build ~8 MB of bogus-type lines followed by fx.echo probes.
    let pad = "x".repeat(1024);
    let mut huge_buf = Vec::with_capacity(8 * 1024 * 1024 + 4096);
    while huge_buf.len() < 8 * 1024 * 1024 {
        let line = json!({"type": "bogus", "call_id": id(), "pad": pad});
        serde_json::to_writer(&mut huge_buf, &line).unwrap();
        huge_buf.push(b'\n');
    }
    for i in 0..4 {
        let echo =
            json!({"type": "call", "call_id": id(), "operation": "fx.echo", "input": {"i": i}});
        serde_json::to_writer(&mut huge_buf, &echo).unwrap();
        huge_buf.push(b'\n');
    }

    let write_result =
        tokio::time::timeout(Duration::from_secs(3), raw.send_bytes(&huge_buf)).await;
    assert!(
        write_result.is_err(),
        "expected 8 MB write to block once unadmitted refusal queue filled"
    );
    assert_eq!(
        Counts::get(&served.fixture.counts.calls),
        calls_before,
        "trailing fx.echo calls were dispatched despite full unadmitted queue"
    );
}
