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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, LazyLock, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};
use syncular_client::api::{
    CommitDelivery, CommitOperationOutcome, CommitOutcomeStatus, WindowCoverage,
};
use syncular_client::native_transport::{HostTransport, Inbound};
use syncular_client::{
    BlobDownload, BlobUploadGrant, ClientLimits, FileQuerySnapshotReader, Mutation, RealtimeState,
    RemoteClientError, SegmentRequest, SyncClient, SyncIntent, SyncOutcome, SyncRemoteClient,
    Transport, TransportError,
};
use tokio::sync::watch;

use crate::hq::port::{
    Answered, Covered, Delivery, LinkState, Opener, Peer, Rejection, Replica, ReplicaError,
    ReplicaSpec, ReplicaState, RowOp, Subscription, WindowBase, WindowState, CONFLICT,
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
    Upload(Vec<u8>, Option<String>, Reply<Value>),
    Fetch(String, Reply<Vec<u8>>),
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
    state: watch::Sender<ReplicaState>,
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
        let mut client = SyncClient::open_path_with_identity(
            spec.client_id.clone(),
            schema()?,
            ClientLimits::default(),
            path,
        )
        .map_err(ReplicaError::Client)?;
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
            state,
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
            Command::Upload(bytes, media_type, reply) => {
                let outcome = self
                    .client
                    .upload_blob(&bytes, media_type, None)
                    .map_err(ReplicaError::Client);
                self.dirty |= outcome.is_ok();
                let _ = reply.send(outcome);
            }
            Command::Fetch(blob, reply) => {
                // With the network closed the client serves its cache and
                // refuses a miss itself; the gate refuses it again.
                let _ = reply.send(
                    self.client
                        .fetch_blob_bytes(&mut self.transport, &blob)
                        .map(|blob| blob.bytes)
                        .map_err(|(code, message)| {
                            ReplicaError::Client(format!("{code}: {message}"))
                        }),
                );
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
        for frame in self.transport.inner.take_inbound() {
            match frame {
                // Hello, presence and wake frames come as text; only a
                // presence fanout changes a peer, and telling them apart
                // here would parse each frame twice.
                Inbound::Text(text) => {
                    self.client.on_realtime_text(&text);
                    self.presence += 1;
                }
                Inbound::Binary(bytes) => {
                    self.client.on_realtime_binary(&mut self.transport, &bytes)
                }
            }
        }
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
        if !self.online() || self.connected() || !self.schedule.socket_due(Instant::now()) {
            return;
        }
        match self.client.connect_realtime(&mut self.transport) {
            Ok(()) => {
                self.schedule.socket_done(true, Instant::now());
                self.round();
            }
            Err(error) => {
                self.schedule.socket_done(false, Instant::now());
                tracing::debug!(database = %self.database, %error, "agents server socket refused");
            }
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

    fn round(&mut self) {
        self.dirty = false;
        let outcome = self.client.sync_until_idle(&mut self.transport, None);
        let verdict = verdict(&outcome);
        self.schedule.round_done(&verdict, Instant::now());
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
                self.client.disconnect_realtime(&mut self.transport);
                self.transport.inner.shutdown();
            }
            RoundVerdict::Unreachable(why) => {
                tracing::info!(
                    database = %self.database,
                    retry_in_ms = self.schedule.http.wait(Instant::now()).as_millis() as u64,
                    %why,
                    "agents server not reached"
                );
                if self.connected() {
                    self.client.disconnect_realtime(&mut self.transport);
                }
            }
        }
        self.absorb_changes();
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
        self.client.disconnect_realtime(&mut self.transport);
        self.transport.inner.shutdown();
        let next = ReplicaState {
            link: LinkState::Closed,
            outbox: self.client.status_snapshot().outbox,
            directory: self.directory,
            rows: self.rows,
            presence: self.presence,
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
                Ok(owner) => {
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
    ) -> Result<Answered, ReplicaError> {
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
        // is replayed (a retry whose earlier answer was lost) — a success,
        // which says nothing about the room now.
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
        Ok(if answer.status == "cached" {
            Answered::Cached
        } else {
            Answered::Applied
        })
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
}
