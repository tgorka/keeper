//! An approval is consumed once on a real homeserver (story 93.2,
//! acceptance 10; ruling R75): the `consumed` event is an unencrypted state
//! event keyed by the approval's id, the server refuses it from a person,
//! and the first one in the room's order — read forward from the request —
//! is the consumption whichever copy reads it.
//!
//! `consume_once_on_a_real_homeserver` is a sequential transport check:
//! copy A sends the request, uploads the action's canonical bytes as an
//! encrypted file, consumes, and is dropped; copy B, a fresh client that
//! never saw A's Megolm sessions, reads the room forward from the request
//! and finds A's `consumed` first, and its own, sent after, is not first.
//! A person's `consumed` is refused by the server.
//!
//! `two_copies_racing_run_the_effect_once` is the race at the transport
//! and effect level (R181): two copies send their `consumed` at the same
//! time through the production adapter and the production decision
//! (`ClientApprovals`, `consume_once`); only a winner performs the effect,
//! and over several approvals exactly one effect happens each time — the
//! copy the room ordered first. Two whole hosts with lease expiry and a
//! crash between acceptance and the local mirror are not driven here
//! (DW-503).
//!
//! `#[ignore]`; endpoints and secrets come from the environment:
//!
//! ```sh
//! KEEPER_AGENTS_SMOKE_HOMESERVER=http://100.101.101.23:8008 \
//! KEEPER_AGENTS_SMOKE_SECRETS=$HOME/.config/keeper-smoke/synapse.env \
//! cargo test --manifest-path src-tauri/Cargo.toml -p keeper-agentd --test live_approvals -- --ignored --nocapture
//! ```

// matrix-sdk's sync future is deep enough to need it, as in the library.
#![recursion_limit = "256"]
#![cfg(target_os = "linux")]

use keeper_core::agents::approval::sha256_hex;
use keeper_core::agents::events::{ConsumedContent, APPROVAL_CONSUMED, APPROVAL_REQUEST};
use keeper_core::agents::matrix::RoomKind;
use keeper_core::agents::session::SessionKind;
use serde_json::json;

mod common;

use common::Smoke;

#[ignore = "live: Synapse on delectra"]
#[tokio::test(flavor = "multi_thread")]
async fn consume_once_on_a_real_homeserver() {
    let smoke = Smoke::from_env();
    let root = tempfile::tempdir().expect("tempdir");
    let nixi = smoke.user("nixi-smoke");
    let tgorka = smoke.user("tgorka-smoke");
    smoke.clear_devices(&nixi).await;
    smoke.set_ratelimit(&nixi, 0, 0).await;
    let password = smoke.secret("NIXI_SMOKE_PASSWORD").to_owned();
    let a = smoke
        .client(&root.path().join("a"), "nixi-smoke", &password)
        .await;
    let room = a
        .create_room(
            RoomKind::Session(SessionKind::Conversation),
            "approvals smoke",
            vec![tgorka.clone()],
            &[],
        )
        .await
        .expect("a session room");
    let person = smoke
        .client(
            &root.path().join("tgorka"),
            "tgorka-smoke",
            smoke.secret("TGORKA_SMOKE_PASSWORD"),
        )
        .await;
    person.join(&room).await.expect("the person joins");
    person.sync_once().await.expect("sync");

    let id = ulid::Ulid::new().to_string();
    // R86: the action's canonical bytes, encrypted, by reference.
    let canonical = format!(
        "{{\"content\":\"{}\",\"path\":\"a.md\",\"profile\":\"tgdrive\"}}",
        "x".repeat(20 * 1024)
    );
    let file = a
        .upload_encrypted(canonical.as_bytes())
        .await
        .expect("the encrypted upload");
    assert!(
        file["url"]
            .as_str()
            .is_some_and(|url| url.starts_with("mxc://")),
        "{file}"
    );
    assert!(
        file["key"].is_object() && file["hashes"]["sha256"].is_string(),
        "{file}"
    );
    a.sync_once().await.expect("sync");
    let request = a
        .send(
            &room,
            APPROVAL_REQUEST,
            json!({"v": 1, "id": id, "file": file, "file_sha256": sha256_hex(canonical.as_bytes())}),
            None,
        )
        .await
        .expect("the request");

    // Q3(b): a person cannot send the consumed state event.
    let forged = person
        .send_state(
            &room,
            APPROVAL_CONSUMED,
            &id,
            &json!({"v": 1, "id": id, "epoch": 9, "host": "forged"}),
        )
        .await;
    assert!(
        forged.is_err(),
        "the server took a person's consumed event: {forged:?}"
    );

    let consumed = |host: &str| {
        serde_json::to_value(ConsumedContent {
            v: 1,
            id: id.clone(),
            epoch: 4,
            host: host.to_owned(),
        })
        .expect("json")
    };
    let mine = a
        .send_state(&room, APPROVAL_CONSUMED, &id, &consumed("electra"))
        .await
        .expect("A's consumed is accepted");
    let first_for_a = a
        .state_events_from(&room, Some(&request), APPROVAL_CONSUMED, &id)
        .await
        .expect("A reads forward");
    assert!(first_for_a.complete);
    assert_eq!(
        first_for_a.found.first().map(|s| s.event_id.clone()),
        Some(mine.clone())
    );
    // A dies here, before pushing anything.
    drop(a);

    let b = smoke
        .client(&root.path().join("b"), "nixi-smoke", &password)
        .await;
    let seen = b
        .state_events_from(&room, Some(&request), APPROVAL_CONSUMED, &id)
        .await
        .expect("B reads forward from the request")
        .found;
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(seen[0].event_id, mine);
    assert_eq!(seen[0].sender, nixi);
    assert_eq!(seen[0].content["host"], "electra");

    // B's own consumed, sent after A's, is accepted but is not first.
    let theirs = b
        .send_state(&room, APPROVAL_CONSUMED, &id, &consumed("hesperia"))
        .await
        .expect("B's consumed is accepted too");
    let order = b
        .state_events_from(&room, Some(&request), APPROVAL_CONSUMED, &id)
        .await
        .expect("B reads again")
        .found;
    let ids: Vec<_> = order.iter().map(|state| state.event_id.clone()).collect();
    assert_eq!(ids, [mine.clone(), theirs], "room order");
    // The server's current state shows only the last — why the first is
    // read from the timeline.
    let current = b
        .server_state(&room, APPROVAL_CONSUMED, &id)
        .await
        .expect("state")
        .expect("one");
    assert_eq!(current.content["host"], "hesperia");
    // Read from the room's start, as a host that lost the request event
    // does, the first is still A's.
    let from_start = b
        .state_events_from(&room, None, APPROVAL_CONSUMED, &id)
        .await
        .expect("B reads from the start")
        .found;
    assert_eq!(from_start.first().map(|s| s.event_id.clone()), Some(mine));
}

