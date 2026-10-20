//! An account's open timeline views and the rooms its sliding sync
//! subscribes for them (102.6, R468, R509).
//!
//! A room is subscribed while any view of it is open and unsubscribed with
//! the last one; the subscription set is written only here, under the
//! registry's lock, so two changes never subscribe out of order. A view's
//! producer is spawned held and runs only once its view is registered: a
//! producer that ends at once (its sink already closed) then reaps what was
//! registered, instead of reaping nothing and leaving its room subscribed
//! behind a dead view.

use std::collections::{BTreeSet, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use matrix_sdk::ruma::{OwnedRoomId, RoomId};
use matrix_sdk_ui::sync_service::SyncService;
use matrix_sdk_ui::timeline::Timeline;
use tokio::sync::{oneshot, Mutex};
use tokio::task::JoinHandle;

use crate::timeline::{self, HistoryAsks};

/// One open view of a room's timeline: the exact `Arc<Timeline>` that
/// produced its items (send/retry/media reach it by room, AD-19) and where
/// the person's ask for older history reaches its producer.
pub(super) struct OpenView {
    pub(super) room: OwnedRoomId,
    pub(super) timeline: Arc<Timeline>,
    pub(super) asks: HistoryAsks,
}

/// Something open on a room.
pub(super) trait OnRoom {
    fn room(&self) -> &RoomId;
}

impl OnRoom for OpenView {
    fn room(&self) -> &RoomId {
        &self.room
    }
}

/// An account's open views, keyed by their timeline subscription id.
pub(super) type Views<V> = Arc<Mutex<HashMap<u64, V>>>;

/// An account's producer tasks, keyed by subscription id.
pub(super) type Tasks = Arc<Mutex<HashMap<u64, JoinHandle<()>>>>;

/// What subscribes exactly the given rooms in an account's sliding sync,
/// replacing the set it subscribed before.
pub(super) type RoomSubscriber =
    Arc<dyn Fn(BTreeSet<OwnedRoomId>) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

/// The account's sliding sync as its [`RoomSubscriber`].
pub(super) fn sliding_sync(sync: &Arc<SyncService>) -> RoomSubscriber {
    let sync = Arc::clone(sync);
    Arc::new(move |rooms| {
        let sync = Arc::clone(&sync);
        Box::pin(async move {
            timeline::subscribe_open_rooms(&sync.room_list_service(), &rooms).await;
        })
    })
}

/// Change `views` by `change`, and when the rooms they are open on changed
/// with it, subscribe exactly those rooms through `subscriber`.
pub(super) async fn change<V: OnRoom, R>(
    subscriber: &RoomSubscriber,
    views: &Views<V>,
    change: impl FnOnce(&mut HashMap<u64, V>) -> R,
) -> R {
    fn rooms<V: OnRoom>(open: &HashMap<u64, V>) -> BTreeSet<OwnedRoomId> {
        open.values().map(|view| view.room().to_owned()).collect()
    }
    let mut open = views.lock().await;
    let before = rooms(&open);
    let changed = change(&mut open);
    let after = rooms(&open);
    if after != before {
        subscriber(after).await;
    }
    changed
}

/// A view's producer task, spawned but held until its view is registered.
pub(super) struct Held {
    pub(super) task: JoinHandle<()>,
    start: oneshot::Sender<()>,
}

/// Spawn `producer` for view `id`, held: it runs once [`register`] has
/// registered the view, and when it ends it reaps its task and its view.
/// Dropped unregistered, it never runs.
pub(super) fn spawn_held<V: OnRoom + Send + 'static>(
    id: u64,
    subscriber: RoomSubscriber,
    tasks: Tasks,
    views: Views<V>,
    producer: impl Future<Output = ()> + Send + 'static,
) -> Held {
    let (start, started) = oneshot::channel();
    let task = tokio::spawn(async move {
        if started.await.is_err() {
            return;
        }
        producer.await;
        // A naturally-completed producer reaps its own task entry and its
        // view, unsubscribing the room with its last view (AD-19).
        tasks.lock().await.remove(&id);
        change(&subscriber, &views, |open| open.remove(&id)).await;
    });
    Held { task, start }
}

/// Register view `id` and its held producer, then let the producer run.
pub(super) async fn register<V: OnRoom>(
    subscriber: &RoomSubscriber,
    tasks: &Tasks,
    views: &Views<V>,
    id: u64,
    view: V,
    held: Held,
) {
    tasks.lock().await.insert(id, held.task);
    change(subscriber, views, |open| open.insert(id, view)).await;
    // The producer is waiting on this, unless its task was aborted.
    let _ = held.start.send(());
}

