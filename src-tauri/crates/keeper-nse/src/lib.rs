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

use std::ffi::{c_char, CStr, CString};
use std::panic::UnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use keeper_core::agents::push::{
    answer_edit, classify_notification, latest_text, shown_name, Classified, PushContext, Shown,
    ANSWER_BUDGET,
};
use keeper_core::agents::room::{holds_agent_power, sealed_by_sender, AgentRoomKind};
use keeper_core::auth::StoredSession;
use keeper_core::container::{group_data_dir, MOVE_DONE};
use matrix_sdk::cross_process_lock::CrossProcessLockConfig;
use matrix_sdk::deserialized_responses::TimelineEvent;
use matrix_sdk::room::{IncludeRelations, RelationsOptions};
use matrix_sdk::ruma::events::relation::RelationType;
use matrix_sdk::ruma::{EventId, OwnedEventId, OwnedRoomId, RoomId, UInt, UserId};
use matrix_sdk::store::RoomLoadSettings;
use matrix_sdk::{Client, Room};
use matrix_sdk_ui::notification_client::{
    NotificationClient, NotificationProcessSetup, NotificationStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The cross-process lock holder of the extension's client; the app's is
/// the SDK's default, `main`.
pub const HOLDER: &str = "nse";

/// The whole of the extension's work: iOS gives an extension about thirty
/// seconds before it shows the best attempt, and this leaves it room.
pub const DEADLINE: Duration = Duration::from_secs(20);

/// How often the edits of an answer are asked for while it is waited on.
const EDIT_POLL: Duration = Duration::from_millis(1500);

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

/// What the extension does with the notification, as JSON (`kind` tags it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum NseAnswer {
    /// Show this.
    Show(Shown),
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

fn json_of(event: &TimelineEvent) -> Option<Value> {
    event.raw().deserialize_as::<Value>().ok()
}

fn sealed(event: &TimelineEvent) -> bool {
    sealed_by_sender(event.encryption_info().map(|info| &**info))
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
    let json = json_of(&event).ok_or("the event does not read")?;
    let sender = json["sender"]
        .as_str()
        .and_then(|sender| UserId::parse(sender).ok())
        .ok_or("the event names no sender")?;
    let own = client.user_id().ok_or("not signed in")?.to_owned();
    let agent_power = holds_agent_power(room.power_levels().await.ok().as_ref(), &sender);
    let sealed_now = sealed(&event);
    let is_message = json["type"] == "m.room.message";
    let answer_agent = if is_message && agent_power && sealed_now && sender != own {
        match client.get_room(&room_id) {
            Some(own_room) => keeper_core::account::proxy_answer_agent(&own_room).await,
            None => None,
        }
    } else {
        None
    };
    let replaced = match json["content"]["m.relates_to"]["event_id"].as_str() {
        Some(target) if is_message => match EventId::parse(target) {
            Ok(target) => room
                .event(&target, None)
                .await
                .ok()
                .filter(sealed)
                .and_then(|original| json_of(&original)),
            Err(_) => None,
        },
        _ => None,
    };
    let name = sender_name(&room, &sender).await;
    let ctx = PushContext {
        own: &own,
        room: AgentRoomKind::of(room.room_type().as_ref()),
        room_id: room_id.as_str(),
        agent_power,
        sealed: sealed_now,
        answer_agent: answer_agent.as_deref(),
        sender_name: &name,
        replaced: replaced.as_ref(),
    };
    match classify_notification(&json, &ctx) {
        Classified::Drop => Ok(NseAnswer::Drop),
        Classified::Show(mut shown) => {
            if let Some(anchor) = shown.answer.clone() {
                let agent = shown_name(&name, sender.as_str());
                shown.body =
                    newest_words(&room, &anchor, &sender, &shown.body, agent, started).await;
            }
            Ok(NseAnswer::Show(*shown))
        }
    }
}

/// How the notification names `sender`: their display name in the room,
/// else their user id's localpart.
async fn sender_name(room: &Room, sender: &UserId) -> String {
    room.get_member_no_sync(sender)
        .await
        .ok()
        .flatten()
        .and_then(|member| member.display_name().map(str::to_owned))
        .unwrap_or_else(|| sender.localpart().to_owned())
}

/// An answer's words when the budget ends (98.1 #2): its edits are asked for
/// every [`EDIT_POLL`] until [`ANSWER_BUDGET`] from the push's arrival, and
/// [`latest_text`] picks the newest seen, else `shown`.
async fn newest_words(
    room: &Room,
    anchor: &str,
    sender: &UserId,
    shown: &str,
    name: &str,
    started: Instant,
) -> String {
    let Ok(anchor_id) = EventId::parse(anchor) else {
        return shown.to_owned();
    };
    let mut seen: Vec<(Duration, String)> = Vec::new();
    while started.elapsed() < ANSWER_BUDGET {
        let left = ANSWER_BUDGET.saturating_sub(started.elapsed());
        tokio::time::sleep(EDIT_POLL.min(left)).await;
        let options = RelationsOptions {
            include_relations: IncludeRelations::RelationsOfType(RelationType::Replacement),
            limit: Some(UInt::from(20u32)),
            ..Default::default()
        };
        let Ok(relations) = room.relations(anchor_id.clone(), options).await else {
            continue;
        };
        let newest = relations
            .chunk
            .iter()
            .filter_map(|edit| {
                let json = json_of(edit)?;
                let at = json["origin_server_ts"].as_u64().unwrap_or(0);
                answer_edit(&json, anchor, sender.as_str(), sealed(edit))
                    .map(|text| (at, text.to_owned()))
            })
            .max_by_key(|(at, _)| *at);
        if let Some((_, text)) = newest {
            seen.push((started.elapsed(), text));
        }
    }
    let edits: Vec<(Duration, &str)> = seen.iter().map(|(at, text)| (*at, text.as_str())).collect();
    latest_text(shown, &edits, ANSWER_BUDGET, name)
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
    let answer = match serde_json::from_str::<NseRequest>(request) {
        Err(_) => generic("the extension's request does not read"),
        Ok(request) => one_at_a_time(|| {
            match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime.block_on(async {
                    tokio::time::timeout(DEADLINE, answer(request))
                        .await
                        .unwrap_or_else(|_| generic("the event took too long to read"))
                }),
                Err(e) => generic(format!("no runtime: {e}")),
            }
        }),
    };
    if let NseAnswer::Generic { why } = &answer {
        tracing::warn!(%why, "keeper-nse: the notification stays as the gateway sent it");
    }
    written(&answer)
}

/// The one C-ABI entry the extension's Swift calls: `request` is the JSON of
/// an [`NseRequest`]; the answer is the JSON of an [`NseAnswer`] — the
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
