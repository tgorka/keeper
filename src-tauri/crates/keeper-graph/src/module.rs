//! The graph as a module of keeper's runtime (story 123.2; feature
//! `module`): the eighteen verbs of `keeper-cli graph` (113.5's seventeen
//! and 116.5's `annotate`), moved here from the command line, bound by id
//! to `keeper-graph-api`'s operations.
//!
//! **A module opens nothing it is not given** (SR-5): the composition root
//! opens one replica per domain and hands it in as a [`DomainBinding`]
//! (or why it could not); this module opens the queue and the projection
//! beside it, under [`GraphConfig::state`], as the verbs always have.
//!
//! **The class witness** (DR-9 b): [`GraphFacade`]'s methods each take the
//! witness of their operation's class — `&Read`, `&Write` or `&Decide` —
//! so a handler bound to a `read` operation holds nothing with which to call
//! a mutating method.
//!
//! **Answers.** A verb's document is its answer, byte for byte as the
//! command line printed it: `Ok` for an `ok` document (or a refusal that
//! carries more than a code and a detail), `Err` for every `fail(code,
//! detail)`, whose code becomes the [`OpError`]'s reason; [`render`] gives
//! the old line back.
//!
//! **Without the worker** (A-20): `graph.inspect` reads through the typed
//! reads (`GraphClient::get_node`, `get_edge`, `get_content`) where no
//! worker is admitted; `graph.query` answers `UNAVAILABLE`
//! (`projection_unavailable`).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use keeper_graph_api as api;
use keeper_ports::server::ServerUrl;
use keeper_ports::{LinkState, ReplicaError, Source};
use keeper_runtime_api::{
    Bindings, Decide, ErrorCode, Manifest, Module, OpError, Read, Reply, Write,
};
use serde_json::{json, Value};

#[cfg(feature = "human-reads")]
use crate::admission::{Admission, Process};
use crate::annotation::{annotate, AnnotateError};
use crate::client::{subscribed_tables, Applied, GraphClient, GraphClientError, Prepared};
use crate::lifecycle::{Lifecycle, LifecycleError};
use crate::outcome::{GraphOutcome, PendingCause};
use crate::protocol::change::Change;
use crate::protocol::inverse::{PreviewOutcome, Reverse};
use crate::protocol::review::Target;
use crate::protocol::tag::normalise;
use crate::protocol::wire::HistoryFilter;
use crate::protocol::worker::InspectEntity;
use crate::protocol::{change, proposal};
#[cfg(feature = "human-reads")]
use crate::query::{Freshness, GraphRemote, HumanReader, Inspected, QueryResult};
use crate::query::{GraphReads, QueryError};
use crate::queue::{resume_sends, Binding, FenceCause, GraphQueue, Restored, State};
use crate::runtime::{Runtime, VerifiedWorker};
use crate::source::{revocation_with, GraphSource, Revocation, ScopeState};
#[cfg(feature = "human-reads")]
use crate::supervisor::Cancellation;

/// What every domain of this module shares: where its state persists and
/// which worker may run its queries.
#[derive(Clone, Debug)]
pub struct GraphConfig {
    /// Where the replicas, queues and projections persist.
    pub state: PathBuf,
    /// The graph worker executable (absolute path).
    pub worker: Option<PathBuf>,
    /// macOS only: run the worker under the evidence-only sandbox.
    pub macos_evidence: bool,
    /// The worker's digest, embedded at the composition root's build.
    pub worker_sha256: Option<&'static str>,
    /// The runtime ledger, embedded at the composition root's build (Linux).
    pub ledger: Option<&'static str>,
}

/// One domain the composition root opened for this module.
pub struct DomainBinding {
    /// keeper-hq's base URL, as given.
    pub server: String,
    pub database: String,
    pub actor: String,
    pub client: String,
    /// The replica the composition root opened, or why it could not.
    pub source: Result<Source, OpError>,
}

/// The arguments the verb bodies read: the shared config and one domain.
struct Args {
    state: PathBuf,
    url: String,
    database: String,
    actor: String,
    client: String,
    worker: Option<PathBuf>,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    macos_evidence: bool,
    worker_sha256: Option<&'static str>,
    ledger: Option<&'static str>,
}

/// The stable codes' classes (an answer's `error` → its [`ErrorCode`]).
fn classify(code: &str) -> ErrorCode {
    match code {
        "projection_unavailable"
        | "storage_unavailable"
        | "sync_pending"
        | "offline"
        | "graph_unavailable"
        | "purge_pending" => ErrorCode::Unavailable,
        "unauthorised" | "forbidden" | "refused" | "not_fenced" | "approval_required" => {
            ErrorCode::Refused
        }
        "unknown_request" | "unknown_domain" | "not_found" => ErrorCode::NotFound,
        "conflict" | "not_ready" => ErrorCode::Conflict,
        "change_limit" => ErrorCode::ResourceExhausted,
        code if code.starts_with("invalid_")
            || code.starts_with("unsupported_")
            || code == "query_unsupported" =>
        {
            ErrorCode::InvalidArgument
        }
        _ => ErrorCode::Internal,
    }
}

/// A verb's document as a [`Reply`]: exactly a `fail(code, detail)` is an
/// error, anything else the answer.
fn reply(answer: Value) -> Reply {
    if let Some(fields) = answer.as_object() {
        let shaped = fields.len() == 3 && fields.get("ok") == Some(&Value::Bool(false));
        if let (true, Some(Value::String(code)), Some(Value::String(detail))) =
            (shaped, fields.get("error"), fields.get("detail"))
        {
            return Err(OpError::new(classify(code), code.clone(), detail.clone()));
        }
    }
    Ok(answer)
}

/// The line the command line prints for an answer: the document, or for an
/// error `{"ok": false, "error": <reason>, "detail": <detail>}` built as the
/// verbs always built it.
#[must_use]
pub fn render(answer: &Reply) -> Value {
    match answer {
        Ok(document) => document.clone(),
        Err(error) => fail(&error.reason, &error.detail),
    }
}

/// One domain, ready for a verb: its arguments, replica and lifecycle.
struct Opened {
    args: Args,
    source: Source,
    lifecycle: Arc<Lifecycle>,
}

struct Inner {
    config: GraphConfig,
    domains: Vec<DomainBinding>,
}

/// The facade the graph's handlers are given: one method per verb, each
/// taking the witness of its operation's class.
pub struct GraphFacade {
    inner: Arc<Inner>,
}

/// The graph module: the facade bound to `keeper-graph-api`'s operations.
pub struct GraphModule {
    facade: Arc<GraphFacade>,
}

impl GraphModule {
    #[must_use]
    pub fn new(config: GraphConfig, domains: Vec<DomainBinding>) -> Self {
        Self {
            facade: Arc::new(GraphFacade {
                inner: Arc::new(Inner { config, domains }),
            }),
        }
    }

    #[must_use]
    pub fn facade(&self) -> Arc<GraphFacade> {
        Arc::clone(&self.facade)
    }
}

macro_rules! bind {
    ($bindings:expr, $facade:expr, $class:ident, $id:literal, $verb:ident) => {{
        let facade = Arc::clone($facade);
        $bindings.$class($id, move |context, input| {
            let facade = Arc::clone(&facade);
            async move { facade.$verb(context.witness(), input).await }
        });
    }};
}

