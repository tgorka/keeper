//! The one adapter of [`crate::hq::port`]: a `syncular_client::SyncClient` on
//! a replica file, driven as a blocking host (SPEC §8.4) on its own owner
//! thread, with realtime over the `native-transport` WebSocket.
//!
//! The owner thread is the only thread that touches the client. Between
//! commands it feeds socket frames to the client, runs the rounds the client
//! asks for, reconnects a socket that is not up, and runs a heartbeat round
//! every [`HEARTBEAT`], because the client cannot see an idle socket die. A
//! round that cannot reach the server backs off (1 s doubling to a minute); a
//! round the server refuses for this device's credential stops the link for
//! good — the replica keeps answering reads and keeps writes in its outbox,
//! and no further request is sent, so a revoked device has no request loop.
//!
//! Reads go to a second, read-only connection on the same file
//! (`FileQuerySnapshotReader`, WAL), so a read never queues behind a round.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};
use syncular_client::native_transport::{HostTransport, Inbound};
use syncular_client::{
    ClientLimits, FileQuerySnapshotReader, Mutation, RealtimeState, SyncClient, SyncIntent,
    SyncOutcome,
};
use tokio::sync::watch;

use crate::hq::port::{
    LinkState, Opener, Peer, Replica, ReplicaError, ReplicaSpec, ReplicaState, RowOp,
};

/// A round at least this often while linked, socket or not.
pub const HEARTBEAT: Duration = Duration::from_secs(5);
/// One HTTP request's deadline; a firewalled server costs a round at most this.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// One round's deadline across every request it makes.
const ROUND_DEADLINE: Duration = Duration::from_secs(60);
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

type Reply<T> = SyncSender<Result<T, ReplicaError>>;

