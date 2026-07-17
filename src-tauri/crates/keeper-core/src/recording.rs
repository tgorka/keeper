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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordingEvent {
    /// The sidecar started its pre-flight (`Idle → Preflight`).
    PreflightStarted,
    /// Capture started (`Preflight → Recording`, or `Rotating → Recording`).
    CaptureStarted,
    /// A segment rotation started (`Recording → Rotating`).
    SegmentRotating,
    /// A segment finished closing. Legal only while `Recording`/`Rotating`; it bumps
    /// the session's segment counter and does NOT change state.
    SegmentClosed {
        /// The zero-based index of the segment that just closed.
        index: u32,
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
            Some(RecordingEvent::SegmentClosed { index })
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

    /// Run one recording session, forwarding each recognized [`RecordingEvent`] to
    /// `on_event` **as it arrives**. Resolves `Ok(())` when the sidecar's event
    /// stream ends cleanly; a spawn/IO failure resolves
    /// [`CoreError::Recording`], and an unavailable sidecar resolves
    /// [`CoreError::Unsupported`]. Never panics on absent / garbage output.
    fn run_session(
        &self,
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

    let run_result = recorder.run_session(sink).await;

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
    use std::sync::{Arc, Mutex};

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
            .apply(RecordingEvent::SegmentClosed { index: 0 })
            .expect("segmentClosed legal while Recording");
        assert_eq!(session.state(), SessionState::Recording);
        assert_eq!(session.segments_closed(), 1);
        // Legal while Rotating too.
        session
            .apply(RecordingEvent::SegmentRotating)
            .expect("legal transition");
        session
            .apply(RecordingEvent::SegmentClosed { index: 1 })
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
            .apply(RecordingEvent::SegmentClosed { index: 0 })
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
            Some(RecordingEvent::SegmentClosed { index: 3 })
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
                RecordingEvent::SegmentClosed { index: 0 },
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
        let terminal = drive_session(&recorder, on_state)
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
        let terminal = drive_session(&recorder, on_state)
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
        let terminal = drive_session(&recorder, on_state)
            .await
            .expect("recovered branch");
        assert_eq!(terminal, SessionState::Recovered);
    }

    #[tokio::test]
    async fn drive_session_surfaces_illegal_transition_error() {
        // An illegal first event (segmentClosed while Idle) surfaces as a
        // CoreError::Recording, not a silent state adoption.
        let recorder = FakeRecorder::new(true, vec![RecordingEvent::SegmentClosed { index: 0 }]);
        let (on_state, _seen) = state_collector();
        let err = drive_session(&recorder, on_state)
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
