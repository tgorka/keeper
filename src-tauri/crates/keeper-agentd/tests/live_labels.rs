//! An invite the label refuses never reaches the homeserver (story 92.6,
//! acceptance 7): in a delegated session room labelled {tgorka}, inviting a
//! person outside the label is refused by `AgentClient::invite` before any
//! request, and the room's members are as they were; the same invite under
//! a label that reaches them goes through, so the server is shown to take
//! invites from this agent at all.
//!
//! `#[ignore]`; endpoints and secrets come from the environment:
//!
//! ```sh
//! KEEPER_AGENTS_SMOKE_HOMESERVER=http://100.101.101.23:8008 \
//! KEEPER_AGENTS_SMOKE_SECRETS=$HOME/.config/keeper-smoke/synapse.env \
//! cargo test --manifest-path src-tauri/Cargo.toml -p keeper-agentd --test live_labels -- --ignored --nocapture
//! ```

// matrix-sdk's sync future is deep enough to need it, as in the library.
#![recursion_limit = "256"]
#![cfg(target_os = "linux")]

use std::collections::BTreeSet;

use keeper_core::agents::label::{Integrity, Label, Readers};
use keeper_core::agents::matrix::{AgentClient, AgentMatrixError, RoomKind};
use keeper_core::agents::session::SessionKind;
use matrix_sdk::ruma::{OwnedUserId, RoomId};
use matrix_sdk::RoomMemberships;

mod common;

use common::Smoke;

/// Who is in `room` or invited to it, as the server says after a sync.
async fn members(client: &AgentClient, room: &RoomId) -> BTreeSet<OwnedUserId> {
    client.sync_once().await.expect("sync");
    let room = client.client().get_room(room).expect("the room");
    room.sync_members().await.expect("members");
    room.members(RoomMemberships::JOIN | RoomMemberships::INVITE)
        .await
        .expect("members")
        .iter()
        .map(|member| member.user_id().to_owned())
        .collect()
}

#[ignore = "live: Synapse on delectra"]
#[tokio::test(flavor = "multi_thread")]
async fn a_refused_invite_leaves_the_room_as_it_was() {
    let smoke = Smoke::from_env();
    let root = tempfile::tempdir().expect("tempdir");
    let nixi = smoke.user("nixi-smoke");
    let tgorka = smoke.user("tgorka-smoke");
    // A person outside {tgorka}: Marta's part.
    let marta = smoke.user("tola-smoke");
    smoke.clear_devices(&nixi).await;
    smoke.set_ratelimit(&nixi, 0, 0).await;
    let client = smoke
        .client(
            &root.path().join("nixi"),
            "nixi-smoke",
            smoke.secret("NIXI_SMOKE_PASSWORD"),
        )
        .await;
    let room = client
        .create_room(
            RoomKind::Session(SessionKind::Delegated),
            "labels smoke",
            vec![tgorka.clone()],
            &[],
        )
        .await
        .expect("a session room");
    let before = members(&client, &room).await;
    assert_eq!(before, BTreeSet::from([nixi.clone(), tgorka.clone()]));

    let tgorkas = Label {
        readers: Readers::Only(BTreeSet::from([tgorka.clone()])),
        integrity: Integrity::Owner,
        local_only: false,
    };
    let refused = client.invite(&room, &marta, &tgorkas, None).await;
    let Err(AgentMatrixError::Label(sentence)) = refused else {
        panic!("refused by the label: {refused:?}")
    };
    assert!(sentence.contains(marta.as_str()), "{sentence}");
    // As a known agent whose audience is wider than {tgorka}, the same.
    assert!(matches!(
        client
            .invite(
                &room,
                &marta,
                &tgorkas,
                Some(Readers::Only(BTreeSet::from([
                    tgorka.clone(),
                    marta.clone()
                ]))),
            )
            .await,
        Err(AgentMatrixError::Label(_))
    ));
    assert_eq!(members(&client, &room).await, before);

    let shared = Label {
        readers: Readers::Only(BTreeSet::from([tgorka.clone(), marta.clone()])),
        ..tgorkas
    };
    client
        .invite(&room, &marta, &shared, None)
        .await
        .expect("an invite the label reaches");
    let after = members(&client, &room).await;
    assert_eq!(
        after,
        BTreeSet::from([nixi.clone(), tgorka.clone(), marta.clone()])
    );
}
