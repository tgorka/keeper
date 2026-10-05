//! A person decides an approval card on a real homeserver (story 93.3,
//! rulings R185–R187).
//!
//! The agent (`nixi-smoke`) sends a request whose digest is keeper's own
//! over what it carries. The person (`tgorka-smoke`) reads it on device A,
//! cross-signed with `bootstrap_cross_signing`, through keeper's timeline
//! producer, which keeps running: the card opens pending and decidable.
//! Device B, a fresh login nobody signed, is refused by the device's
//! production decision path (`keeper_core::account::decide_approval`, the
//! body of the `agent_approval_decide` command) and sends nothing; so is a
//! decision on A for another digest, an unoffered scope or an unknown id.
//! B's decision sent by another client is shown on A's card, which stays
//! decidable, and the agent's trust adapter ignores it. A's decision through
//! the production path reaches the agent sealed, for the request's digest,
//! and counts; the agent's `consumed` then closes the card live.
//!
//! What the host does after a decision counts is the session worker's
//! (`agent_turns::parks`); production installs no decision source until
//! rung 6 (R92), so the agent's client and its trust adapter stand in for
//! the host here.
//!
//! `#[ignore]`; endpoints and secrets come from the environment:
//!
//! ```sh
//! KEEPER_AGENTS_SMOKE_HOMESERVER=http://100.101.101.23:8008 \
//! KEEPER_AGENTS_SMOKE_SECRETS=$HOME/.config/keeper-smoke/synapse.env \
//! cargo test --manifest-path src-tauri/Cargo.toml -p keeper-agentd --test live_card -- --ignored --nocapture
//! ```

// matrix-sdk's sync future is deep enough to need it, as in the library.
#![recursion_limit = "256"]
#![cfg(target_os = "linux")]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use keeper_agent::agent::Arrived;
use keeper_agent::deciding::{judge, ClientDecisions, Seat};
use keeper_agent::runtime::arrival_of;
use keeper_core::account::{approval_payload, decide_approval};
use keeper_core::agents::agentd::TrustEntry;
use keeper_core::agents::approval::{
    binding_digest, canonical, sha256_hex, summary_of, Decision, FilePin, Preconditions, Scope,
};
use keeper_core::agents::approval_card::{
    ApprovalCardVm, ApprovalDecideReq, ApprovalStateVm, HostedRooms, NOT_ATTACHED, NOT_FOUND,
    NOT_WAITING, OTHER_ACTION, PAYLOAD_REFUSED, PAYLOAD_UNAVAILABLE, SCOPE_NOT_OFFERED, UNSEEN,
    UNVERIFIED,
};
use keeper_core::agents::events::{
    ApprovalRequestContent, RequestAction, APPROVAL_CONSUMED, APPROVAL_DECISION, APPROVAL_REQUEST,
    CONTENT_VERSION,
};
use keeper_core::agents::matrix::{AgentClient, RoomKind};
use keeper_core::agents::room::{approvals_of, AgentIcons, AgentKinds};
use keeper_core::agents::session::SessionKind;
use keeper_core::agents::tier::AgentTool;
use keeper_core::agents::trust::{Anchor, UNSIGNED_DEVICE};
use keeper_core::error::CoreError;
use keeper_core::timeline;
use keeper_core::vm::TimelineBatch;
use matrix_sdk::deserialized_responses::EncryptionInfo;
use matrix_sdk::ruma::events::AnySyncTimelineEvent;
use matrix_sdk::ruma::serde::Raw;
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId, RoomId};
use serde_json::{json, Value};

mod common;

use common::{bootstrap, syncing, Smoke};

/// Every decrypted timeline event the agent's client is handed, with its
/// encryption.
type Seen = Arc<Mutex<Vec<(Value, Option<EncryptionInfo>)>>>;

fn capture(client: &AgentClient) -> Seen {
    let seen: Seen = Arc::default();
    let sink = Arc::clone(&seen);
    client.client().add_event_handler(
        move |event: Raw<AnySyncTimelineEvent>, encryption: Option<EncryptionInfo>| {
            let sink = Arc::clone(&sink);
            async move {
                if let Ok(value) = event.deserialize_as::<Value>() {
                    sink.lock().expect("lock").push((value, encryption));
                }
            }
        },
    );
    seen
}

