//! Measurement tooling for story 110.5's P1 class-contract stage (never shipped: an example,
//! not built by `--bins`; `scripts/check-graph-boundary.py` pins the example set). It runs the
//! production spawn path itself — `VerifiedWorker::open`, `Runtime::admit` (ledger rootfs and
//! process checks, P6c), `GraphReads` over the process `Admission`, and for each spawn the
//! per-spawn recheck and the spawning-thread check immediately before `spawn` — over a local
//! `GraphSource` holding a small accepted history, with no server: the context authority
//! comes from a local stub of `graph.admit_read` / `graph.refresh_context`.
//!
//! ```sh
//! rc_spawner [observe --] --worker <abs path> --worker-sha256 <hex> [--ledger <file>] \
//!            [--macos-evidence] --root <projection dir> --mode admit|run
//! rc_spawner [observe --] control reaped|live|exec-failure|none
//! ```
//!
//! It prints one JSON line:
//! - `admitted`, and the refusal `label` when not;
//! - in mode `run`, `steps`: exactly `build`, `query` and `inspect`, each with its outcome,
//!   and `nc9`: `pass` only when all three succeeded with the expected answers (the build
//!   publishes position 1, the query returns the one tagged node, the inspection its title
//!   and tag) — only a worker that reached B0 and answered produces them;
//! - `children.spawn_attempts`: the admission's cumulative count, incremented immediately
//!   before every `runtime.spawn()` whether or not the spawn succeeds;
//! - `telemetry` (peak/live workers, `RUSAGE_CHILDREN` max RSS): observations only, never a
//!   creation count (a live or unwaited child leaves max RSS 0).
//!
//! `observe --` (Linux) runs the same invocation as a child seized with ptrace
//! (`PTRACE_O_TRACEFORK|TRACEVFORK|TRACECLONE`, the child held until seized) and adds
//! `observed`: every task the child or its threads created, classified by the new task's own
//! `/proc/<tid>/status` `Tgid` (a new process is its own thread-group leader), each new
//! process detached at its first stop. This does not depend on the spawn accounting; the
//! `control` invocations calibrate it (a reaped child, a live unwaited child and a failed
//! exec each count one process; `none` counts zero).
//!
//! Predicates: an NC zero-children row is `admitted == false`, its label,
//! `children.spawn_attempts == 0` and `observed.processes_created == 0`. NC-9 is
//! `mode == "run"`, `admitted == true`, `nc9.pass == true` and, observed,
//! `observed.processes_created == children.spawn_attempts == 3`. Exit status: 2 for a usage
//! error; in mode `run` 0 only when `nc9.pass`, else 1; in mode `admit` and for controls 0
//! once the report is written; `observe` exits with the child's status, or 3 when it could
//! not observe.

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

    /// NC-9's answers: exactly the three steps, each successful with the expected answer.
    fn nc9(steps: &[Value]) -> Vec<String> {
        let expected = [
            ("build", json!({"graph_seq": 1, "built": true})),
            ("query", json!([{"id": id(1), "tag": "research"}])),
            ("inspect", json!({"title": "node 1", "tags": ["research"]})),
        ];
        let mut failures = Vec::new();
        if steps.len() != expected.len() {
            failures.push(format!("{} steps, expected 3", steps.len()));
        }
        for (i, (name, answer)) in expected.iter().enumerate() {
            match steps.get(i) {
                None => failures.push(format!("{name}: missing")),
                Some(step) if step["step"] != *name => {
                    failures.push(format!("step {i} is {}, expected {name}", step["step"]));
                }
                Some(step) if step["ok"] != true => {
                    failures.push(format!("{name}: refused {}", step["label"]));
                }
                Some(step) if step["detail"] != *answer => {
                    failures.push(format!("{name}: answered {}", step["detail"]));
                }
                Some(_) => {}
            }
        }
        failures
    }

    /// The report and whether the invocation succeeded (mode `run`: NC-9 passed).
    async fn run(args: Vec<String>) -> Result<(Value, bool)> {
        let mode = arg(&args, "--mode").ok_or("--mode admit|run is required")?;
        if mode != "run" && mode != "admit" {
            return Err(format!("unknown --mode {mode}: admit or run").into());
        }
        let worker = PathBuf::from(arg(&args, "--worker").ok_or("--worker is required")?);
        let expected = arg(&args, "--worker-sha256");
        let ledger_path = arg(&args, "--ledger").map(PathBuf::from);
        let root = PathBuf::from(arg(&args, "--root").ok_or("--root is required")?);
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
        let admitted = match runtime {
            Err(error) => {
                report["label"] = json!(label(&error));
                false
            }
            Ok(runtime) => {
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
                true
            }
        };
        report["admitted"] = json!(admitted);
        let ok = if mode == "run" {
            report["steps"] = Value::Array(steps.clone());
            let mut failures = nc9(&steps);
            if !admitted {
                failures.insert(0, "not admitted".to_owned());
            }
            report["nc9"] = json!({"pass": failures.is_empty(), "failures": failures});
            failures.is_empty()
        } else {
            true
        };
        let stats = admission.stats();
        report["children"] = json!({"spawn_attempts": stats.spawn_attempts});
        report["telemetry"] = json!({
            "note": "observations only, never a creation count",
            "peak_workers": stats.peak_workers,
            "live_workers": stats.live_workers,
            "rusage_children_maxrss_kib": children_maxrss_kib(),
        });
        Ok((report, ok))
    }

    /// The observer's calibration: one child created in each way the review names, or none.
    fn control(kind: &str) -> Result<Value> {
        use std::process::Command;
        let result = match kind {
            "reaped" => json!({"status": Command::new("/bin/true").status()?.code()}),
            "live" => {
                // Left running and never waited for: this process exits first.
                let child = Command::new("/bin/sleep").arg("2").spawn()?;
                json!({"pid": child.id(), "waited": false})
            }
            // `/dev/null` exists and is not executable: the child is created and its exec fails.
            "exec-failure" => {
                json!({"spawn_error": Command::new("/dev/null").spawn().err().map(|e| e.to_string())})
            }
            "none" => json!(null),
            other => return Err(format!("unknown control {other}").into()),
        };
        Ok(json!({"tool": "rc_spawner", "control": kind, "result": result}))
    }

    pub fn main() -> std::process::ExitCode {
        use std::process::ExitCode;
        let mut args: Vec<String> = std::env::args().skip(1).collect();
        if args.first().map(String::as_str) == Some("observe") {
            #[cfg(target_os = "linux")]
            return super::observer::main(
                args.split_off(2.min(args.len())),
                args.get(1).map(String::as_str) == Some("--"),
            );
            #[cfg(not(target_os = "linux"))]
            {
                println!(
                    "{}",
                    json!({"tool": "rc_spawner", "error": "observe is Linux only"})
                );
                return ExitCode::from(2);
            }
        }
        // Under `observe`, wait until the observer has seized this process.
        if args.first().map(String::as_str) == Some(super::AWAIT_GO) {
            args.remove(0);
            let mut go = [0u8; 1];
            if std::io::Read::read_exact(&mut std::io::stdin(), &mut go).is_err() {
                return ExitCode::from(3);
            }
        }
        let outcome = if args.first().map(String::as_str) == Some("control") {
            match args.get(1) {
                Some(kind) => control(kind).map(|r| (r, true)),
                None => Err("control reaped|live|exec-failure|none".into()),
            }
        } else {
            match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime.block_on(run(args)),
                Err(error) => Err(error.into()),
            }
        };
        match outcome {
            Ok((report, ok)) => {
                println!("{report}");
                if ok {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            }
            Err(error) => {
                println!(
                    "{}",
                    json!({"tool": "rc_spawner", "error": error.to_string()})
                );
                ExitCode::from(2)
            }
        }
    }
}

