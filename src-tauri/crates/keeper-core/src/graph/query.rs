//! Bounded `KG-Cypher/1` reads over a domain's complete accepted prefix
//! (contract §5–7; AD-10–AD-13): one [`GraphReads`] per replica and domain,
//! sealed readers per caller.
//!
//! A reader exists only under a server authority: an agent's
//! [`AgentReader`] only from an [`AdmittedRead`] (`graph.admit_read` for
//! that context and domain — server graph scope alone is not session
//! permission), a person's [`HumanReader`] only on a person's device's
//! replica (`graph.read_status`; compiled only with the `human-reads`
//! feature, which the agent runtime never enables). Which generation is
//! read follows from the reader's domain and authority alone, never from a
//! payload path, URI or filter.
//!
//! Every read checks the authority again before it delivers: a refusal, a
//! domain no longer among the context's sources, an authority other than
//! the generation's, an expired context or the domain's kill switch
//! suppresses the result. [`GraphReads::revoke`] is the kill (D12): it
//! blocks delivery, kills and reaps every in-flight worker of the domain,
//! drops its generations and purges its projection; the instance and every
//! reader of it stay refused, and a regrant opens a new one. Conversations
//! are untouched.
//!
//! A result names its domain, its prefix length N, the Syncular
//! checkpoint, the schema/projection/engine versions and its completeness:
//! `Current` when N is still the replica's whole history, else `Stale`,
//! which a caller must accept explicitly (`projection_stale` otherwise).

use std::future::Future;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use serde_json::{Map, Value};
use tokio::sync::{watch, Mutex as AsyncMutex, Notify};

use super::admission::{Admission, Principal};
use super::projection::{Exporter, Generation, Store};
use super::protocol::profile::{admit, MAX_ROWS};
use super::protocol::wire::{
    self, admit_read_params, decode_context, decode_edge, decode_node, decode_read_status,
    read_status_params, refresh_context_params, Authority, ContextRow, NodeView, ReadStatus,
};
use super::protocol::worker::{InspectEntity, InspectRequest, QueryRequest, Response};
use super::protocol::{Refusal, PROFILE};
use super::runtime::Runtime;
use super::source::{GraphSource, ScopeState};
use super::supervisor::{self, Cancellation, Stops, BUILD_DEADLINE, QUERY_DEADLINE};

/// Why a read did not deliver: the contract's §7 codes.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum QueryError {
    #[error("query_unsupported")]
    Unsupported,
    #[error("query_limit")]
    Limit,
    #[error("query_cancelled")]
    Cancelled,
    /// No generation may be served, with the fixed reason.
    #[error("projection_unavailable: {0}")]
    Unavailable(&'static str),
    /// The generation is behind the replica and the caller asked for the
    /// current graph.
    #[error("projection_stale")]
    Stale,
    /// The replica has not pulled the domain's history yet, or the server
    /// could not be asked (a local refusal, a closed or non-syncing replica,
    /// an unreadable answer, a failed join).
    #[error("sync_pending")]
    SyncPending,
    /// The server could not be reached: the request left this device and
    /// its transport failed ([`crate::hq::port::ReplicaError::Unheard`] of a
    /// remote query). Made only by a native remote's authority read, never
    /// from any other failure; its stable code is `sync_pending`, so callers
    /// that do not need the cause read it as one.
    #[error("sync_pending")]
    Unreachable,
    #[error("storage_unavailable: {0}")]
    Storage(&'static str),
    /// Collapsed: the caller may not read this domain now.
    #[error("forbidden")]
    Forbidden,
}

impl QueryError {
    /// The stable code.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unsupported => "query_unsupported",
            Self::Limit => "query_limit",
            Self::Cancelled => "query_cancelled",
            Self::Unavailable(_) => "projection_unavailable",
            Self::Stale => "projection_stale",
            Self::SyncPending | Self::Unreachable => "sync_pending",
            Self::Storage(_) => "storage_unavailable",
            Self::Forbidden => "forbidden",
        }
    }
}

impl From<Refusal> for QueryError {
    fn from(refusal: Refusal) -> Self {
        match refusal {
            Refusal::QueryLimit | Refusal::ChangeLimit => Self::Limit,
            _ => Self::Unsupported,
        }
    }
}

