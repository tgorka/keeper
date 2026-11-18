//! Replica test doubles and on-disk adapter fixtures. Dev-dependency only.

use keeper_ports::*;
use serde_json::{Map, Value};
use std::sync::Arc;
use tokio::sync::watch;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

pub struct FakeReplica {
    pub spec: ReplicaSpec,
    pub rows: Mutex<Vec<Map<String, Value>>>,
    pub closed: AtomicBool,
    pub joined: AtomicBool,
    /// What [`Replica::upload_refusals`] answers.
    pub upload_refusals: Mutex<Vec<UploadRefusal>>,
    /// Downloads attempted through the network-capable blob port.
    pub fetched_blobs: Mutex<Vec<String>>,
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
    fn commit_with_blobs(&self, _: Vec<BlobBody>, ops: Vec<RowOp>) -> Result<String, ReplicaError> {
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

/// What a blob server saw: each request's method and path, and the
/// status it answered.
pub type Seen = Arc<Mutex<Vec<(String, String, u16)>>>;

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
            log.lock().expect("seen").push((method, path, status));
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
        .filter(|(method, path, answered)| {
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
        .any(|(method, path, _)| method == "POST" && path.ends_with("/sync"))
}
