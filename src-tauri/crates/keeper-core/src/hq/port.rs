//! The replica port: what the rest of keeper-core calls to read and write one
//! database's local replica. Its one adapter is [`crate::hq::syncular`]; a
//! replacement client would be a second adapter, and nothing above this file
//! names `syncular_client`.
//!
//! Reads answer from the replica file on the caller's thread and never wait
//! on the network. Everything else is handed to the replica's owner thread
//! and waits for it, which can be behind a round bounded by the transport's
//! request timeout; an async caller runs those on `spawn_blocking`.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Map, Value};
use tokio::sync::watch;

/// Rows a query read, and the window units not complete when it read them.
pub type Covered = (Vec<Map<String, Value>>, Vec<String>);

/// One write to one row.
#[derive(Debug, Clone, PartialEq)]
pub enum RowOp {
    /// Insert or replace the row `values["id"]`.
    Upsert {
        table: String,
        values: Map<String, Value>,
        base_version: Option<i64>,
    },
    /// Change only the named columns of the row `values["id"]`.
    Patch {
        table: String,
        values: Map<String, Value>,
        base_version: Option<i64>,
    },
    /// Delete the row.
    Delete {
        table: String,
        row_id: String,
        base_version: Option<i64>,
    },
}

/// A named subscription: `table`'s rows under `scopes` (each a variable and
/// the values requested).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subscription {
    pub id: String,
    pub table: String,
    pub scopes: Vec<(String, Vec<String>)>,
}

/// A windowed subscription's base (SPEC §4.8): `table` subscribed along
/// `variable`, one unit per value, with `fixed_scopes` shared by every unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowBase {
    pub table: String,
    pub variable: String,
    pub fixed_scopes: Vec<(String, Vec<String>)>,
}

/// The units a window holds, and those whose first pull has not completed —
/// whose rows may be missing and must not be drawn as complete.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WindowState {
    pub units: Vec<String>,
    pub pending: Vec<String>,
}

/// A peer present on a scope key.
#[derive(Debug, Clone, PartialEq)]
pub struct Peer {
    pub actor_id: String,
    pub client_id: String,
    pub doc: Value,
}

/// Where a replica syncs to. `None` is a replica on this device alone: its
/// writes wait in its outbox and it makes no request.
#[derive(Clone)]
pub struct Link {
    /// The database's mount: `<server>/<database>`. Every request the replica
    /// makes, blob capabilities included, goes to this origin or nowhere.
    pub base_url: String,
    /// The device's bearer token. Never logged, never in a view model.
    pub token: String,
}

impl std::fmt::Debug for Link {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Link")
            .field("base_url", &self.base_url)
            .field("token", &"<redacted>")
            .finish()
    }
}

/// What to open.
#[derive(Debug, Clone)]
pub struct ReplicaSpec {
    /// The database's name, for logs and status.
    pub database: String,
    /// The replica file. Its directory exists.
    pub path: PathBuf,
    /// The client id the token was issued to; `None` for a replica on this
    /// device alone, which keeps the id it minted the first time.
    pub client_id: Option<String>,
    pub link: Option<Link>,
    /// Subscriptions the replica holds from its first round, registered
    /// before the owner touches the network.
    pub subscriptions: Vec<Subscription>,
    /// A presence key one of those subscriptions registers (`dir:<database>`)
    /// whose peers already present the replica asks after on every new
    /// socket: [`ReplicaState::presence_known`].
    pub watch_presence: Option<String>,
}

/// How a replica's connection stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    /// No link: everything stays on this device.
    Local,
    /// The first round or the socket is under way.
    Connecting,
    /// The last round succeeded and the socket is open.
    Connected,
    /// The last round could not reach the server; it is retried with backoff.
    Offline,
    /// The server refused this device's credential. The replica's network is
    /// closed for good; only a new replica opens it again.
    Refused,
    /// Closed: the owner has stopped or is stopping.
    Closed,
}

/// A replica's state, as its owner last published it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplicaState {
    pub link: LinkState,
    /// Commits written here that the server has not acknowledged.
    pub outbox: usize,
    /// Bumped whenever the replica's `clients` rows change.
    pub directory: u64,
    /// Bumped whenever any row or window of the replica changes, locally or
    /// from the server: what a view over its rows reads again on.
    pub rows: u64,
    /// Bumped on every socket text frame, a presence fanout among them. A
    /// closing socket changes `link` instead, which is published anyway.
    pub presence: u64,
    /// The peers already present on [`ReplicaSpec::watch_presence`] have
    /// reached this replica over the current socket. A connected socket says
    /// nothing of them yet: until this is true an empty
    /// [`Replica::presence`] there is not "nobody is present". False while
    /// the socket is down; every new socket starts false.
    pub presence_known: bool,
}