#[ignore = "live: Synapse on delectra"]
#[tokio::test(flavor = "multi_thread")]
async fn two_copies_racing_run_the_effect_once() {
    use keeper_agent::approvals::{consume_once, ApprovalRoom, Consumption};
    use keeper_agent::runtime::ClientApprovals;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    let smoke = Smoke::from_env();
    let root = tempfile::tempdir().expect("tempdir");
    let nixi = smoke.user("nixi-smoke");
    let tgorka = smoke.user("tgorka-smoke");
    smoke.clear_devices(&nixi).await;
    smoke.set_ratelimit(&nixi, 0, 0).await;
    let password = smoke.secret("NIXI_SMOKE_PASSWORD").to_owned();
    let a = smoke
        .client(&root.path().join("a"), "nixi-smoke", &password)
        .await;
    let room = a
        .create_room(
            RoomKind::Session(SessionKind::Conversation),
            "approvals race smoke",
            vec![tgorka],
            &[],
        )
        .await
        .expect("a session room");
    let b = smoke
        .client(&root.path().join("b"), "nixi-smoke", &password)
        .await;
    a.sync_once().await.expect("sync a");
    b.sync_once().await.expect("sync b");
    let copies: [(Arc<dyn ApprovalRoom>, &str); 2] = [
        (
            Arc::new(ClientApprovals::new(
                a.clone(),
                room.clone(),
                nixi.clone(),
                "electra",
            )),
            "electra",
        ),
        (
            Arc::new(ClientApprovals::new(
                b.clone(),
                room.clone(),
                nixi.clone(),
                "hesperia",
            )),
            "hesperia",
        ),
    ];

    for round in 0..5 {
        let id = ulid::Ulid::new().to_string();
        let request = a
            .send(&room, APPROVAL_REQUEST, json!({"v": 1, "id": id}), None)
            .await
            .expect("the request");
        // The effect: what only a winner may do, counted where both see it.
        let effects = Arc::new(AtomicUsize::new(0));
        let done_by = Arc::new(Mutex::new(Vec::new()));
        let racers = copies.clone().map(|(port, host)| {
            let (id, request) = (id.clone(), request.clone());
            let (effects, done_by) = (Arc::clone(&effects), Arc::clone(&done_by));
            tokio::spawn(async move {
                let content = ConsumedContent {
                    v: 1,
                    id,
                    epoch: 4,
                    host: host.to_owned(),
                };
                let came = consume_once(port.as_ref(), content, Some(&request), None).await;
                if let Consumption::Won(event) = &came {
                    effects.fetch_add(1, Ordering::SeqCst);
                    done_by.lock().expect("lock").push((host, event.clone()));
                }
                came
            })
        });
        let mut outcomes = Vec::new();
        for racer in racers {
            outcomes.push(racer.await.expect("a racer"));
        }
        assert!(
            outcomes
                .iter()
                .all(|came| !matches!(came, Consumption::Unknown(_))),
            "round {round}: every copy decides: {outcomes:?}"
        );
        assert_eq!(
            effects.load(Ordering::SeqCst),
            1,
            "round {round}: exactly one effect: {outcomes:?}"
        );
        let (winner, event) = done_by.lock().expect("lock")[0].clone();
        let order = b
            .state_events_from(&room, Some(&request), APPROVAL_CONSUMED, &id)
            .await
            .expect("the room's order");
        assert!(order.complete);
        assert_eq!(order.found.len(), 2, "round {round}: both were accepted");
        assert_eq!(order.found[0].event_id, event, "round {round}");
        assert_eq!(order.found[0].content["host"], winner, "round {round}");
    }
}
