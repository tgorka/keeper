//! The replica port: what the rest of keeper-core calls to read and write one
//! database's local replica. Its one adapter is [`crate::hq::syncular`]; a
//! replacement client would be a second adapter, and nothing above this file
//! names `syncular_client`.
//!
//! Every method is synchronous. Reads answer from the replica file on the
//! caller's thread and never wait on the network; everything else is handed to
//! the replica's owner thread and waits for it, which can be behind a round
//! bounded by the transport's request timeout. An async caller runs them on
//! `spawn_blocking`, never on a tokio worker serving IPC.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Map, Value};
use tokio::sync::watch;

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
    /// The database's mount: `<server>/<database>`.
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
}

/// How a replica's connection stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    /// No server: everything stays on this device.
    Local,
    /// The first round or the socket is under way.
    Connecting,
    /// The last round succeeded and the socket is open.
    Connected,
    /// The last round could not reach the server; it is retried with backoff.
    Offline,
    /// The server refused this device's credential. Nothing more is sent
    /// until the replica is opened again.
    Refused,
}

/// A replica's state, as its owner last published it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplicaState {
    pub link: LinkState,
    /// Commits written here that the server has not acknowledged.
    pub outbox: usize,
}

/// Why a replica call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReplicaError {
    /// The client refused the call; its code and message.
    #[error("{0}")]
    Client(String),
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
    /// Ask the server for `table`'s rows under `scopes` (a variable and the
    /// values requested), persisted under `id`.
    fn subscribe(
        &self,
        id: &str,
        table: &str,
        scopes: Vec<(String, Vec<String>)>,
    ) -> Result<(), ReplicaError>;
    /// Publish (or clear, `None`) this client's presence on `scope_key`.
    fn set_presence(&self, scope_key: &str, doc: Option<Value>) -> Result<(), ReplicaError>;
    /// The peers present on `scope_key`.
    fn presence(&self, scope_key: &str) -> Result<Vec<Peer>, ReplicaError>;
    /// Cache `bytes` and queue their upload; the blob reference a column holds.
    fn upload_blob(
        &self,
        bytes: Vec<u8>,
        media_type: Option<String>,
    ) -> Result<Value, ReplicaError>;
    /// A blob's bytes, from the cache or the server.
    fn fetch_blob(&self, blob: &str) -> Result<Vec<u8>, ReplicaError>;
    /// Run a round now rather than at the next heartbeat.
    fn sync_now(&self);
    /// The state its owner last published.
    fn state(&self) -> ReplicaState;
    /// Every state its owner publishes from now on.
    fn watch(&self) -> watch::Receiver<ReplicaState>;
    /// Stop the owner: the socket closes and no request is sent again. The
    /// file stays.
    fn close(&self);
}

/// Opens a replica.
pub trait Opener: Send + Sync {
    fn open(&self, spec: ReplicaSpec) -> Result<Arc<dyn Replica>, ReplicaError>;
}

/// A replica held in memory, with a state the test sets.
#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use std::sync::Mutex;

    pub(crate) struct FakeReplica {
        pub(crate) spec: ReplicaSpec,
        pub(crate) rows: Mutex<Vec<Map<String, Value>>>,
        pub(crate) commits: Mutex<usize>,
        pub(crate) closed: Mutex<bool>,
        state: watch::Sender<ReplicaState>,
    }

    impl FakeReplica {
        pub(crate) fn set(&self, link: LinkState) {
            self.state.send_modify(|state| state.link = link);
        }
    }

    impl Replica for FakeReplica {
        fn mutate(&self, ops: Vec<RowOp>) -> Result<String, ReplicaError> {
            if ops.is_empty() {
                return Err(ReplicaError::Client("no operations".to_owned()));
            }
            for op in ops {
                if let RowOp::Upsert { values, .. } = op {
                    self.rows
                        .lock()
                        .map_err(|_| ReplicaError::Closed)?
                        .push(values);
                }
            }
            let mut commits = self.commits.lock().map_err(|_| ReplicaError::Closed)?;
            *commits += 1;
            self.state.send_modify(|state| state.outbox += 1);
            Ok(format!("commit-{commits}"))
        }
        fn query(&self, _: &str, _: &[Value]) -> Result<Vec<Map<String, Value>>, ReplicaError> {
            Ok(self.rows.lock().map_err(|_| ReplicaError::Closed)?.clone())
        }
        fn subscribe(
            &self,
            _: &str,
            _: &str,
            _: Vec<(String, Vec<String>)>,
        ) -> Result<(), ReplicaError> {
            Ok(())
        }
        fn set_presence(&self, _: &str, _: Option<Value>) -> Result<(), ReplicaError> {
            Ok(())
        }
        fn presence(&self, _: &str) -> Result<Vec<Peer>, ReplicaError> {
            Ok(Vec::new())
        }
        fn upload_blob(&self, _: Vec<u8>, _: Option<String>) -> Result<Value, ReplicaError> {
            Ok(Value::Null)
        }
        fn fetch_blob(&self, _: &str) -> Result<Vec<u8>, ReplicaError> {
            Ok(Vec::new())
        }
        fn sync_now(&self) {}
        fn state(&self) -> ReplicaState {
            *self.state.borrow()
        }
        fn watch(&self) -> watch::Receiver<ReplicaState> {
            self.state.subscribe()
        }
        fn close(&self) {
            if let Ok(mut closed) = self.closed.lock() {
                *closed = true;
            }
        }
    }

    /// Opens [`FakeReplica`]s and keeps every one it opened.
    #[derive(Default)]
    pub(crate) struct FakeOpener {
        pub(crate) opened: Mutex<Vec<Arc<FakeReplica>>>,
    }

    impl Opener for FakeOpener {
        fn open(&self, spec: ReplicaSpec) -> Result<Arc<dyn Replica>, ReplicaError> {
            let link = if spec.link.is_some() {
                LinkState::Connecting
            } else {
                LinkState::Local
            };
            let (state, _) = watch::channel(ReplicaState { link, outbox: 0 });
            let replica = Arc::new(FakeReplica {
                spec,
                rows: Mutex::new(Vec::new()),
                commits: Mutex::new(0),
                closed: Mutex::new(false),
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