/// The server's graph-read authority operations, as a native reader runs
/// them. keeper's implementation is a replica's registered queries;
/// keeper-hq's runs them over its own transport.
pub trait GraphRemote: Send + Sync {
    fn admit_read(&self, context: &str, domain: &str) -> Result<ContextRow, QueryError>;
    fn refresh_context(&self, context: &str) -> Result<ContextRow, QueryError>;
    fn read_status(&self, domain: &str) -> Result<ReadStatus, QueryError>;
}

/// Only the server refusing the caller's authority is `forbidden`
/// ([`crate::hq::port::ReplicaError::is_refusal`]); a server that could not be
/// reached (the query left and its transport failed) is
/// [`QueryError::Unreachable`]; every other failure — an answered internal,
/// storage, unknown or invalid-operation error, retryable or not, a local
/// refusal, an unreadable answer, a closed or non-syncing replica — is
/// `sync_pending`, never a refusal and never unreachable. Nothing is
/// delivered without a fresh answer.
fn remote_error(error: crate::hq::port::ReplicaError) -> QueryError {
    use crate::hq::port::ReplicaError;
    match error {
        error if error.is_refusal() => QueryError::Forbidden,
        ReplicaError::Unheard { .. } => QueryError::Unreachable,
        _ => QueryError::SyncPending,
    }
}

impl GraphRemote for crate::hq::sessions::Source {
    fn admit_read(&self, context: &str, domain: &str) -> Result<ContextRow, QueryError> {
        let rows = self
            .replica
            .remote_query(wire::ADMIT_READ, admit_read_params(context, domain))
            .map_err(remote_error)?;
        decode_context(context, rows).map_err(|_| QueryError::Unavailable("authority.answer"))
    }
    fn refresh_context(&self, context: &str) -> Result<ContextRow, QueryError> {
        let rows = self
            .replica
            .remote_query(wire::REFRESH_CONTEXT, refresh_context_params(context))
            .map_err(remote_error)?;
        decode_context(context, rows).map_err(|_| QueryError::Unavailable("authority.answer"))
    }
    fn read_status(&self, domain: &str) -> Result<ReadStatus, QueryError> {
        let rows = self
            .replica
            .remote_query(wire::READ_STATUS, read_status_params(domain))
            .map_err(remote_error)?;
        decode_read_status(domain, rows).map_err(|_| QueryError::Unavailable("authority.answer"))
    }
}

/// Whether a result's prefix is the replica's whole history.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Completeness {
    Current,
    Stale,
}

/// Which generation a read accepts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Freshness {
    /// Only a generation of the replica's whole history.
    #[default]
    Current,
    /// A complete older generation, reported `Stale`.
    AllowStale,
}

/// A delivered read.
#[derive(Clone, Debug, PartialEq)]
pub struct QueryResult<T> {
    pub rows: T,
    pub domain: String,
    /// N: the prefix holds events 1..=N.
    pub graph_seq: u64,
    pub checkpoint: String,
    pub contract_schema: u32,
    pub projection_version: u32,
    pub engine: String,
    pub completeness: Completeness,
}

/// A typed inspection's answer.
#[derive(Clone, Debug, PartialEq)]
pub enum Inspected {
    Node(NodeView),
    Edge(Map<String, Value>),
}

/// What a refresh found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refreshed {
    pub graph_seq: u64,
    /// Whether a new generation was published.
    pub built: bool,
    pub completeness: Completeness,
}

struct Reads {
    source: Arc<dyn GraphSource>,
    store: Store,
    admission: Arc<Admission>,
    runtime: Arc<Runtime>,
    exporter: AsyncMutex<Option<Exporter>>,
    revoked: watch::Sender<bool>,
    inflight: AtomicUsize,
    idle: Notify,
}

/// One replica's domain's graph reads.
#[derive(Clone)]
pub struct GraphReads(Arc<Reads>);

struct Inflight<'a>(&'a Reads);

impl Drop for Inflight<'_> {
    fn drop(&mut self) {
        if self.0.inflight.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.0.idle.notify_waiters();
        }
    }
}

