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
//!
//! `sync`, `query` and `inspect` are interruptible: their first SIGINT or
//! SIGTERM fires the run's cancellation instead of ending the process, so a
//! running worker is killed and reaped by the cancellation path before the
//! driver answers `query_cancelled` and exits with 128 + the signal's
//! number (130, 143). Later signals are taken and ignored. SIGKILL cannot be
//! taken: it still ends the driver without its worker, which then runs to
//! its own end (no safe parent-death signal for it; a known limitation).

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod driver {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicI32, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use clap::{Parser, Subcommand};
    use keeper_core::graph::admission::{Admission, Process};
    use keeper_core::graph::client::{
        subscribed_tables, subscriptions, Applied, GraphClient, GraphClientError, Prepared,
    };
    use keeper_core::graph::lifecycle::{Lifecycle, LifecycleError};
    use keeper_core::graph::outcome::{GraphOutcome, PendingCause};
    use keeper_core::graph::protocol::change::Change;
    use keeper_core::graph::protocol::inverse::{PreviewOutcome, Reverse};
    use keeper_core::graph::protocol::review::Target;
    use keeper_core::graph::protocol::wire::HistoryFilter;
    use keeper_core::graph::protocol::worker::InspectEntity;
    use keeper_core::graph::protocol::{change, proposal};
    use keeper_core::graph::query::{
        Freshness, GraphReads, GraphRemote, HumanReader, Inspected, QueryError, QueryResult,
    };
    use keeper_core::graph::queue::{
        resume_sends, Binding, FenceCause, GraphQueue, Restored, State,
    };
    use keeper_core::graph::runtime::{Runtime, VerifiedWorker};
    use keeper_core::graph::source::{revocation_with, GraphSource, Revocation, ScopeState};
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
        lifecycle: &Arc<Lifecycle>,
        call: impl FnOnce(GraphClient, &Lifecycle) -> Value + Send + 'static,
    ) -> Value {
        let client = GraphClient::new(source.clone());
        // These commands are not interruptible: nothing fires this.
        wait(
            source,
            Duration::from_secs(30),
            &Cancellation::default(),
            |_| true,
        )
        .await;
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

    fn parse_change(payload: &PathBuf) -> Result<Change, Value> {
        let text = std::fs::read(payload).map_err(|e| fail("invalid_change", &e.to_string()))?;
        keeper_core::graph::protocol::value::parse(&text)
            .and_then(|v| change::parse(&v, normalise))
            .map_err(|refusal| fail(refusal.code(), "payload"))
    }

    /// A fenced domain at `sync`: finish the cleanup, record the replica's
    /// purge once seen, and unlock only with fresh authority (a successful
    /// `read_status`, granted subscriptions and a new projection instance).
    /// A credential fence needs no purge: the purge not yet observed blocks
    /// only a revocation fence ([`Lifecycle::regrant`]).
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
            from => lifecycle
                .with_queue(|q| q.mark(&id, from, State::Submitting, Some("sent"), now_ms())),
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
                Restored::Ready { prepared, state } => {
                    resume_one(lifecycle, client, &prepared, state)
                }
                Restored::Suspended { request_id } => {
                    entry(&request_id, State::Suspended, None, None)
                }
                Restored::Corrupt { request_id } => json!({
                    "request_id": request_id, "state": "corrupt",
                    "error": "the stored request does not match its digest",
                }),
            })
            .collect();
        json!({"ok": true, "entries": entries})
    }

    /// SIGINT and SIGTERM, as numbered on Linux and macOS alike.
    const SIGINT: i32 = 2;
    const SIGTERM: i32 = 15;

    /// The signal that interrupted this run, 0 when none did.
    static INTERRUPTED: AtomicI32 = AtomicI32::new(0);

    /// From now on SIGINT and SIGTERM no longer end the process: the first
    /// of them fires `cancel`, whose run kills and reaps its worker. A
    /// signal's default action would end the driver with the worker still
    /// running (`kill_on_drop` acts only on a drop), orphaning it.
    fn cancel_on_interrupt(cancel: &Cancellation) -> std::io::Result<()> {
        use tokio::signal::unix::{signal, SignalKind};
        let mut interrupt = signal(SignalKind::interrupt())?;
        let mut terminate = signal(SignalKind::terminate())?;
        let cancel = cancel.clone();
        tokio::spawn(async move {
            let number = tokio::select! {
                _ = interrupt.recv() => SIGINT,
                _ = terminate.recv() => SIGTERM,
            };
            INTERRUPTED.store(number, Ordering::SeqCst);
            cancel.cancel();
        });
        Ok(())
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
        if matches!(
            args.command,
            Command::Sync { .. } | Command::Query { .. } | Command::Inspect { .. }
        ) {
            if let Err(error) = cancel_on_interrupt(&cancel) {
                return fail("projection_unavailable", &format!("signals: {error}"));
            }
        }
        match &args.command {
            Command::Apply { payload } => {
                let change = match parse_change(payload) {
                    Ok(change) => change,
                    Err(error) => return error,
                };
                blocking(&source, &lifecycle, move |client, lifecycle| {
                    applied(lifecycle, client.apply(&change))
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
                wait(&source, Duration::from_secs(30), &cancel, |_| true).await;
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
                let ready = wait(&source, Duration::from_secs(*wait_secs), &cancel, |s| {
                    s.checkpoint().is_ok_and(|c| c.scope != ScopeState::Pending)
                })
                .await;
                if cancel.is_cancelled() {
                    return query_error(&QueryError::Cancelled);
                }
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
                wait(&source, Duration::from_secs(30), &cancel, |_| true).await;
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
                blocking(&source, &lifecycle, move |client, _| {
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
                blocking(&source, &lifecycle, move |client, lifecycle| {
                    applied(lifecycle, client.propose(&proposal))
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
                blocking(&source, &lifecycle, move |client, lifecycle| {
                    applied(lifecycle, client.decide(&decision))
                })
                .await
            }
            Command::Proposal { proposal_id } => {
                let id = proposal_id.clone();
                blocking(&source, &lifecycle, move |client, _| {
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
                blocking(&source, &lifecycle, move |client, lifecycle| {
                    applied(lifecycle, client.acknowledge(&target))
                })
                .await
            }
            Command::PreviewReversal { event_id } => {
                let id = event_id.clone();
                blocking(&source, &lifecycle, move |client, _| {
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
    /// holds, at most `limit`, and no longer than `cancel` stays unfired.
    async fn wait(
        source: &Source,
        limit: Duration,
        cancel: &Cancellation,
        ready: impl Fn(&Source) -> bool,
    ) -> bool {
        use keeper_core::hq::port::LinkState;
        let started = Instant::now();
        loop {
            let link = source.replica.state().link;
            if matches!(link, LinkState::Connected) && ready(source) {
                return true;
            }
            if cancel.is_cancelled()
                || matches!(link, LinkState::Refused | LinkState::Closed)
                || started.elapsed() > limit
            {
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
        match INTERRUPTED.load(Ordering::SeqCst) {
            _ if ok => std::process::ExitCode::SUCCESS,
            0 => std::process::ExitCode::FAILURE,
            signal => std::process::ExitCode::from(u8::try_from(128 + signal).unwrap_or(1)),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use keeper_core::graph::write::{Caller, GraphWrite, Writer};
        use keeper_core::hq::port::{CommandResult, ReplicaError};
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
            fn receipt(
                &self,
                _: &str,
            ) -> Result<Vec<serde_json::Map<String, Value>>, ReplicaError> {
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
                let dir = std::env::temp_dir()
                    .join(format!("kg1-driver-resume-{}-{n}", std::process::id()));
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
            let dir = std::env::temp_dir()
                .join(format!("kg1-driver-signals-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("dir");
            dir
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