impl Module for GraphModule {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: api::MODULE,
            requires: &[],
            operations: api::OPERATIONS,
        }
    }

    fn bind(&self, b: &mut Bindings) {
        let f = &self.facade;
        bind!(b, f, write, "graph.apply", apply);
        bind!(b, f, write, "graph.enqueue", enqueue);
        bind!(b, f, write, "graph.annotate", annotate);
        bind!(b, f, read, "graph.pending", pending);
        bind!(b, f, write, "graph.resume", resume);
        bind!(b, f, decide, "graph.discard", discard);
        bind!(b, f, write, "graph.sync", sync);
        bind!(b, f, read, "graph.query", query);
        bind!(b, f, read, "graph.inspect", inspect);
        bind!(b, f, read, "graph.history", history);
        bind!(b, f, read, "graph.unreviewed", unreviewed);
        bind!(b, f, write, "graph.propose", propose);
        bind!(b, f, decide, "graph.decide", decide);
        bind!(b, f, read, "graph.proposal", proposal);
        bind!(b, f, decide, "graph.acknowledge", acknowledge);
        bind!(b, f, read, "graph.preview_reversal", preview_reversal);
        bind!(b, f, decide, "graph.reverse", reverse);
        bind!(b, f, read, "graph.status", status);
    }

    fn unavailable(&self, id: &str) -> Option<String> {
        if !matches!(id, "graph.query" | "graph.sync") {
            return None;
        }
        if !cfg!(feature = "human-reads") {
            return Some("projection_unavailable: human_reads".into());
        }
        let config = &self.facade.inner.config;
        let args = Args {
            state: config.state.clone(),
            url: String::new(),
            database: String::new(),
            actor: String::new(),
            client: String::new(),
            worker: config.worker.clone(),
            macos_evidence: config.macos_evidence,
            worker_sha256: config.worker_sha256,
            ledger: config.ledger,
        };
        runtime(&args).err().map(|e| e.to_string())
    }
}

/// `graph.query` and `graph.sync` need a person's reader; a build without
/// `human-reads` has none.
#[cfg(not(feature = "human-reads"))]
fn no_human_reads() -> Value {
    fail(
        "projection_unavailable",
        "projection_unavailable: human_reads",
    )
}

impl GraphFacade {
    /// The domain `target` addresses, its replica (or why it is not open)
    /// first, then its queue and fence: the order the verbs always had.
    fn open(&self, target: &api::Target) -> Result<Opened, Value> {
        let config = &self.inner.config;
        let Some(binding) = self
            .inner
            .domains
            .iter()
            .find(|d| d.database == target.domain && d.server == target.server)
        else {
            return Err(fail(
                "unknown_domain",
                &format!("{} is not open here", target.domain),
            ));
        };
        let source = match &binding.source {
            Ok(source) => source.clone(),
            Err(error) => return Err(render(&Err(error.clone()))),
        };
        let args = Args {
            state: config.state.clone(),
            url: binding.server.clone(),
            database: binding.database.clone(),
            actor: binding.actor.clone(),
            client: binding.client.clone(),
            worker: config.worker.clone(),
            macos_evidence: config.macos_evidence,
            worker_sha256: config.worker_sha256,
            ledger: config.ledger,
        };
        let lifecycle = lifecycle(&args)?;
        Ok(Opened {
            args,
            source,
            lifecycle,
        })
    }

    pub async fn apply(&self, _: &Write, input: api::Targeted) -> Reply {
        reply(self.apply_or_enqueue(input, false).await)
    }

    pub async fn enqueue(&self, _: &Write, input: api::Targeted) -> Reply {
        reply(self.apply_or_enqueue(input, true).await)
    }

    async fn apply_or_enqueue(&self, input: api::Targeted, queue: bool) -> Value {
        let Opened {
            source, lifecycle, ..
        } = match self.open(&input.target) {
            Ok(opened) => opened,
            Err(error) => return error,
        };
        let change = match parse_change(input.payload) {
            Ok(change) => change,
            Err(error) => return error,
        };
        if !queue {
            return blocking(&source, &lifecycle, move |client, lifecycle| {
                applied(lifecycle, client.apply(&change))
            })
            .await;
        }
        if let Some(error) = signals(&lifecycle, &source, None).await {
            return error;
        }
        let client = GraphClient::new(source.clone());
        let prepared = match client.mint(&change) {
            Ok(prepared) => prepared,
            Err(refusal) => return fail(refusal.code(), "payload"),
        };
        match lifecycle.with_queue(|q| q.enqueue(&prepared, now_ms())) {
            Ok(()) => json!({"ok": true, "request_id": prepared.request().request_id,
                                 "state": State::Queued.as_str()}),
            Err(e) => lifecycle_error(&e),
        }
    }

    pub async fn annotate(&self, _: &Write, input: api::Annotate) -> Reply {
        let Opened {
            source, lifecycle, ..
        } = match self.open(&input.target) {
            Ok(opened) => opened,
            Err(error) => return reply(error),
        };
        let api::Annotate {
            entry_id,
            note,
            tags,
            ..
        } = input;
        reply(
            blocking(
                &source,
                &lifecycle,
                move |client, lifecycle| match annotate(&client, &entry_id, note, tags) {
                    Ok(a) => {
                        let mut v = applied(lifecycle, Ok(a.applied));
                        v["node_id"] = json!(a.node_id);
                        v["created"] = json!(a.created);
                        v["retried"] = json!(a.retried);
                        v
                    }
                    Err(AnnotateError::Refused(r)) => fail(r.code(), "annotation"),
                    Err(AnnotateError::Missing(id)) => fail("not_found", &format!("entry {id}")),
                    Err(AnnotateError::Client(e)) => applied(lifecycle, Err(e)),
                },
            )
            .await,
        )
    }

    pub async fn pending(&self, _: &Read, input: api::TargetOnly) -> Reply {
        let Opened {
            source, lifecycle, ..
        } = match self.open(&input.target) {
            Ok(opened) => opened,
            Err(error) => return reply(error),
        };
        if let Some(error) = signals(&lifecycle, &source, None).await {
            return reply(error);
        }
        let now = now_ms();
        reply(match lifecycle.with_queue(|q| q.entries()) {
            Ok(entries) => json!({
                "ok": true,
                "entries": entries.iter().map(|e| json!({
                    "request_id": e.request_id, "state": e.state.as_str(),
                    "attempts": e.attempts, "cause": e.last_cause,
                    "age_ms": now.saturating_sub(e.enqueued_at_ms),
                })).collect::<Vec<_>>(),
            }),
            Err(e) => lifecycle_error(&e),
        })
    }

    pub async fn resume(&self, _: &Write, input: api::TargetOnly) -> Reply {
        let Opened {
            source, lifecycle, ..
        } = match self.open(&input.target) {
            Ok(opened) => opened,
            Err(error) => return reply(error),
        };
        wait(&source, Duration::from_secs(30), |_| true).await;
        if let Some(error) = signals(&lifecycle, &source, None).await {
            return reply(error);
        }
        let client = GraphClient::new(source.clone());
        let held = Arc::clone(&lifecycle);
        reply(
            match tokio::task::spawn_blocking(move || resume(&held, &client)).await {
                Ok(answer) => answer,
                Err(e) => fail("storage_unavailable", &e.to_string()),
            },
        )
    }

    pub async fn discard(&self, _: &Decide, input: api::Discard) -> Reply {
        let Opened {
            source, lifecycle, ..
        } = match self.open(&input.target) {
            Ok(opened) => opened,
            Err(error) => return reply(error),
        };
        if let Some(error) = signals(&lifecycle, &source, None).await {
            return reply(error);
        }
        let request_id = input.request_id;
        reply(
            match lifecycle.with_queue(|q| q.discard(&request_id, now_ms())) {
                Ok(()) => json!({"ok": true, "request_id": request_id,
                                     "state": State::Withdrawn.as_str()}),
                Err(e) => lifecycle_error(&e),
            },
        )
    }

    pub async fn sync(&self, _: &Write, input: api::Sync) -> Reply {
        let Opened {
            args,
            source,
            lifecycle,
        } = match self.open(&input.target) {
            Ok(opened) => opened,
            Err(error) => return reply(error),
        };
        reply(sync(&args, &source, &lifecycle, input.wait_secs).await)
    }