impl GraphReads {
    /// Open the reads of `source`'s domain over the projection root `root`
    /// (one per replica and domain, chosen by the trusted consumer, never
    /// by a caller), sharing the process's `admission` and `runtime`.
    pub fn open(
        source: Arc<dyn GraphSource>,
        root: &Path,
        admission: Arc<Admission>,
        runtime: Arc<Runtime>,
    ) -> Result<Self, QueryError> {
        if !super::available() {
            return Err(QueryError::Unavailable("platform.unsupported"));
        }
        let store = Store::open(root)?;
        Ok(Self(Arc::new(Reads {
            source,
            store,
            admission,
            runtime,
            exporter: AsyncMutex::new(None),
            revoked: watch::channel(false).0,
            inflight: AtomicUsize::new(0),
            idle: Notify::new(),
        })))
    }

    #[must_use]
    pub fn domain(&self) -> &str {
        self.0.source.domain()
    }

    #[must_use]
    pub fn is_revoked(&self) -> bool {
        *self.0.revoked.borrow()
    }

    /// A reader for an admitted agent context; refused for another domain.
    pub fn agent(
        &self,
        remote: Arc<dyn GraphRemote>,
        read: AdmittedRead,
    ) -> Result<AgentReader, QueryError> {
        if read.domain() != self.domain() || self.is_revoked() {
            return Err(QueryError::Forbidden);
        }
        Ok(AgentReader {
            reads: self.clone(),
            remote,
            read,
            invalidated: Cancellation::default(),
        })
    }

    /// A person's reader over this device's own replica.
    #[cfg(feature = "human-reads")]
    #[must_use]
    pub fn human(&self, source: &crate::hq::sessions::Source) -> Option<HumanReader> {
        (source.database == self.domain()).then(|| HumanReader {
            reads: self.clone(),
            remote: Arc::new(source.clone()),
            actor: source.actor.clone(),
            invalidated: Cancellation::default(),
        })
    }

    /// The kill (D12): block delivery, kill and reap every in-flight worker
    /// of the domain, then drop and purge its projection. Idempotent.
    pub async fn revoke(&self) -> Result<(), QueryError> {
        self.0.revoked.send_replace(true);
        loop {
            let idle = self.0.idle.notified();
            if self.0.inflight.load(Ordering::SeqCst) == 0 {
                break;
            }
            idle.await;
        }
        *self.0.exporter.lock().await = None;
        self.0.store.purge()
    }

    /// After a call has left: a domain revoked during it is purged now.
    async fn settle<T>(&self, result: Result<T, QueryError>) -> Result<T, QueryError> {
        if self.is_revoked() {
            // Nothing is released once the domain is revoked, whatever the
            // call produced.
            self.revoke().await?;
            return Err(QueryError::Forbidden);
        }
        result
    }

