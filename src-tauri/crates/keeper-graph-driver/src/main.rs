//! `keeper-graph-driver`: a person's device's graph without a window — the
//! headless app-human consumer of the safe graph API against a real
//! keeper-hq (E1). Each run opens the device's persistent on-disk replica of
//! one database and does one thing, printing one JSON line:
//!
//! - `apply <payload.json>` — a `graph.apply` change (the payload as the
//!   contract writes it) and its outcome, online only;
//! - `enqueue <payload.json>` — persist the change in the device's intent
//!   queue (offline too) and print its request id; nothing is sent;
//! - `pending` — the queue's entries: ids, state, cause and age only;
//! - `resume` — restore every open entry from its stored bytes, resolve it
//!   from its receipt (here or `graph.outcome`) and send the identical
//!   request under its own id only when no receipt exists;
//! - `discard <request_id>` — withdraw an entry never sent, suspended or
//!   locked; it is never sent again;
//! - `sync` — wait until the replica holds the domain's graph, then build
//!   or extend the local query projection; a revoked domain is cleaned up
//!   here, and unlocked only by fresh authority;
//! - `query <source> [--params <json>] [--allow-stale]` — one
//!   `KG-Cypher/1` query over the projection (with `--allow-stale`, served
//!   `stale` from the last-authorised generation while the server cannot be
//!   reached, at most a day after the last successful authority read);
//! - `inspect node|edge|content <id> [--include-archived]` — the typed
//!   lookup over the projection (a content only while its owner is active,
//!   unless archived ones are included);
//! - `history [--after <n>] [--limit <n>]` — one page of retained history;
//! - `unreviewed [--after <n>] [--limit <n>]` — one page of the unreviewed
//!   feed (a reviewer's);
//! - `propose <payload.json>` / `decide <payload.json>` — a `graph.propose`
//!   or `graph.decide` payload and its outcome;
//! - `proposal <proposal_id>` — the proposal's revisions as its enumerated
//!   reviewer reads them;
//! - `acknowledge <event_id> <delta_digest>` — a human review of one event;
//! - `preview-reversal <event_id>` — the inverse and what it requires now;
//! - `reverse <event_id> [--approval-id <id>]` — preview, then reverse under
//!   exactly the previewed revisions (refused without a send unless the
//!   preview is ready, or `approval_required` and an approval id is given:
//!   an agent's per-call approval);
//! - `status` — the replica's head and checkpoint, the projection's runtime
//!   admission, the revocation fence and the queue's counts.
//!
//! The queue, its revocation fence and the last read authority persist in
//! `<state>/<database>.graph-queue`, beside the replica. A revocation (a
//! refused link, a revoked subscription, a server refusal) locks the domain
//! before anything else: reads, history and the queue refuse until `sync`
//! sees the replica's purge and fresh authority.
//!
//! The device token is read from `KEEPER_GRAPH_TOKEN`, never from argv, and
//! never printed. The worker is `--worker <absolute path>`, admitted only
//! with the digest (`KEEPER_GRAPH_WORKER_SHA256`) and, on Linux, the runtime
//! ledger (`KEEPER_GRAPH_RUNTIME_LEDGER`) embedded at build time; macOS runs
//! a worker only with `--macos-evidence` (evidence only, OD-C).

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod driver {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use clap::{Parser, Subcommand};
    use keeper_core::graph::admission::{Admission, Process};
    use keeper_core::graph::client::{
        subscriptions, Applied, GraphClient, GraphClientError, Prepared,
    };
    use keeper_core::graph::lifecycle::{Lifecycle, LifecycleError};
    use keeper_core::graph::outcome::GraphOutcome;
    use keeper_core::graph::protocol::change::Change;
    use keeper_core::graph::protocol::inverse::{PreviewOutcome, Reverse};
    use keeper_core::graph::protocol::review::Target;
    use keeper_core::graph::protocol::wire::HistoryFilter;
    use keeper_core::graph::protocol::worker::InspectEntity;
    use keeper_core::graph::protocol::{change, proposal};
    use keeper_core::graph::query::{
        Freshness, GraphReads, GraphRemote, HumanReader, Inspected, QueryError, QueryResult,
    };
    use keeper_core::graph::queue::{self, Binding, GraphQueue, Restored, State};
    use keeper_core::graph::runtime::{Runtime, VerifiedWorker};
    use keeper_core::graph::source::{GraphSource, ScopeState};
    use keeper_core::graph::supervisor::Cancellation;
    use keeper_core::hq::port::{Link, LinkState, ReplicaSpec};
    use keeper_core::hq::server::ServerUrl;
    use keeper_core::hq::sessions::Source;
    use keeper_core::hq::syncular::SyncularReplica;
    use keeper_core::notes::tags::normalise;
    use serde_json::{json, Value};

    mod embedded {
        include!(concat!(env!("OUT_DIR"), "/ledger.rs"));
    }

    #[derive(Parser)]
    #[command(about = "keeper's headless graph consumer")]
    struct Args {
        /// Where the replica and the projection persist.
        #[arg(long)]
        state: PathBuf,
        /// keeper-hq's base URL (`<url>/<database>` is the mount).
        #[arg(long)]
        url: String,
        #[arg(long)]
        database: String,
        /// The person's actor id.
        #[arg(long)]
        actor: String,
        /// This device's client id.
        #[arg(long)]
        client: String,
        /// The graph worker executable (absolute path).
        #[arg(long)]
        worker: Option<PathBuf>,
        /// macOS only: run the worker under the evidence-only sandbox.
        #[arg(long)]
        macos_evidence: bool,
        #[command(subcommand)]
        command: Command,
    }

    #[derive(Subcommand)]
    enum Command {
        Apply {
            payload: PathBuf,
        },
        Enqueue {
            payload: PathBuf,
        },
        Pending,
        Resume,
        Discard {
            request_id: String,
        },
        Sync {
            #[arg(long, default_value_t = 60)]
            wait_secs: u64,
        },
        Query {
            source: String,
            #[arg(long, default_value = "{}")]
            params: String,
            #[arg(long)]
            allow_stale: bool,
        },
        Inspect {
            entity: String,
            id: String,
            #[arg(long)]
            include_archived: bool,
            #[arg(long)]
            allow_stale: bool,
        },
        History {
            #[arg(long, default_value = "0")]
            after: String,
            #[arg(long, default_value_t = 100)]
            limit: u32,
        },
        Unreviewed {
            #[arg(long, default_value = "0")]
            after: String,
            #[arg(long, default_value_t = 100)]
            limit: u32,
        },
        Propose {
            payload: PathBuf,
        },
        Decide {
            payload: PathBuf,
        },
        Proposal {
            proposal_id: String,
        },
        Acknowledge {
            event_id: String,
            delta_digest: String,
        },
        PreviewReversal {
            event_id: String,
        },
        Reverse {
            event_id: String,
            /// The per-call approval an agent's reversal needs when the
            /// preview answers `approval_required`.
            #[arg(long)]
            approval_id: Option<String>,
        },
        Status,
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
            other => json!({"outcome": "pending", "state": format!("{other:?}")}),
        }
    }

    /// A command's answer: its request id, outcome and the stable code a
    /// caller reports for it.
    fn applied(answer: Result<Applied, GraphClientError>) -> Value {
        match answer {
            Ok(applied) => json!({
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
            }),
            Err(e) => fail("refused", &e.to_string()),
        }
    }

    fn payload(path: &PathBuf) -> Result<Value, Value> {
        let text = std::fs::read(path).map_err(|e| fail("invalid_change", &e.to_string()))?;
        keeper_core::graph::protocol::value::parse(&text).map_err(|r| fail(r.code(), "payload"))
    }

    /// Run `call` on a blocking thread once the replica's first round is
    /// done, and only while the domain is not fenced: a revocation signal
    /// seen now locks the domain first, and a locked domain is never sent to
    /// or read from.
    async fn blocking(
        source: &Source,
        lifecycle: &Lifecycle,
        call: impl FnOnce(GraphClient) -> Value + Send + 'static,
    ) -> Value {
        let client = GraphClient::new(source.clone());
        wait(source, Duration::from_secs(30), |_| true).await;
        if let Some(error) = signals(lifecycle, source, None).await {
            return error;
        }
        if let Err(e) = lifecycle.gate() {
            return query_error(&e);
        }
        tokio::task::spawn_blocking(move || call(client))
            .await
            .unwrap_or_else(|e| fail("storage_unavailable", &e.to_string()))
    }

    fn result<T>(r: &QueryResult<T>, rows: Value) -> Value {
        json!({
            "ok": true, "rows": rows, "domain": r.domain, "graph_seq": r.graph_seq,
            "checkpoint": r.checkpoint, "contract_schema": r.contract_schema,
            "projection_version": r.projection_version, "engine": r.engine,
            "completeness": format!("{:?}", r.completeness).to_lowercase(),
        })
    }

    fn open(args: &Args) -> Result<Source, Value> {
        let token = std::env::var("KEEPER_GRAPH_TOKEN")
            .map_err(|_| fail("unauthorised", "KEEPER_GRAPH_TOKEN is required"))?;
        std::fs::create_dir_all(&args.state)
            .map_err(|e| fail("storage_unavailable", &e.to_string()))?;
        let replica = SyncularReplica::open(ReplicaSpec {
            database: args.database.clone(),
            path: args.state.join(format!("{}.sqlite", args.database)),
            client_id: Some(args.client.clone()),
            link: Some(Link {
                base_url: format!("{}/{}", args.url.trim_end_matches('/'), args.database),
                token,
            }),
            subscriptions: subscriptions(&args.database, &args.actor, &args.client),
            watch_presence: None,
        })
        .map_err(|e| fail("storage_unavailable", &e.to_string()))?;
        Ok(Source {
            database: args.database.clone(),
            replica: Arc::new(replica),
            actor: args.actor.clone(),
            client: args.client.clone(),
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
        let worker = VerifiedWorker::open(path, option_env!("KEEPER_GRAPH_WORKER_SHA256"))?;
        #[cfg(target_os = "macos")]
        if args.macos_evidence {
            return Runtime::macos_evidence(worker).map(Arc::new);
        }
        Runtime::admit(worker, embedded::LEDGER).map(Arc::new)
    }

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
    /// refused link (the device was de-authorised) or a revoked graph
    /// subscription. Either fences the domain (`reads` purged when held);
    /// a fence that could not be completed is the run's answer (the domain
    /// stays locked in this process).
    async fn signals(
        lifecycle: &Lifecycle,
        source: &Source,
        reads: Option<&GraphReads>,
    ) -> Option<Value> {
        let refused = source.replica.state().link == LinkState::Refused;
        let revoked = source
            .checkpoint()
            .is_ok_and(|c| c.scope == ScopeState::Revoked);
        if !(refused || revoked) {
            return None;
        }
        lifecycle
            .revoke(reads, now_ms())
            .await
            .err()
            .map(|error| lifecycle_error(&error))
    }

    fn parse_change(payload: &PathBuf) -> Result<Change, Value> {
        let text = std::fs::read(payload).map_err(|e| fail("invalid_change", &e.to_string()))?;
        keeper_core::graph::protocol::value::parse(&text)
            .and_then(|v| change::parse(&v, normalise))
            .map_err(|refusal| fail(refusal.code(), "payload"))
    }

    /// A fenced domain at `sync`: finish the cleanup, record the replica's
    /// purge once seen, and unlock only with fresh authority (a successful
    /// `read_status`, granted subscriptions and a new projection instance).
    /// The reads to go on with, or why the domain stays locked.
    async fn unfence(
        args: &Args,
        source: &Source,
        lifecycle: &Lifecycle,
    ) -> Result<GraphReads, Value> {
        let held = reads(args, source).map_err(|e| query_error(&e))?;
        lifecycle
            .resume(Some(&held), now_ms())
            .await
            .map_err(|e| lifecycle_error(&e))?;
        let checkpoint = source
            .checkpoint()
            .map_err(|e| fail("storage_unavailable", &e.to_string()))?;
        lifecycle
            .observe_purge(checkpoint, now_ms())
            .map_err(|e| lifecycle_error(&e))?;
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
    /// identical request under its own id only when there is none.
    fn resume_one(
        lifecycle: &Lifecycle,
        client: &GraphClient,
        prepared: &Prepared,
        mut state: State,
    ) -> Value {
        let id = prepared.request().request_id.clone();
        if let Some(done) = settle(lifecycle, &id, &mut state, &client.outcome(prepared, None)) {
            return done;
        }
        if state == State::PendingVisibility {
            // Applied, its receipt not visible yet: never sent again.
            return entry(&id, state, None);
        }
        let moved = match state {
            State::Submitting => lifecycle.with_queue(|q| q.retry(&id, "resent_same_id", now_ms())),
            from => lifecycle
                .with_queue(|q| q.mark(&id, from, State::Submitting, Some("sent"), now_ms())),
        };
        if let Err(error) = moved {
            return entry(&id, state, Some(&error.to_string()));
        }
        state = State::Submitting;
        let attempt = match client.send(prepared) {
            Ok(attempt) => attempt,
            Err(error) => return entry(&id, state, Some(&error.to_string())),
        };
        let outcome = client.outcome(prepared, Some(&attempt));
        settle(lifecycle, &id, &mut state, &outcome).unwrap_or_else(|| entry(&id, state, None))
    }

    /// Persist what `outcome` concludes, if anything: `Some` when the entry
    /// is resolved, its receipt is not visible yet, or the outcome could
    /// not be read (nothing is sent then); `None` while it stays pending.
    fn settle(
        lifecycle: &Lifecycle,
        id: &str,
        state: &mut State,
        outcome: &Result<GraphOutcome, GraphClientError>,
    ) -> Option<Value> {
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) => return Some(entry(id, *state, Some(&error.to_string()))),
        };
        if let Some((to, resolution)) = queue::terminal(outcome) {
            return Some(
                match lifecycle.with_queue(|q| q.resolve(id, *state, to, &resolution, now_ms())) {
                    Ok(()) => entry(id, to, None),
                    Err(error) => entry(id, *state, Some(&error.to_string())),
                },
            );
        }
        if matches!(outcome, GraphOutcome::PendingVisibility { .. })
            && *state != State::PendingVisibility
        {
            let from = *state;
            return Some(
                match lifecycle
                    .with_queue(|q| q.mark(id, from, State::PendingVisibility, None, now_ms()))
                {
                    Ok(()) => {
                        *state = State::PendingVisibility;
                        entry(id, *state, None)
                    }
                    Err(error) => entry(id, from, Some(&error.to_string())),
                },
            );
        }
        None
    }

    fn entry(id: &str, state: State, error: Option<&str>) -> Value {
        json!({"request_id": id, "state": state.as_str(), "error": error})
    }

    fn resume(lifecycle: &Lifecycle, client: &GraphClient) -> Value {
        let restored = match lifecycle.with_queue(|q| q.restore(client, now_ms())) {
            Ok(restored) => restored,
            Err(error) => return lifecycle_error(&error),
        };
        let entries: Vec<Value> = restored
            .into_iter()
            .map(|restored| match restored {
                Restored::Ready { prepared, state } => {
                    resume_one(lifecycle, client, &prepared, state)
                }
                Restored::Suspended { request_id } => entry(&request_id, State::Suspended, None),
                Restored::Corrupt { request_id } => json!({
                    "request_id": request_id, "state": "corrupt",
                    "error": "the stored request does not match its digest",
                }),
            })
            .collect();
        json!({"ok": true, "entries": entries})
    }

    async fn run(args: Args) -> Value {
        let source = match open(&args) {
            Ok(source) => source,
            Err(error) => return error,
        };
        let lifecycle = match lifecycle(&args) {
            Ok(lifecycle) => lifecycle,
            Err(error) => return error,
        };
        let cancel = Cancellation::default();
        match &args.command {
            Command::Apply { payload } => {
                let change = match parse_change(payload) {
                    Ok(change) => change,
                    Err(error) => return error,
                };
                blocking(&source, &lifecycle, move |client| {
                    applied(client.apply(&change))
                })
                .await
            }
            Command::Enqueue { payload } => {
                let change = match parse_change(payload) {
                    Ok(change) => change,
                    Err(error) => return error,
                };
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
            Command::Pending => {
                if let Some(error) = signals(&lifecycle, &source, None).await {
                    return error;
                }
                let now = now_ms();
                match lifecycle.with_queue(|q| q.entries()) {
                    Ok(entries) => json!({
                        "ok": true,
                        "entries": entries.iter().map(|e| json!({
                            "request_id": e.request_id, "state": e.state.as_str(),
                            "attempts": e.attempts, "cause": e.last_cause,
                            "age_ms": now.saturating_sub(e.enqueued_at_ms),
                        })).collect::<Vec<_>>(),
                    }),
                    Err(e) => lifecycle_error(&e),
                }
            }
            Command::Resume => {
                wait(&source, Duration::from_secs(30), |_| true).await;
                if let Some(error) = signals(&lifecycle, &source, None).await {
                    return error;
                }
                let client = GraphClient::new(source.clone());
                let held = Arc::clone(&lifecycle);
                match tokio::task::spawn_blocking(move || resume(&held, &client)).await {
                    Ok(answer) => answer,
                    Err(e) => fail("storage_unavailable", &e.to_string()),
                }
            }
            Command::Discard { request_id } => {
                if let Some(error) = signals(&lifecycle, &source, None).await {
                    return error;
                }
                match lifecycle.with_queue(|q| q.discard(request_id, now_ms())) {
                    Ok(()) => json!({"ok": true, "request_id": request_id,
                                     "state": State::Withdrawn.as_str()}),
                    Err(e) => lifecycle_error(&e),
                }
            }
            Command::Sync { wait_secs } => {
                let ready = wait(&source, Duration::from_secs(*wait_secs), |s| {
                    s.checkpoint().is_ok_and(|c| c.scope != ScopeState::Pending)
                })
                .await;
                if let Some(error) = signals(&lifecycle, &source, None).await {
                    return error;
                }
                if !ready && !lifecycle.is_locked() {
                    return fail(
                        "sync_pending",
                        "the graph subscriptions have not pulled yet",
                    );
                }
                let reads = if lifecycle.is_locked() {
                    match unfence(&args, &source, &lifecycle).await {
                        Ok(reads) => reads,
                        Err(error) => return error,
                    }
                } else {
                    match reads(&args, &source) {
                        Ok(reads) => reads,
                        Err(e) => return query_error(&e),
                    }
                };
                match reader(&reads, &source, &lifecycle) {
                    Ok(reader) => match reader.refresh(&cancel).await {
                        Ok(r) => json!({"ok": true, "graph_seq": r.graph_seq, "built": r.built,
                                        "completeness": format!("{:?}", r.completeness).to_lowercase()}),
                        Err(e) => query_error(&e),
                    },
                    Err(e) => query_error(&e),
                }
            }
            Command::Query {
                source: text,
                params,
                allow_stale,
            } => {
                let reads = match reads(&args, &source) {
                    Ok(reads) => reads,
                    Err(e) => return query_error(&e),
                };
                if let Some(error) = signals(&lifecycle, &source, Some(&reads)).await {
                    return error;
                }
                match reader(&reads, &source, &lifecycle) {
                    Ok(reader) => {
                        let freshness = if *allow_stale {
                            Freshness::AllowStale
                        } else {
                            Freshness::Current
                        };
                        match reader
                            .query(text, params.as_bytes(), freshness, &cancel)
                            .await
                        {
                            Ok(r) => result(&r, json!(r.rows)),
                            Err(e) => query_error(&e),
                        }
                    }
                    Err(e) => query_error(&e),
                }
            }
            Command::Inspect {
                entity,
                id,
                include_archived,
                allow_stale,
            } => {
                let entity = match entity.as_str() {
                    "node" => InspectEntity::Node,
                    "edge" => InspectEntity::Edge,
                    "content" => InspectEntity::Content,
                    _ => return fail("query_unsupported", "entity is node, edge or content"),
                };
                let freshness = if *allow_stale {
                    Freshness::AllowStale
                } else {
                    Freshness::Current
                };
                let reads = match reads(&args, &source) {
                    Ok(reads) => reads,
                    Err(e) => return query_error(&e),
                };
                if let Some(error) = signals(&lifecycle, &source, Some(&reads)).await {
                    return error;
                }
                match reader(&reads, &source, &lifecycle) {
                    Ok(reader) => match reader
                        .inspect(entity, id, *include_archived, freshness, &cancel)
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
            Command::History { after, limit } => {
                let client = GraphClient::new(source.clone());
                wait(&source, Duration::from_secs(30), |_| true).await;
                if let Some(error) = signals(&lifecycle, &source, None).await {
                    return error;
                }
                if let Err(e) = lifecycle.gate() {
                    return query_error(&e);
                }
                let (after, limit) = (after.clone(), *limit);
                match tokio::task::spawn_blocking(move || {
                    client.history(&after, limit, &HistoryFilter::default())
                })
                .await
                {
                    Ok(Ok(page)) => json!({"ok": true, "page": page}),
                    Ok(Err(e)) => fail("refused", &e.to_string()),
                    Err(e) => fail("storage_unavailable", &e.to_string()),
                }
            }
            Command::Unreviewed { after, limit } => {
                let (after, limit) = (after.clone(), *limit);
                blocking(&source, &lifecycle, move |client| {
                    match client.unreviewed(&after, limit) {
                        Ok(page) => json!({"ok": true, "page": page}),
                        Err(e) => fail("refused", &e.to_string()),
                    }
                })
                .await
            }
            Command::Propose { payload: path } => {
                let proposal = match payload(path).and_then(|v| {
                    proposal::parse(&v, normalise).map_err(|r| fail(r.code(), "payload"))
                }) {
                    Ok(p) => p,
                    Err(e) => return e,
                };
                blocking(&source, &lifecycle, move |client| {
                    applied(client.propose(&proposal))
                })
                .await
            }
            Command::Decide { payload: path } => {
                let decision = match payload(path)
                    .and_then(|v| proposal::parse_decide(&v).map_err(|r| fail(r.code(), "payload")))
                {
                    Ok(d) => d,
                    Err(e) => return e,
                };
                blocking(&source, &lifecycle, move |client| {
                    applied(client.decide(&decision))
                })
                .await
            }
            Command::Proposal { proposal_id } => {
                let id = proposal_id.clone();
                blocking(&source, &lifecycle, move |client| {
                    match client.proposal(&id) {
                        Ok(rows) => json!({"ok": true, "revisions": rows}),
                        Err(e) => fail("refused", &e.to_string()),
                    }
                })
                .await
            }
            Command::Acknowledge {
                event_id,
                delta_digest,
            } => {
                let target = match Target::new(event_id, delta_digest) {
                    Ok(t) => t,
                    Err(r) => return fail(r.code(), "target"),
                };
                blocking(&source, &lifecycle, move |client| {
                    applied(client.acknowledge(&target))
                })
                .await
            }
            Command::PreviewReversal { event_id } => {
                let id = event_id.clone();
                blocking(&source, &lifecycle, move |client| {
                    match client.preview_reversal(&id) {
                        Ok(preview) => json!({"ok": true, "preview": preview}),
                        Err(e) => fail("refused", &e.to_string()),
                    }
                })
                .await
            }
            Command::Reverse {
                event_id,
                approval_id,
            } => {
                let id = event_id.clone();
                let approval_id = approval_id.clone();
                blocking(&source, &lifecycle, move |client| {
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
                            Ok(reverse) => applied(client.reverse(&reverse)),
                            Err(r) => fail(r.code(), "reverse"),
                        },
                        None if preview.outcome == PreviewOutcome::ApprovalRequired => {
                            json!({"ok": false, "error": "approval_required", "preview": preview})
                        }
                        None => json!({"ok": false, "error": "not_ready", "preview": preview}),
                    }
                })
                .await
            }
            Command::Status => {
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
                json!({
                    "ok": true,
                    "head": head.ok().flatten(),
                    "checkpoint": checkpoint.ok().map(|c| json!({"scope": format!("{:?}", c.scope).to_lowercase(), "cursor": c.cursor})),
                    "runtime": match runtime { Ok(()) => json!("admitted"), Err(e) => json!(e) },
                    "fence": fence.ok().flatten(),
                    "queue": counts.ok(),
                })
            }
        }
    }

    /// Wait until the replica's link has made its first round and `ready`
    /// holds, at most `limit`.
    async fn wait(source: &Source, limit: Duration, ready: impl Fn(&Source) -> bool) -> bool {
        use keeper_core::hq::port::LinkState;
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

    pub fn main() -> std::process::ExitCode {
        let args = Args::parse();
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(_) => return std::process::ExitCode::FAILURE,
        };
        let answer = runtime.block_on(run(args));
        let ok = answer["ok"] == true;
        println!("{answer}");
        if ok {
            std::process::ExitCode::SUCCESS
        } else {
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn main() -> std::process::ExitCode {
    driver::main()
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn main() {
    std::process::exit(2);
}
