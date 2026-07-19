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
use keeper_core::recording::{
    capabilities_request, list_sources_request, parse_capabilities_result, parse_event,
    parse_request_screen_recording_result, parse_sources_result, request_screen_recording_request,
    response_id, response_protocol_version, start_recording_request, stop_recording_request,
    verify_protocol_version, RecordingEvent, SessionParams,
};
#[cfg(any(desktop, target_os = "ios"))]
use keeper_core::vm::{RecordingCapabilitiesVm, RecordingSourcesVm};

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

/// The fixed correlation id for the one-shot `getCapabilities` round-trip. Each
/// request/response call spawns a fresh sidecar, so a constant id is unambiguous
/// (a persistent multi-request session is 16.6's concern).
#[cfg(desktop)]
const CAPABILITIES_REQUEST_ID: u64 = 1;

/// The fixed correlation id for the one-shot `listSources` round-trip.
#[cfg(desktop)]
const LIST_SOURCES_REQUEST_ID: u64 = 2;

/// The fixed correlation id for the one-shot `requestScreenRecording` round-trip
/// (Story 16.5).
#[cfg(desktop)]
const REQUEST_SCREEN_RECORDING_REQUEST_ID: u64 = 3;

/// The fixed correlation id for the session-opening `startRecording` request
/// (Story 16.6). Each session is a fresh sidecar spawn, so a constant is
/// unambiguous; the id-carrying acknowledgement is skipped by the event reader
/// ([`parse_event`] recognizes only `event` lines).
#[cfg(desktop)]
const START_RECORDING_REQUEST_ID: u64 = 4;

/// The fixed correlation id for the session-closing `stop` request (Story 16.6).
#[cfg(desktop)]
const STOP_RECORDING_REQUEST_ID: u64 = 5;

/// The bound on one permission pre-flight round-trip against the sidecar (Story
/// 16.5; closes the deferred unbounded-`request_response` spinner risk). The
/// pre-flight re-runs on every focus/return, so a wedged sidecar must resolve a
/// clean error rather than pending forever — the exact "spinner waiting on a
/// grant that will never come" the story exists to prevent. Generous versus the
/// expected millisecond round-trip (`CGRequestScreenCaptureAccess` returns
/// immediately; the OS prompt is posted asynchronously), tight enough that the
/// Recording view recovers promptly.
#[cfg(desktop)]
const PREFLIGHT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Bound a pre-flight round-trip future with `timeout` (Story 16.5): on expiry
/// the future is dropped — `kill_on_drop` reaps the wedged sidecar — and a clean
/// [`RecordingError::SidecarFailed`](keeper_core::error::RecordingError::SidecarFailed)
/// names the unanswered method. The timeout lives here in the shell only;
/// `keeper-core` stays tokio-free.
#[cfg(desktop)]
async fn bounded<T>(
    method: &str,
    timeout: std::time::Duration,
    round_trip: impl std::future::Future<Output = Result<T, CoreError>>,
) -> Result<T, CoreError> {
    use keeper_core::error::RecordingError;

    match tokio::time::timeout(timeout, round_trip).await {
        Ok(result) => result,
        Err(_) => Err(CoreError::Recording(RecordingError::SidecarFailed(
            format!(
                "keeper-rec did not answer {method} within {:.1}s",
                timeout.as_secs_f64()
            ),
        ))),
    }
}

