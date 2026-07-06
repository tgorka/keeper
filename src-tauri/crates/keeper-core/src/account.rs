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

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use futures_util::StreamExt;
use matrix_sdk::encryption::VerificationState;
use matrix_sdk::event_handler::EventHandlerHandle;
use matrix_sdk::ruma::events::room::message::{
    MessageType, OriginalSyncRoomMessageEvent, Relation,
};
use matrix_sdk::ruma::events::room::redaction::OriginalSyncRoomRedactionEvent;
use matrix_sdk::ruma::events::room::MediaSource;
use matrix_sdk::ruma::events::{
    AnySyncMessageLikeEvent, AnySyncTimelineEvent, SyncMessageLikeEvent,
};
use matrix_sdk::ruma::{EventId, OwnedRoomId, RoomId};
use matrix_sdk::{Client, Room};
use matrix_sdk_ui::eyeball_im::VectorDiff;
use matrix_sdk_ui::room_list_service::filters::new_filter_non_left;
use matrix_sdk_ui::room_list_service::{RoomList, RoomListItem, RoomListLoadingState};
use matrix_sdk_ui::sync_service::{State, SyncService};
use matrix_sdk_ui::timeline::{Timeline, TimelineBuilder};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use tracing::Instrument;

use crate::archive::{self, ArchiveEvent, ArchiveHandle, ArchiveMedia, ArchiveWriter};
use crate::auth::{self, session_keychain_key};
use crate::backup::{self, BackupSink};
use crate::bridge;
use crate::bridges::health::{
    BridgeHealthSink, HealthAggregator, HealthMonitor, HealthState, MonitoredSession, SessionKey,
};
use crate::bridges::login::{self, BridgeLoginSink};
use crate::bridges::transport::bot::BotDriver;
use crate::bridges::transport::provisioning::Provisioning;
use crate::bridges::transport::BridgeTransport;
use crate::drafts;
use crate::error::{
    AccountError, BackupError, BridgeError, CoreError, InboxError, MediaError, SendError,
    TimelineError,
};
use crate::inbox::{InboxMerger, NetworksSink, SpacesSink};
use crate::media::{self, MediaBytes, MediaHandle, MediaVariant};
use crate::platform::Platform;
use crate::registry;
use crate::send::{self, SendTrigger};
use crate::signals;
use crate::timeline;
use crate::verification::{self, VerificationSink};
use crate::vm::BridgeLoginInput;
use crate::vm::{
    ApprovalDraftVm, ConnectionStatus, ConnectionStatusBatch, DraftMirrorBatch, EditVersionVm,
    EncryptionStatus, EncryptionStatusBatch, InboxBatch, IncognitoVm, PaginationStatusBatch,
    RemoteDraftVm, RoomListBatch, RoomListOp, RoomVm, SpaceVm, TimelineBatch, TypingBatch,
    TypistVm,
};

/// Number of rooms in the initial fixed window (seeded windowing per AD-20).
const ROOM_LIST_PAGE_SIZE: usize = 200;

/// Defensive upper bound on a rendered message preview before it crosses IPC.
const MAX_PREVIEW_CHARS: usize = 256;

/// Defensive upper bound on a single back-pagination request from the webview
/// (AD-4 trust boundary): the UI pages in batches of 40, so 200 is generous.
const MAX_PAGINATE_EVENTS: u16 = 200;

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

/// Sink that receives each produced [`TypingBatch`] (Story 3.9, AD-14). The shell
/// wraps a Tauri `Channel::send`; tests capture into a vector. Returns `true` if
/// the batch was delivered, `false` if the channel is closed (the producer stops).
pub type TypingSink = Box<dyn Fn(TypingBatch) -> bool + Send + Sync>;

/// Sink that receives each produced [`PaginationStatusBatch`] (Story 3.9). The
/// shell wraps a Tauri `Channel::send`; tests capture into a vector. Returns
/// `true` if the batch was delivered, `false` if the channel is closed (the
/// producer then stops).
pub type PaginationSink = Box<dyn Fn(PaginationStatusBatch) -> bool + Send + Sync>;

/// Sink that receives each produced [`DraftMirrorBatch`] (Story 7.2, AD-15). The
/// shell wraps a Tauri `Channel::send`; tests capture into a vector. Returns
/// `true` if the batch was delivered, `false` if the channel is closed (the
/// relay then stops).
pub type DraftMirrorSink = Box<dyn Fn(DraftMirrorBatch) -> bool + Send + Sync>;

/// Registry of an account's live open room timelines, keyed by the *timeline*
/// subscription id → its room id and the exact `Arc<Timeline>` that produced the
/// subscribed items (AD-19). Send/retry look it up by room id; teardown drops the
/// entry.
type OpenTimelines = Arc<Mutex<HashMap<u64, (OwnedRoomId, Arc<Timeline>)>>>;

/// A live native-bridge-login session (Story 6.3, FR-26, AD-16): the driver task
/// running [`login::drive_login`] and the input sender that `submit_bridge_login`
/// pushes a [`BridgeLoginInput`] into (a flow choice or field values). Keyed by a
/// `session_id` from [`NEXT_SUBSCRIPTION_ID`]. Cancelling aborts the task.
struct LoginSession {
    /// The running driver task.
    task: JoinHandle<()>,
    /// The input sender the driver drains for flow choices / field values.
    input_tx: mpsc::UnboundedSender<BridgeLoginInput>,
    /// A clone of the transport that powered this session, kept so an explicit
    /// cancel / graceful-shutdown drain can best-effort cancel the server-side login
    /// (POST `/login/cancel/{login_id}` for provisioning, or send the bot's cancel
    /// command) before aborting the task (the task's own copy is dropped when it is
    /// aborted).
    transport: LoginTransport,
    /// The login id populated by the driver once `login_start` succeeds; `None`
    /// until then (a cancel before start has no server-side login to cancel).
    /// A `std::sync::Mutex` (not tokio's) — it is a synchronous, single-writer
    /// slot the driver sets once and cancel reads.
    login_id: Arc<std::sync::Mutex<Option<String>>>,
}

/// The transport that powered a bridge-login session (Story 6.4, AD-16): either the
/// mautrix bridgev2 [`Provisioning`] API or the [`BotDriver`] Bridge Bot chat
/// fallback. Held on the [`LoginSession`] (both are `Clone`) so an explicit cancel /
/// graceful-shutdown drain can best-effort cancel the server-side login on whichever
/// transport drove it.
#[derive(Clone)]
enum LoginTransport {
    /// A login driven over the bridgev2 provisioning API (Story 6.3).
    Provisioning(Provisioning),
    /// A login driven over the raw Bridge Bot chat (Story 6.4).
    Bot(BotDriver),
}

impl LoginTransport {
    /// Best-effort cancel the login recorded in `login_id`, dispatching to the arm
    /// that powered the session. A failure is logged and swallowed inside each
    /// transport's `login_cancel`.
    ///
    /// The `None` case matters for the bot: `drive_login` only records the login id
    /// *after* `login_start` succeeds, but `BotDriver::login_start` sends the login
    /// command to the bot chat before awaiting the reply — so a reply timeout leaves
    /// the slot `None` even though a login was already initiated on the bot. The
    /// bot's cancel command ignores the id and is idempotent, so fire it regardless
    /// to avoid orphaning that pending bot login. Provisioning needs a real login id
    /// (`None` = no `/login/start` succeeded = nothing server-side to cancel).
    async fn cancel_recorded(&self, login_id: Option<String>) {
        match self {
            LoginTransport::Provisioning(t) => {
                if let Some(id) = login_id {
                    t.login_cancel(&id).await;
                }
            }
            LoginTransport::Bot(t) => {
                t.login_cancel(login_id.as_deref().unwrap_or_default())
                    .await
            }
        }
    }
}

/// An account's live bridge-login sessions, keyed by `session_id`.
type LoginSessions = Arc<Mutex<HashMap<u64, LoginSession>>>;

/// The live artifacts produced by [`activate`]: the `Client`, its `SyncService`,
/// the two lifetime-of-account supervisor task handles (reconnect supervisor and
/// session persister), and the account-wide archive event-handler handle — all
/// stored by the caller on the [`AccountHandle`].
type ActivatedAccount = (
    Client,
    std::sync::Arc<SyncService>,
    JoinHandle<()>,
    JoinHandle<()>,
    EventHandlerHandle,
    EventHandlerHandle,
    EventHandlerHandle,
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
    /// Live native-bridge-login sessions (Story 6.3), keyed by `session_id`. Each
    /// holds the driver task + the input sender `submit_bridge_login` pushes into.
    /// Cancelled/aborted on `cancel_bridge_login`, on natural completion (the
    /// driver self-reaps its entry), and on account shutdown.
    login_sessions: LoginSessions,
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
    /// Account-wide post-decryption archive event handler (Story 5.1): registered
    /// on the `Client` in [`activate`], it maps each message-like room event into
    /// an [`ArchiveEvent`] and hands it to the single serialized archive writer.
    /// Removed from the `Client` in [`AccountManager::shutdown`] so no handler
    /// leaks and no further rows are ingested after the account goes down.
    archive_handler: EventHandlerHandle,
    /// Account-wide redaction event handler (Story 5.2, FR-36): registered on the
    /// `Client` in [`activate`], it marks the archived target row's `redacted_ts`
    /// (marks only, never erases) via the single serialized writer. Removed from
    /// the `Client` in [`AccountManager::shutdown`] alongside `archive_handler`.
    redaction_handler: EventHandlerHandle,
    /// Account-wide `dev.keeper.draft` room-account-data handler (Story 7.2,
    /// AD-15): registered on the `Client` in [`activate`], it observes live remote
    /// draft edits and forwards each into the manager's draft-mirror broadcast so
    /// the app-wide `subscribe_draft_mirror` relay can stream them to the UI.
    /// Removed from the `Client` in [`AccountManager::shutdown`] alongside the
    /// archive/redaction handlers so no handler leaks past teardown.
    draft_handler: EventHandlerHandle,
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
    /// Per-account Spaces producers (Story 4.5): each recomputes the account's
    /// joined Spaces + child membership on every sync batch and pokes the merger.
    /// Tracked separately from `producers` so teardown aborts them too.
    spaces_producers: HashMap<String, JoinHandle<()>>,
}

/// A live bridge-session-health subscription (Story 6.5, FR-28, AD-16): the shared
/// cross-account [`HealthAggregator`] its per-account [`HealthMonitor`]s feed, keyed by
/// account id (so one account's monitor drains on that account's sign-out, not only on
/// a whole-subscription unsubscribe). There is at most one at a time.
struct BridgeHealthHandle {
    subscription_id: u64,
    aggregator: HealthAggregator,
    monitors: HashMap<String, HealthMonitor>,
}

/// Multi-account supervisor (AD-3, AD-19, AD-20). Owns the live per-account
/// state (each a supervised `Client`/`SyncService`) and the single active
/// merged-inbox subscription; the shell manages exactly one instance in its
/// `AppState`. No account-count limit is enforced anywhere.
pub struct AccountManager {
    accounts: Mutex<HashMap<String, AccountHandle>>,
    /// The single active merged-inbox subscription, if any. Sign-out/shutdown
    /// notify it so a removed account's rooms leave the inbox immediately.
    inbox: Mutex<Option<InboxHandle>>,
    /// The single active bridge-session-health subscription, if any (Story 6.5).
    /// Sign-out/shutdown drain the removed account's monitor so its sessions leave
    /// the health snapshot immediately.
    bridge_health: Mutex<Option<BridgeHealthHandle>>,
    /// Serializes [`AccountManager::subscribe_bridge_health`] against itself. The
    /// subscribe body does slow Matrix I/O (discovery + monitor spawn) between draining
    /// the prior subscription and storing the new handle; without this guard two
    /// concurrent subscribes (e.g. a StrictMode double-mount) could both build monitors
    /// and the loser's would leak (never drained). Held for the whole subscribe body.
    bridge_health_subscribe: Mutex<()>,
    /// The single app-wide archive writer handle (Story 5.1): created ONCE in
    /// [`AccountManager::new`] from the platform data dir and cloned into every
    /// account's post-decryption event handler, guaranteeing exactly one
    /// serialized writer / one `archive.db` for all Accounts. `None` only if the
    /// archive DB could not be opened at startup — ingestion is then skipped
    /// (never fatal: the archive path must never block or abort the app).
    archive: Option<ArchiveHandle>,
    /// Process broadcast of live remote draft edits (Story 7.2, AD-15): each
    /// account's `dev.keeper.draft` handler (registered in [`activate`]) forwards
    /// observed edits here, and the single app-wide `subscribe_draft_mirror` relay
    /// fans them out to the UI. Created once in [`AccountManager::new`] and cloned
    /// into every account's handler on activation.
    draft_mirror_tx: tokio::sync::broadcast::Sender<DraftMirrorBatch>,
    /// Live app-wide draft-mirror relay subscriptions, keyed by subscription id →
    /// the relay task. Aborted on `draft_mirror_unsubscribe`; a relay whose broadcast
    /// sender is dropped (all accounts torn down) ends on its own and its finished
    /// handle is cleared by the eventual `draft_mirror_unsubscribe`. Not tied to any
    /// one account — there is at most one in the shell, so the map is bounded.
    draft_mirror_subs: Mutex<HashMap<u64, JoinHandle<()>>>,
}

/// Monotonic source of subscription ids handed back to the frontend.
static NEXT_SUBSCRIPTION_ID: AtomicU64 = AtomicU64::new(1);

