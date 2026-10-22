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
//! [`keeper_core::hq::push::classify`] what shows: an [`NseAnswer::Item`].
//! No store is opened and nothing is written; an item that cannot be read —
//! no pairing, the server or the tailnet unreachable, a refusal — is the
//! generic notification.
//!
//! **One notification per item, as far as an extension can (R1066K-02,
//! R1066K-06).** On arrival, before anything is read, the Swift asks for the
//! item's key ([`KeyRequest`]): the key the delivery's notification carries
//! whatever it turns out to be — composed, generic, or the fallback shown
//! when the extension runs out of time. That call records the delivery's
//! arrival ([`DELIVERIES`]) and starts its one deadline ([`DEADLINE`]),
//! which every later step shares: the wait for its turn, the read, and the
//! Swift's waits on the notification center. A delivery whose deadline has
//! passed reads nothing; one still waiting for its turn at it is answered
//! without the turn. Every answer that shows something names the
//! notifications of the same item to remove first
//! ([`keeper_core::hq::push::Fold`]) — those on the lock screen, and those
//! this process handed iOS or saw arrive earlier — and so does the
//! fallback's ([`ExpiryRequest`]), which takes no lock and makes no read. A
//! removal is kept until the Swift reports the notification gone
//! ([`RemovedRequest`]); until then every later answer of any item names it
//! again (`pending`).
//!
//! This is best effort, and the limit is the platform's: iOS names a remote
//! notification itself, the person's gateway sends no APNs collapse id
//! (Sygnal has none), the record is this extension process's, and a
//! removal only applies to a notification already on the lock screen. No
//! test here can show that iOS ends with one notification; the boundary
//! that would guarantee it is a collapse id equal to the item key on the
//! APNs request itself (`docs/agents.md`, the proposed deferred work).

use std::collections::BTreeMap;
use std::ffi::{c_char, CStr, CString};
use std::future::Future;
use std::panic::UnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError, TryLockError};
use std::time::{Duration, Instant};

use keeper_core::agents::push::{shown_of, Classified, Shown};
use keeper_core::auth::StoredSession;
use keeper_core::container::{group_data_dir, MOVE_DONE};
use keeper_core::hq::push::{
    credential_for, fold, read_item, Delivered, Fold, Handed, HqClassified, HqShown, PushedItem,
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

/// The whole of the extension's work for one notification: iOS gives an
/// extension about thirty seconds before it shows the best attempt, and
/// this leaves it room. An agents item's runs from its arrival
/// ([`KeyRequest`]) through its wait for a turn, its read and the Swift's
/// waits on the notification center; a Matrix event's from its turn.
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
/// as JSON: the App Group container's root, the push's `keeper_hq`, this
/// delivery's notification identifier, the keychain's `agents-server/<base
/// URL>` items by key, and the keeper notifications already on the lock
/// screen.
#[derive(Debug, Clone, Deserialize)]
pub struct HqRequest {
    pub group: PathBuf,
    pub hq: serde_json::Value,
    #[serde(default)]
    pub identifier: String,
    #[serde(default)]
    pub pairings: BTreeMap<String, String>,
    #[serde(default)]
    pub delivered: Vec<Delivered>,
}

/// The Swift's first call for a push from the agents server, on arrival and
/// before any read: the push's `keeper_hq`, this delivery's notification
/// identifier and, optionally, a budget shorter than [`DEADLINE`]. Answered
/// with the item's key and the budget its deadline was set with
/// ([`NseAnswer::Key`]).
#[derive(Debug, Clone, Deserialize)]
pub struct KeyRequest {
    pub hq_key: serde_json::Value,
    #[serde(default)]
    pub identifier: String,
    #[serde(default)]
    pub budget_ms: Option<u64>,
}

/// The Swift's call when iOS says the extension's time is up
/// (`serviceExtensionTimeWillExpire`) for a push from the agents server:
/// the push's `keeper_hq`, this delivery's identifier, and the
/// notifications on the lock screen as far as the Swift knows them without
/// waiting. Answered at once with what the fallback removes
/// ([`answer_expired`]): no lock, no read.
#[derive(Debug, Clone, Deserialize)]
pub struct ExpiryRequest {
    pub hq_expired: serde_json::Value,
    #[serde(default)]
    pub identifier: String,
    #[serde(default)]
    pub delivered: Vec<Delivered>,
}

/// The Swift's report after it removed notifications: the ones it removed
/// that were on the lock screen when it did. Only these are forgotten
/// ([`reconcile`]).
#[derive(Debug, Clone, Deserialize)]
pub struct RemovedRequest {
    pub hq_removed: Vec<String>,
}

/// What a delivery of an agents item removes: its item's [`Fold`], and the
/// removals an earlier answer named for other items that the Swift has not
/// reported done (`pending`) — removed too, never waited for.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct ItemFold {
    #[serde(flatten)]
    pub fold: Fold,
    pub pending: Vec<String>,
}

