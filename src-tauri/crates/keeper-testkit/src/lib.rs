//! Replica test doubles, on-disk adapter fixtures and the harness that gives
//! a module a [`Source`] over the replica its test chooses ([`harness`]).
//! Dev-dependency only: no shipped crate depends on it.

use keeper_ports::*;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use tokio::sync::watch;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

pub mod harness;

pub use harness::{harness, Harness, Over};

/// A body's content address, `sha256:<hex>` ([`BlobBody::id`]).
pub fn blob_id(bytes: &[u8]) -> String {
    use sha2::Digest;
    format!("sha256:{}", hex::encode(sha2::Sha256::digest(bytes)))
}

/// The [`Replica`] methods whose offline semantics [`FakeReplica`] models,
/// as the real replica answers them with no server: keeper-replica's
/// parity suite runs each against both. The other methods are stubs that
/// stand in only for a collaborator outside the claim a test proves:
/// `query` and `query_covered` answer the rows a test set
/// ([`FakeReplica::replicate`]), with nothing covered; `mutate_superseding`
/// commits as `mutate` does and sets no refusal aside; a remote call
/// answers [`ReplicaError::NotSyncing`]; `delivery` answers
/// [`Delivery::Unknown`]; `close` and `join` set their flag; every other
/// one answers empty (or `Ok`).
pub const MODELLED: [&str; 8] = [
    "mutate",
    "commit_with_blobs",
    "cached_blob",
    "fetch_blob",
    "upload_refusals",
    "refused_commits",
    "sync_now",
    "state",
];

/// The fake's blob bodies and outbox: what the port keeps on this device.
#[derive(Default)]
struct Store {
    /// Cached bodies, by content address.
    bodies: BTreeMap<String, Vec<u8>>,
    /// Bodies staged for upload.
    queued: BTreeSet<String>,
    /// Commits waiting for the server, oldest first, with what they write.
    outbox: Vec<(String, Vec<RowOp>)>,
    /// `(commit, blob)` pins the server's refusal of the blob's upload
    /// released, as the adapter releases them.
    released: BTreeSet<(String, String)>,
    /// Standing refusals of commits written here, newest first.
    refused: Vec<RefusedCommit>,
}

/// The row an operation writes.
fn row_id(op: &RowOp) -> Option<String> {
    match op {
        RowOp::Upsert { values, .. } | RowOp::Patch { values, .. } => {
            values.get("id").and_then(Value::as_str).map(str::to_owned)
        }
        RowOp::Delete { row_id, .. } => Some(row_id.clone()),
    }
}

/// Whether `op` writes a column holding a blob reference to `blob`, read as
/// the adapter reads one for an upload refusal: a JSON text column whose
/// `blobId` it is. The port gives the fake no schema, so its pins
/// ([`FakeReplica::pins`]) read references the same way; syncular pins only
/// columns its schema types as blob references, so the fake over-approximates
/// a pin for a JSON text column that only looks like one.
fn references(op: &RowOp, blob: &str) -> bool {
    let (RowOp::Upsert { values, .. } | RowOp::Patch { values, .. }) = op else {
        return false;
    };
    values.values().any(|value| {
        value
            .as_str()
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
            .is_some_and(|reference| reference.get("blobId").and_then(Value::as_str) == Some(blob))
    })
}

/// A [`Replica`] held in memory. It models the port's offline semantics for
/// the methods [`MODELLED`] names, never a server's: a commit waits in its
/// outbox, a body committed with it is cached, staged and pinned, and a
/// failed commit leaves nothing staged. A server's answer is what a test
/// sets ([`FakeReplica::refuse_upload`], [`FakeReplica::refuse_commit`]),
/// and it surfaces as the adapter reports a later round's: through
/// [`Replica::upload_refusals`] and [`Replica::refused_commits`], never as
/// an error from a call.
pub struct FakeReplica {
    pub spec: ReplicaSpec,
    pub rows: Mutex<Vec<Map<String, Value>>>,
    pub closed: AtomicBool,
    pub joined: AtomicBool,
    /// What [`Replica::upload_refusals`] answers.
    pub upload_refusals: Mutex<Vec<UploadRefusal>>,
    /// Downloads attempted through the network-capable blob port.
    pub fetched_blobs: Mutex<Vec<String>>,
    store: Mutex<Store>,
    /// Commits written so far: the next commit's id.
    commits: AtomicUsize,
    state: watch::Sender<ReplicaState>,
}

