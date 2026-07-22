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

use crate::error::{format_gb, CoreError, DestinationRejection, RecordingError};
use crate::vm::{
    RecordingCapabilitiesVm, RecordingPermissionVm, RecordingSourcesVm, ScreenRecordingAccess,
    TccPermission,
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
    /// A non-fatal, sticky session warning (Story 19.4) — e.g. a microphone
    /// unplugged mid-recording (`code: "micLost"`). Legal only while the
    /// session is live (`Recording`/`Rotating`/`Stopping`); like
    /// [`RecordingEvent::SegmentClosed`] it updates a session field (the
    /// sticky warning message) and NEVER changes [`SessionState`] — a
    /// mic-only fault must not take the terminal `Failed` path.
    Warning {
        /// A stable, machine-readable warning code (e.g. `"micLost"`).
        code: String,
        /// A non-secret, human-readable description of the warning (never a
        /// path, token, or media bytes).
        message: String,
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
    /// The sticky, non-fatal session warning (Story 19.4): set by
    /// [`RecordingEvent::Warning`] (last-write-wins message) and never cleared
    /// for the session's lifetime — a fresh session starts clean.
    warning: Option<String>,
}

impl RecordingSession {
    /// A fresh session in [`SessionState::Idle`] with no segments closed and
    /// no warning raised.
    pub fn new() -> Self {
        Self {
            state: SessionState::Idle,
            segments_closed: 0,
            warning: None,
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

    /// The sticky, non-fatal session warning (Story 19.4), or `None` when the
    /// session never warned. Last-write-wins; never cleared for the session's
    /// lifetime (a fresh session starts clean via [`RecordingSession::new`]).
    pub fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
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
    /// (bumps the counter, no state change), and [`RecordingEvent::Warning`]
    /// only in `Recording`/`Rotating`/`Stopping` (sets the sticky warning, no
    /// state change — Story 19.4). Anything else →
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

        // A `Warning` is non-fatal and sticky (Story 19.4): legal only while
        // the session is live (`Recording`/`Rotating`/`Stopping`), it records
        // the message (last-write-wins) and NEVER changes state — the
        // `SegmentClosed` precedent. In a terminal (or not-yet-live) state it
        // is rejected like any other misplaced event; the shell's sink drops
        // the rejection best-effort, so a late warning never resurrects a
        // settled session.
        if let E::Warning { message, .. } = &event {
            return match self.state {
                S::Recording | S::Rotating | S::Stopping => {
                    self.warning = Some(message.clone());
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
        RecordingEvent::Warning { .. } => "warning",
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
/// - `"event":"warning"` + `"code"`/`"message"` — [`RecordingEvent::Warning`]
///   (Story 19.4; both fields best-effort, defaulted when missing/blank).
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
        "warning" => {
            // Story 19.4: the non-fatal warning line. Best-effort like
            // `segmentClosed`: a missing / mistyped / blank `code` or
            // `message` degrades to a stable, non-secret default and any
            // unknown extra key is ignored — the warning itself is never
            // dropped (a lost mic-loss signal would leave the user unwarned
            // while the mic track runs silent).
            let code = obj
                .get("code")
                .and_then(serde_json::Value::as_str)
                .filter(|code| !code.trim().is_empty())
                .unwrap_or("unknown")
                .to_owned();
            let message = obj
                .get("message")
                .and_then(serde_json::Value::as_str)
                .filter(|message| !message.trim().is_empty())
                .unwrap_or("keeper-rec reported a recording warning")
                .to_owned();
            Some(RecordingEvent::Warning { code, message })
        }
        "error" => {
            // A malformed / absent `message` must NOT swallow the failure — surface a
            // `Failed` with a generic, non-secret fallback so the machine still reaches
            // its `Failed` terminal (a lost error would strand the session as if capture
            // were still live).
            let message = obj
                .get("message")
                .and_then(serde_json::Value::as_str)
                // A present-but-blank message is as useless as an absent one:
                // filter it so the loud-failure triad (tray/notification/banner,
                // Story 18.4) always names a reason, never "Recording failed — ".
                // Mirrors the `warning` arm's blank-filter above.
                .filter(|message| !message.trim().is_empty())
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
///
/// Story 19.5 added the additive, always-present `fps` field to
/// `startRecording` (like `segmentMB`), decoded best-effort by the sidecar with
/// a default of 30 when absent — per the same additive precedent the version
/// stays 1.
///
/// Story 20.1 added the additive `cameraEnabled`/`cameraDeviceId`
/// `startRecording` fields (absent = camera off, preserving the pre-20.1
/// wire), the `requestCamera` method, and `segmentClosed{track:"camera"}`
/// lines (the `track` field existed since 17.1) — per the same additive
/// precedent the version stays 1.
pub const PROTOCOL_VERSION: u32 = 1;

/// The shared disk-guard hard floor in bytes (Story 19.5; 2 GiB): the minimum
/// free space the destination volume must have for a Recording Session to
/// start. Story 18.5's *live* during-recording guard consumes the same constant
/// — a probe below this floor while recording triggers the graceful
/// stop-and-finalize leg of [`plan_disk_guard_action`]; the pre-Start leg stays
/// [`evaluate_destination`]. An authored default — product-owner sign-off at
/// phase release (PRD §14.7), changeable in this one edit; never a settings row.
pub const RECORDING_MIN_FREE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// The live disk-guard warn threshold in bytes (Story 18.5; 10 GiB, same binary
/// idiom as the 2 GiB [`RECORDING_MIN_FREE_BYTES`] floor): free space below this
/// while recording raises the persistent low-disk warning (tray ⚠ line, banner
/// amber, one native notification) without interrupting capture. An authored
/// default — product-owner sign-off at phase release (PRD §14.7), changeable in
/// this one edit; never a settings row.
pub const RECORDING_WARN_FREE_BYTES: u64 = 10 * 1024 * 1024 * 1024;

/// Decide whether the destination folder can host a Recording Session (Story
/// 19.5, AD-33) from already-probed facts — pure and platform-free, so the
/// low-free-space path unit-tests by injecting a small `free_bytes`, never by
/// filling a real disk. The SHELL gathers the facts (`create_dir_all` →
/// `creatable_or_exists`, a probe-file write+remove → `writable`, an
/// `available_space` probe → `free_bytes`) and calls this BEFORE creating any
/// session folder or spawning the sidecar; a rejection means no capture begins.
///
/// Rejections are ordered by actionability: a folder that cannot exist at all
/// ([`DestinationRejection::NotADirectory`]) trumps writability, which trumps
/// free space — the user fixes the most fundamental problem first.
pub fn evaluate_destination(
    creatable_or_exists: bool,
    writable: bool,
    free_bytes: u64,
    min_free_bytes: u64,
) -> Result<(), DestinationRejection> {
    if !creatable_or_exists {
        return Err(DestinationRejection::NotADirectory);
    }
    if !writable {
        return Err(DestinationRejection::NotWritable);
    }
    if free_bytes < min_free_bytes {
        return Err(DestinationRejection::InsufficientSpace {
            free_bytes,
            required_bytes: min_free_bytes,
        });
    }
    Ok(())
}

// --- Live disk-space guard policy (Story 18.5, AD-33/AD-39) --------------------
//
// The during-recording twin of `evaluate_destination`: pure, platform-free
// policy over an already-probed free-space figure. The SHELL measures (the same
// `fs4::available_space` probe the pre-start gate uses, on a ~1 Hz tick while a
// session is live) and executes the returned action; core alone owns the
// thresholds, the warn/floor decision, the one-shot latching, and the
// user-facing copy — so the whole guard unit-tests by injecting a simulated
// `free_bytes`, never by filling a real disk.

/// The instantaneous band a probed free-space figure falls into (Story 18.5):
/// the stateless half of the live disk guard. `free < floor` ⇒ [`Stop`],
/// `floor ≤ free < warn` ⇒ [`Warn`], else [`Ok`] — boundaries deliberately
/// mirror [`evaluate_destination`]'s `<` guard (exactly the floor still warns
/// rather than stops; exactly the warn threshold is still Ok).
///
/// [`Stop`]: DiskGuardDecision::Stop
/// [`Warn`]: DiskGuardDecision::Warn
/// [`Ok`]: DiskGuardDecision::Ok
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskGuardDecision {
    /// Free space is comfortably above the warn threshold — no action.
    Ok,
    /// Free space is below the warn threshold but at or above the hard floor —
    /// warn and keep recording.
    Warn,
    /// Free space is below the hard floor — gracefully stop-and-finalize.
    Stop,
}

/// Classify a probed free-space figure against the guard thresholds (Story
/// 18.5). Pure and stateless; the once-per-event latching lives one layer up in
/// [`plan_disk_guard_action`]. A failed probe must be reported by the shell as
/// `u64::MAX` ("plenty", fail-open) — never as 0, which would read as a full
/// volume and force a spurious stop.
pub fn evaluate_disk_guard(
    free_bytes: u64,
    warn_bytes: u64,
    floor_bytes: u64,
) -> DiskGuardDecision {
    if free_bytes < floor_bytes {
        DiskGuardDecision::Stop
    } else if free_bytes < warn_bytes {
        DiskGuardDecision::Warn
    } else {
        DiskGuardDecision::Ok
    }
}

/// The per-session one-shot memory of the live disk guard (Story 18.5): which
/// of the two distinct events already fired. Fresh (`Default`) at every session
/// start; owned by the shell's guard task alongside its probe loop.
///
/// Deliberately never resets when space recovers: the sticky warning stays (the
/// 19.4 model — ending/acknowledging the session clears it) and the stop is
/// never re-issued.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiskGuardLatch {
    /// The warn-onset event already fired (warning raised + one notification).
    pub warned: bool,
    /// The hard-floor stop already fired (graceful stop requested + one
    /// notification). Once set, every later tick plans [`DiskGuardAction::None`].
    pub stopped: bool,
}

/// What the shell must execute for one guard tick (Story 18.5): nothing, raise
/// the persistent low-disk warning once, or gracefully stop-and-finalize once.
/// Carries the exact user-facing copy so the shell never words anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiskGuardAction {
    /// Nothing to do this tick (healthy band, repeat of an already-latched
    /// event, or post-stop).
    None,
    /// Raise the sticky low-disk warning (tray ⚠ line + banner amber) and post
    /// one native notification. Recording continues.
    Warn {
        /// The user-facing warning copy, naming the probed free space.
        message: String,
    },
    /// Request the graceful stop (the same idempotent trigger as the user's
    /// Stop — `Stopping` → `Finalized`, never a `Failed` fault), set the sticky
    /// reason, and post one native notification.
    Stop {
        /// The user-facing stop-reason copy.
        message: String,
    },
}

/// Plan the shell's action for one guard tick from a probed (or simulated)
/// free-space figure (Story 18.5): the latched layer over
/// [`evaluate_disk_guard`]. Each distinct event is returned **at most once per
/// session** — the warn onset once, the hard-floor stop once *even if a warn
/// already fired* (two distinct events; a user who ignored the warn must still
/// be pinged by the stop) — and a session that plunges straight past both
/// thresholds in one tick plans only the Stop. After the stop fired, every
/// tick plans [`DiskGuardAction::None`], whatever the band: the stop is never
/// re-issued and a recovered volume never resurrects the guard mid-session.
pub fn plan_disk_guard_action(
    free_bytes: u64,
    warn_bytes: u64,
    floor_bytes: u64,
    latch: &mut DiskGuardLatch,
) -> DiskGuardAction {
    if latch.stopped {
        return DiskGuardAction::None;
    }
    match evaluate_disk_guard(free_bytes, warn_bytes, floor_bytes) {
        DiskGuardDecision::Stop => {
            latch.stopped = true;
            DiskGuardAction::Stop {
                message: "Recording stopped — low disk".to_owned(),
            }
        }
        DiskGuardDecision::Warn if !latch.warned => {
            latch.warned = true;
            DiskGuardAction::Warn {
                message: format!("Low disk space — {} free", format_gb(free_bytes)),
            }
        }
        DiskGuardDecision::Warn | DiskGuardDecision::Ok => DiskGuardAction::None,
    }
}

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

/// Build the one-line `requestMicrophone` request (Story 19.3, FR-69, AD-36; no
/// trailing newline — the shell port owns line framing). Wire shape:
/// `{"id":<id>,"method":"requestMicrophone"}`. Only ever sent lazily — when the
/// user enables the mic source, never preemptively. Additive to the v1 protocol
/// — keeper and keeper-rec ship in lockstep, so [`PROTOCOL_VERSION`] is
/// unchanged (the 16.5 `requestScreenRecording` precedent).
pub fn request_microphone_request(id: u64) -> String {
    serde_json::json!({ "id": id, "method": "requestMicrophone" }).to_string()
}

/// Build the one-line `requestCamera` request (Story 20.1, FR-70, AD-36; no
/// trailing newline — the shell port owns line framing). Wire shape:
/// `{"id":<id>,"method":"requestCamera"}`. Only ever sent lazily — when the
/// user enables the Webcam switch, never preemptively (the 19.3 mic
/// precedent verbatim). Additive to the v1 protocol — keeper and keeper-rec
/// ship in lockstep, so [`PROTOCOL_VERSION`] is unchanged.
pub fn request_camera_request(id: u64) -> String {
    serde_json::json!({ "id": id, "method": "requestCamera" }).to_string()
}

/// A single-application capture target (Story 19.1) — the running process id and
/// bundle identifier the sidecar re-resolves live against `SCShareableContent` at
/// Start. Pure data; no platform token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationTarget {
    /// The application's running process id.
    pub pid: i32,
    /// The application's bundle identifier.
    pub bundle_id: String,
}

/// The microphone selection of one capture session (Story 19.3, FR-69, AD-36)
/// — present on [`SessionParams`] only when the mic source is enabled. Pure
/// data; no platform token (the device id is an opaque string the sidecar
/// resolves against its own enumeration).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicSelection {
    /// The selected input device's unique id, or `None` for the system default
    /// input.
    pub device_id: Option<String>,
}

/// The camera selection of one capture session (Story 20.1, FR-70, AD-37) —
/// present on [`SessionParams`] only when the webcam source is enabled. Pure
/// data; no platform token (the device id is an opaque string the sidecar
/// resolves against its own enumeration, falling back to the system default
/// camera when the id vanished). The sidecar then records the camera as its
/// own separate `camera-####.mov` per segment — never a track inside
/// `screen-####`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CameraSelection {
    /// The selected camera's unique id, or `None` for the system default
    /// camera.
    pub device_id: Option<String>,
}

/// The parameters of one capture session (Story 16.6, FR-68/FR-69/FR-71, AD-37).
///
/// The host owns the output path (directory + local-time-stamped filename); the
/// sidecar creates parent directories as needed and writes exactly this file.
/// The video target is additive (Story 19.1): `application` (`Some`) scopes
/// capture to one app's windows and **wins** over `display_id`; otherwise
/// `display_id` picks a specific display (`None` = the main display), the
/// unchanged 16.6 path. `system_audio` toggles the AAC system-audio track (with
/// keeper's own process audio excluded — FR-69).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionParams {
    /// Absolute path of the `.mov` file to write.
    pub output_path: String,
    /// The macOS display id to capture, or `None` for the main display. Ignored
    /// when `application` is `Some` (an application target wins).
    pub display_id: Option<u32>,
    /// The single-application capture target (Story 19.1), or `None` for a
    /// display target. When `Some`, the wire carries `applicationPid`+`bundleId`
    /// and omits `displayId`.
    pub application: Option<ApplicationTarget>,
    /// Whether to capture system audio (true in the walking skeleton).
    pub system_audio: bool,
    /// The microphone selection (Story 19.3, FR-69), or `None` when the mic
    /// source is off (the default). When `Some`, the wire carries
    /// `micEnabled:true` (+ `micDeviceId` when a specific device is picked)
    /// and the sidecar writes the mic as its own, unmixed AAC track.
    pub microphone: Option<MicSelection>,
    /// The camera selection (Story 20.1, FR-70), or `None` when the webcam
    /// source is off (the default). When `Some`, the wire carries
    /// `cameraEnabled:true` (+ `cameraDeviceId` when a specific device is
    /// picked) and the sidecar writes a separate `camera-####.mov` per
    /// segment beside `screen-####`.
    pub camera: Option<CameraSelection>,
    /// Segment size in decimal MB before a gapless rotation (Story 17.5,
    /// FR-72); the sidecar's `segmentMB`.
    pub segment_mb: u32,
    /// Duration-cap rotation fallback in whole seconds (Story 17.5, FR-72);
    /// the sidecar's `maxSegmentSeconds`.
    pub max_segment_seconds: u32,
    /// Capture frame rate (Story 19.5): 30 (default) or 60, already normalized
    /// by the registry read; the sidecar's `fps` (normalized again defensively
    /// Swift-side, so a bad value can never reach `SCStreamConfiguration`).
    pub fps: u32,
    /// Video codec (Story 21.1): `"h264"` or `"hevc"`, already normalized by
    /// the registry read; the sidecar's additive `codec` param (absent ⇒
    /// h264 — older wire preserved; normalized again Swift-side).
    pub codec: String,
    /// Capture scale percent (Story 21.2): 100/75/50, already normalized; the
    /// sidecar's additive `scalePercent` param (absent ⇒ 100).
    pub scale_percent: u32,
    /// Audio-only session (Story 21.3): no SCStream video output, no video
    /// track — `audio-####.m4a` segments. The sidecar's additive `audioOnly`
    /// param (absent ⇒ false, the classic video path).
    pub audio_only: bool,
}

/// Build the one-line `startRecording` request (Story 16.6 + 17.5 + 19.5; no
/// trailing newline — the shell port owns line framing). Wire shape:
/// `{"id":<id>,"method":"startRecording","params":{"path":…,"systemAudio":…,
/// "segmentMB":…,"maxSegmentSeconds":…,"fps":…[,"displayId":…]}}`. `segmentMB`
/// / `maxSegmentSeconds` (17.5) and the always-present `fps` (19.5) are
/// additive fields the sidecar reads best-effort (defaulting when absent), so —
/// per the additive precedent — keeper and keeper-rec ship in lockstep and
/// [`PROTOCOL_VERSION`] is unchanged. The destination stays fully host-side:
/// there is no `dir` wire field; the sidecar keeps deriving its directory from
/// the one absolute `path`.
pub fn start_recording_request(id: u64, params: &SessionParams) -> String {
    let mut wire = serde_json::json!({
        "path": params.output_path,
        "systemAudio": params.system_audio,
        "segmentMB": params.segment_mb,
        "maxSegmentSeconds": params.max_segment_seconds,
        "fps": params.fps,
        // Additive (Story 21.1/21.2): absent ⇒ h264 / 100 on older sidecars.
        "codec": params.codec,
        "scalePercent": params.scale_percent,
        // Additive (Story 21.3): absent ⇒ the classic video session.
        "audioOnly": params.audio_only,
    });
    // An application target wins (Story 19.1): emit `applicationPid`+`bundleId`
    // and omit `displayId` entirely, so the sidecar builds an app-scoped filter.
    // Otherwise emit `displayId` only when a specific display was picked — the
    // 16.6 display path stays byte-for-byte unchanged.
    if let Some(application) = &params.application {
        wire["applicationPid"] = application.pid.into();
        wire["bundleId"] = application.bundle_id.clone().into();
    } else if let Some(display_id) = params.display_id {
        wire["displayId"] = display_id.into();
    }
    // The mic source (Story 19.3): additive fields, absent entirely while the
    // mic is off so the 16.6/19.2 wire stays byte-for-byte unchanged.
    // `micDeviceId` is emitted only for a specific device — its absence with
    // `micEnabled:true` means the system default input.
    if let Some(microphone) = &params.microphone {
        wire["micEnabled"] = true.into();
        if let Some(device_id) = &microphone.device_id {
            wire["micDeviceId"] = device_id.clone().into();
        }
    }
    // The webcam source (Story 20.1): additive fields, absent entirely while
    // the camera is off so the pre-20.1 wire stays byte-for-byte unchanged.
    // `cameraDeviceId` is emitted only for a specific device — its absence
    // with `cameraEnabled:true` means the system default camera.
    if let Some(camera) = &params.camera {
        wire["cameraEnabled"] = true.into();
        if let Some(device_id) = &camera.device_id {
            wire["cameraDeviceId"] = device_id.clone().into();
        }
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

/// Parse the id-correlated `requestMicrophone` response line (Story 19.3,
/// FR-69, AD-36) into the sidecar-reported post-request TCC state
/// (`result.status`). Unlike Screen Recording's two-valued preflight, the
/// sidecar's audio-permission probe reports the authoritative granted /
/// denied / notDetermined tri-state directly, so no session-flag lifting is
/// needed here. A sidecar `error` answer and any malformed / missing field
/// surface as [`RecordingError::Protocol`] — never a panic.
pub fn parse_request_microphone_result(line: &str) -> Result<TccPermission, RecordingError> {
    let result = response_result(line, "requestMicrophone")?;
    let raw = result
        .as_object()
        .and_then(|obj| obj.get("status"))
        .cloned()
        .ok_or_else(|| protocol_error("requestMicrophone: missing status"))?;
    serde_json::from_value(raw)
        .map_err(|e| protocol_error(format!("requestMicrophone: unrecognized status: {e}")))
}

/// Parse the id-correlated `requestCamera` response line (Story 20.1, FR-70,
/// AD-36) into the sidecar-reported post-request TCC state (`result.status`)
/// — the `parse_request_microphone_result` twin for the `.video` media type.
/// A sidecar `error` answer and any malformed / missing field surface as
/// [`RecordingError::Protocol`] — never a panic.
pub fn parse_request_camera_result(line: &str) -> Result<TccPermission, RecordingError> {
    let result = response_result(line, "requestCamera")?;
    let raw = result
        .as_object()
        .and_then(|obj| obj.get("status"))
        .cloned()
        .ok_or_else(|| protocol_error("requestCamera: missing status"))?;
    serde_json::from_value(raw)
        .map_err(|e| protocol_error(format!("requestCamera: unrecognized status: {e}")))
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

/// Map a live mic/camera TCC state onto the shared pre-flight tri-state
/// (Story 20.2, FR-67, AD-36) — pure and total, unit-tested without a Mac.
///
/// Unlike screen (whose non-prompting preflight is two-valued and needs the
/// session "already requested" flag to tell a denial from a never-asked state),
/// `AVCaptureDevice.authorizationStatus` is a true tri-state, so the mapping is
/// direct and needs no persisted flag: `Granted → Granted`, `Denied → Denied`,
/// `NotDetermined → NotYetRequested` (the OS prompt is still available).
pub fn resolve_source_access(tcc: TccPermission) -> ScreenRecordingAccess {
    match tcc {
        TccPermission::Granted => ScreenRecordingAccess::Granted,
        TccPermission::Denied => ScreenRecordingAccess::Denied,
        TccPermission::NotDetermined => ScreenRecordingAccess::NotYetRequested,
    }
}

/// Resolve the full pre-flight [`RecordingPermissionVm`] over the three
/// permission legs (Story 20.2, FR-67, AD-36) — the single, pure Start gate.
///
/// `microphone`/`camera` are `Some` iff that source is enabled (a disabled
/// leg is `None`, renders no row, and never gates Start). `can_start` is
/// `true` only when Screen Recording is `Granted` **and** every enabled leg
/// is `Granted` — an enabled source whose permission is not granted is a
/// blocking permission.
pub fn resolve_recording_permission(
    screen_recording: ScreenRecordingAccess,
    microphone: Option<ScreenRecordingAccess>,
    camera: Option<ScreenRecordingAccess>,
) -> RecordingPermissionVm {
    fn leg_green(leg: Option<ScreenRecordingAccess>) -> bool {
        leg.is_none_or(|access| access == ScreenRecordingAccess::Granted)
    }
    RecordingPermissionVm {
        screen_recording,
        microphone,
        camera,
        can_start: screen_recording == ScreenRecordingAccess::Granted
            && leg_green(microphone)
            && leg_green(camera),
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
// The per-session folder `keeper-rec <local ts>/` holds the `screen-####.mov`
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

/// Screen-track segment files are named `screen-####.mov`. The terminal
/// reconcile ingests only the session's own stem prefixes (this one and
/// [`CAMERA_SEGMENT_STEM_PREFIX`], Story 20.1), so a stray `*.mov` (a user
/// drop) with a trailing digit run never pollutes the authoritative ledger.
const SEGMENT_STEM_PREFIX: &str = "screen-";

/// Camera-track segment files are named `camera-####.mov` (Story 20.1,
/// FR-70/FR-73): the optional webcam's own separate per-segment file, sharing
/// the session folder and the segment index space with `screen-####` —
/// disambiguated in the ledger by `track`, never by index alone.
const CAMERA_SEGMENT_STEM_PREFIX: &str = "camera-";

/// Audio-only-track segment files are named `audio-####.m4a` (Story 21.3).
const AUDIO_SEGMENT_STEM_PREFIX: &str = "audio-";

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
    /// The segment file's basename, e.g. `"screen-0003.mov"`.
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

/// What the session captures (Story 17.2 → 19.1). `kind` is `"display"` or
/// `"application"`; the other fields are populated per kind (`display_id` for a
/// display, `bundle_id`+`pid` for an application) and serialized only when
/// present, so a display manifest stays byte-compatible with pre-19.1 readers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureTarget {
    /// The target kind — `"display"` or `"application"` (Story 19.1).
    pub kind: String,
    /// The captured display id, or `None` for the main display / an application
    /// target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_id: Option<u32>,
    /// The captured application's bundle identifier (Story 19.1), or `None` for
    /// a display target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle_id: Option<String>,
    /// The captured application's process id (Story 19.1), or `None` for a
    /// display target. Informational — the manifest records what was targeted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<i32>,
}

impl CaptureTarget {
    /// A display capture target (`None` = the main display).
    pub fn display(display_id: Option<u32>) -> Self {
        Self {
            kind: "display".to_owned(),
            display_id,
            bundle_id: None,
            pid: None,
        }
    }

    /// A single-application capture target (Story 19.1) — the exclusionary
    /// app-scoped capture the manifest records.
    pub fn application(bundle_id: String, pid: i32) -> Self {
        Self {
            kind: "application".to_owned(),
            display_id: None,
            bundle_id: Some(bundle_id),
            pid: Some(pid),
        }
    }

    /// An audio-only capture target (Story 21.3): no video track at all —
    /// system audio and/or the microphone into `audio-####.m4a` segments.
    pub fn audio_only() -> Self {
        Self {
            kind: "audioOnly".to_owned(),
            display_id: None,
            bundle_id: None,
            pid: None,
        }
    }
}

/// Which device tracks the session records (Story 17.2 → 20.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDevices {
    /// Whether the system-audio AAC track is recorded.
    pub system_audio: bool,
    /// Whether a microphone track is recorded (live since Story 19.3).
    pub microphone: bool,
    /// Whether the separate `camera-####.mov` camera files are recorded
    /// (live since Story 20.1; `false` while the Webcam switch is off).
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
    /// Optional user-supplied session metadata (Story 21.5): who/what this
    /// recording is about. Local-only — never uploaded anywhere.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub meta: Option<SessionMeta>,
    /// Wall-clock session start, ISO-8601 with UTC offset (Story 21.5).
    /// Host-clock PTS bounds stay authoritative for continuity; this is the
    /// human-facing "when". Absent in pre-21.5 manifests (tolerated on read).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub started_at: Option<String>,
    /// Wall-clock session end, written at every terminal reconcile.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub ended_at: Option<String>,
    /// The absolute session folder path (runtime-only, never serialized).
    #[serde(skip)]
    folder: PathBuf,
}

/// Optional user-supplied session metadata (Story 21.5): all fields free-text
/// and optional; absent fields are omitted from the serialized manifest.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionMeta {
    /// A human title for the session (also drives the folder name host-side).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub title: Option<String>,
    /// Who the conversation/recording is with.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub participants: Option<String>,
    /// Which program/session this is (free-form note).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub note: Option<String>,
    /// Free-form tags (Story 22.3) — comma-separated in the UI, a list here.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tags: Option<Vec<String>>,
    /// Repeatable custom name/value pairs (Story 22.3).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub custom: Option<Vec<SessionMetaField>>,
}

/// One custom name/value metadata pair (Story 22.3) — both free text.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMetaField {
    /// The field's user-chosen name.
    pub name: String,
    /// The field's value.
    pub value: String,
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
        Self::create_with_meta(folder, capture_target, devices, None, None)
    }

    /// [`Self::create`] plus the optional user meta and the wall-clock start
    /// stamp (Story 21.5). `started_at` is host-supplied (ISO-8601 with
    /// offset) so this platform-free module never touches a clock.
    pub fn create_with_meta(
        folder: PathBuf,
        capture_target: CaptureTarget,
        devices: SessionDevices,
        meta: Option<SessionMeta>,
        started_at: Option<String>,
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
            meta,
            started_at,
            ended_at: None,
            folder,
        };
        manifest.write()?;
        Ok(manifest)
    }

    /// Stamp the wall-clock session end (Story 21.5) — the caller supplies the
    /// ISO-8601 instant and then [`Self::write`]s (typically alongside the
    /// terminal status + reconcile).
    pub fn set_ended_at(&mut self, ended_at: String) {
        self.ended_at = Some(ended_at);
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

    /// The number of **screen-track** segments in the ledger (Story 20.3, FR-71)
    /// — the authoritative "Saved N segments" count. Filters `segments` to
    /// `track == "screen"`, so a camera-track segment (Story 20.1) never inflates
    /// the count and, in a rotation, screen == camera so screen is authoritative.
    /// Read straight off the manifest, which the terminal
    /// [`Self::reconcile_from_dir`] rebuilds from the on-disk `.mov` files
    /// (including the final never-closed segment) — never the live
    /// `segments_closed` rotation counter, which is 0 for a single-segment
    /// session and track-agnostic.
    pub fn screen_segment_count(&self) -> u32 {
        self.segments
            .iter()
            .filter(|segment| segment.track == "screen")
            .count() as u32
    }

    /// The total on-disk bytes across **all** segments (Story 20.3) — screen and
    /// camera tracks summed — for the completion/recovery card's `{size}` line.
    /// The manifest's `bytes` are `fs::metadata`-authoritative at every terminal.
    pub fn total_bytes(&self) -> u64 {
        self.segments.iter().map(|segment| segment.bytes).sum()
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

    /// Rebuild the segment ledger **entirely from the on-disk `.mov` files** —
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
    /// `pts_start`/`pts_end` can NEVER be recovered from the `.mov` files.
    /// Each rebuilt entry therefore inherits the bounds the event-fed ledger
    /// already recorded for its `(track, index)` pair (`None` — persisted as
    /// `null` — when no prior entry exists: the final segment, a DW-992
    /// backfill, an older sidecar). Since Story 20.1 both `screen-####` and
    /// `camera-####` files are ingested into the ONE segments vec,
    /// disambiguated by `track` (FR-73).
    pub fn reconcile_from_dir(&mut self) -> Result<(), RecordingError> {
        let entries =
            std::fs::read_dir(&self.folder).map_err(|e| manifest_io("read session folder", &e))?;
        // Snapshot the only-capture-time-known bounds by (track, index)
        // before the event-fed list is discarded. Keyed on the PAIR (Story
        // 20.1, FR-73): `screen-0000` and `camera-0000` share an index by
        // design, so an index-only key would let one track's bounds clobber
        // the other's.
        let known_bounds: std::collections::HashMap<(String, u32), (Option<f64>, Option<f64>)> =
            self.segments
                .iter()
                .map(|s| ((s.track.clone(), s.index), (s.pts_start, s.pts_end)))
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
            let is_segment_file =
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| {
                        ext.eq_ignore_ascii_case("mov") || ext.eq_ignore_ascii_case("m4a")
                    });
            if !is_segment_file {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                tracing::warn!("manifest reconcile: skipping .mov with a non-UTF-8 stem");
                continue;
            };
            // Only this session's own `screen-####.mov` / `camera-####.mov`
            // segments (Story 20.1) are authoritative ledger entries — a stray
            // `*.mov` with a trailing digit run must not pollute the ledger or
            // collide on `(track, index)`. The stem prefix names the track.
            let track = if stem.starts_with(SEGMENT_STEM_PREFIX) {
                "screen"
            } else if stem.starts_with(CAMERA_SEGMENT_STEM_PREFIX) {
                "camera"
            } else if stem.starts_with(AUDIO_SEGMENT_STEM_PREFIX) {
                // Story 21.3: audio-only sessions ledger their `audio-####.m4a`
                // files as their own track.
                "audio"
            } else {
                continue;
            };
            let Some(index) = segment_index_from_stem(stem) else {
                tracing::warn!(
                    "manifest reconcile: skipping segment-prefixed file without a segment index"
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
                tracing::warn!("manifest reconcile: skipping non-file .mov entry");
                continue;
            }
            let (pts_start, pts_end) = known_bounds
                .get(&(track.to_owned(), index))
                .copied()
                .unwrap_or((None, None));
            segments.push(SegmentEntry {
                index,
                file: file.to_owned(),
                bytes: metadata.len(),
                track: track.to_owned(),
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

    /// Load a session's `manifest.json` from `folder` (Story 17.3). Read
    /// `folder/manifest.json`, deserialize it, and re-bind the runtime-only
    /// `#[serde(skip)]` folder field to `folder` (the persisted manifest never
    /// carries an absolute path — the caller supplies it, so a subsequent
    /// [`Self::reconcile_from_dir`]/[`Self::write`] operates on the right
    /// path). Any fs read or parse failure surfaces as a secret-free
    /// [`RecordingError::ManifestIo`] (the message names the failing operation
    /// only — never the folder path or any captured-media path), mirroring the
    /// write-side discipline.
    pub fn load(folder: &Path) -> Result<Self, RecordingError> {
        let raw = std::fs::read_to_string(folder.join("manifest.json"))
            .map_err(|e| manifest_io("read session manifest", &e))?;
        let mut manifest: Self = serde_json::from_str(&raw)
            .map_err(|e| RecordingError::ManifestIo(format!("parse session manifest: {e}")))?;
        manifest.folder = folder.to_path_buf();
        Ok(manifest)
    }
}

/// Scan `base_dir` for crash-orphaned sessions and recover each one (Story
/// 17.3, FR-73, AD-37) — a platform-free, disk-authoritative, best-effort
/// recovery pass. Returns the folders it marked `recovered` (sorted, so the
/// list is deterministic across `read_dir` orders and platforms).
///
/// For each immediate subdirectory of `base_dir` — **symlinked entries are
/// skipped** via the `DirEntry` file type (which does not follow symlinks), so
/// recovery never rewrites a manifest outside the destination tree — that
/// holds a `manifest.json`, [`SessionManifest::load`] it; when its `status` is
/// still [`ManifestStatus::Recording`] AND its `version` is within
/// [`MANIFEST_VERSION`] AND `is_active` reports the folder inactive, rebuild
/// the segment ledger from the on-disk `.mov` files
/// ([`SessionManifest::reconcile_from_dir`] — disk is authoritative, the stale
/// event-fed list is discarded, never a bare status flip), mark the manifest
/// [`ManifestStatus::Recovered`], and atomically rewrite ONLY `manifest.json`
/// ([`SessionManifest::write`] — no segment file is ever opened for write, so
/// recovery is remux-free).
///
/// **The live-session guard.** An on-disk `status:"recording"` cannot by
/// itself distinguish a crashed orphan from a session that is *currently*
/// recording (a live session persists `recording` for its whole duration), so
/// the shell passes `is_active` — a predicate over its reserved-live-folder
/// set. It is consulted **immediately before** the salvage write, and a
/// reserved folder is skipped untouched: recovery must never rewrite an active
/// session's manifest to `recovered` mid-capture. A bare `&dyn Fn` keeps this
/// module firewall-clean — no shell or Apple types cross the seam.
///
/// Best-effort and total: a missing `base_dir` is "no recordings yet" (silent
/// empty list); an unreadable `base_dir` is logged and yields an empty list;
/// an entry that is not a session dir, is symlinked, has no/unreadable/
/// malformed manifest, is not `recording`, is a newer schema version, is
/// reserved live, or whose per-folder reconcile/write fails is skipped (logged
/// via `tracing`) — one bad entry NEVER aborts the scan, and the pass never
/// propagates an error or panics. Because a recovered manifest is no longer
/// `recording`, a second run is a no-op (idempotent).
///
/// Firewall-clean: `std::fs` + serde + the bare predicate only — no shell, no
/// Apple framework, no process API (enforced by
/// `tests::dependency_firewall_holds`).
pub fn recover_orphaned_sessions(
    base_dir: &Path,
    is_active: &dyn Fn(&Path) -> bool,
) -> Vec<PathBuf> {
    let entries = match std::fs::read_dir(base_dir) {
        Ok(entries) => entries,
        // A missing base dir means "no recordings yet" — not even worth a warn.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            tracing::warn!(%error, "recovery: could not read the recordings base dir (non-fatal)");
            return Vec::new();
        }
    };
    let mut recovered = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                tracing::warn!(%error, "recovery: skipping unreadable base-dir entry");
                continue;
            }
        };
        // `DirEntry::file_type` does not follow symlinks (and costs no extra
        // syscall on the platforms keeper ships on): a symlinked entry — which
        // `is_dir()`-style probes would follow — must never be recovered, or
        // the pass would rewrite a manifest OUTSIDE the destination tree.
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                tracing::warn!(%error, "recovery: skipping entry with unreadable file type");
                continue;
            }
        };
        if file_type.is_symlink() {
            tracing::debug!("recovery: skipping symlinked base-dir entry");
            continue;
        }
        // Only immediate real subdirectories are session folders; a loose file
        // is a stray, never a session.
        if !file_type.is_dir() {
            continue;
        }
        let folder = entry.path();
        // A subdirectory without a `manifest.json` is not a session folder
        // (`load` backstops a real session whose probe races with a warn).
        if !folder.join("manifest.json").is_file() {
            continue;
        }
        let mut manifest = match SessionManifest::load(&folder) {
            Ok(manifest) => manifest,
            Err(error) => {
                // Unreadable / malformed manifest — skip this folder, never
                // abort the scan of its siblings.
                tracing::warn!(%error, "recovery: skipping folder with an unreadable manifest");
                continue;
            }
        };
        // Only a still-`recording` manifest can be orphaned; a terminal
        // (`finalized`/`recovered`/`failed`) session is read once and left
        // untouched — this is what makes recovery idempotent.
        if manifest.status != ManifestStatus::Recording {
            continue;
        }
        // Never rewrite an unknown future schema.
        if manifest.version > MANIFEST_VERSION {
            tracing::warn!(
                version = manifest.version,
                "recovery: skipping newer-schema manifest"
            );
            continue;
        }
        // The live-session guard, consulted IMMEDIATELY before the salvage: a
        // folder the shell has reserved belongs to a session that is live (or
        // mid-start, or still landing its terminal write) — skip it untouched.
        // A genuinely-crashed orphan is never reserved and still salvages.
        if is_active(&folder) {
            tracing::debug!("recovery: skipping reserved live session folder");
            continue;
        }
        // Rebuild the ledger from disk (remux-free — only `manifest.json` is
        // written), mark recovered, atomically rewrite.
        if let Err(error) = manifest.reconcile_from_dir() {
            tracing::warn!(%error, "recovery: skipping folder whose segment dir could not be read");
            continue;
        }
        manifest.set_status(ManifestStatus::Recovered);
        if let Err(error) = manifest.write() {
            tracing::warn!(%error, "recovery: skipping folder whose manifest could not be rewritten");
            continue;
        }
        recovered.push(folder);
    }
    // `read_dir` order is filesystem-defined; sort so the recovered list is
    // deterministic across runs and platforms.
    recovered.sort();
    recovered
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

/// Sum the on-disk byte sizes of this session's own `screen-####.mov` segment
/// files in `folder` (Story 18.1) — the live figure behind the tray's
/// elapsed/segment/size line.
///
/// Applies the exact ownership rule of [`SessionManifest::reconcile_from_dir`]
/// ([`SEGMENT_STEM_PREFIX`] plus a trailing numeric run), so the growing tray
/// figure always matches what the eventual terminal manifest will report: a
/// stray `*.mov` (a user drop, a future `camera-####.mov`), `manifest.json`,
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
        let is_mov = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("mov"));
        if !is_mov {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let is_session_stem =
            stem.starts_with(SEGMENT_STEM_PREFIX) || stem.starts_with(AUDIO_SEGMENT_STEM_PREFIX);
        if !is_session_stem || segment_index_from_stem(stem).is_none() {
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

/// The on-disk length of this session's **current** (open) segment — the
/// highest-index `screen-####.mov` file in `folder` (Story 18.3) — the numerator
/// behind the in-app segment meter.
///
/// Applies the exact ownership rule of [`session_bytes_on_disk`]
/// ([`SEGMENT_STEM_PREFIX`] plus a trailing numeric run), so a stray `*.mov`, a
/// future `camera-####.mov`, `manifest.json`, and directories are ignored.
/// Whereas [`session_bytes_on_disk`] sums *all* segments (the total size line),
/// this returns only the length of the highest-index one — the segment capture
/// is actively writing — so the figure naturally falls back toward ~0 at each
/// gapless rotation as a fresh segment file starts. Best-effort and total: a
/// missing/unreadable folder, no matching segment, or an unreadable entry
/// contributes 0, never an error, never a panic.
pub fn current_segment_bytes_on_disk(folder: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return 0;
    };
    let mut current: Option<(u32, u64)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        let is_mov = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("mov"));
        if !is_mov {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if !(stem.starts_with(SEGMENT_STEM_PREFIX) || stem.starts_with(AUDIO_SEGMENT_STEM_PREFIX)) {
            continue;
        }
        let Some(index) = segment_index_from_stem(stem) else {
            continue;
        };
        // `fs::metadata` (follows symlinks) — the real file's current length.
        let Ok(metadata) = std::fs::metadata(&path) else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        if current.is_none_or(|(highest, _)| index >= highest) {
            current = Some((index, metadata.len()));
        }
    }
    current.map_or(0, |(_, bytes)| bytes)
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

    /// Run the `requestMicrophone` round-trip (Story 19.3, FR-69, AD-36): ask
    /// the sidecar to request microphone access (the OS shows its one real
    /// prompt where allowed) and resolve the reported post-request
    /// [`TccPermission`] tri-state. Only ever called lazily — when the user
    /// enables the mic source, never preemptively. Same error surface as
    /// [`Recorder::get_capabilities`].
    fn request_microphone(&self) -> impl Future<Output = Result<TccPermission, CoreError>> + Send;

    /// Run the `requestCamera` round-trip (Story 20.1, FR-70, AD-36): ask
    /// the sidecar to request camera access (the OS shows its one real
    /// prompt where allowed) and resolve the reported post-request
    /// [`TccPermission`] tri-state. Only ever called lazily — when the user
    /// enables the Webcam switch, never preemptively; a denial never blocks
    /// Start (the mic precedent). Same error surface as
    /// [`Recorder::get_capabilities`].
    fn request_camera(&self) -> impl Future<Output = Result<TccPermission, CoreError>> + Send;
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

    // --- non-fatal warning (Story 19.4) -------------------------------------

    /// A `micLost` warning event with the given message.
    fn warning(message: &str) -> RecordingEvent {
        RecordingEvent::Warning {
            code: "micLost".to_owned(),
            message: message.to_owned(),
        }
    }

    #[test]
    fn warning_keeps_state_and_is_sticky_across_events() {
        // The never-abort proof (Story 19.4): a mic-loss warning mid-recording
        // changes NO state, segments keep closing, the sticky message is
        // last-write-wins, and the session still reaches `Finalized` on stop.
        let mut session = RecordingSession::new();
        session
            .apply(RecordingEvent::PreflightStarted)
            .expect("legal transition");
        session
            .apply(RecordingEvent::CaptureStarted)
            .expect("legal transition");
        session
            .apply(warning(
                "microphone disconnected — using system default input",
            ))
            .expect("warning is legal while Recording");
        assert_eq!(session.state(), SessionState::Recording, "no state change");
        assert_eq!(
            session.warning(),
            Some("microphone disconnected — using system default input")
        );
        // Segments keep closing; the warning survives every later event.
        session
            .apply(segment_closed(0))
            .expect("segmentClosed still legal after a warning");
        session
            .apply(RecordingEvent::SegmentRotating)
            .expect("legal transition");
        session
            .apply(warning("microphone disconnected — no microphone input"))
            .expect("warning is legal while Rotating");
        assert_eq!(
            session.warning(),
            Some("microphone disconnected — no microphone input"),
            "the sticky message is last-write-wins"
        );
        session
            .apply(RecordingEvent::CaptureStarted)
            .expect("legal transition");
        session
            .apply(RecordingEvent::Stopping)
            .expect("legal transition");
        session
            .apply(warning("late but still live"))
            .expect("warning is legal while Stopping");
        session
            .apply(RecordingEvent::Finalized)
            .expect("legal transition");
        assert_eq!(
            session.state(),
            SessionState::Finalized,
            "a warned session still finalizes — never Failed"
        );
        assert_eq!(session.warning(), Some("late but still live"));
        assert_eq!(session.segments_closed(), 1);
    }

    #[test]
    fn warning_outside_live_states_is_rejected_without_effect() {
        // Idle and Preflight: not live yet — rejected, no warning recorded.
        for events in [vec![], vec![RecordingEvent::PreflightStarted]] {
            let mut session = RecordingSession::new();
            for event in events {
                session.apply(event).expect("legal setup transition");
            }
            let before = session.state();
            let err = session
                .apply(warning("too early"))
                .expect_err("warning outside live states is illegal");
            assert!(matches!(
                err,
                RecordingError::IllegalTransition { event, .. } if event == "warning"
            ));
            assert_eq!(session.state(), before, "state unchanged");
            assert_eq!(session.warning(), None, "no warning set");
        }
        // Terminal: a late warning never resurrects a settled session.
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
        assert!(session.apply(warning("too late")).is_err());
        assert_eq!(session.state(), SessionState::Finalized);
        assert_eq!(session.warning(), None);
    }

    #[test]
    fn parse_warning_reads_code_and_message() {
        assert_eq!(
            parse_event(
                r#"{"event":"warning","code":"micLost","message":"microphone disconnected — using system default input"}"#
            ),
            Some(warning(
                "microphone disconnected — using system default input"
            ))
        );
    }

    #[test]
    fn parse_warning_tolerates_missing_or_malformed_fields() {
        // A missing / mistyped / blank `code` or `message` degrades to the
        // stable default, and unknown extra keys are ignored — the warning is
        // never dropped (unlike an unknown discriminator, which yields None).
        for line in [
            r#"{"event":"warning"}"#,
            r#"{"event":"warning","code":"micLost"}"#,
            r#"{"event":"warning","message":42}"#,
            r#"{"event":"warning","code":7,"message":"   "}"#,
            r#"{"event":"warning","code":"","message":null}"#,
        ] {
            match parse_event(line) {
                Some(RecordingEvent::Warning { code, message }) => {
                    assert!(!code.trim().is_empty(), "line {line:?}: defaulted code");
                    assert!(
                        !message.trim().is_empty(),
                        "line {line:?}: defaulted message"
                    );
                }
                other => panic!("line {line:?} must parse as a Warning, got {other:?}"),
            }
        }
        // A fully-specified line with an unknown extra key keeps its fields.
        assert_eq!(
            parse_event(r#"{"event":"warning","code":"micLost","message":"m","extra":true}"#),
            Some(warning("m"))
        );
    }

    #[tokio::test]
    async fn drive_session_with_a_warning_still_finalizes() {
        // The simulated-signal never-abort proof at the drive level (Story
        // 19.4): a warning mid-stream is folded in without an error and the
        // run still resolves the Finalized terminal.
        let recorder = FakeRecorder::new(
            true,
            vec![
                RecordingEvent::PreflightStarted,
                RecordingEvent::CaptureStarted,
                warning("microphone disconnected — using system default input"),
                segment_closed(0),
                RecordingEvent::Stopping,
                RecordingEvent::Finalized,
            ],
        );
        let (on_state, _seen) = state_collector();
        let terminal = drive_session(&recorder, test_params(), std::future::pending(), on_state)
            .await
            .expect("a warning is never a drive error");
        assert_eq!(terminal, SessionState::Finalized);
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
        let line = r#"{"event":"segmentClosed","index":2,"path":"/rec/keeper/screen-0002.mov","bytes":123,"track":"screen","ptsStart":1000.5,"ptsEnd":1030.25}"#;
        assert_eq!(
            parse_event(line),
            Some(RecordingEvent::SegmentClosed {
                index: 2,
                path: Some("/rec/keeper/screen-0002.mov".to_owned()),
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
        // A failure report with a missing/mistyped/blank `message` must NOT be dropped —
        // it surfaces a `Failed` with a NON-BLANK fallback so the session still reaches
        // its terminal AND the Story 18.4 loud-failure triad (tray/notification/banner)
        // always names a reason, never a reasonless "Recording failed — ".
        for line in [
            r#"{"event":"error"}"#,
            r#"{"event":"error","message":42}"#,
            r#"{"event":"error","message":{"nested":true}}"#,
            r#"{"event":"error","message":""}"#,
            r#"{"event":"error","message":"   "}"#,
        ] {
            let Some(RecordingEvent::Failed { message }) = parse_event(line) else {
                panic!("error line {line:?} must surface a Failed, never None");
            };
            assert!(
                !message.trim().is_empty(),
                "error line {line:?} must carry a non-blank reason for the triad"
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

    /// A healthy `listSources` response line: a real display plus a real
    /// application (Story 19.1) — one with an icon data-URI, one without (icon
    /// `null`) — exercising the `RecordingApplicationVm.icon: Option` mapping.
    const SOURCES_RESPONSE: &str = r#"{"id":2,"result":{"displays":[{"id":1,"width":3456,"height":2234,"isMain":true}],"applications":[{"bundleId":"com.apple.Safari","name":"Safari","pid":501,"icon":"data:image/png;base64,iVBORw0KGgo="},{"bundleId":"com.example.NoIcon","name":"No Icon","pid":777,"icon":null}],"microphones":[],"cameras":[]}}"#;

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
        assert_eq!(
            request_microphone_request(6),
            r#"{"id":6,"method":"requestMicrophone"}"#
        );
        assert_eq!(
            request_camera_request(7),
            r#"{"id":7,"method":"requestCamera"}"#
        );
        // No line framing inside the request — the shell port owns the newline.
        assert!(!capabilities_request(7).contains('\n'));
        assert!(!request_screen_recording_request(7).contains('\n'));
        assert!(!request_microphone_request(7).contains('\n'));
        assert!(!request_camera_request(7).contains('\n'));
    }

    #[test]
    fn start_recording_request_carries_the_segmentation_params() {
        // Story 17.5 (FR-72): every `start` always carries the configured
        // segment size and duration cap (in seconds) alongside the 16.6 fields.
        let params = SessionParams {
            output_path: "/tmp/keeper-rec/screen-0000.mov".to_owned(),
            display_id: Some(7),
            application: None,
            system_audio: true,
            microphone: None,
            camera: None,
            segment_mb: 800,
            max_segment_seconds: 2700,
            fps: 30,
            codec: "h264".to_owned(),
            scale_percent: 100,
            audio_only: false,
        };
        let line = start_recording_request(4, &params);
        assert!(!line.contains('\n'), "the shell port owns line framing");
        let wire: serde_json::Value = serde_json::from_str(&line).expect("request is JSON");
        assert_eq!(wire["id"], 4);
        assert_eq!(wire["method"], "startRecording");
        assert_eq!(wire["params"]["path"], "/tmp/keeper-rec/screen-0000.mov");
        assert_eq!(wire["params"]["systemAudio"], true);
        assert_eq!(wire["params"]["displayId"], 7);
        assert_eq!(wire["params"]["segmentMB"], 800);
        assert_eq!(wire["params"]["maxSegmentSeconds"], 2700);
        assert_eq!(wire["params"]["fps"], 30);

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

    #[test]
    fn start_recording_request_carries_system_audio_off() {
        // Story 19.2: toggling the Audio card off threads `system_audio: false`
        // all the way to the wire's `"systemAudio"` field — the sidecar reads
        // this to skip `capturesAudio`/`excludesCurrentProcessAudio` and write
        // no audio track (16.6). The default-`true` case is covered by
        // `start_recording_request_carries_the_segmentation_params` above.
        let params = SessionParams {
            output_path: "/tmp/keeper-rec/screen-0000.mov".to_owned(),
            display_id: None,
            application: None,
            system_audio: false,
            microphone: None,
            camera: None,
            segment_mb: 500,
            max_segment_seconds: 1800,
            fps: 30,
            codec: "h264".to_owned(),
            scale_percent: 100,
            audio_only: false,
        };
        let line = start_recording_request(6, &params);
        let wire: serde_json::Value = serde_json::from_str(&line).expect("request is JSON");
        assert_eq!(wire["params"]["systemAudio"], false);
    }

    #[test]
    fn start_recording_request_omits_mic_fields_while_off() {
        // Story 19.3: a mic-off session (`microphone: None`) carries NO
        // `micEnabled`/`micDeviceId` key at all — the 16.6/19.2 wire stays
        // byte-for-byte unchanged, and the sidecar adds no mic track.
        let params = SessionParams {
            output_path: "/tmp/keeper-rec/screen-0000.mov".to_owned(),
            display_id: None,
            application: None,
            system_audio: true,
            microphone: None,
            camera: None,
            segment_mb: 500,
            max_segment_seconds: 1800,
            fps: 30,
            codec: "h264".to_owned(),
            scale_percent: 100,
            audio_only: false,
        };
        let line = start_recording_request(10, &params);
        let wire: serde_json::Value = serde_json::from_str(&line).expect("request is JSON");
        assert!(
            wire["params"].get("micEnabled").is_none(),
            "a mic-off session must not carry micEnabled"
        );
        assert!(
            wire["params"].get("micDeviceId").is_none(),
            "a mic-off session must not carry micDeviceId"
        );
    }

    #[test]
    fn start_recording_request_carries_mic_enabled_with_default_input() {
        // Story 19.3: the mic on with the system default input — `micEnabled:
        // true` and NO `micDeviceId` (its absence means the default device).
        let params = SessionParams {
            output_path: "/tmp/keeper-rec/screen-0000.mov".to_owned(),
            display_id: None,
            application: None,
            system_audio: true,
            microphone: Some(MicSelection { device_id: None }),
            camera: None,
            segment_mb: 500,
            max_segment_seconds: 1800,
            fps: 30,
            codec: "h264".to_owned(),
            scale_percent: 100,
            audio_only: false,
        };
        let line = start_recording_request(11, &params);
        let wire: serde_json::Value = serde_json::from_str(&line).expect("request is JSON");
        assert_eq!(wire["params"]["micEnabled"], true);
        assert!(
            wire["params"].get("micDeviceId").is_none(),
            "the system default input must omit micDeviceId"
        );
        // The 16.6/19.2 fields ride along unchanged.
        assert_eq!(wire["params"]["systemAudio"], true);
        assert_eq!(wire["params"]["segmentMB"], 500);
    }

    #[test]
    fn start_recording_request_carries_mic_device_id() {
        // Story 19.3: a specific device picked in the Audio card threads its
        // opaque unique id through as `micDeviceId`.
        let params = SessionParams {
            output_path: "/tmp/keeper-rec/screen-0000.mov".to_owned(),
            display_id: None,
            application: None,
            system_audio: false,
            microphone: Some(MicSelection {
                device_id: Some("X".to_owned()),
            }),
            camera: None,
            segment_mb: 500,
            max_segment_seconds: 1800,
            fps: 30,
            codec: "h264".to_owned(),
            scale_percent: 100,
            audio_only: false,
        };
        let line = start_recording_request(12, &params);
        let wire: serde_json::Value = serde_json::from_str(&line).expect("request is JSON");
        assert_eq!(wire["params"]["micEnabled"], true);
        assert_eq!(wire["params"]["micDeviceId"], "X");
        // The mic is independent of system audio — off here, mic still on.
        assert_eq!(wire["params"]["systemAudio"], false);
    }

    #[test]
    fn parse_request_microphone_result_maps_every_status() {
        // Story 19.3: the sidecar's post-request audio-permission tri-state
        // maps onto `TccPermission` verbatim.
        for (status, expected) in [
            ("granted", TccPermission::Granted),
            ("denied", TccPermission::Denied),
            ("notDetermined", TccPermission::NotDetermined),
        ] {
            let line = format!(r#"{{"id":6,"result":{{"status":"{status}"}}}}"#);
            assert_eq!(
                parse_request_microphone_result(&line).expect("status parses"),
                expected
            );
        }
    }

    #[test]
    fn parse_request_microphone_result_surfaces_faults() {
        // A sidecar `error` answer, a missing status, and an unknown status
        // string all surface as `Protocol` — never a panic.
        for line in [
            r#"{"id":6,"error":{"code":"unknownMethod","message":"unknown method"}}"#,
            r#"{"id":6,"result":{}}"#,
            r#"{"id":6,"result":{"status":"maybe"}}"#,
            "not json",
        ] {
            assert!(
                matches!(
                    parse_request_microphone_result(line),
                    Err(RecordingError::Protocol(_))
                ),
                "line {line:?} must surface a Protocol fault"
            );
        }
    }

    #[test]
    fn start_recording_request_omits_camera_fields_while_off() {
        // Story 20.1: a camera-off session (`camera: None`, the default)
        // carries NO `cameraEnabled`/`cameraDeviceId` key at all — the
        // pre-20.1 wire stays byte-for-byte unchanged, and the sidecar
        // writes no camera file and touches no Camera-TCC.
        let params = SessionParams {
            output_path: "/tmp/keeper-rec/screen-0000.mov".to_owned(),
            display_id: None,
            application: None,
            system_audio: true,
            microphone: None,
            camera: None,
            segment_mb: 500,
            max_segment_seconds: 1800,
            fps: 30,
            codec: "h264".to_owned(),
            scale_percent: 100,
            audio_only: false,
        };
        let line = start_recording_request(14, &params);
        let wire: serde_json::Value = serde_json::from_str(&line).expect("request is JSON");
        assert!(
            wire["params"].get("cameraEnabled").is_none(),
            "a camera-off session must not carry cameraEnabled"
        );
        assert!(
            wire["params"].get("cameraDeviceId").is_none(),
            "a camera-off session must not carry cameraDeviceId"
        );
    }

    #[test]
    fn start_recording_request_carries_camera_enabled_with_default_device() {
        // Story 20.1: the webcam on with the system default camera —
        // `cameraEnabled: true` and NO `cameraDeviceId` (its absence means
        // the default device, the mic-wire precedent).
        let params = SessionParams {
            output_path: "/tmp/keeper-rec/screen-0000.mov".to_owned(),
            display_id: None,
            application: None,
            system_audio: true,
            microphone: None,
            camera: Some(CameraSelection { device_id: None }),
            segment_mb: 500,
            max_segment_seconds: 1800,
            fps: 30,
            codec: "h264".to_owned(),
            scale_percent: 100,
            audio_only: false,
        };
        let line = start_recording_request(15, &params);
        let wire: serde_json::Value = serde_json::from_str(&line).expect("request is JSON");
        assert_eq!(wire["params"]["cameraEnabled"], true);
        assert!(
            wire["params"].get("cameraDeviceId").is_none(),
            "the system default camera must omit cameraDeviceId"
        );
        // The earlier fields ride along unchanged.
        assert_eq!(wire["params"]["systemAudio"], true);
        assert_eq!(wire["params"]["segmentMB"], 500);
    }

    #[test]
    fn start_recording_request_carries_camera_device_id_and_mic_together() {
        // Story 20.1: a specific camera picked on the Webcam card threads its
        // opaque unique id through as `cameraDeviceId` — and the camera is
        // independent of the mic (both sources on, distinct keys).
        let params = SessionParams {
            output_path: "/tmp/keeper-rec/screen-0000.mov".to_owned(),
            display_id: None,
            application: None,
            system_audio: false,
            microphone: Some(MicSelection {
                device_id: Some("MIC".to_owned()),
            }),
            camera: Some(CameraSelection {
                device_id: Some("CAM".to_owned()),
            }),
            segment_mb: 500,
            max_segment_seconds: 1800,
            fps: 30,
            codec: "h264".to_owned(),
            scale_percent: 100,
            audio_only: false,
        };
        let line = start_recording_request(16, &params);
        let wire: serde_json::Value = serde_json::from_str(&line).expect("request is JSON");
        assert_eq!(wire["params"]["cameraEnabled"], true);
        assert_eq!(wire["params"]["cameraDeviceId"], "CAM");
        assert_eq!(wire["params"]["micEnabled"], true);
        assert_eq!(wire["params"]["micDeviceId"], "MIC");
        assert_eq!(wire["params"]["systemAudio"], false);
    }

    #[test]
    fn parse_request_camera_result_maps_every_status() {
        // Story 20.1: the sidecar's post-request video-permission tri-state
        // maps onto `TccPermission` verbatim (the requestMicrophone twin).
        for (status, expected) in [
            ("granted", TccPermission::Granted),
            ("denied", TccPermission::Denied),
            ("notDetermined", TccPermission::NotDetermined),
        ] {
            let line = format!(r#"{{"id":7,"result":{{"status":"{status}"}}}}"#);
            assert_eq!(
                parse_request_camera_result(&line).expect("status parses"),
                expected
            );
        }
    }

    #[test]
    fn parse_request_camera_result_surfaces_faults() {
        // A sidecar `error` answer, a missing status, and an unknown status
        // string all surface as `Protocol` — never a panic.
        for line in [
            r#"{"id":7,"error":{"code":"unknownMethod","message":"unknown method"}}"#,
            r#"{"id":7,"result":{}}"#,
            r#"{"id":7,"result":{"status":"maybe"}}"#,
            "not json",
        ] {
            assert!(
                matches!(
                    parse_request_camera_result(line),
                    Err(RecordingError::Protocol(_))
                ),
                "line {line:?} must surface a Protocol fault"
            );
        }
    }

    #[test]
    fn start_recording_request_app_target_wins_over_display() {
        // Story 19.1: an application target emits `applicationPid`+`bundleId`
        // and OMITS `displayId` (even when a display id is also set), so the
        // sidecar builds the exclusionary app-scoped filter.
        let params = SessionParams {
            output_path: "/tmp/keeper-rec/screen-0000.mov".to_owned(),
            // A stray display id must be ignored once an app target is present.
            display_id: Some(7),
            application: Some(ApplicationTarget {
                pid: 4242,
                bundle_id: "com.apple.Safari".to_owned(),
            }),
            system_audio: true,
            microphone: None,
            camera: None,
            segment_mb: 500,
            max_segment_seconds: 1800,
            fps: 30,
            codec: "h264".to_owned(),
            scale_percent: 100,
            audio_only: false,
        };
        let line = start_recording_request(9, &params);
        let wire: serde_json::Value = serde_json::from_str(&line).expect("request is JSON");
        assert_eq!(wire["params"]["applicationPid"], 4242);
        assert_eq!(wire["params"]["bundleId"], "com.apple.Safari");
        assert!(
            wire["params"].get("displayId").is_none(),
            "an application target must omit displayId"
        );
        // The always-present segmentation + audio fields are unchanged.
        assert_eq!(wire["params"]["systemAudio"], true);
        assert_eq!(wire["params"]["segmentMB"], 500);
        assert_eq!(wire["params"]["maxSegmentSeconds"], 1800);
    }

    #[test]
    fn start_recording_request_always_carries_fps() {
        // Story 19.5: every `start` carries an always-present `"fps"` field
        // (like `segmentMB`) — 30 by default and 60 when selected. The sidecar
        // decodes it best-effort (absent → 30), so the field stays additive and
        // PROTOCOL_VERSION stays 1.
        for fps in [30u32, 60] {
            let params = SessionParams {
                output_path: "/tmp/keeper-rec/screen-0000.mov".to_owned(),
                display_id: None,
                application: None,
                system_audio: true,
                microphone: None,
                camera: None,
                segment_mb: 500,
                max_segment_seconds: 1800,
                fps,
                codec: "h264".to_owned(),
                scale_percent: 100,
                audio_only: false,
            };
            let line = start_recording_request(13, &params);
            let wire: serde_json::Value = serde_json::from_str(&line).expect("request is JSON");
            assert_eq!(wire["params"]["fps"], fps, "fps {fps} must ride the wire");
        }
    }

    #[test]
    fn evaluate_destination_accepts_a_usable_folder() {
        // Story 19.5: exists/creatable + writable + roomy volume → Ok.
        assert!(
            evaluate_destination(true, true, 500 * 1_000_000_000, RECORDING_MIN_FREE_BYTES).is_ok()
        );
    }

    #[test]
    fn evaluate_destination_rejects_a_non_directory() {
        // A destination that neither exists nor can be created is the most
        // fundamental rejection — it wins over writability and free space.
        assert_eq!(
            evaluate_destination(false, false, 0, RECORDING_MIN_FREE_BYTES),
            Err(DestinationRejection::NotADirectory)
        );
    }

    #[test]
    fn evaluate_destination_rejects_an_unwritable_folder() {
        assert_eq!(
            evaluate_destination(true, false, 500 * 1_000_000_000, RECORDING_MIN_FREE_BYTES),
            Err(DestinationRejection::NotWritable)
        );
    }

    #[test]
    fn evaluate_destination_rejects_simulated_low_free_space() {
        // The simulated-signal proof: the low-free-space path is exercised by
        // injecting a small `free_bytes` — never by filling a real disk. The
        // rejection carries both figures so the error can name the shortfall.
        let free = RECORDING_MIN_FREE_BYTES - 1;
        assert_eq!(
            evaluate_destination(true, true, free, RECORDING_MIN_FREE_BYTES),
            Err(DestinationRejection::InsufficientSpace {
                free_bytes: free,
                required_bytes: RECORDING_MIN_FREE_BYTES,
            })
        );
        // The rejection's message is actionable and secret-free — it names the
        // figures and the fix, never a filesystem path.
        let message = DestinationRejection::InsufficientSpace {
            free_bytes: free,
            required_bytes: RECORDING_MIN_FREE_BYTES,
        }
        .to_string();
        assert!(message.contains("free up space"), "actionable: {message}");
        assert!(!message.contains('/'), "secret-free (no path): {message}");
    }

    #[test]
    fn evaluate_destination_accepts_the_exact_floor() {
        // Exactly the hard floor is enough — the guard is `<`, not `<=`.
        assert!(evaluate_destination(
            true,
            true,
            RECORDING_MIN_FREE_BYTES,
            RECORDING_MIN_FREE_BYTES
        )
        .is_ok());
    }

    // --- Story 18.5: live disk-guard policy (simulated free-space signal) ---

    /// Shorthand: plan one guard tick at the authored thresholds.
    fn plan(free_bytes: u64, latch: &mut DiskGuardLatch) -> DiskGuardAction {
        plan_disk_guard_action(
            free_bytes,
            RECORDING_WARN_FREE_BYTES,
            RECORDING_MIN_FREE_BYTES,
            latch,
        )
    }

    #[test]
    fn evaluate_disk_guard_classifies_the_bands_and_exact_boundaries() {
        // The whole guard is exercised by injecting simulated `free_bytes` —
        // never by filling a real disk.
        let warn = RECORDING_WARN_FREE_BYTES;
        let floor = RECORDING_MIN_FREE_BYTES;
        // Healthy band: at or above the warn threshold.
        assert_eq!(
            evaluate_disk_guard(u64::MAX, warn, floor),
            DiskGuardDecision::Ok
        );
        assert_eq!(
            evaluate_disk_guard(warn + 1, warn, floor),
            DiskGuardDecision::Ok
        );
        // Exactly the warn threshold is still Ok — the guard is `<`, not `<=`
        // (mirrors `evaluate_destination`'s floor boundary).
        assert_eq!(
            evaluate_disk_guard(warn, warn, floor),
            DiskGuardDecision::Ok
        );
        // Warn band: below warn, at or above the floor.
        assert_eq!(
            evaluate_disk_guard(warn - 1, warn, floor),
            DiskGuardDecision::Warn
        );
        // Exactly the floor still warns rather than stops.
        assert_eq!(
            evaluate_disk_guard(floor, warn, floor),
            DiskGuardDecision::Warn
        );
        // Stop band: below the floor.
        assert_eq!(
            evaluate_disk_guard(floor - 1, warn, floor),
            DiskGuardDecision::Stop
        );
        assert_eq!(evaluate_disk_guard(0, warn, floor), DiskGuardDecision::Stop);
    }

    #[test]
    fn plan_disk_guard_warn_fires_once_with_the_free_space_copy() {
        let mut latch = DiskGuardLatch::default();
        // 9 GiB free: inside the warn band. The copy names the probed figure
        // in decimal GB (the pre-start rejection's formatting).
        let free = 9 * 1024 * 1024 * 1024;
        assert_eq!(
            plan(free, &mut latch),
            DiskGuardAction::Warn {
                message: "Low disk space — 9.7 GB free".to_owned(),
            }
        );
        assert!(latch.warned);
        assert!(!latch.stopped);
        // Warn-sticky: later ticks still in the warn band plan nothing more —
        // the sticky warning persists on the snapshot; no repeat notification.
        assert_eq!(plan(free, &mut latch), DiskGuardAction::None);
        assert_eq!(plan(free - 1024, &mut latch), DiskGuardAction::None);
    }

    #[test]
    fn plan_disk_guard_warn_then_stop_yields_each_event_exactly_once() {
        let mut latch = DiskGuardLatch::default();
        assert!(matches!(
            plan(RECORDING_WARN_FREE_BYTES - 1, &mut latch),
            DiskGuardAction::Warn { .. }
        ));
        // The hard-floor stop is a DISTINCT event: it still fires even though
        // a warn already did — a user who ignored the warn is still pinged.
        assert_eq!(
            plan(RECORDING_MIN_FREE_BYTES - 1, &mut latch),
            DiskGuardAction::Stop {
                message: "Recording stopped — low disk".to_owned(),
            }
        );
        assert!(latch.stopped);
        // Post-stop: always None, never a re-issued stop.
        assert_eq!(
            plan(RECORDING_MIN_FREE_BYTES - 1, &mut latch),
            DiskGuardAction::None
        );
    }

    #[test]
    fn plan_disk_guard_sudden_drop_plans_the_stop_only() {
        // A plunge straight past both thresholds in one tick emits only the
        // Stop — the warn is skipped, not queued.
        let mut latch = DiskGuardLatch::default();
        assert!(matches!(
            plan(RECORDING_MIN_FREE_BYTES - 1, &mut latch),
            DiskGuardAction::Stop { .. }
        ));
        assert!(!latch.warned, "the skipped warn is never latched");
        assert_eq!(plan(0, &mut latch), DiskGuardAction::None);
    }

    #[test]
    fn plan_disk_guard_post_stop_is_always_none_whatever_the_band() {
        // Once stopped, even a recovered volume (back in the warn or healthy
        // band) never resurrects the guard mid-session.
        let mut latch = DiskGuardLatch {
            warned: false,
            stopped: true,
        };
        assert_eq!(plan(0, &mut latch), DiskGuardAction::None);
        assert_eq!(
            plan(RECORDING_WARN_FREE_BYTES - 1, &mut latch),
            DiskGuardAction::None
        );
        assert_eq!(plan(u64::MAX, &mut latch), DiskGuardAction::None);
        assert!(!latch.warned, "post-stop ticks never latch a late warn");
    }

    #[test]
    fn plan_disk_guard_failed_probe_reads_as_plenty() {
        // Fail-open: the shell reports a probe error as `u64::MAX`, which the
        // policy classifies as healthy — never a warn or stop on a failed stat.
        let mut latch = DiskGuardLatch::default();
        assert_eq!(plan(u64::MAX, &mut latch), DiskGuardAction::None);
        assert_eq!(latch, DiskGuardLatch::default());
    }

    #[test]
    fn capture_target_application_round_trips() {
        // Story 19.1: the manifest's application `CaptureTarget` serializes to
        // `{kind:"application",bundleId,pid}` (no `displayId`) and round-trips.
        let target = CaptureTarget::application("com.apple.Safari".to_owned(), 501);
        let value = serde_json::to_value(&target).expect("serialize");
        assert_eq!(value["kind"], "application");
        assert_eq!(value["bundleId"], "com.apple.Safari");
        assert_eq!(value["pid"], 501);
        assert!(
            value.get("displayId").is_none(),
            "an application target omits displayId"
        );
        let parsed: CaptureTarget = serde_json::from_value(value).expect("round-trip");
        assert_eq!(parsed, target);

        // The display target stays compatible: `kind:"display"`, no app fields.
        let display = CaptureTarget::display(Some(3));
        let value = serde_json::to_value(&display).expect("serialize");
        assert_eq!(value["kind"], "display");
        assert_eq!(value["displayId"], 3);
        assert!(value.get("bundleId").is_none());
        assert!(value.get("pid").is_none());
        assert_eq!(
            serde_json::from_value::<CaptureTarget>(value).expect("round-trip"),
            display
        );
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

    // --- Mic/Camera pre-flight legs (Story 20.2) ------------------------------

    #[test]
    fn resolve_source_access_maps_the_av_tristate_directly() {
        use ScreenRecordingAccess as A;
        use TccPermission as P;
        // A true tri-state needs no session flag: the mapping is direct.
        assert_eq!(resolve_source_access(P::Granted), A::Granted);
        assert_eq!(resolve_source_access(P::Denied), A::Denied);
        // Not determined ⇒ the OS prompt is still available.
        assert_eq!(resolve_source_access(P::NotDetermined), A::NotYetRequested);
    }

    #[test]
    fn resolve_recording_permission_gates_can_start_over_every_leg_combination() {
        use ScreenRecordingAccess as A;
        // The full matrix: screen × mic leg × camera leg, where each source leg
        // is None (disabled) or one of the three enabled states. can_start iff
        // screen is Granted and every enabled leg is Granted.
        let legs = [
            None,
            Some(A::Granted),
            Some(A::NotYetRequested),
            Some(A::Denied),
        ];
        for screen in [A::Granted, A::NotYetRequested, A::Denied] {
            for microphone in legs {
                for camera in legs {
                    let vm = resolve_recording_permission(screen, microphone, camera);
                    // The legs pass through untouched (the rows render them).
                    assert_eq!(vm.screen_recording, screen);
                    assert_eq!(vm.microphone, microphone);
                    assert_eq!(vm.camera, camera);
                    let expected = screen == A::Granted
                        && microphone.unwrap_or(A::Granted) == A::Granted
                        && camera.unwrap_or(A::Granted) == A::Granted;
                    assert_eq!(
                        vm.can_start, expected,
                        "screen={screen:?} mic={microphone:?} camera={camera:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn resolve_recording_permission_blocker_scenarios_match_the_io_matrix() {
        use ScreenRecordingAccess as A;
        // Both sources disabled: screen alone decides (the 16.5 behavior).
        assert!(resolve_recording_permission(A::Granted, None, None).can_start);
        // An enabled-but-not-granted mic blocks Start (denied or not requested).
        assert!(!resolve_recording_permission(A::Granted, Some(A::Denied), None).can_start);
        assert!(
            !resolve_recording_permission(A::Granted, Some(A::NotYetRequested), None).can_start
        );
        // An enabled-but-denied camera blocks Start even with screen+mic green
        // (the frontend names Camera — the lowest-priority blocker last).
        assert!(
            !resolve_recording_permission(A::Granted, Some(A::Granted), Some(A::Denied)).can_start
        );
        // Screen denied blocks regardless of green source legs (the frontend
        // names Screen Recording first — highest priority).
        assert!(
            !resolve_recording_permission(A::Denied, Some(A::Granted), Some(A::Granted)).can_start
        );
        // All three green: Start unlocks.
        assert!(
            resolve_recording_permission(A::Granted, Some(A::Granted), Some(A::Granted)).can_start
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
        // Real applications with icons (Story 19.1): name/pid/bundleId + an
        // `Option` icon data-URI (`None` when the sidecar reported `null`).
        assert_eq!(vm.applications.len(), 2);
        assert_eq!(vm.applications[0].bundle_id, "com.apple.Safari");
        assert_eq!(vm.applications[0].name, "Safari");
        assert_eq!(vm.applications[0].pid, 501);
        assert_eq!(
            vm.applications[0].icon.as_deref(),
            Some("data:image/png;base64,iVBORw0KGgo=")
        );
        assert_eq!(vm.applications[1].pid, 777);
        assert!(vm.applications[1].icon.is_none());
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
                pixel_width: 3456,
                pixel_height: 2234,
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

        async fn request_microphone(&self) -> Result<TccPermission, CoreError> {
            if !self.available {
                return Err(CoreError::Unsupported(
                    "keeper-rec is not available".to_owned(),
                ));
            }
            // Canned grant — the fake port answers the wire, it never fakes TCC.
            Ok(TccPermission::Granted)
        }

        async fn request_camera(&self) -> Result<TccPermission, CoreError> {
            if !self.available {
                return Err(CoreError::Unsupported(
                    "keeper-rec is not available".to_owned(),
                ));
            }
            // Canned grant — the fake port answers the wire, it never fakes TCC.
            Ok(TccPermission::Granted)
        }
    }

    /// Canned params for the fake port (never touch the filesystem).
    fn test_params() -> SessionParams {
        SessionParams {
            output_path: "/tmp/keeper-test.mov".to_owned(),
            display_id: None,
            application: None,
            system_audio: true,
            microphone: None,
            camera: None,
            segment_mb: 500,
            max_segment_seconds: 1800,
            fps: 30,
            codec: "h264".to_owned(),
            scale_percent: 100,
            audio_only: false,
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
        assert_eq!(
            recorder.request_camera().await.expect("canned tri-state"),
            TccPermission::Granted
        );

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
        assert!(matches!(
            unavailable.request_camera().await,
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
        manifest.record_segment(screen_entry(0, "screen-0000.mov", 123));
        manifest.write().expect("atomic rewrite");
        assert!(
            !folder.join(".manifest.json.tmp").exists(),
            "the sibling temp file must be renamed over manifest.json"
        );
        let raw = std::fs::read_to_string(folder.join("manifest.json")).expect("manifest on disk");
        let parsed: SessionManifest = serde_json::from_str(&raw).expect("always parseable");
        assert_eq!(
            parsed.segments,
            vec![screen_entry(0, "screen-0000.mov", 123)]
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
        std::fs::write(folder.join("screen-0000.mov"), vec![0u8; 10]).expect("segment 0");
        std::fs::write(folder.join("screen-0001.mov"), vec![0u8; 20]).expect("segment 1");
        std::fs::write(folder.join("screen-0002.mov"), vec![0u8; 30]).expect("final segment");
        // Event-fed live view: segment 0 landed with a stale zero size, segment
        // 1's `segmentClosed` was suppressed by a mid-rotation stop (DW-992),
        // and segment 2 is the final segment (never gets a `segmentClosed`).
        manifest.record_segment(screen_entry(0, "screen-0000.mov", 0));
        manifest.reconcile_from_dir().expect("terminal reconcile");
        assert_eq!(
            manifest.segments,
            vec![
                screen_entry(0, "screen-0000.mov", 10), // disk bytes override the stale 0
                screen_entry(1, "screen-0001.mov", 20), // DW-992 backfill
                screen_entry(2, "screen-0002.mov", 30), // final segment included
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
        std::fs::write(folder.join("screen-0000.mov"), vec![0u8; 10]).expect("segment 0");
        std::fs::write(folder.join("screen-0001.mov"), vec![0u8; 20]).expect("segment 1");
        std::fs::write(folder.join("screen-0002.mov"), vec![0u8; 30]).expect("final segment");
        // Event-fed view: bounds present, but segment 0's bytes are stale.
        manifest.record_segment(screen_entry_with_bounds(
            0,
            "screen-0000.mov",
            0,
            1000.0,
            1029.75,
        ));
        manifest.record_segment(screen_entry_with_bounds(
            1,
            "screen-0001.mov",
            20,
            1029.8,
            1059.5,
        ));
        manifest.reconcile_from_dir().expect("terminal reconcile");
        assert_eq!(
            manifest.segments,
            vec![
                // Disk bytes win; the event-fed host-clock bounds survive.
                screen_entry_with_bounds(0, "screen-0000.mov", 10, 1000.0, 1029.75),
                screen_entry_with_bounds(1, "screen-0001.mov", 20, 1029.8, 1059.5),
                // No prior entry → bounds honestly null, never invented.
                screen_entry(2, "screen-0002.mov", 30),
            ]
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn segment_entry_serializes_missing_bounds_as_null_and_reads_old_manifests() {
        // Missing bounds are persisted as explicit `null` (the spec's "recorded
        // as null" — no skip_serializing_if), and a pre-17.4 manifest without
        // the fields still deserializes (tolerant recovery paths).
        let json = serde_json::to_value(screen_entry(0, "screen-0000.mov", 10)).expect("serialize");
        assert!(json["ptsStart"].is_null(), "absent ptsStart must be null");
        assert!(json["ptsEnd"].is_null(), "absent ptsEnd must be null");
        let json = serde_json::to_value(screen_entry_with_bounds(
            1,
            "screen-0001.mov",
            20,
            1000.0,
            1029.75,
        ))
        .expect("serialize");
        assert_eq!(json["ptsStart"], 1000.0);
        assert_eq!(json["ptsEnd"], 1029.75);
        // Pre-17.4 wire shape (no bounds fields at all) → None, never an error.
        let old: SegmentEntry = serde_json::from_str(
            r#"{"index":3,"file":"screen-0003.mov","bytes":7,"track":"screen"}"#,
        )
        .expect("pre-17.4 entry deserializes");
        assert_eq!(old, screen_entry(3, "screen-0003.mov", 7));
    }

    #[test]
    fn reconcile_ingests_both_tracks_and_skips_strays_deterministically() {
        let folder = fresh_temp_dir("strays");
        let mut manifest = test_manifest(folder.clone());
        std::fs::write(folder.join("screen-0002.mov"), vec![0u8; 5]).expect("segment 2");
        std::fs::write(folder.join("screen-0000.mov"), vec![0u8; 4]).expect("segment 0");
        // Story 20.1 (FR-73): a camera segment is this session's own track
        // now — ingested with `track:"camera"`, never treated as a stray.
        std::fs::write(folder.join("camera-0000.mov"), vec![0u8; 6]).expect("camera segment 0");
        // Strays that must NOT enter the authoritative ledger:
        std::fs::write(folder.join("extra-0001.mov"), vec![0u8; 6]).expect("wrong prefix");
        std::fs::write(folder.join("notes.mov"), vec![0u8; 7]).expect("no numeric run");
        std::fs::write(folder.join("cover.png"), vec![0u8; 8]).expect("non-mp4");
        std::fs::create_dir(folder.join("screen-0009.mov")).expect("dir masquerading as segment");
        manifest
            .reconcile_from_dir()
            .expect("strays are skipped, never aborting");
        let entries: Vec<(&str, &str)> = manifest
            .segments
            .iter()
            .map(|s| (s.file.as_str(), s.track.as_str()))
            .collect();
        assert_eq!(
            entries,
            vec![
                // (index, file) sort: camera-0000 < screen-0000 at index 0.
                ("camera-0000.mov", "camera"),
                ("screen-0000.mov", "screen"),
                ("screen-0002.mov", "screen"),
            ],
            "screen-####.mov and camera-####.mov enter the ledger disambiguated by track, \
             sorted by (index, file); wrong-prefix / no-run / non-mp4 / directory entries \
             are skipped"
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// Shorthand for a camera-track ledger entry (Story 20.1).
    fn camera_entry(index: u32, file: &str, bytes: u64) -> SegmentEntry {
        SegmentEntry {
            track: "camera".to_owned(),
            ..screen_entry(index, file, bytes)
        }
    }

    #[test]
    fn reconcile_dual_track_keeps_bounds_per_track_index_without_clobber() {
        // Story 20.1 (FR-73), the one real core hazard: `screen-0000` and
        // `camera-0000` share index 0 by design, so the reconcile snapshot
        // must key the capture-time PTS bounds on the (track, index) PAIR —
        // an index-only key would let one track's bounds clobber the other's.
        let folder = fresh_temp_dir("dual-track");
        let mut manifest = test_manifest(folder.clone());
        std::fs::write(folder.join("screen-0000.mov"), vec![0u8; 10]).expect("screen 0");
        std::fs::write(folder.join("camera-0000.mov"), vec![0u8; 20]).expect("camera 0");
        std::fs::write(folder.join("screen-0001.mov"), vec![0u8; 30]).expect("screen final");
        std::fs::write(folder.join("camera-0001.mov"), vec![0u8; 40]).expect("camera final");
        // Event-fed view: DIFFERENT bounds per track at the same index (the
        // camera anchors a hair after the screen — the real capture shape).
        manifest.record_segment(screen_entry_with_bounds(
            0,
            "screen-0000.mov",
            10,
            1000.0,
            1029.75,
        ));
        manifest.record_segment(SegmentEntry {
            pts_start: Some(1000.02),
            pts_end: Some(1029.77),
            ..camera_entry(0, "camera-0000.mov", 20)
        });
        manifest.reconcile_from_dir().expect("terminal reconcile");
        assert_eq!(
            manifest.segments,
            vec![
                // (index, file) order: camera before screen at each index.
                SegmentEntry {
                    pts_start: Some(1000.02),
                    pts_end: Some(1029.77),
                    ..camera_entry(0, "camera-0000.mov", 20)
                },
                screen_entry_with_bounds(0, "screen-0000.mov", 10, 1000.0, 1029.75),
                // Final segments never got a segmentClosed → honest null
                // bounds per track, no cross-track inheritance.
                camera_entry(1, "camera-0001.mov", 40),
                screen_entry(1, "screen-0001.mov", 30),
            ],
            "both tracks ingest into ONE ledger, bounds preserved per (track, index)"
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn reconcile_camera_only_bounds_never_leak_onto_screen() {
        // The clobber regression in its sharpest form: only the CAMERA entry
        // at index 0 carries event-fed bounds. The rebuilt screen-0000 entry
        // must stay honestly null — inheriting the camera's bounds would
        // fabricate a host-clock claim the screen never made.
        let folder = fresh_temp_dir("no-cross-leak");
        let mut manifest = test_manifest(folder.clone());
        std::fs::write(folder.join("screen-0000.mov"), vec![0u8; 10]).expect("screen 0");
        std::fs::write(folder.join("camera-0000.mov"), vec![0u8; 20]).expect("camera 0");
        manifest.record_segment(SegmentEntry {
            pts_start: Some(2000.0),
            pts_end: Some(2030.0),
            ..camera_entry(0, "camera-0000.mov", 20)
        });
        manifest.reconcile_from_dir().expect("terminal reconcile");
        assert_eq!(
            manifest.segments,
            vec![
                SegmentEntry {
                    pts_start: Some(2000.0),
                    pts_end: Some(2030.0),
                    ..camera_entry(0, "camera-0000.mov", 20)
                },
                screen_entry(0, "screen-0000.mov", 10),
            ],
            "bounds keyed on (track, index): the screen twin stays null"
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    // --- completion/recovery summary accessors (Story 20.3) -----------------

    #[test]
    fn screen_segment_count_counts_one_for_a_single_segment_session() {
        // The one case the live `segments_closed` rotation counter gets wrong
        // (it reports 0 — no rotation ever closed): a single-segment session
        // has exactly one screen segment on disk and the card must say "1".
        let folder = fresh_temp_dir("summary-single");
        let mut manifest = test_manifest(folder.clone());
        manifest.record_segment(screen_entry(0, "screen-0000.mov", 1_000));
        assert_eq!(manifest.screen_segment_count(), 1);
        assert_eq!(manifest.total_bytes(), 1_000);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn summary_counts_only_screen_segments_but_sums_bytes_across_tracks() {
        // A screen+camera session (Story 20.1): two screen segments and two
        // camera segments. "N segments" counts the screen track only (2), yet
        // "{size}" sums the on-disk bytes of every segment, both tracks.
        let folder = fresh_temp_dir("summary-dual");
        let mut manifest = test_manifest(folder.clone());
        manifest.record_segment(screen_entry(0, "screen-0000.mov", 10));
        manifest.record_segment(camera_entry(0, "camera-0000.mov", 20));
        manifest.record_segment(screen_entry(1, "screen-0001.mov", 30));
        manifest.record_segment(camera_entry(1, "camera-0001.mov", 40));
        assert_eq!(
            manifest.screen_segment_count(),
            2,
            "camera segments excluded"
        );
        assert_eq!(
            manifest.total_bytes(),
            100,
            "bytes summed across both tracks"
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    // --- live session bytes (Story 18.1) ------------------------------------

    #[test]
    fn session_bytes_sums_this_sessions_segments_only() {
        let folder = fresh_temp_dir("bytes");
        std::fs::create_dir_all(&folder).expect("session folder");
        std::fs::write(folder.join("screen-0000.mov"), vec![0u8; 10]).expect("segment 0");
        std::fs::write(folder.join("screen-0001.mov"), vec![0u8; 20]).expect("segment 1");
        // Foreign files that must NOT count (same ownership rule as reconcile):
        std::fs::write(folder.join("manifest.json"), b"{}").expect("manifest");
        std::fs::write(folder.join("camera-0000.mov"), vec![0u8; 40]).expect("future track prefix");
        std::fs::write(folder.join("notes.mov"), vec![0u8; 50]).expect("no numeric run");
        std::fs::create_dir(folder.join("screen-0009.mov")).expect("dir masquerading as segment");
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
        std::fs::write(folder.join("screen-0000.mov"), vec![0u8; 10]).expect("first flush");
        assert_eq!(session_bytes_on_disk(&folder), 10);
        std::fs::write(folder.join("screen-0000.mov"), vec![0u8; 25]).expect("more bytes flushed");
        assert_eq!(session_bytes_on_disk(&folder), 25);
        let _ = std::fs::remove_dir_all(&folder);
    }

    // --- current-segment bytes (Story 18.3) ---------------------------------

    #[test]
    fn current_segment_bytes_is_zero_for_no_segments() {
        // Missing folder, then an empty one, then one holding only foreign files
        // — every case yields 0 (no `screen-####.mov` to measure).
        let folder = fresh_temp_dir("current-none");
        assert_eq!(
            current_segment_bytes_on_disk(&folder),
            0,
            "missing folder is 0"
        );
        std::fs::create_dir_all(&folder).expect("session folder");
        assert_eq!(
            current_segment_bytes_on_disk(&folder),
            0,
            "empty folder is 0"
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn current_segment_bytes_reports_a_single_growing_segment() {
        let folder = fresh_temp_dir("current-growing");
        std::fs::create_dir_all(&folder).expect("session folder");
        std::fs::write(folder.join("screen-0000.mov"), vec![0u8; 123]).expect("segment 0");
        assert_eq!(current_segment_bytes_on_disk(&folder), 123);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn current_segment_bytes_reads_the_highest_index_after_rotation() {
        // Post-rotation: a full `screen-0000.mov` plus a fresh `screen-0001.mov`
        // — the meter tracks the highest index (the open segment), not the total.
        let folder = fresh_temp_dir("current-rotated");
        std::fs::create_dir_all(&folder).expect("session folder");
        std::fs::write(folder.join("screen-0000.mov"), vec![0u8; 500]).expect("closed segment");
        std::fs::write(folder.join("screen-0001.mov"), vec![0u8; 40]).expect("open segment");
        assert_eq!(current_segment_bytes_on_disk(&folder), 40);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn current_segment_bytes_ignores_foreign_files() {
        // Same ownership rule as `session_bytes_on_disk`: only `screen-####.mov`
        // is measured; a `camera-####.mov` (higher trailing run), a `manifest.json`,
        // a no-run `.mov`, and a directory masquerading as a segment are ignored.
        let folder = fresh_temp_dir("current-foreign");
        std::fs::create_dir_all(&folder).expect("session folder");
        std::fs::write(folder.join("screen-0000.mov"), vec![0u8; 77]).expect("real segment");
        std::fs::write(folder.join("camera-0009.mov"), vec![0u8; 40]).expect("foreign track");
        std::fs::write(folder.join("manifest.json"), b"{}").expect("manifest");
        std::fs::write(folder.join("notes.mov"), vec![0u8; 50]).expect("no numeric run");
        std::fs::create_dir(folder.join("screen-0009.mov")).expect("dir masquerading as segment");
        assert_eq!(current_segment_bytes_on_disk(&folder), 77);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[cfg(unix)]
    #[test]
    fn reconcile_skips_an_unreadable_entry_without_aborting() {
        let folder = fresh_temp_dir("unreadable");
        let mut manifest = test_manifest(folder.clone());
        std::fs::write(folder.join("screen-0000.mov"), vec![0u8; 9]).expect("healthy segment");
        // A dangling symlink: `fs::metadata` (which follows links) fails on it,
        // so the entry must be skipped — one bad entry never fails the
        // terminal write.
        std::os::unix::fs::symlink(folder.join("gone.mov"), folder.join("screen-0001.mov"))
            .expect("dangling symlink");
        manifest
            .reconcile_from_dir()
            .expect("one unreadable entry must not abort the reconcile");
        assert_eq!(
            manifest.segments,
            vec![screen_entry(0, "screen-0000.mov", 9)]
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
            std::fs::write(folder.join("screen-0000.mov"), vec![0u8; 11]).expect("segment 0");
            std::fs::write(folder.join("screen-0001.mov"), vec![0u8; 22]).expect("final segment");
            manifest.reconcile_from_dir().expect("terminal reconcile");
            manifest.set_status(ManifestStatus::from_state(state));
            manifest.write().expect("terminal write");
            let raw =
                std::fs::read_to_string(folder.join("manifest.json")).expect("manifest on disk");
            let value: serde_json::Value = serde_json::from_str(&raw).expect("parseable manifest");
            assert_eq!(value["status"], wire, "terminal {wire} persists its status");
            let segments = value["segments"].as_array().expect("segments array");
            assert_eq!(segments.len(), 2, "terminal {wire} lists every segment");
            assert_eq!(segments[0]["file"], "screen-0000.mov");
            assert_eq!(segments[0]["bytes"], 11);
            assert_eq!(segments[1]["file"], "screen-0001.mov");
            assert_eq!(segments[1]["bytes"], 22);
            let _ = std::fs::remove_dir_all(&folder);
        }
    }

    // --- startup recovery of orphaned segments (Story 17.3) ----------------

    /// An `is_active` predicate reporting every folder inactive — the shape of
    /// a scan with no live recording session (the common case).
    fn no_folder_is_active(_folder: &std::path::Path) -> bool {
        false
    }

    /// Build a session folder holding a manifest at `status` plus
    /// `segment_count` dummy `screen-####.mov` files (each with a distinct,
    /// non-zero byte length so byte-identity is observable), and return the
    /// folder. The manifest's event-fed segment list deliberately misses the
    /// FINAL segment (which emits no `segmentClosed`) and carries a stale byte
    /// figure for the ones it has — exercising recovery's disk-authoritative
    /// rebuild (the final segment surfaces; sizes are repaired).
    fn stale_session(
        base: &std::path::Path,
        name: &str,
        status: ManifestStatus,
        segment_count: u32,
    ) -> std::path::PathBuf {
        let folder = base.join(name);
        std::fs::create_dir_all(&folder).expect("session folder");
        for index in 0..segment_count {
            // Distinct, non-zero lengths (10, 20, 30, …) so a stray remux or a
            // stale event-fed size is caught by the byte assertions.
            let bytes = vec![index as u8; ((index + 1) * 10) as usize];
            std::fs::write(folder.join(format!("screen-{index:04}.mov")), bytes)
                .expect("dummy segment");
        }
        let manifest = SessionManifest {
            version: MANIFEST_VERSION,
            session: name.to_owned(),
            status,
            capture_target: CaptureTarget::display(None),
            devices: test_devices(),
            // The event-fed view a crash freezes: every segment but the last
            // (no `segmentClosed` for the final one), each with a stale
            // 1-byte size a disk rebuild must repair.
            segments: (0..segment_count.saturating_sub(1))
                .map(|index| screen_entry(index, &format!("screen-{index:04}.mov"), 1))
                .collect(),
            meta: None,
            started_at: None,
            ended_at: None,
            folder: folder.clone(),
        };
        manifest.write().expect("write stale manifest");
        folder
    }

    #[test]
    fn load_round_trips_a_written_manifest_and_rebinds_the_folder() {
        let folder = fresh_temp_dir("load");
        let mut original = test_manifest(folder.clone());
        original.record_segment(screen_entry(0, "screen-0000.mov", 42));
        original.write().expect("write manifest");

        let loaded = SessionManifest::load(&folder).expect("load round-trip");
        assert_eq!(loaded.version, original.version);
        assert_eq!(loaded.session, original.session);
        assert_eq!(loaded.status, ManifestStatus::Recording);
        assert_eq!(loaded.capture_target, original.capture_target);
        assert_eq!(loaded.devices, original.devices);
        assert_eq!(loaded.segments, original.segments);
        // The runtime-only folder field is re-bound from the argument, not the
        // (never-persisted) serialized value.
        assert_eq!(loaded.folder(), folder.as_path());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn load_surfaces_a_secret_free_error_on_a_malformed_manifest() {
        let folder = fresh_temp_dir("load-bad");
        std::fs::create_dir_all(&folder).expect("folder");
        std::fs::write(folder.join("manifest.json"), b"{ not json").expect("bad manifest");
        let err = SessionManifest::load(&folder).expect_err("malformed manifest must error");
        match err {
            RecordingError::ManifestIo(message) => assert!(
                !message.contains(folder.to_string_lossy().as_ref()),
                "the error must not embed the folder path, got: {message}"
            ),
            other => panic!("expected ManifestIo, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn recover_marks_a_stale_recording_session_and_rebuilds_the_ledger() {
        let base = fresh_temp_dir("recover-base");
        // A stale `recording` session with three on-disk segments; the ledger
        // misses the final one (no `segmentClosed`) and carries stale sizes.
        let folder = stale_session(
            &base,
            "keeper-rec 2026-07-17 14.00.00",
            ManifestStatus::Recording,
            3,
        );

        let recovered = recover_orphaned_sessions(&base, &no_folder_is_active);
        assert_eq!(recovered, vec![folder.clone()]);

        // The manifest is now `recovered`, with the ledger rebuilt from disk:
        // the final/suppressed segment surfaced, real byte sizes, sorted by
        // (index, file).
        let reloaded = SessionManifest::load(&folder).expect("reload recovered manifest");
        assert_eq!(reloaded.status, ManifestStatus::Recovered);
        assert_eq!(
            reloaded.segments,
            vec![
                screen_entry(0, "screen-0000.mov", 10),
                screen_entry(1, "screen-0001.mov", 20),
                screen_entry(2, "screen-0002.mov", 30),
            ]
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn recover_handles_a_session_that_crashed_before_the_first_segment() {
        let base = fresh_temp_dir("recover-empty");
        let folder = stale_session(
            &base,
            "keeper-rec no-segments",
            ManifestStatus::Recording,
            0,
        );

        let recovered = recover_orphaned_sessions(&base, &no_folder_is_active);
        assert_eq!(recovered, vec![folder.clone()]);
        let reloaded = SessionManifest::load(&folder).expect("reload");
        assert_eq!(reloaded.status, ManifestStatus::Recovered);
        assert!(
            reloaded.segments.is_empty(),
            "no segments on disk ⇒ an honest empty ledger"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn recover_never_remuxes_segment_files_are_byte_identical() {
        let base = fresh_temp_dir("recover-noremux");
        let folder = stale_session(&base, "keeper-rec noremux", ManifestStatus::Recording, 3);
        // Capture every segment file's bytes before recovery.
        let before: Vec<(std::path::PathBuf, Vec<u8>)> = (0..3)
            .map(|index| {
                let path = folder.join(format!("screen-{index:04}.mov"));
                let bytes = std::fs::read(&path).expect("read segment before");
                (path, bytes)
            })
            .collect();

        recover_orphaned_sessions(&base, &no_folder_is_active);

        // Every segment file is byte-for-byte unchanged — only `manifest.json`
        // may have been rewritten.
        for (path, expected) in &before {
            let after = std::fs::read(path).expect("read segment after");
            assert_eq!(
                &after, expected,
                "segment {path:?} must be byte-identical after recovery (no remux)"
            );
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn recover_leaves_terminal_sessions_byte_untouched_and_idempotent() {
        for status in [
            ManifestStatus::Finalized,
            ManifestStatus::Recovered,
            ManifestStatus::Failed,
        ] {
            let base = fresh_temp_dir(&format!("recover-terminal-{status:?}"));
            let folder = stale_session(&base, "keeper-rec terminal", status, 2);
            let before =
                std::fs::read(folder.join("manifest.json")).expect("manifest bytes before");

            let recovered = recover_orphaned_sessions(&base, &no_folder_is_active);
            assert!(
                recovered.is_empty(),
                "a {status:?} session must not be recovered"
            );
            // The terminal manifest is left byte-for-byte as-is: no status
            // flip, no reconcile, no rewrite.
            let after = std::fs::read(folder.join("manifest.json")).expect("manifest bytes after");
            assert_eq!(
                after, before,
                "a {status:?} manifest must be byte-untouched"
            );
            let _ = std::fs::remove_dir_all(&base);
        }
    }

    #[test]
    fn recover_returns_empty_for_a_missing_base_dir() {
        let base = fresh_temp_dir("recover-missing");
        // `base` is a fresh path that does not exist.
        assert!(!base.exists());
        let recovered = recover_orphaned_sessions(&base, &no_folder_is_active);
        assert!(recovered.is_empty(), "a missing base dir is a silent no-op");
    }

    #[cfg(unix)]
    #[test]
    fn recover_returns_empty_for_an_unreadable_base_dir() {
        use std::os::unix::fs::PermissionsExt;
        let base = fresh_temp_dir("recover-unreadable-base");
        stale_session(&base, "keeper-rec hidden", ManifestStatus::Recording, 1);
        // Revoke every permission on the base so `read_dir` itself fails.
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o000))
            .expect("chmod base 000");

        let recovered = recover_orphaned_sessions(&base, &no_folder_is_active);
        assert!(
            recovered.is_empty(),
            "an unreadable base dir must warn and yield an empty list"
        );

        // Restore so cleanup (and the leftover-dir sweep) can remove the tree.
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o755))
            .expect("chmod base 755");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn recover_skips_a_malformed_manifest_without_aborting_a_sibling() {
        let base = fresh_temp_dir("recover-malformed");
        // A folder with a malformed manifest, and a sibling valid stale session.
        let bad = base.join("keeper-rec broken");
        std::fs::create_dir_all(&bad).expect("bad folder");
        std::fs::write(bad.join("manifest.json"), b"{ not json at all").expect("bad manifest");
        let good = stale_session(&base, "keeper-rec good", ManifestStatus::Recording, 2);

        let recovered = recover_orphaned_sessions(&base, &no_folder_is_active);
        assert_eq!(
            recovered,
            vec![good.clone()],
            "the malformed sibling must be skipped"
        );
        // The good session was recovered; the corrupt manifest is untouched.
        assert_eq!(
            SessionManifest::load(&good).expect("reload good").status,
            ManifestStatus::Recovered
        );
        assert_eq!(
            std::fs::read(bad.join("manifest.json")).expect("corrupt manifest bytes"),
            b"{ not json at all",
            "a corrupt manifest must be left byte-untouched"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn recover_skips_stray_non_session_entries() {
        let base = fresh_temp_dir("recover-strays");
        std::fs::create_dir_all(&base).expect("base");
        // A loose file (not a directory).
        std::fs::write(base.join("loose.txt"), b"stray").expect("loose file");
        // A subdirectory without a manifest.
        std::fs::create_dir(base.join("no-manifest")).expect("empty subdir");
        // A valid stale session that must still be recovered.
        let good = stale_session(&base, "keeper-rec real", ManifestStatus::Recording, 1);

        let recovered = recover_orphaned_sessions(&base, &no_folder_is_active);
        assert_eq!(recovered, vec![good], "only the real session is recovered");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn recover_skips_a_newer_schema_version() {
        let base = fresh_temp_dir("recover-newer");
        let folder = base.join("keeper-rec future");
        std::fs::create_dir_all(&folder).expect("folder");
        std::fs::write(folder.join("screen-0000.mov"), vec![0u8; 10]).expect("segment");
        // A `recording` manifest whose schema version is newer than we understand.
        let manifest = SessionManifest {
            version: MANIFEST_VERSION + 1,
            session: "keeper-rec future".to_owned(),
            status: ManifestStatus::Recording,
            capture_target: CaptureTarget::display(None),
            devices: test_devices(),
            segments: Vec::new(),
            meta: None,
            started_at: None,
            ended_at: None,
            folder: folder.clone(),
        };
        manifest.write().expect("write newer manifest");

        let recovered = recover_orphaned_sessions(&base, &no_folder_is_active);
        assert!(
            recovered.is_empty(),
            "a newer-schema manifest must never be rewritten"
        );
        // Its status is left untouched.
        let reloaded = SessionManifest::load(&folder).expect("reload");
        assert_eq!(reloaded.status, ManifestStatus::Recording);
        assert_eq!(reloaded.version, MANIFEST_VERSION + 1);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn recover_is_idempotent_a_second_run_is_a_no_op() {
        let base = fresh_temp_dir("recover-idempotent");
        stale_session(&base, "keeper-rec once", ManifestStatus::Recording, 2);

        let first = recover_orphaned_sessions(&base, &no_folder_is_active);
        assert_eq!(first.len(), 1, "the first run recovers the stale session");
        let second = recover_orphaned_sessions(&base, &no_folder_is_active);
        assert!(
            second.is_empty(),
            "the second run finds no `recording` manifest — a no-op"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn recover_salvages_multiple_orphans_in_deterministic_folder_order() {
        let base = fresh_temp_dir("recover-order");
        // Create sessions out of lexicographic order; recovery must salvage
        // every one and return them folder-sorted regardless of `read_dir`
        // order.
        let noon = stale_session(
            &base,
            "keeper-rec 2026-07-17 12.00.00",
            ManifestStatus::Recording,
            1,
        );
        let morning = stale_session(
            &base,
            "keeper-rec 2026-07-17 09.00.00",
            ManifestStatus::Recording,
            1,
        );
        let evening = stale_session(
            &base,
            "keeper-rec 2026-07-17 15.00.00",
            ManifestStatus::Recording,
            1,
        );

        let recovered = recover_orphaned_sessions(&base, &no_folder_is_active);
        assert_eq!(recovered, vec![morning, noon, evening]);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn recover_isolates_a_per_folder_write_failure() {
        use std::os::unix::fs::PermissionsExt;
        let base = fresh_temp_dir("recover-write-fail");
        // Two stale sessions; `wedged`'s folder is read-only so recovery's
        // atomic `write` (which creates `.manifest.json.tmp` inside it) fails.
        let wedged = stale_session(&base, "keeper-rec a-wedged", ManifestStatus::Recording, 1);
        let good = stale_session(&base, "keeper-rec b-good", ManifestStatus::Recording, 1);
        std::fs::set_permissions(&wedged, std::fs::Permissions::from_mode(0o555))
            .expect("chmod wedged 555");

        let recovered = recover_orphaned_sessions(&base, &no_folder_is_active);
        assert_eq!(
            recovered,
            vec![good.clone()],
            "the wedged folder's write failure must not stop its sibling"
        );
        assert_eq!(
            SessionManifest::load(&good).expect("reload good").status,
            ManifestStatus::Recovered
        );
        // The wedged folder's manifest still reads `recording` (the write
        // never landed) — the next pass will retry it.
        assert_eq!(
            SessionManifest::load(&wedged)
                .expect("reload wedged")
                .status,
            ManifestStatus::Recording
        );

        std::fs::set_permissions(&wedged, std::fs::Permissions::from_mode(0o755))
            .expect("chmod wedged 755");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn recover_leaves_a_reserved_live_folder_byte_untouched_and_unreturned() {
        let base = fresh_temp_dir("recover-live-guard");
        // Two `recording` sessions: `live` stands in for a session the shell
        // has reserved (live, mid-start, or still landing its terminal write);
        // `orphan` is a genuine prior crash.
        let live = stale_session(&base, "keeper-rec live", ManifestStatus::Recording, 2);
        let orphan = stale_session(&base, "keeper-rec orphan", ManifestStatus::Recording, 1);
        let live_before = std::fs::read(live.join("manifest.json")).expect("live bytes before");

        let live_guard = live.clone();
        let is_active = move |folder: &std::path::Path| folder == live_guard;
        let recovered = recover_orphaned_sessions(&base, &is_active);

        // Only the orphan is recovered; the reserved live folder is never
        // reconciled, never rewritten, never returned.
        assert_eq!(
            recovered,
            vec![orphan],
            "the reserved live folder must not be recovered"
        );
        let live_after = std::fs::read(live.join("manifest.json")).expect("live bytes after");
        assert_eq!(
            live_after, live_before,
            "the reserved live folder's manifest must be byte-for-byte untouched"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn recover_skips_a_symlinked_base_dir_entry() {
        let base = fresh_temp_dir("recover-symlink");
        std::fs::create_dir_all(&base).expect("base");
        // A real stale `recording` session living OUTSIDE the base dir,
        // reachable only through a symlink inside it.
        let outside = fresh_temp_dir("recover-symlink-target");
        std::fs::create_dir_all(&outside).expect("outside base");
        let target = stale_session(&outside, "keeper-rec outside", ManifestStatus::Recording, 1);
        std::os::unix::fs::symlink(&target, base.join("keeper-rec linked"))
            .expect("symlink into base");

        let recovered = recover_orphaned_sessions(&base, &no_folder_is_active);
        assert!(
            recovered.is_empty(),
            "a symlinked entry must be skipped, never followed"
        );
        // The out-of-tree session was never touched.
        assert_eq!(
            SessionManifest::load(&target)
                .expect("reload target")
                .status,
            ManifestStatus::Recording,
            "recovery must never rewrite a manifest outside the destination tree"
        );
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&outside);
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