    fn enter(&self) -> Result<Inflight<'_>, QueryError> {
        self.0.inflight.fetch_add(1, Ordering::SeqCst);
        let guard = Inflight(&self.0);
        if self.is_revoked() {
            return Err(QueryError::Forbidden);
        }
        Ok(guard)
    }

    fn stops(&self, cancel: &Cancellation, reader: &Cancellation) -> Stops {
        Stops::new(cancel, reader, self.0.revoked.subscribe())
    }

    /// The replica's scope: a revocation kills the domain.
    fn scope(&self) -> Result<i64, QueryError> {
        let checkpoint = self
            .0
            .source
            .checkpoint()
            .map_err(|_| QueryError::Storage("replica"))?;
        match checkpoint.scope {
            ScopeState::Granted => Ok(checkpoint.cursor.unwrap_or(0)),
            ScopeState::Pending => Err(QueryError::SyncPending),
            ScopeState::Revoked => {
                // Block delivery and stop every run now; the purge follows
                // once this call has left (`settle`).
                self.0.revoked.send_replace(true);
                Err(QueryError::Forbidden)
            }
        }
    }

    /// Bring the projection to the replica's complete prefix under
    /// `authority`: verify and export what is new, build and publish a
    /// generation when anything changed.
    async fn refresh(
        &self,
        authority: &Authority,
        cancel: &Cancellation,
        reader: &Cancellation,
    ) -> Result<Refreshed, QueryError> {
        let _inflight = self.enter()?;
        let checkpoint = self.scope()?;
        let mut held = self.0.exporter.lock().await;
        if held.is_none() {
            *held = Some(Exporter::genesis(
                self.domain(),
                &self.0.store.export_path(),
            )?);
        }
        let exporter = held.as_mut().ok_or(QueryError::Storage("export"))?;
        let extended = match exporter.extend(self.0.source.as_ref()) {
            Ok(extended) => extended,
            Err(error) => {
                // A corrupt prefix is never served shortened; the next
                // refresh replays from genesis.
                *held = None;
                tracing::warn!(domain = self.domain(), %error, "graph prefix refused");
                return Err(QueryError::Unavailable("prefix.corrupt"));
            }
        };
        let completeness = if extended.complete {
            Completeness::Current
        } else {
            Completeness::Stale
        };
        let manifest = exporter.manifest(authority, checkpoint);
        if let Some(current) = self.0.store.current() {
            let same = current.manifest.accepted_head == manifest.accepted_head
                && current.manifest.prefix_digest == manifest.prefix_digest
                && current.manifest.authority_epoch == manifest.authority_epoch
                && current.manifest.authority_digest == manifest.authority_digest;
            if same {
                return Ok(Refreshed {
                    graph_seq: manifest.accepted_head,
                    built: false,
                    completeness,
                });
            }
        }
        let _builder = self.0.admission.build()?;
        let staged = self.0.store.stage()?;
        let mut input = manifest
            .encode()
            .map_err(|_| QueryError::Unavailable("manifest"))?;
        let export = tokio::fs::File::open(exporter.path())
            .await
            .map_err(|_| QueryError::Storage("export"))?;
        let bytes = exporter.bytes;
        let last = exporter.last.clone();
        let stream = tokio::io::AsyncReadExt::chain(
            std::io::Cursor::new(std::mem::take(&mut input)),
            tokio::io::AsyncReadExt::take(export, bytes),
        );
        let response = supervisor::run(
            &self.0.runtime,
            &self.0.admission,
            "build",
            staged.path()?,
            stream,
            BUILD_DEADLINE,
            self.stops(cancel, reader),
        )
        .await?;
        if response.manifest != manifest || !response.rows.is_empty() {
            return Err(QueryError::Unavailable("build.answer"));
        }
        drop(held);
        self.0.store.publish(staged, manifest.clone(), last)?;
        tracing::info!(
            domain = self.domain(),
            graph_seq = manifest.accepted_head,
            peak_allocated = response.peak_allocated_bytes,
            "graph generation published"
        );
        Ok(Refreshed {
            graph_seq: manifest.accepted_head,
            built: true,
            completeness,
        })
    }

    /// The generation to serve under `authority`, with its completeness.
    fn select(
        &self,
        authority: &Authority,
        freshness: Freshness,
    ) -> Result<(Generation, Completeness), QueryError> {
        let generation = self
            .0
            .store
            .current()
            .ok_or(QueryError::Unavailable("projection.none"))?;
        if generation.manifest.domain != self.domain() {
            return Err(QueryError::Unavailable("generation.domain"));
        }
        if generation.manifest.authority_epoch != authority.epoch
            || generation.manifest.authority_digest != authority.digest
        {
            return Err(QueryError::Stale);
        }
        let completeness = self.completeness(&generation)?;
        if completeness == Completeness::Stale && freshness == Freshness::Current {
            return Err(QueryError::Stale);
        }
        Ok((generation, completeness))
    }

    /// One snapshot read: is N still the head, with the same event at N?
    fn completeness(&self, generation: &Generation) -> Result<Completeness, QueryError> {
        let n = generation.manifest.accepted_head;
        let batch = self
            .0
            .source
            .prefix_batch(n.saturating_sub(1), 1)
            .map_err(|_| QueryError::Storage("replica"))?;
        if batch.head.unwrap_or(0) != n {
            return Ok(Completeness::Stale);
        }
        if let Some(last) = &generation.last {
            let same = batch.events.iter().any(|e| {
                e.get("id").and_then(Value::as_str) == Some(last.event_id.as_str())
                    && e.get("delta_digest").and_then(Value::as_str)
                        == Some(last.delta_digest.as_str())
            });
            if !same {
                return Err(QueryError::Unavailable("prefix.rewritten"));
            }
        }
        Ok(Completeness::Current)
    }

    async fn request(
        &self,
        principal: &Principal,
        generation: &Generation,
        role: &str,
        bytes: Vec<u8>,
        cancel: &Cancellation,
        reader: &Cancellation,
    ) -> Result<Response, QueryError> {
        let stops = self.stops(cancel, reader);
        if let Some(stop) = stops.stopped() {
            return Err(stop);
        }
        let mut waiting = self.stops(cancel, reader);
        let _permit = tokio::select! {
            permit = self.0.admission.query(principal) => permit?,
            stop = waiting.changed() => return Err(stop),
        };
        let response = supervisor::run(
            &self.0.runtime,
            &self.0.admission,
            role,
            generation.path(),
            bytes.as_slice(),
            QUERY_DEADLINE,
            stops,
        )
        .await
        .inspect_err(|error| {
            // A worker that refused the selected generation itself (exit 74
            // without a startup label: unreadable, not a limit, cancel or
            // class refusal) leaves it unselected, so the next refresh
            // rebuilds it from the accepted history.
            if *error == QueryError::Unavailable("worker.refused") {
                if let Err(discard) = self.0.store.discard(generation) {
                    tracing::warn!(%discard, "graph generation not discarded");
                }
            }
        })?;
        if response.manifest != generation.manifest {
            return Err(QueryError::Unavailable("generation.answer"));
        }
        Ok(response)
    }

    fn result<T>(
        &self,
        rows: T,
        generation: &Generation,
        completeness: Completeness,
    ) -> QueryResult<T> {
        let m = &generation.manifest;
        QueryResult {
            rows,
            domain: m.domain.clone(),
            graph_seq: m.accepted_head,
            checkpoint: m.sync_checkpoint.clone(),
            contract_schema: m.contract_schema,
            projection_version: m.projection_version,
            engine: m.engine.clone(),
            completeness,
        }
    }

    async fn query<F>(
        &self,
        ask: Ask<'_>,
        source: &str,
        parameters: &[u8],
        deliver: F,
    ) -> Result<QueryResult<Vec<Map<String, Value>>>, QueryError>
    where
        F: Future<Output = Result<Authority, QueryError>>,
    {
        let Ask {
            principal,
            authority,
            freshness,
            cancel,
            reader,
            ..
        } = &ask;
        let (freshness, cancel, reader) = (*freshness, *cancel, *reader);
        let _inflight = self.enter()?;
        let admitted = admit(PROFILE, source, parameters)?;
        let bytes = QueryRequest::encode(&admitted, source)
            .map_err(|_| QueryError::Unavailable("frame"))?;
        self.scope()?;
        let (generation, completeness) = self.select(authority, freshness)?;
        let response = self
            .request(principal, &generation, "query", bytes, cancel, reader)
            .await?;
        if response.rows.len() > admitted.limit().min(MAX_ROWS) {
            return Err(QueryError::Unavailable("generation.answer"));
        }
        let mut rows = Vec::with_capacity(response.rows.len());
        for row in response.rows {
            match row {
                Value::Object(row)
                    if row.len() == admitted.aliases().len()
                        && admitted.aliases().iter().all(|a| row.contains_key(a)) =>
                {
                    rows.push(row)
                }
                _ => return Err(QueryError::Unavailable("generation.answer")),
            }
        }
        self.gate(&ask, &generation, deliver).await?;
        Ok(self.result(rows, &generation, completeness))
    }

    async fn inspect<F>(
        &self,
        ask: Ask<'_>,
        entity: InspectEntity,
        id: &str,
        include_archived: bool,
        deliver: F,
    ) -> Result<QueryResult<Option<Inspected>>, QueryError>
    where
        F: Future<Output = Result<Authority, QueryError>>,
    {
        let Ask {
            principal,
            authority,
            freshness,
            cancel,
            reader,
            ..
        } = &ask;
        let (freshness, cancel, reader) = (*freshness, *cancel, *reader);
        let _inflight = self.enter()?;
        let bytes = InspectRequest {
            entity,
            id: id.to_owned(),
            include_archived,
        }
        .encode()
        .map_err(|_| QueryError::Unsupported)?;
        self.scope()?;
        let (generation, completeness) = self.select(authority, freshness)?;
        let response = self
            .request(principal, &generation, "inspect", bytes, cancel, reader)
            .await?;
        let rows: Vec<Map<String, Value>> = response
            .rows
            .into_iter()
            .map(|r| match r {
                Value::Object(r) => Ok(r),
                _ => Err(QueryError::Unavailable("generation.answer")),
            })
            .collect::<Result<_, _>>()?;
        let found = match entity {
            InspectEntity::Node => decode_node(id, rows).map(|n| n.map(Inspected::Node)),
            InspectEntity::Edge => decode_edge(id, rows).map(|e| e.map(Inspected::Edge)),
        }
        .map_err(|_| QueryError::Unavailable("generation.answer"))?;
        self.gate(&ask, &generation, deliver).await?;
        Ok(self.result(found, &generation, completeness))
    }
}

