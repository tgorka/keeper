//! Measurement tooling for story 110.5's P1 class-contract stage (never shipped: an example,
//! not built by `--bins`; `scripts/check-graph-boundary.py` pins that). It runs the
//! production spawn path itself — `VerifiedWorker::open`, `Runtime::admit` (ledger rootfs and
//! process checks, P6c), `GraphReads` over the process `Admission`, and for each spawn the
//! per-spawn recheck and the spawning-thread check immediately before `spawn` — over a local
//! `GraphSource` holding a small accepted history, with no server: the context authority
//! comes from a local stub of `graph.admit_read` / `graph.refresh_context`.
//!
//! ```sh
//! rc_spawner --worker <abs path> --worker-sha256 <hex> [--ledger <file>] [--macos-evidence] \
//!            --root <projection dir> [--mode admit|run]
//! ```
//!
//! It prints one JSON line: whether the runtime was admitted and, if not, its refusal label;
//! each step's outcome (`build`, `query`, `inspect` in mode `run`) with its label; the
//! worker processes the supervisor spawned (`peak_workers`); and an independent child count
//! from `getrusage(RUSAGE_CHILDREN)` (a reaped child always leaves a non-zero max RSS).
//! NC "zero children" rows read `children.spawned == 0` and `children.rusage_maxrss_kib == 0`;
//! NC-9 reads `admitted == true` and every step `ok`.

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod spawner {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use keeper_core::graph::admission::{Admission, Process};
    use keeper_core::graph::protocol::delta::{self, Op};
    use keeper_core::graph::protocol::vocabulary::{EntityType, Operation};
    use keeper_core::graph::protocol::wire::{Authority, ContextRow, ReadStatus};
    use keeper_core::graph::protocol::worker::InspectEntity;
    use keeper_core::graph::query::{
        AdmittedRead, Freshness, GraphReads, GraphRemote, Inspected, QueryError,
    };
    use keeper_core::graph::runtime::{Runtime, VerifiedWorker};
    use keeper_core::graph::source::{
        Checkpoint, GraphSource, PrefixBatch, ScopeState, SourceError,
    };
    use keeper_core::graph::supervisor::Cancellation;
    use serde_json::{json, Map, Value};

    type Result<T, E = Box<dyn std::error::Error>> = std::result::Result<T, E>;
    /// One replicated event: its header row and its operation rows.
    type Event = (Map<String, Value>, Vec<Map<String, Value>>);

    const DOMAIN: &str = "personal";
    const CONTEXT: &str = "01JK0000000000000000000099";

    fn id(n: u64) -> String {
        format!("{n:026}")
    }

    fn create(after: Value, entity_type: EntityType) -> Op {
        let membership = entity_type == EntityType::NodeTag;
        Op {
            ordinal: 0,
            entity_type,
            entity_id: after["id"].as_str().unwrap_or_default().to_owned(),
            operation: Operation::Create,
            before_revision: None,
            after_revision: (!membership).then(|| "1".to_owned()),
            before_value: None,
            after_value: Some(after),
        }
    }

    /// One accepted event: a node, a tag and its membership, and an edge to a second node.
    fn history() -> Result<Vec<Event>> {
        let node = |n: u64| {
            json!({"id": id(n), "kind": "Concept", "title": format!("node {n}"), "description": null,
                                   "description_source_id": null, "properties": {"score": 1.0}, "lifecycle": "active",
                                   "merged_into_id": null, "revision": "1", "lifecycle_revision": "1"})
        };
        let mut ops = vec![
            create(node(1), EntityType::Node),
            create(node(2), EntityType::Node),
            create(
                json!({"id": id(4), "text": "research", "label": null, "revision": "1"}),
                EntityType::Tag,
            ),
            create(
                json!({"id": id(11), "from_id": id(1), "to_id": id(2), "type": "contains", "properties": {},
                          "lifecycle": "active", "revision": "1"}),
                EntityType::Edge,
            ),
            create(
                json!({"id": id(14), "node_id": id(1), "tag_id": id(4)}),
                EntityType::NodeTag,
            ),
        ];
        for (i, op) in ops.iter_mut().enumerate() {
            op.ordinal = u32::try_from(i)?;
        }
        let digest = delta::digest(&ops)?;
        let event = id(100);
        let header = json!({
            "id": event, "graph": DOMAIN, "graph_seq": "1", "descriptor": "graph.apply",
            "request_id": id(101), "request_digest": "0".repeat(64), "actor_id": "measure", "client_id": "measure",
            "origin_class": "human", "origin_actor": null, "origin_chain": null, "context_id": null,
            "approval_basis": {"basis": "direct"}, "reverses_event_id": null, "event_time_ms": null,
            "recorded_at_ms": 1, "op_count": ops.len(), "delta_digest": digest,
        });
        let rows = ops
            .iter()
            .map(|op| {
                let mut row = serde_json::to_value(op)?
                    .as_object()
                    .cloned()
                    .unwrap_or_default();
                row.insert("event_id".into(), json!(event));
                Ok(row)
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(vec![(
            header.as_object().cloned().unwrap_or_default(),
            rows,
        )])
    }

    /// The local history, always fully granted.
    struct Local(Vec<Event>);

    impl GraphSource for Local {
        fn domain(&self) -> &str {
            DOMAIN
        }
        fn prefix_batch(&self, after: u64, limit: usize) -> Result<PrefixBatch, SourceError> {
            let head = self.0.len() as u64;
            let mut batch = PrefixBatch {
                head: Some(head),
                ..PrefixBatch::default()
            };
            for (i, (header, ops)) in self.0.iter().enumerate() {
                let at = i as u64 + 1;
                if at > after && at <= after.saturating_add(limit as u64) {
                    batch.events.push(header.clone());
                    batch.ops.extend(ops.iter().cloned());
                }
            }
            Ok(batch)
        }
        fn checkpoint(&self) -> Result<Checkpoint, SourceError> {
            Ok(Checkpoint {
                scope: ScopeState::Granted,
                cursor: Some(1),
            })
        }
    }

    /// The server's context operations, answered locally with one fixed authority.
    struct Stub;

    impl GraphRemote for Stub {
        fn admit_read(&self, context: &str, domain: &str) -> Result<ContextRow, QueryError> {
            Ok(ContextRow {
                context: context.into(),
                session: "measure".into(),
                permitted: vec![domain.into()],
                sources: vec![domain.into()],
                expires_at_ms: u64::MAX / 2,
                authority: Authority {
                    epoch: "1".into(),
                    digest: "ab".repeat(32),
                },
            })
        }
        fn refresh_context(&self, context: &str) -> Result<ContextRow, QueryError> {
            self.admit_read(context, DOMAIN)
        }
        fn read_status(&self, _: &str) -> Result<ReadStatus, QueryError> {
            Err(QueryError::Forbidden)
        }
    }

    fn label(error: &QueryError) -> String {
        match error {
            QueryError::Unavailable(label) | QueryError::Storage(label) => {
                format!("{}: {label}", error.code())
            }
            other => other.code().to_owned(),
        }
    }

    #[cfg(target_os = "linux")]
    fn children_maxrss_kib() -> i64 {
        nix::sys::resource::getrusage(nix::sys::resource::UsageWho::RUSAGE_CHILDREN)
            .map(|u| u.max_rss())
            .unwrap_or(-1)
    }
    #[cfg(not(target_os = "linux"))]
    fn children_maxrss_kib() -> i64 {
        nix::sys::resource::getrusage(nix::sys::resource::UsageWho::RUSAGE_CHILDREN)
            .map(|u| u.max_rss() / 1024)
            .unwrap_or(-1)
    }

    fn arg(args: &[String], name: &str) -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    }

    fn file_sha256(path: &Path) -> Option<String> {
        use sha2::Digest;
        std::fs::read(path).ok().map(|b| {
            sha2::Sha256::digest(b)
                .iter()
                .map(|x| format!("{x:02x}"))
                .collect()
        })
    }

    async fn run(args: Vec<String>) -> Result<Value> {
        let worker = PathBuf::from(arg(&args, "--worker").ok_or("--worker is required")?);
        let expected = arg(&args, "--worker-sha256");
        let ledger_path = arg(&args, "--ledger").map(PathBuf::from);
        let root = PathBuf::from(arg(&args, "--root").ok_or("--root is required")?);
        let mode = arg(&args, "--mode").unwrap_or_else(|| "run".into());
        let ledger = match &ledger_path {
            Some(p) => Some(std::fs::read_to_string(p)?),
            None => None,
        };
        let admission = Admission::process(Process::Headless)?;
        let mut report = json!({
            "tool": "rc_spawner", "mode": mode, "worker": worker, "worker_sha256": file_sha256(&worker),
            "ledger_sha256": ledger_path.as_deref().and_then(file_sha256),
        });
        let runtime = VerifiedWorker::open(&worker, expected.as_deref()).and_then(|w| {
            #[cfg(target_os = "macos")]
            if args.iter().any(|a| a == "--macos-evidence") {
                return Runtime::macos_evidence(w);
            }
            Runtime::admit(w, ledger.as_deref())
        });
        let mut steps = Vec::new();
        match runtime {
            Err(error) => {
                report["admitted"] = json!(false);
                report["label"] = json!(label(&error));
            }
            Ok(runtime) => {
                report["admitted"] = json!(true);
                if mode == "run" {
                    let reads = GraphReads::open(
                        Arc::new(Local(history()?)),
                        &root,
                        admission.clone(),
                        Arc::new(runtime),
                    )?;
                    let read = AdmittedRead::admit(&Stub, "measure", "measure", CONTEXT, DOMAIN)?;
                    let reader = reads.agent(Arc::new(Stub), read)?;
                    let cancel = Cancellation::default();
                    let mut step = |name: &str, outcome: std::result::Result<Value, QueryError>| {
                        steps.push(match outcome {
                            Ok(detail) => json!({"step": name, "ok": true, "detail": detail}),
                            Err(e) => json!({"step": name, "ok": false, "label": label(&e)}),
                        });
                    };
                    let built = reader.refresh(&cancel).await;
                    step(
                        "build",
                        built.map(|r| json!({"graph_seq": r.graph_seq, "built": r.built})),
                    );
                    let queried = reader
                        .query("MATCH (n)-[:HAS_TAG]->(t:Tag) RETURN n.id AS id, t.text AS tag LIMIT 10", b"{}", Freshness::Current, &cancel)
                        .await;
                    step("query", queried.map(|r| json!(r.rows)));
                    let inspected = reader
                        .inspect(
                            InspectEntity::Node,
                            &id(1),
                            false,
                            Freshness::Current,
                            &cancel,
                        )
                        .await;
                    step(
                        "inspect",
                        inspected.map(|r| match r.rows {
                            Some(Inspected::Node(v)) => {
                                json!({"title": v.image["title"], "tags": v.tags})
                            }
                            other => json!(format!("{other:?}")),
                        }),
                    );
                }
            }
        }
        report["steps"] = Value::Array(steps);
        report["children"] = json!({
            "spawned": admission.stats().peak_workers,
            "live_after": admission.stats().live_workers,
            "rusage_maxrss_kib": children_maxrss_kib(),
        });
        Ok(report)
    }

    pub fn main() -> std::process::ExitCode {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(r) => r,
            Err(_) => return std::process::ExitCode::FAILURE,
        };
        match runtime.block_on(run(args)) {
            Ok(report) => {
                println!("{report}");
                std::process::ExitCode::SUCCESS
            }
            Err(error) => {
                println!(
                    "{}",
                    json!({"tool": "rc_spawner", "error": error.to_string()})
                );
                std::process::ExitCode::FAILURE
            }
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn main() -> std::process::ExitCode {
    spawner::main()
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn main() {}
