//! One-time verified copy, as the app sees it (Epic 33, AD-C1..AD-C6).
//!
//! A copy is a **job**, never a relationship: it has a lifecycle, a per-file
//! report, and finishing it changes nothing about the folders it touched. It is
//! never written into `profiles` and never joins the sync journal (AD-C1).
//!
//! The engine ([`keeper_sync::copy_verified`]) hashes every byte on the way out
//! and reads the destination back to prove it matches, so "copied" here means
//! verified rather than "write returned Ok" (AD-C2). That second read is why a
//! job is slow in a way `cp` is not, and why its result is worth showing.
//!
//! Status is polled rather than streamed. The Sync view already polls its three
//! lists, the engine coalesces progress to ~10 Hz anyway, and a poll needs no
//! channel lifecycle to get wrong — a subscription whose webview vanished is
//! the bug class this avoids entirely.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use keeper_core::vm::{IpcError, IpcErrorCode};
use keeper_sync::copy::{CopyOptions, CopyOutcome, CopyProgress, CopyReport};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ipc::AppState;

/// How long a finished job stays readable before it is swept.
///
/// A terminal job must outlive the poll that will ask for it, or the surface
/// that started the copy would see it vanish and have nothing to show. Ten
/// minutes is far longer than any poll interval and short enough that a long
/// session does not accumulate reports nobody will read.
const RETAIN_TERMINAL_MS: u64 = 10 * 60 * 1_000;

/// Where a job is in its life (AD-C1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub enum CopyJobState {
    Copying,
    /// Every file is written; the destination is being read back.
    Verifying,
    Done,
    Failed,
    Cancelled,
}

/// One file's outcome, flattened for the wire.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct CopyEntryVm {
    /// Source-relative, so the report reads as the user's own tree.
    pub path: String,
    #[ts(type = "number")]
    pub bytes: u64,
    /// `copied` | `identical` | `collision` | `failed`.
    pub outcome: String,
    /// Present only for `failed`.
    pub reason: Option<String>,
}

/// A job as the UI polls it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct CopyJobVm {
    pub id: String,
    pub source: String,
    pub destination: String,
    pub state: CopyJobState,
    #[ts(type = "number")]
    pub files_done: u64,
    #[ts(type = "number")]
    pub files_total: u64,
    #[ts(type = "number")]
    pub bytes_done: u64,
    #[ts(type = "number")]
    pub bytes_total: u64,
    /// The file in flight, source-relative.
    pub current: Option<String>,
    /// Populated once the job reaches a terminal state. Empty before that —
    /// never a partial report, which would read as a finished one.
    pub entries: Vec<CopyEntryVm>,
    /// Why the job itself could not run. A per-file problem is an entry, not
    /// this: one unreadable file must not present as a failed job.
    pub error: Option<String>,
}

/// Everything a job holds while it runs.
struct Job {
    source: PathBuf,
    destination: PathBuf,
    state: CopyJobState,
    progress: CopyProgress,
    report: Option<CopyReport>,
    error: Option<String>,
    cancel: Arc<AtomicBool>,
    /// When this job reached a terminal state, for the retention sweep.
    settled_ms: Option<u64>,
}

/// Live and recently-finished copy jobs.
///
/// Mirrors [`crate::ipc::ExportRegistry`]'s shape — a monotonic id source plus a
/// mutex-guarded map — because a copy has the same lifecycle as an export: one
/// blocking worker, one cancel flag, one terminal result.
#[derive(Default)]
pub struct CopyRegistry {
    next_id: AtomicU64,
    jobs: Mutex<HashMap<String, Job>>,
}

impl CopyRegistry {
    fn register(&self, source: PathBuf, destination: PathBuf) -> (String, Arc<AtomicBool>) {
        let id = format!("copy-{}", self.next_id.fetch_add(1, Ordering::Relaxed) + 1);
        let cancel = Arc::new(AtomicBool::new(false));
        if let Ok(mut jobs) = self.jobs.lock() {
            // Sweep on insert rather than on a timer: the only moment the map can
            // grow is here, so this is exactly often enough and never runs when
            // nothing is happening.
            let now = now_ms();
            jobs.retain(|_, job| match job.settled_ms {
                Some(at) => now.saturating_sub(at) < RETAIN_TERMINAL_MS,
                None => true,
            });
            jobs.insert(
                id.clone(),
                Job {
                    source,
                    destination,
                    state: CopyJobState::Copying,
                    progress: CopyProgress::default(),
                    report: None,
                    error: None,
                    cancel: Arc::clone(&cancel),
                    settled_ms: None,
                },
            );
        }
        (id, cancel)
    }

