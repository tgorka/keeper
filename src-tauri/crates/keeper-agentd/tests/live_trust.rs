//! Who may decide an approval, on a real homeserver (story 93.3,
//! acceptance 3, 7 and 9; rulings R30, R87, R88).
//!
//! The person (`tgorka-smoke`) has three devices made with matrix-sdk:
//! device A cross-signed with `bootstrap_cross_signing`, device B a fresh
//! login nobody signed, and device C — "the phone" — signed by A with the
//! identity's self-signing key. Marta (`marta-smoke`, made by the admin API
//! with a password of this run) is in the room and reads nothing. Each
//! sends one `dev.keeper.agent.approval.decision` into a session room of
//! the agent (`nixi-smoke`). The agent's client decrypts them; keeper's own
//! `arrival_of` reads each as an arrival with its device and seal, keeper's
//! `classify` drops Marta's, and the trust adapter
//! (`keeper_agent::deciding::ClientDecisions`, over the agent's client)
//! with `judge` decides the rest: A's and C's count, B's does not, an
//! in-process device at T4 does not, nobody counts unpinned, and after A
//! resets the identity the old pin no longer matches.
//!
//! What runs after a decision counts — `decision.json`, the consume, the
//! resume — is the session worker's, proved over the same source by
//! `agent_turns::parks` and through a whole agentd host by `live_delegate`;
//! no running host decides here.
//!
//! `#[ignore]`; endpoints and secrets come from the environment:
//!
//! ```sh
//! KEEPER_AGENTS_SMOKE_HOMESERVER=http://100.101.101.23:8008 \
//! KEEPER_AGENTS_SMOKE_SECRETS=$HOME/.config/keeper-smoke/synapse.env \
//! cargo test --manifest-path src-tauri/Cargo.toml -p keeper-agentd --test live_trust -- --ignored --nocapture
//! ```

// matrix-sdk's sync future is deep enough to need it, as in the library.
#![recursion_limit = "256"]
#![cfg(target_os = "linux")]

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use keeper_agent::agent::Arrived;
use keeper_agent::deciding::{judge, ClientDecisions, Seat};
use keeper_agent::rooms::{classify, Disposition, Served, UNTRUSTED_DECISION};
use keeper_agent::runtime::arrival_of;
use keeper_core::agents::agentd::TrustEntry;
use keeper_core::agents::events::APPROVAL_DECISION;
use keeper_core::agents::home::AgentKind;
use keeper_core::agents::label::Readers;
use keeper_core::agents::matrix::{AgentClient, RoomKind};
use keeper_core::agents::session::SessionKind;
use keeper_core::agents::trust::{
    Anchor, OwnAccount, PinState, KEY_MOVED, NOT_PINNED, THIS_DEVICE, UNSIGNED_DEVICE,
};
use matrix_sdk::deserialized_responses::EncryptionInfo;
use matrix_sdk::ruma::events::AnySyncTimelineEvent;
use matrix_sdk::ruma::serde::Raw;
use matrix_sdk::ruma::{OwnedDeviceId, OwnedUserId};
use serde_json::{json, Value};

mod common;

use common::{bootstrap, Smoke};

/// The person resets their cross-signing identity from `client`: a new
/// master key is published.
async fn reset(client: &AgentClient, user: &OwnedUserId, password: &str) {
    use matrix_sdk::encryption::CrossSigningResetAuthType;
    use matrix_sdk::ruma::api::client::uiaa;
    let handle = client
        .client()
        .encryption()
        .reset_cross_signing()
        .await
        .expect("a reset");
    let Some(handle) = handle else {
        return;
    };
    let CrossSigningResetAuthType::Uiaa(challenge) = handle.auth_type() else {
        panic!("the test homeserver asks for a password");
    };
    let mut auth = uiaa::Password::new(
        uiaa::UserIdentifier::Matrix(uiaa::MatrixUserIdentifier::new(user.to_string())),
        password.to_owned(),
    );
    auth.session = challenge.session.clone();
    handle
        .auth(Some(uiaa::AuthData::Password(auth)))
        .await
        .expect("the reset with the password");
}

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

fn device_of(client: &AgentClient) -> OwnedDeviceId {
    client.device_id().expect("a device").into()
}

