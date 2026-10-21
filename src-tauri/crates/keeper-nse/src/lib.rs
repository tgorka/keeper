//! keeper's iPhone notification extension (Epic 98, story 98.1; AD-412,
//! R24(14), the codemap's Q7 (a)).
//!
//! A push carries an event id and nothing else. iOS starts the `KeeperNotify`
//! extension — a second process, under a 24 MB ceiling, beside the app — and
//! its Swift (`gen/apple/KeeperNotify/NotificationService.swift`) reads the
//! account's session and store passphrase from the shared Keychain group and
//! hands them here with the App Group container's root, the room and the
//! event. This crate opens that one account's store, fetches and decrypts
//! the one event through matrix-sdk-ui's notification client, and asks
//! [`keeper_core::agents::push::classify_notification`] what the lock screen
//! shows: an [`NseAnswer`].
//!
//! - **Nothing before the move.** The app moves its data directory into the
//!   App Group container once (`keeper_core::container`), and that move
//!   refuses a destination holding files it did not put there. So this
//!   crate looks at the group's data directory only through [`store_of`],
//!   which reads three things — the move's [`MOVE_DONE`] marker, the
//!   account's `sdk` directory, the registry's row — and creates nothing;
//!   until all three are there the answer is [`NseAnswer::Generic`], the
//!   notification as the gateway sent it.
//! - **One process among two.** The store is opened with its own
//!   cross-process lock holder ([`HOLDER`]); the notification client takes
//!   its own (`notifications`). The session is restored without refreshing
//!   tokens: an OAuth refresh token is used once, and a refresh here would
//!   strand the app's. An expired token is a generic notification.
//! - **One room.** Only the room the push names is loaded from the store.
//! - **One notification at a time.** iOS may run several of the service's
//!   `didReceive` at once in one extension process; the Swift hands them to
//!   one serial queue, and [`handle_json`] holds a process-wide lock, so
//!   one client lives at a time under the 24 MB ceiling (R339).
//! - **No unwinding across the C ABI.** A panic below [`keeper_nse_handle`]
//!   is the generic notification, not an aborted extension.
//!
//! **An agents server item (106.6; AD-432, D-35).** A push from the agents
//! server names an item by ids alone (`userInfo["keeper_hq"]`). The Swift
//! then hands over the keychain's `agents-server/…` pairings and the
//! notifications already delivered ([`HqRequest`]) through the same entry,
//! and [`answer_item`] makes one authenticated read of that item
//! (`GET /<database>/push/item/{kind}/{id}`) with the pairing of the server
//! whose replica of that database this device holds, and asks
//! [`keeper_core::hq::push::classify`] what shows: an [`NseAnswer::Item`],
//! which names the earlier notifications of the same item it replaces. No
//! store is opened and nothing is written; an item that cannot be read —
//! no pairing, the server or the tailnet unreachable, a refusal — is the
//! generic notification.

use std::collections::BTreeMap;
use std::ffi::{c_char, CStr, CString};
use std::panic::UnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use keeper_core::agents::push::{shown_of, Classified, Shown};
use keeper_core::auth::StoredSession;
use keeper_core::container::{group_data_dir, MOVE_DONE};
use keeper_core::hq::push::{
    credential_for, read_item, replaces, Delivered, HqClassified, HqShown, PushedItem,
};
use keeper_core::hq::routes::HttpRoutes;
use matrix_sdk::cross_process_lock::CrossProcessLockConfig;
use matrix_sdk::ruma::{EventId, OwnedEventId, OwnedRoomId, RoomId};
use matrix_sdk::store::RoomLoadSettings;
use matrix_sdk::Client;
use matrix_sdk_ui::notification_client::{
    NotificationClient, NotificationProcessSetup, NotificationStatus,
};
use serde::{Deserialize, Serialize};

/// The cross-process lock holder of the extension's client; the app's is
/// the SDK's default, `main`.
pub const HOLDER: &str = "nse";

/// The whole of the extension's work: iOS gives an extension about thirty
/// seconds before it shows the best attempt, and this leaves it room.
pub const DEADLINE: Duration = Duration::from_secs(20);