    fn advance(&self, id: &str, progress: CopyProgress) {
        if let Ok(mut jobs) = self.jobs.lock() {
            if let Some(job) = jobs.get_mut(id) {
                // A late progress event must not resurrect a settled job.
                if job.settled_ms.is_none() {
                    job.progress = progress;
                }
            }
        }
    }

    fn settle(
        &self,
        id: &str,
        state: CopyJobState,
        report: Option<CopyReport>,
        error: Option<String>,
    ) {
        if let Ok(mut jobs) = self.jobs.lock() {
            if let Some(job) = jobs.get_mut(id) {
                job.state = state;
                job.report = report;
                job.error = error;
                job.settled_ms = Some(now_ms());
            }
        }
    }

    fn cancel(&self, id: &str) {
        if let Ok(jobs) = self.jobs.lock() {
            if let Some(job) = jobs.get(id) {
                job.cancel.store(true, Ordering::Relaxed);
            }
        }
    }

    fn snapshot(&self, id: &str) -> Option<CopyJobVm> {
        let jobs = self.jobs.lock().ok()?;
        let job = jobs.get(id)?;
        Some(CopyJobVm {
            id: id.to_owned(),
            source: job.source.display().to_string(),
            destination: job.destination.display().to_string(),
            state: job.state,
            files_done: job.progress.files_done,
            files_total: job.progress.files_total,
            bytes_done: job.progress.bytes_done,
            bytes_total: job.progress.bytes_total,
            current: job.progress.current.clone(),
            entries: job
                .report
                .as_ref()
                .map(|report| report.entries.iter().map(entry_vm).collect())
                .unwrap_or_default(),
            error: job.error.clone(),
        })
    }
}

fn entry_vm(entry: &keeper_sync::copy::CopyEntry) -> CopyEntryVm {
    let (outcome, reason) = match &entry.outcome {
        CopyOutcome::Copied => ("copied", None),
        CopyOutcome::Identical => ("identical", None),
        CopyOutcome::Collision => ("collision", None),
        CopyOutcome::Failed { reason } => ("failed", Some(reason.clone())),
    };
    CopyEntryVm {
        path: entry.path.clone(),
        bytes: entry.bytes,
        outcome: outcome.to_owned(),
        reason,
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn invalid(message: String) -> IpcError {
    IpcError {
        code: IpcErrorCode::Internal,
        message,
        account_id: None,
        retriable: false,
    }
}

/// Start copying `source` into `destination`, verifying every file.
///
/// Returns the job id immediately; the work runs on a blocking thread because
/// it hashes every byte twice and would otherwise hold a runtime worker for the
/// length of the copy.
#[tauri::command]
pub async fn copy_start(
    state: tauri::State<'_, AppState>,
    source: String,
    destination: String,
    replace_existing: Option<bool>,
) -> Result<String, IpcError> {
    let source = PathBuf::from(&source);
    let destination = PathBuf::from(&destination);
    // Refuse the two shapes that destroy data before the engine ever runs, and
    // say which one it was. Both are cheap to check and expensive to discover
    // halfway through a copy.
    if !source.exists() {
        return Err(invalid(format!("{} does not exist", source.display())));
    }
    if destination.starts_with(&source) {
        return Err(invalid(
            "the destination is inside the source, which would copy the tree into itself"
                .to_owned(),
        ));
    }

    let registry = Arc::clone(&state.copies);
    let (id, cancel) = registry.register(source.clone(), destination.clone());
    let options = CopyOptions {
        replace_existing: replace_existing.unwrap_or(false),
    };

    let job_id = id.clone();
    let sink_registry = Arc::clone(&registry);
    let sink_id = id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let sink: keeper_sync::copy::CopySink = Box::new(move |progress: CopyProgress| {
            sink_registry.advance(&sink_id, progress);
            true
        });
        let outcome =
            keeper_sync::copy::copy_verified(&source, &destination, &options, Some(&sink), &cancel);
        match outcome {
            Ok(report) => {
                let state = if cancel.load(Ordering::Relaxed) {
                    CopyJobState::Cancelled
                } else {
                    CopyJobState::Done
                };
                // The log is written before the job settles, so a UI that reacts
                // to `done` by opening the destination finds it already there.
                //
                // Written even for a cancelled job: the files that *were* copied
                // are still on disk and still need a record, and a log that only
                // appears for a perfect run is missing exactly when someone most
                // wants to know what happened. A failure to write it is logged
                // and swallowed — the copy itself succeeded, and refusing to
                // report a good copy because its receipt could not be filed
                // would be the wrong trade.
                let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
                let when = chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
                let log_path = destination.join(keeper_sync::copy::copy_log_filename(&stamp));
                let body =
                    keeper_sync::copy::render_copy_log(&report, &source, &destination, &when);
                if let Err(err) = std::fs::write(&log_path, body) {
                    tracing::warn!(
                        path = %log_path.display(),
                        %err,
                        "could not write the copy log"
                    );
                }
                registry.settle(&job_id, state, Some(report), None);
            }
            Err(err) => {
                registry.settle(&job_id, CopyJobState::Failed, None, Some(err.to_string()));
            }
        }
    });

    Ok(id)
}

