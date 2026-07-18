//! The platform-free recording session heart (Story 16.2, Epic 16, AD-33).
//!
//! Owns the screen-recording session state machine
//! `idle → preflight → recording → rotating → stopping → finalized | recovered |
//! failed`, a tolerant NDJSON event parser, the [`Recorder`] port that sits beside
//! [`crate::platform::Platform`], and the [`drive_session`] orchestrator.
//!
//! **Platform-free invariant (load-bearing).** Nothing here touches the Tauri shell,
//! an Apple framework, the Objective-C runtime, a child process, or a `keeper-rec`
//! process handle — the shell port
//! ([`crate::platform`]'s sibling `keeper/src/recorder.rs`, `#[cfg(desktop)]`)
//! spawns `keeper-rec` and parses its stdout NDJSON, then feeds the resulting
//! [`RecordingEvent`]s in here. The [`RecordingSession`] NEVER holds a process
//! handle; it advances only on events the port reports. A unit-test source guard
//! (`tests::dependency_firewall_holds`) enforces the token ban, mirroring
//! `signals::tests::presence_is_withheld_everywhere`.
//!
//! **Port mirrors `bridges::bbctl::BbctlRunner`.** [`Recorder`] is a native
//! `async fn` trait dispatched **statically** (`impl Future + Send`, no `async-trait`,
//! no trait object). `is_available()` returns `false` (never an error) when the
//! sidecar can't be resolved.

use std::future::Future;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, RecordingError};
use crate::vm::{
    RecordingCapabilitiesVm, RecordingSourcesVm, ScreenRecordingAccess, TccPermission,
};

/// The states a screen-recording session walks through (Story 16.2, AD-33).
///
/// Terminal states are [`SessionState::Finalized`], [`SessionState::Recovered`], and
/// [`SessionState::Failed`] — no transition leaves them. `Recovered` is a reachable
/// terminal here (a salvaged partial finalize); full crash-recovery *entry* semantics
/// are Epic 17.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// No capture in progress — the initial state after [`RecordingSession::new`].
    Idle,
    /// The sidecar is running its pre-flight (permission / source checks) before
    /// capture starts.
    Preflight,
    /// Capture is live, writing the current segment.
    Recording,
    /// A segment rotation is in progress (closing the current fragment, opening the
    /// next).
    Rotating,
    /// A stop was requested; the sidecar is finalizing the output.
    Stopping,
    /// Terminal — the recording finalized cleanly into a playable file.
    Finalized,
    /// Terminal — a partial recording was salvaged/finalized after an interruption.
    Recovered,
    /// Terminal — the session failed.
    Failed,
}

/// A sidecar-reported fact that drives a [`RecordingSession`] transition (Story 16.2).
///
/// These are the events the [`Recorder`] port parses out of `keeper-rec`'s stdout
/// NDJSON (via [`parse_event`]) and feeds into the machine — host intent
/// (start/stop) becomes a command the sidecar acts on and then *reports back* as one
/// of these, so the core needs no process handle or command side-channel.
// (No `Eq`: the Story 17.4 `pts_start`/`pts_end` bounds are `f64` seconds.)
#[derive(Debug, Clone, PartialEq)]
pub enum RecordingEvent {
    /// The sidecar started its pre-flight (`Idle → Preflight`).
    PreflightStarted,
    /// Capture started (`Preflight → Recording`, or `Rotating → Recording`).
    CaptureStarted,
    /// A segment rotation started (`Recording → Rotating`).
    SegmentRotating,
    /// A segment finished closing. Legal only while `Recording`/`Rotating`; it bumps
    /// the session's segment counter and does NOT change state.
    ///
    /// Story 17.1 enriched the sidecar's `segmentClosed` line with the closed
    /// file's `path`/`bytes`/`track`; Story 17.2 carries them here (best-effort —
    /// absent or mistyped extras parse as `None`, never a dropped event) so the
    /// shell's segment ledger needs no second parse path.
    SegmentClosed {
        /// The zero-based index of the segment that just closed.
        index: u32,
        /// The absolute path of the closed segment file, when the sidecar
        /// reported it (Story 17.1's enriched shape).
        path: Option<String>,
        /// The closed segment's size in bytes, when reported. Informational —
        /// the terminal manifest reconcile always re-reads the authoritative
        /// size from disk.
        bytes: Option<u64>,
        /// The track the segment belongs to (`"screen"` in Epic 17), when
        /// reported.
        track: Option<String>,
        /// The segment's first appended video sample's PTS in **original
        /// capture-clock seconds** (Story 17.4, NFR-22), when reported. The
        /// muxer rebases every segment file's own timeline to 0, so this
        /// host-clock bound exists only at capture time — the manifest
        /// persists it for the gapless-concat gate.
        pts_start: Option<f64>,
        /// The segment's last appended video sample's PTS in original
        /// capture-clock seconds, when reported.
        pts_end: Option<f64>,
    },
    /// A stop was requested / begun (`Recording → Stopping`, or `Rotating → Stopping`).
    Stopping,
    /// The recording finalized cleanly (`Stopping → Finalized`) — terminal.
    Finalized,
    /// A partial recording was salvaged (`Stopping → Recovered`) — terminal.
    Recovered,
    /// The session failed (from any non-terminal state → `Failed`) — terminal.
    Failed {
        /// A non-secret description of the failure (never a path, token, or media
        /// bytes).
        message: String,
    },
}

/// The platform-free recording session state machine (Story 16.2, AD-33).
///
/// Holds only the current [`SessionState`] and a segment counter — **never** a
/// `keeper-rec` process handle. It advances solely through [`RecordingSession::apply`]
/// on events the [`Recorder`] port feeds in.
#[derive(Debug, Clone)]
pub struct RecordingSession {
    state: SessionState,
    segments_closed: u32,
}

impl RecordingSession {
    /// A fresh session in [`SessionState::Idle`] with no segments closed.
    pub fn new() -> Self {
        Self {
            state: SessionState::Idle,
            segments_closed: 0,
        }
    }

    /// The current session state.
    pub fn state(&self) -> SessionState {
        self.state
    }

    /// The number of segments that have closed so far (bumped by
    /// [`RecordingEvent::SegmentClosed`]).
    pub fn segments_closed(&self) -> u32 {
        self.segments_closed
    }

    /// Apply a sidecar-reported [`RecordingEvent`], advancing the machine per the
    /// transition table, or rejecting an illegal transition with
    /// [`RecordingError::IllegalTransition`] (state left unchanged).
    ///
    /// Transition table (terminals: `Finalized`, `Recovered`, `Failed`):
    ///
    /// ```text
    /// Idle      → Preflight
    /// Preflight → Recording | Failed
    /// Recording → Rotating | Stopping | Failed
    /// Rotating  → Recording | Stopping | Failed
    /// Stopping  → Finalized | Recovered | Failed
    /// ```
    ///
    /// [`RecordingEvent::SegmentClosed`] is legal only in `Recording`/`Rotating`
    /// (bumps the counter, no state change). Anything else →
    /// [`RecordingError::IllegalTransition`].
    pub fn apply(&mut self, event: RecordingEvent) -> Result<(), RecordingError> {
        use RecordingEvent as E;
        use SessionState as S;

        // A `Failed` event is legal from any non-terminal state — the sidecar can
        // report a failure at any point before a terminal is reached.
        if let E::Failed { .. } = event {
            return match self.state {
                S::Finalized | S::Recovered | S::Failed => Err(self.illegal(&event)),
                _ => {
                    self.state = S::Failed;
                    Ok(())
                }
            };
        }

        // `SegmentClosed` is legal only while capturing/rotating; it bumps the
        // counter and never changes state.
        if let E::SegmentClosed { .. } = event {
            return match self.state {
                S::Recording | S::Rotating => {
                    self.segments_closed = self.segments_closed.saturating_add(1);
                    Ok(())
                }
                _ => Err(self.illegal(&event)),
            };
        }

        let next = match (self.state, &event) {
            (S::Idle, E::PreflightStarted) => S::Preflight,
            (S::Preflight, E::CaptureStarted) => S::Recording,
            (S::Recording, E::SegmentRotating) => S::Rotating,
            (S::Recording, E::Stopping) => S::Stopping,
            (S::Rotating, E::CaptureStarted) => S::Recording,
            (S::Rotating, E::Stopping) => S::Stopping,
            (S::Stopping, E::Finalized) => S::Finalized,
            (S::Stopping, E::Recovered) => S::Recovered,
            _ => return Err(self.illegal(&event)),
        };
        self.state = next;
        Ok(())
    }

    /// Build the honest [`RecordingError::IllegalTransition`] for `event` in the
    /// current state (a stable, secret-free event label).
    fn illegal(&self, event: &RecordingEvent) -> RecordingError {
        RecordingError::IllegalTransition {
            from: self.state,
            event: event_label(event).to_owned(),
        }
    }
}

impl Default for RecordingSession {
    fn default() -> Self {
        Self::new()
    }
}

/// A stable, secret-free label for a [`RecordingEvent`] used in the illegal-transition
/// error message (never carries the `Failed` message text or a segment index).
fn event_label(event: &RecordingEvent) -> &'static str {
    match event {
        RecordingEvent::PreflightStarted => "preflightStarted",
        RecordingEvent::CaptureStarted => "captureStarted",
        RecordingEvent::SegmentRotating => "segmentRotating",
        RecordingEvent::SegmentClosed { .. } => "segmentClosed",
        RecordingEvent::Stopping => "stopping",
        RecordingEvent::Finalized => "finalized",
        RecordingEvent::Recovered => "recovered",
        RecordingEvent::Failed { .. } => "failed",
    }
}

/// Parse one `keeper-rec` → host NDJSON line into a [`RecordingEvent`], or `None`
/// when the line carries no recognized event (unrecognized / malformed lines are
/// dropped — never a panic). Pure and unit-tested.
///
/// **Provisional shape (code-owned).** The field names below are *provisional*, like
/// bbctl's provisional prose markers — the typed RPC contract is finalized in
/// 16.4/16.6. A line is a JSON object with an `"event"` discriminator:
///
/// - `"event":"state"` + `"state":"<s>"` — maps `"preflight"`, `"recording"`,
///   `"rotating"`, `"stopping"`, `"finalized"`, `"recovered"` to the matching event.
/// - `"event":"segmentClosed"` + `"index":<u32>` — [`RecordingEvent::SegmentClosed`].
/// - `"event":"error"` + `"message":"<m>"` — [`RecordingEvent::Failed`].
///
/// Anything else (unknown `event`, unknown `state`, missing/mistyped field, non-JSON)
/// returns `None`.
pub fn parse_event(line: &str) -> Option<RecordingEvent> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let obj = value.as_object()?;
    match obj.get("event")?.as_str()? {
        "state" => match obj.get("state")?.as_str()? {
            "preflight" => Some(RecordingEvent::PreflightStarted),
            "recording" => Some(RecordingEvent::CaptureStarted),
            "rotating" => Some(RecordingEvent::SegmentRotating),
            "stopping" => Some(RecordingEvent::Stopping),
            "finalized" => Some(RecordingEvent::Finalized),
            "recovered" => Some(RecordingEvent::Recovered),
            _ => None,
        },
        "segmentClosed" => {
            let index = u32::try_from(obj.get("index")?.as_u64()?).ok()?;
            // The Story 17.1 enrichment (`path`/`bytes`/`track`) is read
            // best-effort: an absent or mistyped extra parses as `None` — a
            // bare index-only `segmentClosed` stays a legal event, never a
            // dropped one (the counter bump must survive a lean sidecar line).
            let path = obj
                .get("path")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            let bytes = obj.get("bytes").and_then(serde_json::Value::as_u64);
            let track = obj
                .get("track")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            // Story 17.4's host-clock PTS bounds (seconds) — same best-effort
            // tolerance: absent/mistyped bounds degrade to `None`. A
            // non-finite value is rejected too, so the bounds persisted on the
            // manifest stay provably finite (`serde_json` refuses to serialize
            // NaN/Infinity, which would otherwise fail a terminal manifest
            // write for the whole session).
            let finite = |v: &serde_json::Value| v.as_f64().filter(|f| f.is_finite());
            let pts_start = obj.get("ptsStart").and_then(finite);
            let pts_end = obj.get("ptsEnd").and_then(finite);
            Some(RecordingEvent::SegmentClosed {
                index,
                path,
                bytes,
                track,
                pts_start,
                pts_end,
            })
        }
        "error" => {
            // A malformed / absent `message` must NOT swallow the failure — surface a
            // `Failed` with a generic, non-secret fallback so the machine still reaches
            // its `Failed` terminal (a lost error would strand the session as if capture
            // were still live).
            let message = obj
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("keeper-rec reported an unspecified error")
                .to_owned();
            Some(RecordingEvent::Failed { message })
        }
        _ => None,
    }
}

