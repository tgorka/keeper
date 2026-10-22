//! The one adapter of [`crate::hq::port`]: a `syncular_client::SyncClient` on
//! a replica file, driven as a blocking host (SPEC §8.4) on its own owner
//! thread, with realtime over the `native-transport` WebSocket.
//!
//! Every request the client makes passes through a [`GatedTransport`]: a gate
//! any thread can close, after which no request starts, and an origin check
//! that refuses a server-issued blob URL on any host but the replica's own
//! server — the only host the egress list names for it. Closing a replica
//! closes its gate before anything else; a refused credential closes it for
//! good and turns the client's own transport switch off.
//!
//! The owner thread is the only thread that touches the client. Its timing is
//! [`Schedule`]'s: a round at least every [`HEARTBEAT`], because the client
//! cannot see an idle socket die; a round that cannot reach the server backs
//! off (1 s doubling to a minute); a socket that will not open backs off on
//! its own streak, which a successful HTTP round does not reset; a refused
//! round stops the link. Reads go to a second, read-only connection on the
//! same file (`FileQuerySnapshotReader`, WAL), so a read never queues behind a
//! round.
//!
//! The blob cache (`hq::blob_cache`): the client's own cap is set from
//! [`BLOB_CACHE_MAX_BYTES`], and the owner keeps the cached bodies under it
//! by keeper's rule — least recently used first, never a body the outbox
//! pins — through a third connection on the same file, used only on the
//! owner thread, after an upload, an open and a round.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};
use syncular_client::api::{
    ClientDiagnosticsRequest, CommitDelivery, CommitOperationOutcome, CommitOutcomeResolution,
    CommitOutcomeStatus, ResolveCommitOutcomeInput, WindowCoverage,
};
use syncular_client::native_transport::{HostTransport, Inbound};
use syncular_client::{
    BlobDownload, BlobUploadGrant, ClientLimits, FileQuerySnapshotReader, Mutation, RealtimeState,
    RemoteClientError, SegmentRequest, SyncClient, SyncIntent, SyncOutcome, SyncRemoteClient,
    Transport, TransportError,
};
use tokio::sync::watch;

use crate::hq::blob_cache::{evictions, AccessLog, CachedBlob, BLOB_CACHE_MAX_BYTES};
use crate::hq::port::{
    Covered, Delivery, LinkState, Opener, Peer, Refusal, RefusedCommit, Rejection, Replica,
    ReplicaError, ReplicaSpec, ReplicaState, RowOp, RowWrites, Subscription, WindowBase,
    WindowState, CONFLICT,
};

/// A round at least this often while linked, socket or not.
pub const HEARTBEAT: Duration = Duration::from_secs(5);
/// One HTTP request's deadline; a firewalled server costs a round at most this.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// One round's deadline across every request it makes.
const ROUND_DEADLINE: Duration = Duration::from_secs(30);
/// The longest wait between two attempts on a server that cannot be reached.
const MAX_BACKOFF: Duration = Duration::from_secs(60);

/// The contract's schema IR, compiled into keeper (`syncular/`).
const SCHEMA_IR: &str = include_str!("../../../../../syncular/syncular.ir.json");

/// The contract's schema, parsed once.
static SCHEMA: LazyLock<Result<Value, String>> =
    LazyLock::new(|| serde_json::from_str(SCHEMA_IR).map_err(|error| error.to_string()));

/// The contract's schema.
pub fn schema() -> Result<&'static Value, ReplicaError> {
    SCHEMA
        .as_ref()
        .map_err(|error| ReplicaError::Client(format!("the contract's schema: {error}")))
}

/// The code a request refused by a closed gate fails with.
const CLOSED: &str = "transport.closed";
/// The code a server-issued URL off the server's origin fails with.
const FOREIGN: &str = "transport.foreign_origin";

/// The client's transport behind a gate and an origin check.
pub(crate) struct GatedTransport {
    inner: HostTransport,
    open: Arc<AtomicBool>,
    /// The replica's server; `None` for a replica that never links.
    origin: Option<url::Origin>,
    /// What became of the last remote operation: `None` while none passed
    /// the gate (nothing was sent), `Some(false)` when it went out and its
    /// transport failed, `Some(true)` when an answer's bytes came back.
    reached: Option<bool>,
}

impl GatedTransport {
    fn check(&self) -> Result<(), TransportError> {
        if self.open.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(TransportError::new(
                CLOSED,
                "this replica's network is closed",
            ))
        }
    }

    /// A URL the server handed out (a signed segment, a blob download or
    /// upload grant) is fetched only on the server's own origin.
    fn check_url(&self, url: &str) -> Result<(), TransportError> {
        self.check()?;
        let same = url::Url::parse(url)
            .ok()
            .is_some_and(|url| Some(url.origin()) == self.origin);
        if same {
            Ok(())
        } else {
            Err(TransportError::new(
                FOREIGN,
                "the server named a host other than its own; keeper contacts only the agents server",
            ))
        }
    }
}

impl Transport for GatedTransport {
    fn sync(&mut self, request: &[u8]) -> Result<Vec<u8>, TransportError> {
        self.check()?;
        self.inner.sync(request)
    }
    fn remote_operation(&mut self, request: &[u8]) -> Result<Vec<u8>, TransportError> {
        self.check()?;
        let answer = self.inner.remote_operation(request);
        self.reached = Some(answer.is_ok());
        answer
    }
    fn realtime_sync(&mut self, request: &[u8]) -> Result<Vec<u8>, TransportError> {
        self.check()?;
        self.inner.realtime_sync(request)
    }
    fn download_segment(
        &mut self,
        request: &SegmentRequest,
        on_progress: &mut dyn FnMut(u64),
    ) -> Result<Vec<u8>, TransportError> {
        self.check()?;
        self.inner.download_segment(request, on_progress)
    }
    fn supports_url_fetch(&self) -> bool {
        self.inner.supports_url_fetch()
    }
    fn fetch_url(
        &mut self,
        url: &str,
        on_progress: &mut dyn FnMut(u64),
    ) -> Result<Vec<u8>, TransportError> {
        self.check_url(url)?;
        self.inner.fetch_url(url, on_progress)
    }
    fn segment_fetch_started_at(&self, url: &str) -> Option<i64> {
        self.inner.segment_fetch_started_at(url)
    }
    fn blob_upload(
        &mut self,
        blob_id: &str,
        bytes: &[u8],
        media_type: Option<&str>,
    ) -> Result<(), TransportError> {
        self.check()?;
        self.inner.blob_upload(blob_id, bytes, media_type)
    }
    fn blob_download(&mut self, blob_id: &str) -> Result<BlobDownload, TransportError> {
        self.check()?;
        self.inner.blob_download(blob_id)
    }
    fn fetch_blob_url(&mut self, url: &str) -> Result<Vec<u8>, TransportError> {
        self.check_url(url)?;
        self.inner.fetch_blob_url(url)
    }
    fn blob_upload_grant(
        &mut self,
        blob_id: &str,
        byte_length: u64,
        media_type: Option<&str>,
    ) -> Result<BlobUploadGrant, TransportError> {
        self.check()?;
        self.inner
            .blob_upload_grant(blob_id, byte_length, media_type)
    }
    fn blob_put_url(
        &mut self,
        url: &str,
        bytes: &[u8],
        media_type: Option<&str>,
    ) -> Result<(), TransportError> {
        self.check_url(url)?;
        self.inner.blob_put_url(url, bytes, media_type)
    }
    fn realtime_connect(&mut self) -> Result<(), TransportError> {
        self.check()?;
        self.inner.realtime_connect()
    }
    fn realtime_connect_for_client(&mut self, client_id: &str) -> Result<(), TransportError> {
        self.check()?;
        self.inner.realtime_connect_for_client(client_id)
    }
    fn round_deadline(&self) -> Option<Duration> {
        self.inner.round_deadline()
    }
    fn set_round_deadline(&mut self, deadline: Option<Instant>) {
        self.inner.set_round_deadline(deadline);
    }
    fn realtime_send(&mut self, text: &str) -> Result<(), TransportError> {
        self.check()?;
        self.inner.realtime_send(text)
    }
    fn realtime_close(&mut self) -> Result<(), TransportError> {
        // Closing sends at most a close frame on a socket already open.
        self.inner.realtime_close()
    }
}

/// What one round's outcome means for the link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RoundVerdict {
    /// The server answered.
    Answered,
    /// The server refused this device's credential.
    Refused,
    /// The server could not be reached, or failed; try again later.
    Unreachable(String),
}

fn http_status(details: Option<&Value>) -> Option<u64> {
    details
        .and_then(|details| details.get("httpStatus"))
        .and_then(Value::as_u64)
}

/// Read a round's outcome. A 401 or 403 — or the client's own
/// `sync.auth_required` — is the server refusing this credential; anything
/// else that failed is the network or the server being unwell.
pub(crate) fn verdict(outcome: &SyncOutcome) -> RoundVerdict {
    match outcome {
        SyncOutcome::Ok(_) | SyncOutcome::BudgetExhausted(_) => RoundVerdict::Answered,
        SyncOutcome::Failed {
            error_code,
            message,
            details,
        } => {
            let status = http_status(details.as_ref());
            if error_code == "sync.auth_required"
                || error_code == "sync.forbidden"
                || matches!(status, Some(401 | 403))
            {
                RoundVerdict::Refused
            } else {
                RoundVerdict::Unreachable(format!("{error_code}: {message}"))
            }
        }
        SyncOutcome::RealtimeUnavailable { state, .. } => {
            RoundVerdict::Unreachable(format!("realtime {state:?}"))
        }
    }
}

/// The server's refusal of a blob upload, read from a round that failed on
/// one. The client's transport answers a refused upload with
/// `transport.failed` and the HTTP status, not the server's code: a `413` on
/// the upload grant or the `PUT` is the server's `blob.too_large`, a `400` on
/// the `PUT` its `blob.hash_mismatch`. Any other failure is not an upload's.
pub(crate) fn upload_refusal(outcome: &SyncOutcome) -> Option<Rejection> {
    let SyncOutcome::Failed {
        message, details, ..
    } = outcome
    else {
        return None;
    };
    if !message.starts_with("blob upload") {
        return None;
    }
    match http_status(details.as_ref()) {
        Some(413) => Some(Rejection::new(
            "blob.too_large",
            "the server refused a file larger than it takes",
        )),
        Some(400) => Some(Rejection::new(
            "blob.hash_mismatch",
            "the server found the uploaded bytes differ from their address",
        )),
        _ => None,
    }
}