/// A reader's end of a call: a `forbidden` answer — a refusal from the
/// server (at the start or at delivery), or the domain's kill — invalidates
/// the reader for good and stops its other runs.
fn invalidate_on_refusal<T>(
    invalidated: &Cancellation,
    result: Result<T, QueryError>,
) -> Result<T, QueryError> {
    if matches!(result, Err(QueryError::Forbidden)) {
        invalidated.cancel();
    }
    result
}

/// Who asks, under which authority, for which generation, cancellable by
/// what.
struct Ask<'a> {
    principal: &'a Principal,
    authority: &'a Authority,
    freshness: Freshness,
    cancel: &'a Cancellation,
    reader: &'a Cancellation,
    /// What a fresh authority other than `authority` at delivery is: for
    /// an agent's admitted context, `forbidden` (its admission can never
    /// become valid again, so the reader is invalidated); for a person's
    /// device, whose authority is read anew on every call, `projection_stale`.
    mismatch: QueryError,
}

impl GraphReads {
    /// The delivery gate: the kill switch, the fresh authority equal to the
    /// one the result was computed under, then the kill switch again.
    async fn gate<F>(
        &self,
        ask: &Ask<'_>,
        generation: &Generation,
        fresh: F,
    ) -> Result<(), QueryError>
    where
        F: Future<Output = Result<Authority, QueryError>>,
    {
        if self.is_revoked() || ask.reader.is_cancelled() {
            return Err(QueryError::Forbidden);
        }
        let now = fresh.await?;
        if now != *ask.authority
            || generation.manifest.authority_epoch != now.epoch
            || generation.manifest.authority_digest != now.digest
        {
            return Err(ask.mismatch.clone());
        }
        // A sibling call may have invalidated the reader, or the domain may
        // have been revoked, while the server was being asked.
        if self.is_revoked() || ask.reader.is_cancelled() {
            return Err(QueryError::Forbidden);
        }
        Ok(())
    }
}