// --- NDJSON-RPC request/response wire half (Story 16.4, AD-34) -----------------
//
// The host→sidecar request channel: id-correlated, one JSON object per line.
// Everything here is pure string/JSON logic — the round-trip I/O (spawn, piped
// stdin, id-correlated read, reap) lives only in the shell port
// (`keeper/src/recorder.rs`, `#[cfg(desktop)]`), keeping the dependency firewall
// intact.

/// The NDJSON-RPC protocol version keeper expects `keeper-rec` to speak (Story
/// 16.4, AD-34). Carried by the `getCapabilities` handshake: the sidecar reports
/// its version in `result.protocolVersion` and the host compares it against this
/// constant via [`verify_protocol_version`]. A mismatch is an honest
/// [`CoreError::Unsupported`], never a crash.
///
/// The sidecar hardcodes the same value in
/// `tools/keeper-rec/Sources/keeper-rec/main.swift` (`capabilitiesResult`) — there
/// is no shared source of truth across the language boundary, so bump both
/// together; a drift surfaces at runtime as an honest `Unsupported`, not silently.
///
/// Story 17.4 (NFR-22) added the additive `ptsStart`/`ptsEnd` fields (original
/// capture-clock seconds) to the sidecar's `segmentClosed` event, consumed
/// tolerantly by [`parse_event`] — per the additive-change precedent (16.5's
/// `requestScreenRecording`, 16.6's `startRecording`/`stop`) the version stays 1.
pub const PROTOCOL_VERSION: u32 = 1;

/// Build the one-line `getCapabilities` request (no trailing newline — the shell
/// port owns line framing). Wire shape: `{"id":<id>,"method":"getCapabilities"}`.
pub fn capabilities_request(id: u64) -> String {
    serde_json::json!({ "id": id, "method": "getCapabilities" }).to_string()
}

/// Build the one-line `listSources` request (no trailing newline — the shell port
/// owns line framing). Wire shape: `{"id":<id>,"method":"listSources"}`.
pub fn list_sources_request(id: u64) -> String {
    serde_json::json!({ "id": id, "method": "listSources" }).to_string()
}

/// Build the one-line `requestScreenRecording` request (Story 16.5, AD-36; no
/// trailing newline — the shell port owns line framing). Wire shape:
/// `{"id":<id>,"method":"requestScreenRecording"}`. Additive to the v1 protocol —
/// keeper and keeper-rec ship in lockstep, so [`PROTOCOL_VERSION`] is unchanged.
pub fn request_screen_recording_request(id: u64) -> String {
    serde_json::json!({ "id": id, "method": "requestScreenRecording" }).to_string()
}

/// The parameters of one capture session (Story 16.6, FR-68/FR-69/FR-71, AD-37).
///
/// The host owns the output path (directory + local-time-stamped filename); the
/// sidecar creates parent directories as needed and writes exactly this file.
/// `display_id` picks a specific display (`None` = the main display);
/// `system_audio` toggles the AAC system-audio track (with keeper's own process
/// audio excluded — FR-69).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionParams {
    /// Absolute path of the `.mp4` file to write.
    pub output_path: String,
    /// The macOS display id to capture, or `None` for the main display.
    pub display_id: Option<u32>,
    /// Whether to capture system audio (true in the walking skeleton).
    pub system_audio: bool,
    /// Segment size in decimal MB before a gapless rotation (Story 17.5,
    /// FR-72); the sidecar's `segmentMB`.
    pub segment_mb: u32,
    /// Duration-cap rotation fallback in whole seconds (Story 17.5, FR-72);
    /// the sidecar's `maxSegmentSeconds`.
    pub max_segment_seconds: u32,
}

/// Build the one-line `startRecording` request (Story 16.6 + 17.5; no trailing
/// newline — the shell port owns line framing). Wire shape:
/// `{"id":<id>,"method":"startRecording","params":{"path":…,"systemAudio":…,
/// "segmentMB":…,"maxSegmentSeconds":…[,"displayId":…]}}`. `segmentMB` /
/// `maxSegmentSeconds` are additive fields the 17.1 sidecar already reads
/// (defaulting when absent), so — per the additive precedent — keeper and
/// keeper-rec ship in lockstep and [`PROTOCOL_VERSION`] is unchanged.
pub fn start_recording_request(id: u64, params: &SessionParams) -> String {
    let mut wire = serde_json::json!({
        "path": params.output_path,
        "systemAudio": params.system_audio,
        "segmentMB": params.segment_mb,
        "maxSegmentSeconds": params.max_segment_seconds,
    });
    if let Some(display_id) = params.display_id {
        wire["displayId"] = display_id.into();
    }
    serde_json::json!({ "id": id, "method": "startRecording", "params": wire }).to_string()
}

/// Build the one-line `stop` request (Story 16.6; no trailing newline — the shell
/// port owns line framing). Wire shape: `{"id":<id>,"method":"stop"}`. The sidecar
/// answers with `stopping`/`finalized` (or `error`) *events*, then exits.
pub fn stop_recording_request(id: u64) -> String {
    serde_json::json!({ "id": id, "method": "stop" }).to_string()
}

/// Extract the correlation `id` from one sidecar stdout line, or `None` when the
/// line is not an id-carrying response (an unsolicited event, garbage, a blank —
/// anything the awaiting reader should skip). Pure and tolerant: never a panic.
pub fn response_id(line: &str) -> Option<u64> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    value.as_object()?.get("id")?.as_u64()
}

/// Shorthand for a non-secret [`RecordingError::Protocol`].
fn protocol_error(message: impl Into<String>) -> RecordingError {
    RecordingError::Protocol(message.into())
}

/// Unwrap the `result` object out of one id-correlated response line, surfacing a
/// sidecar `{id,error}` answer and any malformed/missing `result` as
/// [`RecordingError::Protocol`]. `method` labels the failing call in the message.
fn response_result(line: &str, method: &str) -> Result<serde_json::Value, RecordingError> {
    let value: serde_json::Value = serde_json::from_str(line)
        .map_err(|e| protocol_error(format!("{method}: response is not valid JSON: {e}")))?;
    let obj = value
        .as_object()
        .ok_or_else(|| protocol_error(format!("{method}: response is not a JSON object")))?;
    if let Some(error) = obj.get("error") {
        let code = error
            .get("code")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        let message = error
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("no message");
        return Err(protocol_error(format!(
            "{method}: keeper-rec answered with an error: {code}: {message}"
        )));
    }
    obj.get("result")
        .cloned()
        .ok_or_else(|| protocol_error(format!("{method}: response carries no result object")))
}

/// Parse one TCC permission string out of the response's `permissions` object.
fn parse_tcc(
    permissions: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<TccPermission, RecordingError> {
    let raw = permissions
        .get(key)
        .cloned()
        .ok_or_else(|| protocol_error(format!("getCapabilities: missing permission {key:?}")))?;
    serde_json::from_value(raw).map_err(|e| {
        protocol_error(format!(
            "getCapabilities: unrecognized permission state for {key:?}: {e}"
        ))
    })
}

/// Parse the id-correlated `getCapabilities` response line into a
/// [`RecordingCapabilitiesVm`] (Story 16.4, AD-34).
///
/// Wire → VM mapping is deliberate (the contract shape is the invariant; field
/// lists stay code-owned): the wire carries `macos` and a nested
/// `permissions{screenRecording,microphone,camera}` object, the VM surfaces
/// `macosVersion` and flattened per-TCC states. A sidecar `error` answer and any
/// malformed / missing field surface as [`RecordingError::Protocol`] — never a
/// panic. The protocol-*version* comparison is separate: [`verify_protocol_version`].
pub fn parse_capabilities_result(line: &str) -> Result<RecordingCapabilitiesVm, RecordingError> {
    let result = response_result(line, "getCapabilities")?;
    let obj = result
        .as_object()
        .ok_or_else(|| protocol_error("getCapabilities: result is not an object"))?;
    let protocol_version = obj
        .get("protocolVersion")
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| protocol_error("getCapabilities: missing/mistyped protocolVersion"))?;
    let macos_version = obj
        .get("macos")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| protocol_error("getCapabilities: missing/mistyped macos version"))?
        .to_owned();
    let features = obj
        .get("features")
        .cloned()
        .ok_or_else(|| protocol_error("getCapabilities: missing features object"))
        .and_then(|raw| {
            serde_json::from_value(raw)
                .map_err(|e| protocol_error(format!("getCapabilities: malformed features: {e}")))
        })?;
    let permissions = obj
        .get("permissions")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| protocol_error("getCapabilities: missing permissions object"))?;
    Ok(RecordingCapabilitiesVm {
        protocol_version,
        macos_version,
        features,
        screen_recording: parse_tcc(permissions, "screenRecording")?,
        microphone: parse_tcc(permissions, "microphone")?,
        camera: parse_tcc(permissions, "camera")?,
    })
}

/// Parse the id-correlated `listSources` response line into a
/// [`RecordingSourcesVm`] (Story 16.4, AD-34). The wire `result` matches the VM
/// shape field-for-field; a sidecar `error` answer and any malformed / missing
/// field surface as [`RecordingError::Protocol`] — never a panic.
pub fn parse_sources_result(line: &str) -> Result<RecordingSourcesVm, RecordingError> {
    let result = response_result(line, "listSources")?;
    serde_json::from_value(result)
        .map_err(|e| protocol_error(format!("listSources: malformed result: {e}")))
}

/// Parse the id-correlated `requestScreenRecording` response line (Story 16.5,
/// AD-36) into the sidecar-reported grant outcome (`result.granted`). `true`
/// means the OS reports the grant green after the request; `false` means it was
/// not granted (an explicit denial, a dismissed prompt, or a prior denial where
/// no prompt is shown at all). A sidecar `error` answer and any malformed /
/// missing field surface as [`RecordingError::Protocol`] — never a panic.
pub fn parse_request_screen_recording_result(line: &str) -> Result<bool, RecordingError> {
    let result = response_result(line, "requestScreenRecording")?;
    result
        .as_object()
        .and_then(|obj| obj.get("granted"))
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| protocol_error("requestScreenRecording: missing/mistyped granted"))
}

/// Lift the sidecar's two-valued Screen Recording preflight into the honest
/// tri-state (Story 16.5, FR-67, AD-36) — pure and total, unit-tested without a
/// Mac.
///
/// `preflight` is the sidecar's non-prompting probe (which cannot distinguish an
/// explicit denial from a never-requested state, so it reports `NotDetermined`
/// for both); `requested` is the host's *session* "already requested this app
/// lifetime" flag. Mapping:
///
/// - `Granted → Granted` (regardless of the flag),
/// - `Denied → Denied` (a future preflight that CAN confirm a denial is honored),
/// - `NotDetermined + not yet requested → NotYetRequested` (the OS prompt is
///   still available),
/// - `NotDetermined + already requested → Denied` (the one real prompt per app
///   lifetime is spent; the fix path is System Settings).
pub fn resolve_screen_recording_access(
    preflight: TccPermission,
    requested: bool,
) -> ScreenRecordingAccess {
    match (preflight, requested) {
        (TccPermission::Granted, _) => ScreenRecordingAccess::Granted,
        (TccPermission::Denied, _) => ScreenRecordingAccess::Denied,
        (TccPermission::NotDetermined, false) => ScreenRecordingAccess::NotYetRequested,
        (TccPermission::NotDetermined, true) => ScreenRecordingAccess::Denied,
    }
}

