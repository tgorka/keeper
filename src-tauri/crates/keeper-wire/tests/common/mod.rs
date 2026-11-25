//! The harness of keeper-wire's suites (story 123.3-AC14): fixture
//! operations of every kind with their own counters and gates, a server on a
//! real socket in a fresh data directory, and a raw line client that fails
//! on a frame after its call's terminal frame and on a `seq` that skips.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use keeper_runtime_api::{Door, ErrorCode, Kind, OpError, Reply};
use keeper_wire::{Dispatch, Hooks, Limits, Server, Stream};
use keeper_wire_api::{DataLock, Detail, OperationEntry};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::UnixStream;
use tokio::sync::{watch, Barrier};

/// A gate a test opens; closed when made.
#[derive(Clone)]
pub struct Gate(Arc<watch::Sender<bool>>);

impl Gate {
    pub fn new() -> Self {
        Self(Arc::new(watch::channel(false).0))
    }
    pub fn open(&self) {
        self.0.send_replace(true);
    }
    pub fn close(&self) {
        self.0.send_replace(false);
    }
    pub async fn wait(&self) {
        let mut rx = self.0.subscribe();
        let _ = rx.wait_for(|open| *open).await;
    }
}

impl Default for Gate {
    fn default() -> Self {
        Self::new()
    }
}

/// The fixture's own counters.
#[derive(Default)]
pub struct Counts {
    /// Handlers called (every kind).
    pub calls: AtomicUsize,
    /// Query and stream handlers dropped before they finished.
    pub drops: AtomicUsize,
    /// Command handlers that started / returned.
    pub commands_started: AtomicUsize,
    pub commands_finished: AtomicUsize,
    /// Events a stream produced.
    pub produced: AtomicUsize,
}

impl Counts {
    pub fn get(field: &AtomicUsize) -> usize {
        field.load(Ordering::SeqCst)
    }
}

/// Fixture operations:
/// - `fx.echo` (query): answers its input;
/// - `fx.held` (query): waits at `gate`, then answers its input;
/// - `fx.barrier` (query): waits at a `barrier_parties`-party barrier, then answers its input;
/// - `fx.big` (query): answers `{"s": "<input.bytes x's>"}`;
/// - `fx.fail_path` (query): fails `UNAVAILABLE` with a detail naming a path;
/// - `fx.cmd` (command): waits at `gate`, then answers its input;
/// - `fx.stream` (stream): `input.count` events (none: endless), never pausing,
///   each `{"n": i, "pad": "<input.pad x's>"}`;
/// - `fx.stream_held` (stream): one event, then waits at `gate`, then ends.
pub struct Fixture {
    pub counts: Counts,
    pub gate: Gate,
    pub barrier: Arc<Barrier>,
}

impl Fixture {
    pub fn new(barrier_parties: usize) -> Arc<Self> {
        Arc::new(Self {
            counts: Counts::default(),
            gate: Gate::new(),
            barrier: Arc::new(Barrier::new(barrier_parties)),
        })
    }
}

/// Counts a drop unless disarmed.
struct DropCount(Arc<Fixture>, bool);

impl Drop for DropCount {
    fn drop(&mut self) {
        if self.1 {
            self.0.counts.drops.fetch_add(1, Ordering::SeqCst);
        }
    }
}

/// The dispatcher over a fixture.
pub struct FixtureDispatch(pub Arc<Fixture>);

const OPS: &[(&str, Kind)] = &[
    ("fx.echo", Kind::Query),
    ("fx.held", Kind::Query),
    ("fx.barrier", Kind::Query),
    ("fx.big", Kind::Query),
    ("fx.fail_path", Kind::Query),
    ("fx.cmd", Kind::Command),
    ("fx.stream", Kind::Stream),
    ("fx.stream_held", Kind::Stream),
];

