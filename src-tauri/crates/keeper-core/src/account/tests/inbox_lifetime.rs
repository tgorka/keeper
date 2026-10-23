//! The merged inbox's producers against a sign-out, a shutdown and a newer
//! subscription (R516, R519; reviews R102L6-01, R102L7-01, R102L7-02): a
//! real `AccountManager` whose accounts sync from a loopback homeserver, held
//! at the lifetime's test pauses to begin a teardown in between.
//!
//! Moved here from the Agents list's tests when the agents' Matrix path left
//! keeper (108.2): the list worker, its kept rows and the seat finder went;
//! the inbox producers these races are about stayed.

use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use matrix_sdk::config::SyncSettings;
use matrix_sdk::ruma::OwnedRoomId;
use matrix_sdk::Client;
use serde_json::{json, Value};

use crate::account::lifetime::Pause;
use crate::account::AccountManager;
use crate::forges::testing::{self, FakePlatform, Reply, Seen as Request};
use crate::vm::{InboxBatch, InboxOp, NetworksSnapshot, SpacesSnapshot};

const ME: &str = "@tgorka:h";
/// The account the tests sign in first, and a second one added after it.
const LIVE: &str = "acct-live";
const SECOND: &str = "acct-second";
/// How many rooms the first sync brings.
const ROOMS: usize = 30;

type Syncs = Arc<Mutex<std::collections::VecDeque<Value>>>;
type Batches = Arc<Mutex<Vec<InboxBatch>>>;

fn state(kind: &str, key: &str, sender: &str, content: Value, n: u64) -> Value {
    json!({
        "type": kind, "state_key": key, "sender": sender, "content": content,
        "event_id": format!("$s{n}:h"), "origin_server_ts": 1_000 + n,
    })
}

/// The first sync: `ROOMS` group chats of the person's.
fn first_sync() -> Value {
    let mut join = serde_json::Map::new();
    for n in 0..ROOMS {
        let base = 10 * n as u64;
        join.insert(
            format!("!r{n:02}:h"),
            json!({
                "state": { "events": [
                    state("m.room.create", "", ME, json!({ "creator": ME, "room_version": "11" }), base),
                    state("m.room.name", "", ME, json!({ "name": format!("Room {n}") }), base + 1),
                    state("m.room.member", ME, ME, json!({ "membership": "join" }), base + 2),
                ]},
                "timeline": { "events": [], "limited": false },
            }),
        );
    }
    json!({ "next_batch": "s1", "rooms": { "join": join } })
}

