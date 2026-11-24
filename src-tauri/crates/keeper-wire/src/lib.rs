//! keeper's local socket server (story 123.3; MA/AD-3 as amended by
//! O-123-WIRE-1, MA/AD-4, MA/AD-5 before 123.4's rule).
//!
//! [`Server::start`] binds `<data dir>/wire/wire.sock` under the data
//! directory's lock, checks every peer's kernel credentials before a byte of
//! its connection is read or written, and serves `keeper.wire.v1` through a
//! [`Dispatch`]: in a binary, keeper-runtime's [`WireDispatcher`], which
//! reaches only the `app` reads; in this crate's tests, fixture operations.
//!
//! Every bound of the envelope is a field of [`Limits`]; its `Default` is
//! MA/AD-3's numbers, and only a test sets others.
//!
//! Role `transport` (MA-G3): edges to keeper-runtime, keeper-runtime-api and
//! keeper-wire-api only, and none of `tonic`, `prost`, `hyper` or `tower`.

mod conn;
mod dispatch;
mod switch;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use keeper_wire_api::place::{clear_stale, effective_uid, prepare_wire_dir, PlaceError};
use keeper_wire_api::DataLock;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::watch;
use tokio::task::JoinSet;

pub use dispatch::{Dispatch, Stream};
pub use keeper_runtime::WireDispatcher;
pub use switch::{debug_override, follow_switch, DEBUG_ON_VAR, SWITCH_POLL};

/// Every number of the envelope (MA/AD-3 *Limits*, *Streamed answers*,
/// *Deadlines*). The composition root passes [`Limits::default`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Calls and streams in flight on one connection.
    pub per_connection: usize,
    /// Connections on the listener.
    pub connections: usize,
    /// Calls and streams in flight on the listener.
    pub listener_calls: usize,
    /// A line, its `\n` included, in both directions.
    pub max_line_bytes: usize,
    /// Events of one stream waiting to be written.
    pub stream_events: usize,
    /// Bytes of one stream's events waiting to be written.
    pub stream_bytes: usize,
    /// Answers to frames that took no place, waiting to be written.
    pub unadmitted: usize,
    /// A frame not wholly written this long after it was queued closes its
    /// connection.
    pub stall: Duration,
    /// A query's deadline ceiling, also its default (a `describe` is a query).
    pub query_ceiling: Duration,
    /// A command's deadline ceiling, also its default.
    pub command_ceiling: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            per_connection: 64,
            connections: 16,
            listener_calls: 128,
            max_line_bytes: keeper_wire_api::MAX_LINE_BYTES,
            stream_events: 256,
            stream_bytes: 4 * 1024 * 1024,
            unadmitted: 64,
            stall: Duration::from_secs(10),
            query_ceiling: Duration::from_secs(30),
            command_ceiling: Duration::from_secs(120),
        }
    }
}

/// What only a test composition sets. A composition root passes
/// `Hooks::default()`.
#[derive(Clone, Default)]
pub struct Hooks {
    /// Holds a command after it took its place and before the start decision
    /// (H11).
    #[cfg(feature = "test-gate")]
    pub start_gate: Option<Arc<dyn Fn() -> dispatch::BoxFuture<'static, ()> + Send + Sync>>,
    /// The uid the peer check expects instead of the effective uid.
    #[cfg(feature = "test-gate")]
    pub expect_uid: Option<u32>,
}

impl Hooks {
    fn expected_uid(&self) -> u32 {
        #[cfg(feature = "test-gate")]
        if let Some(uid) = self.expect_uid {
            return uid;
        }
        effective_uid()
    }

    async fn before_start(&self) {
        #[cfg(feature = "test-gate")]
        if let Some(gate) = &self.start_gate {
            gate().await;
        }
    }
}

/// A count with a ceiling.
#[derive(Debug)]
pub(crate) struct Counter {
    used: AtomicUsize,
    max: usize,
}

impl Counter {
    pub(crate) fn new(max: usize) -> Self {
        Self {
            used: AtomicUsize::new(0),
            max,
        }
    }

    fn try_take(&self) -> bool {
        let mut used = self.used.load(Ordering::Acquire);
        while used < self.max {
            match self.used.compare_exchange_weak(
                used,
                used + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(now) => used = now,
            }
        }
        false
    }

    fn give_back(&self) {
        self.used.fetch_sub(1, Ordering::AcqRel);
    }

    pub(crate) fn used(&self) -> usize {
        self.used.load(Ordering::Acquire)
    }
}

/// What the listener shares with its connections.
pub(crate) struct Shared {
    pub limits: Limits,
    pub hooks: Hooks,
    pub dispatch: Arc<dyn Dispatch>,
    pub calls: Arc<Counter>,
    pub connections: Arc<Counter>,
    pub open_streams: AtomicUsize,
    pub longest_line_held: AtomicUsize,
}

/// Why the server did not start.
#[derive(Debug)]
pub enum ServeError {
    Place(PlaceError),
    Bind {
        path: PathBuf,
        error: std::io::Error,
    },
}

impl std::fmt::Display for ServeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Place(error) => error.fmt(f),
            Self::Bind { path, error } => write!(f, "wire_bind: {}: {error}", path.display()),
        }
    }
}

impl std::error::Error for ServeError {}

impl ServeError {
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Place(error) => error.code(),
            Self::Bind { .. } => "wire_bind",
        }
    }
}