/// One id-correlated NDJSON-RPC round-trip against the `keeper-rec` sidecar at
/// `path` (Story 16.4, AD-34): spawn with **stdin piped + stdout piped +
/// `kill_on_drop`**, write the single `request_line` and flush, read stdout lines
/// byte-level until [`response_id`] matches `id` (skipping interleaved unsolicited
/// events, garbage, and non-UTF-8 lines — never a false EOF), then close stdin so
/// the sidecar's request loop reaches EOF and exits, and reap.
///
/// A response that arrived is honored even if a late non-zero exit follows (the
/// answer is the contract; the exit code is bookkeeping). With no response, the
/// most informative failure surfaces: a spawn/IO fault or non-zero exit as
/// [`RecordingError::SidecarFailed`](keeper_core::error::RecordingError::SidecarFailed),
/// a clean exit without an answer as
/// [`RecordingError::Protocol`](keeper_core::error::RecordingError::Protocol).
/// Never a panic. (`run_session` pipes stdin too since 16.6 — it carries the
/// start/stop requests — but stays a separate, streaming code path.)
#[cfg(desktop)]
async fn request_response(
    path: &std::path::Path,
    request_line: &str,
    id: u64,
) -> Result<String, CoreError> {
    use keeper_core::error::RecordingError;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let mut child = tokio::process::Command::new(path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        // Kill the sidecar if this future is dropped mid-round-trip (caller
        // cancel / app quit) — never orphan a spawned child.
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| {
            CoreError::Recording(RecordingError::SidecarFailed(format!(
                "could not launch keeper-rec: {e}"
            )))
        })?;

    let mut stdin = child.stdin.take().ok_or_else(|| {
        CoreError::Recording(RecordingError::SidecarFailed(
            "could not open keeper-rec stdin".to_owned(),
        ))
    })?;
    let stdout = child.stdout.take().ok_or_else(|| {
        CoreError::Recording(RecordingError::SidecarFailed(
            "could not capture keeper-rec stdout".to_owned(),
        ))
    })?;

    // One request line, newline-framed, flushed. A write failure (e.g. the
    // sidecar died instantly) surfaces honestly below via the no-response path,
    // but the write error itself is the most precise cause — surface it.
    let mut framed = request_line.as_bytes().to_vec();
    framed.push(b'\n');
    if let Err(e) = async {
        stdin.write_all(&framed).await?;
        stdin.flush().await
    }
    .await
    {
        return Err(CoreError::Recording(RecordingError::SidecarFailed(
            format!("could not write the keeper-rec request: {e}"),
        )));
    }

    // Read byte-level lines until the id-correlated response. A non-UTF-8 line is
    // decoded lossily and skipped by the id check — never a false EOF. A read
    // error ends the stream; the no-response arm below reports it.
    let mut reader = BufReader::new(stdout);
    let mut buf = Vec::new();
    let mut response: Option<String> = None;
    let mut read_fault: Option<String> = None;
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf).await {
            Ok(0) => break,
            Ok(_) => {
                let line = String::from_utf8_lossy(&buf);
                let trimmed = line.trim_end_matches(['\n', '\r']);
                if response_id(trimmed) == Some(id) {
                    response = Some(trimmed.to_owned());
                    break;
                }
                // Interleaved unsolicited event / garbage / other-id line — skip.
            }
            Err(e) => {
                read_fault = Some(e.to_string());
                break;
            }
        }
    }

    // Close stdin (EOF) so the sidecar's request loop exits, then reap. An
    // already-received response is honored even on a late non-zero exit.
    drop(stdin);
    let status = child.wait().await;
    if let Some(line) = response {
        if let Ok(exit) = &status {
            if !exit.success() {
                tracing::debug!(
                    request_id = id,
                    %exit,
                    "keeper-rec answered, then exited non-zero; honoring the response"
                );
            }
        }
        return Ok(line);
    }
    match (read_fault, status) {
        (Some(fault), _) => Err(CoreError::Recording(RecordingError::SidecarFailed(
            format!("keeper-rec stdout read failed before request {id} was answered: {fault}"),
        ))),
        (None, Err(e)) => Err(CoreError::Recording(RecordingError::SidecarFailed(
            format!("keeper-rec did not exit cleanly: {e}"),
        ))),
        (None, Ok(exit)) if !exit.success() => Err(CoreError::Recording(
            RecordingError::SidecarFailed(format!(
                "keeper-rec exited with failure status {exit} before answering request {id}"
            )),
        )),
        (None, Ok(_)) => Err(CoreError::Recording(RecordingError::Protocol(format!(
            "keeper-rec closed its stream without answering request {id}"
        )))),
    }
}