/// Poll one job. An unknown id is an error rather than an empty job: a caller
/// polling an id nobody minted is a bug worth seeing, not a job that quietly
/// never finishes.
#[tauri::command]
pub async fn copy_status(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<CopyJobVm, IpcError> {
    state
        .copies
        .snapshot(&id)
        .ok_or_else(|| invalid(format!("no such copy job: {id}")))
}

/// Ask a job to stop. Idempotent, and safe at any moment: the engine checks the
/// flag between files and between chunks, and leaves no temp file behind.
#[tauri::command]
pub async fn copy_cancel(state: tauri::State<'_, AppState>, id: String) -> Result<(), IpcError> {
    state.copies.cancel(&id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> CopyRegistry {
        CopyRegistry::default()
    }

    #[test]
    fn a_job_is_readable_while_running_and_after_it_settles() {
        let reg = registry();
        let (id, _cancel) = reg.register(PathBuf::from("/a"), PathBuf::from("/b"));

        let running = reg.snapshot(&id).expect("registered");
        assert_eq!(running.state, CopyJobState::Copying);
        assert!(
            running.entries.is_empty(),
            "a partial report would read as a finished one"
        );

        reg.settle(&id, CopyJobState::Done, Some(CopyReport::default()), None);
        let settled = reg.snapshot(&id).expect("still readable");
        assert_eq!(settled.state, CopyJobState::Done);
    }

    #[test]
    fn a_late_progress_event_cannot_resurrect_a_settled_job() {
        // The worker coalesces progress, so an event can land just after the
        // terminal one. Applying it would rewind a finished job's counters.
        let reg = registry();
        let (id, _cancel) = reg.register(PathBuf::from("/a"), PathBuf::from("/b"));
        reg.settle(&id, CopyJobState::Done, Some(CopyReport::default()), None);
        reg.advance(
            &id,
            CopyProgress {
                files_done: 1,
                files_total: 9,
                bytes_done: 10,
                bytes_total: 90,
                current: Some("late.txt".to_owned()),
            },
        );
        let after = reg.snapshot(&id).expect("readable");
        assert_eq!(after.state, CopyJobState::Done);
        assert_eq!(after.files_done, 0, "the settled counters stand");
        assert_eq!(after.current, None);
    }

    #[test]
    fn cancelling_an_unknown_job_is_not_an_error() {
        // The UI can race a cancel against a sweep; making that an error would
        // surface a failure for something the user already got.
        registry().cancel("copy-404");
    }

    #[test]
    fn an_entry_carries_its_reason_only_when_it_failed() {
        let failed = entry_vm(&keeper_sync::copy::CopyEntry {
            path: "a.txt".to_owned(),
            bytes: 0,
            outcome: CopyOutcome::Failed {
                reason: "changed while reading".to_owned(),
            },
        });
        assert_eq!(failed.outcome, "failed");
        assert_eq!(failed.reason.as_deref(), Some("changed while reading"));

        let collided = entry_vm(&keeper_sync::copy::CopyEntry {
            path: "b.txt".to_owned(),
            bytes: 4,
            outcome: CopyOutcome::Collision,
        });
        assert_eq!(collided.outcome, "collision");
        assert_eq!(
            collided.reason, None,
            "a collision is not a failure and must not carry a failure reason"
        );
    }
}
