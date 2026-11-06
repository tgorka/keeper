//! One single-use worker run, supervised (contract §5 *Execution boundary*;
//! ported from the reviewed 110.1 proof).
//!
//! A run checks the runtime class before anything exists, spawns the
//! worker with the fixed environment and descriptors, streams its one
//! bounded input, reads at most [`MAX_RESULT_BYTES`] of its answer and
//! reaps it on every path — success, refusal, deadline, cancellation or a
//! domain's revocation, which kill the actual process before the run
//! returns. The deadline covers launch, setup, transfer, native work and
//! exit. An exit status decides the outcome, never a broken pipe: 72 and 75
//! are `query_limit`, 74 with an execution-class or platform label is
//! `projection_unavailable` with that label, anything else unavailable.

use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::sync::watch;

use super::admission::Admission;
use super::protocol::profile::MAX_RESULT_BYTES;
use super::protocol::worker::{
    Response, EXECUTION_CLASS_LABELS, EXECUTION_CLASS_REFUSED, EXIT_BUDGET, EXIT_LIMIT,
    EXIT_REFUSED, MAX_EXPORT_BYTES, PLATFORM_REFUSED,
};
use super::query::QueryError;
use super::runtime::Runtime;

/// A query's wall deadline.
pub const QUERY_DEADLINE: Duration = Duration::from_secs(3);
/// A build's wall deadline.
pub const BUILD_DEADLINE: Duration = Duration::from_secs(180);

/// Cancels the runs it is given to: firing it kills and reaps their
/// workers.
#[derive(Clone)]
pub struct Cancellation(watch::Sender<bool>);

impl Default for Cancellation {
    fn default() -> Self {
        Self(watch::channel(false).0)
    }
}

impl Cancellation {
    pub fn cancel(&self) {
        self.0.send_replace(true);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        *self.0.borrow()
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<bool> {
        self.0.subscribe()
    }
}

/// What a run waits on besides its own work: the caller's cancellation and
/// the domain's kill switch (`true` once revoked).
pub(crate) struct Stops {
    cancel: watch::Receiver<bool>,
    revoked: watch::Receiver<bool>,
    open: (bool, bool),
}

impl Stops {
    pub(crate) fn new(cancel: &Cancellation, revoked: watch::Receiver<bool>) -> Self {
        Self {
            cancel: cancel.subscribe(),
            revoked,
            open: (true, true),
        }
    }

    pub(crate) fn stopped(&self) -> Option<QueryError> {
        if *self.revoked.borrow() {
            Some(QueryError::Forbidden)
        } else if *self.cancel.borrow() {
            Some(QueryError::Cancelled)
        } else {
            None
        }
    }

    pub(crate) async fn changed(&mut self) -> QueryError {
        loop {
            tokio::select! {
                r = self.cancel.changed(), if self.open.0 => self.open.0 = r.is_ok(),
                r = self.revoked.changed(), if self.open.1 => self.open.1 = r.is_ok(),
                else => std::future::pending::<()>().await,
            }
            if let Some(stop) = self.stopped() {
                return stop;
            }
        }
    }
}

/// The label a worker's exit-74 answer names, when it is one of the fixed
/// startup refusals.
fn startup_label(out: &[u8]) -> &'static str {
    let text = std::str::from_utf8(out)
        .unwrap_or_default()
        .trim_end_matches('\n');
    if text == PLATFORM_REFUSED {
        return "worker.platform";
    }
    if let Some(label) = text.strip_prefix(EXECUTION_CLASS_REFUSED) {
        if let Some(found) = EXECUTION_CLASS_LABELS.iter().find(|l| **l == label) {
            return match *found {
                "no_new_privs" => "worker.class.no_new_privs",
                "auxv" => "worker.class.auxv",
                "at_secure" => "worker.class.at_secure",
                "uid" => "worker.class.uid",
                _ => "worker.class.gid",
            };
        }
    }
    "worker.refused"
}