/// An agents server item to show, the key it carries and the notifications
/// it removes (a second delivery shows once).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ItemShown {
    #[serde(flatten)]
    pub shown: HqShown,
    #[serde(flatten)]
    pub fold: ItemFold,
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
    /// log ([`handle_json`] warns it through `tracing`). For an agents item,
    /// `fold`: the item's key the generic notification carries — and nothing
    /// else of keeper's — and what it removes.
    Generic {
        why: String,
        #[serde(flatten)]
        fold: Option<ItemFold>,
    },
    /// The key every notification of the agents item this push names
    /// carries, and the budget of its one deadline ([`KeyRequest`]).
    Key { key: String, budget_ms: u64 },
    /// The removals the Swift reported are forgotten ([`RemovedRequest`]).
    Reconciled,
}

fn generic(why: impl std::fmt::Display) -> NseAnswer {
    NseAnswer::Generic {
        why: why.to_string(),
        fold: None,
    }
}

/// How long a delivery's arrival or handing is remembered: longer than iOS
/// gives the extension.
const REMEMBERED_FOR: Duration = Duration::from_secs(60);

/// The most removals kept until the Swift reports them done; past it the
/// oldest is dropped. One the person dismissed is never reported, so this
/// bounds what an extension process keeps.
const PENDING_MAX: usize = 64;

/// Where a delivery of an agents item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Its key was asked for; nothing of it is handed to iOS yet.
    Arrived,
    /// Its notification was handed to iOS: shown, generic, or the fallback.
    Handed,
    /// Handed, and a later answer named it for removal: kept, and named
    /// again, until the Swift reports it removed from the lock screen.
    Replaced,
}

/// One delivery of an agents item in this extension process.
struct Delivery {
    id: String,
    key: String,
    arrived: Instant,
    deadline: Instant,
    stage: Stage,
}

/// The deliveries of agents items this extension process has seen, in the
/// order they arrived.
static DELIVERIES: Mutex<Vec<Delivery>> = Mutex::new(Vec::new());

fn deliveries() -> MutexGuard<'static, Vec<Delivery>> {
    let mut all = DELIVERIES.lock().unwrap_or_else(PoisonError::into_inner);
    all.retain(|one| one.stage == Stage::Replaced || one.arrived.elapsed() < REMEMBERED_FOR);
    let pending = all
        .iter()
        .filter(|one| one.stage == Stage::Replaced)
        .count();
    let mut over = pending.saturating_sub(PENDING_MAX);
    all.retain(|one| {
        let drop = over > 0 && one.stage == Stage::Replaced;
        over -= usize::from(drop);
        !drop
    });
    all
}

/// The key of the item `request` names; its delivery's arrival recorded,
/// once, and its deadline set: [`DEADLINE`] from now, or the request's
/// shorter budget.
pub fn answer_key(request: &KeyRequest) -> NseAnswer {
    let Some(pushed) = PushedItem::of_payload(&request.hq_key) else {
        return generic("the push names no agents item");
    };
    let key = pushed.tag();
    let budget = request
        .budget_ms
        .map(Duration::from_millis)
        .map_or(DEADLINE, |budget| budget.min(DEADLINE));
    let mut all = deliveries();
    if !request.identifier.is_empty() && !all.iter().any(|one| one.id == request.identifier) {
        let now = Instant::now();
        all.push(Delivery {
            id: request.identifier.clone(),
            key: key.clone(),
            arrived: now,
            deadline: now + budget,
            stage: Stage::Arrived,
        });
    }
    NseAnswer::Key {
        key,
        budget_ms: u64::try_from(budget.as_millis()).unwrap_or(u64::MAX),
    }
}

/// The deadline of the delivery `id`: its arrival's, or [`DEADLINE`] from
/// now for one whose arrival was never recorded; `None` once its
/// notification was handed to iOS (its fallback, at expiry).
fn deadline_of(id: &str) -> Option<Instant> {
    match deliveries().iter().find(|one| one.id == id) {
        None => Some(Instant::now() + DEADLINE),
        Some(one) => (one.stage == Stage::Arrived).then_some(one.deadline),
    }
}