    pub async fn query(&self, _: &Read, input: api::Query) -> Reply {
        let Opened {
            args,
            source,
            lifecycle,
        } = match self.open(&input.target) {
            Ok(opened) => opened,
            Err(error) => return reply(error),
        };
        reply(query(&args, &source, &lifecycle, &input).await)
    }

    pub async fn inspect(&self, _: &Read, input: api::Inspect) -> Reply {
        let Opened {
            args,
            source,
            lifecycle,
        } = match self.open(&input.target) {
            Ok(opened) => opened,
            Err(error) => return reply(error),
        };
        let entity = match input.entity.as_str() {
            "node" => InspectEntity::Node,
            "edge" => InspectEntity::Edge,
            "content" => InspectEntity::Content,
            _ => return reply(fail("query_unsupported", "entity is node, edge or content")),
        };
        // Where no worker is admitted (or no person's reader is built), the
        // typed reads answer, online only (A-20).
        let admitted = cfg!(feature = "human-reads") && runtime(&args).is_ok();
        if !admitted {
            if let Some(error) = signals(&lifecycle, &source, None).await {
                return reply(error);
            }
            if let Err(e) = lifecycle.gate() {
                return reply(query_error(&e));
            }
            let client = GraphClient::new(source.clone());
            return inspect_typed(
                client,
                entity,
                input.id,
                input.include_archived,
                input.allow_stale,
                args.database,
            )
            .await;
        }
        reply(inspect_projection(&args, &source, &lifecycle, entity, &input).await)
    }

    pub async fn history(&self, _: &Read, input: api::Page) -> Reply {
        let Opened {
            source, lifecycle, ..
        } = match self.open(&input.target) {
            Ok(opened) => opened,
            Err(error) => return reply(error),
        };
        let client = GraphClient::new(source.clone());
        wait(&source, Duration::from_secs(30), |_| true).await;
        if let Some(error) = signals(&lifecycle, &source, None).await {
            return reply(error);
        }
        if let Err(e) = lifecycle.gate() {
            return reply(query_error(&e));
        }
        let (after, limit) = (input.after, input.limit);
        reply(
            match tokio::task::spawn_blocking(move || {
                client.history(&after, limit, &HistoryFilter::default())
            })
            .await
            {
                Ok(Ok(page)) => json!({"ok": true, "page": page}),
                Ok(Err(e)) => fail("refused", &e.to_string()),
                Err(e) => fail("storage_unavailable", &e.to_string()),
            },
        )
    }

    pub async fn unreviewed(&self, _: &Read, input: api::Page) -> Reply {
        let Opened {
            source, lifecycle, ..
        } = match self.open(&input.target) {
            Ok(opened) => opened,
            Err(error) => return reply(error),
        };
        let (after, limit) = (input.after, input.limit);
        reply(
            blocking(&source, &lifecycle, move |client, _| {
                match client.unreviewed(&after, limit) {
                    Ok(page) => json!({"ok": true, "page": page}),
                    Err(e) => fail("refused", &e.to_string()),
                }
            })
            .await,
        )
    }

    pub async fn propose(&self, _: &Write, input: api::Targeted) -> Reply {
        let Opened {
            source, lifecycle, ..
        } = match self.open(&input.target) {
            Ok(opened) => opened,
            Err(error) => return reply(error),
        };
        let proposal = match proposal::parse(&Value::Object(input.payload), normalise) {
            Ok(p) => p,
            Err(r) => return reply(fail(r.code(), "payload")),
        };
        reply(
            blocking(&source, &lifecycle, move |client, lifecycle| {
                applied(lifecycle, client.propose(&proposal))
            })
            .await,
        )
    }

    pub async fn decide(&self, _: &Decide, input: api::Targeted) -> Reply {
        let Opened {
            source, lifecycle, ..
        } = match self.open(&input.target) {
            Ok(opened) => opened,
            Err(error) => return reply(error),
        };
        let decision = match proposal::parse_decide(&Value::Object(input.payload)) {
            Ok(d) => d,
            Err(r) => return reply(fail(r.code(), "payload")),
        };
        reply(
            blocking(&source, &lifecycle, move |client, lifecycle| {
                applied(lifecycle, client.decide(&decision))
            })
            .await,
        )
    }

    pub async fn proposal(&self, _: &Read, input: api::ProposalRead) -> Reply {
        let Opened {
            source, lifecycle, ..
        } = match self.open(&input.target) {
            Ok(opened) => opened,
            Err(error) => return reply(error),
        };
        let id = input.proposal_id;
        reply(
            blocking(&source, &lifecycle, move |client, _| {
                match client.proposal(&id) {
                    Ok(rows) => json!({"ok": true, "revisions": rows}),
                    Err(e) => fail("refused", &e.to_string()),
                }
            })
            .await,
        )
    }

    pub async fn acknowledge(&self, _: &Decide, input: api::Acknowledge) -> Reply {
        let Opened {
            source, lifecycle, ..
        } = match self.open(&input.target) {
            Ok(opened) => opened,
            Err(error) => return reply(error),
        };
        let target = match Target::new(&input.event_id, &input.delta_digest) {
            Ok(t) => t,
            Err(r) => return reply(fail(r.code(), "target")),
        };
        reply(
            blocking(&source, &lifecycle, move |client, lifecycle| {
                applied(lifecycle, client.acknowledge(&target))
            })
            .await,
        )
    }

    pub async fn preview_reversal(&self, _: &Read, input: api::EventId) -> Reply {
        let Opened {
            source, lifecycle, ..
        } = match self.open(&input.target) {
            Ok(opened) => opened,
            Err(error) => return reply(error),
        };
        let id = input.event_id;
        reply(
            blocking(&source, &lifecycle, move |client, _| {
                match client.preview_reversal(&id) {
                    Ok(preview) => json!({"ok": true, "preview": preview}),
                    Err(e) => fail("refused", &e.to_string()),
                }
            })
            .await,
        )
    }

    pub async fn reverse(&self, _: &Decide, input: api::Reverse) -> Reply {
        let Opened {
            source, lifecycle, ..
        } = match self.open(&input.target) {
            Ok(opened) => opened,
            Err(error) => return reply(error),
        };
        let id = input.event_id;
        let approval_id = input.approval_id;
        reply(
            blocking(&source, &lifecycle, move |client, lifecycle| {
                let preview = match client.preview_reversal(&id) {
                    Ok(p) => p,
                    Err(e) => return fail("refused", &e.to_string()),
                };
                let ready = match preview.outcome {
                    PreviewOutcome::Ready => true,
                    PreviewOutcome::ApprovalRequired => approval_id.is_some(),
                    PreviewOutcome::Conflict | PreviewOutcome::Rejected => false,
                };
                match preview.plan.as_ref().filter(|_| ready) {
                    Some(plan) => match Reverse::of(plan, approval_id, None) {
                        Ok(reverse) => applied(lifecycle, client.reverse(&reverse)),
                        Err(r) => fail(r.code(), "reverse"),
                    },
                    None if preview.outcome == PreviewOutcome::ApprovalRequired => {
                        json!({"ok": false, "error": "approval_required", "preview": preview})
                    }
                    None => json!({"ok": false, "error": "not_ready", "preview": preview}),
                }
            })
            .await,
        )
    }