/// What the server said when it refused a commit or an operation: its stable
/// code (`hq.v8.not_reader`, `operation.forbidden`, …), matched exactly, and
/// its message — which the server may leave equal to the code, and which
/// for `remove_member` carries the reason (`hq.remove_member.not_owner`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    pub code: String,
    pub message: String,
}

impl Rejection {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Rejection {
        Rejection {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for Rejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.message.is_empty() || self.message == self.code {
            f.write_str(&self.code)
        } else {
            write!(f, "{}: {}", self.code, self.message)
        }
    }
}

/// keeper's own code for a commit another device's change beat: the server
/// kept theirs (a conflict outcome carries no code of its own).
pub const CONFLICT: &str = "keeper.conflict";

/// What became of a commit written here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery {
    /// Still in the outbox.
    Pending,
    /// The server took it.
    Applied,
    /// The server refused it ([`CONFLICT`] when another device's change
    /// came first).
    Refused(Rejection),
    /// Not known here (never written, or its outcome was pruned).
    Unknown,
}

/// A commit written here that the server refused, its refusal still
/// standing: nothing written since has set it aside.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub commit_id: String,
    pub rejection: Rejection,
}

/// What this replica still owes the server, or was refused, for one row —
/// read from the replica file (its outbox and its outcome journal), so it
/// outlives a restart.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RowWrites {
    /// The commits writing the row that wait in the outbox, oldest first.
    pub pending: Vec<String>,
    /// The standing refusals of commits that wrote it, newest first.
    pub refused: Vec<Refusal>,
}

/// A commit written here that the server refused, its refusal still
/// standing, with every row it wrote (`table`, the values written) as the
/// replica's outcome journal keeps them — the rows themselves are gone from
/// the replica once the refusal arrives.
#[derive(Debug, Clone, PartialEq)]
pub struct RefusedCommit {
    pub commit_id: String,
    pub rejection: Rejection,
    pub rows: Vec<(String, Map<String, Value>)>,
}

/// A blob's bytes, staged with the commit that references them
/// ([`Replica::commit_with_blobs`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobBody {
    /// Its content address, `sha256:<hex>`.
    pub id: String,
    pub bytes: Vec<u8>,
    pub media_type: Option<String>,
}

/// The server's refusal of a blob's upload (`blob.too_large`,
/// `blob.hash_mismatch`), tied to one commit written here that references
/// the blob. The replica resolved it when it arrived: the blob is no longer
/// staged or pinned for that commit, so the rounds behind it go on, and the
/// commit goes to the server without its body, which refuses it — its rows
/// then come back through the outcome journal ([`Replica::refused_commits`]).
/// Kept beside the replica, so it outlives a restart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadRefusal {
    pub commit_id: String,
    pub blob_id: String,
    pub rejection: Rejection,
    /// The ids of the commit's rows that reference the blob.
    pub refused_rows: Vec<String>,
    /// The ids of every row the commit writes.
    pub commit_rows: Vec<String>,
}

/// Why a replica call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReplicaError {
    /// The client refused the call; its code and message.
    #[error("{0}")]
    Client(String),
    /// The server answered a command and made no change: its code, its
    /// message, and its word (`retryable`) whether the same command may
    /// succeed later. A definite outcome.
    #[error("{code}: {message}")]
    Remote {
        code: String,
        message: String,
        retryable: bool,
    },
    /// A command was sent and no answer keeper can trust came back — the
    /// connection failed, or the answer could not be read — so whether the
    /// server made it is not known. Asking again under the same request id
    /// answers it (the server replays a request it applied). `retryable` is
    /// only the client's word on asking again now: `false` (an answer keeper
    /// could not read) means something must change first, never that the
    /// command was refused.
    #[error("{code}: {message}")]
    Unheard {
        code: String,
        message: String,
        retryable: bool,
    },
    /// A network call on a replica that does not sync (local, refused or
    /// closed).
    #[error("this database is not syncing on this device")]
    NotSyncing,
    /// The owner thread is gone (the replica was closed).
    #[error("this database's replica is closed")]
    Closed,
}