/// An agent context's admission of one domain: `graph.admit_read`'s
/// answer, bound to the agent, session, context and domain. No other
/// constructor exists, and it is not `Clone`: a reader is never rebuilt
/// without asking the server again.
#[derive(Debug)]
pub struct AdmittedRead {
    agent: String,
    scope: Arc<Scope>,
    authority: Authority,
    expires_at_ms: u64,
}

/// What every delivery re-check asks the server about.
#[derive(Debug)]
struct Scope {
    session: String,
    context: String,
    domain: String,
}

/// The device clock (UTC ms) an admission's expiry is read against. A clock
/// before the epoch reads as the end of time, so every admission reads as
/// expired (fail closed), never as the epoch.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(u64::MAX, |d| {
            u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
        })
}

fn admitted(row: &ContextRow, session: &str, domain: &str) -> Result<(), QueryError> {
    if row.session != session
        || !row.sources.iter().any(|d| d == domain)
        || row.expires_at_ms <= now_ms()
    {
        return Err(QueryError::Forbidden);
    }
    Ok(())
}

impl AdmittedRead {
    /// Admit `domain` into `context` of `agent`'s `session` through the
    /// server (`graph.admit_read`), before anything of the domain is read.
    pub fn admit(
        remote: &dyn GraphRemote,
        agent: &str,
        session: &str,
        context: &str,
        domain: &str,
    ) -> Result<Self, QueryError> {
        let row = remote.admit_read(context, domain)?;
        admitted(&row, session, domain)?;
        Ok(Self {
            agent: agent.to_owned(),
            scope: Arc::new(Scope {
                session: session.to_owned(),
                context: context.to_owned(),
                domain: domain.to_owned(),
            }),
            authority: row.authority,
            expires_at_ms: row.expires_at_ms,
        })
    }