enum Command {
    Mutate(Vec<Mutation>, Reply<String>),
    Subscribe {
        id: String,
        table: String,
        scopes: Vec<(String, Vec<String>)>,
        reply: Reply<()>,
    },
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

struct Owner {
    database: String,
    client: SyncClient,
    transport: HostTransport,
    linked: bool,
    refused: bool,
    /// Rounds failed in a row; zero when the last one was answered.
    failures: u32,
    retry_at: Option<Instant>,
    last_round: Option<Instant>,
    /// A local write or an asked-for round waits to be pushed.
    dirty: bool,
    state: watch::Sender<ReplicaState>,
}

impl Owner {
    fn open(spec: &ReplicaSpec, wake: Sender<Command>) -> Result<Owner, ReplicaError> {
        let path = spec
            .path
            .to_str()
            .ok_or_else(|| ReplicaError::Client("the replica's path is not UTF-8".to_owned()))?;
        let client = SyncClient::open_path_with_identity(
            spec.client_id.clone(),
            schema()?,
            ClientLimits::default(),
            path,
        )
        .map_err(ReplicaError::Client)?;
        let config = match &spec.link {
            Some(link) => json!({
                "baseUrl": link.base_url,
                "headers": { "authorization": format!("Bearer {}", link.token) },
                "requestTimeoutMs": REQUEST_TIMEOUT.as_millis() as u64,
                "roundDeadlineMs": ROUND_DEADLINE.as_millis() as u64,
            }),
            // No `baseUrl`: the transport that refuses every network call.
            None => json!({}),
        };
        let notify: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            let _ = wake.send(Command::Wake);
        });
        let transport = HostTransport::from_config_with_notify(&config, Some(notify))
            .map_err(ReplicaError::Client)?;
        let outbox = client.status_snapshot().outbox;
        let link = if spec.link.is_some() {
            LinkState::Connecting
        } else {
            LinkState::Local
        };
        let (state, _) = watch::channel(ReplicaState { link, outbox });
        Ok(Owner {
            database: spec.database.clone(),
            client,
            transport,
            linked: spec.link.is_some(),
            refused: false,
            failures: 0,
            retry_at: None,
            last_round: None,
            dirty: true,
            state,
        })
    }

    fn online(&self) -> bool {
        self.linked && !self.refused
    }

    fn run(mut self, commands: Receiver<Command>) {
        loop {
            let command = if self.online() {
                match commands.recv_timeout(self.next_due()) {
                    Ok(command) => Some(command),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            } else {
                match commands.recv() {
                    Ok(command) => Some(command),
                    Err(_) => break,
                }
            };
            if let Some(command) = command {
                if !self.handle(command) {
                    break;
                }
                // Everything already queued before the network is touched.
                while let Ok(command) = commands.try_recv() {
                    if !self.handle(command) {
                        self.stop();
                        return;
                    }
                }
            }
            self.drive();
            self.publish();
        }
        self.stop();
    }

    fn next_due(&self) -> Duration {
        let now = Instant::now();
        if let Some(retry_at) = self.retry_at {
            return retry_at.saturating_duration_since(now);
        }
        if self.dirty {
            return Duration::ZERO;
        }
        match self.last_round {
            Some(last) => (last + HEARTBEAT).saturating_duration_since(now),
            None => Duration::ZERO,
        }
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
            Command::Subscribe {
                id,
                table,
                scopes,
                reply,
            } => {
                let outcome = self
                    .client
                    .subscribe(id, table, scopes, None)
                    .map_err(ReplicaError::Client);
                self.dirty |= outcome.is_ok();
                let _ = reply.send(outcome);
            }
            Command::SetPresence {
                scope_key,
                doc,
                reply,
            } => {
                let _ = reply.send(
                    self.client
                        .set_presence(&mut self.transport, &scope_key, doc.as_ref())
                        .map_err(ReplicaError::Client),
                );
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
                let _ = reply.send(
                    self.client
                        .fetch_blob_bytes(&mut self.transport, &blob)
                        .map(|blob| blob.bytes)
                        .map_err(|(code, message)| {
                            ReplicaError::Client(format!("{code}: {message}"))
                        }),
                );
            }
            Command::SyncNow => {
                self.dirty = true;
                // An explicit ask retries a backed-off server at once.
                self.retry_at = None;
            }
            Command::Wake => {}
            Command::Stop => return false,
        }
        true
    }

    /// Frames, the rounds they and the heartbeat ask for, and the socket.
    fn drive(&mut self) {
        if !self.online() {
            return;
        }
        if self.retry_at.is_some_and(|at| Instant::now() < at) {
            return;
        }
        self.retry_at = None;
        for frame in self.transport.take_inbound() {
            match frame {
                Inbound::Text(text) => self.client.on_realtime_text(&text),
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
        // Change batches are read back through queries; the queue is only
        // drained so it does not grow.
        self.client.drain_change_batches();
        let due = self
            .last_round
            .is_none_or(|last| last.elapsed() >= HEARTBEAT);
        if (asked || due || self.dirty || self.client.sync_needed()) && !self.round() {
            return;
        }
        if self.client.realtime_state() != RealtimeState::Connected {
            match self.client.connect_realtime(&mut self.transport) {
                Ok(()) => {
                    self.round();
                }
                Err(error) => {
                    tracing::debug!(database = %self.database, %error, "agents server socket refused");
                    // The round says whether that was the credential or the network.
                    if self.round() {
                        self.fail(format!("socket: {error}"));
                    }
                }
            }
        }
    }

    /// One round until idle; whether the server answered.
    fn round(&mut self) -> bool {
        self.last_round = Some(Instant::now());
        self.dirty = false;
        let outcome = self.client.sync_until_idle(&mut self.transport, None);
        match verdict(&outcome) {
            RoundVerdict::Answered => {
                self.failures = 0;
                true
            }
            RoundVerdict::Refused => {
                tracing::warn!(
                    database = %self.database,
                    "the agents server refused this device's credential; this replica stops syncing"
                );
                self.refused = true;
                self.client.disconnect_realtime(&mut self.transport);
                self.transport.shutdown();
                false
            }
            RoundVerdict::Unreachable(why) => {
                self.fail(why);
                false
            }
        }
    }

    fn fail(&mut self, why: String) {
        self.failures = self.failures.saturating_add(1);
        let wait = backoff(self.failures);
        tracing::info!(
            database = %self.database,
            failures = self.failures,
            retry_in_ms = wait.as_millis() as u64,
            %why,
            "agents server not reached"
        );
        self.retry_at = Some(Instant::now() + wait);
        if self.client.realtime_state() == RealtimeState::Connected {
            self.client.disconnect_realtime(&mut self.transport);
        }
    }

    fn publish(&self) {
        let link = if !self.linked {
            LinkState::Local
        } else if self.refused {
            LinkState::Refused
        } else if self.failures > 0 {
            LinkState::Offline
        } else if self.last_round.is_some()
            && self.client.realtime_state() == RealtimeState::Connected
        {
            LinkState::Connected
        } else {
            LinkState::Connecting
        };
        let next = ReplicaState {
            link,
            outbox: self.client.status_snapshot().outbox,
        };
        self.state.send_if_modified(|state| {
            let changed = *state != next;
            *state = next;
            changed
        });
    }

    fn stop(&mut self) {
        if self.linked && !self.refused {
            self.client.disconnect_realtime(&mut self.transport);
        }
        self.transport.shutdown();
        self.linked = false;
        self.publish();
    }
}

/// A replica whose owner thread runs [`Owner`].
pub struct SyncularReplica {
    commands: Sender<Command>,
    reader: Mutex<FileQuerySnapshotReader>,
    state: watch::Receiver<ReplicaState>,
}

impl SyncularReplica {
    /// Open (creating when absent) the replica at `spec.path` and start its
    /// owner thread. Returns once the file is open, or with why it is not.
    pub fn open(spec: ReplicaSpec) -> Result<SyncularReplica, ReplicaError> {
        let path = spec
            .path
            .to_str()
            .ok_or_else(|| ReplicaError::Client("the replica's path is not UTF-8".to_owned()))?
            .to_owned();
        let (commands, inbox) = mpsc::channel::<Command>();
        let (ready, opened) = mpsc::sync_channel(1);
        let wake = commands.clone();
        std::thread::Builder::new()
            .name(format!("hq-{}", spec.database))
            .spawn(move || match Owner::open(&spec, wake) {
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
            reader: Mutex::new(FileQuerySnapshotReader::new(path)),
            state,
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

impl Replica for SyncularReplica {
    fn mutate(&self, ops: Vec<RowOp>) -> Result<String, ReplicaError> {
        let ops = ops.into_iter().map(mutation).collect();
        self.ask(|reply| Command::Mutate(ops, reply))
    }

    fn query(&self, sql: &str, params: &[Value]) -> Result<Vec<Map<String, Value>>, ReplicaError> {
        let mut reader = self.reader.lock().map_err(|_| ReplicaError::Closed)?;
        reader
            .query_snapshot(sql, params, &[])
            .map(|snapshot| snapshot.rows)
            .map_err(|failure| ReplicaError::Client(failure.to_string()))
    }

    fn subscribe(
        &self,
        id: &str,
        table: &str,
        scopes: Vec<(String, Vec<String>)>,
    ) -> Result<(), ReplicaError> {
        self.ask(|reply| Command::Subscribe {
            id: id.to_owned(),
            table: table.to_owned(),
            scopes,
            reply,
        })
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
        let _ = self.commands.send(Command::Stop);
    }
}

impl Drop for SyncularReplica {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Stop);
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
            verdict(&failed("transport.timeout", None)),
            RoundVerdict::Unreachable(_)
        ));
    }

    #[test]
    fn an_unreachable_server_is_retried_less_and_less_often() {
        assert_eq!(backoff(1), Duration::from_secs(1));
        assert_eq!(backoff(2), Duration::from_secs(2));
        assert_eq!(backoff(6), Duration::from_secs(32));
        assert_eq!(backoff(7), MAX_BACKOFF);
        assert_eq!(backoff(u32::MAX), MAX_BACKOFF);
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

    fn wait_for(replica: &SyncularReplica, outbox: usize) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while replica.state().outbox != outbox && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(replica.state().outbox, outbox);
    }

    #[test]
    fn a_write_with_no_server_waits_in_the_replicas_outbox_across_a_restart() {
        let dir = tempfile::tempdir().expect("tempdir");
        let spec = ReplicaSpec {
            database: "local".to_owned(),
            path: dir.path().join("local.sqlite"),
            client_id: None,
            link: None,
        };
        let replica = SyncularReplica::open(spec.clone()).expect("opens");
        assert_eq!(
            replica.state(),
            ReplicaState {
                link: LinkState::Local,
                outbox: 0
            }
        );
        replica.mutate(vec![message("m-1")]).expect("commits");
        replica.mutate(vec![message("m-2")]).expect("commits");
        wait_for(&replica, 2);
        let rows = replica
            .query("SELECT id, body FROM messages ORDER BY id", &[])
            .expect("reads");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["body"], "hello");
        replica.close();
        drop(replica);

        let again = SyncularReplica::open(spec).expect("reopens");
        wait_for(&again, 2);
        assert_eq!(again.state().link, LinkState::Local);
        assert_eq!(
            again
                .query("SELECT count(*) AS n FROM messages", &[])
                .expect("reads")[0]["n"],
            2
        );
    }

    #[test]
    fn a_server_that_cannot_be_reached_leaves_reads_and_the_outbox_alone() {
        // A port nothing listens on: every round fails at connect.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds");
        let port = listener.local_addr().expect("addr").port();
        drop(listener);
        let dir = tempfile::tempdir().expect("tempdir");
        let replica = SyncularReplica::open(ReplicaSpec {
            database: "db".to_owned(),
            path: dir.path().join("db.sqlite"),
            client_id: Some("t.1".to_owned()),
            link: Some(crate::hq::port::Link {
                base_url: format!("http://127.0.0.1:{port}/db"),
                token: "0123456789abcdef".to_owned(),
            }),
        })
        .expect("opens");
        let deadline = Instant::now() + Duration::from_secs(10);
        while replica.state().link != LinkState::Offline && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(replica.state().link, LinkState::Offline);
        replica
            .mutate(vec![message("m-1")])
            .expect("commits while offline");
        wait_for(&replica, 1);
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
