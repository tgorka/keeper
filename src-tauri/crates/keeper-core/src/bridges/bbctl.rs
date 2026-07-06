//! The `bbctl` self-hosted-bridge runner seam (Story 6.7, FR-29, AD-16).
//!
//! A [`BbctlRunner`] launches the `bbctl` CLI as a launch-on-demand sidecar and
//! streams its output lines to a caller-supplied `on_line` sink. Its production impl
//! ([`crate::bridges`]'s `DesktopBbctlRunner` in the shell) spawns the resolved
//! binary via `tokio::process`; tests provide a `FakeBbctlRunner`. Following
//! [`crate::bridges::transport::BridgeTransport`], the trait uses a native `async fn`
//! returning `impl Future + Send`, dispatched **statically** via the generic
//! orchestrator [`run_self_hosted`] — no `async-trait`, no trait object, so no
//! `async_fn_in_trait` clippy warning and no dynamic dispatch.
//!
//! **Streaming contract (load-bearing).** `bbctl run` starts a **persistent bridge
//! daemon that never exits** (no stdout/stderr EOF). The runner therefore MUST NOT
//! await the process to completion before classifying: it streams each line to
//! `on_line` **as it arrives**; when `on_line` returns [`LineControl::Stop`] the
//! runner stops reading and resolves promptly as [`BbctlRunExit::StoppedEarly`]
//! **without** waiting for exit and **without** killing the child (launch-and-leave,
//! v1.x — no supervision, no restart, no log viewer). The parser matches
//! **human-readable prose** substrings only, so [`bbctl_args`] must **not** request
//! `--json` (the two would disagree).

use std::future::Future;

use crate::error::BridgeError;
use crate::vm::{BbctlPhase, BbctlProgressVm};

/// The maximum length (chars) of a surfaced `bbctl` error message. `bbctl` could
/// emit an arbitrarily large line; capping it keeps an unbounded message from
/// reaching the VM/DOM verbatim (mirrors the provisioning/bot caps).
const MAX_MESSAGE_CHARS: usize = 2000;

/// Truncate a surfaced message to [`MAX_MESSAGE_CHARS`] on a char boundary
/// (`chars().take(..)` never splits a codepoint).
pub fn cap_message(msg: &str) -> String {
    msg.chars().take(MAX_MESSAGE_CHARS).collect::<String>()
}

/// The control value an `on_line` sink returns after each streamed line: keep
/// reading, or stop promptly (leaving the child alive — see [`BbctlRunner::run`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineControl {
    /// Keep reading further lines.
    Continue,
    /// Stop reading now; the runner resolves [`BbctlRunExit::StoppedEarly`] without
    /// waiting for process exit and without killing the child.
    Stop,
}

/// How a [`BbctlRunner::run`] resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BbctlRunExit {
    /// The process reached EOF on both streams and exited with this status code.
    Exited(i32),
    /// `on_line` returned [`LineControl::Stop`]; the runner stopped reading early
    /// (the child may still be running — for `bbctl run` this is the happy path).
    StoppedEarly,
}

/// A launch-on-demand `bbctl` sidecar runner (Story 6.7, AD-16).
///
/// Native `async fn` dispatched statically via [`run_self_hosted`] — no
/// `async-trait`, no trait object. See the module docs for the streaming contract.
pub trait BbctlRunner {
    /// Whether the `bbctl` sidecar can be resolved on this host/build. `false` is
    /// the guided-install path — never an error.
    fn is_available(&self) -> bool;

    /// Run `bbctl` with `args`, streaming **each** output line (stdout AND stderr,
    /// merged) to `on_line` **as it arrives**. When `on_line` returns
    /// [`LineControl::Stop`] the runner stops reading and resolves
    /// [`BbctlRunExit::StoppedEarly`] promptly — it does NOT `wait()` for exit and
    /// does NOT kill the child. A natural EOF resolves [`BbctlRunExit::Exited`].
    /// A spawn/pipe failure resolves [`BridgeError::Bbctl`]. A single bad
    /// (non-UTF-8) line is skipped, not treated as EOF.
    fn run(
        &self,
        args: Vec<String>,
        on_line: Box<dyn FnMut(&str) -> LineControl + Send>,
    ) -> impl Future<Output = Result<BbctlRunExit, BridgeError>> + Send;
}

/// The sink the orchestrator emits recognized [`BbctlProgressVm`] snapshots through
/// (mirrors [`crate::bridges::login::BridgeLoginSink`]). Returns `false` when the
/// consumer has gone away (a closed channel) — the orchestrator keeps going
/// regardless (the run is launch-and-leave).
pub type BbctlSink = Box<dyn Fn(BbctlProgressVm) -> bool + Send + Sync>;