    pub async fn status(&self, _: &Read, input: api::TargetOnly) -> Reply {
        let Opened {
            args,
            source,
            lifecycle,
        } = match self.open(&input.target) {
            Ok(opened) => opened,
            Err(error) => return reply(error),
        };
        let head = source.prefix_batch(0, 0).map(|b| b.head);
        let checkpoint = source.checkpoint();
        let runtime = runtime(&args).map(|_| ()).map_err(|e| e.to_string());
        let fence = lifecycle
            .fence()
            .map(|f| f.map(|f| format!("{f:?}").to_lowercase()));
        let counts = lifecycle.counts().map(|counts| {
            counts
                .into_iter()
                .map(|(state, n)| (state.as_str().to_owned(), json!(n)))
                .collect::<serde_json::Map<_, _>>()
        });
        reply(json!({
            "ok": true,
            "head": head.ok().flatten(),
            "checkpoint": checkpoint.ok().map(|c| json!({"scope": format!("{:?}", c.scope).to_lowercase(), "cursor": c.cursor})),
            "runtime": match runtime { Ok(()) => json!("admitted"), Err(e) => json!(e) },
            "fence": fence.ok().flatten(),
            "queue": counts.ok(),
        }))
    }
}

#[cfg(feature = "human-reads")]
async fn sync(args: &Args, source: &Source, lifecycle: &Arc<Lifecycle>, wait_secs: u64) -> Value {
    let cancel = Cancellation::default();
    let ready = wait(source, Duration::from_secs(wait_secs), |s| {
        s.checkpoint().is_ok_and(|c| c.scope != ScopeState::Pending)
    })
    .await;
    if let Some(error) = signals(lifecycle, source, None).await {
        return error;
    }
    if !ready && !lifecycle.is_locked() {
        return fail(
            "sync_pending",
            "the graph subscriptions have not pulled yet",
        );
    }
    let reads = if lifecycle.is_locked() {
        match unfence(args, source, lifecycle).await {
            Ok(reads) => reads,
            Err(error) => return error,
        }
    } else {
        match reads(args, source) {
            Ok(reads) => reads,
            Err(e) => return query_error(&e),
        }
    };
    match reader(&reads, source, lifecycle) {
        Ok(reader) => match reader.refresh(&cancel).await {
            Ok(r) => json!({"ok": true, "graph_seq": r.graph_seq, "built": r.built,
                                "completeness": format!("{:?}", r.completeness).to_lowercase()}),
            Err(e) => query_error(&e),
        },
        Err(e) => query_error(&e),
    }
}

#[cfg(not(feature = "human-reads"))]
async fn sync(_: &Args, _: &Source, _: &Arc<Lifecycle>, _: u64) -> Value {
    no_human_reads()
}

#[cfg(feature = "human-reads")]
async fn query(
    args: &Args,
    source: &Source,
    lifecycle: &Arc<Lifecycle>,
    input: &api::Query,
) -> Value {
    let cancel = Cancellation::default();
    let reads = match reads(args, source) {
        Ok(reads) => reads,
        Err(e) => return query_error(&e),
    };
    if let Some(error) = signals(lifecycle, source, Some(&reads)).await {
        return error;
    }
    match reader(&reads, source, lifecycle) {
        Ok(reader) => {
            let freshness = if input.allow_stale {
                Freshness::AllowStale
            } else {
                Freshness::Current
            };
            match reader
                .query(&input.source, input.params.as_bytes(), freshness, &cancel)
                .await
            {
                Ok(r) => result(&r, json!(r.rows)),
                Err(e) => query_error(&e),
            }
        }
        Err(e) => query_error(&e),
    }
}

#[cfg(not(feature = "human-reads"))]
async fn query(_: &Args, _: &Source, _: &Arc<Lifecycle>, _: &api::Query) -> Value {
    no_human_reads()
}

#[cfg(feature = "human-reads")]
async fn inspect_projection(
    args: &Args,
    source: &Source,
    lifecycle: &Arc<Lifecycle>,
    entity: InspectEntity,
    input: &api::Inspect,
) -> Value {
    let cancel = Cancellation::default();
    let freshness = if input.allow_stale {
        Freshness::AllowStale
    } else {
        Freshness::Current
    };
    let reads = match reads(args, source) {
        Ok(reads) => reads,
        Err(e) => return query_error(&e),
    };
    if let Some(error) = signals(lifecycle, source, Some(&reads)).await {
        return error;
    }
    match reader(&reads, source, lifecycle) {
        Ok(reader) => match reader
            .inspect(
                entity,
                &input.id,
                input.include_archived,
                freshness,
                &cancel,
            )
            .await
        {
            Ok(r) => {
                let rows = match &r.rows {
                    None => Value::Null,
                    Some(Inspected::Node(v)) => {
                        json!({"image": v.image, "tags": v.tags})
                    }
                    Some(Inspected::Edge(e) | Inspected::Content(e)) => json!(e),
                };
                result(&r, rows)
            }
            Err(e) => query_error(&e),
        },
        Err(e) => query_error(&e),
    }
}

#[cfg(not(feature = "human-reads"))]
async fn inspect_projection(
    _: &Args,
    _: &Source,
    _: &Arc<Lifecycle>,
    _: InspectEntity,
    _: &api::Inspect,
) -> Value {
    no_human_reads()
}

/// `graph.inspect` through the typed reads (A-20): online only, rows shaped
/// as the projection gives them (`{"image", "tags"}` for a node); a stale
/// read is not offered here.
pub async fn inspect_typed(
    client: GraphClient,
    entity: InspectEntity,
    id: String,
    include_archived: bool,
    allow_stale: bool,
    domain: String,
) -> Reply {
    if allow_stale {
        return Err(OpError::invalid_argument(
            "allow_stale",
            "a stale read needs the projection, which this host does not admit",
        ));
    }
    if let Err(error) = client.online() {
        return Err(OpError::new(
            ErrorCode::Unavailable,
            "offline",
            error.to_string(),
        ));
    }
    let answer = tokio::task::spawn_blocking(move || match entity {
        InspectEntity::Node => client
            .get_node(&id, include_archived)
            .map(|node| node.map(|v| json!({"image": v.image, "tags": v.tags}))),
        InspectEntity::Edge => client
            .get_edge(&id, include_archived)
            .map(|edge| edge.map(Value::Object)),
        InspectEntity::Content => client
            .get_content(&id, include_archived)
            .map(|content| content.map(Value::Object)),
    })
    .await
    .map_err(|e| OpError::new(ErrorCode::Internal, "storage_unavailable", e.to_string()))?;
    match answer {
        Ok(rows) => Ok(json!({
            "ok": true,
            "rows": rows.unwrap_or(Value::Null),
            "domain": domain,
            "source": "typed_reads",
        })),
        Err(GraphClientError::Offline) => Err(OpError::new(
            ErrorCode::Unavailable,
            "offline",
            GraphClientError::Offline.to_string(),
        )),
        // A replica still connecting (`online` admits `Connecting`) or not
        // syncing, or a query nothing came back for: the server was not
        // reached, so the read is unavailable, not refused. `Unheard`'s
        // `retryable` is the client's word on asking again now.
        Err(GraphClientError::Replica(
            error @ (ReplicaError::NotSyncing | ReplicaError::Closed),
        )) => Err(OpError::new(
            ErrorCode::Unavailable,
            "offline",
            error.to_string(),
        )),
        Err(GraphClientError::Replica(ReplicaError::Unheard {
            code,
            message,
            retryable,
        })) => Err(OpError::new(
            ErrorCode::Unavailable,
            "offline",
            format!("{code}: {message}"),
        )
        .retryable(retryable)),
        Err(error) => Err(OpError::new(
            ErrorCode::Refused,
            "refused",
            error.to_string(),
        )),
    }
}

fn fail(code: &str, detail: &str) -> Value {
    json!({"ok": false, "error": code, "detail": detail})
}

fn query_error(error: &QueryError) -> Value {
    fail(error.code(), &error.to_string())
}

fn lifecycle_error(error: &LifecycleError) -> Value {
    let code = match error {
        LifecycleError::Queue(error) => error.code(),
        LifecycleError::Query(error) => error.code(),
        LifecycleError::NotFenced => "not_fenced",
        LifecycleError::PurgePending => "purge_pending",
        LifecycleError::NotFresh => "forbidden",
    };
    fail(code, &error.to_string())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
        })
}