/// What the extension's Swift hands over, as JSON.
#[derive(Debug, Clone, Deserialize)]
pub struct NseRequest {
    /// The App Group container's root (`FileManager.containerURL`).
    pub group: PathBuf,
    /// keeper's local account id, from the push's payload
    /// (`push::PAYLOAD_ACCOUNT`).
    pub account: String,
    pub room_id: String,
    pub event_id: String,
    /// The Keychain's `session/<account>`.
    pub session: String,
    /// The Keychain's `store_passphrase/<account>`, when the store was made
    /// encrypted.
    pub passphrase: Option<String>,
}

/// What the extension's Swift hands over for a push from the agents server,
/// as JSON: the App Group container's root, the push's `keeper_hq`, the
/// keychain's `agents-server/<base URL>` items by key, and the keeper
/// notifications already on the lock screen.
#[derive(Debug, Clone, Deserialize)]
pub struct HqRequest {
    pub group: PathBuf,
    pub hq: serde_json::Value,
    #[serde(default)]
    pub pairings: BTreeMap<String, String>,
    #[serde(default)]
    pub delivered: Vec<Delivered>,
}

/// An agents server item to show, and the notifications of the same item it
/// replaces (a second delivery shows once).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ItemShown {
    #[serde(flatten)]
    pub shown: HqShown,
    pub replaces: Vec<String>,
}

/// What the extension does with the notification, as JSON (`kind` tags it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum NseAnswer {
    /// Show this.
    Show(Shown),
    /// Show this agents server item in place of `replaces`.
    Item(ItemShown),
    /// Show nothing of it (whether iOS lets an extension show nothing
    /// without the filtering entitlement is DW-408's question).
    Drop,
    /// Leave the notification as the gateway sent it; `why` goes to the
    /// log ([`handle_json`] warns it through `tracing`).
    Generic { why: String },
}

fn generic(why: impl std::fmt::Display) -> NseAnswer {
    NseAnswer::Generic {
        why: why.to_string(),
    }
}

/// `answer` as the JSON the Swift reads.
fn written(answer: &NseAnswer) -> String {
    serde_json::to_string(answer)
        .unwrap_or_else(|_| r#"{"kind":"generic","why":"the answer does not write"}"#.to_owned())
}

/// The store a push's account is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    pub sdk_dir: PathBuf,
    pub homeserver: String,
}

/// Why there is no store to open: each means a generic notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoStore {
    /// The push named no account id keeper makes (a ULID).
    NotAnAccount,
    /// The app's data is not in the group container: no App Group in this
    /// build's signature, or the move has not finished.
    NotMoved,
    /// No such account in the moved directory.
    NoAccount,
    /// The registry could not be read.
    Unreadable(String),
}

impl std::fmt::Display for NoStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NoStore::NotAnAccount => write!(f, "the push names no keeper account"),
            NoStore::NotMoved => write!(
                f,
                "keeper's data is not in the shared container yet, so the extension opens nothing"
            ),
            NoStore::NoAccount => write!(f, "the shared container holds no such account"),
            NoStore::Unreadable(why) => write!(f, "the account registry could not be read: {why}"),
        }
    }
}

/// The store of `account` in the App Group container whose root is
/// `group`, by reading only: the account id is keeper's (a ULID, so it is
/// one plain path segment), the group's data directory holds the finished
/// move's [`MOVE_DONE`], the account's `sdk` directory is there, and the
/// registry names its homeserver. Creates nothing, wherever it stops.
pub fn store_of(group: &Path, account: &str) -> Result<Store, NoStore> {
    if ulid::Ulid::from_string(account).is_err() {
        return Err(NoStore::NotAnAccount);
    }
    let data_dir = group_data_dir(group);
    if !data_dir.join(MOVE_DONE).is_file() {
        return Err(NoStore::NotMoved);
    }
    let sdk_dir = data_dir.join("accounts").join(account).join("sdk");
    if !sdk_dir.is_dir() {
        return Err(NoStore::NoAccount);
    }
    match keeper_core::registry::homeserver_of_existing(&data_dir, account) {
        Ok(Some(homeserver)) => Ok(Store {
            sdk_dir,
            homeserver,
        }),
        Ok(None) => Err(NoStore::NoAccount),
        Err(e) => Err(NoStore::Unreadable(e.to_string())),
    }
}