/// `answer` to the delivery `own` of `pushed`, with what it removes when it
/// shows anything — the item, or the generic notification whatever made it
/// generic (no pairing, the tailnet off, too slow, the fallback) — so the
/// item shows once.
///
/// It removes every other notification of the item on the lock screen
/// (`delivered`), every one of this process that arrived before it, and
/// every one handed to iOS whenever it arrived; one that arrived later and
/// is not handed yet removes this one when it is. Those not on the lock
/// screen are awaited, except ones an earlier answer named already. Each
/// named is kept until the Swift reports it gone ([`reconcile`]), and named
/// again meanwhile: for its item in `replaces`, for any other in `pending`.
///
/// A delivery that shows nothing is forgotten. One already handed to iOS —
/// its fallback, at expiry — removes nothing more: its read's answer comes
/// after iOS showed something for it, and what it would name may be newer.
fn settle(pushed: &PushedItem, own: &str, delivered: &[Delivered], answer: NseAnswer) -> NseAnswer {
    let mut all = deliveries();
    let at = all.iter().position(|one| one.id == own);
    if answer == NseAnswer::Drop {
        all.retain(|one| one.id != own);
        return answer;
    }
    if at.is_some_and(|at| all[at].stage != Stage::Arrived) {
        let fold = Fold {
            key: pushed.tag(),
            ..Fold::default()
        };
        return attach(
            generic("the delivery's notification was handed to iOS already"),
            ItemFold {
                fold,
                pending: Vec::new(),
            },
        );
    }
    let earlier: Vec<Handed> = all
        .iter()
        .enumerate()
        .filter(|(i, one)| {
            one.id != own && (at.is_none_or(|at| *i < at) || one.stage != Stage::Arrived)
        })
        .map(|(_, one)| Handed {
            id: one.id.clone(),
            key: one.key.clone(),
        })
        .collect();
    let mut item_fold = fold(pushed, own, delivered, &earlier);
    item_fold.awaits.retain(|id| {
        !all.iter()
            .any(|one| one.id == *id && one.stage == Stage::Replaced)
    });
    for one in all.iter_mut() {
        if item_fold.replaces.contains(&one.id) {
            one.stage = Stage::Replaced;
        }
    }
    match at {
        Some(at) => all[at].stage = Stage::Handed,
        None if !own.is_empty() => {
            let now = Instant::now();
            all.push(Delivery {
                id: own.to_owned(),
                key: item_fold.key.clone(),
                arrived: now,
                deadline: now,
                stage: Stage::Handed,
            });
        }
        None => {}
    }
    let pending = all
        .iter()
        .filter(|one| one.stage == Stage::Replaced && one.key != item_fold.key)
        .map(|one| one.id.clone())
        .collect();
    drop(all);
    attach(
        answer,
        ItemFold {
            fold: item_fold,
            pending,
        },
    )
}

/// `answer` carrying `fold`, when it shows something.
fn attach(answer: NseAnswer, fold: ItemFold) -> NseAnswer {
    match answer {
        NseAnswer::Item(item) => NseAnswer::Item(ItemShown { fold, ..item }),
        NseAnswer::Generic { why, .. } => NseAnswer::Generic {
            why,
            fold: Some(fold),
        },
        other => other,
    }
}

/// What the fallback of the delivery `request` removes, at once — no lock
/// and no read, so never behind the read that used up its time — its
/// delivery recorded as handed to iOS ([`settle`]).
pub fn answer_expired(request: &ExpiryRequest) -> NseAnswer {
    let Some(pushed) = PushedItem::of_payload(&request.hq_expired) else {
        return generic("the push names no agents item");
    };
    settle(
        &pushed,
        &request.identifier,
        &request.delivered,
        generic("the extension ran out of time"),
    )
}

/// Forget the deliveries the Swift removed from the lock screen.
pub fn reconcile(request: &RemovedRequest) -> NseAnswer {
    deliveries().retain(|one| !request.hq_removed.contains(&one.id));
    NseAnswer::Reconciled
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
/// of the item with this device's pairing, classified on the phone. What
/// it removes is [`settle`]'s.
pub async fn answer_item(request: &HqRequest) -> NseAnswer {
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
            shown: *shown,
            fold: ItemFold::default(),
        }),
        Ok(HqClassified::Drop) => NseAnswer::Drop,
        Err(why) => generic(format!("the agents item could not be read: {why}")),
    }
}

/// What the delivery `request` shows, by its deadline ([`KeyRequest`]): it
/// waits for its turn no longer than that, reads only before it, and a read
/// still going at it is the generic notification. One whose fallback was
/// handed to iOS already reads nothing.
fn answer_in_time(request: &HqRequest) -> NseAnswer {
    let Some(deadline) = deadline_of(&request.identifier) else {
        return generic("the delivery's notification was handed to iOS already");
    };
    one_at_a_time_until(deadline, || {
        by(
            deadline,
            answer_item(request),
            "the agents item took too long to read",
        )
    })
    .unwrap_or_else(|| generic("the agents item's time ran out while it waited for its turn"))
}

