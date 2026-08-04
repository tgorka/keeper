# Epic 22 Context: Recording Ergonomics II — Precision, Metadata Depth & Debuggability

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

A second owner-requested ergonomics increment on the already-shipped macOS
recording stack: finer capture precision (a quarter-scale option plus a live
hint of the resulting output resolution), a source list that refreshes without
visibly reflowing, richer session metadata in the manifest (tags and arbitrary
name/value fields), the microphone as its own track inside the webcam file,
an opt-in on-disk debug log, file-based configuration overrides for
hand-edited or version-controlled setups, and — the largest item — acoustic
echo cancellation on the microphone feed so that recording without headphones
no longer doubles speaker output back onto the recording. Everything builds
strictly on the shipped capture core; nothing here changes the capture
contract's shape.

## Stories

- Story 22.1: Quarter scale and live effective-resolution hint
- Story 22.2: Flicker-free source refresh indicator
- Story 22.3: Session tags and custom metadata fields
- Story 22.4: Microphone track in the webcam file
- Story 22.5: Debug mode — event and error logs on disk
- Story 22.6: File-based config overrides
- Story 22.7: The microphone stops recording the speakers (echo cancellation)

## Requirements & Constraints

- **Every wire change is additive on the existing sidecar protocol version.**
  An absent field must reproduce today's behaviour exactly — with one
  deliberate, documented exception: echo cancellation's absent-default is
  *on*, so an older host cannot silently disable it.
- **Audio tracks are never premixed.** System audio and microphone stay
  separate AAC tracks, in the screen file and now in the camera file too.
- **Zero egress.** Metadata, logs and config are local-only, secret-free, and
  named in the user-facing recording docs. No upload, no telemetry.
- **Loud failure, never silent degradation.** Every fallback (echo
  cancellation unavailable, malformed config file, device change mid-session)
  emits a warning event and surfaces on the existing sticky-warning surface;
  no path may produce a failed or silently wrong recording.
- Persisted knobs live in the Rust settings registry and apply to the *next*
  session, matching the established "Applies to the next Recording Session"
  idiom. Echo cancellation is persisted rather than per-session (it describes
  the user's room and hardware) and is settable only while no session is live.
- Debug logging is off by default; when on, it writes per-session event logs
  into the session folder and mirrors app-level tracing to the app log.
- Honest costs go in the docs, not hidden: echo cancellation forces a mono mic
  track, applies non-defeatable voice-band noise suppression, requires the
  device's native sample rate, and cannot run on an aggregate input device.
- Explicitly out of scope with recorded verdicts, not to be relitigated: AV1
  encode (no encoder on Apple silicon through M4), per-output-device audio
  capture (would need Core Audio taps), hiding the system capture indicator
  (system-owned by design).
- The echo-cancellation *measurement* (far-end attenuation in the mic track,
  on speakers, against a same-session baseline) is human-in-the-loop: it needs
  a physical Mac, real permission grants and a developer-signed build. If the
  measured reduction is absent, the correct outcome is to default the switch
  off and file the finding — not to ship an inert switch.

## Technical Decisions

- **The platform split is load-bearing.** The recording core stays
  platform-free (session state machine, manifest, segment ledger, folder
  validation, recovery) and never holds an Apple API or a process handle; all
  capture lives in the Swift sidecar behind the shell's recorder port. New
  capture behaviour belongs in the sidecar; new persisted state and manifest
  shape belong in the core.
- **Transport** is one JSON object per line over the sidecar's stdio:
  id-correlated commands host→sidecar, unsolicited state/segment/error/warning
  events sidecar→host. The contract *shape* is the invariant; field lists are
  code-owned and extended additively.
- **Container is fragmented QuickTime `.mov`, not `.mp4`** — a hard-won
  platform fact; keep it on every new path. The idle video heartbeat, the PTS
  monotonic guard, host-clock PTS, and the gapless size-based rotation are all
  preserved invariants; anything scaled must re-append the *scaled* frame.
- The camera file is a separate synchronised writer rotated at the same
  boundaries as the screen file, anchored to the same host clock.
- **Echo cancellation is a third microphone producer** behind the existing
  mic-enabled branch, feeding the single sample-append seam every mic leg
  already uses — the silence-fill, PTS lower bound, camera mirror and rotation
  split stay untouched. The precedent is the existing out-of-stream mic
  session. When on, the mic does *not* ride the capture stream: the system's
  voice-processing audio unit runs instead, with its reference bound to the
  default output device and automatic gain control disabled (AEC and noise
  suppression yes, AGC no). Sample timestamps come from the same host clock as
  every other track, so no clock conversion and no drift. The unit binds its
  reference at init and is **never re-initialised mid-session**: an output
  device change emits a warning instead, because a re-init would break the PTS
  monotonicity the rotation and silence-fill depend on.
- **Config overrides** live in a `config.json` beside the app database in the
  data directory, read once at startup and imported over the settings table
  (file wins). Malformed files are reported loudly and skipped.
- **Manifest growth is additive and tolerant:** recovery and reconciliation
  must handle older manifests that lack the newer metadata fields entirely.
- Every recording surface renders only behind the recording capability flag;
  the frontend never sniffs platform or user agent.

## UX & Interaction Patterns

- The Recording view is a single centred stack of cards (next-session meta,
  source, audio, webcam, destination, segmenting, collapsed advanced) that
  flips in place between setup, active and completion/recovery. Metadata
  fields sit in the next-session card, clear after Start, and offer the last
  values as quick re-fill.
- Periodic background refreshes must not shift layout — an inline indicator
  beside a heading, never a line of text that appears and disappears.
- The recording-red token appears only on the record dot, the active banner
  edge, the tray badge and the loud error banner; never on buttons, text or
  decoration.
- Persistent conditions get sticky, non-dismissible banners, never toasts;
  warnings and errors are the loud-failure surface with a restart affordance.
- New pre-recording switches sit beside their peers (echo cancellation next to
  the microphone switch), are disabled while a session is live, and state
  plainly that they apply to the next session.
- Recording voice: sentence case, no exclamation marks, honest local-only
  framing, and glossary capitalisation for "Recording Session" and "segment".

## Cross-Story Dependencies

- The whole epic depends on the shipped recording epics and the first
  ergonomics increment; nothing here introduces a new subsystem.
- 22.1 extends the existing scale set and shares the Advanced card layout;
  22.3 extends the existing manifest metadata object.
- 22.4 (mic track in the camera file) and 22.7 (echo cancellation) share the
  microphone append seam: whichever lands second must keep the other intact,
  and the processed audio must reach the camera file identically.
- 22.7 also depends on the existing out-of-stream mic path, the mic hot-unplug
  warning/silence-fill/fallback behaviour, and the sticky-warning surface.
- 22.5 and 22.6 are independent of each other but both touch the settings
  registry that 22.7's persisted switch uses.
- Two known open defects in this code area — the false "no frames" outcome
  when stopping during a rotation, and display-sleep teardown — must not be
  regressed; fixing them is fair game for whichever story touches that code.