fn outcome(outcome: &GraphOutcome) -> Value {
    match outcome {
        GraphOutcome::Accepted {
            event_id,
            graph_seq,
            commit_seq,
            detail,
        } => {
            json!({"outcome": "accepted", "event_id": event_id, "graph_seq": graph_seq,
                       "commit_seq": commit_seq, "detail": detail})
        }
        GraphOutcome::Proposed { detail } => json!({"outcome": "proposed", "detail": detail}),
        GraphOutcome::Conflict { detail } => json!({"outcome": "conflict", "detail": detail}),
        GraphOutcome::Rejected { reason, detail } => {
            json!({"outcome": "rejected", "reason": reason, "detail": detail})
        }
        GraphOutcome::PendingVisibility { commit_seq } => {
            json!({"outcome": "pending_visibility", "commit_seq": commit_seq})
        }
        GraphOutcome::Pending { cause } => {
            json!({"outcome": "pending", "cause": cause.as_str()})
        }
        other => json!({"outcome": "pending", "state": format!("{other:?}")}),
    }
}

/// A command's answer: its request id, outcome and the stable code a
/// caller reports for it. A revoking outcome (a refused `graph.outcome`
/// or authority read) fences the domain, from its cause, before the
/// answer.
fn applied(lifecycle: &Lifecycle, answer: Result<Applied, GraphClientError>) -> Value {
    let applied = match answer {
        Ok(applied) => applied,
        Err(e) => return fail("refused", &e.to_string()),
    };
    let fenced = match &applied.outcome {
        Ok(o) => o
            .fence_cause()
            .map(|cause| lifecycle.lock(now_ms(), cause).is_ok()),
        Err(_) => None,
    };
    json!({
        "ok": true,
        "request_id": applied.prepared.request().request_id,
        "descriptor": applied.prepared.key().descriptor(),
        "outcome": match &applied.outcome {
            Ok(o) => {
                let mut v = outcome(o);
                v["code"] = json!(o.code(applied.prepared.key()));
                v
            }
            Err(e) => fail("pending", &e.to_string()),
        },
        "fenced": fenced,
    })
}

/// Run `call` on a blocking thread once the replica's first round is
/// done, and only while the domain is not fenced: a revocation signal
/// seen now locks the domain first, and a locked domain is never sent to
/// or read from.
async fn blocking(
    source: &Source,
    lifecycle: &Arc<Lifecycle>,
    call: impl FnOnce(GraphClient, &Lifecycle) -> Value + Send + 'static,
) -> Value {
    let client = GraphClient::new(source.clone());
    wait(source, Duration::from_secs(30), |_| true).await;
    if let Some(error) = signals(lifecycle, source, None).await {
        return error;
    }
    if let Err(e) = lifecycle.gate() {
        return query_error(&e);
    }
    let lifecycle = Arc::clone(lifecycle);
    tokio::task::spawn_blocking(move || call(client, &lifecycle))
        .await
        .unwrap_or_else(|e| fail("storage_unavailable", &e.to_string()))
}

#[cfg(feature = "human-reads")]
fn result<T>(r: &QueryResult<T>, rows: Value) -> Value {
    json!({
        "ok": true, "rows": rows, "domain": r.domain, "graph_seq": r.graph_seq,
        "checkpoint": r.checkpoint, "contract_schema": r.contract_schema,
        "projection_version": r.projection_version, "engine": r.engine,
        "completeness": format!("{:?}", r.completeness).to_lowercase(),
    })
}

/// The domain's queue, fence and read authority, beside the replica.
fn lifecycle(args: &Args) -> Result<Arc<Lifecycle>, Value> {
    let server = ServerUrl::parse(&args.url)
        .ok()
        .flatten()
        .ok_or_else(|| fail("invalid_server", "--url is not an agents server address"))?;
    let queue = GraphQueue::open(
        &args.state.join(format!("{}.graph-queue", args.database)),
        Binding {
            server: server.as_str().to_owned(),
            domain: args.database.clone(),
            actor: args.actor.clone(),
            client: args.client.clone(),
        },
    )
    .map_err(|e| fail(e.code(), &e.to_string()))?;
    Lifecycle::open(queue)
        .map(Arc::new)
        .map_err(|e| lifecycle_error(&e))
}

fn runtime(args: &Args) -> Result<Arc<Runtime>, QueryError> {
    let path = args
        .worker
        .as_deref()
        .ok_or(QueryError::Unavailable("worker.path"))?;
    let worker = VerifiedWorker::open(path, args.worker_sha256)?;
    #[cfg(target_os = "macos")]
    if args.macos_evidence {
        return Runtime::macos_evidence(worker).map(Arc::new);
    }
    Runtime::admit(worker, args.ledger).map(Arc::new)
}

#[cfg(feature = "human-reads")]
fn reads(args: &Args, source: &Source) -> Result<GraphReads, QueryError> {
    let runtime = runtime(args)?;
    let root = args.state.join(format!("{}.projection", args.database));
    GraphReads::open(
        Arc::new(source.clone()) as Arc<dyn GraphSource>,
        &root,
        Admission::process(Process::Headless)?,
        runtime,
    )
}

#[cfg(feature = "human-reads")]
fn reader(
    reads: &GraphReads,
    source: &Source,
    lifecycle: &Arc<Lifecycle>,
) -> Result<HumanReader, QueryError> {
    reads
        .human(source)
        .ok_or(QueryError::Forbidden)?
        .with_lifecycle(Arc::clone(lifecycle))
}

/// The revocation signals this device can see without asking: a
/// refused link (the device's credential was refused: a credential
/// fence), a revoked graph, receipt or proposal subscription, or history
/// subscriptions that no longer hold the domain (a revocation fence,
/// which wins when both are seen). Each fences the domain (`reads`
/// purged when held); a fence that could not be completed is the run's
/// answer (the domain stays locked in this process).
async fn signals(
    lifecycle: &Lifecycle,
    source: &Source,
    reads: Option<&GraphReads>,
) -> Option<Value> {
    let link = source.replica.state().link;
    let revoked = source
        .checkpoint()
        .is_ok_and(|c| c.scope == ScopeState::Revoked)
        || revocation_with(&subscribed_tables(), |sql, params| {
            Ok(source.replica.query(sql, params)?)
        })
        .is_ok_and(|r| r != Revocation::None);
    fence_on(lifecycle, link, revoked, reads).await
}

/// The fence a signal sets (owner decision 2026-11-12, H3): a revoked
/// subscription is a revocation, a refused link alone (the device's
/// credential refused, as after an attestation lapsed) a credential
/// fence; a revocation wins when both are seen. `None`: no signal.
fn signal_cause(link: LinkState, revoked: bool) -> Option<FenceCause> {
    if revoked {
        Some(FenceCause::Revocation)
    } else if link == LinkState::Refused {
        Some(FenceCause::Credential)
    } else {
        None
    }
}

/// Fence the domain on the signals `link` and `revoked` show, from
/// their cause ([`signal_cause`]); a fence that could not be completed
/// is the run's answer.
async fn fence_on(
    lifecycle: &Lifecycle,
    link: LinkState,
    revoked: bool,
    reads: Option<&GraphReads>,
) -> Option<Value> {
    let cause = signal_cause(link, revoked)?;
    lifecycle
        .revoke(reads, now_ms(), cause)
        .await
        .err()
        .map(|error| lifecycle_error(&error))
}

fn parse_change(payload: serde_json::Map<String, Value>) -> Result<Change, Value> {
    change::parse(&Value::Object(payload), normalise)
        .map_err(|refusal| fail(refusal.code(), "payload"))
}