/// The `bbctl` action being invoked — `register` (register the self-hosted bridge
/// appservice) or `run` (start the persistent bridge daemon).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BbctlAction {
    /// `bbctl register <name>`.
    Register,
    /// `bbctl run <name>`.
    Run,
}

impl BbctlAction {
    /// The `bbctl` subcommand word for this action.
    fn subcommand(self) -> &'static str {
        match self {
            BbctlAction::Register => "register",
            BbctlAction::Run => "run",
        }
    }
}

/// Build the `bbctl` argument vector for `action` on the self-hosted bridge
/// `bbctl_name` (e.g. `["register", "sh-signal"]`). Pure and unit-tested.
///
/// **Deliberately does NOT pass `--json`.** The [`parse_bbctl_event`] parser matches
/// human-readable prose substrings; a `--json` stream would spuriously match
/// "error"/"failed" inside field text and drop the structured stages — so the args
/// and the parser must both speak prose.
pub fn bbctl_args(action: BbctlAction, bbctl_name: &str) -> Vec<String> {
    vec![action.subcommand().to_owned(), bbctl_name.to_owned()]
}

/// Project one `bbctl` output line into a recognized non-terminal [`BbctlPhase`], or
/// `None` when the line carries no recognized marker (unrecognized lines are dropped
/// — there is no path from a raw log line to the UI). Pure and unit-tested.
///
/// The prose marker set is **provisional** — tunable against a real `bbctl` binary
/// (the documented residual risk). Terminal `success`/`failure` are decided by the
/// orchestrator (a started marker → success, an error marker → failure), so this
/// returns only the in-flight `Registering`/`Starting`/`Running` phases.
pub fn parse_bbctl_event(line: &str) -> Option<BbctlPhase> {
    let lower = line.to_lowercase();
    // Order matters: the more specific "running/started" markers win before the
    // generic "registering" ones (a single line rarely carries both).
    if lower.contains("registering") || lower.contains("register bridge") {
        Some(BbctlPhase::Registering)
    } else if lower.contains("starting") || lower.contains("starting bridge") {
        Some(BbctlPhase::Starting)
    } else if lower.contains("running") || lower.contains("bridge is up") {
        Some(BbctlPhase::Running)
    } else {
        None
    }
}

/// Whether a `bbctl run` line marks the bridge as **started** — the launch-and-leave
/// success signal. On this marker the orchestrator sinks [`BbctlPhase::Success`],
/// returns [`LineControl::Stop`], and leaves the daemon running. Pure.
fn is_started_marker(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower.contains("bridge is up")
        || lower.contains("started bridge")
        || lower.contains("now running")
        || lower.contains("connected to homeserver")
}

/// Whether a line marks a `bbctl` **error** — the orchestrator sinks a terminal
/// [`BbctlPhase::Failure`] carrying the capped-verbatim line and returns
/// [`LineControl::Stop`]. Pure.
fn is_error_marker(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower.contains("error") || lower.contains("failed") || lower.contains("fatal")
}

/// Build a non-terminal progress snapshot for `phase` on `network_id`.
fn progress(network_id: &str, phase: BbctlPhase, message: Option<String>) -> BbctlProgressVm {
    BbctlProgressVm {
        network_id: network_id.to_owned(),
        phase,
        message,
        error: None,
    }
}

/// Build a terminal failure snapshot for `network_id` carrying the capped `error`.
fn failure(network_id: &str, error: String) -> BbctlProgressVm {
    BbctlProgressVm {
        network_id: network_id.to_owned(),
        phase: BbctlPhase::Failure,
        message: None,
        error: Some(cap_message(&error)),
    }
}

