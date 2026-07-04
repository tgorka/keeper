//! Single-account supervision and the sliding-sync room-list producer (AD-8,
//! AD-9, AD-19, AD-20).
//!
//! The [`AccountManager`] is a single-account-capable holder keyed by
//! `account_id`. A room-list subscribe lazily *activates* the account — it
//! restores the persisted `MatrixSession` from the Keychain, rebuilds the
//! `Client` against its existing SQLite store, and starts a `SyncService` +
//! `RoomListService` under a supervised tokio task (this is also the Story 1.8
//! cold-start restore path). Activation is idempotent: a second subscribe reuses
//! the live account, never a second `Client`/`SyncService`.
//!
//! Ordering is owned entirely by the SDK. `entries_with_dynamic_adapters`
//! yields a recency-sorted `VectorDiff` sequence; keeper converts each item to a
//! [`RoomVm`] and forwards the diff verbatim as a [`RoomListOp`] — nothing is
//! sorted here or in TypeScript (AD-20). No token, session, or message
//! plaintext beyond the rendered preview crosses IPC or reaches a `tracing` log
//! (NFR-9).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use futures_util::StreamExt;
use matrix_sdk::encryption::VerificationState;
use matrix_sdk::ruma::events::{
    AnySyncMessageLikeEvent, AnySyncTimelineEvent, SyncMessageLikeEvent,
};
use matrix_sdk::ruma::{OwnedRoomId, RoomId};
use matrix_sdk::Client;
use matrix_sdk_ui::eyeball_im::VectorDiff;
use matrix_sdk_ui::room_list_service::filters::new_filter_non_left;
use matrix_sdk_ui::room_list_service::{RoomList, RoomListItem, RoomListLoadingState};
use matrix_sdk_ui::sync_service::{State, SyncService};
use matrix_sdk_ui::timeline::Timeline;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use tracing::Instrument;

use crate::auth::{self, session_keychain_key};
use crate::backup::{self, BackupSink};
use crate::error::{AccountError, BackupError, CoreError, InboxError, SendError, TimelineError};
use crate::inbox::InboxMerger;
use crate::platform::Platform;
use crate::registry;
use crate::send::{self, SendTrigger};
use crate::timeline;
use crate::verification::{self, VerificationSink};
use crate::vm::{
    ConnectionStatus, ConnectionStatusBatch, EncryptionStatus, EncryptionStatusBatch, InboxBatch,
    RoomListBatch, RoomListOp, RoomVm, TimelineBatch,
};

/// Number of rooms in the initial fixed window (seeded windowing per AD-20).
const ROOM_LIST_PAGE_SIZE: usize = 200;

/// Defensive upper bound on a rendered message preview before it crosses IPC.
const MAX_PREVIEW_CHARS: usize = 256;

/// Sink that receives each produced [`RoomListBatch`]. The shell wraps a Tauri
/// `Channel::send`; tests capture into a vector. Returns `true` if the batch was
/// delivered, `false` if the channel is closed (the producer then stops).
pub type BatchSink = Box<dyn Fn(RoomListBatch) -> bool + Send + Sync>;

/// Sink that receives each produced [`TimelineBatch`]. The shell wraps a Tauri
/// `Channel::send`; tests capture into a vector. Returns `true` if the batch was
/// delivered, `false` if the channel is closed (the producer then stops).
pub type TimelineSink = Box<dyn Fn(TimelineBatch) -> bool + Send + Sync>;

/// Sink that receives each produced [`ConnectionStatusBatch`]. The shell wraps a
/// Tauri `Channel::send`; tests capture into a vector. Returns `true` if the
/// batch was delivered, `false` if the channel is closed (the producer stops).
pub type ConnectionSink = Box<dyn Fn(ConnectionStatusBatch) -> bool + Send + Sync>;

/// Sink that receives each produced [`EncryptionStatusBatch`]. The shell wraps a
/// Tauri `Channel::send`; tests capture into a vector. Returns `true` if the batch
/// was delivered, `false` if the channel is closed (the producer then stops).
pub type EncryptionSink = Box<dyn Fn(EncryptionStatusBatch) -> bool + Send + Sync>;

/// Sink that receives each merged [`InboxBatch`]. The shell wraps a Tauri
/// `Channel::send`; tests capture into a vector. Returns `true` if the batch was
/// delivered, `false` if the channel is closed (the merger stops emitting).
pub type InboxSink = Box<dyn Fn(InboxBatch) -> bool + Send + Sync>;

/// Registry of an account's live open room timelines, keyed by the *timeline*
/// subscription id → its room id and the exact `Arc<Timeline>` that produced the
/// subscribed items (AD-19). Send/retry look it up by room id; teardown drops the
/// entry.
type OpenTimelines = Arc<Mutex<HashMap<u64, (OwnedRoomId, Arc<Timeline>)>>>;

/// The live artifacts produced by [`activate`]: the `Client`, its `SyncService`,
/// and the two lifetime-of-account supervisor task handles (reconnect supervisor
/// and session persister) that the caller stores on the [`AccountHandle`].
type ActivatedAccount = (
    Client,
    std::sync::Arc<SyncService>,
    JoinHandle<()>,
    JoinHandle<()>,
);

/// A live, supervised account: its `Client`, `SyncService`, and the abort
/// handles of its room-list subscriptions.
struct AccountHandle {
    client: Client,
    sync: std::sync::Arc<SyncService>,
    /// Live subscriptions, keyed by subscription id → the producer task.
    subscriptions: Arc<Mutex<HashMap<u64, JoinHandle<()>>>>,
    /// Live open room timelines, keyed by the *timeline* subscription id → its
    /// room id and the exact `Arc<Timeline>` that produced the subscribed items.
    /// Reachable by `send_text`/`retry_send` (looked up by room id) and dropped
    /// on the same teardown paths as its subscription (unsubscribe + natural
    /// completion), so no `Timeline` (and its SDK tasks) leaks (AD-19). The
    /// room-list subscription registers nothing here.
    timelines: OpenTimelines,
    /// Flow-id sender into the account's live verification producer (Story 3.2),
    /// present iff a verification subscription is active. `verification_start`
    /// requests a self-verification and forwards its new flow id here so the
    /// producer picks it up and drives it. Set on subscribe, cleared on
    /// unsubscribe/shutdown.
    verification_flow_tx: Arc<Mutex<Option<mpsc::UnboundedSender<String>>>>,
    /// Lifetime-of-account reconnect supervisor task (not tied to any UI
    /// subscription). Observes `sync.state()` and, on every transition into
    /// `Running`, calls `client.send_queue().set_enabled(true)` so a room queue
    /// that a recoverable error disabled is re-enabled and persisted unsent
    /// requests are respawned — the load-bearing "dispatch on reconnect"
    /// mechanism. Aborted in [`AccountManager::shutdown`] and on partial
    /// activation teardown.
    reconnect_supervisor: JoinHandle<()>,
    /// Lifetime-of-account session-persister task (Story 2.2): re-persists the
    /// Keychain blob when the SDK rotates OAuth tokens. It holds its own `Client`
    /// clone, so it MUST be aborted in [`AccountManager::shutdown`] before
    /// `sign_out_cleanup` runs — otherwise it keeps the store's SQLite handles
    /// open past store-dir deletion and could re-persist the just-deleted
    /// Keychain key on a late token refresh (resurrecting a signed-out secret).
    session_persister: JoinHandle<()>,
}

/// A live merged-inbox subscription (AD-20): the merger the per-account
/// room-list producers feed, and their abort handles keyed by account id (so a
/// single account's producer can be torn down on that account's sign-out, not
/// only on a whole-inbox unsubscribe). There is at most one at a time in the
/// shell.
struct InboxHandle {
    subscription_id: u64,
    merger: InboxMerger,
    producers: HashMap<String, JoinHandle<()>>,
}

/// Multi-account supervisor (AD-3, AD-19, AD-20). Owns the live per-account
/// state (each a supervised `Client`/`SyncService`) and the single active
/// merged-inbox subscription; the shell manages exactly one instance in its
/// `AppState`. No account-count limit is enforced anywhere.
#[derive(Default)]
pub struct AccountManager {
    accounts: Mutex<HashMap<String, AccountHandle>>,
    /// The single active merged-inbox subscription, if any. Sign-out/shutdown
    /// notify it so a removed account's rooms leave the inbox immediately.
    inbox: Mutex<Option<InboxHandle>>,
}

/// Monotonic source of subscription ids handed back to the frontend.
static NEXT_SUBSCRIPTION_ID: AtomicU64 = AtomicU64::new(1);

impl AccountManager {
    /// Construct an empty manager with no live accounts.
    pub fn new() -> Self {
        Self::default()
    }

    /// Subscribe to the account's room list, activating the account if it is not
    /// already live. Spawns a supervised producer task that emits a `Reset`
    /// snapshot batch first, then diff batches, into `sink`. Returns the new
    /// subscription id.
    pub async fn subscribe_room_list(
        &self,
        platform: &Arc<dyn Platform>,
        account_id: &str,
        sink: BatchSink,
    ) -> Result<u64, CoreError> {
        // Activate under the manager lock so a concurrent subscribe cannot build
        // a second Client/SyncService for the same account. `did_activate`
        // records whether *this* call brought the account live, so a failure
        // before any subscription attaches can tear it back down.
        let (client, sync, subs_arc, did_activate) = {
            let mut accounts = self.accounts.lock().await;
            let did_activate = !accounts.contains_key(account_id);
            if did_activate {
                let (client, sync, reconnect_supervisor, session_persister) =
                    activate(platform, account_id).await?;
                accounts.insert(
                    account_id.to_owned(),
                    AccountHandle {
                        client,
                        sync,
                        subscriptions: Arc::new(Mutex::new(HashMap::new())),
                        timelines: Arc::new(Mutex::new(HashMap::new())),
                        reconnect_supervisor,
                        session_persister,
                        verification_flow_tx: Arc::new(Mutex::new(None)),
                    },
                );
                tracing::info!(account_id = %account_id, "account activated for room list");
            }
            let handle = accounts.get(account_id).ok_or_else(|| {
                CoreError::Internal("account handle vanished after activation".to_owned())
            })?;
            (
                handle.client.clone(),
                handle.sync.clone(),
                handle.subscriptions.clone(),
                did_activate,
            )
        };

        let room_list = match sync.room_list_service().all_rooms().await {
            Ok(room_list) => room_list,
            Err(e) => {
                // Don't leave a partial live account running (AD-21: "no partial
                // live account is retained"). If this call just activated the
                // account and nothing else has attached a subscription yet, stop
                // its SyncService and drop the handle. The emptiness guard keeps
                // a racing sibling subscribe's live subscription intact.
                if did_activate {
                    let mut accounts = self.accounts.lock().await;
                    let should_remove = match accounts.get(account_id) {
                        Some(handle) => handle.subscriptions.lock().await.is_empty(),
                        None => false,
                    };
                    if should_remove {
                        if let Some(dead) = accounts.remove(account_id) {
                            dead.reconnect_supervisor.abort();
                            dead.session_persister.abort();
                            dead.sync.stop().await;
                            tracing::info!(
                                account_id = %account_id,
                                "torn down partial account after room-list start failure"
                            );
                        }
                    }
                }
                return Err(AccountError::SyncStart(e.to_string()).into());
            }
        };

        let subscription_id = NEXT_SUBSCRIPTION_ID.fetch_add(1, Ordering::Relaxed);
        let account_id_owned = account_id.to_owned();
        let span = tracing::info_span!("room_list_producer", account_id = %account_id);
        let reaper_subs = subs_arc.clone();
        let task = tokio::spawn(
            async move {
                // `client` is captured to keep the account alive for the task's
                // lifetime; the producer reads from the room list only.
                let _keep_alive = client;
                run_producer(room_list, sink, &account_id_owned).await;
                // A naturally-completed producer reaps its own subscription entry.
                reaper_subs.lock().await.remove(&subscription_id);
            }
            .instrument(span),
        );

        {
            let accounts = self.accounts.lock().await;
            match accounts.get(account_id) {
                Some(_) => {
                    subs_arc.lock().await.insert(subscription_id, task);
                }
                None => {
                    task.abort();
                    return Err(AccountError::SyncStart(
                        "account removed during subscribe".to_owned(),
                    )
                    .into());
                }
            }
        }
        tracing::info!(account_id = %account_id, subscription_id, "room list subscribed");
        Ok(subscription_id)
    }