impl AccountManager {
    /// Construct an empty manager with no live accounts, opening the single
    /// app-wide `archive.db` and spawning its one serialized writer (Story 5.1).
    ///
    /// The archive handle is created exactly once here and cloned into each
    /// account's post-decryption event handler on activation. A failure to open
    /// `archive.db` is logged and the manager still constructs with archiving
    /// disabled — the archive path must never block or abort the app.
    pub fn new(data_dir: &Path) -> Self {
        let archive = match ArchiveWriter::spawn(data_dir) {
            Ok(handle) => Some(handle),
            Err(e) => {
                tracing::error!(error = %e, "could not open archive.db; archiving disabled");
                None
            }
        };
        // A modest buffer: draft edits are low-frequency (debounced in the UI) and
        // the relay skips to the newest on lag, so a small ring never loses the
        // convergent final state.
        let (draft_mirror_tx, _) = tokio::sync::broadcast::channel(64);
        Self {
            accounts: Mutex::new(HashMap::new()),
            inbox: Mutex::new(None),
            bridge_health: Mutex::new(None),
            bridge_health_subscribe: Mutex::new(()),
            archive,
            draft_mirror_tx,
            draft_mirror_subs: Mutex::new(HashMap::new()),
        }
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
                let (
                    client,
                    sync,
                    reconnect_supervisor,
                    session_persister,
                    archive_handler,
                    redaction_handler,
                    draft_handler,
                ) = activate(
                    platform,
                    account_id,
                    self.archive.clone(),
                    self.draft_mirror_tx.clone(),
                )
                .await?;
                accounts.insert(
                    account_id.to_owned(),
                    AccountHandle {
                        client,
                        sync,
                        subscriptions: Arc::new(Mutex::new(HashMap::new())),
                        timelines: Arc::new(Mutex::new(HashMap::new())),
                        reconnect_supervisor,
                        session_persister,
                        archive_handler,
                        redaction_handler,
                        draft_handler,
                        verification_flow_tx: Arc::new(Mutex::new(None)),
                        login_sessions: Arc::new(Mutex::new(HashMap::new())),
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
    /// (AD-20, Story 4.2). Activates each account whose Keychain session is
    /// present, opens its room-list stream, and feeds each into a shared
    /// [`InboxMerger`] that partitions the merged window into four
    /// recency/order-authoritative [`InboxBatch`] streams: the Inbox window into
    /// `inbox_sink`, the Archive window into `archive_sink`, the Pins window into
    /// `pins_sink` (seeded from keeper-local [`registry::get_pins`], Story 4.3),
    /// and the Favorites window into `favourites_sink` (SDK-sourced `m.favourite`
    /// tag, Story 4.4). Returns the inbox subscription id. Replacing an
    /// existing inbox subscription (e.g. the frontend re-subscribes after adding an
    /// account) first tears the old one down. Adding the Nth account is identical
    /// to the 2nd — no count limit.
    // Six sinks (Inbox/Archive/Pins/Favorites/Spaces/Networks) plus `self` and the
    // platform each cross the IPC boundary as a distinct stream; grouping them into a
    // struct would only obscure the one-to-one channel mapping.
    #[allow(clippy::too_many_arguments)]
    pub async fn subscribe_inbox(
        &self,
        platform: &Arc<dyn Platform>,
        inbox_sink: InboxSink,
        archive_sink: InboxSink,
        pins_sink: InboxSink,
        favourites_sink: InboxSink,
        spaces_sink: SpacesSink,
        networks_sink: NetworksSink,
    ) -> Result<u64, CoreError> {
        // Only one inbox subscription at a time: tear down any prior one so its
        // producers stop feeding a stale merger/channel.
        self.unsubscribe_inbox_inner().await;

        let accounts = auth::find_restorable_accounts(platform.as_ref())?;
        // Seed the merger's pin map from keeper-local state (Story 4.3): pins have
        // no Matrix representation, so membership + order come from the registry.
        let data_dir = platform.data_dir()?;
        let pins = load_pins(&data_dir)?;
        let merger = InboxMerger::new(
            inbox_sink,
            archive_sink,
            pins_sink,
            favourites_sink,
            pins,
            spaces_sink,
            networks_sink,
        );
        let subscription_id = NEXT_SUBSCRIPTION_ID.fetch_add(1, Ordering::Relaxed);

        // Register every account slot up front so the merge reflects the full set
        // even before any batch arrives, then start each account's producer.
        let mut producers: HashMap<String, JoinHandle<()>> = HashMap::with_capacity(accounts.len());
        let mut spaces_producers: HashMap<String, JoinHandle<()>> =
            HashMap::with_capacity(accounts.len());
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
            // The account is now live (acquire_room_list activated it): fetch its
            // `Client` to drive the Spaces producer (Story 4.5), which enumerates
            // joined Spaces + child membership locally and pokes the merger on every
            // sync batch.
            let client = {
                let accounts = self.accounts.lock().await;
                accounts.get(&account_id).map(|h| h.client.clone())
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
            if let Some(client) = client {
                let merger_for_spaces = merger.clone();
                let spaces_account_id = account.account_id.clone();
                let spaces_task = tokio::spawn(
                    async move {
                        run_spaces_producer(client, merger_for_spaces, &spaces_account_id).await;
                    }
                    .instrument(
                        tracing::info_span!("spaces_producer", account_id = %account.account_id),
                    ),
                );
                spaces_producers.insert(account.account_id.clone(), spaces_task);
            }
        }

        {
            let mut inbox = self.inbox.lock().await;
            *inbox = Some(InboxHandle {
                subscription_id,
                merger,
                producers,
                spaces_producers,
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
            for (_, task) in handle.spaces_producers {
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
                let (
                    client,
                    sync,
                    reconnect_supervisor,
                    session_persister,
                    archive_handler,
                    redaction_handler,
                    draft_handler,
                ) = activate(
                    platform,
                    account_id,
                    self.archive.clone(),
                    self.draft_mirror_tx.clone(),
                )
                .await?;
                accounts.insert(
                    account_id.to_owned(),
                    AccountHandle {
                        client,
                        sync,
                        subscriptions: Arc::new(Mutex::new(HashMap::new())),
                        timelines: Arc::new(Mutex::new(HashMap::new())),
                        reconnect_supervisor,
                        session_persister,
                        archive_handler,
                        redaction_handler,
                        draft_handler,
                        verification_flow_tx: Arc::new(Mutex::new(None)),
                        login_sessions: Arc::new(Mutex::new(HashMap::new())),
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
                let (
                    client,
                    sync,
                    reconnect_supervisor,
                    session_persister,
                    archive_handler,
                    redaction_handler,
                    draft_handler,
                ) = activate(
                    platform,
                    account_id,
                    self.archive.clone(),
                    self.draft_mirror_tx.clone(),
                )
                .await?;
                accounts.insert(
                    account_id.to_owned(),
                    AccountHandle {
                        client,
                        sync,
                        subscriptions: Arc::new(Mutex::new(HashMap::new())),
                        timelines: Arc::new(Mutex::new(HashMap::new())),
                        reconnect_supervisor,
                        session_persister,
                        archive_handler,
                        redaction_handler,
                        draft_handler,
                        verification_flow_tx: Arc::new(Mutex::new(None)),
                        login_sessions: Arc::new(Mutex::new(HashMap::new())),
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
        let open = match timeline::open_timeline(&client, &room_id, account_id).await {
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
                let (
                    client,
                    sync,
                    reconnect_supervisor,
                    session_persister,
                    archive_handler,
                    redaction_handler,
                    draft_handler,
                ) = activate(
                    platform,
                    account_id,
                    self.archive.clone(),
                    self.draft_mirror_tx.clone(),
                )
                .await?;
                accounts.insert(
                    account_id.to_owned(),
                    AccountHandle {
                        client,
                        sync,
                        subscriptions: Arc::new(Mutex::new(HashMap::new())),
                        timelines: Arc::new(Mutex::new(HashMap::new())),
                        reconnect_supervisor,
                        session_persister,
                        archive_handler,
                        redaction_handler,
                        draft_handler,
                        verification_flow_tx: Arc::new(Mutex::new(None)),
                        login_sessions: Arc::new(Mutex::new(HashMap::new())),
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
                let (
                    client,
                    sync,
                    reconnect_supervisor,
                    session_persister,
                    archive_handler,
                    redaction_handler,
                    draft_handler,
                ) = activate(
                    platform,
                    account_id,
                    self.archive.clone(),
                    self.draft_mirror_tx.clone(),
                )
                .await?;
                accounts.insert(
                    account_id.to_owned(),
                    AccountHandle {
                        client,
                        sync,
                        subscriptions: Arc::new(Mutex::new(HashMap::new())),
                        timelines: Arc::new(Mutex::new(HashMap::new())),
                        reconnect_supervisor,
                        session_persister,
                        archive_handler,
                        redaction_handler,
                        draft_handler,
                        verification_flow_tx: Arc::new(Mutex::new(None)),
                        login_sessions: Arc::new(Mutex::new(HashMap::new())),
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
                let (
                    client,
                    sync,
                    reconnect_supervisor,
                    session_persister,
                    archive_handler,
                    redaction_handler,
                    draft_handler,
                ) = activate(
                    platform,
                    account_id,
                    self.archive.clone(),
                    self.draft_mirror_tx.clone(),
                )
                .await?;
                accounts.insert(
                    account_id.to_owned(),
                    AccountHandle {
                        client,
                        sync,
                        subscriptions: Arc::new(Mutex::new(HashMap::new())),
                        timelines: Arc::new(Mutex::new(HashMap::new())),
                        reconnect_supervisor,
                        session_persister,
                        archive_handler,
                        redaction_handler,
                        draft_handler,
                        verification_flow_tx: Arc::new(Mutex::new(None)),
                        login_sessions: Arc::new(Mutex::new(HashMap::new())),
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
                let (
                    client,
                    sync,
                    reconnect_supervisor,
                    session_persister,
                    archive_handler,
                    redaction_handler,
                    draft_handler,
                ) = activate(
                    platform,
                    account_id,
                    self.archive.clone(),
                    self.draft_mirror_tx.clone(),
                )
                .await?;
                accounts.insert(
                    account_id.to_owned(),
                    AccountHandle {
                        client,
                        sync,
                        subscriptions: Arc::new(Mutex::new(HashMap::new())),
                        timelines: Arc::new(Mutex::new(HashMap::new())),
                        reconnect_supervisor,
                        session_persister,
                        archive_handler,
                        redaction_handler,
                        draft_handler,
                        verification_flow_tx: Arc::new(Mutex::new(None)),
                        login_sessions: Arc::new(Mutex::new(HashMap::new())),
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

    /// Run zero-config bridge discovery for a live account (Story 6.2, FR-25,
    /// AD-16). Clones the account's `Client` under the manager lock, then runs the
    /// three-source discovery pass ([`crate::bridges::discover`]) outside the lock
    /// (it performs Matrix I/O). A missing account surfaces the non-retriable
    /// [`BridgeError::AccountNotFound`]; a total transport failure surfaces the
    /// retriable [`BridgeError::Discovery`]. No bot MXID, token, or session
    /// material crosses back — only non-secret network ids + statuses.
    pub async fn discover_bridges(
        &self,
        account_id: &str,
    ) -> Result<crate::vm::BridgeDiscoveryVm, CoreError> {
        let client = {
            let accounts = self.accounts.lock().await;
            accounts.get(account_id).map(|h| h.client.clone())
        };
        let Some(client) = client else {
            return Err(BridgeError::AccountNotFound(account_id.to_owned()).into());
        };
        Ok(crate::bridges::discover(&client).await?)
    }

    /// Project the `bbctl` self-host capability for the "Run your own bridge" surface
    /// (Story 6.7, FR-29). A pure map over the embedded `bbctl.json` (guided-install
    /// steps + the supported self-hostable networks, catalog-joined for display name)
    /// plus the live `runner.is_available()` probe. No I/O beyond the availability
    /// probe; carries only non-secret static data. A malformed embedded data file
    /// funnels through [`BridgeError`].
    pub fn bbctl_availability<R: crate::bridges::bbctl::BbctlRunner>(
        &self,
        runner: &R,
    ) -> Result<crate::vm::BbctlAvailabilityVm, CoreError> {
        let doc = crate::bridges::data::bbctl_doc()?;
        let catalog = crate::bridges::catalog().unwrap_or_default();
        let name_for = |network_id: &str| -> String {
            catalog
                .iter()
                .find(|n| n.network_id == network_id)
                .map(|n| n.name.clone())
                .unwrap_or_else(|| network_id.to_owned())
        };
        let networks = doc
            .networks()
            .into_iter()
            .map(|n| crate::vm::BbctlNetworkVm {
                network_id: n.network_id.clone(),
                name: name_for(&n.network_id),
                bbctl_name: n.bbctl_name.clone(),
            })
            .collect();
        Ok(crate::vm::BbctlAvailabilityVm {
            available: runner.is_available(),
            install: crate::vm::BbctlInstallVm {
                steps: doc.install.steps.clone(),
                docs_url: doc.install.docs_url.clone(),
            },
            networks,
        })
    }

    /// Gate a `bbctl` self-hosted-bridge run and resolve the target network (Story
    /// 6.7, FR-29, AD-16) — defense in depth for the frontend's Beeper-only gate.
    ///
    /// Reads the account's **durable, non-secret** [`Provider`](crate::vm::Provider)
    /// from the registry (never a token) via `platform`; a non-Beeper account is
    /// refused with an honest [`BridgeError::Bbctl`]. Then support-gates `network_id`
    /// against the embedded `bbctl.json` supported set. On success returns the
    /// resolved [`BbctlNetwork`](crate::bridges::data::BbctlNetwork) — the IPC command
    /// owns the actual `run_self_hosted` spawn (so the streaming task is `'static` and
    /// cancelable), and this gate always runs before any spawn.
    pub fn bbctl_run_start(
        &self,
        platform: &Arc<dyn Platform>,
        account_id: &str,
        network_id: &str,
    ) -> Result<crate::bridges::data::BbctlNetwork, CoreError> {
        let data_dir = platform.data_dir()?;
        let row = registry::get_account(&data_dir, account_id)?.ok_or_else(|| {
            BridgeError::Bbctl(
                "Running your own bridge is available for Beeper accounts only".to_owned(),
            )
        })?;
        let is_beeper = row
            .provider
            .as_deref()
            .and_then(crate::vm::Provider::from_registry_str)
            == Some(crate::vm::Provider::Beeper);
        if !is_beeper {
            return Err(BridgeError::Bbctl(
                "Running your own bridge is available for Beeper accounts only".to_owned(),
            )
            .into());
        }

        let doc = crate::bridges::data::bbctl_doc()?;
        let network = doc
            .support_for(network_id)
            .filter(|n| n.supported)
            .ok_or_else(|| {
                BridgeError::Bbctl(format!(
                    "{network_id} can't be self-hosted from keeper right now"
                ))
            })?;
        Ok(network)
    }

    /// Subscribe to live bridge-session health across every active account (Story 6.5,
    /// FR-28, NFR-6, AD-16). Bootstraps the monitored (logged-in) sessions from each
    /// live account's discovery pass, spawns a per-account [`HealthMonitor`] (mgmt-room
    /// notice handler + bounded liveness tick) feeding one shared [`HealthAggregator`],
    /// emits the initial snapshot, and returns the subscription id. The stream then
    /// emits **only on a per-session state change** (diffed). Tears down any prior
    /// subscription first. A per-account discovery / monitor failure is logged and
    /// skipped (others keep monitoring) — never fatal to the whole subscription.
    pub async fn subscribe_bridge_health(&self, sink: BridgeHealthSink) -> u64 {
        // Serialize the whole subscribe body against any concurrent subscribe so the
        // drain→build→store sequence is atomic — otherwise two overlapping subscribes
        // both spawn monitors and the loser's leak (never drained). Held to return.
        let _subscribe_guard = self.bridge_health_subscribe.lock().await;

        // Only one health subscription at a time: drain any prior one.
        self.unsubscribe_bridge_health_inner().await;

        // Resolve the live clients under the lock (cheap handle clones), then run
        // discovery + monitor spawn outside it (Matrix I/O).
        let clients: Vec<(String, Client)> = {
            let accounts = self.accounts.lock().await;
            accounts
                .iter()
                .map(|(id, handle)| (id.clone(), handle.client.clone()))
                .collect()
        };

        // Bootstrap the monitored (logged-in) sessions across accounts, resolving each
        // Network's display name from the catalog. Only `LoggedIn` sessions have health
        // (a `NotLoggedIn`/`Configured` bridge is not monitored).
        let catalog = crate::bridges::catalog().unwrap_or_default();
        let name_for = |network_id: &str| -> String {
            catalog
                .iter()
                .find(|n| n.network_id == network_id)
                .map(|n| n.name.clone())
                .unwrap_or_else(|| network_id.to_owned())
        };

        let mut sessions_by_account: HashMap<String, Vec<MonitoredSession>> = HashMap::new();
        let mut all_sessions: BTreeMap<SessionKey, MonitoredSession> = BTreeMap::new();
        for (account_id, client) in &clients {
            let discovery = match crate::bridges::discover(client).await {
                Ok(discovery) => discovery,
                Err(e) => {
                    tracing::warn!(
                        account_id = %account_id,
                        error = %e,
                        "health: discovery failed for account; skipping (others keep monitoring)"
                    );
                    continue;
                }
            };
            let mut sessions = Vec::new();
            for discovered in &discovery.networks {
                if discovered.status != crate::vm::BridgeStatus::LoggedIn {
                    continue;
                }
                let session = MonitoredSession {
                    account_id: account_id.clone(),
                    network_id: discovered.network_id.clone(),
                    network_name: name_for(&discovered.network_id),
                    state: HealthState::new_healthy(),
                };
                all_sessions.insert(
                    (account_id.clone(), discovered.network_id.clone()),
                    session.clone(),
                );
                sessions.push(session);
            }
            sessions_by_account.insert(account_id.clone(), sessions);
        }

        let aggregator = HealthAggregator::new(sink, all_sessions);
        // Emit the bootstrap snapshot (the stream always opens with the current set).
        aggregator.emit_initial();

        // Spawn a per-account monitor over each account's sessions.
        let mut monitors: HashMap<String, HealthMonitor> = HashMap::new();
        for (account_id, client) in clients {
            let sessions = sessions_by_account.remove(&account_id).unwrap_or_default();
            if sessions.is_empty() {
                continue;
            }
            let monitor =
                HealthMonitor::spawn(client, account_id.clone(), &sessions, aggregator.clone())
                    .await;
            monitors.insert(account_id, monitor);
        }

        let subscription_id = NEXT_SUBSCRIPTION_ID.fetch_add(1, Ordering::Relaxed);
        {
            let mut health = self.bridge_health.lock().await;
            *health = Some(BridgeHealthHandle {
                subscription_id,
                aggregator,
                monitors,
            });
        }
        tracing::info!(subscription_id, "bridge health subscribed");
        subscription_id
    }

    /// Unsubscribe the bridge-health subscription for `subscription_id`, draining every
    /// per-account monitor. Idempotent — a mismatched/unknown id is a no-op.
    pub async fn unsubscribe_bridge_health(&self, subscription_id: u64) {
        let matches = {
            let health = self.bridge_health.lock().await;
            health
                .as_ref()
                .is_some_and(|h| h.subscription_id == subscription_id)
        };
        if matches {
            self.unsubscribe_bridge_health_inner().await;
            tracing::info!(subscription_id, "bridge health unsubscribed");
        }
    }

    /// Tear down any active bridge-health subscription: drain every monitor (abort its
    /// tick + drop its handlers) and drop the handle. Idempotent.
    async fn unsubscribe_bridge_health_inner(&self) {
        let handle = {
            let mut health = self.bridge_health.lock().await;
            health.take()
        };
        if let Some(handle) = handle {
            for (_, monitor) in handle.monitors {
                monitor.drain();
            }
        }
    }

    /// Drain one account's health monitor and drop its sessions from the shared
    /// aggregator (called from `shutdown` so a signed-out account's sessions leave the
    /// health snapshot immediately). Best-effort — a no-active-subscription case is a
    /// harmless no-op.
    async fn drain_account_health(&self, account_id: &str) {
        let mut health = self.bridge_health.lock().await;
        if let Some(handle) = health.as_mut() {
            if let Some(monitor) = handle.monitors.remove(account_id) {
                monitor.drain();
            }
            handle.aggregator.remove_account(account_id);
        }
    }

    /// Start a native bridge login for `network_id` on a live account (Story 6.3,
    /// FR-26, AD-16). Resolves the account's live `Client`, reads its server name
    /// and Matrix access token, connects a [`Provisioning`] transport (the
    /// data-driven base-URL probe), then spawns [`login::drive_login`] to stream a
    /// [`crate::vm::BridgeLoginVm`] state machine into `sink`. Returns the
    /// `session_id` used to submit input / cancel.
    ///
    /// The access token is read here and handed *only* to the transport as a Bearer
    /// header — it never crosses IPC (only rendered VM state reaches the frontend).
    /// A missing account surfaces [`BridgeError::AccountNotFound`]; an unreachable
    /// provisioning API surfaces [`BridgeError::Provisioning`] (both before the task
    /// spawns). The driver self-reaps its session entry on natural completion.
    pub async fn start_bridge_login(
        &self,
        account_id: &str,
        network_id: &str,
        sink: BridgeLoginSink,
    ) -> Result<u64, CoreError> {
        // Resolve the live client + its login-session registry under the lock.
        let (client, sessions) = {
            let accounts = self.accounts.lock().await;
            match accounts.get(account_id) {
                Some(handle) => (handle.client.clone(), handle.login_sessions.clone()),
                None => return Err(BridgeError::AccountNotFound(account_id.to_owned()).into()),
            }
        };

        // The provisioning API is served alongside the account's homeserver C-S API,
        // and the Bearer token is a client-server access token that belongs to that
        // homeserver. So the probe host is the RESOLVED homeserver host — never the
        // bare MXID `server_name`, which under `.well-known` delegation can resolve to
        // a different host operated by another party that must never receive the
        // token (auth.rs keeps the same resolved-homeserver discipline). Both the host
        // and token are read here and never leave the transport.
        let homeserver = client.homeserver();
        let provisioning_host = homeserver.host_str().ok_or_else(|| {
            BridgeError::Provisioning("account homeserver has no host".to_owned())
        })?;
        let provisioning_host = match homeserver.port() {
            Some(port) => format!("{provisioning_host}:{port}"),
            None => provisioning_host.to_owned(),
        };
        let token = client.access_token().ok_or_else(|| {
            BridgeError::Provisioning("account has no live access token".to_owned())
        })?;

        // Select the transport, provisioning-first with a Bridge Bot fallback (Story
        // 6.4, AD-16). Probe the provisioning base URL before spawning so a genuine
        // provisioning error surfaces synchronously as the command's error (never a
        // silent bot fallback): `Ok(Some)` = drive with Provisioning; `Ok(None)` = no
        // provisioning API here, build the BotDriver fallback; `Err` = a real
        // transport error, surfaced.
        let transport = match Provisioning::connect(&provisioning_host, &token, network_id).await? {
            Some(provisioning) => LoginTransport::Provisioning(provisioning),
            None => {
                // No provisioning API — resolve/create the Bridge Bot DM and drive
                // over chat. An unresolvable bot surfaces `BridgeError::Bot` here,
                // before the task spawns (no silent fallback to an unknown bot).
                let (room, bot_mxid) =
                    crate::bridges::resolve_bot_room(&client, network_id).await?;
                let protocol = crate::bridges::data::bot_commands()?.protocol_for(network_id);
                LoginTransport::Bot(BotDriver::new(client.clone(), room, bot_mxid, protocol))
            }
        };
        // A clone is kept on the session so an explicit cancel / shutdown drain can
        // best-effort cancel the server-side login (POST `/login/cancel` or the bot's
        // cancel command) after the driver task (which owns the original) is aborted.
        let cancel_transport = transport.clone();

        let session_id = NEXT_SUBSCRIPTION_ID.fetch_add(1, Ordering::Relaxed);
        let (input_tx, input_rx) = mpsc::unbounded_channel::<BridgeLoginInput>();
        // The shared slot the driver populates with the login id after start, read
        // by cancel to know which server-side login to cancel.
        let login_id: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));
        let driver_login_id = login_id.clone();
        let network_owned = network_id.to_owned();
        let account_owned = account_id.to_owned();
        let reaper_sessions = sessions.clone();
        let span =
            tracing::info_span!("bridge_login", account_id = %account_id, network_id = %network_id);
        // Branch the spawn on the transport arm: `drive_login` is generic and
        // statically dispatched, so each arm monomorphizes a distinct instantiation —
        // the driver, `step_to_vm`, and the VM are identical either way.
        let task = tokio::spawn(
            async move {
                match transport {
                    LoginTransport::Provisioning(t) => {
                        login::drive_login(t, &network_owned, sink, input_rx, driver_login_id)
                            .await;
                    }
                    LoginTransport::Bot(t) => {
                        login::drive_login(t, &network_owned, sink, input_rx, driver_login_id)
                            .await;
                    }
                }
                // A naturally-completed login reaps its own session entry. It does
                // NOT cancel the server-side login — only an explicit cancel does.
                reaper_sessions.lock().await.remove(&session_id);
                tracing::info!(account_id = %account_owned, session_id, "bridge login ended");
            }
            .instrument(span),
        );

        sessions.lock().await.insert(
            session_id,
            LoginSession {
                task,
                input_tx,
                transport: cancel_transport,
                login_id,
            },
        );
        tracing::info!(account_id = %account_id, session_id, "bridge login started");
        Ok(session_id)
    }

    /// Push a [`BridgeLoginInput`] (a flow choice or field values) into a running
    /// bridge-login session (Story 6.3). A stale / unknown `session_id` surfaces
    /// [`BridgeError::Provisioning`] (the session already ended); a missing account
    /// surfaces [`BridgeError::AccountNotFound`].
    pub async fn submit_bridge_login(
        &self,
        account_id: &str,
        session_id: u64,
        input: BridgeLoginInput,
    ) -> Result<(), CoreError> {
        let accounts = self.accounts.lock().await;
        let handle = accounts
            .get(account_id)
            .ok_or_else(|| BridgeError::AccountNotFound(account_id.to_owned()))?;
        let sessions = handle.login_sessions.lock().await;
        let session = sessions.get(&session_id).ok_or_else(|| {
            BridgeError::Provisioning("no active bridge-login session for this id".to_owned())
        })?;
        session.input_tx.send(input).map_err(|_| {
            BridgeError::Provisioning("bridge-login session already ended".to_owned())
        })?;
        Ok(())
    }

    /// Cancel a running bridge-login session (Story 6.3): remove it from the
    /// registry, best-effort POST `/login/cancel/{login_id}` on the retained
    /// transport clone (if the login id has resolved), then abort the driver task.
    /// The server cancel is spawned detached so it never blocks the abort. Only this
    /// explicit path posts cancel — a naturally-completed login does not. Idempotent
    /// — a missing session / account is a silent no-op.
    pub async fn cancel_bridge_login(&self, account_id: &str, session_id: u64) {
        let sessions = {
            let accounts = self.accounts.lock().await;
            accounts.get(account_id).map(|h| h.login_sessions.clone())
        };
        let Some(sessions) = sessions else {
            return;
        };
        let removed = sessions.lock().await.remove(&session_id);
        if let Some(session) = removed {
            // Best-effort cancel the login before aborting the task (detached so it
            // never blocks the abort; `login_cancel` logs+swallows all errors). The
            // bot arm cancels even with no recorded id (its login command was already
            // sent), so a Sheet-close during a slow first reply still cancels.
            let login_id = session.login_id.lock().ok().and_then(|guard| guard.clone());
            let t = session.transport.clone();
            tokio::spawn(async move {
                t.cancel_recorded(login_id).await;
            });
            // Dropping the input sender closes the driver's input channel; aborting
            // the task stops any in-flight long-poll immediately.
            session.task.abort();
            tracing::info!(account_id = %account_id, session_id, "bridge login cancelled");
        }
    }

    /// Resolve-or-create the Bridge Bot DM room for `network_id` on a live account
    /// (Story 6.4, FR-27, UX-DR19) and return its room id, so the frontend can
    /// navigate straight to the raw Bridge Bot chat (the manual escape hatch from the
    /// card Manage menu / a login failure). A missing account surfaces
    /// [`BridgeError::AccountNotFound`]; an unresolvable / uncreatable bot DM surfaces
    /// [`BridgeError::Bot`]. No bot MXID or session material crosses back — only the
    /// non-secret room id.
    pub async fn bridge_bot_room(
        &self,
        account_id: &str,
        network_id: &str,
    ) -> Result<String, CoreError> {
        let client = {
            let accounts = self.accounts.lock().await;
            match accounts.get(account_id) {
                Some(handle) => handle.client.clone(),
                None => return Err(BridgeError::AccountNotFound(account_id.to_owned()).into()),
            }
        };
        let (room, _bot_mxid) = crate::bridges::resolve_bot_room(&client, network_id).await?;
        Ok(room.room_id().to_string())
    }

    /// The data-driven new-chat resolve capability for `network_id` (Story 6.6,
    /// FR-32): a pure projection of `resolve-support.json` (override-or-default) into
    /// a [`crate::vm::ResolveSupportVm`]. Account-agnostic and I/O-free — the frontend
    /// disables the identifier field upfront when `supported` is `false`, before any
    /// resolve call. A malformed embedded data file surfaces [`BridgeError::Data`].
    pub fn bridge_resolve_support(
        &self,
        network_id: &str,
    ) -> Result<crate::vm::ResolveSupportVm, CoreError> {
        let support = crate::bridges::data::resolve_support()?.support_for(network_id);
        Ok(crate::vm::ResolveSupportVm {
            network_id: network_id.to_owned(),
            supported: support.supported,
            identifier_hint: support.identifier_hint,
            placeholder: support.placeholder,
        })
    }

    /// Resolve a new-chat `identifier` on `network_id` through the bridge's
    /// provisioning API (Story 6.6, FR-32) and return the portal room id to open.
    ///
    /// Reuses the [`start_bridge_login`](Self::start_bridge_login) host/token
    /// derivation (resolved-homeserver host + port + the account's Matrix access
    /// token as Bearer — read here, never crossing IPC) and [`Provisioning::connect`].
    /// Then: `resolve_identifier` first (validates cheaply, may return an existing
    /// DM); only if no DM exists yet does it `create_dm` (avoids creating a portal for
    /// a typo'd identifier). The returned room id is opened verbatim — keeper never
    /// scans joined rooms.
    ///
    /// A missing account surfaces [`BridgeError::AccountNotFound`]. A bot-only account
    /// (`Provisioning::connect` → `Ok(None)`) surfaces an honest
    /// [`BridgeError::Provisioning`] naming the Bridge Bot chat (no fabricated resolve
    /// from a bot's prose). A resolve/create error surfaces the bridge's own message
    /// verbatim so the dialog renders "Not found on {Network}" with the input retained.
    pub async fn resolve_bridge_identifier(
        &self,
        account_id: &str,
        network_id: &str,
        identifier: &str,
    ) -> Result<crate::vm::NewChatResolutionVm, CoreError> {
        // Defense-in-depth at the IPC boundary: the dialog disables Start on an empty
        // identifier (Story 6.6 I/O matrix, frontend pure validation), but guard here
        // too so an empty path segment never reaches the bridge's `resolve_identifier`
        // route (undefined behavior there) — honest failure over a late/undefined one.
        if identifier.trim().is_empty() {
            return Err(BridgeError::Provisioning("identifier is empty".to_owned()).into());
        }

        // Resolve the live client under the accounts lock (AccountNotFound on miss).
        let client = {
            let accounts = self.accounts.lock().await;
            match accounts.get(account_id) {
                Some(handle) => handle.client.clone(),
                None => return Err(BridgeError::AccountNotFound(account_id.to_owned()).into()),
            }
        };

        // Same resolved-homeserver-host + Bearer-token discipline as start_bridge_login:
        // the provisioning API is served alongside the account's homeserver C-S API, so
        // the probe host is the RESOLVED homeserver host (never the bare MXID
        // server_name), and the Bearer token belongs to that homeserver. Both are read
        // here and never leave the transport.
        let homeserver = client.homeserver();
        let provisioning_host = homeserver.host_str().ok_or_else(|| {
            BridgeError::Provisioning("account homeserver has no host".to_owned())
        })?;
        let provisioning_host = match homeserver.port() {
            Some(port) => format!("{provisioning_host}:{port}"),
            None => provisioning_host.to_owned(),
        };
        let token = client.access_token().ok_or_else(|| {
            BridgeError::Provisioning("account has no live access token".to_owned())
        })?;

        // Bot-only accounts have no honest structured resolve (a bot reply is prose, no
        // room id) — surface the honest "use the Bridge Bot chat" error rather than
        // fabricate a resolve. `Err` is a genuine transport error, surfaced verbatim.
        let provisioning = match Provisioning::connect(&provisioning_host, &token, network_id)
            .await?
        {
            Some(provisioning) => provisioning,
            None => {
                return Err(BridgeError::Provisioning(format!(
                        "Starting a chat from keeper needs the provisioning API for {network_id}; open the Bridge Bot chat"
                    ))
                    .into());
            }
        };

        // Two structured calls, no guessing: resolve first (may return an existing DM),
        // create only when no DM exists yet.
        let room_id = match provisioning.resolve_identifier(identifier).await? {
            Some(room_id) => room_id,
            None => provisioning.create_dm(identifier).await?,
        };
        Ok(crate::vm::NewChatResolutionVm { room_id })
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

    /// List every pending draft across all accounts for the approval pane (Story
    /// 7.3). Reads the full draft rows from `keeper.db` ([`registry::list_draft_rows`])
    /// and enriches each with the owning account's `user_id`/`hue_index` (from the
    /// registry) and the room's `display_name` + bridge `network` (best-effort via
    /// the live `Room`).
    ///
    /// **Never hide a draft** (the airlock invariant): metadata resolution is
    /// best-effort. When a row's account is offline or its room is not resolvable,
    /// the row is still emitted with `display_name = room_id` and `network = None`.
    /// A draft whose account is missing from the registry falls back to `user_id =
    /// account_id` and `hue_index = 0`. Bodies stay authoritative in Rust and are
    /// never logged.
    pub async fn list_pending_drafts(
        &self,
        platform: &Arc<dyn Platform>,
    ) -> Result<Vec<ApprovalDraftVm>, CoreError> {
        let data_dir = platform.data_dir()?;
        let rows = registry::list_draft_rows(&data_dir)?;
        // Index the registry accounts by id for the identity/hue join.
        let accounts: HashMap<String, registry::AccountRow> = registry::list_accounts(&data_dir)?
            .into_iter()
            .map(|row| (row.account_id.clone(), row))
            .collect();

        let mut out = Vec::with_capacity(rows.len());
        for (account_id, room_id, body, updated_ts) in rows {
            let (account_user_id, hue_index) = match accounts.get(&account_id) {
                Some(row) => (row.user_id.clone(), row.hue_index.unwrap_or(0)),
                None => (account_id.clone(), 0),
            };
            // Best-effort room metadata: parse + resolve the live room. Any failure
            // (offline account, unknown room) falls back to room_id / no network —
            // the row is still emitted.
            let (display_name, network) = match RoomId::parse(&room_id) {
                Ok(parsed) => match self.room_for(&account_id, &parsed).await {
                    Ok(room) => {
                        let name = resolved_room_name(&room, &room_id).await;
                        let network = bridge::room_bridge_network(&room).await;
                        (name, network)
                    }
                    Err(_) => (room_id.clone(), None),
                },
                Err(_) => (room_id.clone(), None),
            };
            out.push(ApprovalDraftVm {
                account_id,
                account_user_id,
                hue_index,
                room_id,
                display_name,
                network,
                body,
                updated_ts,
            });
        }
        Ok(out)
    }

    /// Approve (send) a pending draft's `body` to `room_id`/`account_id` through the
    /// single dispatch gate (FR-41, AD-13, Story 7.3) with the
    /// [`SendTrigger::ApprovalPaneApprove`] trigger — the second and last legal
    /// dispatch trigger. Delegates to [`send::submit`] with the approval trigger; no
    /// new dispatch path or public send API is introduced.
    ///
    /// The Approval Pane is a standalone primary view where the target room's
    /// conversation is NOT open, so [`open_timeline_for`] (open-conversation-only)
    /// alone would return [`SendError::NoOpenTimeline`] for essentially every draft.
    /// This therefore acquires the `Timeline` with the same "reuse-open-else-transient
    /// -build" pattern [`mark_room_read`] (Story 4.1) uses: reuse an already-open
    /// timeline when present, otherwise build a transient
    /// `TimelineBuilder::new(&room).build()` from [`room_for`] — obtaining a
    /// `Timeline` off the open-subscription path without introducing a new dispatch
    /// path (the single-gate boundary holds).
    ///
    /// Errors: an unparsable room id, or an account/room that isn't live (via
    /// [`room_for`]) → [`SendError::RoomNotFound`]/[`TimelineError::RoomNotFound`]; a
    /// transient-build failure → [`TimelineError::Build`]; an SDK enqueue failure →
    /// [`SendError::Dispatch`]. On any error the caller retains the draft (a failed
    /// send never loses unsent text).
    pub async fn send_approval(
        &self,
        account_id: &str,
        room_id: &str,
        body: &str,
    ) -> Result<(), CoreError> {
        // Guard whitespace-only bodies before any timeline work: `send::submit`
        // treats a trim-empty body as a silent no-op `Ok(())`, which would let the
        // frontend clear the draft — destroying unsent text. Surface a typed error
        // instead so the caller's catch retains the draft (the airlock never
        // destroys held text).
        if body.trim().is_empty() {
            return Err(SendError::EmptyBody.into());
        }
        let room_id: OwnedRoomId = RoomId::parse(room_id).map_err(|_| SendError::RoomNotFound)?;
        let room = self.room_for(account_id, &room_id).await?;

        // Acquire the dispatch `Timeline`. Reuse an already-open conversation timeline
        // when present; otherwise build a transient one — the Approval Pane opens no
        // conversation, so this is the normal path (mirrors `mark_room_read`).
        let timeline = match self.open_timeline_for(account_id, &room_id).await {
            Ok(timeline) => timeline,
            Err(_) => Arc::new(
                TimelineBuilder::new(&room)
                    .build()
                    .await
                    .map_err(|e| TimelineError::Build(e.to_string()))?,
            ),
        };
        send::submit(&timeline, body, SendTrigger::ApprovalPaneApprove).await?;
        tracing::info!(account_id = %account_id, room_id = %room_id, "draft approved and dispatched");
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

    /// Read the edit history of the message addressed by `item_key` from the Local
    /// Archive (Story 5.2, FR-11) — never a fresh homeserver fetch.
    ///
    /// Resolves the opaque `item_key` to the message's *original* `event_id` via
    /// the live `Timeline` (matrix-sdk aggregates edits onto the original item, so
    /// this is the chain's join key), then reads the version chain from
    /// `archive.db` and maps each row to an [`EditVersionVm`]: the original's
    /// display text from its top-level `body`, an edit's from `m.new_content.body`
    /// (falling back to the top-level `body`). Versions are ordered oldest→newest
    /// with the last flagged `is_current`. When "honor remote deletions locally"
    /// is enabled, redacted versions are dropped from the result (FR-36). An
    /// unresolvable item, a missing room / timeline, or an empty chain yields an
    /// empty vec — the caller returns `Ok(vec![])` (no history is not an error).
    pub async fn edit_history(
        &self,
        platform: &Arc<dyn Platform>,
        account_id: &str,
        room_id: &str,
        item_key: &str,
    ) -> Result<Vec<EditVersionVm>, CoreError> {
        let Ok(room_id) = RoomId::parse(room_id) else {
            return Ok(Vec::new());
        };
        // Resolve item_key → the original event id via the live timeline. A missing
        // room / timeline / unresolvable item is simply "no history".
        let event_id = {
            let Ok(timeline) = self.open_timeline_for(account_id, &room_id).await else {
                return Ok(Vec::new());
            };
            let items = timeline.items().await;
            let resolved = items
                .iter()
                .find(|item| item.unique_id().0 == item_key)
                .and_then(|item| item.as_event())
                .and_then(|ev| ev.event_id().map(|id| id.to_string()));
            match resolved {
                Some(id) => id,
                None => return Ok(Vec::new()),
            }
        };
        // Read the archive version chain (read-only, off the writer path).
        let data_dir = platform.data_dir()?;
        let conn = archive::db::open_archive_db(&data_dir)?;
        let chain = archive::db::edit_chain(&conn, account_id, &event_id)?;
        // Honor the "honor remote deletions locally" policy on this retrieval
        // surface (FR-36): when enabled, a redacted version is not retrievable, so
        // it is dropped from the popover. Content stays physically on disk either
        // way — marking never erases. Same gate as `archive::db::retrievable_content`.
        let honor_deletions = archive::get_honor_remote_deletions(&data_dir)?;
        Ok(visible_versions(chain, honor_deletions))
    }

    /// Resolve a search hit's `event_id` to the opaque timeline render key
    /// (`unique_id`) so the frontend can deep-link into the timeline at the matched
    /// message (Story 5.4, FR-34). This is the *inverse* of the reply/edit/reaction
    /// resolution: those take an opaque `item_key` and find the event id; this takes
    /// an `event_id` (the sanctioned deep-link handle returned on `SearchHitVm`) and
    /// finds the loaded item's `unique_id`. Crucially, `event_id` is an **input**
    /// only — no event id is ever added to a streamed timeline VM, so the
    /// `TimelineItemVm` no-event-id invariant (NFR-9, AD-1) holds.
    ///
    /// Scans the live `Arc<Timeline>` (the same accessor as `submit_reply` /
    /// `toggle_reaction`) for the event item whose `event_id()` equals the parsed
    /// input, returning `Some(unique_id)` on a hit or `None` when the event is not in
    /// the currently-loaded window (the caller then best-effort paginates and
    /// retries, or degrades honestly). No event id crosses back — only the opaque
    /// render key.
    ///
    /// Errors: an unparsable room id or event id → [`TimelineError::RoomNotFound`]
    /// (mapped to a retriable `TimelineUnavailable`, never a panic); no open timeline
    /// for the room → `Ok(None)` (the Chat may still be opening — the caller retries).
    pub async fn resolve_timeline_event_key(
        &self,
        account_id: &str,
        room_id: &str,
        event_id: &str,
    ) -> Result<Option<String>, CoreError> {
        let room_id: OwnedRoomId =
            RoomId::parse(room_id).map_err(|_| TimelineError::RoomNotFound)?;
        // Parse the event id up front so a malformed handle is an honest typed error,
        // never a silent miss (and never a panic).
        let event_id = EventId::parse(event_id).map_err(|_| TimelineError::RoomNotFound)?;
        // The timeline may not be open yet (the Chat is still mounting): that is a
        // transient "not loaded", so surface `None` rather than an error.
        let Ok(timeline) = self.open_timeline_for(account_id, &room_id).await else {
            return Ok(None);
        };
        let items = timeline.items().await;
        // `unique_id()` lives on the outer `TimelineItem`; `event_id()` on its
        // `EventTimelineItem` view — so match on the event id and return the outer
        // item's opaque render key (the same key the timeline stream emits).
        let resolved = items
            .iter()
            .find(|item| item.as_event().and_then(|ev| ev.event_id()) == Some(&event_id))
            .map(|item| item.unique_id().0.clone());
        if resolved.is_some() {
            tracing::info!(account_id = %account_id, room_id = %room_id, "search deep-link event resolved to render key");
        }
        Ok(resolved)
    }

    /// Redact (delete for everyone) the message addressed by `item_key` (its
    /// opaque render key) on `room_id`/`account_id` through the single dispatch
    /// gate (FR-15, FR-41, AD-13, Story 3.8). Resolves the live `Arc<Timeline>`
    /// and delegates to [`send::redact`]; the `Set` diff that turns the message
    /// into a redacted stub in place arrives through the room's existing timeline
    /// subscription — this synthesizes nothing. `reason` is an optional non-secret
    /// redaction reason (the IPC command passes `None`).
    ///
    /// Errors: an unparsable/unknown room id → [`SendError::RoomNotFound`]; no open
    /// timeline → [`SendError::NoOpenTimeline`]; an unresolvable target →
    /// [`SendError::TargetNotFound`]; an SDK dispatch failure → [`SendError::Dispatch`].
    pub async fn redact_message(
        &self,
        account_id: &str,
        room_id: &str,
        item_key: &str,
        reason: Option<&str>,
    ) -> Result<(), CoreError> {
        let room_id: OwnedRoomId = RoomId::parse(room_id).map_err(|_| SendError::RoomNotFound)?;
        let timeline = self.open_timeline_for(account_id, &room_id).await?;
        send::redact(&timeline, item_key, reason).await?;
        tracing::info!(account_id = %account_id, room_id = %room_id, "message redaction dispatched");
        Ok(())
    }

    /// Resolve the bridged-Chat Network label for `room_id`/`account_id` on demand,
    /// for the delete confirmation's honest framing (FR-15, UX-DR17, Story 3.8).
    /// Resolves the account's live `Room` and delegates to
    /// [`bridge::room_bridge_network`], which reads the MSC2346 `m.bridge` (and
    /// legacy `uk.half-shot.bridge`) state event and returns the Network's display
    /// name — "Telegram", "WhatsApp", … A native Matrix Room (no bridge state)
    /// resolves to `None`, so the confirmation uses native framing. Only the
    /// resolved, non-secret label crosses back (NFR-9); no `RoomVm` is touched.
    ///
    /// Errors: an unparsable/unknown room id, or an account that isn't live →
    /// [`TimelineError::RoomNotFound`].
    pub async fn room_network_label(
        &self,
        account_id: &str,
        room_id: &str,
    ) -> Result<Option<String>, CoreError> {
        let room_id: OwnedRoomId =
            RoomId::parse(room_id).map_err(|_| TimelineError::RoomNotFound)?;
        let client = {
            let accounts = self.accounts.lock().await;
            let handle = accounts
                .get(account_id)
                .ok_or(TimelineError::RoomNotFound)?;
            handle.client.clone()
        };
        let room = client
            .get_room(&room_id)
            .ok_or(TimelineError::RoomNotFound)?;
        Ok(bridge::room_bridge_network(&room).await)
    }

    /// Toggle the account's emoji `emoji` reaction on the message addressed by
    /// `item_key` (its opaque render key) on `room_id`/`account_id` through the
    /// single dispatch gate (FR-41, AD-13, Story 3.5, FR-12). Resolves the live
    /// `Arc<Timeline>` and delegates to [`send::toggle_reaction`]; the updated
    /// reaction set arrives as a `Set` diff through the room's existing timeline
    /// subscription — this synthesizes nothing. The toggle is symmetric: it adds
    /// the reaction if absent and retracts it if the account already reacted.
    ///
    /// Errors: an unparsable/unknown room id → [`SendError::RoomNotFound`]; no open
    /// timeline → [`SendError::NoOpenTimeline`]; an unresolvable target →
    /// [`SendError::TargetNotFound`]; an SDK dispatch failure → [`SendError::Dispatch`].
    pub async fn toggle_reaction(
        &self,
        account_id: &str,
        room_id: &str,
        item_key: &str,
        emoji: &str,
    ) -> Result<(), CoreError> {
        let room_id: OwnedRoomId = RoomId::parse(room_id).map_err(|_| SendError::RoomNotFound)?;
        let timeline = self.open_timeline_for(account_id, &room_id).await?;
        send::toggle_reaction(&timeline, item_key, emoji).await?;
        tracing::info!(account_id = %account_id, room_id = %room_id, "reaction toggled");
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

    /// Send a media attachment from an OS file path to `room_id` on `account_id`
    /// through the single dispatch gate (FR-13, FR-41, AD-4, AD-13, Story 3.7).
    ///
    /// Reads the file with `tokio::fs::read` (bytes never cross IPC — the file path
    /// is the ingestion boundary for the composer attach button + native drag-drop),
    /// derives the display filename from the path, guesses the MIME type from the
    /// extension (`application/octet-stream` fallback), and delegates to
    /// [`send::submit_attachment`]. The local echo + every send-state transition
    /// arrive over the room's existing timeline subscription — this synthesizes
    /// nothing. Logs the opaque room id, media kind (mime top-level), and byte size
    /// only — never the path or file bytes.
    ///
    /// Errors: an unparsable/unknown room id → [`SendError::RoomNotFound`]; no open
    /// timeline → [`SendError::NoOpenTimeline`]; a file that can't be read →
    /// [`SendError::Upload`]; an SDK enqueue failure → [`SendError::Upload`].
    pub async fn send_attachment_path(
        &self,
        account_id: &str,
        room_id: &str,
        path: &Path,
        caption: Option<&str>,
    ) -> Result<(), CoreError> {
        let room_id: OwnedRoomId = RoomId::parse(room_id).map_err(|_| SendError::RoomNotFound)?;
        let timeline = self.open_timeline_for(account_id, &room_id).await?;
        let bytes = tokio::fs::read(path)
            .await
            .map_err(|e| SendError::Upload(format!("could not read the attached file: {e}")))?;
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| "attachment".to_owned());
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        let size = bytes.len();
        send::submit_attachment(&timeline, bytes, &filename, mime.clone(), caption).await?;
        tracing::info!(
            account_id = %account_id,
            room_id = %room_id,
            kind = %mime.type_(),
            size,
            "attachment dispatched from path"
        );
        Ok(())
    }

    /// Send a media attachment from raw bytes (a path-less pasted clipboard image)
    /// to `room_id` on `account_id` through the single dispatch gate (FR-13, FR-41,
    /// AD-4, AD-13, Story 3.7).
    ///
    /// The bytes arrive over a **raw binary IPC body** (never base64/JSON — the
    /// sanctioned exception for path-less pastes); `mime_str` is the caller-supplied
    /// MIME (parsed, `application/octet-stream` fallback on a malformed value).
    /// Delegates to [`send::submit_attachment`]; the local echo + send-state
    /// transitions arrive over the room's existing timeline subscription. Logs the
    /// opaque room id, media kind, and byte size only — never the bytes.
    ///
    /// Errors: an unparsable/unknown room id → [`SendError::RoomNotFound`]; no open
    /// timeline → [`SendError::NoOpenTimeline`]; an SDK enqueue failure →
    /// [`SendError::Upload`].
    pub async fn send_attachment_bytes(
        &self,
        account_id: &str,
        room_id: &str,
        bytes: Vec<u8>,
        filename: &str,
        mime_str: &str,
        caption: Option<&str>,
    ) -> Result<(), CoreError> {
        let room_id: OwnedRoomId = RoomId::parse(room_id).map_err(|_| SendError::RoomNotFound)?;
        let timeline = self.open_timeline_for(account_id, &room_id).await?;
        let mime = mime::Mime::from_str(mime_str).unwrap_or(mime::APPLICATION_OCTET_STREAM);
        let size = bytes.len();
        // Defense-in-depth: the paste filename is frontend-supplied, so reduce it to
        // its final path component — a compromised webview cannot inject directory
        // separators into the sent event's filename. (The path route already derives
        // the name via `Path::file_name`.)
        let safe_name = Path::new(filename)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment");
        send::submit_attachment(&timeline, bytes, safe_name, mime.clone(), caption).await?;
        tracing::info!(
            account_id = %account_id,
            room_id = %room_id,
            kind = %mime.type_(),
            size,
            "attachment dispatched from pasted bytes"
        );
        Ok(())
    }

    /// Cancel an in-flight outgoing media (or text) echo by aborting its SDK send
    /// handle — best-effort, symmetric with [`AccountManager::retry_send`] (Story
    /// 3.7). `item_key` is the echo's opaque `unique_id`. Resolves the same live
    /// `Arc<Timeline>` that produced the item and delegates to [`send::cancel`]; the
    /// echo's removal (or its already-sent no-op) arrives over the room's existing
    /// timeline subscription.
    ///
    /// Errors: unknown room / no open timeline as [`send_text`]; a missing echo →
    /// [`SendError::EchoNotFound`].
    pub async fn cancel_send(
        &self,
        account_id: &str,
        room_id: &str,
        item_key: &str,
    ) -> Result<(), CoreError> {
        let room_id: OwnedRoomId = RoomId::parse(room_id).map_err(|_| SendError::RoomNotFound)?;
        let timeline = self.open_timeline_for(account_id, &room_id).await?;
        send::cancel(&timeline, item_key).await?;
        tracing::info!(account_id = %account_id, room_id = %room_id, "outgoing send cancel requested");
        Ok(())
    }

    /// Mark the room read on `account_id` — dispatch a read receipt on the room's
    /// latest event through the receipt/typing signals seam (Story 3.9, 4.1, AD-14)
    /// and clear any manual `m.marked_unread` flag. The receipt is public (`m.read`)
    /// or private (`m.read.private`) per the effective Incognito policy resolved at
    /// emission time from the registry scopes (Story 8.1); a scope-read failure falls
    /// back to the public path (best-effort). Works for any inbox
    /// row whether or not its timeline is open (Story 4.1): reuses an already-open
    /// `Arc<Timeline>` when present, else builds a transient
    /// `TimelineBuilder::new(&room).build()` purely to advance the receipt through
    /// [`signals::mark_read`] (keeping the mark-as-read call inside the signals seam).
    /// The manual-unread flag is account data, not a receipt API, so
    /// [`matrix_sdk::Room::set_unread_flag`] is cleared here. Best-effort: every
    /// dispatch failure is logged and swallowed (returns `Ok(())`) so a failure is
    /// never a UI error; other clients simply don't observe the advance until the
    /// next successful mark.
    ///
    /// Errors: an unparsable/unknown room id, or an account that isn't live →
    /// [`TimelineError::RoomNotFound`]. A best-effort receipt- or flag-dispatch
    /// failure is NOT an error — it is logged and swallowed.
    pub async fn mark_room_read(
        &self,
        platform: &Arc<dyn Platform>,
        account_id: &str,
        room_id: &str,
    ) -> Result<(), CoreError> {
        let room_id: OwnedRoomId =
            RoomId::parse(room_id).map_err(|_| TimelineError::RoomNotFound)?;
        let room = self.room_for(account_id, &room_id).await?;

        // Resolve the effective Incognito policy at emission time (Story 8.1, AD-14):
        // read the three scope values from the registry and let the `signals` resolver
        // apply Chat > Account > Global. If the registry read fails we cannot know the
        // policy — fail CLOSED: skip the receipt entirely rather than risk leaking a
        // PUBLIC `m.read` for a chat the user meant to keep private. The read position
        // simply doesn't advance to the remote until the next successful mark; this is
        // best-effort and never a UI error. Logged at `warn` because a silent privacy
        // downgrade would be the worse failure for a privacy feature.
        let policy = match platform
            .data_dir()
            .and_then(|dir| registry::incognito_scopes(&dir, account_id, room_id.as_str()))
        {
            Ok((chat, account, global)) => Some(signals::resolve_incognito(chat, account, global)),
            Err(e) => {
                tracing::warn!(account_id = %account_id, room_id = %room_id, error = %e, "incognito scope read failed; skipping receipt to avoid a public leak (fail-closed)");
                None
            }
        };

        // Advance the read receipt through the signals seam (AD-14). Reuse an
        // already-open timeline when present; otherwise build a transient one just
        // to dispatch the receipt — the mark-as-read call stays in signals.rs.
        let timeline = match self.open_timeline_for(account_id, &room_id).await {
            Ok(timeline) => Some(timeline),
            Err(_) => match TimelineBuilder::new(&room).build().await {
                Ok(timeline) => Some(Arc::new(timeline)),
                Err(e) => {
                    tracing::debug!(account_id = %account_id, room_id = %room_id, error = %e, "transient timeline build for mark-read failed (best-effort)");
                    None
                }
            },
        };
        // Only dispatch when the policy is known (fail-closed above). A `None` policy
        // means the scope read failed, so we deliberately emit nothing this pass.
        if let (Some(timeline), Some(policy)) = (timeline, policy) {
            match signals::mark_read(&timeline, policy).await {
                Ok(marked) => {
                    if marked {
                        tracing::debug!(account_id = %account_id, room_id = %room_id, "room marked read");
                    }
                }
                // Best-effort (I/O matrix): a receipt-dispatch failure is logged and
                // swallowed — never surfaced as a UI error.
                Err(e) => {
                    tracing::debug!(account_id = %account_id, room_id = %room_id, error = %e, "mark-read dispatch failed (best-effort)");
                }
            }
        }

        // Clear the manual `m.marked_unread` account-data flag unconditionally. This
        // is not a receipt API, so it lives here rather than in the signals seam. The
        // SDK no-ops the write when the flag is already unset, so we skip the racy
        // `is_marked_unread()` gate (which could miss a flag another client set
        // concurrently after our read) and let the SDK decide.
        if let Err(e) = room.set_unread_flag(false).await {
            tracing::debug!(account_id = %account_id, room_id = %room_id, error = %e, "clear marked-unread flag failed (best-effort)");
        }
        Ok(())
    }

    /// Read the resolved Incognito state for `(account_id, room_id)` (Story 8.1).
    ///
    /// Reads the three registry scope values and applies the `signals` resolver
    /// (Chat over Account over Global), projecting the resolved effective on/off, the
    /// deciding `source`, and the raw scope values into an [`IncognitoVm`] the frontend
    /// renders directly (never re-resolving precedence on the frontend).
    pub fn incognito_get(
        &self,
        platform: &Arc<dyn Platform>,
        account_id: &str,
        room_id: &str,
    ) -> Result<IncognitoVm, CoreError> {
        let data_dir = platform.data_dir()?;
        let (chat, account, global) = registry::incognito_scopes(&data_dir, account_id, room_id)?;
        let policy = signals::resolve_incognito(chat, account, global);
        Ok(IncognitoVm {
            effective: policy.enabled,
            source: policy.source,
            global,
            account,
            chat,
        })
    }

    /// Read the global Incognito default (Story 8.1). Absent = off (Incognito off by
    /// default). Reads the `settings` k/v table key `incognito.global`.
    pub fn incognito_get_global(&self, platform: &Arc<dyn Platform>) -> Result<bool, CoreError> {
        let data_dir = platform.data_dir()?;
        registry::get_incognito_global(&data_dir)
    }

    /// Set the global Incognito default (Story 8.1). Persists into the `settings`
    /// k/v table under `incognito.global`; off by default.
    pub fn incognito_set_global(
        &self,
        platform: &Arc<dyn Platform>,
        enabled: bool,
    ) -> Result<(), CoreError> {
        let data_dir = platform.data_dir()?;
        registry::set_incognito_global(&data_dir, enabled)
    }

    /// Read the per-Account Incognito override for `account_id` (Story 8.1). `None` =
    /// inherit the global scope; `Some(bool)` = an explicit per-Account override. Reads
    /// the nullable `accounts.incognito` column.
    pub fn incognito_get_account(
        &self,
        platform: &Arc<dyn Platform>,
        account_id: &str,
    ) -> Result<Option<bool>, CoreError> {
        let data_dir = platform.data_dir()?;
        registry::get_incognito_account(&data_dir, account_id)
    }

    /// Set (or clear) the per-Account Incognito override for `account_id` (Story
    /// 8.1). `Some(bool)` sets an explicit override; `None` clears it back to inherit
    /// the global scope. Writes the `accounts.incognito` column.
    pub fn incognito_set_account(
        &self,
        platform: &Arc<dyn Platform>,
        account_id: &str,
        value: Option<bool>,
    ) -> Result<(), CoreError> {
        let data_dir = platform.data_dir()?;
        registry::set_incognito_account(&data_dir, account_id, value)
    }

    /// Set (or clear) the per-Chat Incognito override for `(account_id, room_id)`
    /// (Story 8.1). `Some(bool)` upserts an explicit override; `None` clears it back
    /// to inherit the account/global scope. Writes the `chat_incognito` table.
    pub fn incognito_set_chat(
        &self,
        platform: &Arc<dyn Platform>,
        account_id: &str,
        room_id: &str,
        enabled: Option<bool>,
    ) -> Result<(), CoreError> {
        let data_dir = platform.data_dir()?;
        registry::set_incognito_chat(&data_dir, account_id, room_id, enabled)
    }

    /// Manually mark the room unread on `account_id` — set the `m.marked_unread`
    /// account-data flag via [`matrix_sdk::Room::set_unread_flag`] (Story 4.1).
    /// Resolves the account's live `Room` via [`Self::room_for`]. The SDK no-ops
    /// the write when the flag is already set. This is account data, not a receipt
    /// API, so it lives here rather than in the signals seam. Best-effort: a
    /// dispatch failure is logged and swallowed (returns `Ok(())`) — never a UI
    /// error.
    ///
    /// Errors: an unparsable/unknown room id, or an account that isn't live →
    /// [`TimelineError::RoomNotFound`]. A best-effort flag-dispatch failure is NOT
    /// an error — it is logged and swallowed.
    pub async fn mark_room_unread(&self, account_id: &str, room_id: &str) -> Result<(), CoreError> {
        let room_id: OwnedRoomId =
            RoomId::parse(room_id).map_err(|_| TimelineError::RoomNotFound)?;
        let room = self.room_for(account_id, &room_id).await?;
        if let Err(e) = room.set_unread_flag(true).await {
            tracing::debug!(account_id = %account_id, room_id = %room_id, error = %e, "mark-unread dispatch failed (best-effort)");
        }
        Ok(())
    }

    /// Archive the room on `account_id` — set the Matrix low-priority tag
    /// (`m.lowpriority`) via [`matrix_sdk::Room::set_is_low_priority`] (Story 4.2).
    /// Resolves the account's live `Room` via [`Self::room_for`]. The tag persists
    /// across relaunch and syncs to the user's other Matrix clients; the merge
    /// then moves the row into the Archive window (unless it is unread). This is
    /// account data (a tag), not a receipt API, so it lives here rather than in the
    /// signals seam. Best-effort: a dispatch failure is logged and swallowed
    /// (returns `Ok(())`) — never a UI error.
    ///
    /// Errors: an unparsable/unknown room id, or an account that isn't live →
    /// [`TimelineError::RoomNotFound`]. A best-effort tag-dispatch failure is NOT
    /// an error — it is logged and swallowed.
    pub async fn archive_room(&self, account_id: &str, room_id: &str) -> Result<(), CoreError> {
        let room_id: OwnedRoomId =
            RoomId::parse(room_id).map_err(|_| TimelineError::RoomNotFound)?;
        let room = self.room_for(account_id, &room_id).await?;
        if let Err(e) = room.set_is_low_priority(true, None).await {
            tracing::debug!(account_id = %account_id, room_id = %room_id, error = %e, "archive dispatch failed (best-effort)");
        }
        Ok(())
    }

    /// Unarchive the room on `account_id` — clear the Matrix low-priority tag
    /// (`m.lowpriority`) via [`matrix_sdk::Room::set_is_low_priority`] (Story 4.2).
    /// Resolves the account's live `Room` via [`Self::room_for`]. The merge then
    /// returns the row to its chronological Inbox position. This is account data (a
    /// tag), not a receipt API, so it lives here rather than in the signals seam.
    /// Best-effort: a dispatch failure is logged and swallowed (returns `Ok(())`) —
    /// never a UI error.
    ///
    /// Errors: an unparsable/unknown room id, or an account that isn't live →
    /// [`TimelineError::RoomNotFound`]. A best-effort tag-dispatch failure is NOT
    /// an error — it is logged and swallowed.
    pub async fn unarchive_room(&self, account_id: &str, room_id: &str) -> Result<(), CoreError> {
        let room_id: OwnedRoomId =
            RoomId::parse(room_id).map_err(|_| TimelineError::RoomNotFound)?;
        let room = self.room_for(account_id, &room_id).await?;
        if let Err(e) = room.set_is_low_priority(false, None).await {
            tracing::debug!(account_id = %account_id, room_id = %room_id, error = %e, "unarchive dispatch failed (best-effort)");
        }
        Ok(())
    }

    /// Favourite the room on `account_id` — set the Matrix favourite tag
    /// (`m.favourite`) via [`matrix_sdk::Room::set_is_favourite`] (Story 4.4).
    /// Resolves the account's live `Room` via [`Self::room_for`]. `m.favourite` is
    /// a *notable* tag, so this re-emits the room-list stream live (no out-of-band
    /// merger poke) and the tag persists across relaunch and syncs to the user's
    /// other Matrix clients; the merge then moves the row into the Favorites
    /// window. The SDK makes favourite and low-priority mutually exclusive
    /// (`set_is_favourite(true)` auto-clears `m.lowpriority`). This is account data
    /// (a tag), not a receipt API, so it lives here rather than in the signals
    /// seam. Best-effort: a dispatch failure is logged and swallowed (returns
    /// `Ok(())`) — never a UI error.
    ///
    /// Errors: an unparsable/unknown room id, or an account that isn't live →
    /// [`TimelineError::RoomNotFound`]. A best-effort tag-dispatch failure is NOT
    /// an error — it is logged and swallowed.
    pub async fn favourite_room(&self, account_id: &str, room_id: &str) -> Result<(), CoreError> {
        let room_id: OwnedRoomId =
            RoomId::parse(room_id).map_err(|_| TimelineError::RoomNotFound)?;
        let room = self.room_for(account_id, &room_id).await?;
        if let Err(e) = room.set_is_favourite(true, None).await {
            tracing::debug!(account_id = %account_id, room_id = %room_id, error = %e, "favourite dispatch failed (best-effort)");
        }
        Ok(())
    }

    /// Unfavourite the room on `account_id` — clear the Matrix favourite tag
    /// (`m.favourite`) via [`matrix_sdk::Room::set_is_favourite`] (Story 4.4).
    /// Resolves the account's live `Room` via [`Self::room_for`]. The merge then
    /// returns the row to its chronological Inbox position. This is account data (a
    /// tag), not a receipt API, so it lives here rather than in the signals seam.
    /// Best-effort: a dispatch failure is logged and swallowed (returns `Ok(())`) —
    /// never a UI error.
    ///
    /// Errors: an unparsable/unknown room id, or an account that isn't live →
    /// [`TimelineError::RoomNotFound`]. A best-effort tag-dispatch failure is NOT
    /// an error — it is logged and swallowed.
    pub async fn unfavourite_room(&self, account_id: &str, room_id: &str) -> Result<(), CoreError> {
        let room_id: OwnedRoomId =
            RoomId::parse(room_id).map_err(|_| TimelineError::RoomNotFound)?;
        let room = self.room_for(account_id, &room_id).await?;
        if let Err(e) = room.set_is_favourite(false, None).await {
            tracing::debug!(account_id = %account_id, room_id = %room_id, error = %e, "unfavourite dispatch failed (best-effort)");
        }
        Ok(())
    }

    /// Pin the room on `account_id` (Story 4.3, FR-22). Pins are keeper-local: this
    /// appends the ref at the end of the ordered list (`max(existing)+1`), persists
    /// it to the registry, then reloads [`registry::get_pins`] and pushes the whole
    /// map into the live merger via [`InboxMerger::update_pins`] — one code path
    /// keeps the in-memory map and disk in sync, and re-emits all three windows so
    /// the strip updates within one frame. Best-effort: a no-active-subscription
    /// case is a harmless no-op re-emit; a registry write error surfaces.
    pub async fn pin_room(
        &self,
        data_dir: &Path,
        account_id: &str,
        room_id: &str,
    ) -> Result<(), CoreError> {
        // Append at the end of the global order (`max+1`, or 0 when empty).
        let next_order = registry::get_pins(data_dir)?
            .iter()
            .map(|(_, _, o)| *o)
            .max()
            .map(|m| m + 1)
            .unwrap_or(0);
        registry::set_pin(data_dir, account_id, room_id, next_order)?;
        self.reload_pins(data_dir).await?;
        tracing::info!(account_id = %account_id, room_id = %room_id, "room pinned");
        Ok(())
    }

    /// Unpin the room on `account_id` (Story 4.3). Removes the ref from the registry
    /// (idempotent), then reloads the pin map into the merger so the row returns to
    /// its chronological Inbox (or Archive) position. Best-effort re-emit as in
    /// [`Self::pin_room`].
    pub async fn unpin_room(
        &self,
        data_dir: &Path,
        account_id: &str,
        room_id: &str,
    ) -> Result<(), CoreError> {
        registry::remove_pin(data_dir, account_id, room_id)?;
        self.reload_pins(data_dir).await?;
        tracing::info!(account_id = %account_id, room_id = %room_id, "room unpinned");
        Ok(())
    }

    /// Reorder the pins to the exact `order` given (Story 4.3): rewrite the full
    /// ordered ref list to contiguous `0..n` in the registry, then reload the pin
    /// map into the merger so the Pins window re-emits in the new order. Refs not
    /// currently pinned are written anyway (upsert) so the frontend's authoritative
    /// order always wins; the registry is left contiguous and consistent.
    pub async fn reorder_pins(
        &self,
        data_dir: &Path,
        order: &[(String, String)],
    ) -> Result<(), CoreError> {
        for (index, (account_id, room_id)) in order.iter().enumerate() {
            registry::set_pin(data_dir, account_id, room_id, index as i64)?;
        }
        self.reload_pins(data_dir).await?;
        tracing::debug!(count = order.len(), "pins reordered");
        Ok(())
    }

    /// Reload the pin map from the registry and push it into the live merger, if
    /// any (Story 4.3). A no-op re-emit when no inbox subscription is active — the
    /// next `subscribe_inbox` seeds from the same registry.
    async fn reload_pins(&self, data_dir: &Path) -> Result<(), CoreError> {
        let pins = load_pins(data_dir)?;
        let inbox = self.inbox.lock().await;
        if let Some(handle) = inbox.as_ref() {
            handle.merger.update_pins(pins).await;
        }
        Ok(())
    }

    /// Set (or clear) the ephemeral Space filter on the live merger (Story 4.5).
    /// `selection` is `Some((account_id, space_id))` to narrow every inbox window
    /// to that Space's joined children, or `None` to restore the full inbox.
    /// Mirrors [`Self::reorder_pins`]'s poke-the-merger shape: a no-active-inbox
    /// case is a harmless no-op (the filter is ephemeral, so nothing to persist).
    pub async fn set_space_filter(&self, selection: Option<(String, String)>) {
        let inbox = self.inbox.lock().await;
        if let Some(handle) = inbox.as_ref() {
            handle.merger.set_space_filter(selection).await;
        }
    }

    /// Set (or clear) the ephemeral Network filter on the live merger (Story 4.6).
    /// `network` is `Some(name)` to narrow every inbox window to rooms bridged to
    /// that Network (across all accounts — the selection is name-keyed), or `None`
    /// to restore the full inbox. Composes AND with any active Space filter.
    /// Mirrors [`Self::set_space_filter`]'s poke-the-merger shape: a no-active-inbox
    /// case is a harmless no-op (the filter is ephemeral, so nothing to persist).
    pub async fn set_network_filter(&self, network: Option<String>) {
        let inbox = self.inbox.lock().await;
        if let Some(handle) = inbox.as_ref() {
            handle.merger.set_network_filter(network).await;
        }
    }

    /// Set (or clear) the account's typing notice in the room through the
    /// receipt/typing signals seam, gated on the effective Incognito policy (Story 3.9,
    /// 8.2, AD-14, FR-43). Resolves the account's live `Room`, reads the three registry
    /// scope values, and lets the `signals` resolver apply Chat > Account > Global, then
    /// passes the resolved policy to [`signals::set_typing`] — which emits nothing (start
    /// or stop) while Incognito applies, so zero `m.typing` events leave the machine.
    ///
    /// Scope read is **fail-closed** (mirroring [`Self::mark_room_read`]): a registry read
    /// error skips emission entirely rather than risk leaking a typing notice for a chat
    /// the user meant to keep private (logged at `warn`). Best-effort: a dispatch failure
    /// is logged and swallowed (returns `Ok(())`) — typing is never a UI error.
    ///
    /// Errors: an unparsable/unknown room id, or an account that isn't live →
    /// [`TimelineError::RoomNotFound`]. A best-effort typing-dispatch failure is NOT
    /// an error — it is logged and swallowed.
    pub async fn set_typing(
        &self,
        platform: &Arc<dyn Platform>,
        account_id: &str,
        room_id: &str,
        typing: bool,
    ) -> Result<(), CoreError> {
        let room_id: OwnedRoomId =
            RoomId::parse(room_id).map_err(|_| TimelineError::RoomNotFound)?;
        let room = self.room_for(account_id, &room_id).await?;

        // Resolve the effective Incognito policy at emission time (Story 8.1/8.2, AD-14),
        // fail-closed like `mark_room_read`: if the registry read fails we cannot know the
        // policy, so we SKIP emission rather than risk leaking a typing notice for a chat
        // the user meant to keep private. Logged at `warn` because a silent privacy
        // downgrade would be the worse failure for a privacy feature.
        let policy = match platform
            .data_dir()
            .and_then(|dir| registry::incognito_scopes(&dir, account_id, room_id.as_str()))
        {
            Ok((chat, account, global)) => Some(signals::resolve_incognito(chat, account, global)),
            Err(e) => {
                tracing::warn!(account_id = %account_id, room_id = %room_id, error = %e, "incognito scope read failed; skipping typing to avoid a leak (fail-closed)");
                None
            }
        };

        // Only dispatch when the policy is known (fail-closed above). `signals::set_typing`
        // itself suppresses emission when Incognito is effective.
        if let Some(policy) = policy {
            if let Err(e) = signals::set_typing(&room, typing, policy).await {
                tracing::debug!(account_id = %account_id, room_id = %room_id, typing, error = %e, "typing dispatch failed (best-effort)");
            }
        }
        Ok(())
    }

    /// Release a PUBLIC read receipt on the room's latest event — the explicit,
    /// user-triggered "Mark read publicly" action (Story 8.2, AD-14, FR-45). A
    /// best-effort sibling of [`Self::mark_room_read`]: it reuses the same timeline
    /// open / transient-build pattern, then dispatches exactly one public `m.read`
    /// through [`signals::release_receipt`] — regardless of the effective Incognito
    /// policy, because the user chose to acknowledge. Best-effort: every dispatch
    /// failure is logged and swallowed (returns `Ok(())`) so a failure is never a UI
    /// error.
    ///
    /// Errors: an unparsable/unknown room id, or an account that isn't live →
    /// [`TimelineError::RoomNotFound`]. A best-effort receipt-dispatch failure is NOT
    /// an error — it is logged and swallowed.
    pub async fn release_receipt(&self, account_id: &str, room_id: &str) -> Result<(), CoreError> {
        let room_id: OwnedRoomId =
            RoomId::parse(room_id).map_err(|_| TimelineError::RoomNotFound)?;
        let room = self.room_for(account_id, &room_id).await?;

        // Reuse an already-open timeline when present; otherwise build a transient one
        // just to dispatch the receipt — the mark-as-read call stays inside signals.rs.
        let timeline = match self.open_timeline_for(account_id, &room_id).await {
            Ok(timeline) => Some(timeline),
            Err(_) => match TimelineBuilder::new(&room).build().await {
                Ok(timeline) => Some(Arc::new(timeline)),
                Err(e) => {
                    tracing::debug!(account_id = %account_id, room_id = %room_id, error = %e, "transient timeline build for release-receipt failed (best-effort)");
                    None
                }
            },
        };
        if let Some(timeline) = timeline {
            match signals::release_receipt(&timeline).await {
                Ok(released) => {
                    if released {
                        tracing::debug!(account_id = %account_id, room_id = %room_id, "public read receipt released");
                    }
                }
                Err(e) => {
                    tracing::debug!(account_id = %account_id, room_id = %room_id, error = %e, "release-receipt dispatch failed (best-effort)");
                }
            }
        }
        Ok(())
    }

    /// Back-paginate the room's live timeline by up to `num_events` older events
    /// (Story 3.9, pagination). Resolves the room's open `Arc<Timeline>` and
    /// delegates to [`timeline::paginate_backwards`]; the older events arrive over
    /// the room's existing timeline subscription (this synthesizes nothing).
    /// Returns whether the homeserver start of the room was reached.
    ///
    /// Errors: an unparsable/unknown room id, or a room with no open timeline →
    /// [`TimelineError::RoomNotFound`]; an SDK pagination failure →
    /// [`TimelineError::Build`] (both funnel to the retriable `TimelineUnavailable`
    /// code so the boundary shows a retriable inline error, not an infinite spinner).
    pub async fn paginate_backwards(
        &self,
        account_id: &str,
        room_id: &str,
        num_events: u16,
    ) -> Result<bool, CoreError> {
        // The webview is the trust boundary (AD-4): clamp the caller-provided count
        // to a sane page size so no renderer call (or future bug) can request an
        // outsized back-fill. The UI paginates in pages of `PAGINATE_BATCH` (40).
        let num_events = num_events.clamp(1, MAX_PAGINATE_EVENTS);
        let room_id: OwnedRoomId =
            RoomId::parse(room_id).map_err(|_| TimelineError::RoomNotFound)?;
        let timeline = self
            .open_timeline_for(account_id, &room_id)
            .await
            .map_err(|_| TimelineError::RoomNotFound)?;
        let hit_start = timeline::paginate_backwards(&timeline, num_events).await?;
        tracing::debug!(account_id = %account_id, room_id = %room_id, hit_start, "back-paginated");
        Ok(hit_start)
    }

    /// Subscribe to the room's typing notifications (Story 3.9, AD-14, AD-19).
    /// Resolves the account's live `Room`, spawns a supervised producer over the
    /// SDK typing broadcast (via [`signals::subscribe_typing`]) that resolves each
    /// typing member's display name (`room.get_member_no_sync`) and streams a
    /// [`TypingBatch`] into `sink`, and returns the subscription id.
    ///
    /// Mirrors [`subscribe_connection_status`]'s supervised-task + self-reap
    /// lifecycle: the `JoinHandle` registers in the account's `subscriptions` map
    /// and is aborted on unsubscribe / shutdown (AD-19). The SDK typing
    /// event-handler is dropped with the producer (its `EventHandlerDropGuard` is
    /// held for the producer's lifetime). A room-not-found / inactive account
    /// funnels to `TimelineUnavailable`.
    pub async fn subscribe_typing(
        &self,
        account_id: &str,
        room_id: &str,
        sink: TypingSink,
    ) -> Result<u64, CoreError> {
        let room_id: OwnedRoomId =
            RoomId::parse(room_id).map_err(|_| TimelineError::RoomNotFound)?;
        let (room, subs_arc) = {
            let accounts = self.accounts.lock().await;
            let handle = accounts
                .get(account_id)
                .ok_or(TimelineError::RoomNotFound)?;
            let room = handle
                .client
                .get_room(&room_id)
                .ok_or(TimelineError::RoomNotFound)?;
            (room, handle.subscriptions.clone())
        };

        let subscription_id = NEXT_SUBSCRIPTION_ID.fetch_add(1, Ordering::Relaxed);
        let account_id_owned = account_id.to_owned();
        let span =
            tracing::info_span!("typing_producer", account_id = %account_id, room_id = %room_id);
        let reaper_subs = subs_arc.clone();
        let task = tokio::spawn(
            async move {
                run_typing_producer(room, sink, &account_id_owned).await;
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
                    return Err(TimelineError::RoomNotFound.into());
                }
            }
        }
        tracing::info!(account_id = %account_id, subscription_id, room_id = %room_id, "typing subscribed");
        Ok(subscription_id)
    }

    /// Abort exactly the typing producer task for `subscription_id` (Story 3.9);
    /// other account state is untouched. Idempotent.
    pub async fn unsubscribe_typing(&self, account_id: &str, subscription_id: u64) {
        if self.abort_subscription(account_id, subscription_id).await {
            tracing::info!(account_id = %account_id, subscription_id, "typing unsubscribed");
        }
    }

    /// Subscribe to the room's live back-pagination status (Story 3.9, AD-19).
    /// Resolves the room's open `Arc<Timeline>` (the exact instance feeding the
    /// timeline subscription — the room must be open), spawns a supervised
    /// [`timeline::run_pagination_status_producer`] that streams a
    /// [`PaginationStatusBatch`] into `sink`, and returns the subscription id.
    ///
    /// Mirrors [`subscribe_connection_status`]'s supervised-task + self-reap
    /// lifecycle: the `JoinHandle` registers in the account's `subscriptions` map
    /// and is aborted on unsubscribe / shutdown (AD-19). A room-not-found / no-open-
    /// timeline funnels to `TimelineUnavailable`.
    pub async fn subscribe_pagination_status(
        &self,
        account_id: &str,
        room_id: &str,
        sink: PaginationSink,
    ) -> Result<u64, CoreError> {
        let room_id: OwnedRoomId =
            RoomId::parse(room_id).map_err(|_| TimelineError::RoomNotFound)?;
        let (timeline, subs_arc) = {
            let accounts = self.accounts.lock().await;
            let handle = accounts
                .get(account_id)
                .ok_or(TimelineError::RoomNotFound)?;
            let subs = handle.subscriptions.clone();
            drop(accounts);
            // Resolve the newest open `Arc<Timeline>` for the room (mirrors the
            // send path). The room must be open (its timeline subscribed).
            let timeline = self
                .open_timeline_for(account_id, &room_id)
                .await
                .map_err(|_| TimelineError::RoomNotFound)?;
            (timeline, subs)
        };

        let subscription_id = NEXT_SUBSCRIPTION_ID.fetch_add(1, Ordering::Relaxed);
        let span = tracing::info_span!(
            "pagination_status_producer",
            account_id = %account_id,
            room_id = %room_id
        );
        let reaper_subs = subs_arc.clone();
        let task = tokio::spawn(
            async move {
                timeline::run_pagination_status_producer(timeline, sink).await;
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
                    return Err(TimelineError::RoomNotFound.into());
                }
            }
        }
        tracing::info!(account_id = %account_id, subscription_id, room_id = %room_id, "pagination status subscribed");
        Ok(subscription_id)
    }

    /// Abort exactly the pagination-status producer task for `subscription_id`
    /// (Story 3.9); other account state is untouched. Idempotent.
    pub async fn unsubscribe_pagination_status(&self, account_id: &str, subscription_id: u64) {
        if self.abort_subscription(account_id, subscription_id).await {
            tracing::info!(account_id = %account_id, subscription_id, "pagination status unsubscribed");
        }
    }

    /// Resolve the account's live `Room` for `room_id`, or a typed
    /// [`TimelineError::RoomNotFound`] when the account isn't live or the room is
    /// unknown. Used by the typing signal (which needs a `Room`, not a `Timeline`).
    async fn room_for(
        &self,
        account_id: &str,
        room_id: &RoomId,
    ) -> Result<matrix_sdk::Room, CoreError> {
        let accounts = self.accounts.lock().await;
        let handle = accounts
            .get(account_id)
            .ok_or(TimelineError::RoomNotFound)?;
        handle
            .client
            .get_room(room_id)
            .ok_or_else(|| TimelineError::RoomNotFound.into())
    }

    /// Mirror `body` for `(account_id, room_id)` to the account (Story 7.2,
    /// AD-15): the synced `dev.keeper.draft` account-data event plus a best-effort
    /// `save_composer_draft` (Element interop). Resolves the live `Room` via
    /// [`room_for`] and delegates to [`drafts::mirror_draft`], which dedupes by
    /// last-mirrored body and generates the `updated_ts` at write time.
    ///
    /// Best-effort: every error (an inactive account, an unknown room, a rejecting
    /// server) is returned for the caller to swallow and log — it must never block
    /// or fail local persistence. The body is never logged.
    pub async fn mirror_draft(
        &self,
        account_id: &str,
        room_id: &str,
        body: &str,
    ) -> Result<(), CoreError> {
        let room_id: OwnedRoomId =
            RoomId::parse(room_id).map_err(|_| TimelineError::RoomNotFound)?;
        let room = self.room_for(account_id, &room_id).await?;
        drafts::mirror_draft(account_id, &room, body)
            .await
            .map_err(|e| CoreError::Internal(e.to_string()))
    }

    /// Clear `(account_id, room_id)`'s draft mirror (Story 7.2): tombstone the
    /// `dev.keeper.draft` account-data event plus `clear_composer_draft`. Resolves
    /// the live `Room` via [`room_for`] and delegates to
    /// [`drafts::clear_draft_mirror`].
    ///
    /// Best-effort: every error is returned for the caller to swallow and log; a
    /// failed clear can transiently re-present a cleared draft cross-device, which
    /// re-*shows* recoverable text and never destroys it. The body is never logged.
    pub async fn clear_draft_mirror(
        &self,
        account_id: &str,
        room_id: &str,
    ) -> Result<(), CoreError> {
        let room_id: OwnedRoomId =
            RoomId::parse(room_id).map_err(|_| TimelineError::RoomNotFound)?;
        let room = self.room_for(account_id, &room_id).await?;
        drafts::clear_draft_mirror(account_id, &room)
            .await
            .map_err(|e| CoreError::Internal(e.to_string()))
    }

    /// Read `(account_id, room_id)`'s remote draft from the account-data mirror
    /// (Story 7.2), or `None` when there is no draft (an empty-body tombstone maps
    /// to `None`). Resolves the live `Room` via [`room_for`] and delegates to
    /// [`drafts::load_remote_draft`].
    ///
    /// Read only to *offer* adoption — local always wins. A load failure is
    /// returned for the caller to swallow (the composer falls back to local). The
    /// body is never logged.
    pub async fn load_remote_draft(
        &self,
        account_id: &str,
        room_id: &str,
    ) -> Result<Option<RemoteDraftVm>, CoreError> {
        let room_id: OwnedRoomId =
            RoomId::parse(room_id).map_err(|_| TimelineError::RoomNotFound)?;
        let room = self.room_for(account_id, &room_id).await?;
        drafts::load_remote_draft(&room)
            .await
            .map_err(|e| CoreError::Internal(e.to_string()))
    }

    /// Subscribe to live remote draft edits across every account (Story 7.2,
    /// AD-15). Spawns a supervised relay over the manager's draft-mirror broadcast
    /// that forwards each observed [`DraftMirrorBatch`] into `sink`, and returns
    /// the subscription id.
    ///
    /// App-wide (not per account): the single relay drains the broadcast every
    /// account's `dev.keeper.draft` handler feeds. The `JoinHandle` registers in
    /// `draft_mirror_subs` and is aborted on `draft_mirror_unsubscribe`; a `Lagged`
    /// broadcast error skips to the newest value (drafts converge on bodies), and the
    /// relay task ends on its own when the sink closes or the broadcast sender is
    /// dropped (its finished handle is cleared by the eventual unsubscribe).
    pub async fn subscribe_draft_mirror(&self, sink: DraftMirrorSink) -> u64 {
        let mut receiver = self.draft_mirror_tx.subscribe();
        let subscription_id = NEXT_SUBSCRIPTION_ID.fetch_add(1, Ordering::Relaxed);
        let span = tracing::info_span!("draft_mirror_relay");
        let task = tokio::spawn(
            async move {
                loop {
                    match receiver.recv().await {
                        Ok(batch) => {
                            if !(sink)(batch) {
                                tracing::info!("draft mirror channel closed, stopping relay");
                                break;
                            }
                        }
                        // Lagged: skip to the newest edit on the next recv. Drafts
                        // converge on bodies, so a dropped intermediate is harmless.
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        // Sender dropped: no live accounts remain; end the relay.
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
            .instrument(span),
        );
        self.draft_mirror_subs
            .lock()
            .await
            .insert(subscription_id, task);
        tracing::info!(subscription_id, "draft mirror subscribed");
        subscription_id
    }

    /// Abort exactly the draft-mirror relay for `subscription_id` (Story 7.2).
    /// Idempotent — an unknown id is a no-op.
    pub async fn unsubscribe_draft_mirror(&self, subscription_id: u64) {
        if let Some(task) = self.draft_mirror_subs.lock().await.remove(&subscription_id) {
            task.abort();
            tracing::info!(subscription_id, "draft mirror unsubscribed");
        }
    }

    /// Resolve a `keeper-media://` handle to its decrypted bytes for the
    /// custom-protocol handler (Story 3.6, FR-13, AD-4). Resolves the account's
    /// live `Client` and the room's open `Arc<Timeline>`, then delegates to
    /// [`media::fetch_media`] — the sole `get_media_content` gate, which downloads
    /// and (for E2EE) decrypts the attachment from the SDK media cache.
    ///
    /// The decrypted bytes are returned to the protocol handler and served over
    /// `keeper-media://` — they never cross the IPC command surface (AD-4). Logs
    /// carry the opaque room id only — never a URL, key, or `mxc`.
    ///
    /// Errors: an unparsable/unknown room id, an account that isn't live, or a
    /// room with no open timeline → [`MediaError::NotFound`]; an unresolvable item
    /// / non-media item → [`MediaError::NotFound`]; an SDK fetch/decrypt failure →
    /// [`MediaError::Fetch`].
    pub async fn fetch_media(
        &self,
        account_id: &str,
        room_id: &str,
        item_key: &str,
        variant: MediaVariant,
    ) -> Result<MediaBytes, CoreError> {
        let room_id: OwnedRoomId = RoomId::parse(room_id).map_err(|_| MediaError::NotFound)?;
        let (client, timeline) = {
            let accounts = self.accounts.lock().await;
            let handle = accounts.get(account_id).ok_or(MediaError::NotFound)?;
            let timelines = handle.timelines.lock().await;
            // Pick the newest timeline instance for the room (a StrictMode remount
            // can transiently register two) — `unique_id`s are only stable within
            // one instance, mirroring `open_timeline_for`.
            let timeline = timelines
                .iter()
                .filter(|(_, (open_room, _))| *open_room == room_id)
                .max_by_key(|(subscription_id, _)| **subscription_id)
                .map(|(_, (_, tl))| tl.clone())
                .ok_or(MediaError::NotFound)?;
            (handle.client.clone(), timeline)
        };
        let handle = MediaHandle {
            account_id: account_id.to_owned(),
            room_id: room_id.to_string(),
            item_key: item_key.to_owned(),
            variant,
        };
        let bytes = media::fetch_media(&client, &timeline, &handle).await?;
        tracing::debug!(account_id = %account_id, room_id = %room_id, "media resolved for protocol");
        Ok(bytes)
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
        // Drain the account's bridge-health monitor first (Story 6.5): abort its tick,
        // remove its mgmt-room handlers (which hold `Client` clones), and drop its
        // sessions from the shared health snapshot — so a signed-out account's health
        // leaves the UI immediately and no handler leaks past teardown.
        self.drain_account_health(account_id).await;
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
                // Abort + await the account's Spaces producer too (Story 4.5): it
                // holds a `Client` clone, so it must be dropped before `sign_out`
                // deletes the store dir (same reasoning as the room-list producer).
                if let Some(task) = handle.spaces_producers.remove(account_id) {
                    task.abort();
                    let _ = task.await;
                }
            }
        }
        let mut accounts = self.accounts.lock().await;
        if let Some(handle) = accounts.remove(account_id) {
            // Remove the account-wide archive event handlers (Story 5.1/5.2) so no
            // further events are ingested and no redaction is marked after the
            // account goes down, and no handler (holding a `Client` clone) leaks
            // past teardown.
            handle.client.remove_event_handler(handle.archive_handler);
            handle.client.remove_event_handler(handle.redaction_handler);
            // Remove the `dev.keeper.draft` handler (Story 7.2) so no further remote
            // draft edits are observed after the account goes down and no handler
            // (holding a `Client` clone) leaks past teardown.
            handle.client.remove_event_handler(handle.draft_handler);
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
            // Abort any in-flight native bridge-login sessions (Stories 6.3/6.4) so
            // their driver tasks (holding a `Provisioning` or `BotDriver` transport)
            // are dropped. Best-effort cancel first (detached) so sign-out doesn't
            // leak a login: the provisioning arm POSTs `/login/cancel` when a login id
            // was recorded, the bot arm sends its cancel command even without one
            // (its login command may already be pending on the bot).
            for (_, session) in handle.login_sessions.lock().await.drain() {
                let login_id = session.login_id.lock().ok().and_then(|guard| guard.clone());
                let t = session.transport.clone();
                tokio::spawn(async move {
                    t.cancel_recorded(login_id).await;
                });
                session.task.abort();
            }
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

    /// Deliberately purge one account's local archive (Story 5.7, FR-6): its
    /// `events` rows and their `events_fts` entries, routed through the single
    /// serialized archive writer so it never competes with a second connection.
    /// Touches only the target account. Logged ids-only.
    ///
    /// When archiving is disabled (`archive: None`, e.g. `archive.db` could not be
    /// opened at startup) there is nothing on disk to purge, so this is `Ok(())`.
    pub async fn delete_account_archive(&self, account_id: &str) -> Result<(), CoreError> {
        match self.archive.clone() {
            Some(handle) => {
                let result = handle.delete_account(account_id).await;
                match &result {
                    Ok(()) => {
                        tracing::info!(account_id = %account_id, "account archive purged")
                    }
                    Err(e) => tracing::warn!(
                        account_id = %account_id,
                        error = %e,
                        "account archive purge failed"
                    ),
                }
                result.map_err(CoreError::from)
            }
            None => {
                tracing::info!(
                    account_id = %account_id,
                    "archive disabled; nothing to purge"
                );
                Ok(())
            }
        }
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
#[tracing::instrument(skip(platform, archive), fields(account_id = %account_id))]
async fn activate(
    platform: &Arc<dyn Platform>,
    account_id: &str,
    archive: Option<ArchiveHandle>,
    draft_mirror_tx: tokio::sync::broadcast::Sender<DraftMirrorBatch>,
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

    // Register the account-wide post-decryption archive handler (Story 5.1)
    // BEFORE the sync service starts, so no event delivered in the very first
    // sync batch can slip through a gap between `sync.start()` and handler
    // registration. It fires for every `m.room.message` the SDK delivers —
    // including decrypted content for encrypted rooms — regardless of whether a
    // room is open, so message history from the sync flow lands in `archive.db`
    // via the single serialized writer. Mapping never blocks sync (non-blocking
    // `ingest`).
    let archive_handler = register_archive_handler(&client, account_id, archive.clone());
    // Register the account-wide redaction handler (Story 5.2) alongside the
    // message handler and before sync starts, so a redaction in the first sync
    // batch is not missed. It marks the archived target row's `redacted_ts` via
    // the same single writer — marks only, never erases.
    let redaction_handler = register_redaction_handler(&client, account_id, archive);
    // Register the account-wide `dev.keeper.draft` room-account-data handler
    // (Story 7.2, AD-15) alongside the archive/redaction handlers and before sync
    // starts, so a remote draft edit in the first sync batch is observed. It maps
    // each observed edit to a `DraftMirrorBatch` and forwards it into the manager's
    // process broadcast for the app-wide `subscribe_draft_mirror` relay. The body
    // is never logged.
    let draft_handler = register_draft_handler(&client, account_id, draft_mirror_tx);

    // Archive-first back-pagination enablement (Story 5.6, FR-17). Subscribe the
    // SDK event cache once here — alongside the archive/redaction handlers and
    // BEFORE `sync.start()` — so every synced batch (from the very first one)
    // persists continuously into the on-disk `SqliteEventCacheStore` that
    // `.sqlite_store()` already provisions in the sdk dir (SPINE persisted-event-
    // cache storage rule: NFR-1–4 → event cache). `Timeline::paginate_backwards`
    // then serves older events from local disk first — instant and offline —
    // reaching the homeserver only at the true gap. `TimelineBuilder::build()`
    // would otherwise call this lazily on first room open, so without it only
    // rooms opened this session would persist; subscribing at activation closes
    // that gap for any Chat. `subscribe()` is idempotent (`get_or_init`), so the
    // later lazy call is a no-op.
    //
    // `subscribe()` spawns SDK-internal background tasks (room-updates writer,
    // auto-shrink, redecryptor, thread-subscriber) held on the `Client` and
    // aborted when its last clone drops — not by an explicit teardown in
    // `shutdown()`. On sign-out `shutdown()` stops sync first (awaited), which
    // quiesces the room-updates writer before `sign_out_cleanup` removes the sdk
    // dir; tightening that ordering for the event-cache tasks is tracked as
    // deferred work.
    //
    // Archive-first pagination is an enhancement, not a precondition for a usable
    // account: a subscribe failure degrades to homeserver-only back-pagination
    // rather than failing activation (mirrors the infallibly-registered
    // archive/redaction handlers). In matrix-sdk 0.18 this is effectively
    // unreachable — `subscribe()` only errors on an already-dropped client.
    if let Err(e) = client.event_cache().subscribe() {
        tracing::warn!(
            account_id = %account_id,
            error = %e,
            "event cache subscribe failed; falling back to homeserver-only pagination"
        );
    }

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

    Ok((
        client,
        sync,
        reconnect_supervisor,
        session_persister,
        archive_handler,
        redaction_handler,
        draft_handler,
    ))
}

/// Register the account-wide post-decryption archive event handler on `client`
/// and return its [`EventHandlerHandle`] (Story 5.1).
///
/// The handler fires for every `m.room.message` the SDK delivers post-decryption
/// (encrypted rooms included). For each original (non-redacted) message it builds
/// a normalized [`ArchiveEvent`] — event id, room id, sender, `origin_server_ts`
/// (ms), event type, content JSON, and media *metadata* — and hands it to the
/// single serialized writer via [`ArchiveHandle::ingest`] (non-blocking). When
/// archiving is disabled (`archive` is `None`) the handler is still registered but
/// does nothing, so activation shape is uniform.
fn register_archive_handler(
    client: &Client,
    account_id: &str,
    archive: Option<ArchiveHandle>,
) -> EventHandlerHandle {
    let account_id = account_id.to_owned();
    client.add_event_handler(move |ev: OriginalSyncRoomMessageEvent, room: Room| {
        let account_id = account_id.clone();
        let archive = archive.clone();
        async move {
            let Some(archive) = archive else {
                return;
            };
            match build_archive_event(&account_id, room.room_id().as_str(), &ev) {
                Ok(archive_event) => archive.ingest(archive_event),
                Err(e) => tracing::warn!(
                    account_id = %account_id,
                    event_id = %ev.event_id,
                    error = %e,
                    "archive: could not build event; dropping"
                ),
            }
        }
    })
}

/// Register the account-wide redaction event handler on `client` and return its
/// [`EventHandlerHandle`] (Story 5.2, FR-36).
///
/// The handler fires for every `m.room.redaction` the SDK delivers. It resolves
/// the redaction's *target* event id in a room-version-safe way — the id lives in
/// `content.redacts` in room versions ≥ 11 and at the top-level `redacts` field in
/// earlier versions — and marks the archived target row's `redacted_ts` through
/// the single serialized writer ([`ArchiveHandle::redact`], non-blocking). Marks
/// only, never erases; a target not in the archive is a swallowed zero-row update.
/// When archiving is disabled (`archive` is `None`) the handler is still
/// registered but does nothing, so activation shape is uniform.
fn register_redaction_handler(
    client: &Client,
    account_id: &str,
    archive: Option<ArchiveHandle>,
) -> EventHandlerHandle {
    let account_id = account_id.to_owned();
    client.add_event_handler(move |ev: OriginalSyncRoomRedactionEvent| {
        let account_id = account_id.clone();
        let archive = archive.clone();
        async move {
            let Some(archive) = archive else {
                return;
            };
            // Room-version-safe target resolution: v11+ carries `redacts` inside
            // `content`, earlier versions at the event top level. Prefer the
            // content field, fall back to the top-level one.
            let Some(target) = ev.content.redacts.as_ref().or(ev.redacts.as_ref()) else {
                tracing::warn!(
                    account_id = %account_id,
                    event_id = %ev.event_id,
                    "archive: redaction has no target event id; skipping mark"
                );
                return;
            };
            let redacted_ts = i64::from(ev.origin_server_ts.get());
            archive.redact(&account_id, target.as_str(), redacted_ts);
        }
    })
}

/// Register the account-wide `dev.keeper.draft` room-account-data handler on
/// `client` and return its [`EventHandlerHandle`] (Story 7.2, AD-15).
///
/// The handler fires for every `dev.keeper.draft` room-account-data event the SDK
/// delivers — **including this device's own echo**, which is dropped by matching the
/// event's `origin` device id against this client's own (room account data is
/// account-level and the SDK echoes a device's own write back to it, so without this
/// the user would be offered their own just-mirrored text as a bogus conflict). Every
/// other (genuinely remote) edit is forwarded into the manager's process broadcast as
/// a [`DraftMirrorBatch`] (empty body → tombstone). The frontend reconciles local-wins:
/// local text is never overwritten without a user tap. A closed broadcast (no live
/// relay) is a swallowed no-op. The body is never logged.
fn register_draft_handler(
    client: &Client,
    account_id: &str,
    draft_mirror_tx: tokio::sync::broadcast::Sender<DraftMirrorBatch>,
) -> EventHandlerHandle {
    let account_id = account_id.to_owned();
    let own_device = client
        .device_id()
        .map(|d| d.as_str().to_owned())
        .unwrap_or_default();
    client.add_event_handler(move |ev: drafts::KeeperDraftEvent, room: Room| {
        let account_id = account_id.clone();
        let draft_mirror_tx = draft_mirror_tx.clone();
        let own_device = own_device.clone();
        async move {
            // Drop this device's own echo: a non-empty origin equal to our device id
            // is a write we made, not a cross-device edit — never offer it back.
            if !own_device.is_empty() && ev.content.origin == own_device {
                return;
            }
            let batch = drafts::draft_mirror_batch(
                &account_id,
                room.room_id().as_str(),
                ev.content.body,
                ev.content.updated_ts,
            );
            // Best-effort: no live relay simply drops the batch (the next chat open
            // re-reconciles from account data). Never logs the body.
            let _ = draft_mirror_tx.send(batch);
            tracing::debug!(account_id = %account_id, room_id = %room.room_id(), "observed remote draft edit");
        }
    })
}

/// Build a normalized [`ArchiveEvent`] from a post-decryption
/// `m.room.message` event (Story 5.1). Pure over its inputs, so it is
/// unit-testable without a live `Client`.
///
/// Serializes the event content to JSON for `content_json`, converts
/// `origin_server_ts` to an i64 ms epoch, and extracts media *metadata* (never
/// bytes) from the message `msgtype`. Returns an error only if the content cannot
/// be serialized to JSON (surfaced by the caller as a swallowed, id-only log).
fn build_archive_event(
    account_id: &str,
    room_id: &str,
    ev: &OriginalSyncRoomMessageEvent,
) -> Result<ArchiveEvent, serde_json::Error> {
    let content_json = serde_json::to_string(&ev.content)?;
    // Extract an edit relation (Story 5.2): an `m.replace` targets the original
    // event, stored into queryable columns so the version chain can be read back.
    // Any other relation (a reply) or none is a plain message — no relation cols.
    let (relates_to_event_id, rel_type) = match ev.content.relates_to.as_ref() {
        Some(Relation::Replacement(r)) => {
            (Some(r.event_id.to_string()), Some("m.replace".to_owned()))
        }
        _ => (None, None),
    };
    // Extract the display body once, via the shared archive extractor (Story 5.3),
    // so ingest, edit-history, and the migration backfill never drift.
    let body = archive::display_body_from_content(&content_json);
    Ok(ArchiveEvent {
        account_id: account_id.to_owned(),
        event_id: ev.event_id.to_string(),
        room_id: room_id.to_owned(),
        sender: ev.sender.to_string(),
        origin_ts: i64::from(ev.origin_server_ts.get()),
        event_type: "m.room.message".to_owned(),
        content_json,
        body,
        media: archive_media(&ev.content.msgtype),
        relates_to_event_id,
        rel_type,
    })
}

/// Map an archive version chain (original first, edits by `origin_ts` ascending)
/// into the [`EditVersionVm`]s the edit-history popover renders (Story 5.2,
/// FR-11). Pure over its input, so it is unit-testable without a DB.
///
/// The original row's display text is its content's top-level `body`; an edit
/// row's is `m.new_content.body`, falling back to the top-level `body` when the
/// edit content lacks `m.new_content`. A row whose JSON cannot be parsed yields an
/// empty body (honest, never a panic). The version whose `event_id` matches
/// `current_event_id` is flagged `is_current`; when that version is absent (e.g.
/// honoring remote deletions dropped the newest one) no version is flagged.
/// Map a version chain to [`EditVersionVm`]s, applying the honor-remote-deletions
/// policy (FR-36): when `honor_deletions` is `true`, rows marked redacted
/// (`redacted_ts` set) are dropped so redacted versions are not retrievable via
/// the edit-history popover. When `false` (the default), every version is
/// returned. Content is never erased on disk regardless — this gates retrieval
/// only, matching [`crate::archive::db::retrievable_content`].
fn visible_versions(
    chain: Vec<crate::archive::db::StoredEvent>,
    honor_deletions: bool,
) -> Vec<EditVersionVm> {
    // The current version is the newest row of the *full* chain (`edit_chain`
    // orders original→newest), captured before filtering. If honoring remote
    // deletions drops the newest version, no survivor is the current message —
    // flagging an older survivor `is_current` would contradict the live timeline
    // and silently suppress that survivor from the popover's prior-versions list.
    let current_event_id = chain.last().map(|row| row.event_id.clone());
    let visible: Vec<_> = chain
        .into_iter()
        .filter(|row| !(honor_deletions && row.redacted_ts.is_some()))
        .collect();
    edit_versions_from_chain(&visible, current_event_id.as_deref())
}

fn edit_versions_from_chain(
    chain: &[crate::archive::db::StoredEvent],
    current_event_id: Option<&str>,
) -> Vec<EditVersionVm> {
    chain
        .iter()
        .map(|row| EditVersionVm {
            body: archive::display_body_from_content(&row.content_json),
            timestamp: row.origin_ts,
            is_current: current_event_id == Some(row.event_id.as_str()),
        })
        .collect()
}

/// Extract archive media *metadata* from a message `msgtype`, or `None` for a
/// non-media message (Story 5.1). Metadata only — never media bytes. Mirrors the
/// media-info extraction in [`crate::timeline`]/[`crate::media`], reduced to the
/// fields the archive stores.
fn archive_media(msgtype: &MessageType) -> Option<ArchiveMedia> {
    match msgtype {
        MessageType::Image(c) => {
            let info = c.info.as_deref();
            Some(ArchiveMedia {
                mxc: plain_mxc(&c.source),
                mimetype: info.and_then(|i| i.mimetype.clone()),
                size: info.and_then(|i| i.size).map(u64::from),
                width: info.and_then(|i| i.width).map(u64::from),
                height: info.and_then(|i| i.height).map(u64::from),
                filename: Some(c.filename().to_owned()),
                thumbnail_mxc: info
                    .and_then(|i| i.thumbnail_source.as_ref())
                    .and_then(plain_mxc),
            })
        }
        MessageType::Video(c) => {
            let info = c.info.as_deref();
            Some(ArchiveMedia {
                mxc: plain_mxc(&c.source),
                mimetype: info.and_then(|i| i.mimetype.clone()),
                size: info.and_then(|i| i.size).map(u64::from),
                width: info.and_then(|i| i.width).map(u64::from),
                height: info.and_then(|i| i.height).map(u64::from),
                filename: Some(c.filename().to_owned()),
                thumbnail_mxc: info
                    .and_then(|i| i.thumbnail_source.as_ref())
                    .and_then(plain_mxc),
            })
        }
        MessageType::Audio(c) => {
            let info = c.info.as_deref();
            Some(ArchiveMedia {
                mxc: plain_mxc(&c.source),
                mimetype: info.and_then(|i| i.mimetype.clone()),
                size: info.and_then(|i| i.size).map(u64::from),
                width: None,
                height: None,
                filename: Some(c.filename().to_owned()),
                thumbnail_mxc: None,
            })
        }
        MessageType::File(c) => {
            let info = c.info.as_deref();
            Some(ArchiveMedia {
                mxc: plain_mxc(&c.source),
                mimetype: info.and_then(|i| i.mimetype.clone()),
                size: info.and_then(|i| i.size).map(u64::from),
                width: None,
                height: None,
                filename: Some(c.filename().to_owned()),
                thumbnail_mxc: info
                    .and_then(|i| i.thumbnail_source.as_ref())
                    .and_then(plain_mxc),
            })
        }
        _ => None,
    }
}

/// The `mxc://` URI of an unencrypted [`MediaSource::Plain`] source, or `None` for
/// an encrypted source (whose URI stays inside the content JSON, not the metadata
/// column). Pure.
fn plain_mxc(source: &MediaSource) -> Option<String> {
    match source {
        MediaSource::Plain(uri) => Some(uri.to_string()),
        MediaSource::Encrypted(_) => None,
    }
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

/// Drive the room's typing stream: emit the current (empty) typing set as an
/// initial snapshot, then a [`TypingBatch`] on every typing change, resolving each
/// typing member's display name (Story 3.9, AD-14). Stops when the sink reports the
/// channel closed or the broadcast sender is dropped (the room/account went away).
///
/// The SDK typing broadcast already filters the account's own user id out, so every
/// id is another member. Display names are resolved via `room.get_member_no_sync`
/// (no network round-trip on the typing hot path); an unresolvable member carries a
/// `null` display name (the frontend falls back to the id). The SDK event-handler
/// drop guard is held for the whole loop so the handler is unregistered on stop.
/// A `Lagged` broadcast error skips to the newest value rather than ending the
/// stream (a burst of typing changes never kills the row).
async fn run_typing_producer(room: matrix_sdk::Room, sink: TypingSink, account_id: &str) {
    // Hold the drop guard for the producer's lifetime so the SDK typing event
    // handler is unregistered exactly when this task ends (AD-19).
    let (_guard, mut receiver) = signals::subscribe_typing(&room);

    // Open with an explicit empty snapshot so the frontend clears any stale row on
    // (re)subscribe — inherently idempotent.
    if !(sink)(TypingBatch { typists: vec![] }) {
        tracing::info!(account_id = %account_id, "typing channel closed before first batch");
        return;
    }

    loop {
        match receiver.recv().await {
            Ok(user_ids) => {
                let mut typists = Vec::with_capacity(user_ids.len());
                for user_id in user_ids {
                    // Resolve the display name without a network round-trip; a
                    // missing member yields `None` (the UI falls back to the id).
                    let display_name = room
                        .get_member_no_sync(&user_id)
                        .await
                        .ok()
                        .flatten()
                        .and_then(|m| m.display_name().map(str::to_owned));
                    typists.push(TypistVm {
                        user_id: user_id.to_string(),
                        display_name,
                    });
                }
                if !(sink)(TypingBatch { typists }) {
                    tracing::info!(account_id = %account_id, "typing channel closed, stopping producer");
                    break;
                }
            }
            // Lagged: skip to the newest state on the next recv rather than ending.
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            // Sender dropped: the room/account was torn down; end the task.
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
    tracing::info!(account_id = %account_id, "typing stream ended");
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

/// Per-account Spaces producer (Story 4.5). Enumerates the account's joined
/// Spaces and each Space's joined child rooms **from local state only** — no
/// `/hierarchy` network fetch — then pokes the live [`InboxMerger`] with the
/// result, recomputing on every sync batch so the Space list and any active
/// filter stay live.
///
/// It computes once immediately, then loops on
/// `Client::subscribe_to_all_room_updates()`: on `Ok(_)` it recomputes; on
/// `Lagged` it forces a full recompute (a missed batch may have changed
/// membership); on `Closed` it stops (the client is gone). For each
/// `client.joined_space_rooms()` it builds a [`SpaceVm`] (name via
/// `room.display_name().await`, avatar via `room.avatar_url()`) and the Space's
/// child set from `m.space.child` state (`get_state_events_static`), keeping only
/// children the account has actually joined. A read error on one Space is logged
/// at `debug!` and that Space skipped — the others still update.
async fn run_spaces_producer(client: Client, merger: InboxMerger, account_id: &str) {
    let mut updates = client.subscribe_to_all_room_updates();
    // Initial compute so the Space list is present before the first sync batch.
    compute_and_push_spaces(&client, &merger, account_id).await;
    loop {
        match updates.recv().await {
            Ok(_) => compute_and_push_spaces(&client, &merger, account_id).await,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                tracing::debug!(account_id = %account_id, skipped, "spaces updates lagged; forcing recompute");
                compute_and_push_spaces(&client, &merger, account_id).await;
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                tracing::info!(account_id = %account_id, "spaces updates closed, stopping producer");
                break;
            }
        }
    }
}

/// Compute the account's joined Spaces + child membership from local state and
/// push them into the merger (Story 4.5). Factored out of [`run_spaces_producer`]
/// so the initial compute and each sync-driven recompute share one path.
async fn compute_and_push_spaces(client: &Client, merger: &InboxMerger, account_id: &str) {
    use matrix_sdk::deserialized_responses::SyncOrStrippedState;
    use matrix_sdk::ruma::events::space::child::SpaceChildEventContent;
    use matrix_sdk::ruma::events::SyncStateEvent;

    // The set of rooms this account has actually joined — child links are only
    // honored when the child room is joined (view-and-filter joined rooms only).
    let joined: std::collections::HashSet<String> = client
        .joined_rooms()
        .iter()
        .map(|r| r.room_id().to_string())
        .collect();

    let mut spaces: Vec<SpaceVm> = Vec::new();
    let mut memberships: HashMap<String, std::collections::HashSet<String>> = HashMap::new();

    for space in client.joined_space_rooms() {
        let space_id = space.room_id().to_string();
        let name = match space.display_name().await {
            Ok(name) => name.to_string(),
            Err(e) => {
                tracing::debug!(account_id = %account_id, space_id = %space_id, error = %e, "space display name resolve failed, using id");
                space_id.clone()
            }
        };
        let avatar_url = space.avatar_url().map(|u| u.to_string());
        // Read the Space's `m.space.child` state from local store only. Keep the
        // `state_key` (the child room id) of Sync `Original` + Stripped events;
        // drop Redacted; ignore deserialize errors (mirrors matrix-sdk-ui's
        // `build_space_state`). Cross-reference against the account's joined rooms.
        let mut children: std::collections::HashSet<String> = std::collections::HashSet::new();
        match space
            .get_state_events_static::<SpaceChildEventContent>()
            .await
        {
            Ok(child_events) => {
                for raw in child_events {
                    let child_id = match raw.deserialize() {
                        Ok(SyncOrStrippedState::Sync(SyncStateEvent::Original(e))) => {
                            Some(e.state_key.to_string())
                        }
                        Ok(SyncOrStrippedState::Stripped(e)) => Some(e.state_key.to_string()),
                        Ok(SyncOrStrippedState::Sync(SyncStateEvent::Redacted(_))) => None,
                        Err(_) => None,
                    };
                    if let Some(child_id) = child_id {
                        if joined.contains(&child_id) {
                            children.insert(child_id);
                        }
                    }
                }
            }
            Err(e) => {
                tracing::debug!(account_id = %account_id, space_id = %space_id, error = %e, "could not read m.space.child; skipping this space's membership");
            }
        }

        spaces.push(SpaceVm {
            account_id: account_id.to_owned(),
            space_id: space_id.clone(),
            name,
            avatar_url,
        });
        memberships.insert(space_id, children);
    }

    // `joined_space_rooms()` order is unspecified (store-map iteration), so sort
    // deterministically (by name, then id) to keep the sidebar Space list stable
    // across recomputes instead of reshuffling on every sync tick.
    spaces.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.space_id.cmp(&b.space_id))
    });

    merger.update_spaces(account_id, spaces, memberships).await;
}

/// Load keeper-local pins from the registry into a merger-shaped map keyed by
/// `(account_id, room_id)` → `sort_order` (Story 4.3). Pins have no Matrix
/// representation, so membership + order are registry-authoritative.
fn load_pins(data_dir: &Path) -> Result<HashMap<(String, String), i64>, CoreError> {
    Ok(registry::get_pins(data_dir)?
        .into_iter()
        .map(|(account_id, room_id, order)| ((account_id, room_id), order))
        .collect())
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

/// Resolve a live [`matrix_sdk::Room`]'s display name, falling back to the cached
/// name and finally to `fallback` (the room id) when nothing usable resolves
/// (Story 7.3). Mirrors the name-resolution ladder in [`room_item_to_vm`] so an
/// approval-pane row never shows an empty name.
async fn resolved_room_name(room: &matrix_sdk::Room, fallback: &str) -> String {
    room.display_name()
        .await
        .ok()
        .map(|n| n.to_string())
        .filter(|n| !n.trim().is_empty())
        .or_else(|| {
            room.cached_display_name()
                .map(|n| n.to_string())
                .filter(|n| !n.trim().is_empty())
        })
        .unwrap_or_else(|| fallback.to_owned())
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
    // `RoomListItem` derefs to `matrix_sdk::Room`; these client-side counts are
    // precise for E2EE where server counts are not (AD-20). `is_marked_unread`
    // reflects the manual `m.marked_unread` account-data flag.
    let (is_unread, mention_count) = room_unread_state(
        item.is_marked_unread(),
        item.num_unread_messages(),
        item.num_unread_mentions(),
    );
    // `is_low_priority()` reads the cached `m.lowpriority` notable tag (no await);
    // the merge partitions the inbox on it into the Archive window (Story 4.2).
    let is_archived = item.is_low_priority();
    // `is_favourite()` reads the cached `m.favourite` notable tag (no await); the
    // merge partitions the inbox on it into the Favorites window (Story 4.4).
    let is_favourite = item.is_favourite();
    // `is_space()` reads the cached `m.space` room type (no await); the merge
    // excludes Space rooms from all four chat windows (Story 4.5) — Spaces are
    // containers, surfaced separately as filter views.
    let is_space = item.is_space();
    // Resolve the bridged-Network label from the room's local `m.bridge`/legacy
    // bridge state (Story 4.6) via the same untrusted, length-capped resolver the
    // delete confirmation uses (`room_network_label` at ~L1349). `RoomListItem`
    // derefs to `matrix_sdk::Room`, so the `&Room` handle comes free. Reads local
    // state only (no `/hierarchy` or network fetch); a native room resolves to
    // `None` and shows no badge / is excluded from the Networks list.
    let network = bridge::room_bridge_network(item).await;
    // Resolve the room's stable bridge `network_id` — the machine `protocol.id`
    // (Story 6.5, FR-28) — from the same local `m.bridge` state via the pure
    // `parse_bridge_protocol_id` wrapper. This is the join key that matches a room to
    // an unhealthy bridge session on `(account_id, network_id)`, distinct from the
    // display `network` label above. A native room resolves to `None`.
    let network_id = bridge::room_bridge_protocol_id(item).await;

    RoomVm {
        room_id,
        display_name,
        last_message,
        timestamp,
        avatar_url,
        is_unread,
        mention_count,
        is_archived,
        is_favourite,
        is_space,
        network,
        network_id,
    }
}

/// Compute the authoritative unread render state from the SDK's per-room signals.
///
/// A room counts as unread when the manual `m.marked_unread` flag is set, or it
/// has any unread messages, or any unread mentions. `mention_count` is the
/// unread-mention count saturated into `u32` for the mention badge. Pure so it
/// can be unit-tested without a live `Client`.
fn room_unread_state(marked: bool, unread: u64, mentions: u64) -> (bool, u32) {
    let is_unread = marked || unread > 0 || mentions > 0;
    let mention_count = u32::try_from(mentions).unwrap_or(u32::MAX);
    (is_unread, mention_count)
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
        let manager = AccountManager::new(&data_dir);

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

    #[tokio::test]
    async fn resolve_timeline_event_key_maps_invalid_ids_to_error_not_panic() {
        let mut data_dir = std::env::temp_dir();
        data_dir.push(format!(
            "keeper-resolve-evt-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let manager = AccountManager::new(&data_dir);

        // An unparsable room id is an honest typed error (→ TimelineUnavailable),
        // never a panic.
        let bad_room = manager
            .resolve_timeline_event_key("acct", "not-a-room-id", "$evt:example.org")
            .await;
        assert!(matches!(
            bad_room,
            Err(CoreError::Timeline(TimelineError::RoomNotFound))
        ));

        // A well-formed room id but a malformed event id is likewise a typed error,
        // never a silent miss and never a panic.
        let bad_event = manager
            .resolve_timeline_event_key("acct", "!room:example.org", "not-an-event-id")
            .await;
        assert!(matches!(
            bad_event,
            Err(CoreError::Timeline(TimelineError::RoomNotFound))
        ));

        // Well-formed ids but no live timeline for the room ⇒ transient "not loaded"
        // (`Ok(None)`), so the caller retries rather than erroring.
        let no_timeline = manager
            .resolve_timeline_event_key("acct", "!room:example.org", "$evt:example.org")
            .await
            .expect("well-formed ids with no open timeline resolve to Ok(None)");
        assert_eq!(no_timeline, None);

        let _ = std::fs::remove_dir_all(&data_dir);
    }

    /// Story 7.3 airlock invariant: a pending draft whose room/account cannot be
    /// resolved (no live account) is STILL listed, with `display_name = room_id`
    /// and `network = None`. The identity/hue join reads from the registry.
    #[tokio::test]
    async fn list_pending_drafts_emits_unresolved_rows_with_fallback() {
        let mut data_dir = std::env::temp_dir();
        data_dir.push(format!(
            "keeper-approval-list-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let platform: Arc<dyn Platform> = Arc::new(FakePlatform {
            data_dir: data_dir.clone(),
        });
        let manager = AccountManager::new(&data_dir);

        // Seed one account with a known user_id + hue, plus a draft whose account is
        // registered but not live (no room can be resolved).
        registry::insert_account(
            &data_dir,
            "acctA",
            "@alice:example.org",
            "https://example.org",
            "DEV1",
            10,
            3,
            "password",
        )
        .expect("insert account");
        registry::set_draft(&data_dir, "acctA", "!room1:example.org", "held text", 42)
            .expect("set draft with account");
        // A draft whose account is absent from the registry → identity/hue fallback.
        registry::set_draft(
            &data_dir,
            "acctGhost",
            "!room2:example.org",
            "orphan text",
            7,
        )
        .expect("set draft without account");

        let mut rows = manager
            .list_pending_drafts(&platform)
            .await
            .expect("list pending drafts");
        rows.sort_by(|a, b| a.room_id.cmp(&b.room_id));

        assert_eq!(rows.len(), 2, "both drafts are listed, none dropped");

        let with_account = rows
            .iter()
            .find(|r| r.account_id == "acctA")
            .expect("acctA row present");
        assert_eq!(with_account.account_user_id, "@alice:example.org");
        assert_eq!(with_account.hue_index, 3);
        // Room is unresolvable (no live account) → fallback to room_id, no network.
        assert_eq!(with_account.display_name, "!room1:example.org");
        assert_eq!(with_account.network, None);
        assert_eq!(with_account.body, "held text");
        assert_eq!(with_account.updated_ts, 42);

        let orphan = rows
            .iter()
            .find(|r| r.account_id == "acctGhost")
            .expect("orphan row present");
        // Missing registry account → user_id falls back to account_id, hue 0.
        assert_eq!(orphan.account_user_id, "acctGhost");
        assert_eq!(orphan.hue_index, 0);
        assert_eq!(orphan.display_name, "!room2:example.org");
        assert_eq!(orphan.network, None);

        let _ = std::fs::remove_dir_all(&data_dir);
    }

    /// Story 7.3: `send_approval` dispatches through the single gate with the
    /// approval trigger, acquiring its `Timeline` via the reuse-open-else-transient
    /// -build pattern (like `mark_room_read`) so it works from the pane where NO
    /// conversation is open. Crucially it must NOT short-circuit on `NoOpenTimeline`:
    /// with a resolvable-but-not-live account the code path proceeds past
    /// `open_timeline_for` and `room_for` yields `RoomNotFound` (there is no live
    /// homeserver in a unit test to build a transient timeline against) — never the
    /// old `NoOpenTimeline` that baked in the defect.
    #[tokio::test]
    async fn send_approval_routes_through_the_single_gate() {
        let mut data_dir = std::env::temp_dir();
        data_dir.push(format!(
            "keeper-approval-send-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let manager = AccountManager::new(&data_dir);

        // An unparsable room id is an honest typed error, never a panic.
        let bad_room = manager
            .send_approval("acctA", "not-a-room-id", "approve me")
            .await;
        assert!(matches!(
            bad_room,
            Err(CoreError::Send(SendError::RoomNotFound))
        ));

        // Well-formed room id but a non-live account: the conversation is not open, so
        // the code takes the transient-build path — `room_for` resolves no live room
        // and yields `RoomNotFound`. It must NOT return `NoOpenTimeline` (the prior
        // defect): the pane never has an open conversation, so short-circuiting there
        // would make approve non-functional for every draft.
        let not_live = manager
            .send_approval("acctA", "!room:example.org", "approve me")
            .await;
        assert!(matches!(
            not_live,
            Err(CoreError::Timeline(TimelineError::RoomNotFound))
        ));
        assert!(!matches!(
            not_live,
            Err(CoreError::Send(SendError::NoOpenTimeline))
        ));

        let _ = std::fs::remove_dir_all(&data_dir);
    }

    /// Story 7.3 (P7): a whitespace-only approve body is guarded as `EmptyBody`
    /// *before* the timeline opens — never a silent `Ok(())` no-op. The guard runs
    /// ahead of room parsing, so even a well-formed room id returns the typed error
    /// (the frontend's catch then retains the draft — the airlock never destroys
    /// held text). A truly empty and a whitespace-only body both surface it.
    #[tokio::test]
    async fn send_approval_rejects_a_whitespace_only_body() {
        let mut data_dir = std::env::temp_dir();
        data_dir.push(format!(
            "keeper-approval-empty-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let manager = AccountManager::new(&data_dir);

        // An empty body → EmptyBody, not a dispatched no-op.
        let empty = manager
            .send_approval("acctA", "!room:example.org", "")
            .await;
        assert!(matches!(empty, Err(CoreError::Send(SendError::EmptyBody))));

        // A whitespace-only body → EmptyBody as well (the guard trims). The room id
        // is well-formed, proving the guard runs before the timeline is opened.
        let blank = manager
            .send_approval("acctA", "!room:example.org", "   \n\t ")
            .await;
        assert!(matches!(blank, Err(CoreError::Send(SendError::EmptyBody))));

        let _ = std::fs::remove_dir_all(&data_dir);
    }

    /// AD-13 reachability (mirrors `send_approval_routes_through_the_single_gate`):
    /// the `ComposerSend` trigger path is reachable and reaches the gate-acquisition
    /// boundary with a typed error — never `Ok`, never a panic — with no live
    /// homeserver. An unparsable room id is `RoomNotFound`; a well-formed room id on a
    /// non-live account has no open timeline (the composer legitimately requires an
    /// open conversation — there is no transient-build fallback), so it is
    /// `NoOpenTimeline`.
    #[tokio::test]
    async fn send_text_composer_trigger_routes_through_the_gate() {
        let mut data_dir = std::env::temp_dir();
        data_dir.push(format!(
            "keeper-composer-send-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let manager = AccountManager::new(&data_dir);

        // An unparsable room id is an honest typed error, never a panic.
        let bad_room = manager.send_text("acctA", "not-a-room-id", "hi").await;
        assert!(matches!(
            bad_room,
            Err(CoreError::Send(SendError::RoomNotFound))
        ));

        // Well-formed room id but a non-live account: no open timeline, so the
        // composer path reaches the gate boundary and returns the typed
        // `NoOpenTimeline` — never `Ok`, never a panic.
        let not_live = manager.send_text("acctA", "!room:example.org", "hi").await;
        assert!(matches!(
            not_live,
            Err(CoreError::Send(SendError::NoOpenTimeline))
        ));

        let _ = std::fs::remove_dir_all(&data_dir);
    }

    fn room(id: &str) -> RoomVm {
        RoomVm {
            room_id: id.to_owned(),
            display_name: id.to_owned(),
            last_message: None,
            timestamp: None,
            avatar_url: None,
            is_unread: false,
            mention_count: 0,
            is_archived: false,
            is_favourite: false,
            is_space: false,
            network: None,
            network_id: None,
        }
    }

    #[test]
    fn room_unread_state_unread_no_mention() {
        // num_unread_messages > 0, no mentions: unread, no count.
        assert_eq!(room_unread_state(false, 5, 0), (true, 0));
    }

    #[test]
    fn room_unread_state_unread_mention() {
        // num_unread_mentions > 0: unread with the mention count.
        assert_eq!(room_unread_state(false, 5, 2), (true, 2));
    }

    #[test]
    fn room_unread_state_manually_marked_zero_counts() {
        // is_marked_unread with zero counts: unread, no count.
        assert_eq!(room_unread_state(true, 0, 0), (true, 0));
    }

    #[test]
    fn room_unread_state_read() {
        // Zero counts, not marked: read.
        assert_eq!(room_unread_state(false, 0, 0), (false, 0));
    }

    #[test]
    fn room_unread_state_saturates_mention_count() {
        // A u64 mention count above u32::MAX saturates rather than overflowing.
        let (is_unread, count) = room_unread_state(false, 0, u64::from(u32::MAX) + 1);
        assert!(is_unread);
        assert_eq!(count, u32::MAX);
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

    /// The path→MIME guess that `send_attachment_path` uses to classify an
    /// attachment (Story 3.7): common extensions map to their image/video/audio
    /// class, an unknown/missing extension falls back to `application/octet-stream`.
    #[test]
    fn mime_guess_maps_extensions_to_media_classes() {
        let guess = |name: &str| {
            mime_guess::from_path(Path::new(name))
                .first_or_octet_stream()
                .essence_str()
                .to_owned()
        };
        assert_eq!(guess("photo.png"), "image/png");
        assert_eq!(guess("photo.jpg"), "image/jpeg");
        assert_eq!(guess("clip.mp4"), "video/mp4");
        assert_eq!(guess("voice.mp3"), "audio/mpeg");
        assert_eq!(guess("notes.pdf"), "application/pdf");
        // Unknown extension and no extension both fall back to octet-stream.
        assert_eq!(guess("archive.unknownext"), "application/octet-stream");
        assert_eq!(guess("noextension"), "application/octet-stream");
    }

    /// The top-level MIME type used in the `send_attachment_*` `tracing` log (the
    /// only media-classification datum logged — never the bytes/path).
    #[test]
    fn mime_top_level_type_classifies_the_kind() {
        use std::str::FromStr;
        assert_eq!(
            mime::Mime::from_str("image/png").expect("valid").type_(),
            mime::IMAGE
        );
        assert_eq!(
            mime::Mime::from_str("video/mp4").expect("valid").type_(),
            mime::VIDEO
        );
        // A malformed caller-supplied mime falls back to octet-stream (application).
        assert_eq!(
            mime::Mime::from_str("not a mime")
                .unwrap_or(mime::APPLICATION_OCTET_STREAM)
                .type_(),
            mime::APPLICATION
        );
    }

    /// Deserialize an `OriginalSyncRoomMessageEvent` from JSON for the archive
    /// mapping tests (the SDK delivers post-decryption events in this shape).
    fn parse_message_event(json: &str) -> OriginalSyncRoomMessageEvent {
        let raw: matrix_sdk::ruma::serde::Raw<OriginalSyncRoomMessageEvent> =
            serde_json::from_str(json).expect("valid raw message event");
        raw.deserialize().expect("deserialize message event")
    }

    /// A post-decryption text message maps to a normalized `ArchiveEvent`:
    /// event/room/sender ids, ms `origin_ts`, `m.room.message` type, content JSON,
    /// and no media metadata.
    #[test]
    fn build_archive_event_maps_a_text_message() {
        let ev = parse_message_event(
            r#"{
                "type": "m.room.message",
                "event_id": "$evt:example.org",
                "sender": "@bob:example.org",
                "origin_server_ts": 1720000000000,
                "content": { "msgtype": "m.text", "body": "hello" }
            }"#,
        );
        let archived =
            build_archive_event("acctA", "!room:example.org", &ev).expect("build archive event");
        assert_eq!(archived.account_id, "acctA");
        assert_eq!(archived.event_id, "$evt:example.org");
        assert_eq!(archived.room_id, "!room:example.org");
        assert_eq!(archived.sender, "@bob:example.org");
        assert_eq!(archived.origin_ts, 1_720_000_000_000);
        assert_eq!(archived.event_type, "m.room.message");
        assert!(archived.media.is_none());
        // content_json round-trips the message content.
        let content: serde_json::Value =
            serde_json::from_str(&archived.content_json).expect("content json parses");
        assert_eq!(content["msgtype"], "m.text");
        assert_eq!(content["body"], "hello");
        // The indexed body is the top-level body for a plain message (Story 5.3).
        assert_eq!(archived.body, "hello");
        // A plain message carries no relation columns (Story 5.2).
        assert_eq!(archived.relates_to_event_id, None);
        assert_eq!(archived.rel_type, None);
    }

    /// A post-decryption edit (`m.replace`) maps to an `ArchiveEvent` with the
    /// relation columns populated so it links the version chain (Story 5.2).
    #[test]
    fn build_archive_event_extracts_replace_relation() {
        let ev = parse_message_event(
            r#"{
                "type": "m.room.message",
                "event_id": "$edit:example.org",
                "sender": "@bob:example.org",
                "origin_server_ts": 1720000000002,
                "content": {
                    "msgtype": "m.text",
                    "body": "* edited body",
                    "m.new_content": { "msgtype": "m.text", "body": "edited body" },
                    "m.relates_to": {
                        "rel_type": "m.replace",
                        "event_id": "$orig:example.org"
                    }
                }
            }"#,
        );
        let archived =
            build_archive_event("acctA", "!room:example.org", &ev).expect("build archive event");
        assert_eq!(
            archived.relates_to_event_id.as_deref(),
            Some("$orig:example.org")
        );
        assert_eq!(archived.rel_type.as_deref(), Some("m.replace"));
        // The indexed body for an edit is the `m.new_content.body` (Story 5.3), so
        // the edited-away text stays searchable on its own version row.
        assert_eq!(archived.body, "edited body");
    }

    /// A reply is not an edit — no relation columns (Story 5.2).
    #[test]
    fn build_archive_event_reply_has_no_relation_columns() {
        let ev = parse_message_event(
            r#"{
                "type": "m.room.message",
                "event_id": "$reply:example.org",
                "sender": "@bob:example.org",
                "origin_server_ts": 1720000000003,
                "content": {
                    "msgtype": "m.text",
                    "body": "a reply",
                    "m.relates_to": {
                        "m.in_reply_to": { "event_id": "$orig:example.org" }
                    }
                }
            }"#,
        );
        let archived =
            build_archive_event("acctA", "!room:example.org", &ev).expect("build archive event");
        assert_eq!(archived.relates_to_event_id, None);
        assert_eq!(archived.rel_type, None);
    }

    /// The chain→VM mapping extracts each version's display text (original from
    /// top-level `body`, edits from `m.new_content.body`), orders them as given,
    /// and flags the last as current (Story 5.2, FR-11).
    #[test]
    fn edit_versions_from_chain_extracts_bodies_and_flags_current() {
        use crate::archive::db::StoredEvent;
        let mk = |event_id: &str, origin_ts: i64, content_json: &str| StoredEvent {
            account_id: "acctA".to_owned(),
            event_id: event_id.to_owned(),
            room_id: "!r:e.org".to_owned(),
            sender: "@u:e.org".to_owned(),
            origin_ts,
            event_type: "m.room.message".to_owned(),
            content_json: content_json.to_owned(),
            media_json: None,
            inserted_ts: 0,
            relates_to_event_id: None,
            rel_type: None,
            redacted_ts: None,
        };
        let chain = vec![
            mk("$orig", 100, r#"{"msgtype":"m.text","body":"v1"}"#),
            mk(
                "$edit1",
                200,
                r#"{"msgtype":"m.text","body":"* v2","m.new_content":{"body":"v2"}}"#,
            ),
        ];
        let versions = edit_versions_from_chain(&chain, Some("$edit1"));
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].body, "v1");
        assert_eq!(versions[0].timestamp, 100);
        assert!(!versions[0].is_current);
        // The edit's display text comes from m.new_content.body, not the "* v2".
        assert_eq!(versions[1].body, "v2");
        assert_eq!(versions[1].timestamp, 200);
        assert!(versions[1].is_current);
    }

    /// An empty chain maps to an empty vec (Story 5.2).
    #[test]
    fn edit_versions_from_chain_empty_is_empty() {
        assert!(edit_versions_from_chain(&[], None).is_empty());
    }

    /// The honor-remote-deletions gate drops redacted versions from the popover
    /// when enabled, and keeps them (the default) when disabled — content is never
    /// erased on disk either way (Story 5.2, FR-36).
    #[test]
    fn visible_versions_honors_remote_deletions_policy() {
        use crate::archive::db::StoredEvent;
        let mk =
            |event_id: &str, origin_ts: i64, body: &str, redacted_ts: Option<i64>| StoredEvent {
                account_id: "acctA".to_owned(),
                event_id: event_id.to_owned(),
                room_id: "!r:e.org".to_owned(),
                sender: "@u:e.org".to_owned(),
                origin_ts,
                event_type: "m.room.message".to_owned(),
                content_json: format!(r#"{{"msgtype":"m.text","body":"{body}"}}"#),
                media_json: None,
                inserted_ts: origin_ts,
                relates_to_event_id: None,
                rel_type: None,
                redacted_ts,
            };
        let chain = || {
            vec![
                mk("$orig", 100, "v1", Some(150)),
                mk("$edit1", 200, "v2", None),
            ]
        };
        // Default (off): both versions retrievable, newest flagged current.
        let kept = visible_versions(chain(), false);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].body, "v1");
        assert!(kept[1].is_current);
        // Honor on: the redacted original is dropped; only the live edit remains.
        let gated = visible_versions(chain(), true);
        assert_eq!(gated.len(), 1);
        assert_eq!(gated[0].body, "v2");
        assert!(gated[0].is_current);
    }

    /// When honoring remote deletions drops the *newest* (current) version, no
    /// surviving older version is mislabelled `is_current` — the survivors are all
    /// honest prior versions, matching the live timeline (Story 5.2, FR-36).
    #[test]
    fn visible_versions_current_redacted_flags_no_survivor_current() {
        use crate::archive::db::StoredEvent;
        let mk =
            |event_id: &str, origin_ts: i64, body: &str, redacted_ts: Option<i64>| StoredEvent {
                account_id: "acctA".to_owned(),
                event_id: event_id.to_owned(),
                room_id: "!r:e.org".to_owned(),
                sender: "@u:e.org".to_owned(),
                origin_ts,
                event_type: "m.room.message".to_owned(),
                content_json: format!(r#"{{"msgtype":"m.text","body":"{body}"}}"#),
                media_json: None,
                inserted_ts: origin_ts,
                relates_to_event_id: None,
                rel_type: None,
                redacted_ts,
            };
        let chain = || {
            vec![
                mk("$orig", 100, "v1", None),
                mk("$edit1", 200, "v2", None),
                mk("$edit2", 300, "v3", Some(350)),
            ]
        };
        // Honor off: full chain, the true newest ($edit2) is current.
        let kept = visible_versions(chain(), false);
        assert_eq!(kept.len(), 3);
        assert!(kept[2].is_current);
        // Honor on: the redacted current ($edit2) is dropped. The two survivors are
        // both prior versions — neither is flagged current (the current message is
        // redacted and not retrievable here).
        let gated = visible_versions(chain(), true);
        assert_eq!(gated.len(), 2);
        assert_eq!(gated[0].body, "v1");
        assert_eq!(gated[1].body, "v2");
        assert!(gated.iter().all(|v| !v.is_current));
    }

    /// Body extraction falls back to top-level `body` when `m.new_content` is
    /// absent, and to empty on malformed JSON (never panics).
    #[test]
    fn display_body_falls_back_and_never_panics() {
        use crate::archive::display_body_from_content;
        assert_eq!(display_body_from_content(r#"{"body":"top"}"#), "top");
        assert_eq!(
            display_body_from_content(r#"{"body":"top","m.new_content":{"body":"nc"}}"#),
            "nc"
        );
        assert_eq!(display_body_from_content("not json"), "");
        assert_eq!(display_body_from_content("{}"), "");
    }

    /// A post-decryption image message maps to an `ArchiveEvent` whose `media`
    /// carries metadata only (mxc/mimetype/size/dims/filename) — never bytes.
    #[test]
    fn build_archive_event_extracts_image_media_metadata() {
        let ev = parse_message_event(
            r#"{
                "type": "m.room.message",
                "event_id": "$img:example.org",
                "sender": "@bob:example.org",
                "origin_server_ts": 1720000000001,
                "content": {
                    "msgtype": "m.image",
                    "body": "cat.png",
                    "url": "mxc://example.org/abc123",
                    "info": {
                        "mimetype": "image/png",
                        "size": 2048,
                        "w": 640,
                        "h": 480,
                        "thumbnail_url": "mxc://example.org/thumb456"
                    }
                }
            }"#,
        );
        let archived =
            build_archive_event("acctA", "!room:example.org", &ev).expect("build archive event");
        let media = archived.media.expect("media metadata present");
        assert_eq!(media.mxc.as_deref(), Some("mxc://example.org/abc123"));
        assert_eq!(media.mimetype.as_deref(), Some("image/png"));
        assert_eq!(media.size, Some(2048));
        assert_eq!(media.width, Some(640));
        assert_eq!(media.height, Some(480));
        assert_eq!(media.filename.as_deref(), Some("cat.png"));
        assert_eq!(
            media.thumbnail_mxc.as_deref(),
            Some("mxc://example.org/thumb456")
        );
        // No media bytes anywhere in the archived row.
        assert!(!archived.content_json.contains("\"bytes\""));
    }

    /// A non-media msgtype yields no archive media metadata.
    #[test]
    fn archive_media_is_none_for_text() {
        use matrix_sdk::ruma::events::room::message::TextMessageEventContent;
        let mt = MessageType::Text(TextMessageEventContent::plain("hi"));
        assert!(archive_media(&mt).is_none());
    }

    /// An encrypted media source contributes no plain `mxc` (its URI stays inside
    /// the content JSON, not the metadata column), but other metadata is captured.
    #[test]
    fn build_archive_event_encrypted_image_has_no_plain_mxc() {
        let ev = parse_message_event(
            r#"{
                "type": "m.room.message",
                "event_id": "$enc:example.org",
                "sender": "@bob:example.org",
                "origin_server_ts": 1720000000002,
                "content": {
                    "msgtype": "m.image",
                    "body": "secret.png",
                    "file": {
                        "url": "mxc://example.org/enc789",
                        "key": {
                            "kty": "oct",
                            "key_ops": ["encrypt", "decrypt"],
                            "alg": "A256CTR",
                            "k": "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
                            "ext": true
                        },
                        "iv": "AAECAwQFBgcICQoLDA0ODw",
                        "hashes": { "sha256": "ICEiIyQlJicoKSorLC0uLzAxMjM0NTY3ODk6Ozw9Pj8" },
                        "v": "v2"
                    },
                    "info": { "mimetype": "image/png", "size": 999 }
                }
            }"#,
        );
        let archived =
            build_archive_event("acctA", "!room:example.org", &ev).expect("build archive event");
        let media = archived.media.expect("media metadata present");
        assert_eq!(media.mxc, None, "encrypted source contributes no plain mxc");
        assert_eq!(media.mimetype.as_deref(), Some("image/png"));
        assert_eq!(media.size, Some(999));
    }
}