impl Dispatch for FixtureDispatch {
    fn app_version(&self) -> String {
        "0.0.0-fixture".into()
    }
    fn catalogue_digest(&self) -> String {
        "fixture-digest".into()
    }
    fn kind(&self, _door: Door, operation: &str) -> Option<Kind> {
        OPS.iter().find(|(id, _)| *id == operation).map(|(_, k)| *k)
    }
    fn call(
        &self,
        _door: Door,
        operation: &str,
        input: Value,
    ) -> Pin<Box<dyn std::future::Future<Output = Reply> + Send>> {
        let fx = Arc::clone(&self.0);
        fx.counts.calls.fetch_add(1, Ordering::SeqCst);
        let operation = operation.to_owned();
        Box::pin(async move {
            match operation.as_str() {
                "fx.echo" => Ok(input),
                "fx.held" => {
                    let mut guard = DropCount(Arc::clone(&fx), true);
                    fx.gate.wait().await;
                    guard.1 = false;
                    Ok(input)
                }
                "fx.barrier" => {
                    let mut guard = DropCount(Arc::clone(&fx), true);
                    fx.barrier.wait().await;
                    guard.1 = false;
                    Ok(input)
                }
                "fx.big" => {
                    let n = input["bytes"].as_u64().unwrap_or(0) as usize;
                    Ok(json!({ "s": "x".repeat(n) }))
                }
                "fx.fail_path" => Err(OpError::new(
                    ErrorCode::Unavailable,
                    "storage_unavailable",
                    "cannot open /home/secret-person/.local/share/keeper.db",
                )),
                "fx.cmd" => {
                    fx.counts.commands_started.fetch_add(1, Ordering::SeqCst);
                    fx.gate.wait().await;
                    fx.counts.commands_finished.fetch_add(1, Ordering::SeqCst);
                    Ok(input)
                }
                _ => Err(OpError::new(ErrorCode::NotFound, "unknown_operation", "")),
            }
        })
    }
    fn watch(&self, _door: Door, operation: &str, input: Value) -> Stream {
        self.0.counts.calls.fetch_add(1, Ordering::SeqCst);
        let held = operation == "fx.stream_held";
        Box::pin(FxStream {
            fx: Arc::clone(&self.0),
            guard: Some(DropCount(Arc::clone(&self.0), true)),
            left: input["count"].as_u64(),
            pad: "x".repeat(input["pad"].as_u64().unwrap_or(0) as usize),
            n: 0,
            held,
            wait: None,
        })
    }
    fn describe(&self, _door: Door, _detail: Detail) -> Vec<OperationEntry> {
        OPS.iter()
            .map(|(id, kind)| OperationEntry {
                id: (*id).into(),
                kind: kind.as_str().into(),
                effect: "read".into(),
                summary: "fixture".into(),
                input_schema: None,
                output_schema: None,
                write_as: None,
            })
            .collect()
    }
}

struct FxStream {
    fx: Arc<Fixture>,
    guard: Option<DropCount>,
    left: Option<u64>,
    pad: String,
    n: u64,
    held: bool,
    wait: Option<Pin<Box<dyn std::future::Future<Output = ()> + Send>>>,
}

impl futures_core::Stream for FxStream {
    type Item = Reply;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Reply>> {
        let this = &mut *self;
        if this.held && this.n >= 1 {
            let gate = this.fx.gate.clone();
            let wait = this
                .wait
                .get_or_insert_with(|| Box::pin(async move { gate.wait().await }));
            if wait.as_mut().poll(cx).is_pending() {
                return Poll::Pending;
            }
            if let Some(mut guard) = this.guard.take() {
                guard.1 = false;
            }
            return Poll::Ready(None);
        }
        if this.left == Some(this.n) {
            if let Some(mut guard) = this.guard.take() {
                guard.1 = false;
            }
            return Poll::Ready(None);
        }
        this.n += 1;
        this.fx.counts.produced.fetch_add(1, Ordering::SeqCst);
        Poll::Ready(Some(Ok(json!({ "n": this.n, "pad": this.pad }))))
    }
}

/// A server on a real socket in a fresh 0700 data directory.
pub struct Served {
    pub dir: tempfile::TempDir,
    pub server: Option<Server>,
    pub fixture: Arc<Fixture>,
    pub lock: Arc<DataLock>,
}

impl Served {
    pub fn socket(&self) -> PathBuf {
        keeper_wire_api::socket_path(self.dir.path())
    }
    pub fn server(&self) -> &Server {
        self.server.as_ref().unwrap()
    }
    pub async fn raw(&self) -> Raw {
        let mut raw = Raw::connect(&self.socket()).await;
        raw.hello().await;
        raw
    }
}

pub fn data_dir() -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    dir
}

/// Serve the fixture with `limits` (barrier of `limits.per_connection` parties).
pub fn serve(limits: Limits, hooks: Hooks) -> Served {
    serve_with(Fixture::new(limits.per_connection), limits, hooks)
}

pub fn serve_with(fixture: Arc<Fixture>, limits: Limits, hooks: Hooks) -> Served {
    let dir = data_dir();
    let lock = Arc::new(DataLock::acquire(dir.path(), "keeper-wire test").unwrap());
    let server = Server::start(
        Arc::clone(&lock),
        Arc::new(FixtureDispatch(Arc::clone(&fixture))),
        limits,
        hooks,
    )
    .unwrap();
    Served {
        dir,
        server: Some(server),
        fixture,
        lock,
    }
}

/// A fresh canonical call id.
pub fn id() -> String {
    keeper_wire_api::new_call_id()
}

