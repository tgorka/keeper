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
    parse_sources_result, response_id, response_protocol_version, verify_protocol_version,
    RecordingEvent,
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
/// Never a panic. `run_session` (16.2) is untouched — it keeps stdin null.
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
#[cfg(desktop)]
async fn fetch_capabilities(path: &std::path::Path) -> Result<RecordingCapabilitiesVm, CoreError> {
    let line = request_response(
        path,
        &capabilities_request(CAPABILITIES_REQUEST_ID),
        CAPABILITIES_REQUEST_ID,
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

/// The `listSources` round-trip against the sidecar at `path` (Story 16.4). No
/// version handshake here — `getCapabilities` owns it.
#[cfg(desktop)]
async fn fetch_sources(path: &std::path::Path) -> Result<RecordingSourcesVm, CoreError> {
    let line = request_response(
        path,
        &list_sources_request(LIST_SOURCES_REQUEST_ID),
        LIST_SOURCES_REQUEST_ID,
    )
    .await?;
    parse_sources_result(&line).map_err(CoreError::Recording)
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

    async fn get_capabilities(&self) -> Result<RecordingCapabilitiesVm, CoreError> {
        // An unresolvable sidecar is the honest not-available path — `Unsupported`
        // verbatim from `sidecar_path`, never a spawn, never a panic.
        let path = self.platform.sidecar_path(KEEPER_REC_SIDECAR_NAME)?;
        fetch_capabilities(&path).await
    }

    async fn list_sources(&self) -> Result<RecordingSourcesVm, CoreError> {
        let path = self.platform.sidecar_path(KEEPER_REC_SIDECAR_NAME)?;
        fetch_sources(&path).await
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
        let err = fetch_capabilities(sidecar.path())
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
        let err = fetch_capabilities(sidecar.path())
            .await
            .expect_err("a future-version sidecar must be rejected");
        assert!(
            matches!(err, CoreError::Unsupported(_)),
            "a shape-changed future version must be Unsupported, not Protocol; got {err:?}"
        );
    }

    #[tokio::test]
    async fn fetch_sources_round_trips_the_fake_sidecar() {
        let response = r#"{"id":2,"result":{"displays":[{"id":1,"width":1920,"height":1080,"isMain":true}],"applications":[],"microphones":[],"cameras":[]}}"#;
        let script = format!("#!/bin/sh\nread line\necho '{response}'\n");
        let sidecar = FakeSidecar::write("sources", &script);
        let vm = fetch_sources(sidecar.path())
            .await
            .expect("listSources must round-trip");
        assert_eq!(vm.displays.len(), 1);
        assert_eq!(vm.displays[0].width, 1920);
        assert!(vm.applications.is_empty());
    }
}
