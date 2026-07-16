//! The `keeper-rec` screen-recording sidecar port (Story 16.2, Epic 16, AD-24,
//! AD-27) — the shell-side impl of [`keeper_core::recording::Recorder`].
//!
//! The platform-free session machine, the tolerant NDJSON parser, and the
//! [`Recorder`](keeper_core::recording::Recorder) trait all live in
//! [`keeper_core::recording`]. This module holds only the platform glue: the desktop
//! impl spawns `keeper-rec` via [`Platform::sidecar_path`] and streams its stdout
//! NDJSON through the core parser into the caller's `on_event` sink; iOS is honest
//! about its absence and returns [`CoreError::Unsupported`].
//!
//! **Mirrors `DesktopBbctlRunner`** (see `ipc.rs`): `tokio::process` spawn, byte-level
//! line reading (a non-UTF-8 line is skipped, never a false EOF), and an
//! [`AbortOnDrop`] guard so the reader task is torn down whenever the `run_session`
//! future is dropped. Unlike bbctl, `keeper-rec` speaks NDJSON on **stdout only** —
//! there is no stderr merge.
//!
//! This story lands the port seam only; no command / IPC surface consumes it yet
//! (that arrives in a later recording story, 16.3+). Until a consumer exists the
//! non-test build sees the port as dead code — an intentional, allowed seam.
#![allow(dead_code)]

#[cfg(desktop)]
use std::sync::Arc;

#[cfg(any(desktop, target_os = "ios"))]
use keeper_core::error::CoreError;
#[cfg(desktop)]
use keeper_core::platform::Platform;
#[cfg(desktop)]
use keeper_core::recording::{parse_event, RecordingEvent};

/// The logical sidecar name for the first-party Swift capture sidecar (Story 16.1).
/// Resolved per-arch next to the executable via [`Platform::sidecar_path`]
/// (`keeper-rec-aarch64-apple-darwin`).
#[cfg(desktop)]
const KEEPER_REC_SIDECAR_NAME: &str = "keeper-rec";

/// Aborts the wrapped task when dropped — tears down the `keeper-rec` stdout reader
/// task whenever the `run_session` future is dropped (early return or a driver
/// cancel), leaving no reader task or pipe fd leaked. The `keeper-rec` **process**
/// is killed on the same drop by `Command::kill_on_drop(true)` (a live capture
/// daemon must never be orphaned when the session future is cancelled — unlike the
/// short-lived launch-and-leave `bbctl` CLI); on the normal EOF path the process is
/// reaped and its exit status inspected below.
#[cfg(desktop)]
struct AbortOnDrop(tokio::task::JoinHandle<()>);

#[cfg(desktop)]
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// The desktop (macOS) [`Recorder`](keeper_core::recording::Recorder) impl (Story
/// 16.2, AD-24). `is_available` is simply whether the `keeper-rec` sidecar resolves;
/// `run_session` spawns it via `tokio::process` on the resolved path — no
/// `tauri-plugin-shell`, no new capability.
#[cfg(desktop)]
pub struct DesktopRecorder {
    platform: Arc<dyn Platform>,
}

#[cfg(desktop)]
impl DesktopRecorder {
    /// Construct a recorder sharing the app's platform port (for sidecar resolution).
    pub fn new(platform: Arc<dyn Platform>) -> Self {
        Self { platform }
    }
}

#[cfg(desktop)]
impl keeper_core::recording::Recorder for DesktopRecorder {
    fn is_available(&self) -> bool {
        self.platform.sidecar_path(KEEPER_REC_SIDECAR_NAME).is_ok()
    }