/// A fenced domain at `sync`: finish the cleanup, record the replica's
/// purge once seen, and unlock only with fresh authority (a successful
/// `read_status`, granted subscriptions and a new projection instance).
/// A credential fence needs no purge: the purge not yet observed blocks
/// only a revocation fence ([`Lifecycle::regrant`]).
/// The reads to go on with, or why the domain stays locked.
#[cfg(feature = "human-reads")]
async fn unfence(args: &Args, source: &Source, lifecycle: &Lifecycle) -> Result<GraphReads, Value> {
    let held = reads(args, source).map_err(|e| query_error(&e))?;
    lifecycle
        .resume(Some(&held), now_ms())
        .await
        .map_err(|e| lifecycle_error(&e))?;
    let revocation = revocation_with(&subscribed_tables(), |sql, params| {
        Ok(source.replica.query(sql, params)?)
    })
    .map_err(|e| fail("storage_unavailable", &e.to_string()))?;
    lifecycle
        .observe_purge(revocation, now_ms())
        .map_err(|e| lifecycle_error(&e))?;
    let checkpoint = source
        .checkpoint()
        .map_err(|e| fail("storage_unavailable", &e.to_string()))?;
    if checkpoint.scope != ScopeState::Granted {
        return Err(fail(
            "forbidden",
            "this domain is fenced after a revocation",
        ));
    }
    let remote = source.clone();
    let domain = args.database.clone();
    let status = tokio::task::spawn_blocking(move || remote.read_status(&domain))
        .await
        .map_err(|e| fail("sync_pending", &e.to_string()))?
        .map_err(|e| query_error(&e))?;
    // The revoked instance holds the projection's lock: drop it first.
    drop(held);
    let fresh = reads(args, source).map_err(|e| query_error(&e))?;
    lifecycle
        .regrant(&status, checkpoint, &fresh)
        .map_err(|e| lifecycle_error(&e))?;
    lifecycle
        .record_authority(&status.authority, now_ms())
        .map_err(|e| lifecycle_error(&e))?;
    Ok(fresh)
}

/// Resolve or send one restored entry: its receipt first, the
/// identical request under its own id only when the one policy says
/// nothing is known of it ([`resume_sends`]: never after
/// `outcome_unresolved`, never on `graph_unavailable`) and the domain is
/// not locked. Every answer goes through the one policy
/// ([`Lifecycle::settle`]): a revoking one fences the domain before the answer.
fn resume_one(
    lifecycle: &Lifecycle,
    client: &GraphClient,
    prepared: &Prepared,
    state: State,
) -> Value {
    let id = prepared.request().request_id.clone();
    if state == State::AwaitingApproval {
        // An agent's, waiting for a person: never sent from here.
        return entry(&id, state, None, None);
    }
    let looked = match client.outcome(prepared, None) {
        Ok(outcome) => outcome,
        Err(error) => return entry(&id, state, None, Some(&error.to_string())),
    };
    let from = state;
    let state = match lifecycle.settle(&id, from, &looked, now_ms()) {
        Ok(state) => state,
        Err(error) => return entry(&id, from, None, Some(&error.to_string())),
    };
    let cause = looked.pending_cause();
    let agent = prepared.request().context_id.is_some();
    if !resume_sends(from, &looked, state, agent) {
        return entry(&id, state, cause, None);
    }
    if let Err(error) = lifecycle.gate() {
        return entry(&id, state, cause, Some(&error.to_string()));
    }
    // Offline: nothing starts, so the entry is not marked as sent.
    if let Err(error) = client.online() {
        return entry(&id, state, cause, Some(&error.to_string()));
    }
    let moved = match state {
        State::Submitting => lifecycle.with_queue(|q| q.retry(&id, "resent_same_id", now_ms())),
        from => {
            lifecycle.with_queue(|q| q.mark(&id, from, State::Submitting, Some("sent"), now_ms()))
        }
    };
    if let Err(error) = moved {
        return entry(&id, state, cause, Some(&error.to_string()));
    }
    let attempt = match client.send(prepared) {
        Ok(attempt) => attempt,
        Err(error) => return entry(&id, State::Submitting, None, Some(&error.to_string())),
    };
    let outcome = match client.outcome(prepared, Some(&attempt)) {
        Ok(outcome) => outcome,
        Err(error) => return entry(&id, State::Submitting, None, Some(&error.to_string())),
    };
    match lifecycle.settle(&id, State::Submitting, &outcome, now_ms()) {
        Ok(state) => entry(&id, state, outcome.pending_cause(), None),
        Err(error) => entry(&id, State::Submitting, None, Some(&error.to_string())),
    }
}

fn entry(id: &str, state: State, cause: Option<PendingCause>, error: Option<&str>) -> Value {
    json!({"request_id": id, "state": state.as_str(),
               "cause": cause.map(PendingCause::as_str), "error": error})
}