    /// Subscribe to the merged unified inbox across every restorable account
    /// (AD-20). Activates each account whose Keychain session is present, opens
    /// its room-list stream, and feeds each into a shared [`InboxMerger`] that
    /// emits one recency-ordered [`InboxBatch`] stream into `sink`. Returns the
    /// inbox subscription id. Replacing an existing inbox subscription (e.g. the
    /// frontend re-subscribes after adding an account) first tears the old one
    /// down. Adding the Nth account is identical to the 2nd — no count limit.
    pub async fn subscribe_inbox(
        &self,
        platform: &Arc<dyn Platform>,
        sink: InboxSink,
    ) -> Result<u64, CoreError> {
        // Only one inbox subscription at a time: tear down any prior one so its
        // producers stop feeding a stale merger/channel.
        self.unsubscribe_inbox_inner().await;

        let accounts = auth::find_restorable_accounts(platform.as_ref())?;
        let merger = InboxMerger::new(sink);
        let subscription_id = NEXT_SUBSCRIPTION_ID.fetch_add(1, Ordering::Relaxed);

        // Register every account slot up front so the merge reflects the full set
        // even before any batch arrives, then start each account's producer.
        let mut producers: HashMap<String, JoinHandle<()>> = HashMap::with_capacity(accounts.len());
        for account in &accounts {
            merger
                .register_account(&account.account_id, account.hue_index)
                .await;
        }
        for account in accounts {
            let account_id = account.account_id.clone();
            // Activate + acquire the room list. A single account's activation
            // failure is not fatal to the whole inbox: skip it (its rooms simply
            // don't appear) so the other accounts keep syncing.
            let room_list = match self.acquire_room_list(platform, &account_id).await {
                Ok(room_list) => room_list,
                Err(e) => {
                    tracing::warn!(
                        account_id = %account_id,
                        error = %e,
                        "inbox: account could not start; skipping (others keep syncing)"
                    );
                    merger.remove_account(&account_id).await;
                    continue;
                }
            };
            let merger_for_task = merger.clone();
            let task = tokio::spawn(
                async move {
                    run_inbox_producer(room_list, merger_for_task, &account_id).await;
                }
                .instrument(
                    tracing::info_span!("inbox_producer", account_id = %account.account_id),
                ),
            );
            producers.insert(account.account_id.clone(), task);
        }

        {
            let mut inbox = self.inbox.lock().await;
            *inbox = Some(InboxHandle {
                subscription_id,
                merger,
                producers,
            });
        }
        tracing::info!(subscription_id, "merged inbox subscribed");
        Ok(subscription_id)
    }

    /// Unsubscribe the merged inbox for `subscription_id`, aborting every
    /// per-account producer. Idempotent — a mismatched/unknown id is a no-op.
    pub async fn unsubscribe_inbox(&self, subscription_id: u64) {
        let matches = {
            let inbox = self.inbox.lock().await;
            inbox
                .as_ref()
                .is_some_and(|h| h.subscription_id == subscription_id)
        };
        if matches {
            self.unsubscribe_inbox_inner().await;
            tracing::info!(subscription_id, "merged inbox unsubscribed");
        }
    }

    /// Tear down any active inbox subscription: abort its producers and drop the
    /// handle. Idempotent.
    async fn unsubscribe_inbox_inner(&self) {
        let handle = self.inbox.lock().await.take();
        if let Some(handle) = handle {
            for (_, task) in handle.producers {
                task.abort();
            }
        }
    }

    /// Activate `account_id` if needed and return its recency-sorted room list.
    /// Mirrors the activation/teardown contract of [`subscribe_room_list`]
    /// (partial-account cleanup on a room-list start failure) but hands the
    /// `RoomList` back to the caller rather than spawning a producer.
    async fn acquire_room_list(
        &self,
        platform: &Arc<dyn Platform>,
        account_id: &str,
    ) -> Result<RoomList, CoreError> {
        let (sync, did_activate) = {
            let mut accounts = self.accounts.lock().await;
            let did_activate = !accounts.contains_key(account_id);
            if did_activate {
                let (client, sync, reconnect_supervisor, session_persister) =
                    activate(platform, account_id).await?;
                accounts.insert(
                    account_id.to_owned(),
                    AccountHandle {
                        client,
                        sync,
                        subscriptions: Arc::new(Mutex::new(HashMap::new())),
                        timelines: Arc::new(Mutex::new(HashMap::new())),
                        reconnect_supervisor,
                        session_persister,
                        verification_flow_tx: Arc::new(Mutex::new(None)),
                    },
                );
                tracing::info!(account_id = %account_id, "account activated for inbox");
            }
            let handle = accounts.get(account_id).ok_or_else(|| {
                CoreError::Internal("account handle vanished after activation".to_owned())
            })?;
            (handle.sync.clone(), did_activate)
        };

        match sync.room_list_service().all_rooms().await {
            Ok(room_list) => Ok(room_list),
            Err(e) => {
                if did_activate {
                    let mut accounts = self.accounts.lock().await;
                    let should_remove = match accounts.get(account_id) {
                        Some(handle) => handle.subscriptions.lock().await.is_empty(),
                        None => false,
                    };
                    if should_remove {
                        if let Some(dead) = accounts.remove(account_id) {
                            dead.reconnect_supervisor.abort();
                            dead.session_persister.abort();
                            dead.sync.stop().await;
                            tracing::info!(
                                account_id = %account_id,
                                "torn down partial account after inbox room-list start failure"
                            );
                        }
                    }
                }
                Err(InboxError::StreamStart(e.to_string()).into())
            }
        }
    }

    /// Subscribe to a room's timeline, activating the account if it is not
    /// already live (reusing the same `Client`/`SyncService` as the room list —
    /// never a second one). Spawns a supervised producer that emits a `Reset`
    /// snapshot first, then diff batches, into `sink`. Returns the new
    /// subscription id.
    pub async fn subscribe_timeline(
        &self,
        platform: &Arc<dyn Platform>,
        account_id: &str,
        room_id: &str,
        sink: TimelineSink,
    ) -> Result<u64, CoreError> {
        let room_id: OwnedRoomId =
            RoomId::parse(room_id).map_err(|_| TimelineError::RoomNotFound)?;

        // Reuse the live handle under the manager lock, activating idempotently
        // if needed — never a second Client/SyncService. `did_activate` records
        // whether *this* call brought the account live, so a build failure before
        // any subscription attaches can tear it back down (AD-21).
        let (client, subs_arc, timelines_arc, did_activate) = {
            let mut accounts = self.accounts.lock().await;
            let did_activate = !accounts.contains_key(account_id);
            if did_activate {
                let (client, sync, reconnect_supervisor, session_persister) =
                    activate(platform, account_id).await?;
                accounts.insert(
                    account_id.to_owned(),
                    AccountHandle {
                        client,
                        sync,
                        subscriptions: Arc::new(Mutex::new(HashMap::new())),
                        timelines: Arc::new(Mutex::new(HashMap::new())),
                        reconnect_supervisor,
                        session_persister,
                        verification_flow_tx: Arc::new(Mutex::new(None)),
                    },
                );
                tracing::info!(account_id = %account_id, "account activated for timeline");
            }
            let handle = accounts.get(account_id).ok_or_else(|| {
                CoreError::Internal("account handle vanished after activation".to_owned())
            })?;
            (
                handle.client.clone(),
                handle.subscriptions.clone(),
                handle.timelines.clone(),
                did_activate,
            )
        };

        // Build the timeline *synchronously* so a missing room / build failure
        // surfaces to the caller as `TimelineUnavailable` — an honest inline
        // error, not a silent spinner (AC-4) — rather than being buried in the
        // spawned task. Only the diff-forwarding loop runs in the background.
        let open = match timeline::open_timeline(&client, &room_id).await {
            Ok(open) => open,
            Err(e) => {
                // Don't leave a partial live account running (AD-21). If this call
                // just activated the account and nothing else has attached a
                // subscription yet, stop its SyncService and drop the handle. The
                // emptiness guard keeps a racing sibling subscribe's live
                // subscription intact.
                if did_activate {
                    let mut accounts = self.accounts.lock().await;
                    let should_remove = match accounts.get(account_id) {
                        Some(handle) => handle.subscriptions.lock().await.is_empty(),
                        None => false,
                    };
                    if should_remove {
                        if let Some(dead) = accounts.remove(account_id) {
                            dead.reconnect_supervisor.abort();
                            dead.session_persister.abort();
                            dead.sync.stop().await;
                            tracing::info!(
                                account_id = %account_id,
                                "torn down partial account after timeline build failure"
                            );
                        }
                    }
                }
                return Err(e.into());
            }
        };

        let subscription_id = NEXT_SUBSCRIPTION_ID.fetch_add(1, Ordering::Relaxed);
        // Share the exact `Arc<Timeline>` that produces the subscribed items with
        // send/retry — `unique_id`s are only stable within this one instance.
        let shared_timeline = open.timeline();
        let reaper_subs = subs_arc.clone();
        let reaper_timelines = timelines_arc.clone();
        let room_id_task = room_id.clone();
        let room_id_log = room_id.clone();
        let span =
            tracing::info_span!("timeline_producer", account_id = %account_id, room_id = %room_id);
        let task = tokio::spawn(
            async move {
                timeline::forward_timeline(open, room_id_task, sink).await;
                // A naturally-completed producer reaps its own subscription entry
                // and drops its stored `Arc<Timeline>` so nothing leaks (AD-19).
                reaper_subs.lock().await.remove(&subscription_id);
                reaper_timelines.lock().await.remove(&subscription_id);
            }
            .instrument(span),
        );

        {
            let accounts = self.accounts.lock().await;
            match accounts.get(account_id) {
                Some(_) => {
                    subs_arc.lock().await.insert(subscription_id, task);
                    timelines_arc
                        .lock()
                        .await
                        .insert(subscription_id, (room_id.clone(), shared_timeline));
                }
                None => {
                    // The account was shut down in the spawn→register gap;
                    // shutdown already stopped its sync, so aborting the orphaned
                    // producer here leaks nothing.
                    task.abort();
                    return Err(TimelineError::Build(
                        "account removed during subscribe".to_owned(),
                    )
                    .into());
                }
            }
        }
        tracing::info!(account_id = %account_id, subscription_id, room_id = %room_id_log, "timeline subscribed");
        Ok(subscription_id)
    }