/// Run one worker `role` over `path` with `input` within `deadline`.
pub(crate) async fn run<R: AsyncRead + Unpin>(
    runtime: &Runtime,
    admission: &Admission,
    role: &str,
    path: &std::path::Path,
    input: R,
    deadline: Duration,
    mut stops: Stops,
) -> Result<Response, QueryError> {
    let deadline = tokio::time::Instant::now() + deadline;
    if let Some(stop) = stops.stopped() {
        return Err(stop);
    }
    runtime.before_spawn()?;
    let mut command = runtime.command(role, path)?;
    let mut child = runtime.spawn(&mut command)?;
    let _live = admission.live();
    let Some(pid) = child.id() else {
        let _ = child.start_kill();
        let _ = child.wait().await;
        return Err(QueryError::Unavailable("worker.pid"));
    };
    let result = exchange(&mut child, pid, input, deadline, &mut stops).await;
    if result.is_err() {
        let _ = child.start_kill();
    }
    // Every path reaps the child before reporting, success included.
    let waited = child.wait().await;
    waited.map_err(|_| QueryError::Unavailable("worker.wait"))?;
    result
}

async fn exchange<R: AsyncRead + Unpin>(
    child: &mut tokio::process::Child,
    pid: u32,
    input: R,
    deadline: tokio::time::Instant,
    stops: &mut Stops,
) -> Result<Response, QueryError> {
    let mut stdin = child
        .stdin
        .take()
        .ok_or(QueryError::Unavailable("worker.stdin"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or(QueryError::Unavailable("worker.stdout"))?;
    let execute = async {
        // A worker that exits early closes its stdin; its exit status, not
        // the broken pipe, decides the outcome.
        let write = async {
            let _ = tokio::io::copy(&mut input.take(MAX_EXPORT_BYTES + 1), &mut stdin).await;
            let _ = stdin.shutdown().await;
            drop(stdin);
        };
        let read = async {
            let mut out = Vec::new();
            stdout
                .take(MAX_RESULT_BYTES as u64 + 1)
                .read_to_end(&mut out)
                .await
                .map(|_| out)
        };
        let ((), bytes) = tokio::join!(write, read);
        let bytes = bytes.map_err(|_| QueryError::Unavailable("worker.read"))?;
        if bytes.len() > MAX_RESULT_BYTES {
            return Err(QueryError::Unavailable("worker.answer"));
        }
        let status = child
            .wait()
            .await
            .map_err(|_| QueryError::Unavailable("worker.wait"))?;
        if !status.success() {
            return Err(match status.code() {
                Some(EXIT_BUDGET | EXIT_LIMIT) => QueryError::Limit,
                Some(EXIT_REFUSED) => QueryError::Unavailable(startup_label(&bytes)),
                _ => QueryError::Unavailable("worker.exit"),
            });
        }
        let response: Response =
            serde_json::from_slice(&bytes).map_err(|_| QueryError::Unavailable("worker.answer"))?;
        if response.pid != pid {
            return Err(QueryError::Unavailable("worker.answer"));
        }
        Ok(response)
    };
    tokio::select! {
        result = tokio::time::timeout_at(deadline, execute) => result.unwrap_or(Err(QueryError::Limit)),
        stop = stops.changed() => Err(stop),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_labels_are_read_only_from_the_fixed_lines() {
        assert_eq!(
            startup_label(b"platform prerequisite refused\n"),
            "worker.platform"
        );
        assert_eq!(
            startup_label(b"execution class refused: at_secure\n"),
            "worker.class.at_secure"
        );
        assert_eq!(
            startup_label(b"execution class refused: no_new_privs\n"),
            "worker.class.no_new_privs"
        );
        assert_eq!(
            startup_label(b"execution class refused: other\n"),
            "worker.refused"
        );
        assert_eq!(startup_label(b"{\"rows\":[]}"), "worker.refused");
    }
}