    #[must_use]
    pub fn domain(&self) -> &str {
        &self.scope.domain
    }
    #[must_use]
    pub fn session(&self) -> &str {
        &self.scope.session
    }
    #[must_use]
    pub fn context(&self) -> &str {
        &self.scope.context
    }
    #[must_use]
    pub fn expires_at_ms(&self) -> u64 {
        self.expires_at_ms
    }
}

/// An agent context's reader of one admitted domain.
pub struct AgentReader {
    reads: GraphReads,
    remote: Arc<dyn GraphRemote>,
    read: AdmittedRead,
    /// Fired, for good, the first time the server refuses this reader: its
    /// in-flight runs stop and every later call is refused before any work.
    invalidated: Cancellation,
}

impl AgentReader {
    fn principal(&self) -> Principal {
        Principal {
            agent: self.read.agent.clone(),
            context: self.read.scope.context.clone(),
        }
    }

    /// The context's authority now, refreshed through the server.
    async fn fresh(
        remote: Arc<dyn GraphRemote>,
        scope: Arc<Scope>,
    ) -> Result<Authority, QueryError> {
        let row = tokio::task::spawn_blocking(move || {
            remote
                .refresh_context(&scope.context)
                .and_then(|row| admitted(&row, &scope.session, &scope.domain).map(|()| row))
        })
        .await
        .map_err(|_| QueryError::SyncPending)??;
        Ok(row.authority)
    }

    fn check(&self) -> Result<(), QueryError> {
        if self.invalidated.is_cancelled() || self.read.expires_at_ms <= now_ms() {
            return Err(QueryError::Forbidden);
        }
        Ok(())
    }

    #[must_use]
    pub fn admitted(&self) -> &AdmittedRead {
        &self.read
    }

    /// Build or extend the domain's projection under the context's fresh
    /// authority.
    pub async fn refresh(&self, cancel: &Cancellation) -> Result<Refreshed, QueryError> {
        self.check()?;
        let result = async {
            let authority = Self::fresh(self.remote.clone(), self.read.scope.clone()).await?;
            if authority != self.read.authority {
                return Err(QueryError::Forbidden);
            }
            let result = self
                .reads
                .refresh(&authority, cancel, &self.invalidated)
                .await;
            self.reads.settle(result).await
        }
        .await;
        invalidate_on_refusal(&self.invalidated, result)
    }

    /// One `KG-Cypher/1` query.
    pub async fn query(
        &self,
        source: &str,
        parameters: &[u8],
        freshness: Freshness,
        cancel: &Cancellation,
    ) -> Result<QueryResult<Vec<Map<String, Value>>>, QueryError> {
        self.check()?;
        let deliver = Self::fresh(self.remote.clone(), self.read.scope.clone());
        let result = self
            .reads
            .query(
                Ask {
                    principal: &self.principal(),
                    authority: &self.read.authority,
                    freshness,
                    cancel,
                    reader: &self.invalidated,
                    mismatch: QueryError::Forbidden,
                },
                source,
                parameters,
                deliver,
            )
            .await;
        let result = self.reads.settle(result).await;
        invalidate_on_refusal(&self.invalidated, result)
    }