    /// Subscribe to the account's connection status, activating the account if
    /// it is not already live (reusing the same `Client`/`SyncService` — never a
    /// second one). Spawns a supervised producer over `sync.state()` that emits
    /// the current mapped [`ConnectionStatus`] as an initial snapshot, then a
    /// deduped diff on every change, into `sink`. Returns the subscription id.
    ///
    /// Mirrors [`subscribe_room_list`]'s lazy-activation + supervised-task +
    /// self-reap lifecycle: the `JoinHandle` is registered in the subscriptions
    /// map and aborted on unsubscribe / shutdown. An activation failure maps to
    /// the existing `SyncUnavailable` code (retriable).
    pub async fn subscribe_connection_status(
        &self,
        platform: &Arc<dyn Platform>,
        account_id: &str,
        sink: ConnectionSink,
    ) -> Result<u64, CoreError> {
        // Activate under the manager lock so a concurrent subscribe cannot build
        // a second Client/SyncService for the same account. `did_activate`
        // records whether *this* call brought the account live, so a failure
        // before any subscription attaches can tear it back down (AD-21).
        let (client, sync, subs_arc, did_activate) = {
            let mut accounts = self.accounts.lock().await;
            let did_activate = !accounts.contains_key(account_id);
            if did_activate {
                let (client, sync, reconnect_supervisor, session_persister) =
                    activate(platform, account_id).await?;
                accounts.insert(
                    account_id.to_owned(),
                    AccountHandle {
                        client,
                        sync,
                        subscriptions: Arc::new(Mutex::new(HashMap::new())),
                        timelines: Arc::new(Mutex::new(HashMap::new())),
                        reconnect_supervisor,
                        session_persister,
                        verification_flow_tx: Arc::new(Mutex::new(None)),
                    },
                );
                tracing::info!(account_id = %account_id, "account activated for connection status");
            }
            let handle = accounts.get(account_id).ok_or_else(|| {
                CoreError::Internal("account handle vanished after activation".to_owned())
            })?;
            (
                handle.client.clone(),
                handle.sync.clone(),
                handle.subscriptions.clone(),
                did_activate,
            )
        };

        let subscription_id = NEXT_SUBSCRIPTION_ID.fetch_add(1, Ordering::Relaxed);
        let account_id_owned = account_id.to_owned();
        let span = tracing::info_span!("connection_status_producer", account_id = %account_id);
        let reaper_subs = subs_arc.clone();
        let task = tokio::spawn(
            async move {
                // `client` is captured to keep the account alive for the task's
                // lifetime; the producer reads connectivity from `sync` only.
                let _keep_alive = client;
                run_connection_producer(sync, sink, &account_id_owned).await;
                // A naturally-completed producer reaps its own subscription entry.
                reaper_subs.lock().await.remove(&subscription_id);
            }
            .instrument(span),
        );

        {
            let accounts = self.accounts.lock().await;
            match accounts.get(account_id) {
                Some(_) => {
                    subs_arc.lock().await.insert(subscription_id, task);
                }
                None => {
                    // The account was shut down in the spawn→register gap;
                    // aborting the orphaned producer here leaks nothing.
                    task.abort();
                    if did_activate {
                        return Err(AccountError::SyncStart(
                            "account removed during subscribe".to_owned(),
                        )
                        .into());
                    }
                }
            }
        }
        tracing::info!(account_id = %account_id, subscription_id, "connection status subscribed");
        Ok(subscription_id)
    }

    /// Abort exactly the connection-status producer task for `subscription_id`;
    /// other account state is untouched. Idempotent.
    pub async fn unsubscribe_connection_status(&self, account_id: &str, subscription_id: u64) {
        if self.abort_subscription(account_id, subscription_id).await {
            tracing::info!(account_id = %account_id, subscription_id, "connection status unsubscribed");
        }
    }

    /// Subscribe to the account's encryption (device-verification) status,
    /// activating the account if it is not already live (reusing the same
    /// `Client`/`SyncService` — never a second one). Spawns a supervised producer
    /// over `client.encryption().verification_state()` that emits the current
    /// mapped [`EncryptionStatus`] as an initial snapshot, then a deduped diff on
    /// every change, into `sink`. Returns the subscription id.
    ///
    /// Mirrors [`subscribe_connection_status`]'s lazy-activation +
    /// supervised-task + self-reap lifecycle: the `JoinHandle` is registered in
    /// the subscriptions map and aborted on unsubscribe / shutdown. An activation
    /// failure maps to the existing `SyncUnavailable` code (retriable).
    pub async fn subscribe_encryption_status(
        &self,
        platform: &Arc<dyn Platform>,
        account_id: &str,
        sink: EncryptionSink,
    ) -> Result<u64, CoreError> {
        // Activate under the manager lock so a concurrent subscribe cannot build
        // a second Client/SyncService for the same account. `did_activate`
        // records whether *this* call brought the account live, so a failure
        // before any subscription attaches can tear it back down (AD-21).
        let (client, subs_arc, did_activate) = {
            let mut accounts = self.accounts.lock().await;
            let did_activate = !accounts.contains_key(account_id);
            if did_activate {
                let (client, sync, reconnect_supervisor, session_persister) =
                    activate(platform, account_id).await?;
                accounts.insert(
                    account_id.to_owned(),
                    AccountHandle {
                        client,
                        sync,
                        subscriptions: Arc::new(Mutex::new(HashMap::new())),
                        timelines: Arc::new(Mutex::new(HashMap::new())),
                        reconnect_supervisor,
                        session_persister,
                        verification_flow_tx: Arc::new(Mutex::new(None)),
                    },
                );
                tracing::info!(account_id = %account_id, "account activated for encryption status");
            }
            let handle = accounts.get(account_id).ok_or_else(|| {
                CoreError::Internal("account handle vanished after activation".to_owned())
            })?;
            (
                handle.client.clone(),
                handle.subscriptions.clone(),
                did_activate,
            )
        };

        let subscription_id = NEXT_SUBSCRIPTION_ID.fetch_add(1, Ordering::Relaxed);
        let account_id_owned = account_id.to_owned();
        let span = tracing::info_span!("encryption_status_producer", account_id = %account_id);
        let reaper_subs = subs_arc.clone();
        let task = tokio::spawn(
            async move {
                run_encryption_status_producer(&client, sink, &account_id_owned).await;
                // `client` is kept alive for the whole producer via `run_...`'s
                // borrow; the account also holds its own clone.
                let _keep_alive = client;
                // A naturally-completed producer reaps its own subscription entry.
                reaper_subs.lock().await.remove(&subscription_id);
            }
            .instrument(span),
        );

        {
            let accounts = self.accounts.lock().await;
            match accounts.get(account_id) {
                Some(_) => {
                    subs_arc.lock().await.insert(subscription_id, task);
                }
                None => {
                    // The account was shut down in the spawn→register gap;
                    // aborting the orphaned producer here leaks nothing.
                    task.abort();
                    if did_activate {
                        return Err(AccountError::SyncStart(
                            "account removed during subscribe".to_owned(),
                        )
                        .into());
                    }
                }
            }
        }
        tracing::info!(account_id = %account_id, subscription_id, "encryption status subscribed");
        Ok(subscription_id)
    }

    /// Abort exactly the encryption-status producer task for `subscription_id`;
    /// other account state is untouched. Idempotent.
    pub async fn unsubscribe_encryption_status(&self, account_id: &str, subscription_id: u64) {
        if self.abort_subscription(account_id, subscription_id).await {
            tracing::info!(account_id = %account_id, subscription_id, "encryption status unsubscribed");
        }
    }