/// The one listener of a data directory, serving until [`Server::stop`].
pub struct Server {
    shared: Arc<Shared>,
    socket: PathBuf,
    shutdown: watch::Sender<bool>,
    accept: Option<tokio::task::JoinHandle<JoinSet<()>>>,
    lock: Arc<DataLock>,
    stopped: bool,
}

/// A connection whose peer passed the uid comparison. Only
/// [`check_peer`] makes one, so a connection's frames are never read
/// unless its peer was checked (MA/AD-4).
pub(crate) struct CheckedPeer {
    pub stream: UnixStream,
}

/// Read the peer's kernel credentials and compare its uid with `expected`.
/// On a mismatch or a failure the stream is dropped unread and unwritten,
/// and one WARN carries the peer's uid. The pid is logged only.
fn check_peer(stream: UnixStream, expected: u32) -> Option<CheckedPeer> {
    match stream.peer_cred() {
        Ok(cred) if cred.uid() == expected => Some(CheckedPeer { stream }),
        Ok(cred) => {
            tracing::warn!(
                peer_uid = cred.uid(),
                peer_pid = cred.pid(),
                "keeper's socket refused a peer of another user"
            );
            None
        }
        Err(error) => {
            tracing::warn!(%error, "keeper's socket could not read a peer's credentials");
            None
        }
    }
}

impl Server {
    /// Holding `lock`: check the path's length and `wire/`, unlink a stale
    /// socket (anything else at the path is refused and stays), bind,
    /// `chmod 0600`, listen, and serve. Must run inside a tokio runtime.
    pub fn start(
        lock: Arc<DataLock>,
        dispatch: Arc<dyn Dispatch>,
        limits: Limits,
        hooks: Hooks,
    ) -> Result<Self, ServeError> {
        let socket = prepare_wire_dir(lock.data_dir()).map_err(ServeError::Place)?;
        clear_stale(&lock, &socket).map_err(ServeError::Place)?;
        let bind_error = |error| ServeError::Bind {
            path: socket.clone(),
            error,
        };
        let listener = UnixListener::bind(&socket).map_err(bind_error)?;
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))
                .map_err(bind_error)?;
        }
        let shared = Arc::new(Shared {
            calls: Arc::new(Counter::new(limits.listener_calls)),
            connections: Arc::new(Counter::new(limits.connections)),
            limits,
            hooks,
            dispatch,
            open_streams: AtomicUsize::new(0),
            longest_line_held: AtomicUsize::new(0),
        });
        let (shutdown, watching) = watch::channel(false);
        let accept = tokio::spawn(accept_loop(listener, Arc::clone(&shared), watching));
        tracing::info!(socket = %socket.display(), "keeper's socket is listening");
        Ok(Self {
            shared,
            socket,
            shutdown,
            accept: Some(accept),
            lock,
            stopped: false,
        })
    }

    /// The socket's path.
    #[must_use]
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Streams whose handler is alive.
    #[must_use]
    pub fn open_streams(&self) -> usize {
        self.shared.open_streams.load(Ordering::Acquire)
    }

    /// Calls holding a place on the listener.
    #[must_use]
    pub fn calls_in_flight(&self) -> usize {
        self.shared.calls.used()
    }

    /// The most bytes of one unfinished line any reader has held.
    #[must_use]
    pub fn longest_line_held(&self) -> usize {
        self.shared.longest_line_held.load(Ordering::Acquire)
    }

    /// Stop accepting; end each connection with one `close` (`UNAVAILABLE`,
    /// switched off), waiting at most a second for them; unlink the socket
    /// while the lock is still held. The lock itself is the caller's.
    pub async fn stop(mut self) {
        self.stop_inner().await;
    }

    async fn stop_inner(&mut self) {
        let _ = self.shutdown.send(true);
        if let Some(accept) = self.accept.take() {
            if let Ok(mut connections) = accept.await {
                let drained = tokio::time::timeout(Duration::from_secs(1), async {
                    while connections.join_next().await.is_some() {}
                })
                .await;
                if drained.is_err() {
                    connections.abort_all();
                }
            }
        }
        self.unlink();
    }

    fn unlink(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        // Under the lock, the socket at the path is ours.
        let _ = clear_stale(&self.lock, &self.socket);
        tracing::info!(socket = %self.socket.display(), "keeper's socket stopped");
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
        if let Some(accept) = self.accept.take() {
            accept.abort();
        }
        self.unlink();
    }
}

async fn accept_loop(
    listener: UnixListener,
    shared: Arc<Shared>,
    mut shutdown: watch::Receiver<bool>,
) -> JoinSet<()> {
    let mut connections = JoinSet::new();
    loop {
        let accepted = tokio::select! {
            accepted = listener.accept() => accepted,
            _ = shutdown.wait_for(|stop| *stop) => break,
        };
        while connections.try_join_next().is_some() {}
        let stream = match accepted {
            Ok((stream, _)) => stream,
            Err(error) => {
                tracing::warn!(%error, "keeper's socket failed to accept");
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
        };
        let Some(peer) = check_peer(stream, shared.hooks.expected_uid()) else {
            continue;
        };
        let shared = Arc::clone(&shared);
        let shutdown = shutdown.clone();
        connections.spawn(conn::serve(peer, shared, shutdown));
    }
    connections
}