/// Extract just `result.protocolVersion` from a `getCapabilities` response line —
/// nothing else (Story 16.4, AD-34). The handshake must run *before* the full
/// result shape is validated: a future sidecar whose result shape changed across a
/// version bump must still surface an honest version [`CoreError::Unsupported`]
/// (via [`verify_protocol_version`]), not a shape [`RecordingError::Protocol`] from
/// [`parse_capabilities_result`]. A sidecar `error` answer or a missing/mistyped
/// `protocolVersion` is a genuine protocol fault (version cannot be negotiated).
pub fn response_protocol_version(line: &str) -> Result<u32, RecordingError> {
    let result = response_result(line, "getCapabilities")?;
    result
        .as_object()
        .and_then(|obj| obj.get("protocolVersion"))
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| protocol_error("getCapabilities: missing/mistyped protocolVersion"))
}

/// Compare the sidecar-reported protocol version against [`PROTOCOL_VERSION`]
/// (Story 16.4, AD-34). A reachable, parseable sidecar that speaks a different
/// protocol is an *unsupported* condition — the honest not-available funnel —
/// never a crash. (A response we cannot even parse is instead a
/// [`RecordingError::Protocol`] fault, surfaced by the parse fns.)
pub fn verify_protocol_version(reported: u32) -> Result<(), CoreError> {
    if reported == PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(CoreError::Unsupported(format!(
            "unsupported keeper-rec protocol version {reported} (keeper speaks {PROTOCOL_VERSION})"
        )))
    }
}

// --- Session folder, manifest.json & segment ledger (Story 17.2, FR-71, AD-33) --
//
// The per-session folder `keeper-rec <local ts>/` holds the `screen-####.mp4`
// segments plus an atomically-written `manifest.json` an external reader (or
// keeper's own 17.3 recovery) can always parse consistently. Everything here is
// `std::fs` + serde only — firewall-clean (no shell, no Apple framework, no
// process API). The shell's event sink owns the flow: `create` at start,
// `record_segment` per enriched `segmentClosed` (a *live* view), `set_status`
// per state change, and `reconcile_from_dir` at every terminal (disk is the
// authoritative segment list). Core is time-agnostic — the local-timestamp
// string is supplied by the shell, never generated here.

/// The `manifest.json` schema version (Story 17.2).
pub const MANIFEST_VERSION: u32 = 1;

/// Segment files this session owns are named `screen-####.mp4`. The terminal
/// reconcile ingests only this stem prefix, so a stray `*.mp4` (a user drop, or a
/// future Epic 20 `camera-####.mp4` sharing the folder) with a trailing digit run
/// never pollutes the authoritative screen-track ledger.
const SEGMENT_STEM_PREFIX: &str = "screen-";

/// The persisted `status` of a session manifest (Story 17.2). Lowercase on the
/// wire: `"recording" | "finalized" | "recovered" | "failed"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ManifestStatus {
    /// The session is (or was, if the app died) live — every non-terminal state.
    Recording,
    /// Terminal — the session finalized cleanly.
    Finalized,
    /// Terminal — a partial recording was salvaged.
    Recovered,
    /// Terminal — the session failed.
    Failed,
}

impl ManifestStatus {
    /// Map a [`SessionState`] to the persisted status: every non-terminal state
    /// is `"recording"`; the three terminals map to their own status.
    pub fn from_state(state: SessionState) -> Self {
        match state {
            SessionState::Finalized => Self::Finalized,
            SessionState::Recovered => Self::Recovered,
            SessionState::Failed => Self::Failed,
            SessionState::Idle
            | SessionState::Preflight
            | SessionState::Recording
            | SessionState::Rotating
            | SessionState::Stopping => Self::Recording,
        }
    }
}

/// One segment in the manifest's ledger (Story 17.2). `file` is the segment's
/// **basename** (relative to the session folder) so the manifest stays portable
/// and self-describing.
// (No `Eq`: the Story 17.4 PTS bounds are `f64` seconds.)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentEntry {
    /// The zero-based segment index (from `segmentClosed` live, or the
    /// filename's trailing numeric run at reconcile).
    pub index: u32,
    /// The segment file's basename, e.g. `"screen-0003.mp4"`.
    pub file: String,
    /// The segment size in bytes. The terminal reconcile always takes this from
    /// `fs::metadata` — disk is authoritative, never a stale event-fed value.
    pub bytes: u64,
    /// The track the segment belongs to (`"screen"` throughout Epic 17).
    pub track: String,
    /// The segment's first appended video sample's PTS in **original
    /// capture-clock seconds** (Story 17.4, NFR-22) — known only at capture
    /// time (the muxer rebases each file's timeline to 0), so missing bounds
    /// (older sidecar, recovered sessions) are persisted as `null`, never
    /// invented. `#[serde(default)]` keeps pre-17.4 manifests parseable.
    #[serde(default)]
    pub pts_start: Option<f64>,
    /// The segment's last appended video sample's PTS in original
    /// capture-clock seconds, `null` when unknown.
    #[serde(default)]
    pub pts_end: Option<f64>,
}

/// What the session captures (Story 17.2). Epic 17 records a display;
/// application/window targets are a later epic's concern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureTarget {
    /// The target kind — `"display"` throughout Epic 17.
    pub kind: String,
    /// The captured display id, or `None` for the main display.
    pub display_id: Option<u32>,
}

impl CaptureTarget {
    /// A display capture target (`None` = the main display).
    pub fn display(display_id: Option<u32>) -> Self {
        Self {
            kind: "display".to_owned(),
            display_id,
        }
    }
}

/// Which device tracks the session records (Story 17.2). `microphone`/`camera`
/// stay constant `false` until Epic 19/20.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDevices {
    /// Whether the system-audio AAC track is recorded.
    pub system_audio: bool,
    /// Whether a microphone track is recorded (constant `false` in Epic 17).
    pub microphone: bool,
    /// Whether a camera track is recorded (constant `false` in Epic 17).
    pub camera: bool,
}

/// The per-session `manifest.json` plus the folder it lives in (Story 17.2,
/// FR-71, AD-33) — the segment ledger that makes a session self-describing.
///
/// Serialized shape (camelCase; see the story's Design Notes):
/// `{ version, session, status, captureTarget, devices, segments }`. The folder
/// path is runtime-only (`#[serde(skip)]`) — the manifest never persists an
/// absolute path, keeping it portable.
// (No `Eq`: the segment ledger carries `f64` PTS bounds since Story 17.4.)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionManifest {
    /// The manifest schema version ([`MANIFEST_VERSION`]).
    pub version: u32,
    /// The session folder's basename, e.g. `"keeper-rec 2026-07-17 14.23.45"`.
    pub session: String,
    /// The persisted session status.
    pub status: ManifestStatus,
    /// What the session captures.
    pub capture_target: CaptureTarget,
    /// Which device tracks are recorded.
    pub devices: SessionDevices,
    /// The segment ledger — a live event-fed view during recording, rebuilt
    /// authoritatively from disk at every terminal.
    pub segments: Vec<SegmentEntry>,
    /// The absolute session folder path (runtime-only, never serialized).
    #[serde(skip)]
    folder: PathBuf,
}

/// Build a secret-free [`RecordingError::ManifestIo`]: the failing operation name
/// plus the `io::Error` display only — never a filesystem path.
fn manifest_io(operation: &str, error: &std::io::Error) -> RecordingError {
    RecordingError::ManifestIo(format!("{operation}: {error}"))
}

impl SessionManifest {
    /// Create the session folder and its initial `recording` manifest (Story
    /// 17.2). The folder must **not** pre-exist — `fs::create_dir` fails on an
    /// existing directory, so a prior session's folder is never adopted (the
    /// shell disambiguates the name on collision before calling this). Missing
    /// *parent* directories (e.g. `~/Movies/keeper`) are created. Any real fs
    /// failure surfaces as [`RecordingError::ManifestIo`].
    pub fn create(
        folder: PathBuf,
        capture_target: CaptureTarget,
        devices: SessionDevices,
    ) -> Result<Self, RecordingError> {
        if let Some(parent) = folder.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| manifest_io("create session base directory", &e))?;
        }
        // `create_dir` (NOT `create_dir_all`) — an already-existing session
        // folder is an error, never silently reused (same-second restart guard).
        std::fs::create_dir(&folder).map_err(|e| manifest_io("create session folder", &e))?;
        let session = folder
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let manifest = Self {
            version: MANIFEST_VERSION,
            session,
            status: ManifestStatus::Recording,
            capture_target,
            devices,
            segments: Vec::new(),
            folder,
        };
        manifest.write()?;
        Ok(manifest)
    }

    /// The absolute session folder path this manifest lives in.
    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// Append one segment to the ledger — the **live** incremental view an
    /// external reader sees during recording. The event-fed list can be
    /// incomplete or wrong (a suppressed `segmentClosed`, a `bytes` fallback);
    /// the terminal [`Self::reconcile_from_dir`] discards and rebuilds it.
    pub fn record_segment(&mut self, entry: SegmentEntry) {
        self.segments.push(entry);
    }

    /// Set the persisted session status (the caller then [`Self::write`]s).
    pub fn set_status(&mut self, status: ManifestStatus) {
        self.status = status;
    }

    /// Atomically (re)write `manifest.json`: serialize, write the sibling
    /// `.manifest.json.tmp` in the **same** folder, then `fs::rename` it over
    /// `manifest.json` (same-directory rename → atomic on APFS). An external
    /// reader polling the file sees the pre- or post-update manifest, never a
    /// torn one. A failure surfaces as [`RecordingError::ManifestIo`] and
    /// leaves the prior manifest intact (the rename never happened).
    pub fn write(&self) -> Result<(), RecordingError> {
        let json = serde_json::to_vec_pretty(self)
            .map_err(|e| RecordingError::ManifestIo(format!("serialize manifest: {e}")))?;
        let tmp = self.folder.join(".manifest.json.tmp");
        std::fs::write(&tmp, json).map_err(|e| manifest_io("write manifest temp file", &e))?;
        std::fs::rename(&tmp, self.folder.join("manifest.json")).map_err(|e| {
            // The rename never landed — clean up the temp so a failed final write
            // doesn't litter the "self-describing" session folder with a dotfile.
            let _ = std::fs::remove_file(&tmp);
            manifest_io("rename manifest into place", &e)
        })?;
        Ok(())
    }

    /// Rebuild the segment ledger **entirely from the on-disk `.mp4` files** —
    /// run at *every* terminal (`finalized`, `recovered`, `failed`) before the
    /// final write. Disk is authoritative: the event-fed list is discarded, the
    /// index comes from each stem's trailing numeric run
    /// ([`segment_index_from_stem`]), and `bytes` always comes from
    /// `fs::metadata().len()` — never a stale/zero event-fed size. This lists
    /// the final segment (which has no `segmentClosed`), backfills a segment
    /// whose event a mid-rotation stop suppressed (DW-992), and repairs any
    /// wrong size. Best-effort per entry: a non-segment file (no numeric run),
    /// a non-file, or an unreadable entry is skipped with a `tracing::warn` —
    /// never aborting the terminal write. The result is sorted by
    /// `(index, file)` for determinism across equal indices. Only a failure to
    /// read the folder itself is an error.
    ///
    /// **PTS bounds survive the rebuild (Story 17.4, NFR-22).** Disk is the
    /// truth for everything disk can observe (`index`/`file`/`bytes`/`track`),
    /// but the host clock is the truth for time: the muxer rebases every
    /// segment file's own timeline to 0, so the original capture-clock
    /// `pts_start`/`pts_end` can NEVER be recovered from the `.mp4` files.
    /// Each rebuilt entry therefore inherits the bounds the event-fed ledger
    /// already recorded for its index (`None` — persisted as `null` — when no
    /// prior entry exists: the final segment, a DW-992 backfill, an older
    /// sidecar).
    pub fn reconcile_from_dir(&mut self) -> Result<(), RecordingError> {
        let entries =
            std::fs::read_dir(&self.folder).map_err(|e| manifest_io("read session folder", &e))?;
        // Snapshot the only-capture-time-known bounds by index before the
        // event-fed list is discarded.
        let known_bounds: std::collections::HashMap<u32, (Option<f64>, Option<f64>)> = self
            .segments
            .iter()
            .map(|s| (s.index, (s.pts_start, s.pts_end)))
            .collect();
        let mut segments: Vec<SegmentEntry> = Vec::new();
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    tracing::warn!(%error, "manifest reconcile: skipping unreadable dir entry");
                    continue;
                }
            };
            let path = entry.path();
            let Some(file) = path.file_name().and_then(|name| name.to_str()) else {
                tracing::warn!("manifest reconcile: skipping non-UTF-8 file name");
                continue;
            };
            let is_mp4 = path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("mp4"));
            if !is_mp4 {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                tracing::warn!("manifest reconcile: skipping .mp4 with a non-UTF-8 stem");
                continue;
            };
            // Only this session's own `screen-####.mp4` segments are authoritative
            // ledger entries — a stray `*.mp4` with a trailing digit run must not
            // pollute the screen-track list or collide on `index`.
            if !stem.starts_with(SEGMENT_STEM_PREFIX) {
                continue;
            }
            let Some(index) = segment_index_from_stem(stem) else {
                tracing::warn!(
                    "manifest reconcile: skipping screen-* file without a segment index"
                );
                continue;
            };
            // `fs::metadata` (follows symlinks) — the authoritative size of the
            // real file; an unreadable/dangling entry is skipped, never fatal.
            let metadata = match std::fs::metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    tracing::warn!(%error, "manifest reconcile: skipping unreadable segment");
                    continue;
                }
            };
            if !metadata.is_file() {
                tracing::warn!("manifest reconcile: skipping non-file .mp4 entry");
                continue;
            }
            let (pts_start, pts_end) = known_bounds.get(&index).copied().unwrap_or((None, None));
            segments.push(SegmentEntry {
                index,
                file: file.to_owned(),
                bytes: metadata.len(),
                track: "screen".to_owned(),
                pts_start,
                pts_end,
            });
        }
        // Deterministic order even across equal indices (two files whose stems
        // share a numeric run) — sort by (index, file), both compared.
        segments.sort_by(|a, b| (a.index, a.file.as_str()).cmp(&(b.index, b.file.as_str())));
        self.segments = segments;
        Ok(())
    }
}