    /// Subscribe to the account's server-side key-backup status (Story 3.3),
    /// activating the account if it is not already live (reusing the same
    /// `Client` — never a second one). Spawns a supervised
    /// [`backup::run_status_producer`] over `recovery().state_stream()` that emits
    /// the current mapped [`BackupStatus`] as an initial snapshot, then a deduped
    /// diff on every change, into `sink`. Returns the subscription id.
    ///
    /// Mirrors [`subscribe_encryption_status`]'s lazy-activation, supervised-task,
    /// and self-reap lifecycle: the `JoinHandle` is registered in the
    /// subscriptions map and aborted on unsubscribe / shutdown. An activation
    /// failure maps to the existing `SyncUnavailable` code (retriable).
    pub async fn subscribe_backup_status(
        &self,
        platform: &Arc<dyn Platform>,
        account_id: &str,
        sink: BackupSink,
    ) -> Result<u64, CoreError> {
        // Activate under the manager lock so a concurrent subscribe cannot build
        // a second Client/SyncService for the same account. `did_activate` records
        // whether *this* call brought the account live, so a failure before any
        // subscription attaches can tear it back down (AD-21).
        let (client, subs_arc, did_activate) = {
            let mut accounts = self.accounts.lock().await;
            let did_activate = !accounts.contains_key(account_id);
            if did_activate {
                let (client, sync, reconnect_supervisor, session_persister) =
                    activate(platform, account_id).await?;
                accounts.insert(
                    account_id.to_owned(),
                    AccountHandle {
                        client,
                        sync,
                        subscriptions: Arc::new(Mutex::new(HashMap::new())),
                        timelines: Arc::new(Mutex::new(HashMap::new())),
                        reconnect_supervisor,
                        session_persister,
                        verification_flow_tx: Arc::new(Mutex::new(None)),
                    },
                );
                tracing::info!(account_id = %account_id, "account activated for backup status");
            }
            let handle = accounts.get(account_id).ok_or_else(|| {
                CoreError::Internal("account handle vanished after activation".to_owned())
            })?;
            (
                handle.client.clone(),
                handle.subscriptions.clone(),
                did_activate,
            )
        };

        let subscription_id = NEXT_SUBSCRIPTION_ID.fetch_add(1, Ordering::Relaxed);
        let account_id_owned = account_id.to_owned();
        let span = tracing::info_span!("backup_status_producer", account_id = %account_id);
        let reaper_subs = subs_arc.clone();
        let task = tokio::spawn(
            async move {
                backup::run_status_producer(client, sink, &account_id_owned).await;
                // A naturally-completed producer reaps its own subscription entry.
                reaper_subs.lock().await.remove(&subscription_id);
            }
            .instrument(span),
        );

        {
            let accounts = self.accounts.lock().await;
            match accounts.get(account_id) {
                Some(_) => {
                    subs_arc.lock().await.insert(subscription_id, task);
                }
                None => {
                    // The account was shut down in the spawn→register gap;
                    // aborting the orphaned producer here leaks nothing.
                    task.abort();
                    if did_activate {
                        return Err(AccountError::SyncStart(
                            "account removed during subscribe".to_owned(),
                        )
                        .into());
                    }
                }
            }
        }
        tracing::info!(account_id = %account_id, subscription_id, "backup status subscribed");
        Ok(subscription_id)
    }

    /// Abort exactly the backup-status producer task for `subscription_id`; other
    /// account state is untouched. Idempotent.
    pub async fn unsubscribe_backup_status(&self, account_id: &str, subscription_id: u64) {
        if self.abort_subscription(account_id, subscription_id).await {
            tracing::info!(account_id = %account_id, subscription_id, "backup status unsubscribed");
        }
    }

    /// Enable server-side key backup for the account (Story 3.3, FR-14). Resolves
    /// the live `Client` and delegates to [`backup::enable`], returning the base58
    /// recovery key *once* (the deliberate boundary exception — shown once to the
    /// human). A race with an existing server backup surfaces as the named
    /// `AlreadyExistsOnServer`. The recovery key is never logged.
    pub async fn backup_enable(&self, account_id: &str) -> Result<String, CoreError> {
        let client = self.client_for_backup(account_id).await?;
        backup::enable(&client).await
    }

    /// Restore from server-side key backup with a recovery key (Story 3.3,
    /// FR-14). Resolves the live `Client` and delegates to [`backup::restore`];
    /// the SDK downloads room keys automatically afterward, so 3.1's streams
    /// re-render UTD rows for free. Invalid keys surface as named errors
    /// (malformed vs incorrect). The recovery key is never logged.
    pub async fn backup_restore(
        &self,
        account_id: &str,
        recovery_key: &str,
    ) -> Result<(), CoreError> {
        let client = self.client_for_backup(account_id).await?;
        backup::restore(&client, recovery_key).await
    }

    /// Save a recovery key to the OS Keychain at `recovery_key/<account_id>`
    /// (Story 3.3, FR-14) — the user's opt-in after seeing the key once. The key
    /// never touches `tracing`; a Keychain write failure surfaces so the modal can
    /// keep the key visible for manual copy.
    pub async fn backup_save_recovery_key(
        &self,
        platform: &Arc<dyn Platform>,
        account_id: &str,
        recovery_key: &str,
    ) -> Result<(), CoreError> {
        platform.keychain_set(&recovery_key_keychain_key(account_id), recovery_key)?;
        tracing::info!(account_id = %account_id, "recovery key saved to keychain");
        Ok(())
    }

    /// Read a previously-saved recovery key from the OS Keychain at
    /// `recovery_key/<account_id>` (Story 3.3) to prefill the restore textarea,
    /// or `None` if none was saved. The key never touches `tracing`.
    pub async fn backup_saved_recovery_key(
        &self,
        platform: &Arc<dyn Platform>,
        account_id: &str,
    ) -> Result<Option<String>, CoreError> {
        platform.keychain_get(&recovery_key_keychain_key(account_id))
    }

    /// Subscribe to the account's interactive self-verification flow (Story 3.2),
    /// activating the account if it is not already live (reusing the same
    /// `Client` — never a second one). Spawns a supervised
    /// [`verification::run_producer`] that observes incoming self-verification
    /// requests and requests keeper starts, and streams a [`VerificationFlowVm`]
    /// state machine into `sink`. Returns the subscription id.
    ///
    /// Mirrors [`subscribe_encryption_status`]'s lazy-activation, supervised-task,
    /// and self-reap lifecycle. Additionally installs the producer's flow-id sender
    /// on the [`AccountHandle`] so [`AccountManager::verification_start`] can hand a
    /// keeper-started flow to the running producer; it is cleared on unsubscribe or
    /// shutdown. An activation failure maps to the existing `SyncUnavailable` code.
    pub async fn subscribe_verification(
        &self,
        platform: &Arc<dyn Platform>,
        account_id: &str,
        sink: VerificationSink,
    ) -> Result<u64, CoreError> {
        let (client, subs_arc, flow_tx_slot, did_activate) = {
            let mut accounts = self.accounts.lock().await;
            let did_activate = !accounts.contains_key(account_id);
            if did_activate {
                let (client, sync, reconnect_supervisor, session_persister) =
                    activate(platform, account_id).await?;
                accounts.insert(
                    account_id.to_owned(),
                    AccountHandle {
                        client,
                        sync,
                        subscriptions: Arc::new(Mutex::new(HashMap::new())),
                        timelines: Arc::new(Mutex::new(HashMap::new())),
                        reconnect_supervisor,
                        session_persister,
                        verification_flow_tx: Arc::new(Mutex::new(None)),
                    },
                );
                tracing::info!(account_id = %account_id, "account activated for verification");
            }
            let handle = accounts.get(account_id).ok_or_else(|| {
                CoreError::Internal("account handle vanished after activation".to_owned())
            })?;
            (
                handle.client.clone(),
                handle.subscriptions.clone(),
                handle.verification_flow_tx.clone(),
                did_activate,
            )
        };

        // The producer drains this channel for keeper-started flow ids; the sender
        // is stored on the handle for `verification_start`.
        let (flow_tx, flow_rx) = mpsc::unbounded_channel::<String>();
        *flow_tx_slot.lock().await = Some(flow_tx);

        let subscription_id = NEXT_SUBSCRIPTION_ID.fetch_add(1, Ordering::Relaxed);
        let account_id_owned = account_id.to_owned();
        let span = tracing::info_span!("verification_producer", account_id = %account_id);
        let reaper_subs = subs_arc.clone();
        let reaper_flow_tx = flow_tx_slot.clone();
        let task = tokio::spawn(
            async move {
                verification::run_producer(client, sink, flow_rx, &account_id_owned).await;
                // A naturally-completed producer reaps its own subscription entry
                // and clears the flow sender so a later `verification_start` fails
                // fast rather than pushing into a dead producer.
                reaper_subs.lock().await.remove(&subscription_id);
                *reaper_flow_tx.lock().await = None;
            }
            .instrument(span),
        );

        {
            let accounts = self.accounts.lock().await;
            match accounts.get(account_id) {
                Some(_) => {
                    subs_arc.lock().await.insert(subscription_id, task);
                }
                None => {
                    task.abort();
                    *flow_tx_slot.lock().await = None;
                    if did_activate {
                        return Err(AccountError::SyncStart(
                            "account removed during subscribe".to_owned(),
                        )
                        .into());
                    }
                }
            }
        }
        tracing::info!(account_id = %account_id, subscription_id, "verification subscribed");
        Ok(subscription_id)
    }

    /// Abort exactly the verification producer task for `subscription_id` and
    /// clear the account's flow sender. Other account state is untouched.
    /// Idempotent.
    pub async fn unsubscribe_verification(&self, account_id: &str, subscription_id: u64) {
        {
            let accounts = self.accounts.lock().await;
            if let Some(handle) = accounts.get(account_id) {
                *handle.verification_flow_tx.lock().await = None;
            }
        }
        if self.abort_subscription(account_id, subscription_id).await {
            tracing::info!(account_id = %account_id, subscription_id, "verification unsubscribed");
        }
    }

    /// Resolve the account's live `Client`, or a typed error if it is not live.
    async fn client_for(&self, account_id: &str) -> Result<Client, CoreError> {
        let accounts = self.accounts.lock().await;
        accounts
            .get(account_id)
            .map(|h| h.client.clone())
            .ok_or_else(|| {
                crate::error::VerificationError::Unavailable(
                    "account is not live; subscribe to verification first".to_owned(),
                )
                .into()
            })
    }

    /// Resolve the account's live `Client` for a backup action, surfacing a *named*
    /// [`BackupError::Unavailable`] (→ `backupFailed`) when the account is not live
    /// — never a verification error code. Backup status is subscribed at app-shell
    /// mount (which lazily activates the account), so in practice the account is
    /// live before any enable/restore button is reachable; this keeps the failure
    /// honest inside this story's named-backup-error taxonomy regardless.
    async fn client_for_backup(&self, account_id: &str) -> Result<Client, CoreError> {
        let accounts = self.accounts.lock().await;
        accounts
            .get(account_id)
            .map(|h| h.client.clone())
            .ok_or_else(|| {
                BackupError::Unavailable(
                    "account is not live; subscribe to backup status first".to_owned(),
                )
                .into()
            })
    }

    /// Start an interactive self-verification from keeper (Story 3.2). Requests
    /// the verification via the account's `Client`, then forwards the new flow id
    /// into the live verification producer so it surfaces in the stream. Requires
    /// an active verification subscription (its producer holds the flow receiver).
    pub async fn verification_start(&self, account_id: &str) -> Result<(), CoreError> {
        let client = self.client_for(account_id).await?;
        // Require a live producer BEFORE creating an SDK request, so we never leave a
        // dangling `m.key.verification.request` on the other device that keeper won't
        // drive — and never report a false success (the flow must actually stream).
        let sender = {
            let accounts = self.accounts.lock().await;
            match accounts.get(account_id) {
                Some(handle) => handle.verification_flow_tx.lock().await.clone(),
                None => None,
            }
        };
        let Some(sender) = sender else {
            return Err(crate::error::VerificationError::Unavailable(
                "no active verification subscription; subscribe before starting".to_owned(),
            )
            .into());
        };
        let flow_id = verification::start(&client).await?;
        // Hand the new flow id to the live producer so it drives the stream. If the
        // producer died between the check above and here, surface an honest error
        // rather than a silent dangling request.
        sender.send(flow_id).map_err(|_| {
            crate::error::VerificationError::Unavailable(
                "verification producer stopped before the flow could start".to_owned(),
            )
        })?;
        Ok(())
    }

