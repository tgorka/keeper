//! Measurement tooling for story 110.5's P1 class-contract stage (never shipped: an example,
//! not built by `--bins`; `scripts/check-graph-boundary.py` pins the example set). It runs the
//! production spawn path itself — `VerifiedWorker::open`, `Runtime::admit` (ledger rootfs and
//! process checks, P6c), `GraphReads` over the process `Admission`, and for each spawn the
//! per-spawn recheck and the spawning-thread check immediately before `spawn` — over a local
//! `GraphSource` holding a small accepted history, with no server: the context authority
//! comes from a local stub of `graph.admit_read` / `graph.refresh_context`.
//!
//! ```sh
//! rc_spawner --worker <abs path> --worker-sha256 <hex> [--ledger <file>] \
//!            [--macos-evidence] --root <projection dir> --mode admit|run
//! rc_spawner control none|reaped|live|exec-failure|vfork|double-fork|setsid-grandchild|pthread
//! rc_spawner observe -- PROGRAM ARGS…          # Linux; PROGRAM is usually rc_spawner itself
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
//! `observe -- PROGRAM ARGS…` (Linux, module `observer`) traces PROGRAM from before its exec
//! — loader and constructors included — and every descendant until it exits, and adds
//! `observed` to PROGRAM's report: the processes and threads created, by creation event, and
//! `errors` (any uncertainty, which makes it exit 3). It does not depend on the spawn
//! accounting; the `control` kinds and `observer-controls.sh` (a pre-main constructor child)
//! calibrate it.
//!
//! Predicates: an NC zero-children row is `admitted == false`, its label,
//! `children.spawn_attempts == 0`, `observed.processes_created == 0` and
//! `observed.errors == []`. NC-9 is `mode == "run"`, `admitted == true`, `nc9.pass == true`
//! and, observed, `observed.errors == []` and
//! `observed.processes_created == children.spawn_attempts == 3`. Exit status: 2 for a usage
//! error; in mode `run` 0 only when `nc9.pass`, else 1; in mode `admit` and for controls 0
//! once the report is written; `observe` exits with PROGRAM's status, or 3 when its
//! observation is incomplete.

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

    /// The observer's calibration controls: processes created in each way the reviews name.
    /// Expected `observed.processes_created`: none 0, reaped 1, live 1, exec-failure 1,
    /// vfork 1, double-fork 2, setsid-grandchild 2, pthread 0 (one thread). The pre-main
    /// constructor control needs a C shared object and is run by `observer-controls.sh`.
    fn control(kind: &str) -> Result<Value> {
        use std::process::Command;
        let me = std::env::current_exe()?;
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
            // std spawns through posix_spawn: clone(CLONE_VM | CLONE_VFORK).
            "vfork" => json!({"status": Command::new("/bin/true").status()?.code()}),
            // A child that leaves an orphaned grandchild behind and exits.
            "double-fork" => {
                json!({"status": Command::new(&me).args(["control-helper", "orphan"]).status()?.code()})
            }
            // A child that starts a new session, then creates and reaps a grandchild.
            "setsid-grandchild" => {
                json!({"status": Command::new(&me).args(["control-helper", "setsid"]).status()?.code()})
            }
            "pthread" => {
                let joined = std::thread::spawn(|| 7)
                    .join()
                    .map_err(|_| "thread panicked")?;
                json!({"joined": joined})
            }
            "none" => json!(null),
            other => return Err(format!("unknown control {other}").into()),
        };
        Ok(json!({"tool": "rc_spawner", "control": kind, "result": result}))
    }

    /// The second level of `double-fork` and `setsid-grandchild`.
    fn control_helper(kind: Option<&str>) -> std::process::ExitCode {
        use std::process::{Command, ExitCode};
        let done = match kind {
            Some("orphan") => Command::new("/bin/sleep").arg("1").spawn().is_ok(),
            #[cfg(target_os = "linux")]
            Some("setsid") => {
                nix::unistd::setsid().is_ok()
                    && Command::new("/bin/true")
                        .status()
                        .is_ok_and(|s| s.success())
            }
            _ => false,
        };
        if done {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
    }

    pub fn main() -> std::process::ExitCode {
        use std::process::ExitCode;
        let args: Vec<String> = std::env::args().skip(1).collect();
        match args.first().map(String::as_str) {
            #[cfg(target_os = "linux")]
            Some("observe") => return super::observer::main(&args[1..]),
            #[cfg(target_os = "linux")]
            Some(super::observer::STUB) => return super::observer::stub(&args[1..]),
            #[cfg(not(target_os = "linux"))]
            Some("observe") => {
                println!(
                    "{}",
                    json!({"tool": "rc_spawner", "error": "observe is Linux only"})
                );
                return ExitCode::from(2);
            }
            Some("control-helper") => return control_helper(args.get(1).map(String::as_str)),
            _ => {}
        }
        let outcome = if args.first().map(String::as_str) == Some("control") {
            match args.get(1) {
                Some(kind) => control(kind).map(|r| (r, true)),
                None => Err("control <kind>".into()),
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

/// The independent creation observation (Linux): the measured program is traced from before
/// its exec, so its loader, constructors and every instruction after are traced, and every
/// descendant task stays traced until it exits.
///
/// `rc_spawner observe -- PROGRAM ARGS…` starts this binary as a stub with an empty
/// environment; the stub calls `PTRACE_TRACEME`, stops itself with `SIGSTOP` and, once the
/// observer has set `TRACEFORK|TRACEVFORK|TRACECLONE|TRACEEXEC|EXITKILL`, `execve`s PROGRAM
/// with the observer's own environment. The observer is a child subreaper, so orphaned
/// descendants are reparented to it, and it waits until no task is left.
///
/// Creation is counted from the fork/vfork/clone events: fork and vfork always create a
/// process; a clone event creates a process when the new task is its own thread-group leader
/// (`/proc/<tid>/status` `Tgid`, read while it is stopped at creation), else a thread. Every
/// exit the observer reaps is checked against the tasks it knows. Any uncertainty — an
/// unreadable status, a ptrace error other than a vanished task, a task that stopped without
/// a creation event, an exit of an unknown process, or a program that never reached its exec —
/// is listed under `observed.errors`, and the observer exits 3.
#[cfg(target_os = "linux")]
mod observer {
    use std::collections::{BTreeMap, BTreeSet};
    use std::ffi::CString;
    use std::io::Read;
    use std::process::{Command, ExitCode, Stdio};

    use nix::errno::Errno;
    use nix::sys::ptrace::{self, Event, Options};
    use nix::sys::signal::{raise, Signal};
    use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
    use nix::unistd::Pid;
    use serde_json::{json, Value};

    /// The first argument of the traced stub.
    pub const STUB: &str = "--observe-stub";

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

    /// The stub: become traced, stop, then exec the measured program with the observer's
    /// environment (`/proc/<parent>/environ`). It has no environment of its own, so nothing
    /// is preloaded into it.
    pub fn stub(program: &[String]) -> ExitCode {
        let parent = nix::unistd::getppid();
        let env: Vec<CString> = match std::fs::read(format!("/proc/{parent}/environ")) {
            Ok(bytes) => bytes
                .split(|b| *b == 0)
                .filter(|v| !v.is_empty())
                .filter_map(|v| CString::new(v).ok())
                .collect(),
            Err(_) => return ExitCode::from(126),
        };
        let argv: Vec<CString> = match program.iter().map(|a| CString::new(a.as_str())).collect() {
            Ok(argv) => argv,
            Err(_) => return ExitCode::from(126),
        };
        let Some(path) = argv.first() else {
            return ExitCode::from(126);
        };
        if ptrace::traceme().is_err() || raise(Signal::SIGSTOP).is_err() {
            return ExitCode::from(126);
        }
        let _ = nix::unistd::execve(path, &argv, &env);
        ExitCode::from(127)
    }

    #[derive(Default)]
    struct Tally {
        processes: Vec<i32>,
        threads: Vec<i32>,
        events: BTreeMap<&'static str, usize>,
        reaped: usize,
        vanished: usize,
        errors: Vec<String>,
    }

    pub fn main(args: &[String]) -> ExitCode {
        let program = match args.split_first() {
            Some((sep, program)) if sep == "--" && !program.is_empty() => program,
            _ => return fail("usage: observe -- PROGRAM ARGS…".into()),
        };
        if let Err(e) = nix::sys::prctl::set_child_subreaper(true) {
            return fail(format!("PR_SET_CHILD_SUBREAPER: {e}"));
        }
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(e) => return fail(e.to_string()),
        };
        let mut child = match Command::new(exe)
            .arg(STUB)
            .args(program)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(e) => return fail(e.to_string()),
        };
        let root = Pid::from_raw(i32::try_from(child.id()).unwrap_or(-1));
        let mut stdout = child.stdout.take();
        let reader = std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(out) = stdout.as_mut() {
                let _ = out.read_to_string(&mut text);
            }
            text
        });
        let tally = observe(root);
        let text = reader.join().unwrap_or_default();
        let (exit, tally) = match tally {
            Ok(done) => done,
            Err(why) => return fail(why),
        };
        let mut report = match text.lines().last().map(serde_json::from_str::<Value>) {
            Some(Ok(report)) if report.is_object() => report,
            _ => {
                json!({"stdout_tail": text.chars().rev().take(400).collect::<String>().chars().rev().collect::<String>()})
            }
        };
        report["observed"] = json!({
            "method": "PTRACE_TRACEME before exec (TRACEFORK|TRACEVFORK|TRACECLONE|TRACEEXEC|EXITKILL); every descendant traced to exit; observer is a child subreaper",
            "program": program,
            "processes_created": tally.processes.len(),
            "process_pids": tally.processes,
            "threads_created": tally.threads.len(),
            "events": tally.events,
            "processes_reaped": tally.reaped,
            "vanished_on_ptrace": tally.vanished,
            "errors": tally.errors,
            "exit": exit,
        });
        println!("{report}");
        match exit {
            Some(_) if !tally.errors.is_empty() => ExitCode::from(3),
            Some(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
            None => ExitCode::from(3),
        }
    }

    /// Continue `pid`, recording a ptrace failure: a task that vanished (`ESRCH`, e.g.
    /// killed) is counted; anything else is an error.
    fn resume(tally: &mut Tally, pid: Pid, signal: Option<Signal>) {
        match ptrace::cont(pid, signal) {
            Ok(()) => {}
            Err(Errno::ESRCH) => tally.vanished += 1,
            Err(e) => tally.errors.push(format!("cont {pid}: {e}")),
        }
    }

    /// Trace `root` (stopped by its own `SIGSTOP` after `PTRACE_TRACEME`) and every task it
    /// creates until no task is left; the root's exit status and the tally.
    fn observe(root: Pid) -> Result<(Option<i32>, Tally), String> {
        match waitpid(root, Some(WaitPidFlag::__WALL)) {
            Ok(WaitStatus::Stopped(_, Signal::SIGSTOP)) => {}
            other => return Err(format!("the stub did not stop for tracing: {other:?}")),
        }
        let options = Options::PTRACE_O_TRACEFORK
            | Options::PTRACE_O_TRACEVFORK
            | Options::PTRACE_O_TRACECLONE
            | Options::PTRACE_O_TRACEEXEC
            | Options::PTRACE_O_EXITKILL;
        ptrace::setoptions(root, options).map_err(|e| format!("setoptions: {e}"))?;
        let mut tally = Tally::default();
        // Every task the observer knows: the root, then each one a creation event announced.
        let mut processes: BTreeSet<i32> = BTreeSet::from([root.as_raw()]);
        let mut threads: BTreeSet<i32> = BTreeSet::new();
        // New tasks start with SIGSTOP; that one stop is suppressed, whichever of the
        // creation event and the stop the observer sees first.
        let mut announced: BTreeSet<i32> = BTreeSet::new();
        let mut stopped_unannounced: BTreeSet<i32> = BTreeSet::new();
        let mut root_exec = false;
        let mut exit = None;
        resume(&mut tally, root, None);
        loop {
            let status = match waitpid(None, Some(WaitPidFlag::__WALL)) {
                Ok(status) => status,
                Err(Errno::ECHILD) => break,
                Err(Errno::EINTR) => continue,
                Err(e) => return Err(format!("waitpid: {e}")),
            };
            match status {
                WaitStatus::Exited(pid, _) | WaitStatus::Signaled(pid, _, _) => {
                    let raw = pid.as_raw();
                    if pid == root {
                        exit = Some(match status {
                            WaitStatus::Exited(_, code) => code,
                            WaitStatus::Signaled(_, signal, _) => 128 + signal as i32,
                            _ => -1,
                        });
                    }
                    if processes.contains(&raw) {
                        tally.reaped += 1;
                    } else if !threads.contains(&raw) {
                        tally.errors.push(format!("reaped unknown task {raw}"));
                    }
                }
                WaitStatus::PtraceEvent(pid, _, event) => {
                    let kind = match event {
                        e if e == Event::PTRACE_EVENT_FORK as i32 => Some("fork"),
                        e if e == Event::PTRACE_EVENT_VFORK as i32 => Some("vfork"),
                        e if e == Event::PTRACE_EVENT_CLONE as i32 => Some("clone"),
                        e if e == Event::PTRACE_EVENT_EXEC as i32 => {
                            if pid == root {
                                root_exec = true;
                            }
                            *tally.events.entry("exec").or_default() += 1;
                            None
                        }
                        _ => None,
                    };
                    if let Some(kind) = kind {
                        match ptrace::getevent(pid).map(|n| i32::try_from(n).unwrap_or(-1)) {
                            Ok(new) if new > 0 => {
                                let process = match kind {
                                    "clone" => match tgid(Pid::from_raw(new)) {
                                        Some(group) => group == new,
                                        None => {
                                            tally.errors.push(format!(
                                                "unreadable status of new task {new}"
                                            ));
                                            true
                                        }
                                    },
                                    _ => true,
                                };
                                let label = match (kind, process) {
                                    ("clone", true) => "clone_process",
                                    ("clone", false) => "clone_thread",
                                    (other, _) => other,
                                };
                                *tally.events.entry(label).or_default() += 1;
                                if process {
                                    processes.insert(new);
                                    tally.processes.push(new);
                                } else {
                                    threads.insert(new);
                                    tally.threads.push(new);
                                }
                                if stopped_unannounced.remove(&new) {
                                    resume(&mut tally, Pid::from_raw(new), None);
                                } else {
                                    announced.insert(new);
                                }
                            }
                            other => tally
                                .errors
                                .push(format!("{kind} event of {pid}: new task {other:?}")),
                        }
                    }
                    resume(&mut tally, pid, None);
                }
                WaitStatus::Stopped(pid, signal) => {
                    let raw = pid.as_raw();
                    let known = processes.contains(&raw) || threads.contains(&raw);
                    if signal == Signal::SIGSTOP && announced.remove(&raw) {
                        resume(&mut tally, pid, None);
                    } else if signal == Signal::SIGSTOP && !known {
                        // Its creation event is still to come.
                        stopped_unannounced.insert(raw);
                    } else {
                        resume(&mut tally, pid, Some(signal));
                    }
                }
                other => {
                    if let Some(pid) = other.pid() {
                        resume(&mut tally, pid, None);
                    }
                }
            }
        }
        if !root_exec {
            tally
                .errors
                .push("the measured program was never exec'd under trace".into());
        }
        if !stopped_unannounced.is_empty() {
            tally.errors.push(format!(
                "tasks stopped without a creation event: {stopped_unannounced:?}"
            ));
        }
        Ok((exit, tally))
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn main() -> std::process::ExitCode {
    spawner::main()
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn main() {}