/// The card `id` in the latest batch that carried the cards, once `done`
/// holds for it.
async fn card_when(
    batches: &Mutex<Vec<TimelineBatch>>,
    id: &str,
    what: &str,
    done: impl Fn(&ApprovalCardVm) -> bool,
) -> ApprovalCardVm {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        let card = batches
            .lock()
            .expect("lock")
            .iter()
            .rev()
            .find_map(|batch| batch.approvals.clone())
            .and_then(|approvals| {
                approvals
                    .into_iter()
                    .flat_map(|approval| approval.cards)
                    .find(|card| card.id == id)
            });
        match card {
            Some(card) if done(&card) => return card,
            card => assert!(
                tokio::time::Instant::now() < deadline,
                "{what}: the card is {card:?}"
            ),
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// The person's decisions the agent has read, as it reads them.
fn decisions(seen: &Seen) -> Vec<Arrived> {
    seen.lock()
        .expect("lock")
        .iter()
        .filter(|(value, _)| value["type"] == APPROVAL_DECISION)
        .filter_map(|(value, encryption)| {
            arrival_of(value, encryption.as_ref(), tokio::time::Instant::now())
        })
        .collect()
}

async fn decisions_when(seen: &Seen, count: usize) -> Vec<Arrived> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        let arrived = decisions(seen);
        if arrived.len() >= count {
            return arrived;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the agent read {} of {count} decisions",
            arrived.len()
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

fn refused<T: std::fmt::Debug>(result: Result<T, CoreError>) -> String {
    match result {
        Err(CoreError::Unsupported(sentence)) => sentence,
        other => panic!("not refused with a sentence: {other:?}"),
    }
}

fn write(path: &str, content: &str) -> Value {
    json!({"profile": "tgdrive", "path": path, "content": content})
}

/// The agent's request `id` in `room` for a T2 `drive_write` of `args`,
/// once only, `tgorka` asking and deciding: its digest and summary keeper's
/// own over what it carries, as a host's record makes them.
fn request_of(
    id: &str,
    room: &RoomId,
    tgorka: &OwnedUserId,
    args: &Value,
) -> ApprovalRequestContent {
    let session = "60-sessions/active/2026-10-05-card";
    let path = args["path"].as_str().expect("a path");
    let preconditions = Preconditions {
        files: vec![FilePin {
            drive: "tgdrive".to_owned(),
            path: path.to_owned(),
            landing: Some(path.to_owned()),
            sha256: None,
        }],
        ..Preconditions::default()
    };
    let digest = binding_digest(
        id,
        session,
        "nixi",
        "drive_write",
        args,
        &Value::Null,
        "c0ffee",
        &serde_json::to_value(&preconditions).expect("preconditions"),
    )
    .expect("a digest");
    ApprovalRequestContent {
        v: CONTENT_VERSION,
        id: id.to_owned(),
        session: session.to_owned(),
        room: room.to_string(),
        agent: "nixi".to_owned(),
        tier: 2,
        summary: summary_of(AgentTool::from_wire("drive_write").expect("a tool"), args),
        action: RequestAction {
            tool: "drive_write".to_owned(),
            args: args.clone(),
            exec_binding: Value::Null,
        },
        file: None,
        file_sha256: None,
        checkpoint_sha256: "c0ffee".to_owned(),
        preconditions,
        binding_digest: digest,
        scopes: vec!["once".to_owned()],
        expires_at: (chrono::Utc::now() + chrono::Duration::hours(1))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        approvers: vec![tgorka.to_string()],
        dispatch_chain: vec![tgorka.to_string()],
    }
}

#[ignore = "live: Synapse on delectra"]
#[tokio::test(flavor = "multi_thread")]
async fn a_card_is_decided_through_the_devices_own_path_and_closes_when_the_agent_acts() {
    let smoke = Smoke::from_env();
    let root = tempfile::tempdir().expect("tempdir");
    let nixi_user = smoke.user("nixi-smoke");
    let tgorka = smoke.user("tgorka-smoke");
    for user in [&nixi_user, &tgorka] {
        smoke.clear_devices(user).await;
        smoke.set_ratelimit(user, 0, 0).await;
    }
    let tgorka_password = smoke.secret("TGORKA_SMOKE_PASSWORD").to_owned();
    let nixi = smoke
        .client(
            &root.path().join("nixi"),
            "nixi-smoke",
            smoke.secret("NIXI_SMOKE_PASSWORD"),
        )
        .await;
    let a = smoke
        .client(&root.path().join("a"), "tgorka-smoke", &tgorka_password)
        .await;
    bootstrap(&a, &tgorka, &tgorka_password).await;
    let b = smoke
        .client(&root.path().join("b"), "tgorka-smoke", &tgorka_password)
        .await;
    for person in [&a, &b] {
        person
            .client()
            .event_cache()
            .subscribe()
            .expect("event cache");
    }

    let room: OwnedRoomId = nixi
        .create_room(
            RoomKind::Session(SessionKind::Conversation),
            "card smoke",
            vec![tgorka.clone()],
            &[],
        )
        .await
        .expect("a session room");
    let seen = capture(&nixi);
    let _nixi_sync = syncing(&nixi);
    for person in [&a, &b] {
        person.join(&room).await.expect("join");
        person.sync_once().await.expect("sync");
    }
    let _a_sync = syncing(&a);
    let _b_sync = syncing(&b);

    // The agent's request, its digest and summary keeper's own over what it
    // carries, as a host's record makes them.
    let id = ulid::Ulid::new().to_string();
    let request = request_of(&id, &room, &tgorka, &write("notes/plan.md", "hello"));
    let digest = request.binding_digest.clone();
    // A person's encrypted look-alike first, then the agent's own.
    let look_alike = ulid::Ulid::new().to_string();
    let mut forged = serde_json::to_value(&request).expect("request");
    forged["id"] = json!(look_alike);
    b.send(&room, APPROVAL_REQUEST, forged, None)
        .await
        .expect("a person's look-alike");
    nixi.send(
        &room,
        APPROVAL_REQUEST,
        serde_json::to_value(&request).expect("request"),
        None,
    )
    .await
    .expect("the agent's request");

    // A opens the room: the card is pending and decidable.
    let open = timeline::open_timeline(a.client(), &room, "acct")
        .await
        .expect("timeline");
    let batches: Arc<Mutex<Vec<TimelineBatch>>> = Arc::default();
    let sink = Arc::clone(&batches);
    let hosted = Arc::new(HostedRooms::default());
    let producer = tokio::spawn(timeline::forward_timeline(
        open,
        room.clone(),
        Box::new(move |batch| {
            sink.lock().expect("lock").push(batch);
            true
        }),
        Arc::new(AgentKinds::default()),
        Arc::new(AgentIcons::default()),
        Arc::clone(&hosted),
    ));
    let pending = card_when(&batches, &id, "pending", |card| {
        card.state == ApprovalStateVm::Pending
    })
    .await;
    assert!(pending.can_decide, "{pending:?}");
    assert_eq!(pending.binding_digest, digest);
    let ids: Vec<String> = batches
        .lock()
        .expect("lock")
        .iter()
        .rev()
        .find_map(|batch| batch.approvals.clone())
        .into_iter()
        .flatten()
        .flat_map(|approval| approval.cards)
        .map(|card| card.id)
        .collect();
    assert_eq!(
        ids,
        std::slice::from_ref(&id),
        "the person's look-alike is no card"
    );

    let joined =
        |client: &AgentClient, room: &RoomId| client.client().get_room(room).expect("room");
    let (on_a, on_b) = (joined(&a, &room), joined(&b, &room));
    let decide = |digest: &str, scope: Scope, id: &str| ApprovalDecideReq {
        id: id.to_owned(),
        binding_digest: digest.to_owned(),
        decision: Decision::Approve,
        scope,
        note: None,
    };
    let nothing_shown = |_: &str| false;

    // B, nobody's signed device, is refused before anything is sent.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    while approvals_of(&on_b).await.record(&id).is_none() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "B never read the request"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert_eq!(
        refused(
            decide_approval(
                &on_b,
                &hosted,
                &nothing_shown,
                decide(&digest, Scope::Once, &id)
            )
            .await
        ),
        UNVERIFIED
    );
    // A is refused for anything but the card as shown.
    for (req, sentence) in [
        (decide("sha256:else", Scope::Once, &id), OTHER_ACTION),
        (decide(&digest, Scope::Session, &id), SCOPE_NOT_OFFERED),
        (decide(&digest, Scope::Once, "01JNOTHERE"), NOT_FOUND),
    ] {
        assert_eq!(
            refused(decide_approval(&on_a, &hosted, &nothing_shown, req).await),
            sentence
        );
    }

    // B's decision sent by another client: shown, and the card stays
    // decidable; the agent's trust adapter does not count it.
    b.send(
        &room,
        APPROVAL_DECISION,
        json!({"id": id, "binding_digest": digest, "decision": "deny", "scope": "once"}),
        None,
    )
    .await
    .expect("B's decision");
    let shown = card_when(&batches, &id, "B's decision shown", |card| {
        matches!(
            card.state,
            ApprovalStateVm::Decided {
                decision: Decision::Deny,
                ..
            }
        )
    })
    .await;
    assert!(
        shown.can_decide,
        "a decision in the room closes nothing: {shown:?}"
    );

    let pinned = nixi
        .published_master_key(&tgorka)
        .await
        .expect("the homeserver answers")
        .expect("tgorka publishes an identity");
    let host = ClientDecisions {
        client: nixi.clone(),
        anchor: Anchor::Pinned(vec![TrustEntry {
            user: tgorka.clone(),
            master_key: Some(pinned),
            proxy: None,
        }]),
    };
    let seat = || Seat {
        sender_is_agent: false,
        sender_in_label: true,
        sender_in_approvers: true,
        requester: Some(tgorka.to_string()),
    };
    let from_b = decisions_when(&seen, 1).await.remove(0);
    assert_eq!(
        from_b.device.as_ref().map(ToString::to_string),
        b.device_id()
    );
    assert_eq!(
        judge(&host, &from_b, seat(), 2).await.err().as_deref(),
        Some(UNSIGNED_DEVICE)
    );

    // A decides through the production path; the agent reads it sealed,
    // for the request's digest, and it counts.
    decide_approval(
        &on_a,
        &hosted,
        &nothing_shown,
        decide(&digest, Scope::Once, &id),
    )
    .await
    .expect("A's decision is sent");
    let arrived = decisions_when(&seen, 2).await;
    assert_eq!(arrived.len(), 2, "no refused decision was sent");
    let from_a = &arrived[1];
    assert_eq!(from_a.content["binding_digest"], digest.as_str());
    assert_eq!(from_a.content["decision"], "approve");
    let decided = judge(&host, from_a, seat(), 2)
        .await
        .expect("A's decision counts");
    assert!(decided.verified);
    assert_eq!(Some(decided.device), a.device_id());

    // The agent uses it: the card closes on A, live.
    nixi.send_state(
        &room,
        APPROVAL_CONSUMED,
        &id,
        &json!({"v": CONTENT_VERSION, "id": id, "epoch": 1, "host": "live"}),
    )
    .await
    .expect("consumed");
    let consumed = card_when(&batches, &id, "consumed", |card| {
        card.state == ApprovalStateVm::Consumed
    })
    .await;
    assert!(!consumed.can_decide);
    // The command reads the same cache the card is drawn from.
    assert_eq!(
        refused(
            decide_approval(
                &on_a,
                &hosted,
                &nothing_shown,
                decide(&digest, Scope::Once, &id)
            )
            .await
        ),
        NOT_WAITING
    );

    // A large action travels as an encrypted file (R86): approving it waits
    // until this app has fetched it and found it bound to the digest; a
    // file other than the request names, or one the server cannot give,
    // is refused.
    let big = write("notes/big.md", &"x".repeat(32));
    let bytes = canonical(&big).expect("canonical").into_bytes();
    let file = nixi.upload_encrypted(&bytes).await.expect("an upload");
    let attached = |id: &str, file: &Value, sha: String| {
        let mut request = request_of(id, &room, &tgorka, &big);
        request.action.args = Value::Null;
        request.file = Some(file.clone());
        request.file_sha256 = Some(sha);
        request
    };
    let mut gone = file.clone();
    gone["url"] = json!(format!("mxc://{}/nothing-here", smoke.server_name));
    let (good, renamed, missing) = (
        ulid::Ulid::new().to_string(),
        ulid::Ulid::new().to_string(),
        ulid::Ulid::new().to_string(),
    );
    let large = attached(&good, &file, sha256_hex(&bytes));
    for request in [
        large.clone(),
        attached(&renamed, &file, sha256_hex(b"other bytes")),
        attached(&missing, &gone, sha256_hex(&bytes)),
    ] {
        nixi.send(
            &room,
            APPROVAL_REQUEST,
            serde_json::to_value(&request).expect("request"),
            None,
        )
        .await
        .expect("an attached request");
    }
    let card = card_when(&batches, &missing, "the attached cards", |card| {
        card.state == ApprovalStateVm::Pending
    })
    .await;
    assert!(card.payload.is_none() && card.attachment.is_some() && card.can_decide);
    let approve_large = || decide(&large.binding_digest, Scope::Once, &good);
    assert_eq!(
        refused(decide_approval(&on_a, &hosted, &nothing_shown, approve_large()).await),
        UNSEEN
    );
    assert_eq!(
        refused(approval_payload(&on_a, &renamed).await),
        PAYLOAD_REFUSED
    );
    assert_eq!(
        refused(approval_payload(&on_a, &missing).await),
        PAYLOAD_UNAVAILABLE
    );
    assert_eq!(refused(approval_payload(&on_a, &id).await), NOT_ATTACHED);
    let (shown, bound) = approval_payload(&on_a, &good)
        .await
        .expect("the attached action");
    assert_eq!(bound, large.binding_digest);
    assert_eq!(
        serde_json::from_str::<Value>(&shown).expect("json"),
        big,
        "the action as its digest binds it"
    );
    decide_approval(&on_a, &hosted, &|digest| digest == bound, approve_large())
        .await
        .expect("an attached action approved once shown");
    producer.abort();
}