#[ignore = "live: Synapse on delectra"]
#[tokio::test(flavor = "multi_thread")]
async fn a_decision_from_a_cross_signed_device_counts_and_one_from_a_new_device_does_not() {
    let smoke = Smoke::from_env();
    let root = tempfile::tempdir().expect("tempdir");
    let nixi_user = smoke.user("nixi-smoke");
    let tgorka = smoke.user("tgorka-smoke");
    let marta_user = smoke.user("marta-smoke");
    let marta_password = ulid::Ulid::new().to_string();
    smoke
        .admin(
            reqwest::Method::PUT,
            &format!("/_synapse/admin/v2/users/{marta_user}"),
            Some(json!({ "password": marta_password, "admin": false })),
        )
        .await;
    for user in [&nixi_user, &tgorka, &marta_user] {
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
    let c = smoke
        .client(&root.path().join("c"), "tgorka-smoke", &tgorka_password)
        .await;
    let marta = smoke
        .client(&root.path().join("marta"), "marta-smoke", &marta_password)
        .await;
    // A signs the phone with the identity's self-signing key.
    let encryption = a.client().encryption();
    encryption
        .request_user_identity(&tgorka)
        .await
        .expect("A reads its own identity");
    encryption
        .get_device(&tgorka, &device_of(&c))
        .await
        .expect("the store")
        .expect("A knows the phone")
        .verify()
        .await
        .expect("A signs the phone");

    let room = nixi
        .create_room(
            RoomKind::Session(SessionKind::Conversation),
            "trust smoke",
            vec![tgorka.clone(), marta_user.clone()],
            &[],
        )
        .await
        .expect("a session room");
    let seen = capture(&nixi);
    let _sync = common::syncing(&nixi);
    for person in [&a, &b, &c, &marta] {
        person.join(&room).await.expect("join");
        person.sync_once().await.expect("sync");
    }
    for person in [&a, &b, &c, &marta] {
        person.sync_once().await.expect("sync");
    }

    let id = ulid::Ulid::new().to_string();
    let content = json!({
        "id": id,
        "binding_digest": "0".repeat(64),
        "decision": "approve",
        "scope": "once",
    });
    for person in [&a, &b, &c, &marta] {
        person
            .send(&room, APPROVAL_DECISION, content.clone(), None)
            .await
            .expect("a decision");
    }

    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    let arrivals: Vec<Arrived> = loop {
        let arrivals: Vec<Arrived> = seen
            .lock()
            .expect("lock")
            .iter()
            .filter(|(value, _)| value["type"] == APPROVAL_DECISION)
            .filter_map(|(value, encryption)| {
                arrival_of(value, encryption.as_ref(), tokio::time::Instant::now())
            })
            .collect();
        if arrivals.len() == 4 {
            break arrivals;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the agent decrypted {} of 4 decisions",
            arrivals.len()
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    };
    let from = |client: &AgentClient| {
        let device = device_of(client);
        arrivals
            .iter()
            .find(|arrived| arrived.device.as_ref() == Some(&device))
            .cloned()
            .expect("its decision arrived, naming its device")
    };
    let (from_a, from_b, from_c, from_marta) = (from(&a), from(&b), from(&c), from(&marta));
    for arrived in [&from_a, &from_b, &from_c, &from_marta] {
        assert_eq!(
            arrived.arrival,
            keeper_agent::rooms::Arrival::Decision { sealed: true },
            "{}",
            arrived.sender
        );
    }

    // Marta reads nothing of a {tgorka} session: she decides nothing.
    let readers = Readers::Only(BTreeSet::from([tgorka.clone()]));
    let served = Served {
        agent_kind: AgentKind::Proxy,
        human: Some(&tgorka),
        session_kind: SessionKind::Conversation,
        agent_user: &nixi_user,
        requester: &tgorka,
        readers: &readers,
    };
    assert_eq!(
        classify(&served, &from_marta.sender, from_marta.arrival),
        Disposition::Ignored(UNTRUSTED_DECISION)
    );
    for arrived in [&from_a, &from_b, &from_c] {
        assert_eq!(
            classify(&served, &arrived.sender, arrived.arrival),
            Disposition::Decision
        );
    }

    let seat = || Seat {
        sender_is_agent: false,
        sender_in_label: true,
        sender_in_approvers: true,
        requester: Some(tgorka.to_string()),
    };
    let pinned = nixi
        .published_master_key(&tgorka)
        .await
        .expect("the homeserver answers")
        .expect("tgorka publishes an identity");
    let headless = |key: Option<String>| ClientDecisions {
        client: nixi.clone(),
        anchor: Anchor::Pinned(vec![TrustEntry {
            user: tgorka.clone(),
            master_key: key,
            proxy: None,
        }]),
    };
    let pinned_host = headless(Some(pinned.clone()));
    for (arrived, tier) in [(&from_a, 2), (&from_c, 3), (&from_c, 4)] {
        let decided = judge(&pinned_host, arrived, seat(), tier)
            .await
            .unwrap_or_else(|reason| panic!("{} at T{tier}: {reason}", arrived.sender));
        assert!(decided.verified);
        assert_eq!(
            Some(decided.device.as_str()),
            arrived.device.as_deref().map(|d| d.as_str())
        );
    }
    assert_eq!(
        judge(&pinned_host, &from_b, seat(), 2).await,
        Err(UNSIGNED_DEVICE.to_owned()),
        "the fresh login nobody signed"
    );
    assert_eq!(
        judge(&headless(None), &from_a, seat(), 2).await,
        Err(NOT_PINNED.to_owned()),
        "no trust on first use"
    );

    // The desktop's anchor (R87): A is this app's own device; at T4 it
    // cannot agree, the phone can.
    let desktop = ClientDecisions {
        client: nixi.clone(),
        anchor: Anchor::Desktop(vec![OwnAccount {
            user: tgorka.clone(),
            device_id: device_of(&a).to_string(),
            own_identity_verified: true,
            master_key: Some(pinned.clone()),
        }]),
    };
    assert_eq!(
        judge(&desktop, &from_a, seat(), 4).await,
        Err(THIS_DEVICE.to_owned())
    );
    assert!(judge(&desktop, &from_a, seat(), 3).await.is_ok());
    assert!(judge(&desktop, &from_c, seat(), 4).await.is_ok());

    // The pin as `status` reads it (R88), before and after a reset.
    assert_eq!(
        PinState::of(Some(&pinned), Some(&pinned)),
        PinState::Matches
    );
    reset(&a, &tgorka, &tgorka_password).await;
    let published = nixi
        .published_master_key(&tgorka)
        .await
        .expect("the homeserver answers")
        .expect("tgorka publishes an identity");
    assert_ne!(published, pinned, "a reset publishes a new master key");
    assert_eq!(
        PinState::of(Some(&published), Some(&pinned)),
        PinState::Differs
    );
    assert_eq!(PinState::of(Some(&published), None), PinState::NotPinned);
    assert_eq!(
        judge(&pinned_host, &from_a, seat(), 2).await,
        Err(KEY_MOVED.to_owned()),
        "a reset identity's own device does not count under the old pin"
    );
}