    /// Accept an incoming verification request (the peer started it).
    pub async fn verification_accept(
        &self,
        account_id: &str,
        flow_id: &str,
    ) -> Result<(), CoreError> {
        let client = self.client_for(account_id).await?;
        verification::accept(&client, flow_id).await
    }

    /// Start the emoji/SAS sub-flow on a ready request.
    pub async fn verification_start_sas(
        &self,
        account_id: &str,
        flow_id: &str,
    ) -> Result<(), CoreError> {
        let client = self.client_for(account_id).await?;
        verification::start_sas(&client, flow_id).await
    }

    /// Confirm the SAS emoji match (our side).
    pub async fn verification_confirm(
        &self,
        account_id: &str,
        flow_id: &str,
    ) -> Result<(), CoreError> {
        let client = self.client_for(account_id).await?;
        verification::confirm(&client, flow_id).await
    }

    /// Signal that the SAS emoji do not match (cancels with the mismatch code).
    pub async fn verification_mismatch(
        &self,
        account_id: &str,
        flow_id: &str,
    ) -> Result<(), CoreError> {
        let client = self.client_for(account_id).await?;
        verification::mismatch(&client, flow_id).await
    }

    /// Cancel the flow (user closed the modal / pressed Esc).
    pub async fn verification_cancel(
        &self,
        account_id: &str,
        flow_id: &str,
    ) -> Result<(), CoreError> {
        let client = self.client_for(account_id).await?;
        verification::cancel(&client, flow_id).await
    }

    /// Abort exactly the producer task for `subscription_id`; other account
    /// state is untouched. Idempotent — unsubscribing an unknown id is a no-op.
    pub async fn unsubscribe_room_list(&self, account_id: &str, subscription_id: u64) {
        if self.abort_subscription(account_id, subscription_id).await {
            tracing::info!(account_id = %account_id, subscription_id, "room list unsubscribed");
        }
    }

    /// Abort exactly the timeline producer task for `subscription_id`, dropping
    /// its `Timeline` (the SDK drop handle cancels its background tasks). Other
    /// account state — the room-list stream and any sibling timeline — is
    /// untouched. Idempotent.
    pub async fn unsubscribe_timeline(&self, account_id: &str, subscription_id: u64) {
        if self.abort_subscription(account_id, subscription_id).await {
            tracing::info!(account_id = %account_id, subscription_id, "timeline unsubscribed");
        }
    }

    /// Shared abort body: remove and abort exactly one subscription task from an
    /// account's subscription map, and drop any open `Arc<Timeline>` registered
    /// under the same id (a timeline subscription — the SDK drop handle cancels
    /// its background tasks once the last reference is gone). Returns whether a
    /// task was found and aborted.
    async fn abort_subscription(&self, account_id: &str, subscription_id: u64) -> bool {
        let accounts = self.accounts.lock().await;
        if let Some(handle) = accounts.get(account_id) {
            // Drop the stored timeline first (a room-list id simply isn't present
            // here — a no-op), then abort the producer task.
            handle.timelines.lock().await.remove(&subscription_id);
            if let Some(task) = handle.subscriptions.lock().await.remove(&subscription_id) {
                task.abort();
                return true;
            }
        }
        false
    }

    /// Send a plain-text message to `room_id` on `account_id` through the single
    /// dispatch gate (FR-41, AD-13). Resolves the live `AccountHandle` and the
    /// room's open `Arc<Timeline>`, then delegates to [`send::submit`]. The local
    /// echo and every send-state transition arrive through the room's existing
    /// timeline subscription — this call synthesizes nothing.
    ///
    /// Errors: an unparsable/unknown room id → [`SendError::RoomNotFound`]; a room
    /// with no open timeline subscription → [`SendError::NoOpenTimeline`]; an SDK
    /// enqueue failure → [`SendError::Dispatch`].
    pub async fn send_text(
        &self,
        account_id: &str,
        room_id: &str,
        body: &str,
    ) -> Result<(), CoreError> {
        let room_id: OwnedRoomId = RoomId::parse(room_id).map_err(|_| SendError::RoomNotFound)?;
        let timeline = self.open_timeline_for(account_id, &room_id).await?;
        send::submit(&timeline, body, SendTrigger::ComposerSend).await?;
        tracing::info!(account_id = %account_id, room_id = %room_id, "message dispatched");
        Ok(())
    }

    /// Send a plain-text reply to the message addressed by `in_reply_to_key` (the
    /// original item's opaque render key) on `room_id`/`account_id` through the
    /// single dispatch gate (FR-41, AD-13, Story 3.4). Resolves the live
    /// `Arc<Timeline>` and delegates to [`send::submit_reply`]; the reply's local
    /// echo (with its own quoted-original preview) and send-state transitions arrive
    /// through the room's existing timeline subscription — this synthesizes nothing.
    ///
    /// Errors: an unparsable/unknown room id → [`SendError::RoomNotFound`]; no open
    /// timeline → [`SendError::NoOpenTimeline`]; an unresolvable reply target →
    /// [`SendError::TargetNotFound`]; an SDK enqueue failure → [`SendError::Dispatch`].
    pub async fn send_reply(
        &self,
        account_id: &str,
        room_id: &str,
        in_reply_to_key: &str,
        body: &str,
    ) -> Result<(), CoreError> {
        let room_id: OwnedRoomId = RoomId::parse(room_id).map_err(|_| SendError::RoomNotFound)?;
        let timeline = self.open_timeline_for(account_id, &room_id).await?;
        send::submit_reply(&timeline, in_reply_to_key, body).await?;
        tracing::info!(account_id = %account_id, room_id = %room_id, "reply dispatched");
        Ok(())
    }

    /// Edit the own message addressed by `item_key` (its opaque render key) in
    /// place on `room_id`/`account_id` through the single dispatch gate (FR-41,
    /// AD-13, Story 3.4). Resolves the live `Arc<Timeline>` and delegates to
    /// [`send::submit_edit`]; the `Set` diff that replaces the content (and flips
    /// `is_edited`) arrives through the room's existing timeline subscription.
    ///
    /// Errors: unknown room / no open timeline as [`send_text`]; an unresolvable
    /// target → [`SendError::TargetNotFound`]; a non-editable target (not own /
    /// not text) → [`SendError::NotEditable`]; an SDK enqueue failure →
    /// [`SendError::Dispatch`].
    pub async fn edit_message(
        &self,
        account_id: &str,
        room_id: &str,
        item_key: &str,
        body: &str,
    ) -> Result<(), CoreError> {
        let room_id: OwnedRoomId = RoomId::parse(room_id).map_err(|_| SendError::RoomNotFound)?;
        let timeline = self.open_timeline_for(account_id, &room_id).await?;
        send::submit_edit(&timeline, item_key, body).await?;
        tracing::info!(account_id = %account_id, room_id = %room_id, "message edit dispatched");
        Ok(())
    }

    /// Retry a failed outgoing message by re-driving its wedged local echo
    /// (`item_key` = the item's `unique_id`) through the controlled send path —
    /// `SendHandle::unwedge()`, not a new content dispatch (FR-41). Resolves the
    /// same live `Arc<Timeline>` that produced the item so the `unique_id` matches,
    /// then delegates to [`send::retry`].
    ///
    /// Errors: unknown room / no open timeline as [`send_text`]; a missing echo →
    /// [`SendError::EchoNotFound`].
    pub async fn retry_send(
        &self,
        account_id: &str,
        room_id: &str,
        item_key: &str,
    ) -> Result<(), CoreError> {
        let room_id: OwnedRoomId = RoomId::parse(room_id).map_err(|_| SendError::RoomNotFound)?;
        let timeline = self.open_timeline_for(account_id, &room_id).await?;
        send::retry(&timeline, item_key).await?;
        tracing::info!(account_id = %account_id, room_id = %room_id, "outgoing message retry re-driven");
        Ok(())
    }

    /// Resolve the exact open `Arc<Timeline>` for `room_id` on the live account,
    /// or a typed [`SendError::NoOpenTimeline`] if the account isn't live or no
    /// timeline for the room is subscribed.
    async fn open_timeline_for(
        &self,
        account_id: &str,
        room_id: &RoomId,
    ) -> Result<Arc<Timeline>, CoreError> {
        let accounts = self.accounts.lock().await;
        let handle = accounts.get(account_id).ok_or(SendError::NoOpenTimeline)?;
        let timelines = handle.timelines.lock().await;
        // A room can be transiently registered under two subscription ids (a
        // StrictMode remount, or a re-subscribe before the previous producer's
        // reaper runs). Pick the newest (highest subscription id) so send/retry
        // always target the timeline instance currently feeding the UI —
        // `unique_id`s are only stable within one instance, so a stale one would
        // fail the retry item lookup.
        let timeline = timelines
            .iter()
            .filter(|(_, (open_room, _))| open_room == room_id)
            .max_by_key(|(subscription_id, _)| **subscription_id)
            .map(|(_, (_, tl))| tl.clone())
            .ok_or(SendError::NoOpenTimeline)?;
        Ok(timeline)
    }

    /// Tear down the whole account: remove it from the merged inbox (so its rooms
    /// leave immediately while other accounts keep syncing), abort every
    /// subscription, and drop the live `Client`/`SyncService`.
    pub async fn shutdown(&self, account_id: &str) {
        // Remove the account from the active merged inbox first, so its rows are
        // dropped from the emitted window, then abort *its* inbox producer and
        // wait for it to finish. This releases the producer's `RoomList` (and the
        // SQLite handles it holds through the `Client`) before `sign_out` deletes
        // the store dir — without it the producer, which lives in `InboxHandle`
        // rather than the account's `subscriptions`, would keep the store open
        // until the frontend happened to re-subscribe the whole inbox.
        {
            let mut inbox = self.inbox.lock().await;
            if let Some(handle) = inbox.as_mut() {
                handle.merger.remove_account(account_id).await;
                if let Some(task) = handle.producers.remove(account_id) {
                    task.abort();
                    let _ = task.await;
                }
            }
        }
        let mut accounts = self.accounts.lock().await;
        if let Some(handle) = accounts.remove(account_id) {
            // Stop the SyncService first so no further diffs are produced, then
            // abort the reconnect supervisor and any remaining producer tasks.
            handle.sync.stop().await;
            handle.reconnect_supervisor.abort();
            // Abort and await the session-persister so its `Client` clone (and
            // the store's SQLite handles it keeps alive) is dropped before
            // `sign_out_cleanup` deletes the store dir, and so it can never
            // re-persist the just-deleted Keychain key on a late token refresh.
            handle.session_persister.abort();
            let _ = handle.session_persister.await;
            let mut subs = handle.subscriptions.lock().await;
            for (_, task) in subs.drain() {
                task.abort();
            }
            // Drop every stored `Arc<Timeline>` so no room timeline leaks.
            handle.timelines.lock().await.clear();
            tracing::info!(account_id = %account_id, "account shut down");
        }
    }