/// Close subscription `id`: its view (a room-list id is simply not there)
/// and its task. Returns whether a task was found and aborted.
pub(super) async fn close<V: OnRoom>(
    subscriber: &RoomSubscriber,
    tasks: &Tasks,
    views: &Views<V>,
    id: u64,
) -> bool {
    change(subscriber, views, |open| open.remove(&id)).await;
    match tasks.lock().await.remove(&id) {
        Some(task) => {
            task.abort();
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex as StdMutex;
    use std::time::Duration;

    use super::*;

    struct On(OwnedRoomId);

    impl OnRoom for On {
        fn room(&self) -> &RoomId {
            &self.0
        }
    }

    /// An account: its registries and every set its sliding sync was told.
    struct Account {
        told: Arc<StdMutex<Vec<BTreeSet<OwnedRoomId>>>>,
        subscriber: RoomSubscriber,
        tasks: Tasks,
        views: Views<On>,
    }

    impl Account {
        fn new() -> Account {
            let told: Arc<StdMutex<Vec<BTreeSet<OwnedRoomId>>>> = Arc::default();
            let record = Arc::clone(&told);
            Account {
                told,
                subscriber: Arc::new(move |rooms| {
                    record.lock().expect("told").push(rooms);
                    Box::pin(async {})
                }),
                tasks: Arc::default(),
                views: Arc::default(),
            }
        }

        /// The rooms the sliding sync holds subscribed now.
        fn subscribed(&self) -> BTreeSet<OwnedRoomId> {
            self.told
                .lock()
                .expect("told")
                .last()
                .cloned()
                .unwrap_or_default()
        }

        /// Open a view of `room` whose producer runs until `end` fires.
        async fn open(&self, id: u64, room: &str, end: oneshot::Receiver<()>) {
            let held = spawn_held(
                id,
                Arc::clone(&self.subscriber),
                Arc::clone(&self.tasks),
                Arc::clone(&self.views),
                async move {
                    let _ = end.await;
                },
            );
            // The producer may be scheduled before it is registered.
            tokio::time::sleep(Duration::from_millis(20)).await;
            register(
                &self.subscriber,
                &self.tasks,
                &self.views,
                id,
                On(room.try_into().expect("room")),
                held,
            )
            .await;
        }

        async fn settled(&self) {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
            while !self.tasks.lock().await.is_empty() || !self.views.lock().await.is_empty() {
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "a view never reaped itself"
                );
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }

    fn rooms(rooms: &[&str]) -> BTreeSet<OwnedRoomId> {
        rooms
            .iter()
            .map(|room| (*room).try_into().expect("room"))
            .collect()
    }

    /// R509/05: a view whose sink is already closed ends its producer at
    /// once; it ends only after its view is registered, and reaps it, so
    /// its room is not left subscribed behind a dead view.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_view_closed_at_once_leaves_no_room_subscribed() {
        let account = Account::new();
        let (end, ended) = oneshot::channel();
        drop(end);
        account.open(1, "!a:h", ended).await;
        account.settled().await;
        assert_eq!(account.subscribed(), rooms(&[]), "told: {:?}", account.told);
    }

    /// Two views of one room keep it subscribed until the last one closes,
    /// whether a producer ends on its own or the view is closed.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_room_stays_subscribed_until_its_last_view_closes() {
        let account = Account::new();
        let (end_one, one) = oneshot::channel();
        let (_end_two, two) = oneshot::channel();
        account.open(1, "!a:h", one).await;
        account.open(2, "!a:h", two).await;
        assert_eq!(account.subscribed(), rooms(&["!a:h"]));

        end_one.send(()).expect("producer one");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while account.views.lock().await.len() > 1 {
            assert!(
                tokio::time::Instant::now() < deadline,
                "view one never ended"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(account.subscribed(), rooms(&["!a:h"]), "one view is open");

        assert!(close(&account.subscriber, &account.tasks, &account.views, 2).await);
        assert_eq!(account.subscribed(), rooms(&[]), "the last view closed");
        account.settled().await;
    }

    /// Accounts subscribe their own rooms only: closing one account's last
    /// view leaves the other's room subscribed.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn each_account_subscribes_its_own_open_rooms() {
        let (first, second) = (Account::new(), Account::new());
        let (_end_a, a) = oneshot::channel();
        let (_end_b, b) = oneshot::channel();
        first.open(1, "!a:h", a).await;
        second.open(1, "!b:h", b).await;
        assert!(close(&first.subscriber, &first.tasks, &first.views, 1).await);
        assert_eq!(first.subscribed(), rooms(&[]));
        assert_eq!(second.subscribed(), rooms(&["!b:h"]));
    }
}