    /// The typed node or edge lookup (`include_archived` for archived ones).
    pub async fn inspect(
        &self,
        entity: InspectEntity,
        id: &str,
        include_archived: bool,
        freshness: Freshness,
        cancel: &Cancellation,
    ) -> Result<QueryResult<Option<Inspected>>, QueryError> {
        self.check()?;
        let deliver = Self::fresh(self.remote.clone(), self.read.scope.clone());
        let result = self
            .reads
            .inspect(
                Ask {
                    principal: &self.principal(),
                    authority: &self.read.authority,
                    freshness,
                    cancel,
                    reader: &self.invalidated,
                    mismatch: QueryError::Forbidden,
                },
                entity,
                id,
                include_archived,
                deliver,
            )
            .await;
        let result = self.reads.settle(result).await;
        invalidate_on_refusal(&self.invalidated, result)
    }
}

/// A person's reader over their device's own replica of one domain.
#[cfg(feature = "human-reads")]
pub struct HumanReader {
    reads: GraphReads,
    remote: Arc<dyn GraphRemote>,
    actor: String,
    invalidated: Cancellation,
}

#[cfg(feature = "human-reads")]
impl HumanReader {
    fn principal(&self) -> Principal {
        Principal {
            agent: format!("person:{}", self.actor),
            context: "device".to_owned(),
        }
    }

    async fn fresh(remote: Arc<dyn GraphRemote>, domain: String) -> Result<Authority, QueryError> {
        let status = tokio::task::spawn_blocking(move || remote.read_status(&domain))
            .await
            .map_err(|_| QueryError::SyncPending)??;
        Ok(status.authority)
    }

    fn domain(&self) -> String {
        self.reads.domain().to_owned()
    }

    /// Build or extend the domain's projection under the person's current
    /// read authority.
    pub async fn refresh(&self, cancel: &Cancellation) -> Result<Refreshed, QueryError> {
        self.check()?;
        let result = async {
            let authority = Self::fresh(self.remote.clone(), self.domain()).await?;
            let result = self
                .reads
                .refresh(&authority, cancel, &self.invalidated)
                .await;
            self.reads.settle(result).await
        }
        .await;
        invalidate_on_refusal(&self.invalidated, result)
    }

    fn check(&self) -> Result<(), QueryError> {
        if self.invalidated.is_cancelled() {
            return Err(QueryError::Forbidden);
        }
        Ok(())
    }

    /// One `KG-Cypher/1` query.
    pub async fn query(
        &self,
        source: &str,
        parameters: &[u8],
        freshness: Freshness,
        cancel: &Cancellation,
    ) -> Result<QueryResult<Vec<Map<String, Value>>>, QueryError> {
        self.check()?;
        let authority = invalidate_on_refusal(
            &self.invalidated,
            Self::fresh(self.remote.clone(), self.domain()).await,
        )?;
        let deliver = Self::fresh(self.remote.clone(), self.domain());
        let result = self
            .reads
            .query(
                Ask {
                    principal: &self.principal(),
                    authority: &authority,
                    freshness,
                    cancel,
                    reader: &self.invalidated,
                    mismatch: QueryError::Stale,
                },
                source,
                parameters,
                deliver,
            )
            .await;
        let result = self.reads.settle(result).await;
        invalidate_on_refusal(&self.invalidated, result)
    }

    /// The typed node or edge lookup.
    pub async fn inspect(
        &self,
        entity: InspectEntity,
        id: &str,
        include_archived: bool,
        freshness: Freshness,
        cancel: &Cancellation,
    ) -> Result<QueryResult<Option<Inspected>>, QueryError> {
        self.check()?;
        let authority = invalidate_on_refusal(
            &self.invalidated,
            Self::fresh(self.remote.clone(), self.domain()).await,
        )?;
        let deliver = Self::fresh(self.remote.clone(), self.domain());
        let result = self
            .reads
            .inspect(
                Ask {
                    principal: &self.principal(),
                    authority: &authority,
                    freshness,
                    cancel,
                    reader: &self.invalidated,
                    mismatch: QueryError::Stale,
                },
                entity,
                id,
                include_archived,
                deliver,
            )
            .await;
        let result = self.reads.settle(result).await;
        invalidate_on_refusal(&self.invalidated, result)
    }
}

#[cfg(test)]
#[path = "query_tests.rs"]
mod tests;