impl FakeReplica {
    pub fn set(&self, link: LinkState) {
        self.state.send_modify(|state| state.link = link);
    }
    pub fn set_presence_known(&self, known: bool) {
        self.state.send_modify(|state| state.presence_known = known);
    }
    /// Replace the rows every query answers, as a round would, and say
    /// the directory changed.
    pub fn replicate(&self, rows: Vec<Map<String, Value>>) {
        if let Ok(mut held) = self.rows.lock() {
            *held = rows;
        }
        self.state.send_modify(|state| state.directory += 1);
    }
    pub fn closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    fn store(&self) -> std::sync::MutexGuard<'_, Store> {
        self.store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Cache `bytes`, as a download from the server would: neither staged
    /// nor pinned. Their content address.
    pub fn cache_blob(&self, bytes: &[u8]) -> String {
        let id = blob_id(bytes);
        self.store().bodies.insert(id.clone(), bytes.to_vec());
        id
    }

    /// The bodies staged for upload, sorted ([`queued`] of a replica file).
    pub fn queued(&self) -> Vec<String> {
        self.store().queued.iter().cloned().collect()
    }

    /// The commits that pin each blob, `(commit, blob)`, sorted ([`pins`]
    /// of a replica file): every waiting commit's reference to a body the
    /// fake holds, unless a refused upload released it.
    pub fn pins(&self) -> Vec<(String, String)> {
        let store = self.store();
        let mut pins: Vec<(String, String)> = store
            .outbox
            .iter()
            .flat_map(|(commit, ops)| {
                store
                    .bodies
                    .keys()
                    .filter(|blob| ops.iter().any(|op| references(op, blob)))
                    .map(|blob| (commit.clone(), blob.clone()))
            })
            .filter(|pin| !store.released.contains(pin))
            .collect();
        pins.sort();
        pins
    }

    /// The server refused `blob`'s upload, as a later round would report:
    /// the refusal is tied to every waiting commit that references the
    /// blob ([`UploadRefusal`]), and the blob leaves the upload queue and
    /// every commit's pins. A blob no waiting commit references surfaces no
    /// refusal and leaves the queue all the same, as the adapter releases it.
    pub fn refuse_upload(&self, blob: &str, rejection: Rejection) {
        let mut store = self.store();
        let refusals: Vec<UploadRefusal> = store
            .outbox
            .iter()
            .filter_map(|(commit, ops)| {
                let refused_rows: Vec<String> = ops
                    .iter()
                    .filter(|op| references(op, blob))
                    .filter_map(row_id)
                    .collect();
                (!refused_rows.is_empty()).then(|| UploadRefusal {
                    commit_id: commit.clone(),
                    blob_id: blob.to_owned(),
                    rejection: rejection.clone(),
                    refused_rows,
                    commit_rows: ops.iter().filter_map(row_id).collect(),
                })
            })
            .collect();
        store.queued.remove(blob);
        for refusal in &refusals {
            store
                .released
                .insert((refusal.commit_id.clone(), blob.to_owned()));
        }
        drop(store);
        let mut held = self
            .upload_refusals
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for refusal in refusals {
            held.retain(|standing| {
                standing.commit_id != refusal.commit_id || standing.blob_id != refusal.blob_id
            });
            held.push(refusal);
        }
    }

    /// The server refused `refused.commit_id`, as the outcome journal would
    /// record it: it stands in [`Replica::refused_commits`] once the commit
    /// no longer waits in the outbox (a waiting commit's delivery is still
    /// pending, as the replica file answers it).
    pub fn refuse_commit(&self, refused: RefusedCommit) {
        self.store().refused.insert(0, refused);
    }
}