/// The first argument of a child the observer holds until it has seized it.
#[cfg(any(target_os = "linux", target_os = "macos"))]
const AWAIT_GO: &str = "--await-observer";

/// The independent creation observation: ptrace, seizing the measured process before it
/// runs any of its own code, with fork, vfork and clone events of every thread.
#[cfg(target_os = "linux")]
mod observer {
    use std::collections::BTreeSet;
    use std::io::{Read, Write};
    use std::process::{Command, ExitCode, Stdio};

    use nix::errno::Errno;
    use nix::sys::ptrace::{self, Options};
    use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
    use nix::unistd::Pid;
    use serde_json::{json, Value};

    /// The thread group of `tid`, from its own status file.
    fn tgid(tid: Pid) -> Option<i32> {
        std::fs::read_to_string(format!("/proc/{tid}/status"))
            .ok()?
            .lines()
            .find_map(|l| l.strip_prefix("Tgid:"))
            .and_then(|v| v.trim().parse().ok())
    }

    fn fail(why: String) -> ExitCode {
        println!("{}", json!({"tool": "rc_spawner observe", "error": why}));
        ExitCode::from(3)
    }

    pub fn main(inner: Vec<String>, separated: bool) -> ExitCode {
        if !separated || inner.is_empty() {
            return fail("usage: observe -- <rc_spawner arguments>".into());
        }
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(e) => return fail(e.to_string()),
        };
        let mut child = match Command::new(exe)
            .arg(super::AWAIT_GO)
            .args(&inner)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(e) => return fail(e.to_string()),
        };
        let root = Pid::from_raw(child.id() as i32);
        let options = Options::PTRACE_O_TRACEFORK
            | Options::PTRACE_O_TRACEVFORK
            | Options::PTRACE_O_TRACECLONE
            | Options::PTRACE_O_EXITKILL;
        if let Err(e) = ptrace::seize(root, options) {
            let _ = child.kill();
            return fail(format!("ptrace seize: {e}"));
        }
        let mut stdout = child.stdout.take();
        let reader = std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(out) = stdout.as_mut() {
                let _ = out.read_to_string(&mut text);
            }
            text
        });
        // Release the child only now that every task it creates is reported.
        let released = child.stdin.take().map(|mut go| go.write_all(b"g"));
        if !matches!(released, Some(Ok(()))) {
            let _ = child.kill();
            return fail("could not release the observed process".into());
        }
        let mut known: BTreeSet<i32> = BTreeSet::from([root.as_raw()]);
        let mut processes: Vec<i32> = Vec::new();
        let mut threads = 0usize;
        let mut events = 0usize;
        let mut inner_exit: Option<i32> = None;
        loop {
            let status = match waitpid(None, Some(WaitPidFlag::__WALL)) {
                Ok(status) => status,
                Err(Errno::ECHILD) => break,
                Err(Errno::EINTR) => continue,
                Err(e) => return fail(format!("waitpid: {e}")),
            };
            let Some(pid) = status.pid() else { continue };
            if !matches!(status, WaitStatus::Exited(..) | WaitStatus::Signaled(..))
                && known.insert(pid.as_raw())
            {
                // A task seen for the first time: auto-attached at creation, now stopped.
                if tgid(pid) == Some(pid.as_raw()) {
                    processes.push(pid.as_raw());
                    let _ = ptrace::detach(pid, None);
                } else {
                    threads += 1;
                    let _ = ptrace::cont(pid, None);
                }
                continue;
            }
            match status {
                WaitStatus::Exited(p, code) if p == root => inner_exit = Some(code),
                WaitStatus::Signaled(p, signal, _) if p == root => {
                    inner_exit = Some(128 + signal as i32);
                }
                WaitStatus::Exited(..) | WaitStatus::Signaled(..) => {}
                WaitStatus::PtraceEvent(p, _, event) => {
                    if event != libc_event_stop() {
                        events += 1;
                    }
                    let _ = ptrace::cont(p, None);
                }
                WaitStatus::Stopped(p, signal) => {
                    let _ = ptrace::cont(p, Some(signal));
                }
                other => {
                    if let Some(p) = other.pid() {
                        let _ = ptrace::cont(p, None);
                    }
                }
            }
        }
        let text = reader.join().unwrap_or_default();
        let mut report: Value = match text.lines().last().map(serde_json::from_str) {
            Some(Ok(report)) => report,
            _ => return fail(format!("the observed process printed no report: {text:?}")),
        };
        report["observed"] = json!({
            "method": "ptrace seize (TRACEFORK|TRACEVFORK|TRACECLONE) before the process runs; each new task classified by /proc/<tid>/status Tgid",
            "processes_created": processes.len(),
            "process_pids": processes,
            "threads_created": threads,
            "creation_events": events,
            "exit": inner_exit,
        });
        println!("{report}");
        match inner_exit {
            Some(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
            None => ExitCode::from(3),
        }
    }

    /// `PTRACE_EVENT_STOP`: a seized task's group-stop or initial stop, not a creation.
    const fn libc_event_stop() -> i32 {
        128
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn main() -> std::process::ExitCode {
    spawner::main()
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn main() {}