    async fn run_session(
        &self,
        mut on_event: Box<dyn FnMut(RecordingEvent) + Send>,
    ) -> Result<(), CoreError> {
        use keeper_core::error::RecordingError;
        use tokio::io::{AsyncBufReadExt, BufReader};

        // An unresolvable sidecar is the honest not-available path — return the
        // existing `Unsupported` verbatim (matching `sidecar_path` honesty), never a
        // new variant, never a spawn, never a panic.
        let path = self.platform.sidecar_path(KEEPER_REC_SIDECAR_NAME)?;

        let mut child = tokio::process::Command::new(&path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            // Kill the capture daemon if this future is dropped before EOF (driver
            // cancel / app quit) — never orphan a live screen-capture process.
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                CoreError::Recording(RecordingError::SidecarFailed(format!(
                    "could not launch keeper-rec: {e}"
                )))
            })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            CoreError::Recording(RecordingError::SidecarFailed(
                "could not capture keeper-rec stdout".to_owned(),
            ))
        })?;

        // Stream `Vec<u8>` lines byte-level so a non-UTF-8 line is skipped (decoded
        // lossily below), never treated as a false EOF. Wrapped in `AbortOnDrop` so
        // the reader is torn down whenever this future is dropped, never leaking.
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let _reader = AbortOnDrop(tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match reader.read_until(b'\n', &mut buf).await {
                    Ok(0) => break,
                    Ok(_) => {
                        if tx.send(buf.clone()).is_err() {
                            break;
                        }
                    }
                    // A read error ends the stream only — never treated as a panic.
                    Err(_) => break,
                }
            }
        }));

        // Consume NDJSON lines, forwarding each recognized event as it arrives. An
        // unrecognized / malformed line parses to `None` and is dropped.
        while let Some(raw) = rx.recv().await {
            let line = String::from_utf8_lossy(&raw);
            let trimmed = line.trim_end_matches(['\n', '\r']);
            if trimmed.is_empty() {
                continue;
            }
            if let Some(event) = parse_event(trimmed) {
                on_event(event);
            }
        }

        // The stream reached EOF (the sidecar is exiting) — reap AND inspect the
        // status. A reap failure or a non-success exit is a sidecar failure surfaced
        // honestly: a `keeper-rec` that crashes/exits non-zero without having emitted
        // an `error` line must NOT resolve as a clean success. Never a panic.
        let status = child.wait().await.map_err(|e| {
            CoreError::Recording(RecordingError::SidecarFailed(format!(
                "keeper-rec did not exit cleanly: {e}"
            )))
        })?;
        if !status.success() {
            return Err(CoreError::Recording(RecordingError::SidecarFailed(
                format!("keeper-rec exited with failure status: {status}"),
            )));
        }
        Ok(())
    }
}

/// The iOS [`Recorder`](keeper_core::recording::Recorder) impl (Story 16.2, AD-27).
/// Recording is desktop-macOS-only — iOS never records, so this is honest about its
/// absence: `is_available` is `false` and `run_session` returns
/// [`CoreError::Unsupported`], never a spawn, never a panic.
#[cfg(target_os = "ios")]
pub struct IosRecorder;

#[cfg(target_os = "ios")]
impl keeper_core::recording::Recorder for IosRecorder {
    fn is_available(&self) -> bool {
        false
    }

    async fn run_session(
        &self,
        _on_event: Box<dyn FnMut(keeper_core::recording::RecordingEvent) + Send>,
    ) -> Result<(), CoreError> {
        Err(CoreError::Unsupported(
            "recording is not available on iOS".to_owned(),
        ))
    }
}

#[cfg(all(test, desktop))]
mod tests {
    use super::*;
    use crate::ipc::DesktopPlatform;
    use keeper_core::recording::Recorder;

    /// In the dev/CI test env there is no bundled `keeper-rec` sidecar next to the
    /// test binary, so `sidecar_path("keeper-rec")` returns `Unsupported` and the
    /// recorder must honestly report `is_available() == false` — exercising the seam
    /// without hardware, never a panic. (`run_session` returning `Unsupported` in the
    /// same env is covered by the AC; it is not spawned here.)
    #[test]
    fn desktop_recorder_is_unavailable_without_bundled_sidecar() {
        let recorder = DesktopRecorder::new(Arc::new(DesktopPlatform));
        assert!(
            !recorder.is_available(),
            "no bundled keeper-rec in the test env → is_available() must be false"
        );
    }
}
