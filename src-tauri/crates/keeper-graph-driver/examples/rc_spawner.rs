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
//! `observe -- PROGRAM ARGS…` (Linux x86_64, module `observer`) counts every creation
//! attempt at the syscall (an inherited seccomp `RET_TRACE` filter on `clone`, `clone3`,
//! `fork`, `vfork`; `CLONE_UNTRACED` stripped; any `ptrace` denied and reported), traces
//! PROGRAM from before its exec — loader and constructors included — and every descendant
//! until it exits, and adds `observed` to PROGRAM's report: attempts, failed attempts, the
//! processes and threads created, and `errors` (any disagreement or uncertainty, which makes
//! it exit 3). It does not depend on the spawn accounting; the `control` kinds and
//! `observer-controls.sh` (C controls: constructors, raw and untraced clones) calibrate it.
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
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            Some("observe") => return super::observer::main(&args[1..]),
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            Some(super::observer::STUB) => return super::observer::stub(&args[1..]),
            #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
            Some("observe") => {
                println!(
                    "{}",
                    json!({"tool": "rc_spawner", "error": "observe is Linux x86_64 only"})
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

/// The independent creation observation (Linux x86_64). Creation is counted twice, by two
/// mechanisms that must agree:
///
/// 1. **Attempts, at the syscall.** The stub installs a seccomp filter returning
///    `SECCOMP_RET_TRACE` for `clone`, `clone3`, `fork`, `vfork` and `ptrace` (setting
///    `NO_NEW_PRIVS` first, as `seccompiler::apply_filter` does — the production exec class
///    runs under `NO_NEW_PRIVS` too). The filter is inherited by every descendant and cannot
///    be removed, so every creation attempt by any task stops the observer
///    (`PTRACE_O_TRACESECCOMP`) whatever its flags. The observer reads the flags (the `clone`
///    register, or `clone_args.flags` in tracee memory for `clone3`), classifies the attempt
///    as a process or a thread (`CLONE_THREAD`), and follows the call to its syscall exit:
///    a negative return is a failed attempt, otherwise the return is the new task's id.
///    `CLONE_UNTRACED` is **stripped** before the call proceeds, so the child is auto-attached
///    like any other; each strip is reported (`untraced_stripped`). If the flags cannot be
///    read or rewritten the call is denied (`-EPERM`) and an error recorded. Any `ptrace`
///    call by a measured task (e.g. `PTRACE_TRACEME`) is denied with `-EPERM` and recorded
///    as an error: measured software must not trace or detach.
/// 2. **Creations, by tracing.** The measured program is traced from before its exec (the
///    stub: `PTRACE_TRACEME`, filter, `SIGSTOP`, then `execve` with the observer's
///    environment), so its loader and constructors run traced, and every descendant stays
///    traced (`TRACEFORK|TRACEVFORK|TRACECLONE|TRACEEXEC|EXITKILL`) until it exits; the
///    observer is a child subreaper and waits until no task is left. A creation event
///    creates a process when the new task is its own thread-group leader (`/proc` `Tgid`).
///
/// Every successful attempt's new task must be a traced creation of the same kind, and
/// every traced creation must come from a successful attempt. Any disagreement or
/// uncertainty — an unreadable status or register set, a ptrace error other than a vanished
/// task, a task that stopped without a creation event, an exit of an unknown process, an
/// attempt whose outcome was never seen, a measured `ptrace`, or a program that never
/// reached its exec — is listed under `observed.errors`, and the observer exits 3.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod observer {
    use std::collections::{BTreeMap, BTreeSet};
    use std::ffi::CString;
    use std::io::Read;
    use std::process::{Command, ExitCode, Stdio};

    use nix::errno::Errno;
    use nix::libc;
    use nix::sys::ptrace::{self, Event, Options};
    use nix::sys::signal::{raise, Signal};
    use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
    use nix::unistd::Pid;
    use serde_json::{json, Value};

    /// The first argument of the traced stub.
    pub const STUB: &str = "--observe-stub";

    /// The syscalls the stub's filter hands to the observer.
    const TRACED: [i64; 5] = [
        libc::SYS_clone,
        libc::SYS_clone3,
        libc::SYS_fork,
        libc::SYS_vfork,
        libc::SYS_ptrace,
    ];
    const CLONE_THREAD: u64 = libc::CLONE_THREAD as u64;
    const CLONE_UNTRACED: u64 = libc::CLONE_UNTRACED as u64;

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

    /// The filter: trace the creation and ptrace syscalls, allow everything else.
    fn filter() -> Result<seccompiler::BpfProgram, Box<dyn std::error::Error>> {
        let rules = TRACED.iter().map(|n| (*n, Vec::new())).collect();
        Ok(seccompiler::SeccompFilter::new(
            rules,
            seccompiler::SeccompAction::Allow,
            seccompiler::SeccompAction::Trace(0),
            std::env::consts::ARCH.try_into()?,
        )?
        .try_into()?)
    }

    /// The stub: become traced, install the filter, stop, then exec the measured program with
    /// the observer's environment (`/proc/<parent>/environ`). It has no environment of its
    /// own, so nothing is preloaded into it. Each failure exits with its own status, which the
    /// observer reports: 124 filter, 125 traceme, 126 setup.
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
        let Ok(filter) = filter() else {
            return ExitCode::from(124);
        };
        if ptrace::traceme().is_err() {
            return ExitCode::from(125);
        }
        if seccompiler::apply_filter(&filter).is_err() {
            return ExitCode::from(124);
        }
        if raise(Signal::SIGSTOP).is_err() {
            return ExitCode::from(126);
        }
        let _ = nix::unistd::execve(path, &argv, &env);
        ExitCode::from(127)
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Kind {
        Process,
        Thread,
    }

    impl Kind {
        fn name(self) -> &'static str {
            match self {
                Kind::Process => "process",
                Kind::Thread => "thread",
            }
        }
    }

    #[derive(Default)]
    struct Tally {
        processes: Vec<i32>,
        threads: Vec<i32>,
        events: BTreeMap<&'static str, usize>,
        attempts: BTreeMap<&'static str, usize>,
        failed_attempts: usize,
        untraced_stripped: usize,
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
            "method": "seccomp RET_TRACE on clone/clone3/fork/vfork/ptrace (attempts, CLONE_UNTRACED stripped) + PTRACE_TRACEME before exec, every descendant traced to exit, child subreaper (creations)",
            "program": program,
            "creation_attempts": tally.attempts,
            "failed_attempts": tally.failed_attempts,
            "untraced_stripped": tally.untraced_stripped,
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

    /// A pending creation attempt of one task: its kind and, once its creation event was
    /// seen, the new task.
    struct Attempt {
        kind: Kind,
        created: Option<i32>,
    }

    struct State {
        tally: Tally,
        processes: BTreeSet<i32>,
        threads: BTreeSet<i32>,
        /// Created tasks whose first `SIGSTOP` is still to come.
        announced: BTreeSet<i32>,
        /// Tasks that stopped before their creation event.
        stopped_unannounced: BTreeSet<i32>,
        /// Tasks inside a traced creation syscall, followed to its exit.
        pending: BTreeMap<i32, Attempt>,
        /// New tasks of successful attempts, by kind.
        succeeded: Vec<(Kind, i32)>,
    }

    impl State {
        /// Continue `pid` (to its syscall exit while an attempt is pending), recording a
        /// ptrace failure: a vanished task (`ESRCH`) is counted, anything else is an error.
        fn resume(&mut self, pid: Pid, signal: Option<Signal>) {
            let result = if self.pending.contains_key(&pid.as_raw()) {
                ptrace::syscall(pid, signal)
            } else {
                ptrace::cont(pid, signal)
            };
            match result {
                Ok(()) => {}
                Err(Errno::ESRCH) => self.tally.vanished += 1,
                Err(e) => self.tally.errors.push(format!("resume {pid}: {e}")),
            }
        }

        /// Deny the syscall at a seccomp stop: skip it and return `-EPERM`.
        fn deny(&mut self, pid: Pid, mut regs: libc::user_regs_struct) {
            regs.orig_rax = u64::MAX;
            regs.rax = (-i64::from(libc::EPERM)) as u64;
            if let Err(e) = ptrace::setregs(pid, regs) {
                self.tally.errors.push(format!("deny at {pid}: {e}"));
            }
        }

        /// A seccomp stop: one traced syscall of `pid`, before it runs.
        fn seccomp(&mut self, pid: Pid) {
            let mut regs = match ptrace::getregs(pid) {
                Ok(regs) => regs,
                Err(e) => {
                    self.tally.errors.push(format!("registers of {pid}: {e}"));
                    return;
                }
            };
            let nr = regs.orig_rax as i64;
            let flags = match nr {
                n if n == libc::SYS_fork || n == libc::SYS_vfork => Some(0),
                n if n == libc::SYS_clone => {
                    let flags = regs.rdi;
                    if flags & CLONE_UNTRACED != 0 {
                        regs.rdi &= !CLONE_UNTRACED;
                        match ptrace::setregs(pid, regs) {
                            Ok(()) => self.tally.untraced_stripped += 1,
                            Err(e) => {
                                self.tally
                                    .errors
                                    .push(format!("strip CLONE_UNTRACED at {pid}: {e}"));
                                self.deny(pid, regs);
                                return;
                            }
                        }
                    }
                    Some(flags)
                }
                n if n == libc::SYS_clone3 => {
                    let at = regs.rdi as ptrace::AddressType;
                    match ptrace::read(pid, at) {
                        Ok(word) => {
                            let flags = word as u64;
                            if flags & CLONE_UNTRACED != 0 {
                                let stripped = (flags & !CLONE_UNTRACED) as libc::c_long;
                                match ptrace::write(pid, at, stripped) {
                                    Ok(()) => self.tally.untraced_stripped += 1,
                                    Err(e) => {
                                        self.tally.errors.push(format!(
                                            "strip clone3 CLONE_UNTRACED at {pid}: {e}"
                                        ));
                                        self.deny(pid, regs);
                                        return;
                                    }
                                }
                            }
                            Some(flags)
                        }
                        Err(e) => {
                            self.tally
                                .errors
                                .push(format!("clone3 flags of {pid}: {e}"));
                            self.deny(pid, regs);
                            return;
                        }
                    }
                }
                n if n == libc::SYS_ptrace => {
                    self.tally.errors.push(format!(
                        "measured task {pid} called ptrace(request {}): denied",
                        regs.rdi as i64
                    ));
                    self.deny(pid, regs);
                    return;
                }
                other => {
                    self.tally
                        .errors
                        .push(format!("unexpected seccomp stop of {pid}: syscall {other}"));
                    return;
                }
            };
            let kind = match flags {
                Some(f) if f & CLONE_THREAD != 0 => Kind::Thread,
                _ => Kind::Process,
            };
            *self.tally.attempts.entry(kind.name()).or_default() += 1;
            if self
                .pending
                .insert(
                    pid.as_raw(),
                    Attempt {
                        kind,
                        created: None,
                    },
                )
                .is_some()
            {
                self.tally
                    .errors
                    .push(format!("{pid} entered a creation syscall inside another"));
            }
        }

        /// A syscall stop: the exit of `pid`'s pending creation syscall.
        fn syscall_exit(&mut self, pid: Pid) {
            let Some(attempt) = self.pending.remove(&pid.as_raw()) else {
                self.tally
                    .errors
                    .push(format!("unexpected syscall stop of {pid}"));
                return;
            };
            match ptrace::getregs(pid) {
                Ok(regs) if (regs.rax as i64) < 0 => self.tally.failed_attempts += 1,
                Ok(regs) => {
                    let new = i32::try_from(regs.rax).unwrap_or(-1);
                    if attempt.created.is_some_and(|c| c != new) {
                        self.tally.errors.push(format!(
                            "{pid}: creation event {:?} but syscall returned {new}",
                            attempt.created
                        ));
                    }
                    self.succeeded.push((attempt.kind, new));
                }
                Err(e) => self.tally.errors.push(format!("return of {pid}: {e}")),
            }
        }

        /// A fork/vfork/clone event of `pid`: the new task, classified by its own `Tgid`.
        fn created(&mut self, pid: Pid, label: &'static str) {
            let new = match ptrace::getevent(pid).map(|n| i32::try_from(n).unwrap_or(-1)) {
                Ok(new) if new > 0 => new,
                other => {
                    self.tally
                        .errors
                        .push(format!("{label} event of {pid}: new task {other:?}"));
                    return;
                }
            };
            let process = match label {
                "clone" => match tgid(Pid::from_raw(new)) {
                    Some(group) => group == new,
                    None => {
                        self.tally
                            .errors
                            .push(format!("unreadable status of new task {new}"));
                        true
                    }
                },
                _ => true,
            };
            let label = match (label, process) {
                ("clone", true) => "clone_process",
                ("clone", false) => "clone_thread",
                (other, _) => other,
            };
            *self.tally.events.entry(label).or_default() += 1;
            if process {
                self.processes.insert(new);
                self.tally.processes.push(new);
            } else {
                self.threads.insert(new);
                self.tally.threads.push(new);
            }
            match self.pending.get_mut(&pid.as_raw()) {
                Some(attempt) => attempt.created = Some(new),
                None => self
                    .tally
                    .errors
                    .push(format!("{label} event of {pid} without a traced attempt")),
            }
            if self.stopped_unannounced.remove(&new) {
                self.resume(Pid::from_raw(new), None);
            } else {
                self.announced.insert(new);
            }
        }
    }

    /// Trace `root` (stopped by its own `SIGSTOP` after `PTRACE_TRACEME` and the filter) and
    /// every task it creates until no task is left; the root's exit status and the tally.
    fn observe(root: Pid) -> Result<(Option<i32>, Tally), String> {
        match waitpid(root, Some(WaitPidFlag::__WALL)) {
            Ok(WaitStatus::Stopped(_, Signal::SIGSTOP)) => {}
            Ok(WaitStatus::Exited(_, code)) => {
                return Err(format!(
                    "the stub could not set up tracing (exit {code}: {})",
                    match code {
                        124 => "seccomp filter",
                        125 => "PTRACE_TRACEME refused",
                        _ => "setup",
                    }
                ))
            }
            other => return Err(format!("the stub did not stop for tracing: {other:?}")),
        }
        let options = Options::PTRACE_O_TRACEFORK
            | Options::PTRACE_O_TRACEVFORK
            | Options::PTRACE_O_TRACECLONE
            | Options::PTRACE_O_TRACEEXEC
            | Options::PTRACE_O_TRACESECCOMP
            | Options::PTRACE_O_TRACESYSGOOD
            | Options::PTRACE_O_EXITKILL;
        ptrace::setoptions(root, options).map_err(|e| format!("setoptions: {e}"))?;
        let mut s = State {
            tally: Tally::default(),
            processes: BTreeSet::from([root.as_raw()]),
            threads: BTreeSet::new(),
            announced: BTreeSet::new(),
            stopped_unannounced: BTreeSet::new(),
            pending: BTreeMap::new(),
            succeeded: Vec::new(),
        };
        let mut root_exec = false;
        let mut exit = None;
        s.resume(root, None);
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
                    if let Some(attempt) = s.pending.remove(&raw) {
                        // Killed inside the syscall: its outcome is its creation event.
                        match attempt.created {
                            Some(new) => s.succeeded.push((attempt.kind, new)),
                            None => s.tally.errors.push(format!(
                                "{raw} exited inside a creation syscall with no outcome"
                            )),
                        }
                    }
                    if s.processes.contains(&raw) {
                        s.tally.reaped += 1;
                    } else if !s.threads.contains(&raw) {
                        s.tally.errors.push(format!("reaped unknown task {raw}"));
                    }
                }
                WaitStatus::PtraceSyscall(pid) => {
                    s.syscall_exit(pid);
                    s.resume(pid, None);
                }
                WaitStatus::PtraceEvent(pid, _, event) => {
                    match event {
                        e if e == Event::PTRACE_EVENT_SECCOMP as i32 => s.seccomp(pid),
                        e if e == Event::PTRACE_EVENT_FORK as i32 => s.created(pid, "fork"),
                        e if e == Event::PTRACE_EVENT_VFORK as i32 => s.created(pid, "vfork"),
                        e if e == Event::PTRACE_EVENT_CLONE as i32 => s.created(pid, "clone"),
                        e if e == Event::PTRACE_EVENT_EXEC as i32 => {
                            if pid == root {
                                root_exec = true;
                            }
                            *s.tally.events.entry("exec").or_default() += 1;
                        }
                        _ => {}
                    }
                    s.resume(pid, None);
                }
                WaitStatus::Stopped(pid, signal) => {
                    let raw = pid.as_raw();
                    let known = s.processes.contains(&raw) || s.threads.contains(&raw);
                    if signal == Signal::SIGSTOP && s.announced.remove(&raw) {
                        s.resume(pid, None);
                    } else if signal == Signal::SIGSTOP && !known {
                        // Its creation event is still to come.
                        s.stopped_unannounced.insert(raw);
                    } else {
                        s.resume(pid, Some(signal));
                    }
                }
                other => {
                    if let Some(pid) = other.pid() {
                        s.resume(pid, None);
                    }
                }
            }
        }
        if !root_exec {
            s.tally
                .errors
                .push("the measured program was never exec'd under trace".into());
        }
        if !s.stopped_unannounced.is_empty() {
            let left = std::mem::take(&mut s.stopped_unannounced);
            s.tally
                .errors
                .push(format!("tasks stopped without a creation event: {left:?}"));
        }
        if !s.pending.is_empty() {
            let left: Vec<i32> = s.pending.keys().copied().collect();
            s.tally
                .errors
                .push(format!("creation attempts never completed: {left:?}"));
        }
        // The two counts must agree: each successful attempt is one traced creation of its
        // kind, and each traced creation came from a successful attempt.
        let traced: BTreeSet<(bool, i32)> = s
            .tally
            .processes
            .iter()
            .map(|p| (true, *p))
            .chain(s.tally.threads.iter().map(|t| (false, *t)))
            .collect();
        let attempted: BTreeSet<(bool, i32)> = s
            .succeeded
            .iter()
            .map(|(kind, new)| (*kind == Kind::Process, *new))
            .collect();
        if traced != attempted || attempted.len() != s.succeeded.len() {
            s.tally.errors.push(format!(
                "attempts and creations disagree: only attempted {:?}, only traced {:?}",
                attempted.difference(&traced).collect::<Vec<_>>(),
                traced.difference(&attempted).collect::<Vec<_>>()
            ));
        }
        Ok((exit, s.tally))
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn main() -> std::process::ExitCode {
    spawner::main()
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn main() {}