/// The `getCapabilities` round-trip + protocol-version handshake against the
/// sidecar at `path` (Story 16.4). Split from the trait method so the fake-
/// executable harness can drive the real body without a bundled sidecar.
/// Bounded by `timeout` (Story 16.5) — the trait method passes
/// [`PREFLIGHT_TIMEOUT`]; tests pass a short bound to exercise the hang path.
#[cfg(desktop)]
async fn fetch_capabilities(
    path: &std::path::Path,
    timeout: std::time::Duration,
) -> Result<RecordingCapabilitiesVm, CoreError> {
    let line = bounded(
        "getCapabilities",
        timeout,
        request_response(
            path,
            &capabilities_request(CAPABILITIES_REQUEST_ID),
            CAPABILITIES_REQUEST_ID,
        ),
    )
    .await?;
    // Verify the protocol version BEFORE validating the full result shape. A real
    // version bump usually also changes the result shape, so parsing the whole VM
    // first would fail with an opaque `Protocol` fault ("missing permissions") on a
    // future sidecar — defeating the handshake. Extract just the version, verify it
    // (mismatch → honest `Unsupported`), and only then parse the v1 shape.
    let version = response_protocol_version(&line).map_err(CoreError::Recording)?;
    verify_protocol_version(version)?;
    let vm = parse_capabilities_result(&line).map_err(CoreError::Recording)?;
    Ok(vm)
}

/// The `listSources` round-trip against the sidecar at `path` (Story 16.4 →
/// 19.1). No version handshake here — `getCapabilities` owns it. Bounded by
/// `timeout` (Story 19.1): enumeration is now async on the sidecar
/// (`SCShareableContent.current`), so a wedged sidecar must resolve a clean
/// error rather than hanging the picker's ~3s poll — the same anti-spinner
/// guard [`fetch_capabilities`] uses. Split from the trait method so the
/// fake-executable harness can drive the real body.
#[cfg(desktop)]
async fn fetch_sources(
    path: &std::path::Path,
    timeout: std::time::Duration,
) -> Result<RecordingSourcesVm, CoreError> {
    let line = bounded(
        "listSources",
        timeout,
        request_response(
            path,
            &list_sources_request(LIST_SOURCES_REQUEST_ID),
            LIST_SOURCES_REQUEST_ID,
        ),
    )
    .await?;
    parse_sources_result(&line).map_err(CoreError::Recording)
}