/// One database's local replica.
pub trait Replica: Send + Sync {
    /// Commit `ops` locally, in one commit, into the outbox; its id.
    fn mutate(&self, ops: Vec<RowOp>) -> Result<String, ReplicaError>;
    /// Read the replica. Answers from the file, never from the network.
    fn query(&self, sql: &str, params: &[Value]) -> Result<Vec<Map<String, Value>>, ReplicaError>;
    /// [`Replica::query`], and which of `units` of the window `base` are
    /// not complete yet (not held, or their first pull not done), read in
    /// the same snapshot.
    fn query_covered(
        &self,
        sql: &str,
        params: &[Value],
        base: &WindowBase,
        units: &[String],
    ) -> Result<Covered, ReplicaError>;
    /// Hold `subscription`, persisted under its id.
    fn subscribe(&self, subscription: Subscription) -> Result<(), ReplicaError>;
    /// Set a window's units: units added are subscribed, units removed are
    /// evicted (rows the outbox still holds stay). Persisted with the replica.
    fn set_window(&self, base: &WindowBase, units: &[String]) -> Result<(), ReplicaError>;
    /// A window's units and the ones not complete yet.
    fn window_state(&self, base: &WindowBase) -> Result<WindowState, ReplicaError>;
    /// Publish (or clear, `None`) this client's presence on `scope_key`.
    fn set_presence(&self, scope_key: &str, doc: Option<Value>) -> Result<(), ReplicaError>;
    /// The peers present on `scope_key`.
    fn presence(&self, scope_key: &str) -> Result<Vec<Peer>, ReplicaError>;
    /// Stage `blobs` for upload and commit `ops`, which reference them, as
    /// one step of the replica's owner: no round runs and nothing is
    /// evicted between, so each blob is pinned throughout — staged, then
    /// referenced by the commit until the server has it. When a blob or the
    /// commit fails, what this call staged and nothing else holds is
    /// unstaged again. The commit's id.
    fn commit_with_blobs(
        &self,
        blobs: Vec<BlobBody>,
        ops: Vec<RowOp>,
    ) -> Result<String, ReplicaError>;
    /// A blob's bytes, from the cache or, while the replica syncs, its server.
    fn fetch_blob(&self, blob: &str) -> Result<Vec<u8>, ReplicaError>;
    /// A locally cached body only. Must never fetch on a miss.
    fn cached_blob(&self, blob: &str) -> Result<Vec<u8>, ReplicaError>;
    /// What became of the commit `commit_id`. Answers from the file.
    fn delivery(&self, commit_id: &str) -> Result<Delivery, ReplicaError>;
    /// What is owed, or was refused, of the writes made here to `table`'s
    /// row `row_id`, optionally restricted to operations writing `field`.
    /// Answers from the file.
    fn row_writes(
        &self,
        table: &str,
        row_id: &str,
        field: Option<&str>,
    ) -> Result<RowWrites, ReplicaError>;
    /// The standing refusals of commits written here that wrote a row of
    /// `table`, newest first, with what each wrote. Answers from the file.
    fn refused_commits(&self, table: &str) -> Result<Vec<RefusedCommit>, ReplicaError>;
    /// The values of `variable` the server granted this replica's active
    /// subscriptions of `table` on their last pull (their effective scopes):
    /// what it allows, never what was only asked for. A unit asked for and
    /// not granted (revoked), or not pulled yet, is absent. Answers from the
    /// file.
    fn granted(&self, table: &str, variable: &str) -> Result<Vec<String>, ReplicaError>;
    /// The units of `table`'s window whose subscription the server revoked.
    /// syncular-client never asks the server for such a unit again while it
    /// stays in the window; taking it out and putting it back
    /// ([`Replica::set_window`]) subscribes it afresh. Answers from the file.
    fn revoked_units(&self, table: &str) -> Result<Vec<String>, ReplicaError>;
    /// The server's standing refusals of uploads of blobs commits written
    /// here reference, each resolved ([`UploadRefusal`]). Answers from what
    /// the replica kept beside its file, never the network.
    fn upload_refusals(&self) -> Vec<UploadRefusal>;
    /// The bytes of blob bodies the replica caches, as its client reports
    /// them (`blob_cache_bytes_approx`).
    fn blob_cache_bytes(&self) -> Result<u64, ReplicaError>;
    /// [`Replica::mutate`], and set the refusals of `earlier` aside by the
    /// new commit, which writes their row again, in the same step of the
    /// replica's owner: no round runs between the two, so no refusal of
    /// theirs can surface beside the new commit. A commit still waiting in
    /// the outbox is set aside in advance: should the server refuse it, that
    /// refusal never stands. Durable, like the refusals. Once the commit is
    /// written its id is the answer; a set-aside that fails after it is
    /// logged, since the write itself was made.
    fn mutate_superseding(
        &self,
        ops: Vec<RowOp>,
        earlier: Vec<String>,
    ) -> Result<String, ReplicaError>;
    /// Run the server's registered query `operation_id` with `params`: rows
    /// read on the server, never kept in the replica. Not on the owner
    /// thread, so it never waits behind a round; refused while the replica
    /// does not sync ([`ReplicaError::NotSyncing`]).
    fn remote_query(
        &self,
        operation_id: &str,
        params: Value,
    ) -> Result<Vec<Map<String, Value>>, ReplicaError>;
    /// Run the server's registered command `operation_id` with `params`
    /// under `request_id` (a retry with the same id is answered from the
    /// server's cache): its writes reach the replica by sync, never here.
    /// Like [`Replica::remote_query`], off the owner thread and refused
    /// while the replica does not sync.
    fn remote_command(
        &self,
        operation_id: &str,
        request_id: &str,
        params: Value,
    ) -> Result<(), ReplicaError>;
    /// Run a round now rather than at the next heartbeat.
    fn sync_now(&self);
    /// The state its owner last published.
    fn state(&self) -> ReplicaState;
    /// Every state its owner publishes from now on.
    fn watch(&self) -> watch::Receiver<ReplicaState>;
    /// Close the replica's network at once — no request starts after this
    /// returns — and ask its owner to stop. The file stays.
    fn close(&self);
    /// Wait until the owner has stopped and released the file. A request
    /// already on the wire when [`Replica::close`] ran ends within the
    /// transport's request timeout.
    fn join(&self);
}