fn resume(lifecycle: &Lifecycle, client: &GraphClient) -> Value {
    let restored = match lifecycle.with_queue(|q| q.restore(client, now_ms())) {
        Ok(restored) => restored,
        Err(error) => return lifecycle_error(&error),
    };
    let entries: Vec<Value> = restored
        .into_iter()
        .map(|restored| match restored {
            Restored::Ready { prepared, state } => resume_one(lifecycle, client, &prepared, state),
            Restored::Suspended { request_id } => entry(&request_id, State::Suspended, None, None),
            Restored::Corrupt { request_id } => json!({
                "request_id": request_id, "state": "corrupt",
                "error": "the stored request does not match its digest",
            }),
        })
        .collect();
    json!({"ok": true, "entries": entries})
}
/// Wait until the replica's link has made its first round and `ready`
/// holds, at most `limit`.
async fn wait(source: &Source, limit: Duration, ready: impl Fn(&Source) -> bool) -> bool {
    use keeper_ports::LinkState;
    let started = Instant::now();
    loop {
        let link = source.replica.state().link;
        if matches!(link, LinkState::Connected) && ready(source) {
            return true;
        }
        if matches!(link, LinkState::Refused | LinkState::Closed) || started.elapsed() > limit {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// The reasons an admission refuses a worker for this host or this
/// invocation (`projection_unavailable: <reason>`): no worker given or
/// found, no or another digest, no or another ledger, a target, rootfs,
/// ELF, process or launch environment the runtime class does not admit.
/// A worker that was admitted and then failed is operational.
pub fn missing_prerequisite(reason: &str) -> bool {
    matches!(reason, "worker.path" | "worker.digest" | "worker.ledger")
        || [
            "ledger.",
            "platform.",
            "macos.",
            "elf.",
            "rootfs.",
            "process.",
            "launch.",
        ]
        .iter()
        .any(|class| reason.starts_with(class))
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::write::{Caller, GraphWrite, Writer};
    use keeper_ports::{CommandResult, ReplicaError};
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A connected server whose every answer about the request is
    /// `outcome_unresolved`; it counts the commands sent.
    struct Unresolved(AtomicU32);

    fn unresolved() -> ReplicaError {
        ReplicaError::Remote {
            code: "operation.forbidden".into(),
            message: "outcome_unresolved".into(),
            retryable: false,
        }
    }

    impl GraphWrite for Unresolved {
        fn command(&self, _: &str, _: &str, _: Value) -> Result<CommandResult, ReplicaError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(unresolved())
        }
        fn receipt(&self, _: &str) -> Result<Vec<serde_json::Map<String, Value>>, ReplicaError> {
            Ok(vec![])
        }
        fn remote_query(
            &self,
            _: &str,
            _: Value,
        ) -> Result<Vec<serde_json::Map<String, Value>>, ReplicaError> {
            Err(unresolved())
        }
        fn link(&self) -> LinkState {
            LinkState::Connected
        }
    }

    /// An entry the server cannot resolve is looked up, never sent
    /// again, whether it was submitting or already unresolved
    /// (112.1 r1 BLOCKING-3).
    #[test]
    fn resume_never_sends_what_the_server_cannot_resolve() {
        for (n, from) in [State::Submitting, State::Unresolved]
            .into_iter()
            .enumerate()
        {
            let port = Arc::new(Unresolved(AtomicU32::new(0)));
            let client = GraphClient::over(
                port.clone(),
                &Writer {
                    domain: "work".into(),
                    actor: "ana".into(),
                    client: "dev1".into(),
                },
                Caller::Person,
            );
            let change = client
                .change(
                    vec![change::ChangeOp::CreateNode {
                        id: "01JF0000000000000000000001".into(),
                        kind: "Concept".into(),
                        title: "resume".into(),
                        description: None,
                        properties: serde_json::Map::new(),
                    }],
                    None,
                )
                .expect("change");
            let prepared = client.mint(&change).expect("prepared");
            let dir =
                std::env::temp_dir().join(format!("kg1-driver-resume-{}-{n}", std::process::id()));
            let mut queue = GraphQueue::open(
                &dir.join("queue"),
                Binding {
                    server: "https://hq.example".into(),
                    domain: "work".into(),
                    actor: "ana".into(),
                    client: "dev1".into(),
                },
            )
            .expect("queue");
            let id = prepared.request().request_id.clone();
            queue.enqueue(&prepared, 1).expect("enqueue");
            queue
                .mark(&id, State::Queued, State::Submitting, None, 2)
                .expect("sent");
            if from == State::Unresolved {
                queue
                    .mark(&id, State::Submitting, State::Unresolved, None, 3)
                    .expect("unresolved");
            }
            let lifecycle = Lifecycle::open(queue).expect("lifecycle");
            let result = resume_one(&lifecycle, &client, &prepared, from);
            let sends = port.0.load(Ordering::SeqCst);
            drop(lifecycle);
            std::fs::remove_dir_all(&dir).expect("cleanup");
            assert_eq!(sends, 0, "{from:?}: {result}");
            assert_eq!(result["state"], json!("unresolved"), "{from:?}");
        }
    }

    /// A lifecycle over a fresh queue file in `dir`.
    fn fresh_lifecycle(dir: &std::path::Path) -> Lifecycle {
        let queue = GraphQueue::open(
            &dir.join("queue"),
            Binding {
                server: "https://hq.example".into(),
                domain: "work".into(),
                actor: "ana".into(),
                client: "dev1".into(),
            },
        )
        .expect("queue");
        Lifecycle::open(queue).expect("lifecycle")
    }

    /// H3 (owner decision 2026-11-12): the driver's signals set the
    /// fence the owner asked for, persisted. A refused link alone (a
    /// `401` after an attestation lapsed) is a credential fence; a
    /// revoke frame (a revoked subscription) a revocation fence; both
    /// at once a revocation; a revoke frame after a credential fence
    /// escalates it; a live link with nothing revoked sets none.
    #[tokio::test(flavor = "current_thread")]
    async fn the_signals_fence_from_their_cause() {
        for (n, (link, revoked, cause)) in [
            (LinkState::Refused, false, Some(FenceCause::Credential)),
            (LinkState::Connected, true, Some(FenceCause::Revocation)),
            (LinkState::Refused, true, Some(FenceCause::Revocation)),
            (LinkState::Connected, false, None),
        ]
        .into_iter()
        .enumerate()
        {
            let dir = scratch(&format!("case-{n}"));
            let lifecycle = fresh_lifecycle(&dir);
            assert_eq!(signal_cause(link, revoked), cause, "case {n}");
            assert!(fence_on(&lifecycle, link, revoked, None).await.is_none());
            assert_eq!(lifecycle.fence_cause(), Ok(cause), "case {n}");
            assert_eq!(lifecycle.is_locked(), cause.is_some(), "case {n}");
            // The cause survives a restart.
            drop(lifecycle);
            let reopened = fresh_lifecycle(&dir);
            assert_eq!(reopened.fence_cause(), Ok(cause), "case {n} reopened");
            drop(reopened);
            std::fs::remove_dir_all(&dir).expect("cleanup");
        }
        // A credential fence, then a revoke frame: escalated, never back.
        let dir = scratch("escalation");
        let lifecycle = fresh_lifecycle(&dir);
        assert!(fence_on(&lifecycle, LinkState::Refused, false, None)
            .await
            .is_none());
        assert_eq!(lifecycle.fence_cause(), Ok(Some(FenceCause::Credential)));
        assert!(fence_on(&lifecycle, LinkState::Connected, true, None)
            .await
            .is_none());
        assert_eq!(lifecycle.fence_cause(), Ok(Some(FenceCause::Revocation)));
        assert!(fence_on(&lifecycle, LinkState::Refused, false, None)
            .await
            .is_none());
        assert_eq!(lifecycle.fence_cause(), Ok(Some(FenceCause::Revocation)));
        drop(lifecycle);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// A new, empty directory for one test case.
    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("kg1-driver-signals-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        dir
    }

    /// A server's typed reads: one fixture row per id, an archived one
    /// answered only when the read includes archived entities.
    struct Reads {
        link: LinkState,
        rows: Vec<(&'static str, Value, bool)>,
        /// What every query fails with, when set.
        fail: Option<fn() -> ReplicaError>,
    }

    impl GraphWrite for Reads {
        fn command(&self, _: &str, _: &str, _: Value) -> Result<CommandResult, ReplicaError> {
            Err(unresolved())
        }
        fn receipt(&self, _: &str) -> Result<Vec<serde_json::Map<String, Value>>, ReplicaError> {
            Ok(vec![])
        }
        fn remote_query(
            &self,
            descriptor: &str,
            params: Value,
        ) -> Result<Vec<serde_json::Map<String, Value>>, ReplicaError> {
            if let Some(fail) = self.fail {
                return Err(fail());
            }
            let archived = params["include_archived"] == json!(true);
            Ok(self
                .rows
                .iter()
                .filter(|(d, row, is_archived)| {
                    *d == descriptor && row["id"] == params["id"] && (archived || !is_archived)
                })
                .filter_map(|(_, row, _)| row.as_object().cloned())
                .collect())
        }
        fn link(&self) -> LinkState {
            self.link
        }
    }

    const NODE: &str = "01JF0000000000000000000001";
    const OTHER: &str = "01JF0000000000000000000002";
    const EDGE: &str = "01JF0000000000000000000010";
    const CONTENT: &str = "01JF0000000000000000000020";
    const ARCHIVED: &str = "01JF0000000000000000000003";

    fn node(id: &str, lifecycle: &str) -> Value {
        json!({"id": id, "kind": "Concept", "title": "N", "description": null,
               "description_source_id": null, "properties": {}, "lifecycle": lifecycle,
               "merged_into_id": null, "revision": "1", "lifecycle_revision": "1",
               "tags": ["alpha"]})
    }

    fn reads_client(link: LinkState) -> GraphClient {
        reads_client_failing(link, None)
    }

    fn reads_client_failing(link: LinkState, fail: Option<fn() -> ReplicaError>) -> GraphClient {
        use crate::protocol::wire::{GET_CONTENT, GET_EDGE, GET_NODE};
        let port = Arc::new(Reads {
            link,
            fail,
            rows: vec![
                (GET_NODE, node(NODE, "active"), false),
                (GET_NODE, node(ARCHIVED, "archived"), true),
                (
                    GET_EDGE,
                    json!({"id": EDGE, "from_id": NODE, "to_id": OTHER, "type": "test:rel",
                           "properties": {}, "lifecycle": "active", "revision": "1"}),
                    false,
                ),
                (
                    GET_CONTENT,
                    json!({"id": CONTENT, "owner_node_id": NODE, "revision": "1",
                           "body": {"inline_text": "hello"}}),
                    false,
                ),
            ],
        });
        GraphClient::over(
            port,
            &Writer {
                domain: "work".into(),
                actor: "ana".into(),
                client: "dev1".into(),
            },
            Caller::Person,
        )
    }

    async fn typed(
        entity: InspectEntity,
        id: &str,
        archived: bool,
        stale: bool,
        link: LinkState,
    ) -> Reply {
        inspect_typed(
            reads_client(link),
            entity,
            id.into(),
            archived,
            stale,
            "work".into(),
        )
        .await
    }

    /// A-20 (AC10): without the worker, `graph.inspect` of a node, an edge
    /// and a content answers through the typed reads, rows shaped as the
    /// projection gives them; an archived one only with `include_archived`.
    #[tokio::test(flavor = "current_thread")]
    async fn inspect_without_the_worker_reads_the_typed_reads() {
        let up = LinkState::Connected;
        let answer = typed(InspectEntity::Node, NODE, false, false, up)
            .await
            .expect("node");
        let mut image = node(NODE, "active");
        let tags = image
            .as_object_mut()
            .and_then(|m| m.remove("tags"))
            .expect("tags");
        assert_eq!(
            answer,
            json!({"ok": true, "rows": {"image": image, "tags": tags}, "domain": "work",
                   "source": "typed_reads"})
        );
        let edge = typed(InspectEntity::Edge, EDGE, false, false, up)
            .await
            .expect("edge");
        assert_eq!(edge["rows"]["type"], json!("test:rel"));
        let content = typed(InspectEntity::Content, CONTENT, false, false, up)
            .await
            .expect("content");
        assert_eq!(content["rows"]["body"], json!({"inline_text": "hello"}));
        let hidden = typed(InspectEntity::Node, ARCHIVED, false, false, up)
            .await
            .expect("hidden");
        assert_eq!(hidden["rows"], Value::Null);
        let shown = typed(InspectEntity::Node, ARCHIVED, true, false, up)
            .await
            .expect("shown");
        assert_eq!(shown["rows"]["image"]["lifecycle"], json!("archived"));
    }

    /// AC10: with the link down the typed read is `UNAVAILABLE`, retryable;
    /// a stale read is `INVALID_ARGUMENT` (it needs the projection).
    #[tokio::test(flavor = "current_thread")]
    async fn inspect_without_the_worker_refuses_offline_and_stale() {
        let down = typed(InspectEntity::Node, NODE, false, false, LinkState::Offline)
            .await
            .expect_err("offline");
        assert_eq!(down.code, ErrorCode::Unavailable);
        assert!(down.retryable);
        let stale = typed(InspectEntity::Node, NODE, false, true, LinkState::Connected)
            .await
            .expect_err("stale");
        assert_eq!(stale.code, ErrorCode::InvalidArgument);
        assert_eq!(stale.reason, "allow_stale");
    }

    /// AC10/A-20: a freshly opened replica is `Connecting`, which `online`
    /// admits; when its query is unheard (the server was not reached), or the
    /// replica does not sync or is closed, the typed read is `UNAVAILABLE`
    /// `offline`, never `REFUSED`; retryable unless the client says otherwise.
    #[tokio::test(flavor = "current_thread")]
    async fn inspect_without_the_worker_is_unavailable_while_the_server_is_unreachable() {
        fn unheard() -> ReplicaError {
            ReplicaError::Unheard {
                code: "transport.failed".into(),
                message: "connection refused".into(),
                retryable: true,
            }
        }
        fn unreadable() -> ReplicaError {
            ReplicaError::Unheard {
                code: "transport.failed".into(),
                message: "unreadable".into(),
                retryable: false,
            }
        }
        let cases: [(fn() -> ReplicaError, bool); 4] = [
            (unheard, true),
            (|| ReplicaError::NotSyncing, true),
            (|| ReplicaError::Closed, true),
            (unreadable, false),
        ];
        for (fail, retryable) in cases {
            let client = reads_client_failing(LinkState::Connecting, Some(fail));
            let error = inspect_typed(
                client,
                InspectEntity::Node,
                NODE.into(),
                false,
                false,
                "work".into(),
            )
            .await
            .expect_err("unreachable");
            assert_eq!(error.code, ErrorCode::Unavailable, "{}", fail());
            assert_eq!(error.reason, "offline", "{}", fail());
            assert_eq!(error.retryable, retryable, "{}", fail());
        }
    }

    fn module_over(state: &std::path::Path, source: Result<Source, OpError>) -> GraphModule {
        GraphModule::new(
            GraphConfig {
                state: state.to_path_buf(),
                worker: None,
                macos_evidence: false,
                worker_sha256: None,
                ledger: None,
            },
            vec![DomainBinding {
                server: "https://hq.example".into(),
                database: "work".into(),
                actor: "ana".into(),
                client: "dev1".into(),
                source,
            }],
        )
    }

    fn work() -> Value {
        json!({"server": "https://hq.example", "domain": "work"})
    }

    /// The graph module behind the real runtime's dispatcher.
    fn dispatcher(module: GraphModule) -> keeper_runtime::LocalDispatcher {
        let info = keeper_runtime::AppInfo {
            version: "test".into(),
            build: "test".into(),
            snapshot_digest: String::new(),
            domains: Vec::new(),
        };
        let (_, dispatcher) =
            keeper_runtime::Runtime::build(info, vec![Arc::new(module)]).expect("starts");
        dispatcher
    }

    /// AC10: on a build where no worker is admitted, `graph.query` answers
    /// `UNAVAILABLE` with `projection_unavailable`, and says so in
    /// `unavailable`; `render` gives the command line's old line back.
    #[tokio::test(flavor = "current_thread")]
    async fn query_without_the_worker_is_unavailable() {
        let dir = scratch("query-unavailable");
        let replica = keeper_replica::SyncularReplica::open(keeper_ports::ReplicaSpec {
            database: "work".to_owned(),
            path: dir.join("work.sqlite"),
            client_id: Some("dev1".to_owned()),
            link: None,
            units: Vec::new(),
            watch_presence: None,
        })
        .expect("opens");
        let source = Source {
            database: "work".into(),
            replica: Arc::new(replica),
            actor: "ana".into(),
            client: "dev1".into(),
        };
        let module = module_over(&dir, Ok(source));
        assert!(module.unavailable("graph.query").is_some());
        assert!(module.unavailable("graph.inspect").is_none());
        let dispatcher = dispatcher(module);
        let answer = dispatcher
            .call(
                "graph.query",
                json!({"target": work(), "source": "MATCH (n) RETURN n"}),
            )
            .await;
        let error = answer.clone().expect_err("unavailable");
        assert_eq!(error.code, ErrorCode::Unavailable);
        assert_eq!(error.reason, "projection_unavailable");
        assert_eq!(
            render(&answer).to_string(),
            fail("projection_unavailable", &error.detail).to_string()
        );
        drop(dispatcher);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// The replica the root could not open answers first, as the command
    /// line always answered it; an unknown domain is `NOT_FOUND`.
    #[tokio::test(flavor = "current_thread")]
    async fn the_roots_refusal_answers_first() {
        let dir = scratch("root-refusal");
        let refusal = OpError::new(
            ErrorCode::Refused,
            "unauthorised",
            "KEEPER_GRAPH_TOKEN is required",
        );
        let dispatcher = dispatcher(module_over(&dir, Err(refusal.clone())));
        let answer = dispatcher
            .call("graph.status", json!({"target": work()}))
            .await;
        assert_eq!(answer, Err(refusal));
        let other = dispatcher
            .call(
                "graph.status",
                json!({"target": {"server": "https://hq.example", "domain": "home"}}),
            )
            .await
            .expect_err("unknown domain");
        assert_eq!(other.code, ErrorCode::NotFound);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
