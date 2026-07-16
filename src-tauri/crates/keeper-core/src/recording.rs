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