/// Opens a replica. Never touches the network.
pub trait Opener: Send + Sync {
    fn open(&self, spec: ReplicaSpec) -> Result<Arc<dyn Replica>, ReplicaError>;
}

/// A replica held in memory, with a state the test sets.
#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    pub(crate) struct FakeReplica {
        pub(crate) spec: ReplicaSpec,
        pub(crate) rows: Mutex<Vec<Map<String, Value>>>,
        pub(crate) closed: AtomicBool,
        pub(crate) joined: AtomicBool,
        /// What [`Replica::upload_refusals`] answers.
        pub(crate) upload_refusals: Mutex<Vec<UploadRefusal>>,
        /// Downloads attempted through the network-capable blob port.
        pub(crate) fetched_blobs: Mutex<Vec<String>>,
        state: watch::Sender<ReplicaState>,
    }

    impl FakeReplica {
        pub(crate) fn set(&self, link: LinkState) {
            self.state.send_modify(|state| state.link = link);
        }
        pub(crate) fn set_presence_known(&self, known: bool) {
            self.state.send_modify(|state| state.presence_known = known);
        }
        /// Replace the rows every query answers, as a round would, and say
        /// the directory changed.
        pub(crate) fn replicate(&self, rows: Vec<Map<String, Value>>) {
            if let Ok(mut held) = self.rows.lock() {
                *held = rows;
            }
            self.state.send_modify(|state| state.directory += 1);
        }
        pub(crate) fn closed(&self) -> bool {
            self.closed.load(Ordering::SeqCst)
        }
    }

    impl Replica for FakeReplica {
        fn mutate(&self, ops: Vec<RowOp>) -> Result<String, ReplicaError> {
            if ops.is_empty() {
                return Err(ReplicaError::Client("no operations".to_owned()));
            }
            self.state.send_modify(|state| state.outbox += 1);
            Ok(format!("commit-{}", self.state.borrow().outbox))
        }
        fn query(&self, _: &str, _: &[Value]) -> Result<Vec<Map<String, Value>>, ReplicaError> {
            Ok(self.rows.lock().map_err(|_| ReplicaError::Closed)?.clone())
        }
        fn query_covered(
            &self,
            sql: &str,
            params: &[Value],
            _: &WindowBase,
            _: &[String],
        ) -> Result<Covered, ReplicaError> {
            Ok((self.query(sql, params)?, Vec::new()))
        }
        fn subscribe(&self, _: Subscription) -> Result<(), ReplicaError> {
            Ok(())
        }
        fn set_window(&self, _: &WindowBase, _: &[String]) -> Result<(), ReplicaError> {
            Ok(())
        }
        fn window_state(&self, _: &WindowBase) -> Result<WindowState, ReplicaError> {
            Ok(WindowState::default())
        }
        fn set_presence(&self, _: &str, _: Option<Value>) -> Result<(), ReplicaError> {
            Ok(())
        }
        fn presence(&self, _: &str) -> Result<Vec<Peer>, ReplicaError> {
            Ok(Vec::new())
        }
        fn commit_with_blobs(
            &self,
            _: Vec<BlobBody>,
            ops: Vec<RowOp>,
        ) -> Result<String, ReplicaError> {
            self.mutate(ops)
        }
        fn fetch_blob(&self, blob: &str) -> Result<Vec<u8>, ReplicaError> {
            self.fetched_blobs
                .lock()
                .expect("fetched blobs")
                .push(blob.to_owned());
            Ok(Vec::new())
        }
        fn cached_blob(&self, _: &str) -> Result<Vec<u8>, ReplicaError> {
            Err(ReplicaError::Closed)
        }
        fn delivery(&self, _: &str) -> Result<Delivery, ReplicaError> {
            Ok(Delivery::Unknown)
        }
        fn row_writes(&self, _: &str, _: &str, _: Option<&str>) -> Result<RowWrites, ReplicaError> {
            Ok(RowWrites::default())
        }
        fn granted(&self, _: &str, _: &str) -> Result<Vec<String>, ReplicaError> {
            Ok(Vec::new())
        }
        fn revoked_units(&self, _: &str) -> Result<Vec<String>, ReplicaError> {
            Ok(Vec::new())
        }
        fn refused_commits(&self, _: &str) -> Result<Vec<RefusedCommit>, ReplicaError> {
            Ok(Vec::new())
        }
        fn upload_refusals(&self) -> Vec<UploadRefusal> {
            self.upload_refusals
                .lock()
                .map(|held| held.clone())
                .unwrap_or_default()
        }
        fn blob_cache_bytes(&self) -> Result<u64, ReplicaError> {
            Ok(0)
        }
        fn mutate_superseding(
            &self,
            ops: Vec<RowOp>,
            _: Vec<String>,
        ) -> Result<String, ReplicaError> {
            self.mutate(ops)
        }
        fn remote_query(&self, _: &str, _: Value) -> Result<Vec<Map<String, Value>>, ReplicaError> {
            Err(ReplicaError::NotSyncing)
        }
        fn remote_command(&self, _: &str, _: &str, _: Value) -> Result<(), ReplicaError> {
            Err(ReplicaError::NotSyncing)
        }
        fn sync_now(&self) {}
        fn state(&self) -> ReplicaState {
            *self.state.borrow()
        }
        fn watch(&self) -> watch::Receiver<ReplicaState> {
            self.state.subscribe()
        }
        fn close(&self) {
            self.closed.store(true, Ordering::SeqCst);
        }
        fn join(&self) {
            self.joined.store(true, Ordering::SeqCst);
        }
    }

    /// Opens [`FakeReplica`]s and keeps every one it opened.
    #[derive(Default)]
    pub(crate) struct FakeOpener {
        pub(crate) opened: Mutex<Vec<Arc<FakeReplica>>>,
        /// A database whose next open waits until the sender is dropped or sends.
        pub(crate) hold: Mutex<std::collections::HashMap<String, std::sync::mpsc::Receiver<()>>>,
    }

    impl FakeOpener {
        pub(crate) fn all(&self) -> Vec<Arc<FakeReplica>> {
            self.opened.lock().map(|o| o.clone()).unwrap_or_default()
        }
        /// The replicas not closed, by database.
        pub(crate) fn live(&self) -> Vec<Arc<FakeReplica>> {
            self.all().into_iter().filter(|r| !r.closed()).collect()
        }
    }

    impl Opener for FakeOpener {
        fn open(&self, spec: ReplicaSpec) -> Result<Arc<dyn Replica>, ReplicaError> {
            let held = self
                .hold
                .lock()
                .ok()
                .and_then(|mut hold| hold.remove(&spec.database));
            if let Some(release) = held {
                let _ = release.recv();
            }
            let link = if spec.link.is_some() {
                LinkState::Connecting
            } else {
                LinkState::Local
            };
            let (state, _) = watch::channel(ReplicaState {
                link,
                outbox: 0,
                directory: 0,
                rows: 0,
                presence: 0,
                presence_known: false,
            });
            let replica = Arc::new(FakeReplica {
                spec,
                rows: Mutex::new(Vec::new()),
                closed: AtomicBool::new(false),
                joined: AtomicBool::new(false),
                upload_refusals: Mutex::new(Vec::new()),
                fetched_blobs: Mutex::new(Vec::new()),
                state,
            });
            self.opened
                .lock()
                .map_err(|_| ReplicaError::Closed)?
                .push(Arc::clone(&replica));
            Ok(replica)
        }
    }
}