/// Derive the fs-safe session folder basename `keeper-rec <local ts>` from the
/// shell-supplied local-timestamp string (Story 17.2). Core is time-agnostic —
/// it validates and formats, never generates the timestamp.
///
/// Rejected (as [`RecordingError::ManifestIo`], message secret-free): an empty
/// or all-whitespace timestamp; a path separator (`/`, `\`), a `:`, a NUL, or
/// any control character; a leading dot (a hidden folder); and a trailing `.`
/// or trailing space (some filesystems normalize these away, diverging the
/// folder basename from `manifest.session`).
pub fn session_folder_name(local_ts: &str) -> Result<String, RecordingError> {
    if local_ts.trim().is_empty() {
        return Err(RecordingError::ManifestIo(
            "session timestamp is empty or all-whitespace".to_owned(),
        ));
    }
    if local_ts
        .chars()
        .any(|c| c == '/' || c == '\\' || c == ':' || c.is_control())
    {
        return Err(RecordingError::ManifestIo(
            "session timestamp contains a path separator, colon, or control character".to_owned(),
        ));
    }
    if local_ts.starts_with('.') {
        return Err(RecordingError::ManifestIo(
            "session timestamp starts with a dot".to_owned(),
        ));
    }
    if local_ts.ends_with('.') || local_ts.ends_with(' ') {
        return Err(RecordingError::ManifestIo(
            "session timestamp ends with a dot or space".to_owned(),
        ));
    }
    Ok(format!("keeper-rec {local_ts}"))
}

/// Extract the segment index from a filename stem's **trailing numeric run**
/// (`"screen-0003"` → `3`), or `None` when the stem has no trailing digits (a
/// stray non-segment file) or the run overflows `u32`. Pure and total.
pub fn segment_index_from_stem(stem: &str) -> Option<u32> {
    let run_start = stem
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii_digit())
        .last()
        .map(|(i, _)| i)?;
    stem[run_start..].parse().ok()
}

/// Sum the on-disk byte sizes of this session's own `screen-####.mp4` segment
/// files in `folder` (Story 18.1) — the live figure behind the tray's
/// elapsed/segment/size line.
///
/// Applies the exact ownership rule of [`SessionManifest::reconcile_from_dir`]
/// ([`SEGMENT_STEM_PREFIX`] plus a trailing numeric run), so the growing tray
/// figure always matches what the eventual terminal manifest will report: a
/// stray `*.mp4` (a user drop, a future `camera-####.mp4`), `manifest.json`,
/// and directories never count. Best-effort and total — a missing/unreadable
/// folder or entry contributes 0, never an error, never a panic (the figure is
/// a convenience readout, not a ledger).
pub fn session_bytes_on_disk(folder: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return 0;
    };
    let mut total: u64 = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        let is_mp4 = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("mp4"));
        if !is_mp4 {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if !stem.starts_with(SEGMENT_STEM_PREFIX) || segment_index_from_stem(stem).is_none() {
            continue;
        }
        // `fs::metadata` (follows symlinks) — the real file's current length; a
        // mid-write segment simply reports what has reached disk so far.
        let Ok(metadata) = std::fs::metadata(&path) else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        total = total.saturating_add(metadata.len());
    }
    total
}

/// The screen-recording sidecar port (Story 16.2, AD-24, AD-27) — sits beside
/// [`crate::platform::Platform`].
///
/// Native `async fn` dispatched **statically** via [`drive_session`] — no
/// `async-trait`, no trait object — exactly like [`crate::bridges::bbctl::BbctlRunner`].
/// The production impl (`keeper/src/recorder.rs`, `#[cfg(desktop)]`) spawns
/// `keeper-rec` via [`crate::platform::Platform::sidecar_path`] and streams its parsed
/// NDJSON events; iOS and the not-available paths return [`CoreError::Unsupported`].
pub trait Recorder {
    /// Whether the `keeper-rec` sidecar can be resolved on this host / build.
    /// `false` (never an error) is the honest not-available signal.
    fn is_available(&self) -> bool;