/// `work`'s answer by `deadline`; `slow`'s generic one at it, and at once
/// when it has passed — `work` is then never started.
fn by(deadline: Instant, work: impl Future<Output = NseAnswer>, slow: &str) -> NseAnswer {
    let Some(left) = deadline
        .checked_duration_since(Instant::now())
        .filter(|left| !left.is_zero())
    else {
        return generic(slow);
    };
    match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime.block_on(async {
            tokio::time::timeout(left, work)
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

/// How often a delivery waiting for its turn looks again.
const TURN_POLL: Duration = Duration::from_millis(5);

/// [`one_at_a_time`], waiting for the turn no later than `deadline`: `None`
/// when it passes first, and `work` is not run.
fn one_at_a_time_until<T>(deadline: Instant, work: impl FnOnce() -> T) -> Option<T> {
    loop {
        let _held = match ONE_AT_A_TIME.try_lock() {
            Ok(held) => held,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => {
                let left = deadline
                    .checked_duration_since(Instant::now())
                    .filter(|left| !left.is_zero())?;
                std::thread::sleep(left.min(TURN_POLL));
                continue;
            }
        };
        return Some(work());
    }
}

/// `work`'s answer, or — when it panics — the generic notification's: a
/// panic must not unwind across the C ABI, where it aborts the extension.
fn unwinding_stops_here(work: impl FnOnce() -> String + UnwindSafe) -> String {
    std::panic::catch_unwind(work).unwrap_or_else(|_| written(&generic("the extension failed")))
}

/// The extension's request as JSON in, its [`NseAnswer`] as JSON out, else
/// generic; one read at a time ([`one_at_a_time`]). A Matrix event is
/// answered within [`DEADLINE`] of its turn; an agents item by the deadline
/// its arrival set ([`answer_in_time`]). The arrival, the expiry and the
/// report of a removal take no lock and read nothing. Why an answer is
/// generic is warned through `tracing`.
pub fn handle_json(request: &str) -> String {
    let value = serde_json::from_str::<serde_json::Value>(request).ok();
    let has = |key: &str| value.as_ref().is_some_and(|value| value.get(key).is_some());
    let unread = || generic("the extension's request does not read");
    let answer = if has("hq_key") {
        // No read and no lock: this answers at once, even while another
        // notification is being read.
        serde_json::from_str::<KeyRequest>(request).map_or_else(|_| unread(), |r| answer_key(&r))
    } else if has("hq_expired") {
        serde_json::from_str::<ExpiryRequest>(request)
            .map_or_else(|_| unread(), |r| answer_expired(&r))
    } else if has("hq_removed") {
        serde_json::from_str::<RemovedRequest>(request).map_or_else(|_| unread(), |r| reconcile(&r))
    } else if has("hq") {
        match serde_json::from_str::<HqRequest>(request) {
            Err(_) => unread(),
            Ok(request) => {
                let answer = answer_in_time(&request);
                match PushedItem::of_payload(&request.hq) {
                    Some(pushed) => {
                        settle(&pushed, &request.identifier, &request.delivered, answer)
                    }
                    None => answer,
                }
            }
        }
    } else {
        match serde_json::from_str::<NseRequest>(request) {
            Err(_) => unread(),
            Ok(request) => one_at_a_time(|| {
                by(
                    Instant::now() + DEADLINE,
                    answer(request),
                    "the event took too long to read",
                )
            }),
        }
    };
    if let NseAnswer::Generic { why, .. } = &answer {
        tracing::warn!(%why, "keeper-nse: the notification stays as the gateway sent it");
    }
    written(&answer)
}

/// The one C-ABI entry the extension's Swift calls: `request` is the JSON of
/// an [`NseRequest`], a [`KeyRequest`], an [`HqRequest`], an
/// [`ExpiryRequest`] or a [`RemovedRequest`]; the answer is the JSON of an
/// [`NseAnswer`] — the
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
    use std::collections::BTreeSet;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
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

    fn payload(item: &str) -> Value {
        json!({"database": "tgdrive", "session_id": "ana.1.3", "kind": "approval",
               "item_id": item})
    }

    /// The Swift's read of the delivery `identifier` of `item`.
    fn request_for(
        group: &std::path::Path,
        url: &str,
        item: &str,
        identifier: &str,
        delivered: Value,
    ) -> String {
        json!({
            "group": group,
            "hq": payload(item),
            "identifier": identifier,
            "pairings": {
                format!("agents-server/{url}"): json!({"token": TOKEN, "actorId": "ana",
                    "clientId": "ana.1", "name": "keeper on kalypso"}).to_string(),
                "session/01J9ZK3Y6V5T4R3Q2P1N0M9K8J": "{}",
            },
            "delivered": delivered,
        })
        .to_string()
    }

    fn request(group: &std::path::Path, url: &str, delivered: Value) -> String {
        request_for(group, url, ITEM, "", delivered)
    }

    /// The Swift's first call for the delivery `identifier` of `item`.
    fn key_of(item: &str, identifier: &str) -> Value {
        answer(&handle_json(
            &json!({"hq_key": payload(item), "identifier": identifier}).to_string(),
        ))
    }

    /// The approval as the server's item route answers it (recorded shape),
    /// with the tool call it runs.
    fn approval_of(item: &str, tier: &str, summary: &str, tool_call: Value) -> String {
        json!({"database": "tgdrive", "kind": "approval", "session_id": "ana.1.3",
            "item_id": item, "row": {
                "id": item, "session_id": "ana.1.3", "page_key": "ana.1.3/2026-10",
                "tool_call_id": "call-1", "tier": tier, "binding_digest": "d1",
                "summary": summary, "approvers": "[\"ana\"]", "state": "pending",
                "decided_by_actor": null, "decided_by_client": null,
                "consumed_by_client": null, "expires_at_ms": 4_102_444_800_000_i64,
                "created_at_ms": 1_000, "author_actor": "nixie",
                "author_client": "nixie.electra"},
            "tool_call": tool_call})
        .to_string()
    }

    /// The approval's tool call as the server's item route answers it.
    fn call(name: &str, args: Value) -> Value {
        json!({"id": "call-1", "session_id": "ana.1.3", "page_key": "ana.1.3/2026-10",
               "turn_id": "t1", "name": name, "args": args.to_string(), "status": "started",
               "result": null, "result_blob": null, "started_at_ms": 900,
               "ended_at_ms": null, "created_at_ms": 900, "author_actor": "nixie",
               "author_client": "nixie.electra"})
    }

    fn approval(tier: &str, summary: &str) -> String {
        approval_of(
            ITEM,
            tier,
            summary,
            call("run", json!({"argv": ["git", "status"]})),
        )
    }

    fn answer(raw: &str) -> Value {
        serde_json::from_str(raw).expect("json")
    }

    #[test]
    fn an_approval_is_read_once_with_the_device_token_and_shown_in_keepers_words() {
        let server = serve(200, approval("T2", "Run `git` in workspace/keeper"));
        let group = group(&server.url);
        let shown = answer(&handle_json(&request(group.path(), &server.url, json!([]))));
        assert_eq!(shown["kind"], "item", "{shown}");
        assert_eq!(shown["title"], "nixie asks: Run `git` in workspace/keeper");
        assert_eq!(shown["body"], r#"run {"argv":["git","status"]}"#);
        assert_eq!(shown["category"], "KEEPER_APPROVAL");
        assert_eq!(shown["thread_id"], "tgdrive/ana.1.3");
        assert_eq!(shown["item"]["item_id"], ITEM);
        assert_eq!(shown["approval"]["binding_digest"], "d1");
        assert_eq!(shown["key"], format!("tgdrive/approval/{ITEM}"));
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
        let summary = "Run `echo` in workspace/keeper";
        for (tier, argv) in [
            ("T4", json!(["rm", "-rf", "backups"])),
            ("T1", json!(["echo", "x".repeat(121)])),
        ] {
            let server = serve(
                200,
                approval_of(ITEM, tier, summary, call("run", json!({ "argv": argv }))),
            );
            let group = group(&server.url);
            let shown = answer(&handle_json(&request(group.path(), &server.url, json!([]))));
            assert_eq!(shown["category"], "KEEPER_APPROVAL_OPEN", "{tier}");
            assert!(!shown["body"].as_str().expect("body").contains("argv"));
        }
    }

    /// R1066K-01 through the extension: a short summary over a request
    /// keeper cannot show whole offers *Deny* alone.
    #[test]
    fn a_short_summary_over_a_request_keeper_cannot_show_whole_offers_deny_alone() {
        let summary = "Run `git` in workspace/keeper";
        for tool_call in [
            Value::Null,
            call(
                "run",
                json!({"argv": ["git", "push", "--force", "x".repeat(120)]}),
            ),
            call("run", Value::from("git push --force")),
            // R1066K-05: reads as `rm report.txt`, removes another file.
            call("run", json!({"argv": ["rm", "report\u{200b}.txt"]})),
            call("mcp__paseo__create_agent", json!({"prompt": "fix it"})),
            call(
                "mcp__paseo__send_agent_prompt",
                json!({"agentId": "a", "prompt": "go"}),
            ),
        ] {
            let server = serve(200, approval_of(ITEM, "T2", summary, tool_call.clone()));
            let group = group(&server.url);
            let shown = answer(&handle_json(&request(group.path(), &server.url, json!([]))));
            assert_eq!(shown["kind"], "item", "{tool_call}");
            assert_eq!(shown["category"], "KEEPER_APPROVAL_OPEN", "{tool_call}");
            assert_eq!(
                shown["approval"]["category"], "KEEPER_APPROVAL_OPEN",
                "{tool_call}"
            );
        }
    }

    #[test]
    fn a_second_delivery_of_the_item_replaces_the_first() {
        let server = serve(200, approval("T2", "list files"));
        let group = group(&server.url);
        let delivered = json!([
            {"id": "first-delivery", "key": format!("tgdrive/approval/{ITEM}")},
            {"id": "another-item", "key": "tgdrive/approval/01JOTHER"},
            {"id": "a-matrix-one", "key": ""},
        ]);
        let shown = answer(&handle_json(&request(group.path(), &server.url, delivered)));
        assert_eq!(shown["replaces"], json!(["first-delivery"]));
    }

    /// The server or the tailnet unreachable: a port nothing listens on.
    fn closed() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        format!("http://{}", listener.local_addr().expect("addr"))
    }

    /// A server that takes every connection and never answers — the
    /// tailnet up and the server gone quiet, not a closed port — so a read
    /// uses all the time it is given. Counts the connections it took.
    fn blackholed() -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let url = format!("http://{}", listener.local_addr().expect("addr"));
        let taken = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&taken);
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                count.fetch_add(1, Ordering::SeqCst);
                held.push(stream);
            }
        });
        (url, taken)
    }

    /// The Swift's first call for the delivery `identifier` of `item`, its
    /// deadline `budget_ms` from now.
    fn key_in(item: &str, identifier: &str, budget_ms: u64) -> Value {
        answer(&handle_json(
            &json!({"hq_key": payload(item), "identifier": identifier, "budget_ms": budget_ms})
                .to_string(),
        ))
    }

    /// The tests whose reads hold the turn for a long time run one at a
    /// time, so one's blackholed read never spends another's deadline.
    static SLOW: Mutex<()> = Mutex::new(());

    fn slow() -> std::sync::MutexGuard<'static, ()> {
        SLOW.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// R1066K-02, the tailnet off: every delivery is generic, each carries
    /// the item's key and nothing else of keeper's, and each removes the one
    /// before it — on the lock screen, or still on its way.
    #[test]
    fn deliveries_with_the_tailnet_off_leave_one_notification() {
        const OFF: &str = "01JFOLDOFF0000000000000001";
        let url = closed();
        let group = group(&url);
        let key = format!("tgdrive/approval/{OFF}");
        assert_eq!(
            key_of(OFF, "off-1"),
            json!({"kind": "key", "key": key, "budget_ms": 20_000})
        );
        let first = answer(&handle_json(&request_for(
            group.path(),
            &url,
            OFF,
            "off-1",
            json!([]),
        )));
        assert_eq!(first["kind"], "generic");
        assert_eq!(
            (&first["key"], &first["replaces"]),
            (&json!(key), &json!([]))
        );
        // The second arrives once the first is on the lock screen.
        assert_eq!(key_of(OFF, "off-2")["key"], json!(key));
        let second = answer(&handle_json(&request_for(
            group.path(),
            &url,
            OFF,
            "off-2",
            json!([{"id": "off-1", "key": key}]),
        )));
        assert_eq!(second["kind"], "generic");
        assert_eq!(second["replaces"], json!(["off-1"]));
        assert_eq!(second["awaits"], json!([]));
        // A third arrives while the second is handed but not yet shown.
        key_of(OFF, "off-3");
        let third = answer(&handle_json(&request_for(
            group.path(),
            &url,
            OFF,
            "off-3",
            json!([]),
        )));
        // The first, named by the second already, is named again: the Swift
        // has not reported it gone.
        assert_eq!(third["replaces"], json!(["off-1", "off-2"]));
        assert_eq!(third["awaits"], json!(["off-2"]));
        // The generic answer carries ids keeper validated and nothing else.
        let keys: BTreeSet<&str> = third
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            BTreeSet::from(["kind", "why", "key", "replaces", "awaits", "pending"])
        );
    }

    /// R1066K-02, the tailnet on: the second delivery arrives while the first
    /// is still being read. Deliveries are read one after the other, so the
    /// second is read once the first is handed to iOS; it removes the first
    /// whether or not iOS shows it yet, and the first never removes the
    /// second.
    #[test]
    fn a_second_delivery_arriving_while_the_first_is_read_leaves_one_notification() {
        const ON: &str = "01JFOLDON00000000000000001";
        let server = serve(
            200,
            approval_of(ON, "T2", "list files", call("run", json!({"argv": ["ls"]}))),
        );
        let group = group(&server.url);
        let key = format!("tgdrive/approval/{ON}");
        // Both arrive before either is read.
        key_of(ON, "on-1");
        key_of(ON, "on-2");
        let first = answer(&handle_json(&request_for(
            group.path(),
            &server.url,
            ON,
            "on-1",
            json!([]),
        )));
        assert_eq!(first["kind"], "item");
        assert_eq!(first["key"], json!(key));
        assert_eq!(first["replaces"], json!([]), "never the later one");
        let second = answer(&handle_json(&request_for(
            group.path(),
            &server.url,
            ON,
            "on-2",
            json!([]),
        )));
        assert_eq!(second["kind"], "item");
        assert_eq!(second["replaces"], json!(["on-1"]));
        assert_eq!(second["awaits"], json!(["on-1"]));
    }

    /// R1066K-02, the fallback: a delivery the extension ran out of time on
    /// is shown by iOS with its key; the next delivery of the item removes
    /// it. A delivery that showed nothing is not waited for.
    #[test]
    fn a_fallback_is_replaced_and_a_dropped_delivery_is_not_waited_for() {
        const LATE: &str = "01JFOLDLATE000000000000001";
        let url = closed();
        let late_group = group(&url);
        key_of(LATE, "late-1"); // never answered: iOS showed the fallback
        let next = answer(&handle_json(&request_for(
            late_group.path(),
            &url,
            LATE,
            "late-2",
            json!([]),
        )));
        assert_eq!(next["replaces"], json!(["late-1"]));

        const ASK: &str = "01JFOLDASK0000000000000001";
        let ask = json!({"database": "tgdrive", "kind": "message", "session_id": "ana.1.3",
            "item_id": ASK, "row": {"id": ASK, "session_id": "ana.1.3", "role": "agent",
            "body": "Which one?", "metadata": "{\"kind\":\"ask\"}", "created_at_ms": 1_000,
            "author_actor": "nixie", "author_client": "nixie.electra"}});
        let server = serve(200, ask.to_string());
        let ask_group = group(&server.url);
        let message = |identifier: &str| {
            request_for(ask_group.path(), &server.url, ASK, identifier, json!([]))
                .replace("\"approval\"", "\"message\"")
        };
        let ask_payload = json!({"database": "tgdrive", "session_id": "ana.1.3",
                                 "kind": "message", "item_id": ASK});
        for identifier in ["ask-1", "ask-2"] {
            handle_json(&json!({"hq_key": ask_payload, "identifier": identifier}).to_string());
            assert_eq!(answer(&handle_json(&message(identifier)))["kind"], "drop");
        }
        // Neither is waited for by a later delivery that shows.
        let later = settle(
            &PushedItem::of_payload(&ask_payload).expect("an item"),
            "ask-3",
            &[],
            generic("the tailnet is off"),
        );
        let NseAnswer::Generic {
            fold: Some(later), ..
        } = later
        else {
            panic!("a folded generic answer");
        };
        assert!(later.fold.replaces.is_empty(), "{later:?}");
    }

    /// The generic metadata holds ids keeper validated: a push naming no
    /// item keeper reads has no key at all.
    #[test]
    fn a_push_naming_no_readable_item_has_no_key() {
        for hq in [
            json!({"database": "../x", "session_id": "s", "kind": "approval", "item_id": "i"}),
            json!({"database": "tgdrive", "session_id": "s", "kind": "approval",
                   "item_id": "a\u{202e}b"}),
            json!("tgdrive/approval/i"),
        ] {
            let shown = answer(&handle_json(
                &json!({"hq_key": hq, "identifier": "bad-1"}).to_string(),
            ));
            assert_eq!(shown["kind"], "generic", "{hq}");
            assert_eq!(shown.get("key"), None, "{hq}");
        }
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

        let closed = closed();
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
        let keys: BTreeSet<&str> = shown
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            BTreeSet::from([
                "kind",
                "title",
                "subtitle",
                "body",
                "category",
                "thread_id",
                "item",
                "approval",
                "key",
                "replaces",
                "awaits",
                "pending",
            ])
        );
    }

    /// R1066K-06: deliveries of one item queued behind one another while
    /// the server takes the connection and never answers share the
    /// deadline their arrival set. The first read uses it up; the ones
    /// waiting behind it read nothing and answer at once, so the three end
    /// within one budget, not one each. Each still removes the ones before.
    #[test]
    fn queued_deliveries_of_one_item_share_the_deadline_their_arrival_set() {
        let _slow = slow();
        const Q: &str = "01JFOLDQUEUE00000000000001";
        let (url, taken) = blackholed();
        let group = group(&url);
        let key = format!("tgdrive/approval/{Q}");
        let arrived = Instant::now();
        for id in ["q-1", "q-2", "q-3"] {
            assert_eq!(
                key_in(Q, id, 600),
                json!({"kind": "key", "key": key, "budget_ms": 600})
            );
        }
        // The Swift's serial queue: one read after the other.
        let answers: Vec<Value> = ["q-1", "q-2", "q-3"]
            .into_iter()
            .map(|id| {
                answer(&handle_json(&request_for(
                    group.path(),
                    &url,
                    Q,
                    id,
                    json!([]),
                )))
            })
            .collect();
        let took = arrived.elapsed();
        assert!(
            took >= Duration::from_millis(500),
            "the read waited: {took:?}"
        );
        assert!(took < Duration::from_millis(1_100), "one budget: {took:?}");
        assert!(
            taken.load(Ordering::SeqCst) >= 1,
            "a read reached the server"
        );
        for one in &answers {
            assert_eq!(
                (&one["kind"], &one["key"]),
                (&json!("generic"), &json!(key))
            );
        }
        assert_eq!(answers[0]["replaces"], json!([]));
        assert_eq!(answers[1]["replaces"], json!(["q-1"]));
        assert_eq!(answers[1]["awaits"], json!(["q-1"]));
        assert_eq!(answers[2]["replaces"], json!(["q-1", "q-2"]));
        assert_eq!(
            answers[2]["awaits"],
            json!(["q-2"]),
            "q-1 is not awaited twice"
        );
    }

    /// R1066K-06: iOS's expiry of the current duplicate while its read
    /// waits for its turn behind a read that never answers. The expiry is
    /// answered at once — no lock, no read — and its fallback removes the
    /// item's notifications already shown. The queued read gives up at its
    /// own deadline, not the blocker's, and its late answer removes
    /// nothing: the fallback is the item's newest. A later delivery removes
    /// the fallback in turn.
    #[test]
    fn the_expiry_of_a_delivery_queued_behind_a_read_removes_its_item_at_once() {
        let _slow = slow();
        const X: &str = "01JFOLDEXPIRY0000000000001";
        const Y: &str = "01JFOLDBLOCK00000000000001";
        let server = serve(
            200,
            approval_of(X, "T2", "list files", call("run", json!({"argv": ["ls"]}))),
        );
        let shown_group = group(&server.url);
        let read = |id: &str| {
            answer(&handle_json(&request_for(
                shown_group.path(),
                &server.url,
                X,
                id,
                json!([]),
            )))
        };
        let key = format!("tgdrive/approval/{X}");
        key_of(X, "x-1");
        assert_eq!(read("x-1")["kind"], "item");
        key_of(X, "x-2");
        assert_eq!(read("x-2")["replaces"], json!(["x-1"]));

        let (quiet, _) = blackholed();
        let blocked_group = group(&quiet);
        key_in(Y, "y-1", 1_500);
        let started = Instant::now();
        std::thread::scope(|scope| {
            let blocker = scope.spawn(|| {
                answer(&handle_json(&request_for(
                    blocked_group.path(),
                    &quiet,
                    Y,
                    "y-1",
                    json!([]),
                )))
            });
            std::thread::sleep(Duration::from_millis(100));
            key_in(X, "x-3", 600);
            let queued = scope.spawn(|| (read("x-3"), Instant::now()));
            std::thread::sleep(Duration::from_millis(100));

            let asked = Instant::now();
            let expired = answer(&handle_json(
                &json!({"hq_expired": payload(X), "identifier": "x-3", "delivered": []})
                    .to_string(),
            ));
            assert!(
                asked.elapsed() < Duration::from_millis(100),
                "the expiry waits for no turn: {:?}",
                asked.elapsed()
            );
            assert_eq!(
                (&expired["kind"], &expired["key"]),
                (&json!("generic"), &json!(key))
            );
            assert_eq!(expired["replaces"], json!(["x-1", "x-2"]));

            let (late, at) = queued.join().expect("the queued read");
            assert!(
                at.duration_since(started) < Duration::from_millis(1_200),
                "the queued read ends at its own deadline: {:?}",
                at.duration_since(started)
            );
            assert_eq!(late["kind"], "generic");
            assert_eq!(late["replaces"], json!([]), "never the fallback's survivor");
            assert_eq!(late["pending"], json!([]));
            assert_eq!(blocker.join().expect("the blocker")["kind"], "generic");
        });
        key_of(X, "x-4");
        let after = read("x-4");
        assert_eq!(after["kind"], "item");
        assert!(
            after["replaces"]
                .as_array()
                .expect("replaces")
                .contains(&json!("x-3")),
            "{after}"
        );
    }

    /// R1066K-06: a removal is kept until the Swift reports the notification
    /// gone. Named again by the item's next delivery (not awaited twice),
    /// and by another item's (`pending`); forgotten once reported.
    #[test]
    fn a_removal_is_named_again_until_the_extension_reports_it_gone() {
        const P: &str = "01JFOLDPEND000000000000001";
        const R: &str = "01JFOLDREST000000000000001";
        let url = closed();
        let group = group(&url);
        let deliver = |item: &str, id: &str| {
            key_of(item, id);
            answer(&handle_json(&request_for(
                group.path(),
                &url,
                item,
                id,
                json!([]),
            )))
        };
        let pending = |answer: &Value| -> BTreeSet<String> {
            answer["pending"]
                .as_array()
                .expect("pending")
                .iter()
                .filter_map(|id| id.as_str().map(str::to_owned))
                .collect()
        };
        assert_eq!(deliver(P, "p-1")["replaces"], json!([]));
        let second = deliver(P, "p-2");
        assert_eq!(
            (&second["replaces"], &second["awaits"]),
            (&json!(["p-1"]), &json!(["p-1"]))
        );
        // p-1 never reached the lock screen while p-2 waited: not reported.
        let third = deliver(P, "p-3");
        assert_eq!(third["replaces"], json!(["p-1", "p-2"]));
        assert_eq!(third["awaits"], json!(["p-2"]));
        let other = deliver(R, "r-1");
        assert_eq!(other["replaces"], json!([]));
        assert!(pending(&other).is_superset(&BTreeSet::from(["p-1".into(), "p-2".into()])));

        assert_eq!(
            answer(&handle_json(
                &json!({"hq_removed": ["p-1", "p-2"]}).to_string()
            )),
            json!({"kind": "reconciled"})
        );
        assert_eq!(deliver(P, "p-4")["replaces"], json!(["p-3"]));
        let next = deliver(R, "r-2");
        assert_eq!(next["replaces"], json!(["r-1"]));
        let left = pending(&next);
        assert!(left.contains("p-3"), "{left:?}");
        assert!(!left.contains("p-1") && !left.contains("p-2"), "{left:?}");
    }
}