    /// Sign out an account: tear down its live in-memory state, then delete its
    /// persisted state (SDK store dir + Keychain session + registry row) — local
    /// only, no server-side logout (AD-10, Story 1.8).
    ///
    /// `shutdown` runs *first* so the SDK's SQLite handles are released before the
    /// store dir is removed, and it is a no-op when the account was never
    /// activated (restored-but-never-subscribed). [`crate::auth::sign_out_cleanup`]
    /// then removes exactly this account's three persisted targets, each
    /// idempotent — so sign-out converges whether or not the account was live.
    pub async fn sign_out(
        &self,
        platform: &Arc<dyn Platform>,
        account_id: &str,
    ) -> Result<(), CoreError> {
        self.shutdown(account_id).await;
        crate::auth::sign_out_cleanup(platform.as_ref(), account_id)?;
        Ok(())
    }
}

/// Keychain key under which an account's saved base58 recovery key is stored
/// (Story 3.3, FR-14) — the user's opt-in save after enabling key backup.
/// Namespaced by account id so it is scoped exactly to one account. The stored
/// value is the human-facing recovery key, never any other secret.
fn recovery_key_keychain_key(account_id: &str) -> String {
    format!("recovery_key/{account_id}")
}

/// Lazily rebuild the `Client` from the persisted session and start a
/// `SyncService`. Also the Story 1.8 cold-start restore path.
///
/// The `SyncService` is built with `.with_offline_mode()` so it exposes a real
/// `Offline` state (auto-resumed via `/_matrix/client/versions`). After
/// `sync.start()`, the send queue is enabled once so any persisted queued sends
/// from a prior process reload and dispatch (force-quit resilience). A
/// lifetime-of-account reconnect supervisor is spawned to re-enable the send
/// queue on every transition back into `Running`; its `JoinHandle` is returned
/// for the caller to store on the `AccountHandle`.
#[tracing::instrument(skip(platform), fields(account_id = %account_id))]
async fn activate(
    platform: &Arc<dyn Platform>,
    account_id: &str,
) -> Result<ActivatedAccount, CoreError> {
    let session_json = platform
        .keychain_get(&session_keychain_key(account_id))?
        .ok_or(AccountError::SessionMissing)?;
    // Legacy-tolerant read: a tagged `StoredSession` (password or OAuth), or a
    // pre-2.2 bare `MatrixSession` blob read as a password session.
    let stored = auth::StoredSession::from_json(&session_json)
        .map_err(|e| AccountError::RestoreFailed(e.to_string()))?;

    let data_dir = platform.data_dir()?;
    let row = registry::get_account(&data_dir, account_id)?.ok_or(AccountError::SessionMissing)?;
    let sdk_dir = data_dir.join("accounts").join(account_id).join("sdk");

    // At-rest encryption (Story 2.6, AD-22): the per-account SDK-store passphrase
    // is self-describing — present in the Keychain iff the store was created
    // encrypted, so re-open it with `Some(passphrase)` exactly then, else `None`
    // (FileVault posture). The passphrase never leaves Rust.
    let passphrase = platform.keychain_get(&auth::store_passphrase_keychain_key(account_id))?;

    // OAuth refresh tokens are one-time-use (MAS); `handle_refresh_tokens()` lets
    // the SDK rotate them, and the session-change watcher below re-persists the
    // rotated blob so a restart-after-refresh restores cleanly.
    let client = Client::builder()
        .homeserver_url(&row.homeserver_url)
        .sqlite_store(&sdk_dir, passphrase.as_deref())
        .handle_refresh_tokens()
        .build()
        .await
        .map_err(|e| AccountError::RestoreFailed(e.to_string()))?;

    stored
        .restore_into(&client)
        .await
        .map_err(|e| AccountError::RestoreFailed(e.to_string()))?;

    // Re-persist the Keychain blob whenever the SDK rotates the session tokens,
    // so the (one-time-use) rotated OAuth refresh token survives a restart. A
    // best-effort background task keyed by this account.
    let session_persister =
        spawn_session_persister(platform.clone(), client.clone(), account_id.to_owned());

    let sync = SyncService::builder(client.clone())
        .with_offline_mode()
        .build()
        .await
        .map_err(|e| AccountError::SyncStart(e.to_string()))?;
    sync.start().await;

    // Enable the persistent send queue once at activation: this reloads any
    // unsent requests persisted by a prior process and respawns their tasks
    // (force-quit resilience). Idempotent.
    client.send_queue().set_enabled(true).await;

    let sync = std::sync::Arc::new(sync);
    let reconnect_supervisor = tokio::spawn(
        run_reconnect_supervisor(client.clone(), sync.clone(), account_id.to_owned())
            .instrument(tracing::info_span!("reconnect_supervisor", account_id = %account_id)),
    );

    Ok((client, sync, reconnect_supervisor, session_persister))
}

/// Re-persist the account's live session blob to the Keychain (best-effort).
///
/// Reads the current [`crate::auth::StoredSession`] from the live `client` — so
/// it always reflects the newest rotated tokens — and writes it via the
/// [`Platform`] keychain port. Failures are logged, never propagated.
fn persist_current_session(platform: &dyn Platform, client: &Client, account_id: &str) {
    let Some(stored) = auth::StoredSession::from_client(client) else {
        return;
    };
    match stored.to_json() {
        Ok(json) => {
            let key = session_keychain_key(account_id);
            if let Err(e) = platform.keychain_set(&key, &json) {
                tracing::warn!(
                    account_id = %account_id,
                    error = %e,
                    "could not re-persist refreshed session to keychain"
                );
            } else {
                tracing::info!(
                    account_id = %account_id,
                    "refreshed session tokens re-persisted to keychain"
                );
            }
        }
        Err(e) => tracing::warn!(
            account_id = %account_id,
            error = %e,
            "could not serialize refreshed session"
        ),
    }
}