/// The `requestScreenRecording` round-trip against the sidecar at `path` (Story
/// 16.5, AD-36): the sidecar calls `CGRequestScreenCaptureAccess` (TCC attributes
/// the request to keeper because the sidecar is keeper's own child process) and
/// answers `{granted}`. Bounded by `timeout` like [`fetch_capabilities`]. Split
/// from the trait method so the fake-executable harness can drive the real body.
#[cfg(desktop)]
async fn fetch_request_screen_recording(
    path: &std::path::Path,
    timeout: std::time::Duration,
) -> Result<bool, CoreError> {
    let line = bounded(
        "requestScreenRecording",
        timeout,
        request_response(
            path,
            &request_screen_recording_request(REQUEST_SCREEN_RECORDING_REQUEST_ID),
            REQUEST_SCREEN_RECORDING_REQUEST_ID,
        ),
    )
    .await?;
    parse_request_screen_recording_result(&line).map_err(CoreError::Recording)
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
        params: SessionParams,
        stop: impl std::future::Future<Output = ()> + Send + 'static,
        mut on_event: Box<dyn FnMut(RecordingEvent) + Send>,
    ) -> Result<(), CoreError> {
        use keeper_core::error::RecordingError;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        // An unresolvable sidecar is the honest not-available path — return the
        // existing `Unsupported` verbatim (matching `sidecar_path` honesty), never a
        // new variant, never a spawn, never a panic.
        let path = self.platform.sidecar_path(KEEPER_REC_SIDECAR_NAME)?;

        let mut child = tokio::process::Command::new(&path)
            // Piped stdin carries the `startRecording` request and, when `stop`
            // resolves, the graceful `stop` request (Story 16.6). Held open for
            // the whole session — the sidecar exits on its own after finalizing.
            .stdin(std::process::Stdio::piped())
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

        let mut stdin = child.stdin.take().ok_or_else(|| {
            CoreError::Recording(RecordingError::SidecarFailed(
                "could not open keeper-rec stdin".to_owned(),
            ))
        })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            CoreError::Recording(RecordingError::SidecarFailed(
                "could not capture keeper-rec stdout".to_owned(),
            ))
        })?;

        // Fire the start request. A write failure here means the sidecar died at
        // spawn — surface it as the honest sidecar failure it is.
        let start_line = format!(
            "{}\n",
            start_recording_request(START_RECORDING_REQUEST_ID, &params)
        );
        stdin.write_all(start_line.as_bytes()).await.map_err(|e| {
            CoreError::Recording(RecordingError::SidecarFailed(format!(
                "could not send startRecording to keeper-rec: {e}"
            )))
        })?;
        stdin.flush().await.map_err(|e| {
            CoreError::Recording(RecordingError::SidecarFailed(format!(
                "could not flush startRecording to keeper-rec: {e}"
            )))
        })?;

        // Forward the caller's stop intent as the graceful `stop` request. The
        // task owns stdin (kept open — closing it would EOF the sidecar's request
        // loop early); it is aborted on drop with the rest of the session.
        let _stopper = AbortOnDrop(tokio::spawn(async move {
            stop.await;
            let stop_line = format!("{}\n", stop_recording_request(STOP_RECORDING_REQUEST_ID));
            // Best-effort: if the sidecar already exited (failed/finalized), the
            // pipe is broken and the session is ending anyway.
            let _ = stdin.write_all(stop_line.as_bytes()).await;
            let _ = stdin.flush().await;
            // Keep stdin open until the sidecar exits on its own.
            std::future::pending::<()>().await;
        }));

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

    async fn get_capabilities(&self) -> Result<RecordingCapabilitiesVm, CoreError> {
        // An unresolvable sidecar is the honest not-available path — `Unsupported`
        // verbatim from `sidecar_path`, never a spawn, never a panic.
        let path = self.platform.sidecar_path(KEEPER_REC_SIDECAR_NAME)?;
        fetch_capabilities(&path, PREFLIGHT_TIMEOUT).await
    }

    async fn list_sources(&self) -> Result<RecordingSourcesVm, CoreError> {
        let path = self.platform.sidecar_path(KEEPER_REC_SIDECAR_NAME)?;
        fetch_sources(&path, PREFLIGHT_TIMEOUT).await
    }

    async fn request_screen_recording(&self) -> Result<bool, CoreError> {
        let path = self.platform.sidecar_path(KEEPER_REC_SIDECAR_NAME)?;
        fetch_request_screen_recording(&path, PREFLIGHT_TIMEOUT).await
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
        _params: keeper_core::recording::SessionParams,
        _stop: impl std::future::Future<Output = ()> + Send + 'static,
        _on_event: Box<dyn FnMut(keeper_core::recording::RecordingEvent) + Send>,
    ) -> Result<(), CoreError> {
        Err(CoreError::Unsupported(
            "recording is not available on iOS".to_owned(),
        ))
    }

    async fn get_capabilities(&self) -> Result<RecordingCapabilitiesVm, CoreError> {
        Err(CoreError::Unsupported(
            "recording is not available on iOS".to_owned(),
        ))
    }

    async fn list_sources(&self) -> Result<RecordingSourcesVm, CoreError> {
        Err(CoreError::Unsupported(
            "recording is not available on iOS".to_owned(),
        ))
    }

    async fn request_screen_recording(&self) -> Result<bool, CoreError> {
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

    // --- fake-executable NDJSON harness (Story 16.4) ------------------------
    //
    // Exercises the real `request_response` spawn → write → id-correlated read →
    // close-stdin → reap round-trip without a signed sidecar: a tiny temp shell
    // script reads the request line and echoes canned NDJSON. This closes the
    // deferred "spawn/stream/reap untested" seam for the request path.

    /// Write a minimal executable shell script to the OS temp dir and return its
    /// path. Removed by [`FakeSidecar`]'s `Drop` even when an assertion fails.
    struct FakeSidecar(std::path::PathBuf);

    impl FakeSidecar {
        fn write(name: &str, body: &str) -> Self {
            use std::os::unix::fs::PermissionsExt;
            let path =
                std::env::temp_dir().join(format!("keeper-rec-fake-{name}-{}", std::process::id()));
            std::fs::write(&path, body).expect("write fake sidecar script");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .expect("mark fake sidecar executable");
            Self(path)
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for FakeSidecar {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    /// A canned, protocol-matching `getCapabilities` response body (id 1).
    const CANNED_CAPABILITIES: &str = r#"{"id":1,"result":{"protocolVersion":1,"macos":"15.5.0","features":{"systemAudio":true,"microphone":false,"camera":false},"permissions":{"screenRecording":"granted","microphone":"notDetermined","camera":"notDetermined"}}}"#;

    #[tokio::test]
    async fn request_response_skips_interleaved_lines_and_reaps() {
        // The script echoes an unsolicited event line and a garbage line BEFORE the
        // id-correlated response — the reader must skip both, return the response,
        // and reap the child cleanly.
        let script = format!(
            "#!/bin/sh\nread line\necho '{{\"event\":\"state\",\"state\":\"preflight\"}}'\necho 'garbage, not json'\necho '{CANNED_CAPABILITIES}'\n"
        );
        let sidecar = FakeSidecar::write("interleaved", &script);
        let line = request_response(sidecar.path(), &capabilities_request(1), 1)
            .await
            .expect("id-correlated response must round-trip");
        assert_eq!(line, CANNED_CAPABILITIES);
    }

    #[tokio::test]
    async fn request_response_honors_response_despite_late_nonzero_exit() {
        // A response that arrived is the contract — a late non-zero exit after the
        // answer must NOT mask it (resolves the deferred exit-code-contract entry).
        let script = format!("#!/bin/sh\nread line\necho '{CANNED_CAPABILITIES}'\nexit 3\n");
        let sidecar = FakeSidecar::write("late-exit", &script);
        let line = request_response(sidecar.path(), &capabilities_request(1), 1)
            .await
            .expect("an answered request survives a late non-zero exit");
        assert_eq!(line, CANNED_CAPABILITIES);
    }

    #[tokio::test]
    async fn request_response_surfaces_a_silent_clean_exit_as_protocol_fault() {
        // A sidecar that exits 0 without ever answering is a protocol fault, not a
        // success and not a sidecar crash.
        let script = "#!/bin/sh\nread line\nexit 0\n";
        let sidecar = FakeSidecar::write("silent", script);
        let err = request_response(sidecar.path(), &capabilities_request(1), 1)
            .await
            .expect_err("no answer must not resolve Ok");
        assert!(
            matches!(
                err,
                CoreError::Recording(keeper_core::error::RecordingError::Protocol(_))
            ),
            "expected a Protocol fault, got {err:?}"
        );
    }

    #[tokio::test]
    async fn mismatched_protocol_version_resolves_unsupported() {
        // The real `fetch_capabilities` body against a sidecar reporting version 2:
        // the handshake must resolve a clean `Unsupported`, never a crash.
        let mismatched =
            CANNED_CAPABILITIES.replace("\"protocolVersion\":1", "\"protocolVersion\":2");
        let script = format!("#!/bin/sh\nread line\necho '{mismatched}'\n");
        let sidecar = FakeSidecar::write("version-skew", &script);
        let err = fetch_capabilities(sidecar.path(), PREFLIGHT_TIMEOUT)
            .await
            .expect_err("a version skew must be rejected");
        assert!(
            matches!(err, CoreError::Unsupported(_)),
            "expected Unsupported, got {err:?}"
        );
    }

    #[tokio::test]
    async fn future_version_with_a_changed_shape_still_resolves_unsupported() {
        // A realistic v2 bump also changes the result shape. The handshake must
        // verify the version BEFORE validating the shape, so this surfaces the
        // honest `Unsupported` — NOT an opaque `Protocol` "missing permissions"
        // fault. This is the regression guard for the version-before-shape fix.
        let future = r#"{"id":1,"result":{"protocolVersion":2,"macos":"16.0.0","capabilities":{"totally":"different"}}}"#;
        let script = format!("#!/bin/sh\nread line\necho '{future}'\n");
        let sidecar = FakeSidecar::write("future-shape", &script);
        let err = fetch_capabilities(sidecar.path(), PREFLIGHT_TIMEOUT)
            .await
            .expect_err("a future-version sidecar must be rejected");
        assert!(
            matches!(err, CoreError::Unsupported(_)),
            "a shape-changed future version must be Unsupported, not Protocol; got {err:?}"
        );
    }

    #[tokio::test]
    async fn fetch_request_screen_recording_round_trips_the_fake_sidecar() {
        // The real `fetch_request_screen_recording` body against a fake sidecar
        // echoing the id-correlated `{granted}` result — both outcomes.
        for (granted, expected) in [("true", true), ("false", false)] {
            let response = format!(r#"{{"id":3,"result":{{"granted":{granted}}}}}"#);
            let script = format!("#!/bin/sh\nread line\necho '{response}'\n");
            let sidecar = FakeSidecar::write(&format!("request-{granted}"), &script);
            let outcome = fetch_request_screen_recording(sidecar.path(), PREFLIGHT_TIMEOUT)
                .await
                .expect("requestScreenRecording must round-trip");
            assert_eq!(outcome, expected);
        }
    }

    #[tokio::test]
    async fn a_hung_sidecar_resolves_a_clean_timeout_error_not_a_spinner() {
        // The spinner guard (Story 16.5): a sidecar that reads the request and then
        // wedges (sleeps past the bound) must resolve a clean SidecarFailed within
        // the timeout — never pend forever. `kill_on_drop` reaps the wedged child
        // when the timed-out round-trip future is dropped. A short test-only bound
        // keeps the suite fast; the script sleeps well past it.
        let script = "#!/bin/sh\nread line\nsleep 30\n";
        let bound = std::time::Duration::from_millis(200);

        let sidecar = FakeSidecar::write("hang-capabilities", script);
        let err = fetch_capabilities(sidecar.path(), bound)
            .await
            .expect_err("a hung getCapabilities must time out");
        assert!(
            matches!(
                &err,
                CoreError::Recording(keeper_core::error::RecordingError::SidecarFailed(m))
                    if m.contains("getCapabilities")
            ),
            "expected a timeout SidecarFailed naming getCapabilities, got {err:?}"
        );

        let sidecar = FakeSidecar::write("hang-request", script);
        let err = fetch_request_screen_recording(sidecar.path(), bound)
            .await
            .expect_err("a hung requestScreenRecording must time out");
        assert!(
            matches!(
                &err,
                CoreError::Recording(keeper_core::error::RecordingError::SidecarFailed(m))
                    if m.contains("requestScreenRecording")
            ),
            "expected a timeout SidecarFailed naming requestScreenRecording, got {err:?}"
        );
    }

    #[tokio::test]
    async fn fetch_sources_round_trips_the_fake_sidecar() {
        // Story 19.1: the sources round-trip now carries real applications
        // (name/pid/bundleId + an optional icon data-URI), no longer an empty
        // list — the fake sidecar echoes a canned display + application.
        let response = r#"{"id":2,"result":{"displays":[{"id":1,"width":1920,"height":1080,"isMain":true}],"applications":[{"bundleId":"com.apple.Safari","name":"Safari","pid":501,"icon":"data:image/png;base64,iVBORw0KGgo="}],"microphones":[],"cameras":[]}}"#;
        let script = format!("#!/bin/sh\nread line\necho '{response}'\n");
        let sidecar = FakeSidecar::write("sources", &script);
        let vm = fetch_sources(sidecar.path(), PREFLIGHT_TIMEOUT)
            .await
            .expect("listSources must round-trip");
        assert_eq!(vm.displays.len(), 1);
        assert_eq!(vm.displays[0].width, 1920);
        assert_eq!(vm.applications.len(), 1);
        assert_eq!(vm.applications[0].bundle_id, "com.apple.Safari");
        assert_eq!(vm.applications[0].pid, 501);
        assert!(vm.applications[0].icon.is_some());
    }

    #[tokio::test]
    async fn a_hung_list_sources_resolves_a_clean_timeout_error_not_a_spinner() {
        // Story 19.1: `listSources` enumeration is async on the sidecar
        // (SCShareableContent), so a wedged sidecar must resolve a clean
        // SidecarFailed within the bound rather than hanging the picker poll.
        let script = "#!/bin/sh\nread line\nsleep 30\n";
        let bound = std::time::Duration::from_millis(200);
        let sidecar = FakeSidecar::write("hang-sources", script);
        let err = fetch_sources(sidecar.path(), bound)
            .await
            .expect_err("a hung listSources must time out");
        assert!(
            matches!(
                &err,
                CoreError::Recording(keeper_core::error::RecordingError::SidecarFailed(m))
                    if m.contains("listSources")
            ),
            "expected a timeout SidecarFailed naming listSources, got {err:?}"
        );
    }
}
