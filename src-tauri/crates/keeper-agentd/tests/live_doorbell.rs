//! The doorbell against a real homeserver (story 92.4, acceptance 9;
//! rulings R59, R160, R161): in a control room a visiting agent — power 0 —
//! may ring a drive's doorbell, an unencrypted state event, and may write no
//! host manifest; a control room made before the doorbell row is brought up
//! to date by its creator. A host that first syncs the room hears the bell
//! from the sync through `doorbell::listen` and asks one pull, and the
//! room's real membership — the visitor at power 0 included — passes the
//! audience check only with the visitor known as an agent. The routing is
//! `hosts::tests::a_shared_drive_rings_every_control_room_that_lists_it`.
//!
//! `#[ignore]`; endpoints and secrets come from the environment:
//!
//! ```sh
//! KEEPER_AGENTS_SMOKE_HOMESERVER=http://100.101.101.23:8008 \
//! KEEPER_AGENTS_SMOKE_SECRETS=$HOME/.config/keeper-smoke/synapse.env \
//! cargo test --manifest-path src-tauri/Cargo.toml -p keeper-agentd --test live_doorbell -- --ignored --nocapture
//! ```

// matrix-sdk's sync future is deep enough to need it, as in the library.
#![recursion_limit = "256"]
#![cfg(target_os = "linux")]

use std::sync::{Arc, Mutex};

use keeper_agent::doorbell::{room_sink, Answer, Doorbell, DriveEngine};
use keeper_agent::rooms::KnownAgent;
use keeper_core::agents::events::{
    control_levels, ControlLevels, DoorbellContent, DoorbellReason, CONTENT_VERSION, DOORBELL, HOST,
};
use keeper_core::agents::home::AgentKind;
use keeper_core::agents::label::{check_sink, Label, Readers, SinkVerdict};
use keeper_core::agents::matrix::{AgentClient, RoomKind};
use keeper_sync::engine::{PullNow, Pushed};
use keeper_sync::SyncError;
use matrix_sdk::ruma::OwnedUserId;
use serde_json::json;

mod common;

use common::Smoke;

/// An engine that holds no commit and records each pull asked of it.
#[derive(Default)]
struct Recording(Mutex<Vec<(String, String)>>);

impl DriveEngine for Recording {
    fn push_tap(&self) -> tokio::sync::broadcast::Receiver<Pushed> {
        tokio::sync::broadcast::channel(1).1
    }

    fn has_commit(&self, _profile_id: &str, _commit: &str) -> Result<bool, SyncError> {
        Ok(false)
    }

    fn pull_now(&self, profile_id: &str, commit: &str) -> Result<PullNow, SyncError> {
        self.0
            .lock()
            .expect("lock")
            .push((profile_id.to_owned(), commit.to_owned()));
        Ok(PullNow::Queued)
    }

    fn changed_paths(
        &self,
        _profile_id: &str,
        _from: Option<&str>,
        _to: &str,
    ) -> Result<Vec<String>, SyncError> {
        Ok(Vec::new())
    }
}

/// An agent homed in `drive`, read by `audience`.
fn agent(user: &OwnedUserId, drive: &str, audience: &OwnedUserId) -> KnownAgent {
    KnownAgent {
        id: user.localpart().to_owned(),
        drive: drive.to_owned(),
        name: user.localpart().to_owned(),
        matrix_user: user.clone(),
        kind: AgentKind::Steward,
        human: None,
        hosted: false,
        home_readers: Readers::Only([audience.clone()].into()),
        opening: Label::top(),
        drives: vec![drive.to_owned()],
    }
}

