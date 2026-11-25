//! 123.3-AC14 H11: the start decision and the cancel/deadline answer are one step.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{eventually, id, limits, serve_with, Counts, Fixture, Gate, Served};
use keeper_wire::Hooks;
use serde_json::{json, Value};

fn serve_racing() -> (Served, Gate) {
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

fn check(reply: &Value, code: &str, ran: usize, tally: &mut [usize; 3]) -> Option<String> {
    if reply.get("output").is_some() && reply.get("error").is_none() {
        tally[0] += 1;
        if ran != 1 {
            return Some(format!("success reply with ran={ran}: {reply}"));
        }
        return None;
    }
    let actual_code = reply["error"]["code"].as_str().unwrap_or("<missing>");
    if actual_code != code {
        return Some(format!(
            "expected error code {code}, got {actual_code} (ran={ran}): {reply}"
        ));
    }
    let has_id = reply["error"].get("request_id").is_some();
    if has_id {
        tally[1] += 1;
    } else {
        tally[2] += 1;
    }
    if has_id != (ran == 1) {
        return Some(format!("has_request_id={has_id} but ran={ran}: {reply}"));
    }
    None
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancel_racing_the_start_gate_answers_by_whether_it_started() {
    let (served, start_gate) = serve_racing();
    let mut raw = served.raw().await;
    let mut tally = [0usize; 3];
    let mut violations = Vec::new();

    for round in 0..1000usize {
        start_gate.close();
        let before = Counts::get(&served.fixture.counts.commands_started);
        let cmd = id();
        raw.call(&cmd, "fx.cmd", json!({})).await;
        assert!(
            eventually(Duration::from_secs(2), || {
                served.server().calls_in_flight() == 1
            })
            .await,
            "round {round}: command did not take its place at the start gate"
        );

        let spins = (round % 8) * 40;
        let g = start_gate.clone();
        let opener = tokio::spawn(async move {
            for _ in 0..spins {
                std::hint::spin_loop();
            }
            g.open();
        });
        raw.cancel(&cmd).await;
        opener.await.unwrap();

        let mut others = Vec::new();
        let reply = raw.terminal(&cmd, &mut others).await;
        assert!(
            others.is_empty(),
            "round {round}: unexpected frames: {others:?}"
        );
        assert!(
            eventually(Duration::from_secs(2), || {
                served.server().calls_in_flight() == 0
            })
            .await,
            "round {round}: place was not released"
        );

        let ran = Counts::get(&served.fixture.counts.commands_started) - before;
        if let Some(violation) = check(&reply, "CANCELLED", ran, &mut tally) {
            violations.push(format!("round {round}: {violation}"));
        }
    }

    eprintln!(
        "cancel race: success={} with_request_id={} without={}",
        tally[0], tally[1], tally[2]
    );
    assert!(
        violations.is_empty(),
        "{} of 1000 rounds answered against what ran: {:?}",
        violations.len(),
        &violations[..violations.len().min(5)]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deadline_racing_the_start_gate_answers_by_whether_it_started() {
    let (served, start_gate) = serve_racing();
    let mut raw = served.raw().await;
    let mut tally = [0usize; 3];
    let mut violations = Vec::new();

    for round in 0..1000usize {
        start_gate.close();
        let before = Counts::get(&served.fixture.counts.commands_started);
        let cmd = id();
        raw.send(&json!({
            "type": "call",
            "call_id": cmd,
            "operation": "fx.cmd",
            "input": {},
            "deadline_ms": 3
        }))
        .await;

        tokio::time::sleep(Duration::from_millis(3)).await;
        let spins = (round % 8) * 40;
        for _ in 0..spins {
            std::hint::spin_loop();
        }
        start_gate.open();

        let mut others = Vec::new();
        let reply = raw.terminal(&cmd, &mut others).await;
        assert!(
            others.is_empty(),
            "round {round}: unexpected frames: {others:?}"
        );
        assert!(
            eventually(Duration::from_secs(2), || {
                served.server().calls_in_flight() == 0
            })
            .await,
            "round {round}: place was not released"
        );

        let ran = Counts::get(&served.fixture.counts.commands_started) - before;
        if let Some(violation) = check(&reply, "DEADLINE_EXCEEDED", ran, &mut tally) {
            violations.push(format!("round {round}: {violation}"));
        }
    }

    eprintln!(
        "deadline race: success={} with_request_id={} without={}",
        tally[0], tally[1], tally[2]
    );
    assert!(
        violations.is_empty(),
        "{} of 1000 rounds answered against what ran: {:?}",
        violations.len(),
        &violations[..violations.len().min(5)]
    );
}