    /// Run one recording session (Story 16.6): start capture per `params`,
    /// forwarding each recognized [`RecordingEvent`] to `on_event` **as it
    /// arrives**. When `stop` resolves, the port requests a graceful
    /// stop-and-finalize (the sidecar then emits its `stopping`/`finalized`
    /// events and ends the stream). Resolves `Ok(())` when the sidecar's event
    /// stream ends cleanly; a spawn/IO failure resolves
    /// [`CoreError::Recording`], and an unavailable sidecar resolves
    /// [`CoreError::Unsupported`]. Never panics on absent / garbage output.
    fn run_session(
        &self,
        params: SessionParams,
        stop: impl Future<Output = ()> + Send + 'static,
        on_event: Box<dyn FnMut(RecordingEvent) + Send>,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;

    /// Run the `getCapabilities` handshake round-trip (Story 16.4, AD-34):
    /// version, feature flags, and per-TCC permission states, with the protocol
    /// version already verified against [`PROTOCOL_VERSION`]. An unavailable
    /// sidecar or a protocol-version mismatch resolves
    /// [`CoreError::Unsupported`]; a malformed response resolves
    /// [`RecordingError::Protocol`] via [`CoreError::Recording`]. Never a panic.
    fn get_capabilities(
        &self,
    ) -> impl Future<Output = Result<RecordingCapabilitiesVm, CoreError>> + Send;

    /// Run the `listSources` round-trip (Story 16.4, AD-34): the displays /
    /// applications / microphones / cameras the sidecar can offer as capture
    /// sources. Same error surface as [`Recorder::get_capabilities`].
    fn list_sources(&self) -> impl Future<Output = Result<RecordingSourcesVm, CoreError>> + Send;

    /// Run the `requestScreenRecording` round-trip (Story 16.5, AD-36): ask the
    /// sidecar to request Screen Recording access (the OS shows its one real
    /// prompt where allowed) and resolve the reported grant outcome — `true` when
    /// the grant is green after the request, `false` otherwise. Same error
    /// surface as [`Recorder::get_capabilities`].
    fn request_screen_recording(&self) -> impl Future<Output = Result<bool, CoreError>> + Send;
}

/// Drive one recording session to its terminal [`SessionState`] (Story 16.2).
/// Statically dispatched over `R: Recorder`.
///
/// Feeds each event the recorder reports into a [`RecordingSession`], reporting every
/// successful state change via `on_state`. Returns the terminal state on a clean
/// stream, or the **first** transition error (as [`CoreError::Recording`]) — a
/// sidecar/run failure surfaces as the recorder's own [`CoreError`]. Lock-simple and
/// panic-free.
///
/// `on_state` is flushed when the run **resolves**, not incrementally — the live,
/// per-event feed is [`Recorder::run_session`]'s `on_event` sink; `drive_session` is
/// the convenience that folds those events into a session and replays the observed
/// state changes (a live progress feed lands when a real consumer needs one, 16.3+).
/// Buffered transitions are replayed even when the run ultimately errors, so a
/// mid-stream sidecar/IO failure never silently discards the progress already seen.
pub async fn drive_session<R: Recorder>(
    recorder: &R,
    params: SessionParams,
    stop: impl Future<Output = ()> + Send + 'static,
    mut on_state: impl FnMut(SessionState) + Send,
) -> Result<SessionState, CoreError> {
    use std::sync::{Arc, Mutex};

    // The `on_event` sink must be `'static` (it is a `Box<dyn FnMut … + Send>`), so it
    // cannot borrow the non-`'static` `on_state`. The sink drives its own session and
    // records each successful state change into a shared buffer; the caller's
    // `on_state` is invoked from this outer scope after the run resolves. Since events
    // are only delivered while `run_session` is awaited, no state change is lost.
    let session = Arc::new(Mutex::new(RecordingSession::new()));
    let transitions: Arc<Mutex<Vec<SessionState>>> = Arc::new(Mutex::new(Vec::new()));
    let first_error: Arc<Mutex<Option<RecordingError>>> = Arc::new(Mutex::new(None));

    let sink = {
        let session = session.clone();
        let transitions = transitions.clone();
        let first_error = first_error.clone();
        Box::new(move |event: RecordingEvent| {
            // A poisoned lock would only happen on a prior panic while held; recover
            // the guard rather than panicking again (panic-free contract).
            let mut guard = session.lock().unwrap_or_else(|e| e.into_inner());
            match guard.apply(event) {
                Ok(()) => transitions
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(guard.state()),
                Err(e) => {
                    let mut slot = first_error.lock().unwrap_or_else(|e| e.into_inner());
                    if slot.is_none() {
                        *slot = Some(e);
                    }
                }
            }
        }) as Box<dyn FnMut(RecordingEvent) + Send>
    };

    let run_result = recorder.run_session(params, stop, sink).await;

    // Replay every buffered transition BEFORE propagating any error, so a mid-stream
    // sidecar/IO failure never silently discards the progress already observed.
    for state in transitions
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .drain(..)
    {
        on_state(state);
    }

    // Precedence: a spawn/IO failure from the recorder, then the first illegal
    // transition, then the terminal state.
    run_result?;
    if let Some(e) = first_error.lock().unwrap_or_else(|e| e.into_inner()).take() {
        return Err(e.into());
    }
    let terminal = session.lock().unwrap_or_else(|e| e.into_inner()).state();
    Ok(terminal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    /// A bare index-only [`RecordingEvent::SegmentClosed`] (no 17.1/17.4
    /// enrichment) — the shape most state-machine tests need.
    fn segment_closed(index: u32) -> RecordingEvent {
        RecordingEvent::SegmentClosed {
            index,
            path: None,
            bytes: None,
            track: None,
            pts_start: None,
            pts_end: None,
        }
    }

    // --- pure transition table ---------------------------------------------

    #[test]
    fn new_session_starts_idle() {
        let session = RecordingSession::new();
        assert_eq!(session.state(), SessionState::Idle);
        assert_eq!(session.segments_closed(), 0);
    }

    #[test]
    fn full_lifecycle_walks_every_state() {
        let mut session = RecordingSession::new();
        let steps = [
            (RecordingEvent::PreflightStarted, SessionState::Preflight),
            (RecordingEvent::CaptureStarted, SessionState::Recording),
            (RecordingEvent::SegmentRotating, SessionState::Rotating),
            (RecordingEvent::CaptureStarted, SessionState::Recording),
            (RecordingEvent::Stopping, SessionState::Stopping),
            (RecordingEvent::Finalized, SessionState::Finalized),
        ];
        for (event, expected) in steps {
            session.apply(event).expect("legal transition");
            assert_eq!(session.state(), expected);
        }
        assert_eq!(session.state(), SessionState::Finalized);
    }

    #[test]
    fn segment_closed_bumps_counter_without_changing_state() {
        let mut session = RecordingSession::new();
        session
            .apply(RecordingEvent::PreflightStarted)
            .expect("legal transition");
        session
            .apply(RecordingEvent::CaptureStarted)
            .expect("legal transition");
        session
            .apply(segment_closed(0))
            .expect("segmentClosed legal while Recording");
        assert_eq!(session.state(), SessionState::Recording);
        assert_eq!(session.segments_closed(), 1);
        // Legal while Rotating too.
        session
            .apply(RecordingEvent::SegmentRotating)
            .expect("legal transition");
        session
            .apply(segment_closed(1))
            .expect("segmentClosed legal while Rotating");
        assert_eq!(session.state(), SessionState::Rotating);
        assert_eq!(session.segments_closed(), 2);
    }

    #[test]
    fn failure_branch_reaches_failed_terminal() {
        let mut session = RecordingSession::new();
        session
            .apply(RecordingEvent::PreflightStarted)
            .expect("legal transition");
        session
            .apply(RecordingEvent::CaptureStarted)
            .expect("legal transition");
        session
            .apply(RecordingEvent::Failed {
                message: "sidecar crashed".to_owned(),
            })
            .expect("Failed is legal from Recording");
        assert_eq!(session.state(), SessionState::Failed);
    }

    #[test]
    fn recovered_branch_reaches_recovered_terminal() {
        let mut session = RecordingSession::new();
        session
            .apply(RecordingEvent::PreflightStarted)
            .expect("legal transition");
        session
            .apply(RecordingEvent::CaptureStarted)
            .expect("legal transition");
        session
            .apply(RecordingEvent::Stopping)
            .expect("legal transition");
        session
            .apply(RecordingEvent::Recovered)
            .expect("Recovered is legal from Stopping");
        assert_eq!(session.state(), SessionState::Recovered);
    }

    #[test]
    fn illegal_segment_closed_while_idle_is_rejected() {
        let mut session = RecordingSession::new();
        let err = session
            .apply(segment_closed(0))
            .expect_err("segmentClosed while Idle is illegal");
        match err {
            RecordingError::IllegalTransition { from, event } => {
                assert_eq!(from, SessionState::Idle);
                assert_eq!(event, "segmentClosed");
            }
            other => panic!("expected IllegalTransition, got {other:?}"),
        }
        // State unchanged.
        assert_eq!(session.state(), SessionState::Idle);
    }

    #[test]
    fn illegal_capture_started_while_idle_is_rejected() {
        let mut session = RecordingSession::new();
        let err = session
            .apply(RecordingEvent::CaptureStarted)
            .expect_err("captureStarted while Idle is illegal");
        assert!(matches!(
            err,
            RecordingError::IllegalTransition {
                from: SessionState::Idle,
                ..
            }
        ));
        assert_eq!(session.state(), SessionState::Idle);
    }

    #[test]
    fn terminal_states_reject_further_events() {
        let mut session = RecordingSession::new();
        session
            .apply(RecordingEvent::PreflightStarted)
            .expect("legal transition");
        session
            .apply(RecordingEvent::CaptureStarted)
            .expect("legal transition");
        session
            .apply(RecordingEvent::Stopping)
            .expect("legal transition");
        session
            .apply(RecordingEvent::Finalized)
            .expect("legal transition");
        assert_eq!(session.state(), SessionState::Finalized);
        // No event, including Failed, escapes a terminal.
        assert!(session.apply(RecordingEvent::CaptureStarted).is_err());
        assert!(session
            .apply(RecordingEvent::Failed {
                message: "late".to_owned()
            })
            .is_err());
        assert_eq!(session.state(), SessionState::Finalized);
    }

    // --- parse_event fixtures ----------------------------------------------

    #[test]
    fn parse_state_lines_map_to_events() {
        assert_eq!(
            parse_event(r#"{"event":"state","state":"preflight"}"#),
            Some(RecordingEvent::PreflightStarted)
        );
        assert_eq!(
            parse_event(r#"{"event":"state","state":"recording"}"#),
            Some(RecordingEvent::CaptureStarted)
        );
        assert_eq!(
            parse_event(r#"{"event":"state","state":"rotating"}"#),
            Some(RecordingEvent::SegmentRotating)
        );
        assert_eq!(
            parse_event(r#"{"event":"state","state":"stopping"}"#),
            Some(RecordingEvent::Stopping)
        );
        assert_eq!(
            parse_event(r#"{"event":"state","state":"finalized"}"#),
            Some(RecordingEvent::Finalized)
        );
        assert_eq!(
            parse_event(r#"{"event":"state","state":"recovered"}"#),
            Some(RecordingEvent::Recovered)
        );
    }

    #[test]
    fn parse_segment_closed_reads_index() {
        assert_eq!(
            parse_event(r#"{"event":"segmentClosed","index":3}"#),
            Some(segment_closed(3))
        );
    }

    #[test]
    fn parse_enriched_segment_closed_yields_the_ledger_fields() {
        // Story 17.1/17.2/17.4 cross-language seam guard: the sidecar's
        // enriched `segmentClosed` (`path`/`bytes`/`track` + the 17.4
        // host-clock `ptsStart`/`ptsEnd` bounds) parses into the enriched
        // event carrying the segment-ledger data, and the state machine still
        // bumps its counter without a state change.
        let line = r#"{"event":"segmentClosed","index":2,"path":"/rec/keeper/screen-0002.mp4","bytes":123,"track":"screen","ptsStart":1000.5,"ptsEnd":1030.25}"#;
        assert_eq!(
            parse_event(line),
            Some(RecordingEvent::SegmentClosed {
                index: 2,
                path: Some("/rec/keeper/screen-0002.mp4".to_owned()),
                bytes: Some(123),
                track: Some("screen".to_owned()),
                pts_start: Some(1000.5),
                pts_end: Some(1030.25),
            })
        );
        let mut session = RecordingSession::new();
        session
            .apply(RecordingEvent::PreflightStarted)
            .expect("legal transition");
        session
            .apply(RecordingEvent::CaptureStarted)
            .expect("legal transition");
        let event = parse_event(line).expect("enriched line parses");
        session
            .apply(event)
            .expect("segmentClosed legal while Recording");
        assert_eq!(session.state(), SessionState::Recording);
        assert_eq!(session.segments_closed(), 1);
    }

    #[test]
    fn parse_bare_segment_closed_is_tolerated_with_none_extras() {
        // Absent OR mistyped enrichment fields must not drop the event — the
        // index is the only required field; extras (including the 17.4 PTS
        // bounds) degrade to `None`.
        assert_eq!(
            parse_event(r#"{"event":"segmentClosed","index":5}"#),
            Some(segment_closed(5))
        );
        assert_eq!(
            parse_event(
                r#"{"event":"segmentClosed","index":5,"path":7,"bytes":"big","track":[],"ptsStart":"soon","ptsEnd":null}"#
            ),
            Some(segment_closed(5))
        );
    }

    #[test]
    fn parse_segment_closed_bounds_are_read_independently() {
        // One present + one absent/mistyped bound must not couple: each bound
        // degrades to `None` on its own (a partial report keeps what it can).
        assert_eq!(
            parse_event(r#"{"event":"segmentClosed","index":1,"ptsStart":2.5}"#),
            Some(RecordingEvent::SegmentClosed {
                index: 1,
                path: None,
                bytes: None,
                track: None,
                pts_start: Some(2.5),
                pts_end: None,
            })
        );
        assert_eq!(
            parse_event(r#"{"event":"segmentClosed","index":1,"ptsStart":"x","ptsEnd":9.75}"#),
            Some(RecordingEvent::SegmentClosed {
                index: 1,
                path: None,
                bytes: None,
                track: None,
                pts_start: None,
                pts_end: Some(9.75),
            })
        );
    }

    #[test]
    fn parse_error_reads_message() {
        assert_eq!(
            parse_event(r#"{"event":"error","message":"boom"}"#),
            Some(RecordingEvent::Failed {
                message: "boom".to_owned()
            })
        );
    }

    #[test]
    fn parse_error_without_message_still_fails_never_swallows() {
        // A failure report with a missing/mistyped `message` must NOT be dropped — it
        // surfaces a `Failed` with a fallback so the session still reaches its terminal.
        for line in [
            r#"{"event":"error"}"#,
            r#"{"event":"error","message":42}"#,
            r#"{"event":"error","message":{"nested":true}}"#,
        ] {
            assert!(
                matches!(parse_event(line), Some(RecordingEvent::Failed { .. })),
                "error line {line:?} must surface a Failed, never None"
            );
        }
    }

    #[test]
    fn parse_unknown_and_malformed_lines_return_none() {
        // Unknown discriminator.
        assert_eq!(parse_event(r#"{"event":"heartbeat"}"#), None);
        // Unknown state value.
        assert_eq!(parse_event(r#"{"event":"state","state":"warp"}"#), None);
        // Missing required field.
        assert_eq!(parse_event(r#"{"event":"segmentClosed"}"#), None);
        assert_eq!(parse_event(r#"{"event":"state"}"#), None);
        // Wrong field type.
        assert_eq!(
            parse_event(r#"{"event":"segmentClosed","index":"x"}"#),
            None
        );
        // Not JSON / not an object / empty.
        assert_eq!(parse_event("not json at all"), None);
        assert_eq!(parse_event("[1,2,3]"), None);
        assert_eq!(parse_event(""), None);
    }

    /// A recorded multi-line NDJSON fixture stream (Story 16.4 AC): `state` /
    /// `segmentClosed` / `error` lines interleaved with a blank and a garbage line.
    /// Replaying it line-by-line through [`parse_event`] must yield exactly the
    /// recognized event sequence, skipping blank/garbage — no hardware, no panic.
    const EVENT_FIXTURE_STREAM: &str = concat!(
        r#"{"event":"state","state":"preflight"}"#,
        "\n",
        r#"{"event":"state","state":"recording"}"#,
        "\n",
        "\n", // blank line — skipped
        r#"{"event":"segmentClosed","index":0}"#,
        "\n",
        "this line is garbage, not JSON\n",
        r#"{"event":"state","state":"rotating"}"#,
        "\n",
        r#"{"event":"state","state":"recording"}"#,
        "\n",
        r#"{"event":"error","message":"disk full"}"#,
        "\n",
    );

    #[test]
    fn event_fixture_stream_replays_to_exact_sequence() {
        let events: Vec<RecordingEvent> = EVENT_FIXTURE_STREAM
            .lines()
            .filter_map(parse_event)
            .collect();
        assert_eq!(
            events,
            vec![
                RecordingEvent::PreflightStarted,
                RecordingEvent::CaptureStarted,
                segment_closed(0),
                RecordingEvent::SegmentRotating,
                RecordingEvent::CaptureStarted,
                RecordingEvent::Failed {
                    message: "disk full".to_owned()
                },
            ],
            "blank/garbage lines must be skipped, recognized lines mapped in order"
        );
    }

    // --- NDJSON-RPC wire half (Story 16.4) ----------------------------------

    /// A healthy `getCapabilities` response line matching [`PROTOCOL_VERSION`].
    const CAPABILITIES_RESPONSE: &str = r#"{"id":1,"result":{"protocolVersion":1,"macos":"15.5.0","features":{"systemAudio":true,"microphone":false,"camera":false},"permissions":{"screenRecording":"granted","microphone":"notDetermined","camera":"notDetermined"}}}"#;

    /// A healthy `listSources` response line (real display, deferred lists empty).
    const SOURCES_RESPONSE: &str = r#"{"id":2,"result":{"displays":[{"id":1,"width":3456,"height":2234,"isMain":true}],"applications":[],"microphones":[],"cameras":[]}}"#;

    #[test]
    fn request_builders_emit_the_wire_shape() {
        assert_eq!(
            capabilities_request(1),
            r#"{"id":1,"method":"getCapabilities"}"#
        );
        assert_eq!(
            list_sources_request(2),
            r#"{"id":2,"method":"listSources"}"#
        );
        assert_eq!(
            request_screen_recording_request(3),
            r#"{"id":3,"method":"requestScreenRecording"}"#
        );
        // No line framing inside the request — the shell port owns the newline.
        assert!(!capabilities_request(7).contains('\n'));
        assert!(!request_screen_recording_request(7).contains('\n'));
    }

    #[test]
    fn start_recording_request_carries_the_segmentation_params() {
        // Story 17.5 (FR-72): every `start` always carries the configured
        // segment size and duration cap (in seconds) alongside the 16.6 fields.
        let params = SessionParams {
            output_path: "/tmp/keeper-rec/screen-0000.mp4".to_owned(),
            display_id: Some(7),
            system_audio: true,
            segment_mb: 800,
            max_segment_seconds: 2700,
        };
        let line = start_recording_request(4, &params);
        assert!(!line.contains('\n'), "the shell port owns line framing");
        let wire: serde_json::Value = serde_json::from_str(&line).expect("request is JSON");
        assert_eq!(wire["id"], 4);
        assert_eq!(wire["method"], "startRecording");
        assert_eq!(wire["params"]["path"], "/tmp/keeper-rec/screen-0000.mp4");
        assert_eq!(wire["params"]["systemAudio"], true);
        assert_eq!(wire["params"]["displayId"], 7);
        assert_eq!(wire["params"]["segmentMB"], 800);
        assert_eq!(wire["params"]["maxSegmentSeconds"], 2700);

        // Without a display id the segmentation fields still always appear.
        let main_display = SessionParams {
            display_id: None,
            ..params
        };
        let wire: serde_json::Value =
            serde_json::from_str(&start_recording_request(5, &main_display))
                .expect("request is JSON");
        assert!(wire["params"].get("displayId").is_none());
        assert_eq!(wire["params"]["segmentMB"], 800);
        assert_eq!(wire["params"]["maxSegmentSeconds"], 2700);
    }

    // --- Screen Recording pre-flight (Story 16.5) ----------------------------

    #[test]
    fn resolve_screen_recording_access_covers_every_branch() {
        use ScreenRecordingAccess as A;
        use TccPermission as P;
        // Granted wins regardless of the session flag.
        assert_eq!(
            resolve_screen_recording_access(P::Granted, false),
            A::Granted
        );
        assert_eq!(
            resolve_screen_recording_access(P::Granted, true),
            A::Granted
        );
        // An (already-confirmed) denial is a denial regardless of the flag.
        assert_eq!(resolve_screen_recording_access(P::Denied, false), A::Denied);
        assert_eq!(resolve_screen_recording_access(P::Denied, true), A::Denied);
        // Undetermined: the flag decides — prompt still available vs spent.
        assert_eq!(
            resolve_screen_recording_access(P::NotDetermined, false),
            A::NotYetRequested
        );
        assert_eq!(
            resolve_screen_recording_access(P::NotDetermined, true),
            A::Denied
        );
    }

    #[test]
    fn parse_request_screen_recording_result_reads_granted() {
        assert!(
            parse_request_screen_recording_result(r#"{"id":3,"result":{"granted":true}}"#)
                .expect("granted true")
        );
        assert!(
            !parse_request_screen_recording_result(r#"{"id":3,"result":{"granted":false}}"#)
                .expect("granted false")
        );
    }

    #[test]
    fn parse_request_screen_recording_result_surfaces_protocol_faults() {
        // Non-JSON, missing result, sidecar error answer, missing/mistyped
        // `granted` — all surface RecordingError::Protocol, never a panic.
        let cases = [
            "not json",
            r#"{"id":3}"#,
            r#"{"id":3,"error":{"code":"unknownMethod","message":"nope"}}"#,
            r#"{"id":3,"result":{}}"#,
            r#"{"id":3,"result":{"granted":"yes"}}"#,
            r#"{"id":3,"result":"granted"}"#,
        ];
        for line in cases {
            assert!(
                matches!(
                    parse_request_screen_recording_result(line),
                    Err(RecordingError::Protocol(_))
                ),
                "line {line:?} must surface a Protocol error"
            );
        }
    }

    #[test]
    fn response_id_extracts_only_id_carrying_lines() {
        assert_eq!(response_id(CAPABILITIES_RESPONSE), Some(1));
        assert_eq!(response_id(SOURCES_RESPONSE), Some(2));
        assert_eq!(
            response_id(r#"{"id":9,"error":{"code":"x","message":"y"}}"#),
            Some(9)
        );
        // Unsolicited events, garbage, blanks → None (the awaiting reader skips them).
        assert_eq!(
            response_id(r#"{"event":"state","state":"recording"}"#),
            None
        );
        assert_eq!(response_id("garbage"), None);
        assert_eq!(response_id(""), None);
        assert_eq!(response_id(r#"{"id":"not-a-number"}"#), None);
    }

    #[test]
    fn parse_capabilities_result_maps_the_wire_to_the_vm() {
        let vm = parse_capabilities_result(CAPABILITIES_RESPONSE).expect("healthy response");
        assert_eq!(vm.protocol_version, PROTOCOL_VERSION);
        assert_eq!(vm.macos_version, "15.5.0");
        assert!(vm.features.system_audio);
        assert!(!vm.features.microphone);
        assert!(!vm.features.camera);
        assert_eq!(vm.screen_recording, TccPermission::Granted);
        assert_eq!(vm.microphone, TccPermission::NotDetermined);
        assert_eq!(vm.camera, TccPermission::NotDetermined);
    }

    #[test]
    fn parse_capabilities_result_surfaces_protocol_faults() {
        // Non-JSON, missing result, sidecar error answer, malformed fields — all
        // surface RecordingError::Protocol, never a panic.
        let cases = [
            "not json",
            r#"{"id":1}"#,
            r#"{"id":1,"error":{"code":"unknownMethod","message":"nope"}}"#,
            r#"{"id":1,"result":{}}"#,
            r#"{"id":1,"result":{"protocolVersion":"one","macos":"15.5.0"}}"#,
            r#"{"id":1,"result":{"protocolVersion":1,"macos":"15.5.0","features":{"systemAudio":true,"microphone":false,"camera":false},"permissions":{"screenRecording":"maybe","microphone":"notDetermined","camera":"notDetermined"}}}"#,
        ];
        for line in cases {
            assert!(
                matches!(
                    parse_capabilities_result(line),
                    Err(RecordingError::Protocol(_))
                ),
                "line {line:?} must surface a Protocol error"
            );
        }
    }

    #[test]
    fn parse_sources_result_maps_the_wire_to_the_vm() {
        let vm = parse_sources_result(SOURCES_RESPONSE).expect("healthy response");
        assert_eq!(vm.displays.len(), 1);
        assert_eq!(vm.displays[0].id, 1);
        assert_eq!(vm.displays[0].width, 3456);
        assert_eq!(vm.displays[0].height, 2234);
        assert!(vm.displays[0].is_main);
        assert!(vm.applications.is_empty());
        assert!(vm.microphones.is_empty());
        assert!(vm.cameras.is_empty());
    }

    #[test]
    fn parse_sources_result_surfaces_protocol_faults() {
        let cases = [
            "not json",
            r#"{"id":2}"#,
            r#"{"id":2,"error":{"code":"boom","message":"broken"}}"#,
            r#"{"id":2,"result":{"displays":"not-a-list"}}"#,
            r#"{"id":2,"result":{"displays":[]}}"#, // missing the other lists
        ];
        for line in cases {
            assert!(
                matches!(parse_sources_result(line), Err(RecordingError::Protocol(_))),
                "line {line:?} must surface a Protocol error"
            );
        }
    }

    #[test]
    fn protocol_version_match_proceeds_mismatch_is_unsupported() {
        assert!(verify_protocol_version(PROTOCOL_VERSION).is_ok());
        let err = verify_protocol_version(PROTOCOL_VERSION + 1)
            .expect_err("a version skew must be rejected");
        match err {
            CoreError::Unsupported(message) => {
                assert!(
                    message.contains("protocol version 2"),
                    "message must name the reported version, got: {message}"
                );
            }
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    #[test]
    fn response_protocol_version_extracts_before_shape_validation() {
        // The healthy v1 response yields its version.
        assert_eq!(
            response_protocol_version(CAPABILITIES_RESPONSE).expect("v1 version"),
            1
        );
        // A future/shape-changed response still yields its version WITHOUT needing
        // the v1 `features`/`permissions` shape — this is what lets the handshake
        // report Unsupported (not Protocol) for a version bump that changed shape.
        assert_eq!(
            response_protocol_version(
                r#"{"id":1,"result":{"protocolVersion":2,"capabilities":{"x":1}}}"#
            )
            .expect("v2 version despite changed shape"),
            2
        );
        // A sidecar error answer or a missing/mistyped version is a Protocol fault
        // (the version cannot be negotiated at all).
        for line in [
            r#"{"id":1,"error":{"code":"unknownMethod","message":"nope"}}"#,
            r#"{"id":1,"result":{}}"#,
            r#"{"id":1,"result":{"protocolVersion":"two"}}"#,
            "not json",
        ] {
            assert!(
                matches!(
                    response_protocol_version(line),
                    Err(RecordingError::Protocol(_))
                ),
                "line {line:?} must surface a Protocol fault"
            );
        }
    }

    // --- FakeRecorder + drive_session --------------------------------------

    /// A scripted [`Recorder`]: replays a fixed event sequence through `on_event`,
    /// then resolves `Ok(())` (a clean stream end). Mirrors `FakeBbctlRunner`.
    struct FakeRecorder {
        available: bool,
        events: Mutex<Vec<RecordingEvent>>,
    }

    impl FakeRecorder {
        fn new(available: bool, events: Vec<RecordingEvent>) -> Self {
            Self {
                available,
                events: Mutex::new(events),
            }
        }
    }

    /// A canned, protocol-matching capabilities VM for the fake port.
    fn canned_capabilities() -> RecordingCapabilitiesVm {
        RecordingCapabilitiesVm {
            protocol_version: PROTOCOL_VERSION,
            macos_version: "15.5.0".to_owned(),
            features: crate::vm::RecordingFeaturesVm {
                system_audio: true,
                microphone: false,
                camera: false,
            },
            screen_recording: TccPermission::Granted,
            microphone: TccPermission::NotDetermined,
            camera: TccPermission::NotDetermined,
        }
    }

    /// A canned sources VM for the fake port (one display, deferred lists empty).
    fn canned_sources() -> RecordingSourcesVm {
        RecordingSourcesVm {
            displays: vec![crate::vm::RecordingDisplayVm {
                id: 1,
                width: 3456,
                height: 2234,
                is_main: true,
            }],
            applications: vec![],
            microphones: vec![],
            cameras: vec![],
        }
    }

    impl Recorder for FakeRecorder {
        fn is_available(&self) -> bool {
            self.available
        }

        async fn run_session(
            &self,
            _params: SessionParams,
            _stop: impl Future<Output = ()> + Send + 'static,
            mut on_event: Box<dyn FnMut(RecordingEvent) + Send>,
        ) -> Result<(), CoreError> {
            let events = std::mem::take(&mut *self.events.lock().expect("events lock"));
            for event in events {
                on_event(event);
            }
            Ok(())
        }

        async fn get_capabilities(&self) -> Result<RecordingCapabilitiesVm, CoreError> {
            if !self.available {
                return Err(CoreError::Unsupported(
                    "keeper-rec is not available".to_owned(),
                ));
            }
            Ok(canned_capabilities())
        }

        async fn list_sources(&self) -> Result<RecordingSourcesVm, CoreError> {
            if !self.available {
                return Err(CoreError::Unsupported(
                    "keeper-rec is not available".to_owned(),
                ));
            }
            Ok(canned_sources())
        }

        async fn request_screen_recording(&self) -> Result<bool, CoreError> {
            if !self.available {
                return Err(CoreError::Unsupported(
                    "keeper-rec is not available".to_owned(),
                ));
            }
            // Canned grant — the fake port answers the wire, it never fakes TCC.
            Ok(true)
        }
    }

    /// Canned params for the fake port (never touch the filesystem).
    fn test_params() -> SessionParams {
        SessionParams {
            output_path: "/tmp/keeper-test.mp4".to_owned(),
            display_id: None,
            system_audio: true,
            segment_mb: 500,
            max_segment_seconds: 1800,
        }
    }

    fn state_collector() -> (
        impl FnMut(SessionState) + Send,
        Arc<Mutex<Vec<SessionState>>>,
    ) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink_seen = seen.clone();
        let sink = move |state: SessionState| {
            sink_seen.lock().expect("seen lock").push(state);
        };
        (sink, seen)
    }

    #[tokio::test]
    async fn drive_session_full_lifecycle_returns_finalized() {
        let recorder = FakeRecorder::new(
            true,
            vec![
                RecordingEvent::PreflightStarted,
                RecordingEvent::CaptureStarted,
                RecordingEvent::SegmentRotating,
                RecordingEvent::CaptureStarted,
                RecordingEvent::Stopping,
                RecordingEvent::Finalized,
            ],
        );
        let (on_state, seen) = state_collector();
        let terminal = drive_session(&recorder, test_params(), std::future::pending(), on_state)
            .await
            .expect("clean lifecycle");
        assert_eq!(terminal, SessionState::Finalized);
        assert_eq!(
            *seen.lock().expect("seen lock"),
            vec![
                SessionState::Preflight,
                SessionState::Recording,
                SessionState::Rotating,
                SessionState::Recording,
                SessionState::Stopping,
                SessionState::Finalized,
            ]
        );
    }

    #[tokio::test]
    async fn drive_session_failure_branch_returns_failed() {
        let recorder = FakeRecorder::new(
            true,
            vec![
                RecordingEvent::PreflightStarted,
                RecordingEvent::CaptureStarted,
                RecordingEvent::Failed {
                    message: "capture died".to_owned(),
                },
            ],
        );
        let (on_state, _seen) = state_collector();
        let terminal = drive_session(&recorder, test_params(), std::future::pending(), on_state)
            .await
            .expect("failure branch is not a drive error");
        assert_eq!(terminal, SessionState::Failed);
    }

    #[tokio::test]
    async fn drive_session_recovered_branch_returns_recovered() {
        let recorder = FakeRecorder::new(
            true,
            vec![
                RecordingEvent::PreflightStarted,
                RecordingEvent::CaptureStarted,
                RecordingEvent::Stopping,
                RecordingEvent::Recovered,
            ],
        );
        let (on_state, _seen) = state_collector();
        let terminal = drive_session(&recorder, test_params(), std::future::pending(), on_state)
            .await
            .expect("recovered branch");
        assert_eq!(terminal, SessionState::Recovered);
    }

    #[tokio::test]
    async fn drive_session_surfaces_illegal_transition_error() {
        // An illegal first event (segmentClosed while Idle) surfaces as a
        // CoreError::Recording, not a silent state adoption.
        let recorder = FakeRecorder::new(true, vec![segment_closed(0)]);
        let (on_state, _seen) = state_collector();
        let err = drive_session(&recorder, test_params(), std::future::pending(), on_state)
            .await
            .expect_err("illegal transition must surface");
        assert!(matches!(
            err,
            CoreError::Recording(RecordingError::IllegalTransition { .. })
        ));
    }

    #[tokio::test]
    async fn fake_recorder_answers_the_new_port_methods() {
        let recorder = FakeRecorder::new(true, vec![]);
        let capabilities = recorder.get_capabilities().await.expect("canned VM");
        assert_eq!(capabilities, canned_capabilities());
        let sources = recorder.list_sources().await.expect("canned VM");
        assert_eq!(sources, canned_sources());
        assert!(recorder
            .request_screen_recording()
            .await
            .expect("canned grant outcome"));

        let unavailable = FakeRecorder::new(false, vec![]);
        assert!(matches!(
            unavailable.get_capabilities().await,
            Err(CoreError::Unsupported(_))
        ));
        assert!(matches!(
            unavailable.list_sources().await,
            Err(CoreError::Unsupported(_))
        ));
        assert!(matches!(
            unavailable.request_screen_recording().await,
            Err(CoreError::Unsupported(_))
        ));
    }

    // --- session folder, manifest.json & segment ledger (Story 17.2) --------

    /// A fresh, uniquely-named temp dir path under `std::env::temp_dir()` that
    /// does NOT yet exist (the code under test creates it). Uniqueness comes
    /// from a per-test label + a static counter — deliberately no process id
    /// (the dependency firewall bans process APIs from this file, tests
    /// included); a leftover dir from an aborted prior run is removed first.
    fn fresh_temp_dir(label: &str) -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("keeper-story-17-2-{label}-{n}"));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// The Epic 17 device set: system audio on, microphone/camera constant off.
    fn test_devices() -> SessionDevices {
        SessionDevices {
            system_audio: true,
            microphone: false,
            camera: false,
        }
    }

    /// Create a session folder + initial manifest at `folder`.
    fn test_manifest(folder: std::path::PathBuf) -> SessionManifest {
        SessionManifest::create(folder, CaptureTarget::display(None), test_devices())
            .expect("create session folder + initial manifest")
    }

    /// Shorthand for the expected reconciled entry shape (no PTS bounds — the
    /// shape a disk-only rebuild produces when the event-fed ledger had none).
    fn screen_entry(index: u32, file: &str, bytes: u64) -> SegmentEntry {
        SegmentEntry {
            index,
            file: file.to_owned(),
            bytes,
            track: "screen".to_owned(),
            pts_start: None,
            pts_end: None,
        }
    }

    /// Shorthand for an event-fed entry carrying the Story 17.4 host-clock
    /// PTS bounds.
    fn screen_entry_with_bounds(
        index: u32,
        file: &str,
        bytes: u64,
        pts_start: f64,
        pts_end: f64,
    ) -> SegmentEntry {
        SegmentEntry {
            pts_start: Some(pts_start),
            pts_end: Some(pts_end),
            ..screen_entry(index, file, bytes)
        }
    }

    #[test]
    fn manifest_status_maps_states_to_the_persisted_status() {
        for state in [
            SessionState::Idle,
            SessionState::Preflight,
            SessionState::Recording,
            SessionState::Rotating,
            SessionState::Stopping,
        ] {
            assert_eq!(ManifestStatus::from_state(state), ManifestStatus::Recording);
        }
        assert_eq!(
            ManifestStatus::from_state(SessionState::Finalized),
            ManifestStatus::Finalized
        );
        assert_eq!(
            ManifestStatus::from_state(SessionState::Recovered),
            ManifestStatus::Recovered
        );
        assert_eq!(
            ManifestStatus::from_state(SessionState::Failed),
            ManifestStatus::Failed
        );
    }

    #[test]
    fn manifest_create_writes_the_initial_recording_shape() {
        let folder = fresh_temp_dir("shape");
        let manifest = test_manifest(folder.clone());
        let raw = std::fs::read_to_string(folder.join("manifest.json")).expect("manifest on disk");
        let value: serde_json::Value = serde_json::from_str(&raw).expect("parseable manifest");
        assert_eq!(value["version"], MANIFEST_VERSION);
        assert_eq!(value["session"], manifest.session.as_str());
        assert!(
            manifest.session.starts_with("keeper-story-17-2-shape"),
            "session is the folder basename"
        );
        assert_eq!(value["status"], "recording");
        assert_eq!(value["captureTarget"]["kind"], "display");
        assert!(value["captureTarget"]["displayId"].is_null());
        assert_eq!(value["devices"]["systemAudio"], true);
        assert_eq!(value["devices"]["microphone"], false);
        assert_eq!(value["devices"]["camera"], false);
        assert_eq!(value["segments"], serde_json::json!([]));
        // Round-trip: the persisted JSON deserializes back to the same data
        // (the folder path is runtime-only and deliberately not persisted).
        let parsed: SessionManifest = serde_json::from_str(&raw).expect("round-trip");
        assert_eq!(parsed.version, manifest.version);
        assert_eq!(parsed.session, manifest.session);
        assert_eq!(parsed.status, ManifestStatus::Recording);
        assert_eq!(parsed.capture_target, manifest.capture_target);
        assert_eq!(parsed.devices, manifest.devices);
        assert_eq!(parsed.segments, manifest.segments);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn session_folder_name_derives_and_rejects_unsafe_timestamps() {
        assert_eq!(
            session_folder_name("2026-07-17 14.23.45").expect("fs-safe timestamp"),
            "keeper-rec 2026-07-17 14.23.45"
        );
        for bad in [
            "",             // empty
            "   ",          // all-whitespace
            "\t \t",        // all-whitespace (tabs)
            "2026/07/17",   // path separator
            "2026\\07\\17", // backslash separator
            "14:23:45",     // colon
            "14.23\u{0}45", // NUL
            "14.23\n45",    // control char
            ".2026-07-17",  // leading dot (hidden folder)
            "2026-07-17.",  // trailing dot (fs-normalized away)
            "2026-07-17 ",  // trailing space (fs-normalized away)
        ] {
            assert!(
                matches!(session_folder_name(bad), Err(RecordingError::ManifestIo(_))),
                "timestamp {bad:?} must be rejected"
            );
        }
    }

    #[test]
    fn segment_index_from_stem_reads_the_trailing_numeric_run() {
        assert_eq!(segment_index_from_stem("screen-0000"), Some(0));
        assert_eq!(segment_index_from_stem("screen-0042"), Some(42));
        assert_eq!(segment_index_from_stem("clip7"), Some(7));
        assert_eq!(segment_index_from_stem("notes"), None);
        assert_eq!(segment_index_from_stem(""), None);
        // A run that overflows u32 is not an index — the file is a stray.
        assert_eq!(segment_index_from_stem("screen-99999999999999999999"), None);
    }

    #[test]
    fn write_is_atomic_temp_then_rename_and_stays_parseable() {
        let folder = fresh_temp_dir("atomic");
        let mut manifest = test_manifest(folder.clone());
        manifest.record_segment(screen_entry(0, "screen-0000.mp4", 123));
        manifest.write().expect("atomic rewrite");
        assert!(
            !folder.join(".manifest.json.tmp").exists(),
            "the sibling temp file must be renamed over manifest.json"
        );
        let raw = std::fs::read_to_string(folder.join("manifest.json")).expect("manifest on disk");
        let parsed: SessionManifest = serde_json::from_str(&raw).expect("always parseable");
        assert_eq!(
            parsed.segments,
            vec![screen_entry(0, "screen-0000.mp4", 123)]
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn create_refuses_an_existing_folder() {
        let folder = fresh_temp_dir("existing");
        std::fs::create_dir_all(&folder).expect("pre-existing folder");
        let result =
            SessionManifest::create(folder.clone(), CaptureTarget::display(None), test_devices());
        assert!(
            matches!(result, Err(RecordingError::ManifestIo(_))),
            "a prior session's folder must never be adopted"
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn reconcile_rebuilds_from_disk_backfills_and_overrides_stale_bytes() {
        let folder = fresh_temp_dir("reconcile");
        let mut manifest = test_manifest(folder.clone());
        std::fs::write(folder.join("screen-0000.mp4"), vec![0u8; 10]).expect("segment 0");
        std::fs::write(folder.join("screen-0001.mp4"), vec![0u8; 20]).expect("segment 1");
        std::fs::write(folder.join("screen-0002.mp4"), vec![0u8; 30]).expect("final segment");
        // Event-fed live view: segment 0 landed with a stale zero size, segment
        // 1's `segmentClosed` was suppressed by a mid-rotation stop (DW-992),
        // and segment 2 is the final segment (never gets a `segmentClosed`).
        manifest.record_segment(screen_entry(0, "screen-0000.mp4", 0));
        manifest.reconcile_from_dir().expect("terminal reconcile");
        assert_eq!(
            manifest.segments,
            vec![
                screen_entry(0, "screen-0000.mp4", 10), // disk bytes override the stale 0
                screen_entry(1, "screen-0001.mp4", 20), // DW-992 backfill
                screen_entry(2, "screen-0002.mp4", 30), // final segment included
            ]
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn reconcile_preserves_event_fed_pts_bounds_by_index() {
        // Story 17.4 (NFR-22): disk is authoritative for index/file/bytes/track
        // — everything disk CAN observe — but the host-clock PTS bounds exist
        // only in the event-fed ledger (the muxer rebased the files to 0), so
        // the rebuild must carry them over by index and record `None` where no
        // prior entry exists (here: the final segment, which never gets a
        // `segmentClosed`).
        let folder = fresh_temp_dir("bounds");
        let mut manifest = test_manifest(folder.clone());
        std::fs::write(folder.join("screen-0000.mp4"), vec![0u8; 10]).expect("segment 0");
        std::fs::write(folder.join("screen-0001.mp4"), vec![0u8; 20]).expect("segment 1");
        std::fs::write(folder.join("screen-0002.mp4"), vec![0u8; 30]).expect("final segment");
        // Event-fed view: bounds present, but segment 0's bytes are stale.
        manifest.record_segment(screen_entry_with_bounds(
            0,
            "screen-0000.mp4",
            0,
            1000.0,
            1029.75,
        ));
        manifest.record_segment(screen_entry_with_bounds(
            1,
            "screen-0001.mp4",
            20,
            1029.8,
            1059.5,
        ));
        manifest.reconcile_from_dir().expect("terminal reconcile");
        assert_eq!(
            manifest.segments,
            vec![
                // Disk bytes win; the event-fed host-clock bounds survive.
                screen_entry_with_bounds(0, "screen-0000.mp4", 10, 1000.0, 1029.75),
                screen_entry_with_bounds(1, "screen-0001.mp4", 20, 1029.8, 1059.5),
                // No prior entry → bounds honestly null, never invented.
                screen_entry(2, "screen-0002.mp4", 30),
            ]
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn segment_entry_serializes_missing_bounds_as_null_and_reads_old_manifests() {
        // Missing bounds are persisted as explicit `null` (the spec's "recorded
        // as null" — no skip_serializing_if), and a pre-17.4 manifest without
        // the fields still deserializes (tolerant recovery paths).
        let json = serde_json::to_value(screen_entry(0, "screen-0000.mp4", 10)).expect("serialize");
        assert!(json["ptsStart"].is_null(), "absent ptsStart must be null");
        assert!(json["ptsEnd"].is_null(), "absent ptsEnd must be null");
        let json = serde_json::to_value(screen_entry_with_bounds(
            1,
            "screen-0001.mp4",
            20,
            1000.0,
            1029.75,
        ))
        .expect("serialize");
        assert_eq!(json["ptsStart"], 1000.0);
        assert_eq!(json["ptsEnd"], 1029.75);
        // Pre-17.4 wire shape (no bounds fields at all) → None, never an error.
        let old: SegmentEntry = serde_json::from_str(
            r#"{"index":3,"file":"screen-0003.mp4","bytes":7,"track":"screen"}"#,
        )
        .expect("pre-17.4 entry deserializes");
        assert_eq!(old, screen_entry(3, "screen-0003.mp4", 7));
    }

    #[test]
    fn reconcile_ingests_only_screen_segments_and_sorts_deterministically() {
        let folder = fresh_temp_dir("strays");
        let mut manifest = test_manifest(folder.clone());
        std::fs::write(folder.join("screen-0002.mp4"), vec![0u8; 5]).expect("segment 2");
        std::fs::write(folder.join("screen-0000.mp4"), vec![0u8; 4]).expect("segment 0");
        // Strays that must NOT enter the authoritative screen-track ledger:
        std::fs::write(folder.join("extra-0001.mp4"), vec![0u8; 6]).expect("wrong prefix");
        std::fs::write(folder.join("camera-0000.mp4"), vec![0u8; 6]).expect("future track prefix");
        std::fs::write(folder.join("notes.mp4"), vec![0u8; 7]).expect("no numeric run");
        std::fs::write(folder.join("cover.png"), vec![0u8; 8]).expect("non-mp4");
        std::fs::create_dir(folder.join("screen-0009.mp4")).expect("dir masquerading as segment");
        manifest
            .reconcile_from_dir()
            .expect("strays are skipped, never aborting");
        let names: Vec<&str> = manifest.segments.iter().map(|s| s.file.as_str()).collect();
        assert_eq!(
            names,
            vec!["screen-0000.mp4", "screen-0002.mp4"],
            "only screen-####.mp4 segments enter the ledger, sorted by (index, file); \
             wrong-prefix / no-run / non-mp4 / directory entries are skipped"
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    // --- live session bytes (Story 18.1) ------------------------------------

    #[test]
    fn session_bytes_sums_this_sessions_segments_only() {
        let folder = fresh_temp_dir("bytes");
        std::fs::create_dir_all(&folder).expect("session folder");
        std::fs::write(folder.join("screen-0000.mp4"), vec![0u8; 10]).expect("segment 0");
        std::fs::write(folder.join("screen-0001.mp4"), vec![0u8; 20]).expect("segment 1");
        // Foreign files that must NOT count (same ownership rule as reconcile):
        std::fs::write(folder.join("manifest.json"), b"{}").expect("manifest");
        std::fs::write(folder.join("camera-0000.mp4"), vec![0u8; 40]).expect("future track prefix");
        std::fs::write(folder.join("notes.mp4"), vec![0u8; 50]).expect("no numeric run");
        std::fs::create_dir(folder.join("screen-0009.mp4")).expect("dir masquerading as segment");
        assert_eq!(session_bytes_on_disk(&folder), 30);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn session_bytes_is_zero_for_a_missing_or_empty_folder() {
        // `fresh_temp_dir` returns a path that does NOT exist yet.
        let folder = fresh_temp_dir("bytes-missing");
        assert_eq!(session_bytes_on_disk(&folder), 0, "missing folder is 0");
        std::fs::create_dir_all(&folder).expect("empty session folder");
        assert_eq!(session_bytes_on_disk(&folder), 0, "empty folder is 0");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn session_bytes_reports_a_growing_segments_current_length() {
        // A mid-write current segment reports whatever has reached disk so far
        // — the tray figure grows live, never waiting for `segmentClosed`.
        let folder = fresh_temp_dir("bytes-growing");
        std::fs::create_dir_all(&folder).expect("session folder");
        std::fs::write(folder.join("screen-0000.mp4"), vec![0u8; 10]).expect("first flush");
        assert_eq!(session_bytes_on_disk(&folder), 10);
        std::fs::write(folder.join("screen-0000.mp4"), vec![0u8; 25]).expect("more bytes flushed");
        assert_eq!(session_bytes_on_disk(&folder), 25);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[cfg(unix)]
    #[test]
    fn reconcile_skips_an_unreadable_entry_without_aborting() {
        let folder = fresh_temp_dir("unreadable");
        let mut manifest = test_manifest(folder.clone());
        std::fs::write(folder.join("screen-0000.mp4"), vec![0u8; 9]).expect("healthy segment");
        // A dangling symlink: `fs::metadata` (which follows links) fails on it,
        // so the entry must be skipped — one bad entry never fails the
        // terminal write.
        std::os::unix::fs::symlink(folder.join("gone.mp4"), folder.join("screen-0001.mp4"))
            .expect("dangling symlink");
        manifest
            .reconcile_from_dir()
            .expect("one unreadable entry must not abort the reconcile");
        assert_eq!(
            manifest.segments,
            vec![screen_entry(0, "screen-0000.mp4", 9)]
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn reconcile_and_write_complete_the_manifest_on_every_terminal() {
        // The exact manifest API the shell's event sink drives at a terminal —
        // for ALL THREE terminals, not just Finalized: reconcile from disk, map
        // the state, atomic write.
        for (state, wire) in [
            (SessionState::Finalized, "finalized"),
            (SessionState::Recovered, "recovered"),
            (SessionState::Failed, "failed"),
        ] {
            let folder = fresh_temp_dir(&format!("terminal-{wire}"));
            let mut manifest = test_manifest(folder.clone());
            std::fs::write(folder.join("screen-0000.mp4"), vec![0u8; 11]).expect("segment 0");
            std::fs::write(folder.join("screen-0001.mp4"), vec![0u8; 22]).expect("final segment");
            manifest.reconcile_from_dir().expect("terminal reconcile");
            manifest.set_status(ManifestStatus::from_state(state));
            manifest.write().expect("terminal write");
            let raw =
                std::fs::read_to_string(folder.join("manifest.json")).expect("manifest on disk");
            let value: serde_json::Value = serde_json::from_str(&raw).expect("parseable manifest");
            assert_eq!(value["status"], wire, "terminal {wire} persists its status");
            let segments = value["segments"].as_array().expect("segments array");
            assert_eq!(segments.len(), 2, "terminal {wire} lists every segment");
            assert_eq!(segments[0]["file"], "screen-0000.mp4");
            assert_eq!(segments[0]["bytes"], 11);
            assert_eq!(segments[1]["file"], "screen-0001.mp4");
            assert_eq!(segments[1]["bytes"], 22);
            let _ = std::fs::remove_dir_all(&folder);
        }
    }

    // --- dependency firewall (AD-33) ---------------------------------------

    /// The recording core must carry NO Tauri-shell, Apple-framework, or
    /// Objective-C-runtime token — the platform seam lives only in
    /// `keeper/src/recorder.rs` under `#[cfg(desktop)]`. Build the forbidden tokens by
    /// concatenation so they do NOT appear as contiguous substrings in this very file
    /// (the scan reads this file), mirroring
    /// `signals::tests::presence_is_withheld_everywhere`.
    #[test]
    fn dependency_firewall_holds() {
        let shell_crate = format!("ta{}ri", "u");
        let apple_runtime = format!("ob{}c", "j");
        let apple_bindings = format!("ob{}2", "jc");
        let screencapturekit = format!("Screen{}Kit", "Capture");
        let avfoundation = format!("AV{}", "Foundation");
        let core_graphics = format!("Core{}", "Graphics");
        // Also ban the two ways a process spawn could leak in — this is the core
        // AD-33 invariant ("never holds a process handle", no sidecar spawn), which
        // the shell/Apple-framework tokens above do NOT cover. The platform port
        // (`keeper/src/recorder.rs`) owns spawning; this module must never import a
        // process API. (Built by concatenation so they never self-match; the plain
        // English word for a running program in the doc comments is not banned.)
        let tokio_process = format!("tokio::pro{}", "cess");
        let std_process = format!("std::pro{}", "cess");
        let forbidden = [
            shell_crate.as_str(),
            apple_runtime.as_str(),
            apple_bindings.as_str(),
            screencapturekit.as_str(),
            avfoundation.as_str(),
            core_graphics.as_str(),
            tokio_process.as_str(),
            std_process.as_str(),
        ];

        // Anchor on the crate manifest dir (like `signals::tests`), NOT the process
        // CWD — a workspace-root CWD (some nextest/IDE runners) would otherwise make
        // this guard panic spuriously or scan the wrong file and pass vacuously.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/recording.rs");
        let source = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "read {} for the dependency firewall scan: {e}",
                path.display()
            )
        });
        for token in forbidden {
            assert!(
                !source.contains(token),
                "recording.rs must stay platform-free, but it contains the forbidden token {token:?}"
            );
        }
    }
}