/// Wait until `f` holds, polling every 5 ms, for at most `within`.
pub async fn eventually(within: Duration, mut f: impl FnMut() -> bool) -> bool {
    let start = tokio::time::Instant::now();
    loop {
        if f() {
            return true;
        }
        if start.elapsed() > within {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[derive(Default)]
struct CallState {
    terminated: bool,
    last_seq: u64,
}

/// A raw line client that checks the envelope's per-call order.
pub struct Raw {
    pub read: BufReader<OwnedReadHalf>,
    pub write: OwnedWriteHalf,
    calls: HashMap<String, CallState>,
}

impl Raw {
    pub async fn connect(socket: &Path) -> Self {
        let stream = UnixStream::connect(socket).await.unwrap();
        let (read, write) = stream.into_split();
        Self {
            read: BufReader::with_capacity(1 << 16, read),
            write,
            calls: HashMap::new(),
        }
    }

    /// Say hello and read the server's.
    pub async fn hello(&mut self) -> Value {
        self.send(&json!({"type": "hello", "wire": ["keeper.wire.v1"], "client": "test/1"}))
            .await;
        let hello = self.frame().await.expect("server hello");
        assert_eq!(hello["type"], "hello", "{hello}");
        hello
    }

    /// Send raw bytes.
    pub async fn send_bytes(&mut self, bytes: &[u8]) {
        self.write.write_all(bytes).await.unwrap();
    }

    /// Send one frame. A frame naming a `call_id` (other than `cancel`)
    /// starts that id afresh in the order check.
    pub async fn send(&mut self, frame: &Value) {
        if frame["type"] != "cancel" {
            if let Some(id) = frame["call_id"].as_str() {
                self.calls.insert(id.to_owned(), CallState::default());
            }
        }
        let mut bytes = serde_json::to_vec(frame).unwrap();
        bytes.push(b'\n');
        self.send_bytes(&bytes).await;
    }

    pub async fn call(&mut self, id: &str, operation: &str, input: Value) {
        self.send(&json!({"type": "call", "call_id": id, "operation": operation, "input": input}))
            .await;
    }

    pub async fn watch(&mut self, id: &str, operation: &str, input: Value) {
        self.send(&json!({"type": "watch", "call_id": id, "operation": operation, "input": input}))
            .await;
    }

    pub async fn cancel(&mut self, id: &str) {
        self.send(&json!({"type": "cancel", "call_id": id})).await;
    }

    /// The next frame within 10 s, or `None` at the end of the connection.
    /// Fails on a frame after its call's terminal frame and on a skipped `seq`.
    pub async fn frame(&mut self) -> Option<Value> {
        self.frame_within(Duration::from_secs(10))
            .await
            .expect("a frame within the time")
    }

    /// `Err(())` when nothing came within `within`.
    pub async fn frame_within(&mut self, within: Duration) -> Result<Option<Value>, ()> {
        let mut line = Vec::new();
        let read = tokio::time::timeout(within, self.read.read_until(b'\n', &mut line)).await;
        let Ok(read) = read else {
            return Err(());
        };
        match read {
            Ok(0) | Err(_) => return Ok(None),
            Ok(_) => {}
        }
        assert_eq!(line.last(), Some(&b'\n'), "a torn line from the server");
        let frame: Value = serde_json::from_slice(&line).unwrap();
        self.check(&frame);
        Ok(Some(frame))
    }

    fn check(&mut self, frame: &Value) {
        let Some(id) = frame["call_id"].as_str() else {
            return;
        };
        let state = self.calls.entry(id.to_owned()).or_default();
        assert!(
            !state.terminated,
            "a frame after {id}'s terminal frame: {frame}"
        );
        match frame["type"].as_str() {
            Some("event") => {
                let seq = frame["seq"].as_u64().unwrap();
                assert_eq!(seq, state.last_seq + 1, "seq skipped for {id}: {frame}");
                state.last_seq = seq;
            }
            Some("reply" | "end") => state.terminated = true,
            other => panic!("unexpected frame type {other:?}: {frame}"),
        }
    }

    /// Frames until the one for `id` that ends it; returns it. Other calls'
    /// frames are collected into `others`.
    pub async fn terminal(&mut self, id: &str, others: &mut Vec<Value>) -> Value {
        loop {
            let frame = self.frame().await.expect("terminal frame before the end");
            if frame["call_id"] == id && matches!(frame["type"].as_str(), Some("reply" | "end")) {
                return frame;
            }
            others.push(frame);
        }
    }

    /// The connection ends within 5 s; frames before the end are returned.
    pub async fn until_closed(&mut self) -> Vec<Value> {
        let mut frames = Vec::new();
        loop {
            match self.frame_within(Duration::from_secs(5)).await {
                Ok(Some(frame)) => frames.push(frame),
                Ok(None) => return frames,
                Err(()) => panic!("the connection stayed open; read {frames:?}"),
            }
        }
    }

    /// Read raw bytes until the end of the connection (no order check).
    pub async fn drain_bytes(&mut self) -> Vec<u8> {
        let mut bytes = Vec::new();
        let _ =
            tokio::time::timeout(Duration::from_secs(5), self.read.read_to_end(&mut bytes)).await;
        bytes
    }
}

/// `limits` with the deadline ceilings, the stall and the places a test sets.
pub fn limits() -> Limits {
    Limits::default()
}