/// What the lock screen shows for `request`.
pub async fn answer(request: NseRequest) -> NseAnswer {
    let started = Instant::now();
    let store = match store_of(&request.group, &request.account) {
        Ok(store) => store,
        Err(why) => return generic(why),
    };
    let (Ok(room_id), Ok(event_id)) = (
        RoomId::parse(&request.room_id),
        EventId::parse(&request.event_id),
    ) else {
        return generic("the push named no room and event");
    };
    decide(&store, &request, room_id, event_id, started)
        .await
        .unwrap_or_else(generic)
}

async fn decide(
    store: &Store,
    request: &NseRequest,
    room_id: OwnedRoomId,
    event_id: OwnedEventId,
    started: Instant,
) -> Result<NseAnswer, String> {
    let stored = StoredSession::from_json(&request.session).map_err(|e| e.to_string())?;
    let client = Client::builder()
        .homeserver_url(&store.homeserver)
        .sqlite_store(&store.sdk_dir, request.passphrase.as_deref())
        .cross_process_store_config(CrossProcessLockConfig::multi_process(HOLDER))
        .build()
        .await
        .map_err(|e| format!("could not open the store: {e}"))?;
    stored
        .restore_into_loading(&client, RoomLoadSettings::One(room_id.clone()))
        .await
        .map_err(|e| e.to_string())?;
    let notifications =
        NotificationClient::new(client.clone(), NotificationProcessSetup::MultipleProcesses)
            .await
            .map_err(|e| format!("could not start the notification client: {e}"))?;
    match notifications
        .get_notification(&room_id, &event_id)
        .await
        .map_err(|e| format!("could not fetch the event: {e}"))?
    {
        NotificationStatus::Event(_) => {}
        NotificationStatus::EventFilteredOut | NotificationStatus::EventRedacted => {
            return Ok(NseAnswer::Drop)
        }
        NotificationStatus::EventNotFound => {
            return Err("the homeserver did not return the event".to_owned())
        }
    }
    let room = notifications
        .get_room(&room_id)
        .or_else(|| client.get_room(&room_id))
        .ok_or("the room is not known")?;
    let event = room
        .event(&event_id, None)
        .await
        .map_err(|e| format!("could not read the event: {e}"))?;
    match shown_of(&client, &room, &event, started).await? {
        Classified::Drop => Ok(NseAnswer::Drop),
        Classified::Show(shown) => Ok(NseAnswer::Show(*shown)),
    }
}

/// What the lock screen shows for an agents server item (106.6): one read
/// of the item with this device's pairing, classified on the phone.
pub async fn answer_item(request: HqRequest) -> NseAnswer {
    let data_dir = group_data_dir(&request.group);
    if !data_dir.join(MOVE_DONE).is_file() {
        return generic(NoStore::NotMoved);
    }
    let Some(pushed) = PushedItem::of_payload(&request.hq) else {
        return generic("the push names no agents item");
    };
    let (server, token, actor) = match credential_for(&data_dir, &pushed, &request.pairings) {
        Ok(credential) => credential,
        Err(why) => return generic(why),
    };
    let read = read_item(
        &HttpRoutes::new(),
        &server,
        &token,
        &actor,
        &pushed,
        keeper_core::hq::sessions::now_ms(),
    )
    .await;
    match read {
        Ok(HqClassified::Show(shown)) => NseAnswer::Item(ItemShown {
            replaces: replaces(&request.delivered, &pushed),
            shown: *shown,
        }),
        Ok(HqClassified::Drop) => NseAnswer::Drop,
        Err(why) => generic(format!("the agents item could not be read: {why}")),
    }
}

/// The request's answer, within [`DEADLINE`].
fn within_deadline(work: impl std::future::Future<Output = NseAnswer>, slow: &str) -> NseAnswer {
    match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime.block_on(async {
            tokio::time::timeout(DEADLINE, work)
                .await
                .unwrap_or_else(|_| generic(slow))
        }),
        Err(e) => generic(format!("no runtime: {e}")),
    }
}