/// Drive one `bbctl` self-hosted-bridge run to a terminal `sink` state (Story 6.7,
/// FR-29). Statically dispatched over `R: BbctlRunner`.
///
/// Flow: emit `checking`; if the sidecar is absent, sink the honest guided-install
/// `failure` and return. Else run `register` then `run`, **sinking each recognized
/// non-terminal phase incrementally as it arrives**. On the `run` started marker sink
/// `success` and return [`LineControl::Stop`] (leaving the daemon alive); on a
/// recognized error marker sink the capped-verbatim terminal `failure` and stop; a
/// non-zero natural exit with no started marker → an honest failure.
pub async fn run_self_hosted<R: BbctlRunner>(
    runner: &R,
    network_id: &str,
    bbctl_name: &str,
    sink: BbctlSink,
) {
    // Share the sink into the `'static` `on_line` closures (a `Box<dyn FnMut>` is
    // `'static`, so it cannot borrow `sink`).
    let sink = std::sync::Arc::new(sink);
    let _ = sink(progress(network_id, BbctlPhase::Checking, None));

    if !runner.is_available() {
        let _ = sink(failure(
            network_id,
            "bbctl was not found. Install it to run your own bridge.".to_owned(),
        ));
        return;
    }

    // --- register -----------------------------------------------------------
    // `register` runs to completion (it is not a daemon). Sink each recognized
    // in-flight phase as it arrives; capture a recognized error line for the
    // failure message; never stop early (we want its natural exit).
    let register_error: std::sync::Arc<std::sync::Mutex<Option<String>>> =
        std::sync::Arc::new(std::sync::Mutex::new(None));
    let reg_err = register_error.clone();
    let reg_network = network_id.to_owned();
    let reg_sink = sink.clone();
    let on_register = Box::new(move |line: &str| {
        if is_error_marker(line) {
            if let Ok(mut slot) = reg_err.lock() {
                if slot.is_none() {
                    *slot = Some(line.to_owned());
                }
            }
            return LineControl::Stop;
        }
        if let Some(phase) = parse_bbctl_event(line) {
            // Log-free: sink the recognized PHASE only — never the raw line (no raw
            // bbctl output crosses IPC or reaches the UI).
            let _ = reg_sink(progress(&reg_network, phase, None));
        }
        LineControl::Continue
    });

    match runner
        .run(bbctl_args(BbctlAction::Register, bbctl_name), on_register)
        .await
    {
        Ok(BbctlRunExit::Exited(0)) => {
            // register succeeded — fall through to run.
        }
        Ok(BbctlRunExit::StoppedEarly) => {
            // Stopped early only happens on our error marker.
            let msg = register_error
                .lock()
                .ok()
                .and_then(|s| s.clone())
                .unwrap_or_else(|| "bbctl register reported an error".to_owned());
            let _ = sink(failure(network_id, msg));
            return;
        }
        Ok(BbctlRunExit::Exited(code)) => {
            let msg = register_error
                .lock()
                .ok()
                .and_then(|s| s.clone())
                .unwrap_or_else(|| format!("bbctl register exited with code {code}"));
            let _ = sink(failure(network_id, msg));
            return;
        }
        Err(e) => {
            let _ = sink(failure(network_id, e.to_string()));
            return;
        }
    }

    // --- run (launch-and-leave) ---------------------------------------------
    let started: std::sync::Arc<std::sync::atomic::AtomicBool> =
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let run_error: std::sync::Arc<std::sync::Mutex<Option<String>>> =
        std::sync::Arc::new(std::sync::Mutex::new(None));
    let run_started = started.clone();
    let run_err = run_error.clone();
    let run_network = network_id.to_owned();
    let run_sink = sink.clone();
    let on_run = Box::new(move |line: &str| {
        if is_error_marker(line) {
            if let Ok(mut slot) = run_err.lock() {
                if slot.is_none() {
                    *slot = Some(line.to_owned());
                }
            }
            return LineControl::Stop;
        }
        if is_started_marker(line) {
            run_started.store(true, std::sync::atomic::Ordering::Relaxed);
            // Log-free: sink the Success PHASE only — never the raw started line.
            let _ = run_sink(progress(&run_network, BbctlPhase::Success, None));
            // Launch-and-leave: stop reading, leave the daemon alive.
            return LineControl::Stop;
        }
        if let Some(phase) = parse_bbctl_event(line) {
            // Log-free: sink the recognized PHASE only — never the raw line.
            let _ = run_sink(progress(&run_network, phase, None));
        }
        LineControl::Continue
    });

    match runner
        .run(bbctl_args(BbctlAction::Run, bbctl_name), on_run)
        .await
    {
        Ok(BbctlRunExit::StoppedEarly) => {
            if started.load(std::sync::atomic::Ordering::Relaxed) {
                // Success already sunk on the started marker; nothing more to do.
            } else {
                // Stopped early on an error marker.
                let msg = run_error
                    .lock()
                    .ok()
                    .and_then(|s| s.clone())
                    .unwrap_or_else(|| "bbctl run reported an error".to_owned());
                let _ = sink(failure(network_id, msg));
            }
        }
        Ok(BbctlRunExit::Exited(code)) => {
            // A natural exit without a started marker is a failure — the daemon
            // should not have exited on the happy path.
            if !started.load(std::sync::atomic::Ordering::Relaxed) {
                let msg = run_error
                    .lock()
                    .ok()
                    .and_then(|s| s.clone())
                    .unwrap_or_else(|| format!("bbctl run exited with code {code}"));
                let _ = sink(failure(network_id, msg));
            }
        }
        Err(e) => {
            if !started.load(std::sync::atomic::Ordering::Relaxed) {
                let _ = sink(failure(network_id, e.to_string()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    // --- pure helpers -------------------------------------------------------

    #[test]
    fn bbctl_args_never_passes_json() {
        let args = bbctl_args(BbctlAction::Register, "sh-signal");
        assert_eq!(args, vec!["register".to_owned(), "sh-signal".to_owned()]);
        assert!(
            !args.iter().any(|a| a == "--json"),
            "bbctl_args must NOT pass --json (parser matches prose)"
        );
        let run = bbctl_args(BbctlAction::Run, "sh-whatsapp");
        assert_eq!(run, vec!["run".to_owned(), "sh-whatsapp".to_owned()]);
    }

    #[test]
    fn parse_bbctl_event_recognizes_prose_and_drops_the_rest() {
        assert_eq!(
            parse_bbctl_event("Registering bridge appservice"),
            Some(BbctlPhase::Registering)
        );
        assert_eq!(
            parse_bbctl_event("Starting bridge sh-signal"),
            Some(BbctlPhase::Starting)
        );
        assert_eq!(
            parse_bbctl_event("Bridge is running now"),
            Some(BbctlPhase::Running)
        );
        // Unrecognized prose is dropped — never surfaced.
        assert_eq!(parse_bbctl_event("2026-07-05T10:00:00Z [debug] tick"), None);
        assert_eq!(parse_bbctl_event(""), None);
    }

    #[test]
    fn cap_message_truncates_on_char_boundary() {
        let long = "x".repeat(MAX_MESSAGE_CHARS + 500);
        assert_eq!(cap_message(&long).chars().count(), MAX_MESSAGE_CHARS);
        // Multi-byte chars are never split.
        let multi = "é".repeat(MAX_MESSAGE_CHARS + 10);
        assert_eq!(cap_message(&multi).chars().count(), MAX_MESSAGE_CHARS);
    }

    // --- FakeBbctlRunner + I/O matrix --------------------------------------

    /// A scripted runner: for each `run` call it replays `lines` through `on_line`,
    /// honoring an early `Stop` (it stops replaying and resolves `StoppedEarly`,
    /// exactly as the real launch-and-leave runner would — it does NOT keep
    /// streaming past a `Stop`). If every line is consumed it resolves with the
    /// scripted exit code. Scripts are keyed by call order (register, then run).
    struct FakeBbctlRunner {
        available: bool,
        /// One `(lines, exit_code)` script per `run` call, in call order.
        scripts: Mutex<std::collections::VecDeque<(Vec<String>, i32)>>,
    }

    impl FakeBbctlRunner {
        fn new(available: bool, scripts: Vec<(Vec<&str>, i32)>) -> Self {
            let scripts = scripts
                .into_iter()
                .map(|(lines, code)| {
                    (
                        lines.into_iter().map(str::to_owned).collect::<Vec<_>>(),
                        code,
                    )
                })
                .collect();
            Self {
                available,
                scripts: Mutex::new(scripts),
            }
        }
    }

    impl BbctlRunner for FakeBbctlRunner {
        fn is_available(&self) -> bool {
            self.available
        }

        async fn run(
            &self,
            _args: Vec<String>,
            mut on_line: Box<dyn FnMut(&str) -> LineControl + Send>,
        ) -> Result<BbctlRunExit, BridgeError> {
            let (lines, code) = {
                let mut scripts = self.scripts.lock().expect("scripts lock");
                scripts
                    .pop_front()
                    .expect("FakeBbctlRunner: no scripted run left")
            };
            for line in &lines {
                if on_line(line) == LineControl::Stop {
                    // Honor early stop: stop replaying, leave the "child" alive.
                    return Ok(BbctlRunExit::StoppedEarly);
                }
            }
            Ok(BbctlRunExit::Exited(code))
        }
    }

    /// Collect every sunk VM for assertions.
    fn recording_sink() -> (BbctlSink, Arc<Mutex<Vec<BbctlProgressVm>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_for_sink = seen.clone();
        let sink: BbctlSink = Box::new(move |vm| {
            seen_for_sink.lock().expect("sink lock").push(vm);
            true
        });
        (sink, seen)
    }

    fn phases(seen: &Arc<Mutex<Vec<BbctlProgressVm>>>) -> Vec<BbctlPhase> {
        seen.lock()
            .expect("seen lock")
            .iter()
            .map(|vm| vm.phase)
            .collect()
    }

    #[tokio::test]
    async fn happy_path_registers_then_runs_and_stops_on_started_marker() {
        // register exits 0; run streams a started marker (the fake would keep going
        // afterwards, but the orchestrator must stop and leave the child alive).
        let runner = FakeBbctlRunner::new(
            true,
            vec![
                (vec!["Registering bridge appservice", "done"], 0),
                (
                    vec![
                        "Starting bridge sh-signal",
                        "Bridge is up and connected to homeserver",
                        // If the orchestrator failed to Stop, this later line would
                        // still be classified — it must NOT be.
                        "some later chatter that should never be read",
                    ],
                    0,
                ),
            ],
        );
        let (sink, seen) = recording_sink();
        run_self_hosted(&runner, "signal", "sh-signal", sink).await;

        let phases = phases(&seen);
        assert_eq!(phases.first(), Some(&BbctlPhase::Checking));
        assert!(phases.contains(&BbctlPhase::Registering));
        assert!(phases.contains(&BbctlPhase::Starting));
        assert_eq!(
            phases.last(),
            Some(&BbctlPhase::Success),
            "the started marker must resolve to Success"
        );
        // No failure ever emitted on the happy path.
        assert!(!phases.contains(&BbctlPhase::Failure));
    }

    #[tokio::test]
    async fn absent_sidecar_sinks_guided_install_failure() {
        let runner = FakeBbctlRunner::new(false, vec![]);
        let (sink, seen) = recording_sink();
        run_self_hosted(&runner, "signal", "sh-signal", sink).await;

        let seen = seen.lock().expect("seen lock");
        assert_eq!(seen[0].phase, BbctlPhase::Checking);
        let last = seen.last().expect("a terminal vm");
        assert_eq!(last.phase, BbctlPhase::Failure);
        assert!(
            last.error.as_deref().unwrap_or_default().contains("bbctl"),
            "the failure must name bbctl (guided install): {:?}",
            last.error
        );
    }

    #[tokio::test]
    async fn register_ok_but_run_fails_reports_failure_not_success() {
        let runner = FakeBbctlRunner::new(
            true,
            vec![
                (vec!["Registering bridge"], 0),
                (vec!["Starting bridge", "error: could not connect"], 0),
            ],
        );
        let (sink, seen) = recording_sink();
        run_self_hosted(&runner, "signal", "sh-signal", sink).await;

        let phases = phases(&seen);
        assert!(!phases.contains(&BbctlPhase::Success), "no fake success");
        assert_eq!(phases.last(), Some(&BbctlPhase::Failure));
        let seen = seen.lock().expect("seen lock");
        let err = seen
            .last()
            .and_then(|vm| vm.error.clone())
            .unwrap_or_default();
        assert!(err.contains("could not connect"), "verbatim error: {err}");
    }

    #[tokio::test]
    async fn register_failure_stops_before_run() {
        let runner = FakeBbctlRunner::new(
            true,
            // Only ONE script: register errors, so `run` must never be invoked (a
            // second script would panic in the fake if it were).
            vec![(vec!["fatal: bbctl not logged in"], 0)],
        );
        let (sink, seen) = recording_sink();
        run_self_hosted(&runner, "signal", "sh-signal", sink).await;

        let phases = phases(&seen);
        assert_eq!(phases.last(), Some(&BbctlPhase::Failure));
        assert!(!phases.contains(&BbctlPhase::Success));
    }

    #[tokio::test]
    async fn run_nonzero_exit_without_started_marker_is_failure() {
        let runner = FakeBbctlRunner::new(
            true,
            vec![
                (vec!["Registering bridge"], 0),
                // No started marker, natural non-zero exit.
                (vec!["Starting bridge"], 1),
            ],
        );
        let (sink, seen) = recording_sink();
        run_self_hosted(&runner, "signal", "sh-signal", sink).await;

        let phases = phases(&seen);
        assert_eq!(phases.last(), Some(&BbctlPhase::Failure));
        assert!(!phases.contains(&BbctlPhase::Success));
    }

    #[tokio::test]
    async fn unrecognized_lines_are_dropped_never_sunk() {
        let runner = FakeBbctlRunner::new(
            true,
            vec![
                (vec!["some noise", "more noise"], 0),
                (vec!["debug: tick", "Bridge is up now", "trailing"], 0),
            ],
        );
        let (sink, seen) = recording_sink();
        run_self_hosted(&runner, "signal", "sh-signal", sink).await;

        // Only Checking + Success were recognizable; the noise never produced a VM.
        let phases = phases(&seen);
        assert_eq!(phases, vec![BbctlPhase::Checking, BbctlPhase::Success]);
    }
}