/// The wait before the next attempt after `failures` failures in a row.
pub(crate) fn backoff(failures: u32) -> Duration {
    let exponent = failures.saturating_sub(1).min(6);
    Duration::from_secs(1u64 << exponent).min(MAX_BACKOFF)
}

/// One capability's failure streak and when it may be tried again.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Backoff {
    failures: u32,
    until: Option<Instant>,
}

impl Backoff {
    fn fail(&mut self, now: Instant) {
        self.failures = self.failures.saturating_add(1);
        self.until = Some(now + backoff(self.failures));
    }
    fn reset(&mut self) {
        *self = Backoff::default();
    }
    fn ready(&self, now: Instant) -> bool {
        self.until.is_none_or(|until| now >= until)
    }
    fn wait(&self, now: Instant) -> Duration {
        self.until
            .map_or(Duration::ZERO, |until| until.saturating_duration_since(now))
    }
}

/// When a linked owner runs a round and opens its socket. HTTP rounds and the
/// socket keep separate streaks: a server whose HTTP answers while its
/// upgrades fail backs the socket off toward a minute and keeps the rounds on
/// the heartbeat.
#[derive(Debug, Clone, Default)]
pub(crate) struct Schedule {
    http: Backoff,
    socket: Backoff,
    last_round: Option<Instant>,
    refused: bool,
}

impl Schedule {
    fn heartbeat_due(&self, now: Instant) -> bool {
        self.last_round
            .is_none_or(|last| now.saturating_duration_since(last) >= HEARTBEAT)
    }

    pub(crate) fn round_due(&self, now: Instant, wanted: bool) -> bool {
        !self.refused && self.http.ready(now) && (wanted || self.heartbeat_due(now))
    }

    pub(crate) fn round_done(&mut self, verdict: &RoundVerdict, now: Instant) {
        self.last_round = Some(now);
        match verdict {
            RoundVerdict::Answered => self.http.reset(),
            RoundVerdict::Refused => self.refused = true,
            RoundVerdict::Unreachable(_) => self.http.fail(now),
        }
    }

    /// A socket is tried only while HTTP answers, so an unreachable server
    /// costs one backoff, not two.
    pub(crate) fn socket_due(&self, now: Instant) -> bool {
        !self.refused && self.http.failures == 0 && self.socket.ready(now)
    }

    pub(crate) fn socket_done(&mut self, opened: bool, now: Instant) {
        if opened {
            self.socket.reset();
        } else {
            self.socket.fail(now);
        }
    }

    /// How long the owner may sleep; `None` until a command arrives.
    pub(crate) fn next_wake(
        &self,
        now: Instant,
        wanted: bool,
        connected: bool,
    ) -> Option<Duration> {
        if self.refused {
            return None;
        }
        let round = if !self.http.ready(now) {
            self.http.wait(now)
        } else if wanted {
            Duration::ZERO
        } else {
            self.last_round.map_or(Duration::ZERO, |last| {
                (last + HEARTBEAT).saturating_duration_since(now)
            })
        };
        let socket = (!connected && self.http.failures == 0).then(|| self.socket.wait(now));
        Some(socket.map_or(round, |socket| socket.min(round)))
    }

    pub(crate) fn link(&self, connected: bool) -> LinkState {
        if self.refused {
            LinkState::Refused
        } else if self.http.failures > 0 {
            LinkState::Offline
        } else if self.last_round.is_some() && connected {
            LinkState::Connected
        } else {
            LinkState::Connecting
        }
    }
}

/// A presence publish the server refused (`presence.forbidden`,
/// `presence.too_large`) comes back as a frame to the publisher alone: its
/// key.
fn refused_presence(text: &str) -> Option<String> {
    if !text.contains("\"error\"") {
        return None;
    }
    let frame = serde_json::from_str::<Value>(text).ok()?;
    if frame["event"] != "presence" || frame["data"]["error"].as_str().is_none() {
        return None;
    }
    Some(frame["data"]["scopeKey"].as_str()?.to_owned())
}

/// Whether the peers already present on one key have reached this replica
/// over the current socket — keeper-hq's `hq-runtime` `PresenceGate`, asked
/// with leaves rather than a document of its own, since a person's device
/// is not one of the directory's instances.
///
/// The server hands a socket the peers already on a key once the key is
/// among the socket's registrations — at connect, or when the registration
/// refresh after a round adds it — in the same step that registers it,
/// before it judges anything that socket sends later. A connected socket
/// proves none of that. So the replica asks: it sends a leave on the key,
/// which a registered key takes silently (nothing of this device is there
/// to leave) and an unregistered one refuses, then a leave on a key it
/// never holds, which the server always refuses — after any refusal of the
/// first, on the same socket. When the second refusal comes alone, the key
/// was registered when the first was judged, so every peer already on it
/// arrived before that answer.
#[derive(Debug, Default)]
pub(crate) struct PresenceGate {
    key: Option<String>,
    /// The ask in flight: its never-held key, and whether the server
    /// refused `key` before answering it.
    probe: Option<(String, bool)>,
    /// The rounds on this socket when an ask found `key` unregistered: the
    /// next ask waits for another round, where registrations change.
    retry_after: Option<u32>,
    asked: u64,
    ready: bool,
}

impl PresenceGate {
    pub(crate) fn watching(key: Option<String>) -> PresenceGate {
        PresenceGate {
            key,
            ..PresenceGate::default()
        }
    }

    /// A new socket: nothing of it is known yet.
    pub(crate) fn reset(&mut self) {
        self.probe = None;
        self.retry_after = None;
        self.ready = false;
    }

    pub(crate) fn ready(&self) -> bool {
        self.ready
    }

    /// The watched key and the never-held key to ask on now, if an ask is
    /// due.
    pub(crate) fn due(&mut self, rounds: u32, client: &str) -> Option<(String, String)> {
        if self.ready || self.probe.is_some() {
            return None;
        }
        let key = self.key.clone()?;
        if self.retry_after.is_some_and(|at| rounds <= at) {
            return None;
        }
        self.asked += 1;
        let probe = format!("probe:{client}:{}", self.asked);
        self.probe = Some((probe.clone(), false));
        Some((key, probe))
    }

    /// The ask could not be sent: the next one is due at once.
    pub(crate) fn unasked(&mut self) {
        self.probe = None;
    }

    /// The server refused a publish on `scope` (`rounds` run on the socket).
    pub(crate) fn refused(&mut self, scope: &str, rounds: u32) {
        let watched = self.key.as_deref() == Some(scope);
        let Some((probe, key_refused)) = &mut self.probe else {
            return;
        };
        if watched {
            *key_refused = true;
            return;
        }
        if probe != scope {
            return;
        }
        if *key_refused {
            self.retry_after = Some(rounds);
        } else {
            self.ready = true;
        }
        self.probe = None;
    }
}

/// The standing refusals of commits written here that wrote one row, newest
/// first: the replica's outcome journal keeps a refused commit's operations
/// until it is resolved.
const ROW_REFUSED: &str =
    "SELECT o.client_commit_id AS commit_id FROM _syncular_commit_outcomes o, \
     json_each(o.operations_json) op \
     WHERE o.resolution = 'active' AND o.status IN ('conflict', 'rejected') \
     AND json_extract(op.value, '$.table') = ?1 AND json_extract(op.value, '$.rowId') = ?2 \
     GROUP BY o.seq ORDER BY o.seq DESC";

/// Whether a commit in the outbox writes one row.
const ROW_PENDING: &str = "SELECT 1 AS pending FROM _syncular_outbox o, json_each(o.ops_json) op \
     WHERE json_extract(op.value, '$.table') = ?1 AND json_extract(op.value, '$.rowId') = ?2 \
     LIMIT 1";

/// The standing refusals of commits written here that wrote a row of one
/// table, newest first, with every operation each wrote.
const TABLE_REFUSED: &str =
    "SELECT o.client_commit_id AS commit_id, o.operations_json AS operations \
     FROM _syncular_commit_outcomes o \
     WHERE o.resolution = 'active' AND o.status IN ('conflict', 'rejected') \
     AND EXISTS (SELECT 1 FROM json_each(o.operations_json) op \
     WHERE json_extract(op.value, '$.table') = ?1) \
     ORDER BY o.seq DESC";

/// The bodies the client caches, with whether the outbox pins each: staged
/// for upload, or referenced by a commit the server has not taken.
const CACHED_BODIES: &str = "SELECT b.blob_id, b.byte_length, b.created_at_ms, \
     (EXISTS (SELECT 1 FROM _syncular_blob_uploads u WHERE u.blob_id = b.blob_id) \
     OR EXISTS (SELECT 1 FROM _syncular_blob_commit_refs r WHERE r.blob_id = b.blob_id)) \
     FROM _syncular_blobs b";

/// One body dropped, unless the outbox pinned it since it was weighed.
const DROP_BODY: &str = "DELETE FROM _syncular_blobs WHERE blob_id = ?1 \
     AND NOT EXISTS (SELECT 1 FROM _syncular_blob_uploads u WHERE u.blob_id = ?1) \
     AND NOT EXISTS (SELECT 1 FROM _syncular_blob_commit_refs r WHERE r.blob_id = ?1)";

/// The replica file's blob bodies, as the owner keeps them under the cap,
/// and when each was last used.
struct BlobCache {
    bodies: rusqlite::Connection,
    log: AccessLog,
}

impl BlobCache {
    fn open(replica: &Path) -> Result<BlobCache, ReplicaError> {
        let failed = |error: rusqlite::Error| {
            ReplicaError::Client(format!("the replica's blob cache: {error}"))
        };
        let bodies = rusqlite::Connection::open(replica).map_err(failed)?;
        bodies
            .busy_timeout(Duration::from_secs(5))
            .map_err(failed)?;
        let log = AccessLog::open(&AccessLog::beside(replica)).map_err(failed)?;
        Ok(BlobCache { bodies, log })
    }

