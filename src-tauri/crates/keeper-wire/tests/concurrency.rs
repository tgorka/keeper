//! 123.3-AC14 H4, H5: per-connection concurrency, listener ceilings, and
//! unread-answer backpressure over a real socket.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::HashMap;
use std::time::Duration;

use common::{eventually, id, limits, serve, serve_with, Counts, Fixture, Raw};
use keeper_wire::Hooks;
use serde_json::{json, Value};

async fn send_held_wave(conns: &mut [Raw], per_conn: usize) -> Vec<HashMap<String, Value>> {
    let mut expected = Vec::with_capacity(conns.len());
    for (c, conn) in conns.iter_mut().enumerate() {
        let mut map = HashMap::with_capacity(per_conn);
        for k in 0..per_conn {
            let call_id = id();
            let input = json!({"c": c, "k": k});
            conn.call(&call_id, "fx.held", input.clone()).await;
            map.insert(call_id, input);
        }
        expected.push(map);
    }
    expected
}

async fn drain_wave(conns: &mut [Raw], mut expected: Vec<HashMap<String, Value>>) {
    for (conn, map) in conns.iter_mut().zip(expected.iter_mut()) {
        let count = map.len();
        for _ in 0..count {
            let reply = conn.frame().await.expect("held reply");
            assert_eq!(reply["type"], "reply", "{reply}");
            assert!(reply.get("error").is_none(), "{reply}");
            let call_id = reply["call_id"].as_str().expect("call_id");
            let want = map
                .remove(call_id)
                .unwrap_or_else(|| panic!("unexpected or duplicate call_id {call_id}: {reply}"));
            assert_eq!(reply["output"], want, "{reply}");
        }
        assert!(map.is_empty());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn h4_concurrent_barrier_rounds_and_per_connection_ceiling() {
    let served = serve(limits(), Hooks::default());
    let mut raw = served.raw().await;

    // A dispatcher serving one call at a time would deadlock at the 64-party
    // barrier because the first call never finishes until all 64 arrive.
    tokio::time::timeout(Duration::from_secs(120), async {
        for round in 0..1000usize {
            let mut expected = HashMap::with_capacity(64);
            for k in 0..64usize {
                let call_id = id();
                let input = json!({"i": round * 64 + k});
                raw.call(&call_id, "fx.barrier", input.clone()).await;
                expected.insert(call_id, input);
            }
            for _ in 0..64 {
                let reply = raw.frame().await.expect("barrier reply");
                assert_eq!(reply["type"], "reply", "{reply}");
                assert!(reply.get("error").is_none(), "{reply}");
                let call_id = reply["call_id"].as_str().expect("call_id");
                let want = expected.remove(call_id).unwrap_or_else(|| {
                    panic!("round {round}: unexpected or duplicate call_id {call_id}: {reply}")
                });
                assert_eq!(reply["output"], want, "round {round}: {reply}");
            }
            assert!(expected.is_empty());
        }
    })
    .await
    .expect("1000 rounds of 64 concurrent barrier calls within 120 s");

    let mut held_expected = HashMap::with_capacity(64);
    for k in 0..64usize {
        let call_id = id();
        let input = json!({"held": k});
        raw.call(&call_id, "fx.held", input.clone()).await;
        held_expected.insert(call_id, input);
    }
    assert!(
        eventually(Duration::from_secs(5), || {
            served.server().calls_in_flight() == 64
        })
        .await,
        "expected 64 held calls in flight, got {}",
        served.server().calls_in_flight()
    );

    let extra = id();
    raw.call(&extra, "fx.echo", json!({"extra": true})).await;
    let mut others = Vec::new();
    let reply = raw.terminal(&extra, &mut others).await;
    assert!(
        others.is_empty(),
        "unexpected frames while gate closed: {others:?}"
    );
    assert_eq!(reply["type"], "reply", "{reply}");
    assert_eq!(reply["call_id"], extra, "{reply}");
    assert_eq!(reply["error"]["code"], "RESOURCE_EXHAUSTED", "{reply}");

    served.fixture.gate.open();
    for _ in 0..64 {
        let reply = raw.frame().await.expect("held reply");
        assert_eq!(reply["type"], "reply", "{reply}");
        assert!(reply.get("error").is_none(), "{reply}");
        let call_id = reply["call_id"].as_str().expect("call_id");
        let want = held_expected
            .remove(call_id)
            .unwrap_or_else(|| panic!("unexpected call_id {call_id}: {reply}"));
        assert_eq!(reply["output"], want, "{reply}");
    }
    assert!(held_expected.is_empty());

    let after = id();
    raw.call(&after, "fx.echo", json!({"after": true})).await;
    let reply = raw.terminal(&after, &mut Vec::new()).await;
    assert_eq!(reply["type"], "reply", "{reply}");
    assert_eq!(reply["output"], json!({"after": true}), "{reply}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn h5_listener_ceiling_and_connection_cap_across_sixteen_connections() {
    let served = serve_with(Fixture::new(64), limits(), Hooks::default());
    let mut conns = Vec::with_capacity(16);
    for _ in 0..16 {
        conns.push(served.raw().await);
    }

    let wave1 = send_held_wave(&mut conns, 8).await;
    assert!(
        eventually(Duration::from_secs(5), || {
            served.server().calls_in_flight() == 128
        })
        .await,
        "expected 128 calls in flight, got {}",
        served.server().calls_in_flight()
    );

    let extra_call = id();
    conns[0]
        .call(&extra_call, "fx.echo", json!({"overflow": true}))
        .await;
    let mut others = Vec::new();
    let reply = conns[0].terminal(&extra_call, &mut others).await;
    assert!(
        others.is_empty(),
        "unexpected frames while gate closed: {others:?}"
    );
    assert_eq!(reply["type"], "reply", "{reply}");
    assert_eq!(reply["call_id"], extra_call, "{reply}");
    assert_eq!(reply["error"]["code"], "RESOURCE_EXHAUSTED", "{reply}");

    let mut extra_conn = Raw::connect(&served.socket()).await;
    let frames = extra_conn.until_closed().await;
    assert_eq!(frames.len(), 1, "{frames:?}");
    assert_eq!(frames[0]["type"], "close", "{frames:?}");
    assert_eq!(
        frames[0]["error"]["code"], "RESOURCE_EXHAUSTED",
        "{frames:?}"
    );

    served.fixture.gate.open();
    drain_wave(&mut conns, wave1).await;
    assert!(
        eventually(Duration::from_secs(5), || {
            served.server().calls_in_flight() == 0
        })
        .await,
        "expected 0 calls in flight after drain, got {}",
        served.server().calls_in_flight()
    );

    served.fixture.gate.close();
    let wave2 = send_held_wave(&mut conns, 8).await;
    assert!(
        eventually(Duration::from_secs(5), || {
            served.server().calls_in_flight() == 128
        })
        .await,
        "expected 128 calls in flight on second wave, got {}",
        served.server().calls_in_flight()
    );

    served.fixture.gate.open();
    drain_wave(&mut conns, wave2).await;
    assert!(
        eventually(Duration::from_secs(5), || {
            served.server().calls_in_flight() == 0
        })
        .await,
        "expected 0 calls in flight after second drain, got {}",
        served.server().calls_in_flight()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn h5_one_connection_at_ceiling_does_not_block_another() {
    let served = serve(limits(), Hooks::default());
    let mut conn_a = served.raw().await;
    let mut conn_b = served.raw().await;

    let mut held_expected = HashMap::with_capacity(64);
    for k in 0..64usize {
        let call_id = id();
        let input = json!({"a": k});
        conn_a.call(&call_id, "fx.held", input.clone()).await;
        held_expected.insert(call_id, input);
    }
    assert!(
        eventually(Duration::from_secs(5), || {
            served.server().calls_in_flight() == 64
        })
        .await,
        "expected 64 calls in flight on A, got {}",
        served.server().calls_in_flight()
    );

    let extra_a = id();
    conn_a
        .call(&extra_a, "fx.echo", json!({"a_overflow": true}))
        .await;
    let mut others = Vec::new();
    let reply_a = conn_a.terminal(&extra_a, &mut others).await;
    assert!(others.is_empty(), "{others:?}");
    assert_eq!(reply_a["type"], "reply", "{reply_a}");
    assert_eq!(reply_a["error"]["code"], "RESOURCE_EXHAUSTED", "{reply_a}");

    let call_b = id();
    conn_b.call(&call_b, "fx.echo", json!({"b": 1})).await;
    let reply_b = conn_b.terminal(&call_b, &mut Vec::new()).await;
    assert_eq!(reply_b["type"], "reply", "{reply_b}");
    assert_eq!(reply_b["output"], json!({"b": 1}), "{reply_b}");

    served.fixture.gate.open();
    for _ in 0..64 {
        let reply = conn_a.frame().await.expect("held reply on A");
        let call_id = reply["call_id"].as_str().expect("call_id");
        let want = held_expected.remove(call_id).expect("known id");
        assert_eq!(reply["output"], want, "{reply}");
    }
    assert!(held_expected.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn h5_unread_answers_hold_their_places_without_blocking_other_connections() {
    let served = serve(limits(), Hooks::default());
    let mut conn_a = served.raw().await;
    let mut conn_b = served.raw().await;

    for _ in 0..64 {
        let call_id = id();
        conn_a
            .call(&call_id, "fx.big", json!({"bytes": 400_000}))
            .await;
    }
    assert!(
        eventually(Duration::from_secs(5), || {
            Counts::get(&served.fixture.counts.calls) == 64
        })
        .await,
        "expected 64 fx.big handlers to run, got {}",
        Counts::get(&served.fixture.counts.calls)
    );
    tokio::time::sleep(Duration::from_millis(300)).await;

    let extra_a = id();
    conn_a
        .call(&extra_a, "fx.echo", json!({"blocked": true}))
        .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        Counts::get(&served.fixture.counts.calls),
        64,
        "65th call on A must be refused before dispatch while A's 64 answers wait unwritten"
    );

    let call_b = id();
    conn_b.call(&call_b, "fx.echo", json!({"b": 2})).await;
    let reply_b = conn_b.terminal(&call_b, &mut Vec::new()).await;
    assert_eq!(reply_b["type"], "reply", "{reply_b}");
    assert_eq!(reply_b["output"], json!({"b": 2}), "{reply_b}");
    assert_eq!(Counts::get(&served.fixture.counts.calls), 65);
}