#[ignore = "live: Synapse on delectra"]
#[tokio::test(flavor = "multi_thread")]
async fn a_shared_drive_rings_every_control_room_that_lists_it() {
    let smoke = Smoke::from_env();
    let root = tempfile::tempdir().expect("tempdir");
    let creator = smoke.user("nixi-smoke");
    let visitor = smoke.user("nixi-paced");
    let person = smoke.user("tgorka-smoke");
    for user in [&creator, &visitor] {
        smoke.clear_devices(user).await;
        smoke.set_ratelimit(user, 0, 0).await;
    }
    let maker = smoke
        .client(
            &root.path().join("maker"),
            "nixi-smoke",
            smoke.secret("NIXI_SMOKE_PASSWORD"),
        )
        .await;
    // The person brings the visiting steward into their control room.
    let room = maker
        .create_room(
            RoomKind::Control,
            "tgorka's agents",
            vec![person.clone(), visitor.clone()],
            &[],
        )
        .await
        .expect("control room");
    let guest = smoke
        .client(
            &root.path().join("visitor"),
            "nixi-paced",
            smoke.secret("NIXI_PACED_PASSWORD"),
        )
        .await;
    guest.join(&room).await.expect("the visitor joins");

    let bell = |commit: char| {
        serde_json::to_value(DoorbellContent {
            v: CONTENT_VERSION,
            drive: "neuradrive".to_owned(),
            commit: commit.to_string().repeat(40),
            reason: DoorbellReason::Memory,
        })
        .expect("doorbell")
    };
    let manifest = json!({"v": 1, "host": "agentd-neuraffica"});

    // A room made with today's levels: the doorbell is accepted at level 0,
    // a host manifest is refused.
    guest
        .send_state(&room, DOORBELL, "neuradrive", &bell('a'))
        .await
        .expect("a level-0 visitor rings the doorbell");
    let heard = maker
        .server_state(&room, DOORBELL, "neuradrive")
        .await
        .expect("read")
        .expect("the doorbell is in the room's state");
    assert_eq!(heard.content, bell('a'));
    assert_eq!(heard.sender, visitor);
    assert!(
        guest
            .send_state(&room, HOST, "agentd-neuraffica", &manifest)
            .await
            .is_err(),
        "a visitor writes no host manifest"
    );

    // A room made before the doorbell row: refused until its creator's host
    // brings the levels up to date (R37's updater, R59's row).
    let mut levels = maker
        .server_state(&room, "m.room.power_levels", "")
        .await
        .expect("read")
        .expect("levels")
        .content;
    levels["events"]
        .as_object_mut()
        .expect("events")
        .remove(DOORBELL);
    maker
        .send_state(&room, "m.room.power_levels", "", &levels)
        .await
        .expect("an old room's levels");
    assert!(
        guest
            .send_state(&room, DOORBELL, "neuradrive", &bell('b'))
            .await
            .is_err(),
        "without the row a visitor cannot ring"
    );
    assert_eq!(control_levels(&levels, &visitor), ControlLevels::NoPower);
    let ControlLevels::Update(updated) = control_levels(&levels, &creator) else {
        panic!("the creator may bring the room up to date");
    };
    maker
        .send_state(&room, "m.room.power_levels", "", &updated)
        .await
        .expect("the update");
    guest
        .send_state(&room, DOORBELL, "neuradrive", &bell('c'))
        .await
        .expect("after the update the visitor rings");
    let heard = maker
        .server_state(&room, DOORBELL, "neuradrive")
        .await
        .expect("read")
        .expect("rung");
    assert_eq!(
        heard.content,
        bell('c'),
        "the last doorbell is the room's state"
    );

    // A host that syncs the room for the first time hears the bell the sync
    // carries, through the state handler, and asks one pull.
    let engine = Arc::new(Recording::default());
    let doorbell = Arc::new(Doorbell::default());
    doorbell.set_engine(Arc::clone(&engine) as Arc<dyn DriveEngine>);
    let readers = [
        agent(&visitor, "neuradrive", &person),
        agent(&creator, "tgdrive", &person),
    ];
    doorbell.set_drives(
        [("neuradrive".to_owned(), "neuradrive".to_owned())],
        readers.to_vec(),
    );
    let listener = AgentClient::open(
        &smoke.homeserver,
        &root.path().join("listener"),
        "live-passphrase",
    )
    .await
    .expect("client");
    keeper_agent::doorbell::listen(listener.client(), Arc::clone(&doorbell));
    listener
        .login(
            "nixi-smoke",
            smoke.secret("NIXI_SMOKE_PASSWORD"),
            None,
            "live listener",
        )
        .await
        .expect("login");
    listener.sync_once().await.expect("first sync");
    assert_eq!(
        doorbell.deliver(std::time::Instant::now(), 4),
        vec![("neuradrive".to_owned(), Answer::Pulled)],
        "the bell came with the first sync"
    );
    assert_eq!(
        *engine.0.lock().expect("lock"),
        vec![("neuradrive".to_owned(), "c".repeat(40))]
    );

    // The room's members as the server lists them — the visitor at power 0
    // among them — reach only the drive's reader when the visitor is known
    // as an agent of the drive; unknown, it counts as a person and blocks.
    let synced = listener
        .client()
        .get_room(&room)
        .expect("the listener is in the room");
    let members = keeper_core::agents::room::room_members(&synced)
        .await
        .expect("members");
    assert!(members.contains(&visitor), "{members:?}");
    let label = Label {
        readers: Readers::Only([person.clone()].into()),
        ..Label::top()
    };
    assert_eq!(
        check_sink(&label, &room_sink(members.clone(), &readers)),
        SinkVerdict::Allow
    );
    assert!(matches!(
        check_sink(&label, &room_sink(members, &readers[1..])),
        SinkVerdict::Block { .. }
    ));
}