/// A homeserver that answers the queued `/sync`s in order, then nothing new,
/// and refuses the sliding sync an account's sync service asks for: these
/// tests sync by hand.
fn homeserver() -> (testing::Fake, Syncs) {
    let syncs: Syncs = Arc::new(Mutex::new(std::collections::VecDeque::from([first_sync()])));
    let queued = Arc::clone(&syncs);
    let fake = testing::serve(move |seen: &Request| {
        let path = seen.path.as_str();
        if path.contains("msc3575") {
            return Reply::json(404, r#"{"errcode":"M_UNRECOGNIZED","error":"no"}"#);
        }
        if path.ends_with("/sync") {
            let next = queued.lock().expect("syncs").pop_front();
            return Reply::json(
                200,
                &next
                    .unwrap_or_else(|| json!({"next_batch": "idle"}))
                    .to_string(),
            );
        }
        if path.ends_with("/versions") {
            return Reply::json(200, r#"{"versions":["v1.1","v1.11","v1.12"]}"#);
        }
        if path.ends_with("/keys/upload") {
            return Reply::json(200, r#"{"one_time_key_counts":{"signed_curve25519":50}}"#);
        }
        if path.ends_with("/keys/query") {
            return Reply::json(200, r#"{"device_keys":{},"failures":{}}"#);
        }
        Reply::json(404, r#"{"errcode":"M_NOT_FOUND","error":"no"}"#)
    });
    (fake, syncs)
}

/// The person's session as keeper keeps it in the keychain.
fn session_blob() -> String {
    format!(r#"{{"kind":"Password","user_id":"{ME}","device_id":"MAC","access_token":"t"}}"#)
}

/// Wait up to `bound` for `ready` to resolve true.
async fn eventually<F, Ready>(bound: Duration, mut ready: F) -> bool
where
    F: FnMut() -> Ready,
    Ready: Future<Output = bool>,
{
    let start = Instant::now();
    while start.elapsed() < bound {
        if ready().await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    ready().await
}

/// An account manager on a scratch platform, as the shell holds one.
struct Signed {
    platform: Arc<FakePlatform>,
    manager: AccountManager,
}

impl Signed {
    fn new() -> Signed {
        let platform = Arc::new(FakePlatform::default());
        let manager = AccountManager::new(platform.as_ref(), &platform.data_dir);
        Signed { platform, manager }
    }

    fn platform(&self) -> Arc<dyn crate::platform::Platform> {
        self.platform.clone()
    }

    /// Sign `account` in as the person against `homeserver`, added at
    /// `created`: accounts are started in the order they were added.
    fn sign_in_at(&self, account: &str, homeserver: &str, created: i64) {
        use crate::platform::Platform;
        crate::registry::insert_account(
            &self.platform.data_dir,
            account,
            ME,
            homeserver,
            "MAC",
            created,
            0,
            "password",
        )
        .expect("registry");
        self.platform
            .keychain_set(&crate::auth::session_keychain_key(account), &session_blob())
            .expect("keychain");
    }

    /// Inbox producer tasks of `account` running now, whichever
    /// subscription started them.
    fn running(&self, account: &str) -> usize {
        self.manager.inbox_owners.running(account)
    }

    /// How many rooms the palette finds now.
    async fn palette_rooms(&self) -> usize {
        self.manager.palette.lock().await.len()
    }

    /// The inbox subscription that owns the merged inbox now.
    async fn inbox_owner(&self) -> Option<u64> {
        self.manager
            .inbox
            .lock()
            .await
            .as_ref()
            .map(|handle| handle.subscription_id)
    }

    /// What one `/sync` of `homeserver` brings, already in `account`'s
    /// store, as a launch finds it.
    async fn warm(&self, account: &str, homeserver: &str) {
        let sdk = self
            .platform
            .data_dir
            .join("accounts")
            .join(account)
            .join("sdk");
        let client = Client::builder()
            .homeserver_url(homeserver)
            .sqlite_store(&sdk, None)
            .build()
            .await
            .expect("client");
        let session: matrix_sdk::authentication::matrix::MatrixSession = serde_json::from_value(
            json!({ "user_id": ME, "device_id": "MAC", "access_token": "t" }),
        )
        .expect("session");
        client.restore_session(session).await.expect("restore");
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .expect("sync");
    }

    /// Subscribe the merged inbox, as the Chats view does, its Inbox window
    /// captured into `inbox`.
    async fn subscribe(&self, inbox: &Batches) -> u64 {
        let into = Arc::clone(inbox);
        self.manager
            .subscribe_inbox(
                &self.platform(),
                Box::new(move |batch: InboxBatch| {
                    into.lock().expect("inbox").push(batch);
                    true
                }),
                Box::new(|_: InboxBatch| true),
                Box::new(|_: InboxBatch| true),
                Box::new(|_: InboxBatch| true),
                Box::new(|_: SpacesSnapshot| true),
                Box::new(|_: NetworksSnapshot| true),
            )
            .await
            .expect("subscribed")
    }

    /// `account`'s live client, its sync service stopped: these tests sync
    /// by hand.
    async fn live(&self, account: &str) -> Client {
        let (client, sync) = {
            let accounts = self.manager.accounts.lock().await;
            let handle = accounts.get(account).expect("live");
            (handle.client.clone(), Arc::clone(&handle.sync))
        };
        sync.stop().await;
        client
    }
}

/// Subscribe the merged inbox on its own task.
fn subscribing(signed: &Arc<Signed>, inbox: &Batches) -> tokio::task::JoinHandle<u64> {
    let (signed, inbox) = (Arc::clone(signed), Arc::clone(inbox));
    tokio::spawn(async move { signed.subscribe(&inbox).await })
}

/// Whether any Inbox batch drew a row of `account`.
fn drawn(inbox: &Batches, account: &str) -> bool {
    inbox.lock().expect("inbox").iter().any(|batch| {
        batch.ops.iter().any(|op| match op {
            InboxOp::Reset { rooms } | InboxOp::Append { rooms } => {
                rooms.iter().any(|row| row.account_id == account)
            }
            InboxOp::Insert { room, .. } | InboxOp::Set { room, .. } => room.account_id == account,
            _ => false,
        })
    })
}

/// What a subscription registered of `account` once published: whether any
/// producer of it is registered or runs.
fn producing(signed: &Signed, account: &str) -> bool {
    signed.manager.inbox_owners.registered(account) > 0 || signed.running(account) > 0
}

/// R516 (review R102L6-01): a subscription about to acquire an account's
/// room list while the account signs out: once it goes on, it registers no
/// producer and draws none of its rows.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_subscription_that_resumes_after_a_sign_out_registers_nothing_of_it() {
    let (fake, _) = homeserver();
    let signed = Arc::new(Signed::new());
    signed.sign_in_at(LIVE, &fake.base, 1);
    let paused = signed
        .manager
        .lifetimes
        .pause_at_for(Pause::Acquiring, LIVE);
    let inbox = Batches::default();
    let subscription = subscribing(&signed, &inbox);
    paused
        .reached
        .await
        .expect("the subscription is about to acquire the room list");

    signed
        .manager
        .sign_out(&signed.platform(), LIVE, None)
        .await
        .expect("signed out");
    let _ = paused.release.send(());
    subscription.await.expect("subscribed");

    assert!(
        !producing(&signed, LIVE),
        "a producer of the signed-out account"
    );
    assert!(!drawn(&inbox, LIVE), "a row of the signed-out account");
}

/// R516 (review R102L6-01): a subscription that built an account's
/// producers and has not published them when the account signs out stops
/// them as it publishes: none runs, none of the rows is drawn.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_subscription_that_publishes_after_a_sign_out_stops_what_it_built() {
    let (fake, _) = homeserver();
    let signed = Arc::new(Signed::new());
    signed.sign_in_at(LIVE, &fake.base, 1);
    let paused = signed.manager.lifetimes.pause_at(Pause::Publishing);
    let inbox = Batches::default();
    let subscription = subscribing(&signed, &inbox);
    paused
        .reached
        .await
        .expect("the subscription built its producers");

    signed
        .manager
        .sign_out(&signed.platform(), LIVE, None)
        .await
        .expect("signed out");
    let _ = paused.release.send(());
    subscription.await.expect("subscribed");

    assert!(
        !producing(&signed, LIVE),
        "a producer of the signed-out account"
    );
    assert_eq!(
        signed.palette_rooms().await,
        0,
        "its rooms stay in the palette"
    );
}

/// R519 (review R102L7-01): a subscription that started an account's
/// producers and waits on another account's room list when the first
/// account signs out. The sign-out stops and awaits every producer of it
/// before it returns — before the subscription goes on — and takes its
/// rooms out of the palette; once the subscription goes on, nothing of the
/// account runs or comes back.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_sign_out_while_the_inbox_waits_on_another_account_stops_every_producer_of_it() {
    let (fake, _) = homeserver();
    let signed = Arc::new(Signed::new());
    signed.sign_in_at(LIVE, &fake.base, 1);
    signed.sign_in_at(SECOND, &fake.base, 2);
    signed.warm(LIVE, &fake.base).await;
    let waiting = signed
        .manager
        .lifetimes
        .pause_at_for(Pause::Acquiring, SECOND);
    let inbox = Batches::default();
    let subscription = subscribing(&signed, &inbox);
    waiting
        .reached
        .await
        .expect("the subscription waits on the second account");
    assert!(
        eventually(Duration::from_secs(5), || async {
            signed.running(LIVE) == 3 && signed.palette_rooms().await == ROOMS
        })
        .await,
        "the first account's three producers run, its rooms in the palette: {} running",
        signed.running(LIVE)
    );

    signed
        .manager
        .sign_out(&signed.platform(), LIVE, None)
        .await
        .expect("signed out");
    assert_eq!(
        (signed.running(LIVE), signed.palette_rooms().await),
        (0, 0),
        "(its producers running, its rooms in the palette) once the sign-out returned"
    );
    assert!(!producing(&signed, LIVE));

    let _ = waiting.release.send(());
    subscription.await.expect("subscribed");
    assert!(
        eventually(Duration::from_secs(5), || async {
            signed.running(SECOND) == 3
        })
        .await,
        "the subscription went on with the other account"
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !producing(&signed, LIVE),
        "a producer of the signed-out account"
    );
    assert_eq!(
        signed.palette_rooms().await,
        0,
        "the palette finds a room of the signed-out account"
    );
}

/// R519 (review R102L7-01): two inbox subscriptions overlap and the older
/// one finishes last. The newer one stopped and awaited the older one's
/// producers when it began, and stays the owner; a teardown then stops
/// every producer there is.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_older_inbox_subscription_finishing_last_leaves_the_newer_one_in_charge() {
    let (fake, _) = homeserver();
    let signed = Arc::new(Signed::new());
    signed.sign_in_at(LIVE, &fake.base, 1);
    signed.sign_in_at(SECOND, &fake.base, 2);
    signed.warm(LIVE, &fake.base).await;
    let waiting = signed
        .manager
        .lifetimes
        .pause_at_for(Pause::Acquiring, SECOND);
    let (older_inbox, newer_inbox) = (Batches::default(), Batches::default());
    let older = subscribing(&signed, &older_inbox);
    waiting
        .reached
        .await
        .expect("the older subscription waits on the second account");
    assert!(
        eventually(Duration::from_secs(5), || async {
            signed.running(LIVE) == 3
        })
        .await,
        "the older subscription's producers of the first account run"
    );

    let newer = signed.subscribe(&newer_inbox).await;
    assert!(
        eventually(Duration::from_secs(5), || async {
            signed.running(LIVE) == 3
        })
        .await,
        "the older subscription's producers were stopped: only the newer one's run, not {}",
        signed.running(LIVE)
    );
    let _ = waiting.release.send(());
    let older = older.await.expect("the older subscription finished");

    assert_ne!(older, newer);
    assert_eq!(
        signed.inbox_owner().await,
        Some(newer),
        "the newer subscription owns the inbox"
    );
    assert!(
        eventually(Duration::from_secs(5), || async {
            (signed.running(LIVE), signed.running(SECOND)) == (3, 3)
        })
        .await,
        "only the newer subscription's producers run: ({}, {})",
        signed.running(LIVE),
        signed.running(SECOND)
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        (signed.running(LIVE), signed.running(SECOND)),
        (3, 3),
        "the older subscription started nothing more"
    );

    signed.manager.shutdown(LIVE).await;
    signed.manager.shutdown(SECOND).await;
    assert_eq!(
        (signed.running(LIVE), signed.running(SECOND)),
        (0, 0),
        "a teardown stops every producer of its account"
    );
}

/// R519 (review R102L7-02): an old subscription held while its account
/// shut down, came back, and was subscribed afresh. When the old one
/// finishes, it cleans up only its own: the fresh subscription keeps the
/// inbox, its producers and the palette.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_old_subscription_finishing_after_a_new_activation_leaves_the_new_one_intact() {
    let (fake, _) = homeserver();
    let signed = Arc::new(Signed::new());
    signed.sign_in_at(LIVE, &fake.base, 1);
    signed.warm(LIVE, &fake.base).await;
    let paused = signed.manager.lifetimes.pause_at(Pause::Publishing);
    let (old_inbox, fresh_inbox) = (Batches::default(), Batches::default());
    let old = subscribing(&signed, &old_inbox);
    paused
        .reached
        .await
        .expect("the old subscription built its producers");

    signed.manager.shutdown(LIVE).await;
    let fresh = signed.subscribe(&fresh_inbox).await;
    assert!(
        eventually(Duration::from_secs(5), || async {
            signed.running(LIVE) == 3 && signed.palette_rooms().await == ROOMS
        })
        .await,
        "the fresh subscription's producers run, its rooms in the palette"
    );

    let _ = paused.release.send(());
    old.await.expect("the old subscription finished");
    assert_eq!(signed.inbox_owner().await, Some(fresh));
    assert_eq!(
        signed.palette_rooms().await,
        ROOMS,
        "the fresh subscription's rooms left the palette"
    );
    assert_eq!(signed.running(LIVE), 3, "the fresh producers run");
}

/// R519 (review R102L7-01): between the moment an account's teardown
/// begins and the moment its producers are stopped, a producer that hears
/// of a change has no effect and stops: the palette does not learn a room
/// of an account that is going.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_producer_has_no_effect_once_its_account_began_closing() {
    let (fake, syncs) = homeserver();
    let signed = Arc::new(Signed::new());
    signed.sign_in_at(LIVE, &fake.base, 1);
    signed.warm(LIVE, &fake.base).await;
    let inbox = Batches::default();
    let _chats = signed.subscribe(&inbox).await;
    assert!(
        eventually(Duration::from_secs(5), || async {
            signed.running(LIVE) == 3 && signed.palette_rooms().await == ROOMS
        })
        .await,
        "the producers run, the rooms in the palette"
    );
    let client = signed.live(LIVE).await;

    let stopping = signed.manager.lifetimes.pause_at(Pause::StoppingProducers);
    let shutdown = {
        let signed = Arc::clone(&signed);
        tokio::spawn(async move { signed.manager.shutdown(LIVE).await })
    };
    stopping
        .reached
        .await
        .expect("the teardown began, its producers not stopped yet");
    syncs.lock().expect("syncs").push_back(json!({
        "next_batch": "s2",
        "rooms": { "join": { "!new:h": {
            "state": { "events": [
                state("m.room.create", "", ME, json!({ "creator": ME, "room_version": "11" }), 900),
                state("m.room.member", ME, ME, json!({ "membership": "join" }), 901),
            ]},
            "timeline": { "events": [], "limited": false },
        }}},
    }));
    client
        .sync_once(SyncSettings::default().timeout(Duration::ZERO))
        .await
        .expect("a new room");
    let new_room = OwnedRoomId::try_from("!new:h").expect("room id");
    assert!(client.get_room(&new_room).is_some(), "the room came");
    assert!(
        eventually(Duration::from_secs(5), || async {
            signed.running(LIVE) <= 1
        })
        .await,
        "the producers that heard of the room went on: {} running",
        signed.running(LIVE)
    );
    assert_eq!(
        signed.palette_rooms().await,
        ROOMS,
        "the palette learned a room of an account that is going"
    );

    let _ = stopping.release.send(());
    shutdown.await.expect("shut down");
    assert_eq!(signed.running(LIVE), 0);
    assert_eq!(signed.palette_rooms().await, 0);
}