/// Spawn a best-effort background task that re-persists the account's Keychain
/// session blob whenever the SDK rotates its tokens (Story 2.2).
///
/// MAS OAuth refresh tokens are one-time-use, so an in-session refresh must be
/// written back or the next cold-start restore would fail. Subscribes to
/// `client.subscribe_to_session_changes()` and, on `TokensRefreshed`, re-reads
/// and writes the live session. A `UnknownToken` (revoked/invalid) change is
/// logged but not acted on here — the account simply needs re-login on next
/// launch. The returned `JoinHandle` is stored on the `AccountHandle` and
/// aborted in [`AccountManager::shutdown`]; the task also ends on its own when
/// the broadcast sender is dropped (the `Client` was torn down).
fn spawn_session_persister(
    platform: Arc<dyn Platform>,
    client: Client,
    account_id: String,
) -> JoinHandle<()> {
    let mut changes = client.subscribe_to_session_changes();
    tokio::spawn(async move {
        use matrix_sdk::SessionChange;
        loop {
            match changes.recv().await {
                Ok(SessionChange::TokensRefreshed) => {
                    persist_current_session(platform.as_ref(), &client, &account_id);
                }
                // A revoked/unknown token: the account needs re-login next launch.
                Ok(_) => {
                    tracing::info!(
                        account_id = %account_id,
                        "session token became invalid; account will need re-login"
                    );
                }
                // Lagged: one or more changes were dropped from the buffer. The
                // dropped change may have been the final rotation before the
                // client is torn down, so persist the current (latest) live
                // session now rather than waiting for a next event that may
                // never come — otherwise the one-time-use refresh token is lost.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    persist_current_session(platform.as_ref(), &client, &account_id);
                }
                // Sender dropped: the client was torn down; end the task.
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    })
}

/// Lifetime-of-account reconnect supervisor: observe `sync.state()` and, on every
/// transition **into** `Running`, call `client.send_queue().set_enabled(true)`.
///
/// A recoverable send error disables a room's queue (it does not self-retry), so
/// re-enabling the send queue on return to `Running` is what makes queued sends
/// dispatch automatically on reconnect. `set_enabled(true)` is idempotent and
/// internally respawns tasks for rooms with unsent requests. The task ends when
/// the state subscriber closes (the `SyncService` was dropped) or it is aborted
/// on shutdown/teardown.
async fn run_reconnect_supervisor(
    client: Client,
    sync: std::sync::Arc<SyncService>,
    account_id: String,
) {
    let mut states = sync.state();
    // Seed with the current state so an initial `Running` isn't treated as a
    // transition into `Running` (activation already enabled the send queue).
    let mut was_running = matches!(states.get(), State::Running);
    while let Some(state) = states.next().await {
        let is_running = matches!(state, State::Running);
        if is_running && !was_running {
            client.send_queue().set_enabled(true).await;
            tracing::info!(
                account_id = %account_id,
                "reconnected: send queue re-enabled and unsent requests respawned"
            );
        }
        was_running = is_running;
    }
}

/// Drive the connectivity stream: emit the current mapped [`ConnectionStatus`]
/// as an initial snapshot, then a batch on every `sync.state()` change, deduping
/// consecutive-equal statuses (the snapshot-then-diff contract, AD-8). Stops when
/// the sink reports the channel is closed or the state subscriber ends.
async fn run_connection_producer(
    sync: std::sync::Arc<SyncService>,
    sink: ConnectionSink,
    account_id: &str,
) {
    let mut states = sync.state();
    let mut last = map_connection_status(&states.get());
    if !(sink)(ConnectionStatusBatch { status: last }) {
        tracing::info!(account_id = %account_id, "connection status channel closed, stopping producer");
        return;
    }
    while let Some(state) = states.next().await {
        let status = map_connection_status(&state);
        if status == last {
            continue;
        }
        last = status;
        if !(sink)(ConnectionStatusBatch { status }) {
            tracing::info!(account_id = %account_id, "connection status channel closed, stopping producer");
            break;
        }
    }
    tracing::info!(account_id = %account_id, "connection status stream ended");
}

/// Pure mapping of the SDK `SyncService` state to a [`ConnectionStatus`]:
/// `Running` is `Online`; every other state (`Idle`, `Terminated`, `Error`,
/// `Offline`) is `Offline`. The offline pill and the "Queued" caption both derive
/// from this one signal. Unit-tested over the publicly constructible variants
/// (`Error(Arc<Error>)` has no public constructor — covered by reasoning).
fn map_connection_status(state: &State) -> ConnectionStatus {
    match state {
        State::Running => ConnectionStatus::Online,
        State::Idle | State::Terminated | State::Error(_) | State::Offline => {
            ConnectionStatus::Offline
        }
    }
}

/// Drive the device-verification stream: emit the current mapped
/// [`EncryptionStatus`] as an initial snapshot, then a batch on every
/// `verification_state()` change, deduping consecutive-equal statuses (the
/// snapshot-then-diff contract, AD-8). Stops when the sink reports the channel is
/// closed or the subscriber ends. Reads only the SDK verification state — no key,
/// session, or plaintext material is touched (NFR-9, AD-1).
async fn run_encryption_status_producer(client: &Client, sink: EncryptionSink, account_id: &str) {
    let mut states = client.encryption().verification_state();
    let mut last = map_encryption_status(&states.get());
    if !(sink)(EncryptionStatusBatch { status: last }) {
        tracing::info!(account_id = %account_id, "encryption status channel closed, stopping producer");
        return;
    }
    while let Some(state) = states.next().await {
        let status = map_encryption_status(&state);
        if status == last {
            continue;
        }
        last = status;
        if !(sink)(EncryptionStatusBatch { status }) {
            tracing::info!(account_id = %account_id, "encryption status channel closed, stopping producer");
            break;
        }
    }
    tracing::info!(account_id = %account_id, "encryption status stream ended");
}

/// Pure mapping of the SDK [`VerificationState`] to an [`EncryptionStatus`]:
/// `Unknown` → `Unknown` (crypto not synced — no nag), `Verified` → `Verified`,
/// `Unverified` → `Unverified`. The "verify this device" banner and the Settings
/// badge derive from this one signal; the banner shows only on `Unverified`.
fn map_encryption_status(state: &VerificationState) -> EncryptionStatus {
    match state {
        VerificationState::Unknown => EncryptionStatus::Unknown,
        VerificationState::Verified => EncryptionStatus::Verified,
        VerificationState::Unverified => EncryptionStatus::Unverified,
    }
}

/// Drive the recency-sorted entries stream, converting each `VectorDiff` batch
/// into a [`RoomListBatch`] and forwarding it to `sink`. The stream yields
/// nothing until the filter is set, then a `Reset` and live diffs.
async fn run_producer(room_list: RoomList, sink: BatchSink, account_id: &str) {
    let mut loading_state = room_list.loading_state();
    let (stream, controller) = room_list.entries_with_dynamic_adapters(ROOM_LIST_PAGE_SIZE);
    if !controller.set_filter(Box::new(new_filter_non_left())) {
        tracing::warn!(account_id = %account_id, "room list filter not applied (stream dropped)");
        return;
    }
    // Keep the controller alive for the stream's lifetime; dropping it would
    // terminate the entries stream.
    let _controller = controller;

    let mut total = loaded_total(&loading_state.get());
    // Once the loading-state stream terminates, its `select!` branch is disabled
    // so it is never re-polled after returning `None`.
    let mut loading_done = false;

    futures_util::pin_mut!(stream);
    loop {
        tokio::select! {
            maybe_diffs = stream.next() => {
                match maybe_diffs {
                    Some(diffs) => {
                        let mut ops = Vec::with_capacity(diffs.len());
                        for diff in diffs {
                            ops.push(diff_to_op(diff).await);
                        }
                        if !(sink)(RoomListBatch { ops, total }) {
                            tracing::info!(account_id = %account_id, "room list channel closed, stopping producer");
                            break;
                        }
                    }
                    None => {
                        tracing::info!(account_id = %account_id, "room list stream ended");
                        break;
                    }
                }
            }
            maybe_state = loading_state.next(), if !loading_done => {
                match maybe_state {
                    Some(state) => total = loaded_total(&state),
                    None => loading_done = true,
                }
            }
        }
    }
}

/// Drive one account's recency-sorted entries stream for the merged inbox:
/// convert each `VectorDiff` batch into a per-account [`RoomListBatch`] (reusing
/// the exact same conversion as [`run_producer`]) and hand it to the shared
/// [`InboxMerger`], which folds it into that account's slot and re-emits the
/// merged window. Stops when the merger reports the output channel is closed or
/// the entries stream ends.
async fn run_inbox_producer(room_list: RoomList, merger: InboxMerger, account_id: &str) {
    let mut loading_state = room_list.loading_state();
    let (stream, controller) = room_list.entries_with_dynamic_adapters(ROOM_LIST_PAGE_SIZE);
    if !controller.set_filter(Box::new(new_filter_non_left())) {
        tracing::warn!(account_id = %account_id, "inbox room list filter not applied (stream dropped)");
        return;
    }
    let _controller = controller;

    let mut total = loaded_total(&loading_state.get());
    let mut loading_done = false;

    futures_util::pin_mut!(stream);
    loop {
        tokio::select! {
            maybe_diffs = stream.next() => {
                match maybe_diffs {
                    Some(diffs) => {
                        let mut ops = Vec::with_capacity(diffs.len());
                        for diff in diffs {
                            ops.push(diff_to_op(diff).await);
                        }
                        if !merger.apply_account_batch(account_id, RoomListBatch { ops, total }).await {
                            tracing::info!(account_id = %account_id, "inbox channel closed, stopping producer");
                            break;
                        }
                    }
                    None => {
                        tracing::info!(account_id = %account_id, "inbox room list stream ended");
                        break;
                    }
                }
            }
            maybe_state = loading_state.next(), if !loading_done => {
                match maybe_state {
                    Some(state) => total = loaded_total(&state),
                    None => loading_done = true,
                }
            }
        }
    }
}

/// Extract the known room total from a [`RoomListLoadingState`], if loaded.
fn loaded_total(state: &RoomListLoadingState) -> Option<u32> {
    match state {
        RoomListLoadingState::Loaded {
            maximum_number_of_rooms,
        } => *maximum_number_of_rooms,
        RoomListLoadingState::NotLoaded => None,
    }
}

/// Convert a `VectorDiff<RoomListItem>` into a [`RoomListOp`], resolving each
/// carried item to a [`RoomVm`] (async) before delegating to the pure
/// [`vector_diff_to_op`] seam.
async fn diff_to_op(diff: VectorDiff<RoomListItem>) -> RoomListOp {
    let mapped = map_vector_diff(diff).await;
    vector_diff_to_op(mapped)
}

/// Map every `RoomListItem` in a diff to a [`RoomVm`], preserving the variant.
///
/// Kept separate from [`vector_diff_to_op`] because item→VM conversion is async
/// (display name / latest event) while the diff→op conversion is pure.
async fn map_vector_diff(diff: VectorDiff<RoomListItem>) -> VectorDiff<RoomVm> {
    match diff {
        VectorDiff::Append { values } => {
            let mut vms = Vec::with_capacity(values.len());
            for item in values {
                vms.push(room_item_to_vm(&item).await);
            }
            VectorDiff::Append { values: vms.into() }
        }
        VectorDiff::Clear => VectorDiff::Clear,
        VectorDiff::PushFront { value } => VectorDiff::PushFront {
            value: room_item_to_vm(&value).await,
        },
        VectorDiff::PushBack { value } => VectorDiff::PushBack {
            value: room_item_to_vm(&value).await,
        },
        VectorDiff::PopFront => VectorDiff::PopFront,
        VectorDiff::PopBack => VectorDiff::PopBack,
        VectorDiff::Insert { index, value } => VectorDiff::Insert {
            index,
            value: room_item_to_vm(&value).await,
        },
        VectorDiff::Set { index, value } => VectorDiff::Set {
            index,
            value: room_item_to_vm(&value).await,
        },
        VectorDiff::Remove { index } => VectorDiff::Remove { index },
        VectorDiff::Truncate { length } => VectorDiff::Truncate { length },
        VectorDiff::Reset { values } => {
            let mut vms = Vec::with_capacity(values.len());
            for item in values {
                vms.push(room_item_to_vm(&item).await);
            }
            VectorDiff::Reset { values: vms.into() }
        }
    }
}

/// Resolve a [`RoomListItem`] to a non-secret [`RoomVm`]: display name plus a
/// latest-event text preview and timestamp.
async fn room_item_to_vm(item: &RoomListItem) -> RoomVm {
    let room_id = item.room_id().to_string();
    let resolved = item.display_name().await;
    if resolved.is_err() {
        tracing::debug!(room_id = %room_id, "display name resolve failed, using fallback");
    }
    let display_name = resolved
        .ok()
        .map(|n| n.to_string())
        .filter(|n| !n.trim().is_empty())
        .or_else(|| {
            item.cached_display_name()
                .map(|n| n.to_string())
                .filter(|n| !n.trim().is_empty())
        })
        .unwrap_or_else(|| room_id.clone());
    let avatar_url = item.avatar_url().map(|u| u.to_string());
    let (last_message, timestamp) = latest_event_preview(item);

    RoomVm {
        room_id,
        display_name,
        last_message,
        timestamp,
        avatar_url,
    }
}

/// Extract the plain-text preview + timestamp from a room's latest event.
///
/// A remote `m.room.message` yields its body (truncated) and origin ts; every
/// other event kind yields `None` for the preview and the room's latest-event
/// timestamp when available. No SDK preview helper exists — this decodes the raw
/// event directly.
fn latest_event_preview(item: &RoomListItem) -> (Option<String>, Option<i64>) {
    use matrix_sdk::latest_events::LatestEventValue;

    let fallback_ts = item.latest_event_timestamp().map(|ts| ts.get().into());

    match item.latest_event() {
        LatestEventValue::Remote(event) => {
            let event_ts = event.timestamp().map(|ts| ts.get().into());
            decode_message_preview(event.raw(), event_ts, fallback_ts)
        }
        // Local / invite / none: no rendered remote-message preview.
        _ => (None, fallback_ts),
    }
}

/// Decode the preview + timestamp from a raw remote timeline event.
///
/// A remote `m.room.message` yields its truncated body; every other event kind
/// yields `None`. In both cases the timestamp is the event's origin ts when
/// present, else `fallback_ts`. Pure, so it is unit-testable without a live
/// `Client`.
fn decode_message_preview(
    raw: &matrix_sdk::ruma::serde::Raw<AnySyncTimelineEvent>,
    event_ts: Option<i64>,
    fallback_ts: Option<i64>,
) -> (Option<String>, Option<i64>) {
    match raw.deserialize() {
        Ok(AnySyncTimelineEvent::MessageLike(AnySyncMessageLikeEvent::RoomMessage(
            SyncMessageLikeEvent::Original(original),
        ))) => (
            Some(truncate_preview(original.content.body())),
            event_ts.or(fallback_ts),
        ),
        // Any other event kind: no text preview.
        _ => (None, event_ts.or(fallback_ts)),
    }
}

/// Truncate a preview to [`MAX_PREVIEW_CHARS`] characters (by `char`, so a
/// multi-byte grapheme is never split mid-byte).
fn truncate_preview(body: &str) -> String {
    if body.chars().count() <= MAX_PREVIEW_CHARS {
        body.to_owned()
    } else {
        body.chars().take(MAX_PREVIEW_CHARS).collect()
    }
}

/// Pure conversion of an already-`RoomVm` `VectorDiff` into a [`RoomListOp`].
///
/// This is the unit-tested seam: it needs no live `Client`. Every eyeball-im
/// variant maps one-to-one to the corresponding op.
pub fn vector_diff_to_op(diff: VectorDiff<RoomVm>) -> RoomListOp {
    match diff {
        VectorDiff::Append { values } => RoomListOp::Append {
            rooms: values.into_iter().collect(),
        },
        VectorDiff::Clear => RoomListOp::Clear,
        VectorDiff::PushFront { value } => RoomListOp::PushFront { room: value },
        VectorDiff::PushBack { value } => RoomListOp::PushBack { room: value },
        VectorDiff::PopFront => RoomListOp::PopFront,
        VectorDiff::PopBack => RoomListOp::PopBack,
        VectorDiff::Insert { index, value } => RoomListOp::Insert {
            index: index as u32,
            room: value,
        },
        VectorDiff::Set { index, value } => RoomListOp::Set {
            index: index as u32,
            room: value,
        },
        VectorDiff::Remove { index } => RoomListOp::Remove {
            index: index as u32,
        },
        VectorDiff::Truncate { length } => RoomListOp::Truncate {
            length: length as u32,
        },
        VectorDiff::Reset { values } => RoomListOp::Reset {
            rooms: values.into_iter().collect(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CoreError;
    use matrix_sdk_ui::eyeball_im::Vector;
    use std::path::PathBuf;

    /// Fake platform with a fixed data dir; keychain ops are no-ops. Enough for
    /// the sign-out idempotency test (the account is never active, so cleanup
    /// touches only already-absent state).
    struct FakePlatform {
        data_dir: PathBuf,
    }

    impl Platform for FakePlatform {
        fn data_dir(&self) -> Result<PathBuf, CoreError> {
            Ok(self.data_dir.clone())
        }
        fn keychain_set(&self, _key: &str, _value: &str) -> Result<(), CoreError> {
            Ok(())
        }
        fn keychain_get(&self, _key: &str) -> Result<Option<String>, CoreError> {
            Ok(None)
        }
        fn keychain_delete(&self, _key: &str) -> Result<(), CoreError> {
            Ok(())
        }
        fn open_url(&self, _url: &str) -> Result<(), CoreError> {
            Ok(())
        }
        fn notify(&self, _title: &str, _body: &str) -> Result<(), CoreError> {
            Ok(())
        }
        fn sidecar_path(&self, _name: &str) -> Result<PathBuf, CoreError> {
            Err(CoreError::Unsupported("sidecar unused in tests".to_owned()))
        }
    }

    #[tokio::test]
    async fn sign_out_is_idempotent_when_account_not_active() {
        let mut data_dir = std::env::temp_dir();
        data_dir.push(format!(
            "keeper-account-signout-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let platform: Arc<dyn Platform> = Arc::new(FakePlatform {
            data_dir: data_dir.clone(),
        });
        let manager = AccountManager::new();

        // The account was never activated (absent from the manager map): shutdown
        // is a no-op and cleanup touches only already-absent persisted state.
        manager
            .sign_out(&platform, "01ARZ3NDEKTSV4RRFFQ69G5FAV")
            .await
            .expect("sign_out of an inactive account should succeed");
        // A second sign-out is likewise a no-op.
        manager
            .sign_out(&platform, "01ARZ3NDEKTSV4RRFFQ69G5FAV")
            .await
            .expect("second sign_out should succeed");

        let _ = std::fs::remove_dir_all(&data_dir);
    }

    fn room(id: &str) -> RoomVm {
        RoomVm {
            room_id: id.to_owned(),
            display_name: id.to_owned(),
            last_message: None,
            timestamp: None,
            avatar_url: None,
        }
    }

    #[test]
    fn truncate_preview_keeps_short_bodies() {
        assert_eq!(truncate_preview("hello"), "hello");
    }

    #[test]
    fn truncate_preview_caps_long_bodies_by_char() {
        let long = "x".repeat(MAX_PREVIEW_CHARS + 50);
        let out = truncate_preview(&long);
        assert_eq!(out.chars().count(), MAX_PREVIEW_CHARS);
    }

    #[test]
    fn truncate_preview_does_not_split_multibyte_chars() {
        let long = "é".repeat(MAX_PREVIEW_CHARS + 10);
        let out = truncate_preview(&long);
        assert_eq!(out.chars().count(), MAX_PREVIEW_CHARS);
        // Valid UTF-8 (would panic on a mid-byte split).
        assert!(out.chars().all(|c| c == 'é'));
    }

    #[test]
    fn loaded_total_reads_loaded_maximum() {
        assert_eq!(
            loaded_total(&RoomListLoadingState::Loaded {
                maximum_number_of_rooms: Some(9),
            }),
            Some(9)
        );
        assert_eq!(loaded_total(&RoomListLoadingState::NotLoaded), None);
    }

    #[test]
    fn op_reset() {
        let diff = VectorDiff::Reset {
            values: Vector::from_iter([room("a"), room("b")]),
        };
        assert_eq!(
            vector_diff_to_op(diff),
            RoomListOp::Reset {
                rooms: vec![room("a"), room("b")],
            }
        );
    }

    #[test]
    fn op_append() {
        let diff = VectorDiff::Append {
            values: Vector::from_iter([room("a")]),
        };
        assert_eq!(
            vector_diff_to_op(diff),
            RoomListOp::Append {
                rooms: vec![room("a")],
            }
        );
    }

    #[test]
    fn op_clear() {
        assert_eq!(
            vector_diff_to_op(VectorDiff::<RoomVm>::Clear),
            RoomListOp::Clear
        );
    }

    #[test]
    fn op_push_front_and_back() {
        assert_eq!(
            vector_diff_to_op(VectorDiff::PushFront { value: room("a") }),
            RoomListOp::PushFront { room: room("a") }
        );
        assert_eq!(
            vector_diff_to_op(VectorDiff::PushBack { value: room("b") }),
            RoomListOp::PushBack { room: room("b") }
        );
    }

    #[test]
    fn op_pop_front_and_back() {
        assert_eq!(
            vector_diff_to_op(VectorDiff::<RoomVm>::PopFront),
            RoomListOp::PopFront
        );
        assert_eq!(
            vector_diff_to_op(VectorDiff::<RoomVm>::PopBack),
            RoomListOp::PopBack
        );
    }

    #[test]
    fn op_insert_and_set() {
        assert_eq!(
            vector_diff_to_op(VectorDiff::Insert {
                index: 2,
                value: room("a"),
            }),
            RoomListOp::Insert {
                index: 2,
                room: room("a"),
            }
        );
        assert_eq!(
            vector_diff_to_op(VectorDiff::Set {
                index: 5,
                value: room("b"),
            }),
            RoomListOp::Set {
                index: 5,
                room: room("b"),
            }
        );
    }

    #[test]
    fn decode_message_preview_extracts_room_message_body() {
        let raw: matrix_sdk::ruma::serde::Raw<AnySyncTimelineEvent> = serde_json::from_str(
            r#"{
                "type": "m.room.message",
                "event_id": "$evt:example.org",
                "sender": "@bob:example.org",
                "origin_server_ts": 1000,
                "content": { "msgtype": "m.text", "body": "hello" }
            }"#,
        )
        .expect("valid raw timeline event");
        let (preview, ts) = decode_message_preview(&raw, Some(1000), Some(500));
        assert_eq!(preview, Some("hello".to_owned()));
        assert_eq!(ts, Some(1000));
    }

    #[test]
    fn decode_message_preview_ignores_non_message_events() {
        let raw: matrix_sdk::ruma::serde::Raw<AnySyncTimelineEvent> = serde_json::from_str(
            r#"{
                "type": "m.reaction",
                "event_id": "$rx:example.org",
                "sender": "@bob:example.org",
                "origin_server_ts": 1000,
                "content": {
                    "m.relates_to": {
                        "rel_type": "m.annotation",
                        "event_id": "$evt:example.org",
                        "key": "👍"
                    }
                }
            }"#,
        )
        .expect("valid raw timeline event");
        let (preview, ts) = decode_message_preview(&raw, None, Some(500));
        assert_eq!(preview, None);
        assert_eq!(ts, Some(500));
    }

    #[test]
    fn map_connection_status_running_is_online() {
        assert_eq!(
            map_connection_status(&State::Running),
            ConnectionStatus::Online
        );
    }

    #[test]
    fn map_connection_status_non_running_is_offline() {
        // Every publicly constructible non-Running variant maps to Offline.
        // `Error(Arc<Error>)` has no public constructor — covered by reasoning.
        for state in [State::Idle, State::Terminated, State::Offline] {
            assert_eq!(map_connection_status(&state), ConnectionStatus::Offline);
        }
    }

    #[test]
    fn map_encryption_status_covers_all_variants() {
        assert_eq!(
            map_encryption_status(&VerificationState::Unknown),
            EncryptionStatus::Unknown
        );
        assert_eq!(
            map_encryption_status(&VerificationState::Verified),
            EncryptionStatus::Verified
        );
        assert_eq!(
            map_encryption_status(&VerificationState::Unverified),
            EncryptionStatus::Unverified
        );
    }

    #[test]
    fn op_remove_and_truncate() {
        assert_eq!(
            vector_diff_to_op(VectorDiff::<RoomVm>::Remove { index: 4 }),
            RoomListOp::Remove { index: 4 }
        );
        assert_eq!(
            vector_diff_to_op(VectorDiff::<RoomVm>::Truncate { length: 3 }),
            RoomListOp::Truncate { length: 3 }
        );
    }
}