impl Replica for FakeReplica {
    fn mutate(&self, ops: Vec<RowOp>) -> Result<String, ReplicaError> {
        self.commit_with_blobs(Vec::new(), ops)
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
    fn subscribe(&self, _: Unit) -> Result<(), ReplicaError> {
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
    /// Stages and commits as one step, as the adapter does: a body whose
    /// bytes are not at its address, or a commit with no operation, fails
    /// with nothing staged; otherwise every body is cached and staged, and
    /// the commit waits in the outbox, pinning those it references.
    fn commit_with_blobs(
        &self,
        blobs: Vec<BlobBody>,
        ops: Vec<RowOp>,
    ) -> Result<String, ReplicaError> {
        if let Some(blob) = blobs.iter().find(|blob| blob_id(&blob.bytes) != blob.id) {
            return Err(ReplicaError::Client(format!(
                "the replica staged {} under another content address",
                blob.id
            )));
        }
        if ops.is_empty() {
            return Err(ReplicaError::Client("no operations".to_owned()));
        }
        let commit = format!("commit-{}", self.commits.fetch_add(1, Ordering::SeqCst) + 1);
        let mut store = self.store();
        for blob in blobs {
            store.queued.insert(blob.id.clone());
            store.bodies.insert(blob.id, blob.bytes);
        }
        store.outbox.push((commit.clone(), ops));
        drop(store);
        self.state.send_modify(|state| state.outbox += 1);
        Ok(commit)
    }
    /// The cached body; a miss is refused, never fetched (the fake has no
    /// server). The attempt is recorded in [`FakeReplica::fetched_blobs`].
    fn fetch_blob(&self, blob: &str) -> Result<Vec<u8>, ReplicaError> {
        self.fetched_blobs
            .lock()
            .expect("fetched blobs")
            .push(blob.to_owned());
        self.cached_blob(blob)
    }
    fn cached_blob(&self, blob: &str) -> Result<Vec<u8>, ReplicaError> {
        self.store()
            .bodies
            .get(blob)
            .cloned()
            .ok_or_else(|| ReplicaError::Client(format!("{blob} is not cached here")))
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
    fn revoked_units(&self, _: &WindowBase) -> Result<Vec<String>, ReplicaError> {
        Ok(Vec::new())
    }
    fn refused_commits(&self, table: &str) -> Result<Vec<RefusedCommit>, ReplicaError> {
        let store = self.store();
        Ok(store
            .refused
            .iter()
            .filter(|refused| {
                !store
                    .outbox
                    .iter()
                    .any(|(commit, _)| *commit == refused.commit_id)
            })
            .filter(|refused| refused.rows.iter().any(|(written, _)| written == table))
            .cloned()
            .collect())
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
    fn mutate_superseding(&self, ops: Vec<RowOp>, _: Vec<String>) -> Result<String, ReplicaError> {
        self.mutate(ops)
    }
    fn remote_query(&self, _: &str, _: Value) -> Result<Vec<Map<String, Value>>, ReplicaError> {
        Err(ReplicaError::NotSyncing)
    }
    fn remote_command(&self, _: &str, _: &str, _: Value) -> Result<CommandResult, ReplicaError> {
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
pub struct FakeOpener {
    pub opened: Mutex<Vec<Arc<FakeReplica>>>,
    /// A database whose next open waits until the sender is dropped or sends.
    pub hold: Mutex<std::collections::HashMap<String, std::sync::mpsc::Receiver<()>>>,
}

impl FakeOpener {
    pub fn all(&self) -> Vec<Arc<FakeReplica>> {
        self.opened.lock().map(|o| o.clone()).unwrap_or_default()
    }
    /// The replicas not closed, by database.
    pub fn live(&self) -> Vec<Arc<FakeReplica>> {
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
            store: Mutex::new(Store::default()),
            commits: AtomicUsize::new(0),
            state,
        });
        self.opened
            .lock()
            .map_err(|_| ReplicaError::Closed)?
            .push(Arc::clone(&replica));
        Ok(replica)
    }
}

use std::io::{Read, Write};
use std::time::Duration;

/// The blobs the replica file still queues for upload.
pub fn queued(path: &std::path::Path) -> Vec<String> {
    let conn = rusqlite::Connection::open(path).expect("opens");
    let mut ids: Vec<String> = conn
        .prepare("SELECT blob_id FROM _syncular_blob_uploads")
        .expect("prepares")
        .query_map([], |row| row.get(0))
        .expect("reads")
        .collect::<Result<_, _>>()
        .expect("reads");
    ids.sort();
    ids
}

/// The commits that pin each blob in the replica file, `(commit, blob)`.
pub fn pins(path: &std::path::Path) -> Vec<(String, String)> {
    let conn = rusqlite::Connection::open(path).expect("opens");
    let mut pins: Vec<(String, String)> = conn
        .prepare("SELECT commit_id, blob_id FROM _syncular_blob_commit_refs")
        .expect("prepares")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("reads")
        .collect::<Result<_, _>>()
        .expect("reads");
    pins.sort();
    pins
}

/// Mark the replica at `path` as bootstrapped against its server, as its
/// first round would: from then on a round uploads what is staged.
pub fn bootstrapped(path: &std::path::Path) {
    let conn = rusqlite::Connection::open(path).expect("opens");
    conn.busy_timeout(Duration::from_secs(5)).expect("timeout");
    conn.execute(
        "INSERT OR REPLACE INTO _syncular_meta (key, value) VALUES ('logEpoch', 'epoch-1')",
        [],
    )
    .expect("bootstraps");
}

/// What a blob server saw: each request's method and path, the status it
/// answered, and — for a push (`POST …/sync`) — its body, so a test can tell
/// which commits it carried (empty for every other request).
pub type Seen = Arc<Mutex<Vec<(String, String, u16, String)>>>;

/// A server for the blob half of a round: an upload grant is answered
/// `{}` (no signed URL: `PUT` it here) and a `PUT` `200`, unless
/// `refused` names the blob (then that status, on both); anything else
/// — the round's push, the socket — `503`: the network cut after the
/// upload and before the metadata exchange.
pub fn blob_server(refused: Vec<(String, u16)>) -> (u16, Seen) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds");
    let port = listener.local_addr().expect("addr").port();
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut request = Vec::new();
            let mut buf = [0u8; 8192];
            let head_end = loop {
                if let Some(at) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    break Some(at + 4);
                }
                match stream.read(&mut buf) {
                    Ok(0) | Err(_) => break None,
                    Ok(n) => request.extend_from_slice(&buf[..n]),
                }
            };
            let Some(head_end) = head_end else { continue };
            let head = String::from_utf8_lossy(&request[..head_end]).to_string();
            let length: usize = head
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse().ok())?
                })
                .unwrap_or(0);
            while request.len() < head_end + length {
                match stream.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => request.extend_from_slice(&buf[..n]),
                }
            }
            let mut words = head.split_whitespace();
            let method = words.next().unwrap_or_default().to_owned();
            let path = words.next().unwrap_or_default().to_owned();
            let refusal = refused
                .iter()
                .find(|(blob, _)| path.contains(blob.as_str()))
                .map(|(_, status)| *status);
            let (status, body) = match (method.as_str(), refusal) {
                (_, Some(status)) if path.contains("/blobs/") => (status, "{}"),
                ("POST", None) if path.ends_with("/upload-grant") => (200, "{}"),
                ("PUT", None) if path.contains("/blobs/") => (200, "{}"),
                _ => (503, r#"{"error":"unavailable"}"#),
            };
            let pushed = if method == "POST" && path.ends_with("/sync") {
                String::from_utf8_lossy(&request[head_end..]).into_owned()
            } else {
                String::new()
            };
            log.lock()
                .expect("seen")
                .push((method, path, status, pushed));
            let _ = write!(
                stream,
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    (port, seen)
}

/// How many `PUT`s of `blob` the server answered with `status`.
pub fn puts(seen: &Seen, blob: &str, status: u16) -> usize {
    seen.lock()
        .expect("seen")
        .iter()
        .filter(|(method, path, answered, _)| {
            method == "PUT" && path.ends_with(blob) && *answered == status
        })
        .count()
}

/// Whether a round reached its push (`/sync`) after `after` requests.
pub fn pushed_after(seen: &Seen, after: usize) -> bool {
    seen.lock()
        .expect("seen")
        .iter()
        .skip(after)
        .any(|(method, path, _, _)| method == "POST" && path.ends_with("/sync"))
}

/// Where in the record a push (`/sync`) carried `commit`, in order.
pub fn pushes_of(seen: &Seen, commit: &str) -> Vec<usize> {
    seen.lock()
        .expect("seen")
        .iter()
        .enumerate()
        .filter(|(_, (method, path, _, body))| {
            method == "POST" && path.ends_with("/sync") && body.contains(commit)
        })
        .map(|(at, _)| at)
        .collect()
}