    /// `blob_id` was opened or written now.
    fn touch(&self, database: &str, blob_id: &str) {
        if let Err(error) = self.log.touch(blob_id, crate::hq::sessions::now_ms()) {
            tracing::debug!(%database, %error, "a blob's last use was not recorded");
        }
    }

    /// Drop what [`evictions`] names once the bodies pass `max`; the ids
    /// dropped.
    fn enforce(&self, max: u64) -> Result<Vec<String>, rusqlite::Error> {
        let total: i64 = self.bodies.query_row(
            "SELECT COALESCE(SUM(byte_length), 0) FROM _syncular_blobs",
            [],
            |row| row.get(0),
        )?;
        if u64::try_from(total).unwrap_or(0) <= max {
            return Ok(Vec::new());
        }
        let used = self.log.used()?;
        let mut statement = self.bodies.prepare(CACHED_BODIES)?;
        let cached = statement
            .query_map([], |row| {
                let blob_id: String = row.get(0)?;
                let bytes: i64 = row.get(1)?;
                let cached_at: i64 = row.get(2)?;
                let pinned: bool = row.get(3)?;
                Ok(CachedBlob {
                    used_at_ms: used.get(&blob_id).copied().unwrap_or(cached_at),
                    blob_id,
                    bytes: u64::try_from(bytes).unwrap_or(0),
                    pinned,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let doomed = evictions(&cached, max);
        if doomed.is_empty() {
            return Ok(doomed);
        }
        let mut dropped = Vec::with_capacity(doomed.len());
        for blob_id in doomed {
            if self.bodies.execute(DROP_BODY, [&blob_id])? > 0 {
                dropped.push(blob_id);
            }
        }
        self.log.forget(&dropped)?;
        Ok(dropped)
    }
}

type Reply<T> = SyncSender<Result<T, ReplicaError>>;

enum Command {
    Mutate(Vec<Mutation>, Reply<String>),
    Subscribe(Subscription, Reply<()>),
    SetWindow(WindowBase, Vec<String>, Reply<()>),
    WindowState(WindowBase, Reply<WindowState>),
    SetPresence {
        scope_key: String,
        doc: Option<Value>,
        reply: Reply<()>,
    },
    Presence(String, Reply<Vec<Peer>>),
    Supersede(Vec<String>, String, Reply<()>),
    Upload(Vec<u8>, Option<String>, Reply<Value>),
    Fetch(String, Reply<Vec<u8>>),
    CacheBytes(Reply<u64>),
    SyncNow,
    /// A socket frame arrived.
    Wake,
    Stop,
}

fn mutation(op: RowOp) -> Mutation {
    match op {
        RowOp::Upsert {
            table,
            values,
            base_version,
        } => Mutation::Upsert {
            table,
            values,
            base_version,
        },
        RowOp::Patch {
            table,
            values,
            base_version,
        } => Mutation::Patch {
            table,
            values,
            base_version,
        },
        RowOp::Delete {
            table,
            row_id,
            base_version,
        } => Mutation::Delete {
            table,
            row_id,
            base_version,
        },
    }
}

fn window_base(base: &WindowBase) -> syncular_client::WindowBase {
    syncular_client::WindowBase {
        table: base.table.clone(),
        variable: base.variable.clone(),
        fixed_scopes: base.fixed_scopes.clone(),
        params: None,
    }
}

struct Owner {
    database: String,
    client: SyncClient,
    transport: GatedTransport,
    linked: bool,
    schedule: Schedule,
    /// A local write or an asked-for round waits to be pushed.
    dirty: bool,
    directory: u64,
    rows: u64,
    presence: u64,
    /// Whether the peers already on the watched key have arrived over this
    /// socket.
    gate: PresenceGate,
    /// Rounds run over the current socket.
    rounds_on_socket: u32,
    state: watch::Sender<ReplicaState>,
    /// The most bytes of blob bodies kept beyond what the outbox pins.
    cache_max: u64,
    cache: BlobCache,
    /// The server's refusal of the last upload a round tried, shared with
    /// the replica's readers.
    upload_refusal: Arc<Mutex<Option<Rejection>>>,
}

impl Owner {
    fn open(
        spec: &ReplicaSpec,
        gate: Arc<AtomicBool>,
        wake: Sender<Command>,
    ) -> Result<Owner, ReplicaError> {
        let path = spec
            .path
            .to_str()
            .ok_or_else(|| ReplicaError::Client("the replica's path is not UTF-8".to_owned()))?;
        let limits = ClientLimits {
            blob_cache_max_bytes: Some(i64::try_from(BLOB_CACHE_MAX_BYTES).unwrap_or(i64::MAX)),
            ..ClientLimits::default()
        };
        let mut client =
            SyncClient::open_path_with_identity(spec.client_id.clone(), schema()?, limits, path)
                .map_err(ReplicaError::Client)?;
        let cache = BlobCache::open(&spec.path)?;
        let (config, origin) = match &spec.link {
            Some(link) => {
                let (config, origin) = link_config(link)?;
                (config, Some(origin))
            }
            // No `baseUrl`: the transport that refuses every network call.
            None => (json!({}), None),
        };
        let notify: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            let _ = wake.send(Command::Wake);
        });
        let inner = HostTransport::from_config_with_notify(&config, Some(notify))
            .map_err(ReplicaError::Client)?;
        let linked = spec.link.is_some();
        if !linked {
            gate.store(false, Ordering::SeqCst);
        }
        let mut transport = GatedTransport {
            inner,
            open: gate,
            origin,
            reached: None,
        };
        if !linked {
            client.set_transport_enabled(&mut transport, false);
        }
        for subscription in &spec.subscriptions {
            client
                .subscribe(
                    subscription.id.clone(),
                    subscription.table.clone(),
                    subscription.scopes.clone(),
                    None,
                )
                .map_err(ReplicaError::Client)?;
        }
        let outbox = client.status_snapshot().outbox;
        let link = if linked {
            LinkState::Connecting
        } else {
            LinkState::Local
        };
        let (state, _) = watch::channel(ReplicaState {
            link,
            outbox,
            directory: 0,
            rows: 0,
            presence: 0,
            presence_known: false,
        });
        Ok(Owner {
            database: spec.database.clone(),
            client,
            transport,
            linked,
            schedule: Schedule::default(),
            dirty: true,
            directory: 0,
            rows: 0,
            presence: 0,
            gate: PresenceGate::watching(spec.watch_presence.clone()),
            rounds_on_socket: 0,
            state,
            cache_max: BLOB_CACHE_MAX_BYTES,
            cache,
            upload_refusal: Arc::new(Mutex::new(None)),
        })
    }

    /// Whether this owner may touch the network now.
    fn online(&self) -> bool {
        self.linked && !self.schedule.refused && self.transport.open.load(Ordering::SeqCst)
    }

    fn connected(&self) -> bool {
        self.client.realtime_state() == RealtimeState::Connected
    }

    fn run(mut self, commands: Receiver<Command>) {
        loop {
            let wake = if self.online() {
                self.schedule.next_wake(
                    Instant::now(),
                    self.dirty || self.client.sync_needed(),
                    self.connected(),
                )
            } else {
                None
            };
            let command = match wake {
                Some(wait) => match commands.recv_timeout(wait) {
                    Ok(command) => Some(command),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => break,
                },
                None => match commands.recv() {
                    Ok(command) => Some(command),
                    Err(_) => break,
                },
            };
            if let Some(command) = command {
                if !self.handle(command) {
                    break;
                }
                // Everything already queued before the network is touched.
                let mut stop = false;
                while let Ok(command) = commands.try_recv() {
                    if !self.handle(command) {
                        stop = true;
                        break;
                    }
                }
                if stop {
                    break;
                }
            }
            self.absorb_changes();
            self.drive();
            self.publish();
        }
        self.stop();
    }

    /// One command; `false` to stop.
    fn handle(&mut self, command: Command) -> bool {
        match command {
            Command::Mutate(ops, reply) => {
                let outcome = self.client.mutate(ops).map_err(|error| {
                    ReplicaError::Client(format!("{}: {}", error.code, error.message))
                });
                self.dirty |= outcome.is_ok();
                let _ = reply.send(outcome);
            }
            Command::Subscribe(subscription, reply) => {
                let outcome = self
                    .client
                    .subscribe(
                        subscription.id,
                        subscription.table,
                        subscription.scopes,
                        None,
                    )
                    .map_err(ReplicaError::Client);
                self.dirty |= outcome.is_ok();
                let _ = reply.send(outcome);
            }
            Command::SetWindow(base, units, reply) => {
                let outcome = self
                    .client
                    .set_window(&window_base(&base), &units)
                    .map(|_| ())
                    .map_err(ReplicaError::Client);
                self.dirty |= outcome.is_ok();
                let _ = reply.send(outcome);
            }
            Command::WindowState(base, reply) => {
                let state = self.client.window_state(&window_base(&base));
                let _ = reply.send(Ok(WindowState {
                    units: state.units,
                    pending: state.pending,
                }));
            }
            Command::SetPresence {
                scope_key,
                doc,
                reply,
            } => {
                let outcome = if self.online() {
                    self.client
                        .set_presence(&mut self.transport, &scope_key, doc.as_ref())
                        .map_err(ReplicaError::Client)
                } else {
                    Err(ReplicaError::NotSyncing)
                };
                let _ = reply.send(outcome);
            }
            Command::Presence(scope_key, reply) => {
                let peers = self
                    .client
                    .presence(&scope_key)
                    .into_iter()
                    .map(|peer| Peer {
                        actor_id: peer.actor_id,
                        client_id: peer.client_id,
                        doc: peer.doc,
                    })
                    .collect();
                let _ = reply.send(Ok(peers));
            }
            Command::Supersede(commits, replacement, reply) => {
                let mut outcome = Ok(());
                for commit in commits {
                    let resolved = self
                        .client
                        .resolve_commit_outcome(ResolveCommitOutcomeInput {
                            client_commit_id: commit,
                            resolution: CommitOutcomeResolution::Superseded,
                            replacement_client_commit_id: Some(replacement.clone()),
                        });
                    if let Err(error) = resolved {
                        outcome = Err(ReplicaError::Client(error));
                    }
                }
                let _ = reply.send(outcome);
            }
            Command::Upload(bytes, media_type, reply) => {
                let outcome = self
                    .client
                    .upload_blob(&bytes, media_type, None)
                    .map_err(ReplicaError::Client);
                if let Some(blob) = outcome
                    .as_ref()
                    .ok()
                    .and_then(|reference| reference["blobId"].as_str())
                {
                    self.cache.touch(&self.database, blob);
                }
                self.dirty |= outcome.is_ok();
                let _ = reply.send(outcome);
                self.enforce_cache();
            }
            Command::Fetch(blob, reply) => {
                // With the network closed the client serves its cache and
                // refuses a miss itself; the gate refuses it again.
                let fetched = self
                    .client
                    .fetch_blob_bytes(&mut self.transport, &blob)
                    .map(|fetched| {
                        self.cache.touch(&self.database, &fetched.blob_id);
                        fetched.bytes
                    })
                    .map_err(|(code, message)| ReplicaError::Client(format!("{code}: {message}")));
                let _ = reply.send(fetched);
                self.enforce_cache();
            }
            Command::CacheBytes(reply) => {
                let bytes = self
                    .client
                    .diagnostics_snapshot(&ClientDiagnosticsRequest::default())
                    .map_err(ReplicaError::Client)
                    .and_then(|snapshot| {
                        snapshot
                            .storage
                            .blob_cache_bytes_approx
                            .and_then(|bytes| u64::try_from(bytes).ok())
                            .ok_or_else(|| {
                                ReplicaError::Client(
                                    "the replica's blob cache could not be measured".to_owned(),
                                )
                            })
                    });
                let _ = reply.send(bytes);
            }
            Command::SyncNow => self.dirty = true,
            Command::Wake => {}
            Command::Stop => return false,
        }
        true
    }

    /// Frames, the rounds they and the schedule ask for, and the socket.
    fn drive(&mut self) {
        if !self.online() {
            return;
        }
        self.take_frames();
        let asked = self
            .client
            .drain_sync_intents()
            .iter()
            .any(|intent| !matches!(intent, SyncIntent::None));
        self.absorb_changes();
        let wanted = asked || self.dirty || self.client.sync_needed();
        if self.schedule.round_due(Instant::now(), wanted) {
            self.round();
        }
        self.ask_presence();
        if !self.online() || self.connected() || !self.schedule.socket_due(Instant::now()) {
            return;
        }
        match self.new_socket() {
            Ok(()) => {
                self.schedule.socket_done(true, Instant::now());
                self.round();
                self.ask_presence();
            }
            Err(error) => {
                self.schedule.socket_done(false, Instant::now());
                tracing::debug!(database = %self.database, %error, "agents server socket refused");
            }
        }
    }

    /// The frames the socket's reader queued since the last call.
    fn take_frames(&mut self) {
        for frame in self.transport.inner.take_inbound() {
            match frame {
                // Hello, presence and wake frames come as text; only a
                // presence fanout changes a peer, and telling them apart
                // here would parse each frame twice.
                Inbound::Text(text) => {
                    self.client.on_realtime_text(&text);
                    if let Some(scope) = refused_presence(&text) {
                        self.gate.refused(&scope, self.rounds_on_socket);
                    }
                    self.presence += 1;
                }
                Inbound::Binary(bytes) => {
                    self.client.on_realtime_binary(&mut self.transport, &bytes)
                }
            }
        }
    }

    /// A socket of its own: whatever the last one saw is gone first, and
    /// nothing of it is known of this one yet.
    fn new_socket(&mut self) -> Result<(), String> {
        self.release_socket();
        self.gate.reset();
        self.rounds_on_socket = 0;
        self.client.connect_realtime(&mut self.transport)
    }

    /// Let the socket go: its peers and every frame it left behind.
    ///
    /// Its reader shares one queue with the next socket's, and a frame it
    /// queued while a round ran — after the last drain — would otherwise
    /// reach the next socket's peers: a join of an instance that has since
    /// stopped, which that socket's snapshot never mentions and so nothing
    /// would ever remove. So the reader is joined first, after which nothing
    /// joins the queue until a new socket starts, and the queue is then
    /// emptied. A dropped row delta costs nothing: the next round pulls from
    /// the cursor it never moved.
    fn release_socket(&mut self) {
        // A lost socket is released too; one already let go is left alone.
        self.client.disconnect_realtime(&mut self.transport);
        self.transport.inner.shutdown();
        let stale = self.transport.inner.take_inbound().len();
        if stale > 0 {
            tracing::debug!(database = %self.database, stale, "frames of a released socket dropped");
        }
    }

    /// Change batches only say what changed: any change bumps the rows'
    /// revision, and a change of `clients` the directory's too.
    fn absorb_changes(&mut self) {
        let batches = self.client.drain_change_batches();
        if batches.is_empty() {
            return;
        }
        self.rows += 1;
        let directory = batches
            .iter()
            .any(|batch| batch.tables.iter().any(|change| change.table == "clients"));
        if directory {
            self.directory += 1;
        }
    }

    /// Ask whether the watched key's peers have arrived, when an ask is due
    /// ([`PresenceGate`]).
    fn ask_presence(&mut self) {
        if !self.online() || !self.connected() {
            return;
        }
        let Some((key, probe)) = self
            .gate
            .due(self.rounds_on_socket, self.client.client_id())
        else {
            return;
        };
        let asked = self
            .client
            .set_presence(&mut self.transport, &key, None)
            .and_then(|()| self.client.set_presence(&mut self.transport, &probe, None));
        if let Err(error) = asked {
            tracing::debug!(database = %self.database, %error, "the directory's presence was not asked after");
            self.gate.unasked();
        }
    }

    fn round(&mut self) {
        self.dirty = false;
        let outcome = self.client.sync_until_idle(&mut self.transport, None);
        if self.connected() {
            self.rounds_on_socket = self.rounds_on_socket.saturating_add(1);
        }
        let verdict = verdict(&outcome);
        self.schedule.round_done(&verdict, Instant::now());
        match upload_refusal(&outcome) {
            Some(refusal) => {
                tracing::warn!(database = %self.database, code = %refusal.code, "the agents server refused an upload; nothing more is pushed until it is resolved");
                *self
                    .upload_refusal
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = Some(refusal);
            }
            None if verdict == RoundVerdict::Answered => {
                *self
                    .upload_refusal
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = None;
            }
            None => {}
        }
        match verdict {
            RoundVerdict::Answered => {}
            RoundVerdict::Refused => {
                tracing::warn!(
                    database = %self.database,
                    "the agents server refused this device's credential; this replica stops syncing"
                );
                self.transport.open.store(false, Ordering::SeqCst);
                self.client
                    .set_transport_enabled(&mut self.transport, false);
                self.release_socket();
            }
            RoundVerdict::Unreachable(why) => {
                tracing::info!(
                    database = %self.database,
                    retry_in_ms = self.schedule.http.wait(Instant::now()).as_millis() as u64,
                    %why,
                    "agents server not reached"
                );
                if matches!(
                    self.client.realtime_state(),
                    RealtimeState::Connected | RealtimeState::Lost
                ) {
                    self.release_socket();
                }
            }
        }
        self.absorb_changes();
        // A commit the server took no longer pins its blobs.
        self.enforce_cache();
    }

    /// Keep the cached blob bodies under the cap ([`BlobCache::enforce`]).
    fn enforce_cache(&self) {
        match self.cache.enforce(self.cache_max) {
            Ok(dropped) if !dropped.is_empty() => {
                tracing::debug!(database = %self.database, dropped = dropped.len(), "least recently used blobs evicted from this device's cache");
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(database = %self.database, %error, "this device's blob cache could not be kept under its cap");
            }
        }
    }

    fn publish(&self) {
        let link = if !self.linked {
            LinkState::Local
        } else if !self.transport.open.load(Ordering::SeqCst) && !self.schedule.refused {
            LinkState::Closed
        } else {
            self.schedule.link(self.connected())
        };
        let next = ReplicaState {
            link,
            outbox: self.client.status_snapshot().outbox,
            directory: self.directory,
            rows: self.rows,
            presence: self.presence,
            presence_known: link == LinkState::Connected && self.gate.ready(),
        };
        self.state.send_if_modified(|state| {
            let changed = *state != next;
            *state = next;
            changed
        });
    }

    fn stop(&mut self) {
        self.transport.open.store(false, Ordering::SeqCst);
        self.client
            .set_transport_enabled(&mut self.transport, false);
        self.release_socket();
        let next = ReplicaState {
            link: LinkState::Closed,
            outbox: self.client.status_snapshot().outbox,
            directory: self.directory,
            rows: self.rows,
            presence: self.presence,
            presence_known: false,
        };
        self.state.send_replace(next);
    }
}

/// The transport configuration of a link, and its server's origin.
fn link_config(link: &crate::hq::port::Link) -> Result<(Value, url::Origin), ReplicaError> {
    let origin = url::Url::parse(&link.base_url)
        .map_err(|error| ReplicaError::Client(format!("the server's address: {error}")))?
        .origin();
    Ok((
        json!({
            "baseUrl": link.base_url,
            "headers": { "authorization": format!("Bearer {}", link.token) },
            "requestTimeoutMs": REQUEST_TIMEOUT.as_millis() as u64,
            "roundDeadlineMs": ROUND_DEADLINE.as_millis() as u64,
        }),
        origin,
    ))
}

/// What a remote query needs: its own transport per call, behind the
/// replica's gate, so a read never waits behind the owner's round.
struct Remote {
    config: Value,
    origin: url::Origin,
    client_id: String,
}

/// A replica whose owner thread runs [`Owner`].
pub struct SyncularReplica {
    commands: Sender<Command>,
    gate: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
    reader: Mutex<FileQuerySnapshotReader>,
    state: watch::Receiver<ReplicaState>,
    remote: Option<Remote>,
    upload_refusal: Arc<Mutex<Option<Rejection>>>,
}

impl SyncularReplica {
    /// Open (creating when absent) the replica at `spec.path`, register its
    /// subscriptions and start its owner thread. Returns once the file is
    /// open, before any request.
    pub fn open(spec: ReplicaSpec) -> Result<SyncularReplica, ReplicaError> {
        let path = spec
            .path
            .to_str()
            .ok_or_else(|| ReplicaError::Client("the replica's path is not UTF-8".to_owned()))?
            .to_owned();
        let (commands, inbox) = mpsc::channel::<Command>();
        let (ready, opened) = mpsc::sync_channel(1);
        let wake = commands.clone();
        let gate = Arc::new(AtomicBool::new(true));
        let owner_gate = Arc::clone(&gate);
        let upload_refusal = Arc::new(Mutex::new(None));
        let owner_refusal = Arc::clone(&upload_refusal);
        let remote = match (&spec.link, &spec.client_id) {
            (Some(link), Some(client_id)) => {
                let (config, origin) = link_config(link)?;
                Some(Remote {
                    config,
                    origin,
                    client_id: client_id.clone(),
                })
            }
            _ => None,
        };
        let thread = std::thread::Builder::new()
            .name(format!("hq-{}", spec.database))
            .spawn(move || match Owner::open(&spec, owner_gate, wake) {
                Ok(mut owner) => {
                    owner.upload_refusal = owner_refusal;
                    let _ = ready.send(Ok(owner.state.subscribe()));
                    owner.run(inbox);
                }
                Err(error) => {
                    let _ = ready.send(Err(error));
                }
            })
            .map_err(|error| ReplicaError::Client(format!("owner thread: {error}")))?;
        let state = opened.recv().map_err(|_| ReplicaError::Closed)??;
        Ok(SyncularReplica {
            commands,
            gate,
            thread: Mutex::new(Some(thread)),
            reader: Mutex::new(FileQuerySnapshotReader::new(path)),
            state,
            remote,
            upload_refusal,
        })
    }

    fn ask<T>(&self, command: impl FnOnce(Reply<T>) -> Command) -> Result<T, ReplicaError> {
        let (reply, answer) = mpsc::sync_channel(1);
        self.commands
            .send(command(reply))
            .map_err(|_| ReplicaError::Closed)?;
        answer.recv().map_err(|_| ReplicaError::Closed)?
    }
}

/// The code the client gives an answer it could not decode or validate.
const UNREADABLE: &str = "client.invalid_host_response";

/// What a failed command means, by how far it got (`reached`, from the
/// gate): never sent is the client's own refusal (or a closed network); sent
/// with no answer back, or with an answer keeper could not read, is
/// [`ReplicaError::Unheard`] — the server may have made it, whatever the
/// client's `retryable` says; an answer the server gave is its refusal.
fn command_error(error: RemoteClientError, reached: Option<bool>) -> ReplicaError {
    match reached {
        None if error.code == CLOSED => ReplicaError::Closed,
        None => ReplicaError::Client(error.to_string()),
        Some(false) => ReplicaError::Unheard {
            code: error.code,
            message: error.message,
            retryable: error.retryable,
        },
        Some(true) if error.code == UNREADABLE => ReplicaError::Unheard {
            code: error.code,
            message: error.message,
            retryable: error.retryable,
        },
        Some(true) => ReplicaError::Remote {
            code: error.code,
            message: error.message,
            retryable: error.retryable,
        },
    }
}

impl Replica for SyncularReplica {
    fn mutate(&self, ops: Vec<RowOp>) -> Result<String, ReplicaError> {
        let ops = ops.into_iter().map(mutation).collect();
        self.ask(|reply| Command::Mutate(ops, reply))
    }

    fn query_covered(
        &self,
        sql: &str,
        params: &[Value],
        base: &WindowBase,
        units: &[String],
    ) -> Result<Covered, ReplicaError> {
        let coverage = [WindowCoverage {
            base: window_base(base),
            units: units.to_vec(),
        }];
        let mut reader = self.reader.lock().map_err(|_| ReplicaError::Closed)?;
        let snapshot = reader
            .query_snapshot(sql, params, &coverage)
            .map_err(|failure| ReplicaError::Client(failure.to_string()))?;
        let incomplete = snapshot
            .coverage
            .pending
            .into_iter()
            .chain(snapshot.coverage.missing)
            .map(|unit| unit.unit)
            .collect();
        Ok((snapshot.rows, incomplete))
    }

    fn query(&self, sql: &str, params: &[Value]) -> Result<Vec<Map<String, Value>>, ReplicaError> {
        let mut reader = self.reader.lock().map_err(|_| ReplicaError::Closed)?;
        reader
            .query_snapshot(sql, params, &[])
            .map(|snapshot| snapshot.rows)
            .map_err(|failure| ReplicaError::Client(failure.to_string()))
    }

    fn subscribe(&self, subscription: Subscription) -> Result<(), ReplicaError> {
        self.ask(|reply| Command::Subscribe(subscription, reply))
    }

    fn set_window(&self, base: &WindowBase, units: &[String]) -> Result<(), ReplicaError> {
        let (base, units) = (base.clone(), units.to_vec());
        self.ask(|reply| Command::SetWindow(base, units, reply))
    }

    fn window_state(&self, base: &WindowBase) -> Result<WindowState, ReplicaError> {
        let base = base.clone();
        self.ask(|reply| Command::WindowState(base, reply))
    }

    fn set_presence(&self, scope_key: &str, doc: Option<Value>) -> Result<(), ReplicaError> {
        self.ask(|reply| Command::SetPresence {
            scope_key: scope_key.to_owned(),
            doc,
            reply,
        })
    }

    fn presence(&self, scope_key: &str) -> Result<Vec<Peer>, ReplicaError> {
        self.ask(|reply| Command::Presence(scope_key.to_owned(), reply))
    }

    fn upload_blob(
        &self,
        bytes: Vec<u8>,
        media_type: Option<String>,
    ) -> Result<Value, ReplicaError> {
        self.ask(|reply| Command::Upload(bytes, media_type, reply))
    }

    fn fetch_blob(&self, blob: &str) -> Result<Vec<u8>, ReplicaError> {
        self.ask(|reply| Command::Fetch(blob.to_owned(), reply))
    }

    fn delivery(&self, commit_id: &str) -> Result<Delivery, ReplicaError> {
        let mut reader = self.reader.lock().map_err(|_| ReplicaError::Closed)?;
        let delivery = reader
            .commit_delivery(commit_id)
            .map_err(|failure| ReplicaError::Client(failure.to_string()))?;
        Ok(match delivery {
            CommitDelivery::Pending { .. } => Delivery::Pending,
            CommitDelivery::Unknown { .. } => Delivery::Unknown,
            CommitDelivery::Known { outcome, .. } => match outcome.status {
                CommitOutcomeStatus::Applied | CommitOutcomeStatus::Cached => Delivery::Applied,
                CommitOutcomeStatus::Conflict => Delivery::Refused(Rejection::new(
                    CONFLICT,
                    "Someone changed this first; keeper shows what the server holds now.",
                )),
                CommitOutcomeStatus::Rejected => Delivery::Refused(
                    outcome
                        .results
                        .iter()
                        .find_map(|result| match result {
                            CommitOperationOutcome::Error { rejection } => Some(Rejection::new(
                                rejection.code.clone(),
                                rejection.message.clone(),
                            )),
                            _ => None,
                        })
                        .unwrap_or_else(|| {
                            Rejection::new("sync.rejected", "The server refused this.")
                        }),
                ),
            },
        })
    }

    fn row_writes(&self, table: &str, row_id: &str) -> Result<RowWrites, ReplicaError> {
        let params = [Value::from(table), Value::from(row_id)];
        let pending = !self.query(ROW_PENDING, &params)?.is_empty();
        let mut refused = Vec::new();
        for row in self.query(ROW_REFUSED, &params)? {
            let Some(commit_id) = row.get("commit_id").and_then(Value::as_str) else {
                continue;
            };
            if let Delivery::Refused(rejection) = self.delivery(commit_id)? {
                refused.push(Refusal {
                    commit_id: commit_id.to_owned(),
                    rejection,
                });
            }
        }
        Ok(RowWrites { pending, refused })
    }

    fn refused_commits(&self, table: &str) -> Result<Vec<RefusedCommit>, ReplicaError> {
        let mut refused = Vec::new();
        for row in self.query(TABLE_REFUSED, &[Value::from(table)])? {
            let Some(commit_id) = row.get("commit_id").and_then(Value::as_str) else {
                continue;
            };
            let Delivery::Refused(rejection) = self.delivery(commit_id)? else {
                continue;
            };
            let operations = match row.get("operations") {
                Some(Value::String(raw)) => serde_json::from_str(raw).unwrap_or(Value::Null),
                Some(other) => other.clone(),
                None => Value::Null,
            };
            let rows = operations
                .as_array()
                .map(|operations| {
                    operations
                        .iter()
                        .filter_map(|operation| {
                            let table = operation.get("table")?.as_str()?.to_owned();
                            let values = operation.get("values")?.as_object()?.clone();
                            Some((table, values))
                        })
                        .collect()
                })
                .unwrap_or_default();
            refused.push(RefusedCommit {
                commit_id: commit_id.to_owned(),
                rejection,
                rows,
            });
        }
        Ok(refused)
    }

    fn upload_refusal(&self) -> Option<Rejection> {
        self.upload_refusal
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn blob_cache_bytes(&self) -> Result<u64, ReplicaError> {
        self.ask(Command::CacheBytes)
    }

    fn supersede(&self, commit_ids: Vec<String>, replacement: &str) -> Result<(), ReplicaError> {
        if commit_ids.is_empty() {
            return Ok(());
        }
        let replacement = replacement.to_owned();
        self.ask(|reply| Command::Supersede(commit_ids, replacement, reply))
    }

    fn remote_query(
        &self,
        operation_id: &str,
        params: Value,
    ) -> Result<Vec<Map<String, Value>>, ReplicaError> {
        let Some(remote) = &self.remote else {
            return Err(ReplicaError::NotSyncing);
        };
        if !self.gate.load(Ordering::SeqCst) {
            return Err(ReplicaError::NotSyncing);
        }
        let mut transport = GatedTransport {
            inner: HostTransport::from_config(&remote.config).map_err(ReplicaError::Client)?,
            open: Arc::clone(&self.gate),
            origin: Some(remote.origin.clone()),
            reached: None,
        };
        let client = SyncRemoteClient::for_operations(remote.client_id.clone())
            .map_err(|error| ReplicaError::Client(error.to_string()))?;
        let answer = client
            .query::<_, Value, Map<String, Value>>(&mut transport, operation_id, &params)
            .map_err(|error| ReplicaError::Client(error.to_string()));
        transport.inner.shutdown();
        Ok(answer?.rows)
    }

    fn remote_command(
        &self,
        operation_id: &str,
        request_id: &str,
        params: Value,
    ) -> Result<(), ReplicaError> {
        let Some(remote) = &self.remote else {
            return Err(ReplicaError::NotSyncing);
        };
        if !self.gate.load(Ordering::SeqCst) {
            return Err(ReplicaError::NotSyncing);
        }
        let mut transport = GatedTransport {
            inner: HostTransport::from_config(&remote.config).map_err(ReplicaError::Client)?,
            open: Arc::clone(&self.gate),
            origin: Some(remote.origin.clone()),
            reached: None,
        };
        let client = SyncRemoteClient::for_operations(remote.client_id.clone())
            .map_err(|error| ReplicaError::Client(error.to_string()))?;
        let answer = client.command(&mut transport, operation_id, request_id, &params);
        transport.inner.shutdown();
        let answer = answer.map_err(|error| command_error(error, transport.reached))?;
        // `cached`: this request id was applied before and its first answer
        // is replayed (a retry whose earlier answer was lost) — a success.
        if answer.status == "rejected" {
            let refusal = answer.results.iter().find_map(|result| {
                let code = result.get("code").and_then(Value::as_str)?;
                let message = result
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or(code);
                Some((code.to_owned(), message.to_owned()))
            });
            let (code, message) = refusal.unwrap_or_else(|| {
                (
                    "operation.rejected".to_owned(),
                    Value::from(answer.results).to_string(),
                )
            });
            return Err(ReplicaError::Remote {
                code,
                message,
                retryable: false,
            });
        }
        Ok(())
    }

    fn sync_now(&self) {
        let _ = self.commands.send(Command::SyncNow);
    }

    fn state(&self) -> ReplicaState {
        *self.state.borrow()
    }

    fn watch(&self) -> watch::Receiver<ReplicaState> {
        self.state.clone()
    }

    fn close(&self) {
        self.gate.store(false, Ordering::SeqCst);
        let _ = self.commands.send(Command::Stop);
    }

    fn join(&self) {
        let thread = self.thread.lock().ok().and_then(|mut slot| slot.take());
        if let Some(thread) = thread {
            if thread.join().is_err() {
                tracing::warn!("an agents replica's owner thread panicked");
            }
        }
    }
}

impl Drop for SyncularReplica {
    fn drop(&mut self) {
        self.close();
    }
}

/// Opens [`SyncularReplica`]s.
pub struct SyncularOpener;

impl Opener for SyncularOpener {
    fn open(&self, spec: ReplicaSpec) -> Result<Arc<dyn Replica>, ReplicaError> {
        Ok(Arc::new(SyncularReplica::open(spec)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::sync::atomic::AtomicUsize;
    use syncular_client::SyncReport;

    fn failed(code: &str, status: Option<u64>) -> SyncOutcome {
        SyncOutcome::Failed {
            error_code: code.to_owned(),
            message: "m".to_owned(),
            details: status.map(|status| json!({ "causeKind": "status", "httpStatus": status })),
        }
    }

    #[test]
    fn a_refused_credential_is_told_apart_from_an_unreachable_server() {
        assert_eq!(
            verdict(&SyncOutcome::Ok(SyncReport::default())),
            RoundVerdict::Answered
        );
        assert_eq!(
            verdict(&failed("transport.failed", Some(401))),
            RoundVerdict::Refused
        );
        assert_eq!(
            verdict(&failed("transport.failed", Some(403))),
            RoundVerdict::Refused
        );
        assert_eq!(
            verdict(&failed("sync.auth_required", None)),
            RoundVerdict::Refused
        );
        assert!(matches!(
            verdict(&failed("transport.failed", Some(502))),
            RoundVerdict::Unreachable(_)
        ));
        assert!(matches!(
            verdict(&failed(CLOSED, None)),
            RoundVerdict::Unreachable(_)
        ));
    }

    #[test]
    fn a_socket_that_will_not_open_backs_off_while_http_rounds_answer() {
        let mut schedule = Schedule::default();
        let mut now = Instant::now();
        let mut waits = Vec::new();
        for _ in 0..7 {
            assert!(schedule.round_due(now, true));
            schedule.round_done(&RoundVerdict::Answered, now);
            assert!(
                schedule.socket_due(now),
                "the socket is tried once its wait is over"
            );
            schedule.socket_done(false, now);
            waits.push(schedule.socket.wait(now).as_secs());
            assert!(!schedule.socket_due(now));
            assert_eq!(schedule.link(false), LinkState::Connecting);
            // The owner sleeps no longer than a heartbeat while rounds answer.
            assert_eq!(
                schedule.next_wake(now, false, false),
                Some(HEARTBEAT.min(schedule.socket.wait(now)))
            );
            now += schedule.socket.wait(now);
        }
        assert_eq!(waits, [1, 2, 4, 8, 16, 32, 60]);
        schedule.socket_done(true, now);
        schedule.round_done(&RoundVerdict::Answered, now);
        assert_eq!(schedule.link(true), LinkState::Connected);
        assert_eq!(schedule.socket.failures, 0);
    }

    #[test]
    fn an_unreachable_server_backs_off_and_a_refusal_ends_the_schedule() {
        let mut schedule = Schedule::default();
        let now = Instant::now();
        schedule.round_done(&RoundVerdict::Unreachable("x".to_owned()), now);
        schedule.round_done(&RoundVerdict::Unreachable("x".to_owned()), now);
        assert_eq!(schedule.link(false), LinkState::Offline);
        assert!(
            !schedule.round_due(now, true),
            "a wanted round waits for the backoff"
        );
        assert!(!schedule.socket_due(now), "no socket while HTTP fails");
        assert_eq!(
            schedule.next_wake(now, true, false),
            Some(Duration::from_secs(2))
        );
        schedule.round_done(&RoundVerdict::Refused, now + Duration::from_secs(2));
        assert_eq!(schedule.link(false), LinkState::Refused);
        assert!(!schedule.round_due(now + MAX_BACKOFF, true));
        assert_eq!(schedule.next_wake(now, true, false), None);
    }

    fn gated(open: bool) -> GatedTransport {
        GatedTransport {
            inner: HostTransport::from_config(&json!({})).expect("null transport"),
            open: Arc::new(AtomicBool::new(open)),
            origin: Some(
                url::Url::parse("https://hq.example.org:8443/alpha")
                    .expect("url")
                    .origin(),
            ),
            reached: None,
        }
    }

    #[test]
    fn the_gate_refuses_every_request_and_a_url_off_the_servers_origin() {
        let code =
            |result: Result<Vec<u8>, TransportError>| result.map(|_| ()).err().map(|e| e.code);
        let mut open = gated(true);
        // Through the gate: the null transport answers for itself.
        assert_eq!(
            code(open.sync(b"x")).as_deref(),
            Some("transport.unavailable")
        );
        assert_eq!(
            code(open.fetch_blob_url("https://hq.example.org:8443/alpha/blobs/x")).as_deref(),
            Some("transport.unavailable")
        );
        for foreign in [
            "https://cdn.example.net/x",
            "https://hq.example.org/alpha/blobs/x",
            "http://hq.example.org:8443/x",
            "not a url",
        ] {
            assert_eq!(
                code(open.fetch_blob_url(foreign)).as_deref(),
                Some(FOREIGN),
                "{foreign}"
            );
            assert_eq!(
                code(open.fetch_url(foreign, &mut |_| {})).as_deref(),
                Some(FOREIGN)
            );
            assert_eq!(
                open.blob_put_url(foreign, b"x", None)
                    .err()
                    .map(|e| e.code)
                    .as_deref(),
                Some(FOREIGN)
            );
        }
        let mut closed = gated(false);
        assert_eq!(code(closed.sync(b"x")).as_deref(), Some(CLOSED));
        assert_eq!(code(closed.realtime_sync(b"x")).as_deref(), Some(CLOSED));
        assert_eq!(
            code(closed.fetch_blob_url("https://hq.example.org:8443/x")).as_deref(),
            Some(CLOSED)
        );
        assert_eq!(
            closed
                .blob_download("sha256:00")
                .err()
                .map(|e| e.code)
                .as_deref(),
            Some(CLOSED)
        );
        assert_eq!(
            closed
                .realtime_connect_for_client("c")
                .err()
                .map(|e| e.code)
                .as_deref(),
            Some(CLOSED)
        );
    }

    fn message(id: &str) -> RowOp {
        let Value::Object(values) = json!({
            "id": id,
            "session_id": "local.1",
            "page_key": "local.1/2026-10",
            "role": "person",
            "body": "hello",
            "created_at_ms": 1_792_000_000_000_i64,
            "author_actor": "",
            "author_client": "",
        }) else {
            unreachable!("a literal object");
        };
        RowOp::Upsert {
            table: "messages".to_owned(),
            values,
            base_version: None,
        }
    }

    fn wait_until(replica: &SyncularReplica, what: &str, ok: impl Fn(&ReplicaState) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ok(&replica.state()) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(ok(&replica.state()), "{what}: {:?}", replica.state());
    }

    fn local(path: std::path::PathBuf) -> ReplicaSpec {
        ReplicaSpec {
            database: "local".to_owned(),
            path,
            client_id: None,
            link: None,
            subscriptions: Vec::new(),
            watch_presence: None,
        }
    }

    #[test]
    fn a_write_with_no_server_waits_in_the_replicas_outbox_across_a_restart() {
        let dir = tempfile::tempdir().expect("tempdir");
        let spec = local(dir.path().join("local.sqlite"));
        let replica = SyncularReplica::open(spec.clone()).expect("opens");
        assert_eq!(replica.state().link, LinkState::Local);
        replica.mutate(vec![message("m-1")]).expect("commits");
        replica.mutate(vec![message("m-2")]).expect("commits");
        wait_until(&replica, "two commits wait", |s| s.outbox == 2);
        let rows = replica
            .query("SELECT id, body FROM messages ORDER BY id", &[])
            .expect("reads");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["body"], "hello");
        replica.close();
        replica.join();
        drop(replica);

        let again = SyncularReplica::open(spec).expect("reopens");
        wait_until(&again, "the outbox survived", |s| s.outbox == 2);
        assert_eq!(again.state().link, LinkState::Local);
        assert!(matches!(
            again.set_presence("page:x", None),
            Err(ReplicaError::NotSyncing)
        ));
    }

    #[test]
    fn the_directorys_presence_is_known_only_once_an_ask_finds_the_key_registered() {
        let mut gate = PresenceGate::watching(Some("dir:alpha".to_owned()));
        // The socket is connected and nothing is asked yet: not known.
        assert!(!gate.ready());
        // Round 1 ran; the key is not registered yet: the server refuses the
        // leave on it before it answers the never-held key.
        let (key, probe) = gate.due(1, "ana.dev").expect("an ask");
        assert_eq!(key, "dir:alpha");
        gate.refused("dir:alpha", 1);
        gate.refused(&probe, 1);
        assert!(
            !gate.ready(),
            "refused: the peers on the key may not be here"
        );
        assert_eq!(
            gate.due(1, "ana.dev"),
            None,
            "asked again only after a round"
        );
        // After round 2 the key is registered: the answer comes alone.
        let (_, probe) = gate.due(2, "ana.dev").expect("a second ask");
        gate.refused("page:other", 2);
        assert!(!gate.ready(), "another key's refusal says nothing");
        gate.refused(&probe, 2);
        assert!(gate.ready());
        assert_eq!(gate.due(3, "ana.dev"), None, "known: nothing more to ask");
        // A new socket knows nothing yet, and asks at once.
        gate.reset();
        assert!(!gate.ready());
        assert!(gate.due(0, "ana.dev").is_some());
        // An ask that could not be sent is due again at once.
        gate.unasked();
        assert!(gate.due(0, "ana.dev").is_some());
        // A replica that watches no key never asks, and never knows.
        let mut none = PresenceGate::watching(None);
        assert_eq!(none.due(5, "ana.dev"), None);
        assert!(!none.ready());
    }

    #[test]
    fn a_presence_refusal_frame_names_its_key() {
        assert_eq!(
            refused_presence(
                r#"{"event":"presence","data":{"scopeKey":"probe:a:1","error":"presence.forbidden","timestamp":1}}"#
            )
            .as_deref(),
            Some("probe:a:1")
        );
        assert_eq!(
            refused_presence(
                r#"{"event":"presence","data":{"scopeKey":"dir:alpha","kind":"join","actorId":"nixie","clientId":"n.e","doc":{"error":1}}}"#
            ),
            None,
            "a join whose document says error is no refusal"
        );
        assert_eq!(refused_presence(r#"{"event":"hello","data":{}}"#), None);
    }

    /// The server's end of the realtime sockets a replica opens, in order.
    /// The first answers a round only with a join of `nixie.electra` on
    /// `dir:alpha` and then dies; every later one has no peer on the key and
    /// answers the presence asks as keeper-hq does: a leave on the registered
    /// key silently, one on a never-held key with a refusal.
    fn realtime_server() -> u16 {
        use tungstenite::Message;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds");
        let port = listener.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            let mut sockets = 0;
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                let Ok(mut socket) = tungstenite::accept(stream) else {
                    continue;
                };
                sockets += 1;
                while let Ok(message) = socket.read() {
                    match message {
                        Message::Binary(_) if sockets == 1 => {
                            let join = r#"{"event":"presence","data":{"scopeKey":"dir:alpha","kind":"join","actorId":"nixie","clientId":"nixie.electra","doc":{"name":"electra"}}}"#;
                            let _ = socket.send(Message::Text(join.into()));
                            let _ = socket.close(None);
                            while socket.read().is_ok() {}
                            break;
                        }
                        Message::Text(text) if sockets > 1 => {
                            let ask: Value = serde_json::from_str(&text).expect("a presence ask");
                            let key = ask["data"]["scopeKey"].as_str().expect("a key");
                            if key.starts_with("probe:") {
                                let refusal = json!({ "event": "presence", "data": {
                                    "scopeKey": key, "error": "presence.forbidden", "timestamp": 1,
                                }});
                                let _ = socket.send(Message::Text(refusal.to_string().into()));
                            }
                        }
                        _ => {}
                    }
                }
            }
        });
        port
    }

    #[test]
    fn a_join_the_last_socket_left_queued_never_reaches_the_next_sockets_peers() {
        let port = realtime_server();
        let dir = tempfile::tempdir().expect("tempdir");
        let (wake, woken) = mpsc::channel();
        let mut owner = Owner::open(
            &linked(dir.path(), port),
            Arc::new(AtomicBool::new(true)),
            wake,
        )
        .expect("opens");
        owner.new_socket().expect("the first socket");
        // A round on it: the server's answer is a join, and then the socket
        // dies. The join was queued after the drive's last drain.
        owner.round();
        assert_ne!(owner.client.realtime_state(), RealtimeState::Connected);
        assert!(
            woken.try_iter().count() > 0,
            "the dying socket queued a frame"
        );
        // nixie.electra stops, and the next socket's snapshot has no peer.
        owner.new_socket().expect("the next socket");
        owner.ask_presence();
        woken
            .recv_timeout(Duration::from_secs(10))
            .expect("the ask is answered");
        owner.take_frames();
        assert!(owner.gate.ready(), "the new socket's ask completed");
        assert_eq!(
            owner.client.presence("dir:alpha").len(),
            0,
            "a peer of the last socket is shown on this one"
        );
        owner.stop();
    }

    fn prefs_row(id: &str) -> RowOp {
        let Value::Object(values) = json!({
            "id": id, "person_actor": "ana", "agent_actor": "nixie",
            "default_client": "nixie.electra", "fallback": "[]", "updated_at_ms": 1,
            "author_actor": "ana", "author_client": "ana.dev",
        }) else {
            unreachable!("a literal object");
        };
        RowOp::Upsert {
            table: "instance_prefs".to_owned(),
            values,
            base_version: None,
        }
    }

    /// Record, as a round would, the server refusing commit `commit` that
    /// wrote `instance_prefs` row `row_id`.
    fn journal_refusal(path: &std::path::Path, commit: &str, row_id: &str, code: &str) {
        use syncular_client::api::{CommitOperation, RejectionRecord};
        let operations = vec![CommitOperation {
            table: "instance_prefs".to_owned(),
            row_id: row_id.to_owned(),
            op: "upsert".to_owned(),
            base_version: None,
            values: json!({ "id": row_id, "default_client": "nixie.electra" })
                .as_object()
                .cloned(),
        }];
        let results = vec![CommitOperationOutcome::Error {
            rejection: RejectionRecord {
                client_commit_id: commit.to_owned(),
                op_index: 0,
                code: code.to_owned(),
                message: "not yours".to_owned(),
                retryable: false,
                details: None,
                operation: None,
            },
        }];
        let conn = rusqlite::Connection::open(path).expect("opens");
        conn.busy_timeout(Duration::from_secs(5)).expect("timeout");
        conn.execute(
            "INSERT INTO _syncular_commit_outcomes(client_commit_id, status, recorded_at_ms, \
             results_json, operations_json) VALUES (?1, 'rejected', 1, ?2, ?3)",
            rusqlite::params![
                commit,
                serde_json::to_string(&results).expect("results"),
                serde_json::to_string(&operations).expect("operations")
            ],
        )
        .expect("records");
    }

    #[test]
    fn what_a_row_still_owes_and_was_refused_is_read_from_the_file_across_a_restart() {
        let dir = tempfile::tempdir().expect("tempdir");
        let spec = local(dir.path().join("local.sqlite"));
        let replica = SyncularReplica::open(spec.clone()).expect("opens");
        assert_eq!(
            replica
                .row_writes("instance_prefs", "ana/nixie")
                .expect("reads"),
            RowWrites::default(),
            "never written"
        );
        let written = replica
            .mutate(vec![prefs_row("ana/nixie")])
            .expect("commits");
        let owed = replica
            .row_writes("instance_prefs", "ana/nixie")
            .expect("reads");
        assert!(owed.pending && owed.refused.is_empty());
        assert_eq!(
            replica
                .row_writes("instance_prefs", "kuba/nixie")
                .expect("reads"),
            RowWrites::default(),
            "another person's row"
        );
        replica.close();
        replica.join();
        drop(replica);

        // A new process: nothing of the last one but the file.
        let again = SyncularReplica::open(spec.clone()).expect("reopens");
        assert!(
            again
                .row_writes("instance_prefs", "ana/nixie")
                .expect("reads")
                .pending,
            "the outbox survived"
        );
        journal_refusal(&spec.path, "c-old", "ana/nixie", "hq.v9.person_actor");
        journal_refusal(&spec.path, "c-other", "kuba/nixie", "hq.v9.person_actor");
        let writes = again
            .row_writes("instance_prefs", "ana/nixie")
            .expect("reads");
        assert_eq!(writes.refused.len(), 1);
        assert_eq!(writes.refused[0].commit_id, "c-old");
        assert_eq!(writes.refused[0].rejection.code, "hq.v9.person_actor");
        // Written again: the refusal is set aside, durably.
        again
            .supersede(vec!["c-old".to_owned()], &written)
            .expect("supersedes");
        assert!(again
            .row_writes("instance_prefs", "ana/nixie")
            .expect("reads")
            .refused
            .is_empty());
        again.close();
        again.join();
        drop(again);
        let third = SyncularReplica::open(spec).expect("reopens");
        let writes = third
            .row_writes("instance_prefs", "ana/nixie")
            .expect("reads");
        assert!(writes.refused.is_empty(), "set aside across a restart");
        assert_eq!(
            third
                .row_writes("instance_prefs", "kuba/nixie")
                .expect("reads")
                .refused
                .len(),
            1,
            "another row's refusal stands"
        );
    }

    #[test]
    fn a_window_and_its_units_persist_with_the_replica() {
        let dir = tempfile::tempdir().expect("tempdir");
        let spec = local(dir.path().join("w.sqlite"));
        let base = WindowBase {
            table: "messages".to_owned(),
            variable: "page".to_owned(),
            fixed_scopes: Vec::new(),
        };
        let units = vec!["s.1/2026-09".to_owned(), "s.1/2026-10".to_owned()];
        let replica = SyncularReplica::open(spec.clone()).expect("opens");
        replica.set_window(&base, &units).expect("sets");
        assert_eq!(replica.window_state(&base).expect("reads").units, units);
        replica.set_window(&base, &units[1..]).expect("narrows");
        replica.close();
        replica.join();
        drop(replica);
        let again = SyncularReplica::open(spec).expect("reopens");
        let state = again.window_state(&base).expect("reads");
        assert_eq!(state.units, units[1..]);
        assert_eq!(state.pending, units[1..], "never pulled, so not complete");
        let wrong = WindowBase {
            variable: "trace".to_owned(),
            ..base
        };
        assert!(
            again.set_window(&wrong, &units).is_err(),
            "messages has no trace scope"
        );
    }

    /// A server on a local port that answers every request with `status`,
    /// counting the connections it accepts.
    fn server(status: &'static str) -> (u16, Arc<AtomicUsize>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds");
        let port = listener.local_addr().expect("addr").port();
        let count = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&count);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                seen.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0u8; 8192];
                let _ = stream.read(&mut buf);
                let body = r#"{"error":"sync.auth_required"}"#;
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        (port, count)
    }

    fn linked(dir: &std::path::Path, port: u16) -> ReplicaSpec {
        ReplicaSpec {
            database: "alpha".to_owned(),
            path: dir.join("alpha.sqlite"),
            client_id: Some("ana.1".to_owned()),
            link: Some(crate::hq::port::Link {
                base_url: format!("http://127.0.0.1:{port}/alpha"),
                token: "0123456789abcdef".to_owned(),
            }),
            subscriptions: vec![Subscription {
                id: "directoryClients".to_owned(),
                table: "clients".to_owned(),
                scopes: vec![("dir".to_owned(), vec!["alpha".to_owned()])],
            }],
            watch_presence: Some("dir:alpha".to_owned()),
        }
    }

    #[test]
    fn a_refused_replica_sends_nothing_more_and_still_answers_reads() {
        let (port, connections) = server("401 Unauthorized");
        let dir = tempfile::tempdir().expect("tempdir");
        let replica = SyncularReplica::open(linked(dir.path(), port)).expect("opens");
        wait_until(&replica, "refused", |s| s.link == LinkState::Refused);
        let after_refusal = connections.load(Ordering::SeqCst);
        replica
            .mutate(vec![message("m-1")])
            .expect("commits locally");
        replica.sync_now();
        assert!(replica
            .fetch_blob("sha256:0000000000000000000000000000000000000000000000000000000000000000")
            .is_err());
        assert!(matches!(
            replica.set_presence("page:x", None),
            Err(ReplicaError::NotSyncing)
        ));
        std::thread::sleep(HEARTBEAT + Duration::from_secs(1));
        assert_eq!(
            connections.load(Ordering::SeqCst),
            after_refusal,
            "a refused replica connected"
        );
        assert_eq!(
            replica
                .query("SELECT id FROM messages", &[])
                .expect("reads")
                .len(),
            1
        );
        assert_eq!(replica.state().outbox, 1);
    }

    #[test]
    fn a_closed_replica_joins_and_connects_no_more() {
        let (port, connections) = server("503 Service Unavailable");
        let dir = tempfile::tempdir().expect("tempdir");
        let replica = SyncularReplica::open(linked(dir.path(), port)).expect("opens");
        wait_until(&replica, "offline", |s| s.link == LinkState::Offline);
        replica.close();
        let at_close = connections.load(Ordering::SeqCst);
        replica.sync_now();
        replica.join();
        assert_eq!(replica.state().link, LinkState::Closed);
        std::thread::sleep(Duration::from_secs(3));
        assert_eq!(
            connections.load(Ordering::SeqCst),
            at_close,
            "a closed replica connected"
        );
        // Reads still answer from the file.
        assert!(replica.query("SELECT id FROM messages", &[]).is_ok());
    }

    #[test]
    fn a_server_that_cannot_be_reached_leaves_reads_and_the_outbox_alone() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds");
        let port = listener.local_addr().expect("addr").port();
        drop(listener);
        let dir = tempfile::tempdir().expect("tempdir");
        let replica = SyncularReplica::open(linked(dir.path(), port)).expect("opens");
        wait_until(&replica, "offline", |s| s.link == LinkState::Offline);
        replica
            .mutate(vec![message("m-1")])
            .expect("commits while offline");
        wait_until(&replica, "one waits", |s| s.outbox == 1);
        let started = Instant::now();
        assert_eq!(
            replica
                .query("SELECT id FROM messages", &[])
                .expect("reads")
                .len(),
            1
        );
        assert!(started.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn a_refused_upload_is_read_from_its_status_and_nothing_else_is_one() {
        let failed = |message: &str, status: Option<u64>| SyncOutcome::Failed {
            error_code: "transport.failed".to_owned(),
            message: message.to_owned(),
            details: status.map(|status| json!({ "causeKind": "status", "httpStatus": status })),
        };
        let code = |outcome: SyncOutcome| upload_refusal(&outcome).map(|r| r.code);
        assert_eq!(
            code(failed("blob upload grant request failed", Some(413))).as_deref(),
            Some("blob.too_large")
        );
        assert_eq!(
            code(failed("blob upload failed", Some(413))).as_deref(),
            Some("blob.too_large")
        );
        assert_eq!(
            code(failed("blob upload failed", Some(400))).as_deref(),
            Some("blob.hash_mismatch")
        );
        // A failed upload that is the network, and a sync request's 413.
        assert_eq!(code(failed("blob upload failed", Some(502))), None);
        assert_eq!(code(failed("blob upload failed", None)), None);
        assert_eq!(code(failed("sync request failed", Some(413))), None);
        assert_eq!(code(SyncOutcome::Ok(SyncReport::default())), None);
    }

    fn sha(bytes: &[u8]) -> String {
        use sha2::Digest;
        format!("sha256:{}", hex::encode(sha2::Sha256::digest(bytes)))
    }

    /// Put `bytes` in the client's cache as a download would, cached at `at`.
    fn downloaded(path: &std::path::Path, bytes: &[u8], at: i64) -> String {
        let id = sha(bytes);
        let conn = rusqlite::Connection::open(path).expect("opens");
        conn.busy_timeout(Duration::from_secs(5)).expect("timeout");
        conn.execute(
            "INSERT INTO _syncular_blobs(blob_id, bytes, byte_length, media_type, created_at_ms) \
             VALUES (?1, ?2, ?3, NULL, ?4)",
            rusqlite::params![id, bytes, bytes.len() as i64, at],
        )
        .expect("caches");
        id
    }

    fn cached(path: &std::path::Path) -> Vec<String> {
        let conn = rusqlite::Connection::open(path).expect("opens");
        let mut ids: Vec<String> = conn
            .prepare("SELECT blob_id FROM _syncular_blobs")
            .expect("prepares")
            .query_map([], |row| row.get(0))
            .expect("reads")
            .collect::<Result<_, _>>()
            .expect("reads");
        ids.sort();
        ids
    }

    fn ask<T>(owner: &mut Owner, command: impl FnOnce(Reply<T>) -> Command) -> T {
        let (reply, answer) = mpsc::sync_channel(1);
        assert!(owner.handle(command(reply)));
        answer.recv().expect("answered").expect("succeeds")
    }

    #[test]
    fn past_the_cap_the_least_recently_opened_bodies_go_and_never_one_the_outbox_pins() {
        let dir = tempfile::tempdir().expect("tempdir");
        let spec = local(dir.path().join("alpha.sqlite"));
        let (wake, _woken) = mpsc::channel();
        let mut owner = Owner::open(&spec, Arc::new(AtomicBool::new(false)), wake).expect("opens");
        assert_eq!(owner.cache_max, BLOB_CACHE_MAX_BYTES, "the named constant");
        owner.cache_max = 250;
        // Staged here, 100 bytes: pinned until the server takes it.
        let pinned = vec![7u8; 100];
        let reference: Value = ask(&mut owner, |reply| {
            Command::Upload(pinned.clone(), Some("application/pdf".to_owned()), reply)
        });
        assert_eq!(reference["blobId"], json!(sha(&pinned)));
        // Three downloads of 80 bytes, cached in this order, long ago.
        let a = downloaded(&spec.path, &[1u8; 80], 1);
        let b = downloaded(&spec.path, &[2u8; 80], 2);
        let c = downloaded(&spec.path, &[3u8; 80], 3);
        // Opening the oldest makes it the most recently used; past the cap
        // (340 > 250) the least recently used unpinned bodies go — b, then
        // c — and the pinned one stays though it is the largest.
        let bytes: Vec<u8> = ask(&mut owner, |reply| Command::Fetch(a.clone(), reply));
        assert_eq!(bytes, vec![1u8; 80], "served from the cache, no server");
        let mut want = vec![sha(&pinned), a.clone()];
        want.sort();
        assert_eq!(cached(&spec.path), want, "{b} and {c} evicted");
        let reported: u64 = ask(&mut owner, Command::CacheBytes);
        assert_eq!(reported, 180, "the client's blob_cache_bytes_approx");

        // Pinned bodies alone past the cap: every unpinned one goes, never
        // the pinned one, and the order survives a relaunch.
        owner.stop();
        drop(owner);
        let (wake, _woken) = mpsc::channel();
        let mut owner =
            Owner::open(&spec, Arc::new(AtomicBool::new(false)), wake).expect("reopens");
        owner.cache_max = 50;
        let d = downloaded(&spec.path, &[4u8; 10], 9_999_999_999_999);
        owner.enforce_cache();
        assert_eq!(
            cached(&spec.path),
            vec![sha(&pinned)],
            "{a} and {d} evicted"
        );
    }
}