/// The lock that keeps one notification's client alive at a time in this
/// process (R339): two concurrent clients would each open the store, the
/// crypto store and a sliding sync with the same lock holder and connection
/// id, under one 24 MB ceiling.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// `work`, while no other notification of this process is being read. A
/// panic in an earlier one does not stop this one: its poison is cleared.
fn one_at_a_time<T>(work: impl FnOnce() -> T) -> T {
    let _held = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
    work()
}

/// `work`'s answer, or — when it panics — the generic notification's: a
/// panic must not unwind across the C ABI, where it aborts the extension.
fn unwinding_stops_here(work: impl FnOnce() -> String + UnwindSafe) -> String {
    std::panic::catch_unwind(work).unwrap_or_else(|_| written(&generic("the extension failed")))
}

/// The extension's request as JSON in, its [`NseAnswer`] as JSON out —
/// within [`DEADLINE`] of this notification's turn, else generic; one
/// notification at a time ([`one_at_a_time`]). Why an answer is generic is
/// warned through `tracing`.
pub fn handle_json(request: &str) -> String {
    let is_item = serde_json::from_str::<serde_json::Value>(request)
        .is_ok_and(|value| value.get("hq").is_some());
    let answer = if is_item {
        match serde_json::from_str::<HqRequest>(request) {
            Err(_) => generic("the extension's request does not read"),
            Ok(request) => one_at_a_time(|| {
                within_deadline(
                    answer_item(request),
                    "the agents item took too long to read",
                )
            }),
        }
    } else {
        match serde_json::from_str::<NseRequest>(request) {
            Err(_) => generic("the extension's request does not read"),
            Ok(request) => one_at_a_time(|| {
                within_deadline(answer(request), "the event took too long to read")
            }),
        }
    };
    if let NseAnswer::Generic { why } = &answer {
        tracing::warn!(%why, "keeper-nse: the notification stays as the gateway sent it");
    }
    written(&answer)
}

/// The one C-ABI entry the extension's Swift calls: `request` is the JSON of
/// an [`NseRequest`] or an [`HqRequest`]; the answer is the JSON of an [`NseAnswer`] — the
/// generic one when anything below panics — owned by the caller until it
/// hands it to [`keeper_nse_free`]. Null only when the answer held a NUL
/// byte, which JSON escapes never do.
///
/// # Safety
///
/// `request` is null or a NUL-terminated buffer that stays valid for the
/// whole call (Swift's `withCString`).
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn keeper_nse_handle(request: *const c_char) -> *mut c_char {
    let answer = unwinding_stops_here(|| {
        let request = if request.is_null() {
            String::new()
        } else {
            // SAFETY: the caller's contract above — a NUL-terminated buffer
            // valid for this call; it is copied before the call returns.
            unsafe { CStr::from_ptr(request) }
                .to_string_lossy()
                .into_owned()
        };
        handle_json(&request)
    });
    CString::new(answer).map_or(std::ptr::null_mut(), CString::into_raw)
}

