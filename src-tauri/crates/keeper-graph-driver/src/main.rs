//! `keeper-graph-driver`: a person's device's graph without a window — the
//! headless app-human consumer of the safe graph API against a real
//! keeper-hq (E1). Each run opens the device's persistent on-disk replica of
//! one database and does one thing, printing one JSON line:
//!
//! - `apply <payload.json>` — a `graph.apply` change (the payload as the
//!   contract writes it) and its outcome;
//! - `sync` — wait until the replica holds the domain's graph, then build
//!   or extend the local query projection;
//! - `query <source> [--params <json>] [--allow-stale]` — one
//!   `KG-Cypher/1` query over the projection;
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
//! - `status` — the replica's head and checkpoint and the projection's
//!   runtime admission.
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
    use keeper_core::graph::client::{subscriptions, Applied, GraphClient, GraphClientError};
    use keeper_core::graph::outcome::GraphOutcome;
    use keeper_core::graph::protocol::inverse::{PreviewOutcome, Reverse};
    use keeper_core::graph::protocol::review::Target;
    use keeper_core::graph::protocol::wire::HistoryFilter;
    use keeper_core::graph::protocol::worker::InspectEntity;
    use keeper_core::graph::protocol::{change, proposal};
    use keeper_core::graph::query::{
        Freshness, GraphReads, HumanReader, Inspected, QueryError, QueryResult,
    };
    use keeper_core::graph::runtime::{Runtime, VerifiedWorker};
    use keeper_core::graph::source::{GraphSource, ScopeState};
    use keeper_core::graph::supervisor::Cancellation;
    use keeper_core::hq::port::{Link, LinkState, ReplicaSpec};
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

    /// The refusal signals this device can see without asking (112.1's
    /// driver `signals`, without the intent fence this rung has not got): a
    /// refused link (the device was de-authorised) or a revoked graph
    /// subscription (no graph role, or one revoked) is `forbidden` (110.5
    /// AC4), never `sync_pending`.
    fn refused(link: LinkState, scope: Option<ScopeState>) -> Option<Value> {
        (link == LinkState::Refused || scope == Some(ScopeState::Revoked))
            .then(|| fail("forbidden", "this domain is not readable by this person"))
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
    /// done.
    async fn blocking(
        source: &Source,
        call: impl FnOnce(GraphClient) -> Value + Send + 'static,
    ) -> Value {
        let client = GraphClient::new(source.clone());
        wait(source, Duration::from_secs(30), |_| true).await;
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

    fn reader(args: &Args, source: &Source) -> Result<HumanReader, QueryError> {
        let runtime = runtime(args)?;
        let root = args.state.join(format!("{}.projection", args.database));
        let reads = GraphReads::open(
            Arc::new(source.clone()) as Arc<dyn GraphSource>,
            &root,
            Admission::process(Process::Headless)?,
            runtime,
        )?;
        reads.human(source).ok_or(QueryError::Forbidden)
    }

    async fn run(args: Args) -> Value {
        let source = match open(&args) {
            Ok(source) => source,
            Err(error) => return error,
        };
        let cancel = Cancellation::default();
        match &args.command {
            Command::Apply { payload } => {
                let text = match std::fs::read(payload) {
                    Ok(text) => text,
                    Err(e) => return fail("invalid_change", &e.to_string()),
                };
                let change = keeper_core::graph::protocol::value::parse(&text)
                    .and_then(|v| change::parse(&v, normalise));
                let change = match change {
                    Ok(change) => change,
                    Err(refusal) => return fail(refusal.code(), "payload"),
                };
                blocking(&source, move |client| applied(client.apply(&change))).await
            }
            Command::Sync { wait_secs } => {
                // Until the scope is settled either way, as 112.1's driver
                // waits: a scope the server refuses reads `Revoked`, never
                // `Granted`, and must not time out as `sync_pending`.
                let ready = wait(&source, Duration::from_secs(*wait_secs), |s| {
                    s.checkpoint().is_ok_and(|c| c.scope != ScopeState::Pending)
                })
                .await;
                if let Some(error) = refused(
                    source.replica.state().link,
                    source.checkpoint().ok().map(|c| c.scope),
                ) {
                    return error;
                }
                if !ready {
                    return fail(
                        "sync_pending",
                        "the graph subscriptions have not pulled yet",
                    );
                }
                match reader(&args, &source) {
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
            } => match reader(&args, &source) {
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
            },
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
                match reader(&args, &source) {
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
                blocking(&source, move |client| {
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
                blocking(&source, move |client| applied(client.propose(&proposal))).await
            }
            Command::Decide { payload: path } => {
                let decision = match payload(path)
                    .and_then(|v| proposal::parse_decide(&v).map_err(|r| fail(r.code(), "payload")))
                {
                    Ok(d) => d,
                    Err(e) => return e,
                };
                blocking(&source, move |client| applied(client.decide(&decision))).await
            }
            Command::Proposal { proposal_id } => {
                let id = proposal_id.clone();
                blocking(&source, move |client| match client.proposal(&id) {
                    Ok(rows) => json!({"ok": true, "revisions": rows}),
                    Err(e) => fail("refused", &e.to_string()),
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
                blocking(&source, move |client| applied(client.acknowledge(&target))).await
            }
            Command::PreviewReversal { event_id } => {
                let id = event_id.clone();
                blocking(&source, move |client| match client.preview_reversal(&id) {
                    Ok(preview) => json!({"ok": true, "preview": preview}),
                    Err(e) => fail("refused", &e.to_string()),
                })
                .await
            }
            Command::Reverse {
                event_id,
                approval_id,
            } => {
                let id = event_id.clone();
                let approval_id = approval_id.clone();
                blocking(&source, move |client| {
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
                json!({
                    "ok": true,
                    "head": head.ok().flatten(),
                    "checkpoint": checkpoint.ok().map(|c| json!({"scope": format!("{:?}", c.scope).to_lowercase(), "cursor": c.cursor})),
                    "runtime": match runtime { Ok(()) => json!("admitted"), Err(e) => json!(e) },
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

    #[cfg(test)]
    mod tests {
        use super::*;

        /// 110.5 AC4 (R-B6): a revoked graph subscription (a person with no
        /// graph role) or a refused link is `forbidden`; a granted or still
        /// pending scope on a live link is no refusal.
        #[test]
        fn a_revoked_scope_or_a_refused_link_is_forbidden() {
            for (link, scope) in [
                (LinkState::Connected, Some(ScopeState::Revoked)),
                (LinkState::Refused, Some(ScopeState::Granted)),
                (LinkState::Refused, None),
            ] {
                let answer = refused(link, scope).expect("refused");
                assert_eq!(
                    (answer["ok"].clone(), answer["error"].clone()),
                    (json!(false), json!("forbidden")),
                    "{link:?} {scope:?}"
                );
            }
            for scope in [Some(ScopeState::Granted), Some(ScopeState::Pending), None] {
                assert_eq!(refused(LinkState::Connected, scope), None, "{scope:?}");
            }
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