/// Release an answer [`keeper_nse_handle`] returned. A panic while it is
/// released stops here, at the C ABI.
///
/// # Safety
///
/// `answer` is null or a pointer [`keeper_nse_handle`] returned, released
/// exactly once.
#[allow(unsafe_code)]
#[no_mangle]
pub unsafe extern "C" fn keeper_nse_free(answer: *mut c_char) {
    if answer.is_null() {
        return;
    }
    // SAFETY: by the contract above it came from `CString::into_raw` in
    // `keeper_nse_handle` and is released once.
    let _ = std::panic::catch_unwind(|| drop(unsafe { CString::from_raw(answer) }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const ACCOUNT: &str = "01J9ZK3Y6V5T4R3Q2P1N0M9K8J";

    /// Every path under `root`, relative, with each file's length.
    fn tree(root: &Path) -> Vec<(PathBuf, u64)> {
        fn walk(root: &Path, dir: &Path, out: &mut Vec<(PathBuf, u64)>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let len = entry.metadata().map(|m| m.len()).unwrap_or(0);
                out.push((path.strip_prefix(root).expect("under").to_owned(), len));
                if path.is_dir() {
                    walk(root, &path, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(root, root, &mut out);
        out.sort();
        out
    }

    fn request(group: &Path, account: &str) -> String {
        serde_json::json!({
            "group": group, "account": account, "room_id": "!r:example.org",
            "event_id": "$e:example.org", "session": "{}", "passphrase": null,
        })
        .to_string()
    }

    fn generic_kind(answer: &str) -> bool {
        serde_json::from_str::<Value>(answer).expect("json")["kind"] == "generic"
    }

    /// The coordinator's contract for rung 3 (rung 2's move refuses a
    /// destination holding files it did not put there): before the move's
    /// marker exists, the extension shows the generic notification and
    /// creates nothing — not the group's data directory, not a file in it.
    #[test]
    fn nothing_is_touched_before_the_move_finished() {
        let group = tempfile::tempdir().expect("group");
        let answer = handle_json(&request(group.path(), ACCOUNT));
        assert!(generic_kind(&answer), "{answer}");
        assert_eq!(tree(group.path()), Vec::new());
        assert!(!group_data_dir(group.path()).exists());

        // A move under way: the journal and an account already moved, no
        // marker yet.
        let data = group_data_dir(group.path());
        std::fs::create_dir_all(data.join("accounts").join(ACCOUNT).join("sdk")).expect("sdk");
        std::fs::write(data.join(keeper_core::container::MOVE_JOURNAL), "/old").expect("journal");
        let before = tree(group.path());
        let answer = handle_json(&request(group.path(), ACCOUNT));
        assert!(generic_kind(&answer), "{answer}");
        assert_eq!(store_of(group.path(), ACCOUNT), Err(NoStore::NotMoved));
        assert_eq!(tree(group.path()), before);
    }

    /// After the move, the store is the named account's own, read without
    /// a write: an id that is not keeper's, an account not moved, or one
    /// without a registry row is generic, and nothing is created.
    #[test]
    fn the_store_is_the_moved_accounts_own() {
        let group = tempfile::tempdir().expect("group");
        let data = group_data_dir(group.path());
        std::fs::create_dir_all(&data).expect("data");
        std::fs::write(data.join(MOVE_DONE), "/old").expect("marker");
        let before = tree(group.path());

        for account in ["../../x", "", "accounts"] {
            assert_eq!(
                store_of(group.path(), account),
                Err(NoStore::NotAnAccount),
                "{account}"
            );
        }
        assert_eq!(store_of(group.path(), ACCOUNT), Err(NoStore::NoAccount));
        assert_eq!(tree(group.path()), before);

        let sdk = data.join("accounts").join(ACCOUNT).join("sdk");
        std::fs::create_dir_all(&sdk).expect("sdk");
        let before = tree(group.path());
        assert_eq!(store_of(group.path(), ACCOUNT), Err(NoStore::NoAccount));
        assert_eq!(tree(group.path()), before, "no keeper.db is created");

        keeper_core::registry::insert_account(
            &data,
            ACCOUNT,
            "@tgorka:example.org",
            "https://matrix.example.org",
            "DEV",
            1,
            0,
            "password",
        )
        .expect("row");
        assert_eq!(
            store_of(group.path(), ACCOUNT),
            Ok(Store {
                sdk_dir: sdk,
                homeserver: "https://matrix.example.org".to_owned(),
            })
        );
        let other = "01J9ZK3Y6V5T4R3Q2P1N0M9K8K";
        std::fs::create_dir_all(data.join("accounts").join(other).join("sdk")).expect("sdk");
        assert_eq!(store_of(group.path(), other), Err(NoStore::NoAccount));

        // A registry row whose store did not move: opening it would create
        // the store's directory, so it is not opened.
        let unmoved = "01J9ZK3Y6V5T4R3Q2P1N0M9K8M";
        keeper_core::registry::insert_account(
            &data,
            unmoved,
            "@marta:example.org",
            "https://matrix.example.org",
            "DEV2",
            1,
            1,
            "password",
        )
        .expect("row");
        assert_eq!(store_of(group.path(), unmoved), Err(NoStore::NoAccount));
        assert!(!data.join("accounts").join(unmoved).exists());
    }
}

#[cfg(test)]
mod one_at_a_time_tests {
    use super::*;
    use keeper_core::agents::push::{Category, PushApproval};
    use serde_json::Value;
    use std::collections::BTreeSet;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// R339: two notifications read at once in one process never have two
    /// clients alive together.
    #[test]
    fn two_notifications_at_once_are_read_one_after_the_other() {
        let inside = AtomicUsize::new(0);
        let most = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..2 {
                scope.spawn(|| {
                    one_at_a_time(|| {
                        let now = inside.fetch_add(1, Ordering::SeqCst) + 1;
                        most.fetch_max(now, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(200));
                        inside.fetch_sub(1, Ordering::SeqCst);
                    })
                });
            }
        });
        assert_eq!(most.load(Ordering::SeqCst), 1);
    }

    /// R339: a panic below the C ABI is the generic notification, and the
    /// next notification is still read.
    #[test]
    fn a_panic_is_the_generic_notification() {
        let answer = std::panic::catch_unwind(|| {
            unwinding_stops_here(|| one_at_a_time(|| panic!("inside the SDK")))
        });
        let answer = answer.expect("the panic stopped at the C ABI");
        assert_eq!(
            serde_json::from_str::<Value>(&answer).expect("json")["kind"],
            "generic"
        );
        let next = std::panic::catch_unwind(|| one_at_a_time(|| 7));
        assert_eq!(next.ok(), Some(7));
    }

    fn keys(value: &Value) -> BTreeSet<&str> {
        value
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect()
    }

    /// The wire the Swift reads (`NotificationService.swift`) and rung 4's
    /// action handler reads back from `userInfo["keeper_approval"]`: a
    /// cross-language contract, so its keys are pinned here.
    #[test]
    fn the_answer_has_the_keys_the_swift_reads() {
        let approval = PushApproval {
            room: "!r:example.org".to_owned(),
            id: "01A".to_owned(),
            binding_digest: "d".to_owned(),
            tier: 2,
            category: Some(Category::Approval),
        };
        let show = serde_json::to_value(NseAnswer::Show(Shown {
            title: "t".to_owned(),
            subtitle: Some("s".to_owned()),
            body: "b".to_owned(),
            category: Some(Category::Approval),
            thread_id: "!r:example.org".to_owned(),
            approval: Some(approval),
            answer: None,
        }))
        .expect("json");
        assert_eq!(
            keys(&show),
            BTreeSet::from([
                "kind",
                "title",
                "subtitle",
                "body",
                "category",
                "thread_id",
                "approval",
                "answer",
            ])
        );
        assert_eq!(show["kind"], "show");
        assert_eq!(show["category"], Category::Approval.id());
        assert_eq!(
            keys(&show["approval"]),
            BTreeSet::from(["room", "id", "binding_digest", "tier", "category"])
        );
        assert_eq!(show["approval"]["category"], Category::Approval.id());
        assert_eq!(
            serde_json::to_value(NseAnswer::Drop).expect("json"),
            serde_json::json!({"kind": "drop"})
        );
        assert_eq!(
            keys(&serde_json::to_value(generic("why")).expect("json")),
            BTreeSet::from(["kind", "why"])
        );
    }
}

#[cfg(test)]
mod item_tests {
    //! The agents server branch over a real HTTP listener standing in for
    //! keeper-hq's item route, answering recorded item JSON.
    use super::*;
    use keeper_core::hq::server::ServerUrl;
    use serde_json::{json, Value};
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    const TOKEN: &str = "tok-0123456789abcdef";
    const ITEM: &str = "01JAPPROVAL0000000000000001";

    /// A listener that answers every request with `status` and `body`, and
    /// keeps each request's line and bearer header.
    struct Server {
        url: String,
        seen: Arc<Mutex<Vec<(String, String)>>>,
    }

    fn serve(status: u16, body: String) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let url = format!("http://{}", listener.local_addr().expect("addr"));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&seen);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut reader = BufReader::new(stream.try_clone().expect("clone"));
                let mut line = String::new();
                let _ = reader.read_line(&mut line);
                let mut bearer = String::new();
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).is_err() || header.trim().is_empty() {
                        break;
                    }
                    if let Some(value) = header
                        .split_once(':')
                        .filter(|(name, _)| name.eq_ignore_ascii_case("authorization"))
                    {
                        bearer = value.1.trim().to_owned();
                    }
                }
                kept.lock()
                    .expect("seen")
                    .push((line.trim().to_owned(), bearer));
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        Server { url, seen }
    }

    /// A moved group container holding ana's replica of `tgdrive` on `url`.
    fn group(url: &str) -> tempfile::TempDir {
        let group = tempfile::tempdir().expect("group");
        let data = group_data_dir(group.path());
        std::fs::create_dir_all(&data).expect("data");
        std::fs::write(data.join(MOVE_DONE), "/old").expect("marker");
        let server = ServerUrl::parse(url).expect("parses").expect("an address");
        let replica = keeper_core::hq::databases::replica(&data, &server, "tgdrive", "ana");
        std::fs::create_dir_all(replica.parent().expect("parent")).expect("mkdir");
        std::fs::write(&replica, b"").expect("replica");
        group
    }

    fn request(group: &std::path::Path, url: &str, delivered: Value) -> String {
        json!({
            "group": group,
            "hq": {"database": "tgdrive", "session_id": "ana.1.3", "kind": "approval",
                   "item_id": ITEM},
            "pairings": {
                format!("agents-server/{url}"): json!({"token": TOKEN, "actorId": "ana",
                    "clientId": "ana.1", "name": "keeper on kalypso"}).to_string(),
                "session/01J9ZK3Y6V5T4R3Q2P1N0M9K8J": "{}",
            },
            "delivered": delivered,
        })
        .to_string()
    }

    /// The approval as the server's item route answers it (recorded shape).
    fn approval(tier: &str, summary: &str) -> String {
        json!({"database": "tgdrive", "kind": "approval", "session_id": "ana.1.3",
            "item_id": ITEM, "row": {
                "id": ITEM, "session_id": "ana.1.3", "page_key": "ana.1.3/2026-10",
                "tool_call_id": "call-1", "tier": tier, "binding_digest": "d1",
                "summary": summary, "approvers": "[\"ana\"]", "state": "pending",
                "decided_by_actor": null, "decided_by_client": null,
                "consumed_by_client": null, "expires_at_ms": 4_102_444_800_000_i64,
                "created_at_ms": 1_000, "author_actor": "nixie",
                "author_client": "nixie.electra"}})
        .to_string()
    }

    fn answer(raw: &str) -> Value {
        serde_json::from_str(raw).expect("json")
    }

    #[test]
    fn an_approval_is_read_once_with_the_device_token_and_shown_in_keepers_words() {
        let server = serve(200, approval("T2", "Run `git status` in keeper"));
        let group = group(&server.url);
        let shown = answer(&handle_json(&request(group.path(), &server.url, json!([]))));
        assert_eq!(shown["kind"], "item", "{shown}");
        assert_eq!(shown["title"], "nixie asks for a decision");
        assert_eq!(shown["body"], "Run `git status` in keeper");
        assert_eq!(shown["category"], "KEEPER_APPROVAL");
        assert_eq!(shown["thread_id"], "tgdrive/ana.1.3");
        assert_eq!(shown["item"]["item_id"], ITEM);
        assert_eq!(shown["approval"]["binding_digest"], "d1");
        assert_eq!(shown["replaces"], json!([]));
        assert_eq!(
            *server.seen.lock().expect("seen"),
            [(
                format!("GET /tgdrive/push/item/approval/{ITEM} HTTP/1.1"),
                format!("Bearer {TOKEN}")
            )],
            "one authenticated read"
        );
    }

    #[test]
    fn a_t4_or_long_request_offers_deny_alone() {
        for (tier, summary) in [
            ("T4", "Delete the backups".to_owned()),
            ("T1", "x".repeat(121)),
        ] {
            let server = serve(200, approval(tier, &summary));
            let group = group(&server.url);
            let shown = answer(&handle_json(&request(group.path(), &server.url, json!([]))));
            assert_eq!(shown["category"], "KEEPER_APPROVAL_OPEN", "{tier}");
            assert_ne!(shown["body"], summary.as_str());
        }
    }

    #[test]
    fn a_second_delivery_of_the_item_replaces_the_first() {
        let server = serve(200, approval("T2", "list files"));
        let group = group(&server.url);
        let item = json!({"database": "tgdrive", "session_id": "ana.1.3", "kind": "approval",
                          "item_id": ITEM});
        let delivered = json!([
            {"id": "first-delivery", "item": item},
            {"id": "another-item", "item": {"database": "tgdrive", "session_id": "ana.1.3",
                                             "kind": "approval", "item_id": "01JOTHER"}},
            {"id": "a-matrix-one"},
        ]);
        let shown = answer(&handle_json(&request(group.path(), &server.url, delivered)));
        assert_eq!(shown["replaces"], json!(["first-delivery"]));
    }

    #[test]
    fn the_proxys_answer_shows_its_words_and_an_ask_shows_nothing() {
        let message = |metadata: Value| {
            json!({"database": "tgdrive", "kind": "message", "session_id": "ana.1.3",
                "item_id": ITEM, "row": {"id": ITEM, "session_id": "ana.1.3",
                "role": "agent", "body": "The build is green.", "body_blob": null,
                "metadata": metadata, "created_at_ms": 1_000, "author_actor": "nixie",
                "author_client": "nixie.electra"}})
            .to_string()
        };
        let ask = |raw: String| raw.replace("\"approval\"", "\"message\"");
        for (metadata, kind) in [(Value::Null, "item"), (json!("{\"kind\":\"ask\"}"), "drop")] {
            let server = serve(200, message(metadata));
            let group = group(&server.url);
            let shown = answer(&handle_json(&ask(request(
                group.path(),
                &server.url,
                json!([]),
            ))));
            assert_eq!(shown["kind"], kind, "{shown}");
            if kind == "item" {
                assert_eq!(shown["body"], "The build is green.");
                assert_eq!(shown["category"], Value::Null);
            }
        }
    }

    #[test]
    fn anything_unreadable_is_the_generic_notification() {
        let refusals = [
            (401, json!({"error": "sync.auth_required"}).to_string()),
            (404, json!({"error": "hq.push_item.not_found"}).to_string()),
            (403, json!({"error": "hq.push_item.not_reader"}).to_string()),
            (200, "not json".to_owned()),
            (200, approval("T2", "list").replace("ana.1.3", "ana.1.9")),
        ];
        for (status, body) in refusals {
            let server = serve(status, body.clone());
            let group = group(&server.url);
            let shown = answer(&handle_json(&request(group.path(), &server.url, json!([]))));
            assert_eq!(shown["kind"], "generic", "{status} {body}: {shown}");
        }

        // The server or the tailnet unreachable: a port nothing listens on.
        let closed = {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
            format!("http://{}", listener.local_addr().expect("addr"))
        };
        let group_dir = group(&closed);
        let shown = answer(&handle_json(&request(group_dir.path(), &closed, json!([]))));
        assert_eq!(shown["kind"], "generic");

        // No pairing for the server holding the database, or the move not
        // done: generic, and no request made.
        let server = serve(200, approval("T2", "list"));
        let elsewhere = group("https://hq.example.org");
        let shown = answer(&handle_json(&request(
            elsewhere.path(),
            &server.url,
            json!([]),
        )));
        assert_eq!(shown["kind"], "generic");
        let unmoved = tempfile::tempdir().expect("group");
        let shown = answer(&handle_json(&request(
            unmoved.path(),
            &server.url,
            json!([]),
        )));
        assert_eq!(shown["kind"], "generic");
        assert!(server.seen.lock().expect("seen").is_empty());
        assert!(!group_data_dir(unmoved.path()).exists(), "nothing created");
    }

    /// The keys the Swift reads from an item answer.
    #[test]
    fn the_item_answer_has_the_keys_the_swift_reads() {
        let server = serve(200, approval("T2", "list files"));
        let group = group(&server.url);
        let shown = answer(&handle_json(&request(group.path(), &server.url, json!([]))));
        let keys: std::collections::BTreeSet<&str> = shown
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            std::collections::BTreeSet::from([
                "kind",
                "title",
                "subtitle",
                "body",
                "category",
                "thread_id",
                "item",
                "approval",
                "replaces",
            ])
        );
    }
}
