---
title: 'Microphone Hot-Unplug Resilience'
type: 'feature'
created: '2026-07-19'
baseline_revision: '1c47444044823dba4d18902138f5b4c05c8fe696'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
final_revision: 'b6e29d1'
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-19-context.md'
warnings: [oversized]
---

<intent-contract>

## Intent

**Problem:** Unplugging the mic mid-recording currently has no graceful path: the sidecar has no device-change observers, no non-fatal event shape (every `{"event":"error"}` is paired with `exit(0)`), no silence-fill or fallback, and the core `SessionState`/`RecordingStatusVm`/tray/banner carry no `warning` concept at all (18.4's warning surface is still backlog). A bumped cable would either be silently swallowed (mic track just gaps, no signal) or kill the whole session. On the setup surface a mic unplugged before Start leaves a stale device id that ships to the sidecar.

**Approach:** Introduce a generic, non-fatal, sticky warning slot that threads sidecar → core → view model → tray + banner, and drive it from a mic-loss path that never aborts: on a device-removal signal the sidecar emits `{"event":"warning",...}` (not `error`), keeps video + system audio rolling, silence-fills the mic track, and attempts fallback to the system default input. Validate never-abort via a simulated device-removal RPC + a pure Swift decision function + a Rust state-machine test (real silence/fallback/A-V-sync on hardware is Story 20.6). On the setup surface, reconcile a vanished mic selection back to System default input.

## Boundaries & Constraints

**Always:**
- Mic loss is **non-fatal**: the session state stays `recording`/`rotating`, segments keep closing, video + system-audio tracks are unaffected, and the session still reaches `finalized` on Stop. `dependency_firewall_holds` must stay green — no `tauri`/Apple/process tokens in `keeper-core`.
- The warning is **persistent & non-dismissible**: once raised it survives for the rest of the session (last-write-wins message, never auto-clears back to `None`); it resets only when a new session starts. It is raised on **both** the tray status surface and the in-app banner (amber variant).
- Warning is **additive & tolerant** on the wire: a new `{"event":"warning","code","message"}` line, parsed best-effort like `segmentClosed` (missing/malformed keys degrade to a default message, the underlying state transition is never dropped), no `PROTOCOL_VERSION` bump.
- The simulated device-removal signal drives the **identical** code path as a real hardware unplug (one branch, exercised by both).
- Local-only: no network destination, upload, or notification-egress affordance added anywhere.

**Block If:**
- The wire/state-machine cannot express a non-fatal warning without forcing a terminal transition or breaking `dependency_firewall_holds` (would require redesigning the event contract).

**Never:**
- Never post a native OS notification — the 5s notification leg of the loud-failure triad stays Story 18.4; 19.4 does tray + banner only (its AC names exactly those two surfaces).
- Never claim on-hardware correctness of real silence samples, A/V-sync of the fallback track, or real Continuity-Camera/mic churn — that verification is deferred to Story 20.6, consistent with every real-capture leg since 16.6.
- No webcam/camera hot-unplug (Epic 20). No new persisted setting. No dedicated tray warning **icon asset** (18.4's polished triad) — reuse the recording icon with a warning-marked status line.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Real mic unplug mid-recording | device-disconnect notification (or `AVCaptureSessionRuntimeError` on 13–14) while `wantsMic` and live | session stays `recording`; sidecar emits `{"event":"warning","code":"micLost",...}`; mic track silence-filled; fallback to system default attempted; `RecordingStatusVm.warning=Some`; banner amber + tray warning line | never emits `error`/`exit(0)`; if fallback finds no input, warning message says "no microphone input", track stays silence-filled |
| Simulated removal (test hook) | `{"method":"simulateMicRemoval"}` RPC during an active session | drives the identical mic-loss path (warning event, no exit) | with no active session: clean no-op reply, exits 0 (mirrors existing smoke no-ops) |
| Warning event parsed | `{"event":"warning","code":"micLost","message":"…"}` line | `RecordingEvent::Warning{code,message}`; `RecordingSession::apply` keeps `state=Recording`, sets sticky `warning` | missing/blank `message` → tolerant default text; unknown keys ignored; state never changes |
| Warning in terminal/late state | Warning event arrives when session is `Finalized`/`Recovered`/`Failed` | ignored — no state resurrection, no warning set | best-effort drop, never a panic |
| Pre-Start mic unplug (setup) | selected `micDeviceId` no longer in live `microphones` before Start | `recording-audio-controls` reconciles selection back to `null` (System default input); Start ships no dead id | `null`/default is always available; reconciliation never blocks Start or requests permission again |

</intent-contract>

## Code Map

- `tools/keeper-rec/Sources/keeper-rec/main.swift` -- NDJSON-RPC switch (~293-420): add `case "simulateMicRemoval"` dispatching into `CaptureEngine`; keep `writeLine`/`emitEvent` (43-64) as the warning emitter. Device-change observers may register here or in `CaptureEngine`.
- `tools/keeper-rec/Sources/keeper-rec/Capture.swift` -- the mic-loss engine work: `beginCapture` (~268-358) install `AVCaptureDevice` disconnect observers for the active mic; on 13–14 also observe `AVCaptureSessionRuntimeError` on `micSession` (fixes the DW gap) and nil out `micSession` in `stop()` (~630-651); a `handleMicLost()` that emits the non-fatal `warning` event, starts silence-fill on `micInput` (~71 / `appendMicSample` ~491-496, add lower-bound PTS trim), and attempts fallback to the system default input; `didStopWithError` (~768-776) stays fatal only for whole-stream loss, never for mic-only.
- `tools/keeper-rec/Sources/keeper-rec/MicHealth.swift` -- NEW pure, hardware-free decision function (mirrors `Rotation.swift`'s testable-policy pattern): given a removal signal + current selection → `{shouldWarn, warningCode/message, fallbackToDefault}`. Unit-tested via `swift test`.
- `tools/keeper-rec/Tests/keeper-recTests/MicHealthTests.swift` -- NEW XCTest for the decision function (the simulated-signal contract at the sidecar boundary).
- `src-tauri/crates/keeper-core/src/recording.rs` -- add `RecordingEvent::Warning { code: String, message: String }` (~69-118); `parse_event` (~257-317) tolerant parse of `{"event":"warning",...}`; add sticky `RecordingSession.warning: Option<String>` (~126-129) set in `apply` (~168-209) for `Warning` (legal only in live/non-terminal states, no `SessionState` change); PROTOCOL_VERSION unchanged (~342). New unit tests (see Tasks).
- `src-tauri/crates/keeper-core/src/vm.rs` -- `RecordingStatusVm` (~2766-2803) gains `pub warning: Option<String>` (`#[ts(export)]`, camelCase); regenerates `src/lib/ipc/gen/RecordingStatusVm.ts` on `test:rust`.
- `src-tauri/crates/keeper/src/ipc.rs` -- the `recording_start` sink (~3556-3681, snapshot writes ~3604-3615) maps `RecordingEvent::Warning` → `RecordingStatusVm.warning` (sticky; not tied to `state==failed`); reset `warning=None` at session start; `recording_snapshot` (~3714-3741) carries it through.
- `src-tauri/crates/keeper/src/tray.rs` -- `render_recording`/`status_line` (~395-489) reflect `snapshot.warning`: a warning-marked status line while set; `decide_presence` (~273-288) and `is_live()` semantics unchanged (mic loss is still live/present).
- `src/components/recording/active-recording-banner.tsx` -- add the amber warning variant (~line 74 border/class branch): when `status.warning` is set, render the persistent amber left-edge + warning line that never auto-clears; still gated on live.
- `src/lib/stores/recording-mic.ts` -- add `isMicSelectionAvailable(deviceId, sources)` mirroring `recording-source.ts::isSelectionAvailable` (~63-82); `null`/default always available.
- `src/components/recording/recording-audio-controls.tsx` -- reconcile: an effect (~after 100-101) resets `micDeviceId` to `null` via `setMicDeviceId` when the selected id vanishes from `useRecordingSources()?.microphones` (closes the pre-Start stale-id DW item).
- Tests: `recording.rs` unit tests; `active-recording-banner.test.tsx` (warning variant + persistence); `recording-audio-controls.test.tsx` + `recording-mic.test.ts` (reconciliation); `swift test`; `scripts/smoke-keeper-rec.sh` (simulateMicRemoval clean no-op).

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/recording.rs` -- add `RecordingEvent::Warning{code,message}`, sticky `RecordingSession.warning`, `apply` handling (non-terminal only, no state change), and tolerant `parse_event` of `{"event":"warning",...}` -- platform-free never-abort warning contract.
- [x] `src-tauri/crates/keeper-core/src/recording.rs` (tests) -- warning keeps `state=Recording` and is sticky across events; warning in a terminal state is ignored; `parse_event` warning fixture incl. malformed/absent `message`; `dependency_firewall_holds` still passes -- the simulated-signal never-abort proof in core.
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- `RecordingStatusVm.warning: Option<String>` (`#[ts(export)]`) -- persistent warning slot for tray + banner.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- sink maps `Warning` → sticky `RecordingStatusVm.warning`, reset at session start; snapshot carries it -- surfaces the warning without a terminal transition.
- [x] `src-tauri/crates/keeper/src/tray.rs` -- warning-marked status line when `snapshot.warning` is set; presence/`is_live` unchanged -- raises the warning on the tray.
- [x] `tools/keeper-rec/Sources/keeper-rec/MicHealth.swift` (+ `Tests/keeper-recTests/MicHealthTests.swift`) -- NEW pure mic-loss decision function + XCTest -- hardware-free simulated-signal contract.
- [x] `tools/keeper-rec/Sources/keeper-rec/main.swift` -- `simulateMicRemoval` RPC dispatch; clean no-op with no active session -- the test/simulation entry point.
- [x] `tools/keeper-rec/Sources/keeper-rec/Capture.swift` -- device-disconnect observers (+ 13–14 `AVCaptureSessionRuntimeError` + nil `micSession` in `stop`), `handleMicLost` emitting non-fatal `warning`, silence-fill on `micInput` with lower-bound PTS trim, fallback to system default; mic loss never `error`/`exit` -- the real never-abort path (correctness gate: compile + release build + smoke; on-hardware = 20.6).
- [x] `src/lib/ipc/gen/RecordingStatusVm.ts` -- regenerated (committed) with the `warning` field -- wire parity.
- [x] `src/components/recording/active-recording-banner.tsx` (+ `.test.tsx`) -- amber, persistent, non-dismissible warning variant driven by `status.warning` -- raises the warning on the banner.
- [x] `src/lib/stores/recording-mic.ts` (+ `.test.ts`) -- `isMicSelectionAvailable(deviceId, sources)` helper -- selection-reconciliation predicate.
- [x] `src/components/recording/recording-audio-controls.tsx` (+ `.test.tsx`) -- reset `micDeviceId` to `null` when the selected device vanishes from the live list -- closes the pre-Start stale-device-id gap.

**Acceptance Criteria:**
- Given a live recording with a microphone, when the mic is removed via the simulated device-removal signal, then the session **never transitions to `failed`**, video and system-audio segments keep closing, the sidecar emits a `warning` (not `error`) event with no process exit, and the session still reaches `finalized` on Stop.
- Given a mic-loss warning has been raised, when the session continues, then `RecordingStatusVm.warning` stays `Some` for the rest of the session (non-dismissible, never auto-clears), surfaces on the tray status line and the banner amber variant, and resets to `None` only when a new session starts.
- Given the mic-loss path runs, when fallback is attempted, then the warning message honestly distinguishes fallback-succeeded ("using system default input") from no-input ("no microphone input"), and in both cases the mic track is silence-filled rather than gapped or aborted.
- Given the idle setup surface with a specific mic device selected, when that device disappears from the enumerated `microphones` list before Start, then the picker reconciles to "System default input" (`micDeviceId=null`), Start ships no dead device id, and no mic permission is re-requested.
- Given `bun run check`, `bun run check:rust`, `bun run test:rust`, `swift test --package-path tools/keeper-rec`, and `bash scripts/build-keeper-rec.sh`, then biome/tsc/vitest, clippy (`-D warnings`), cargo-nextest (with regenerated committed `.ts`), the Swift unit tests, and the Swift release build + NDJSON smoke (incl. `simulateMicRemoval` no-op) all pass.

## Spec Change Log

## Review Triage Log

### 2026-07-19 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 3: (high 0, medium 0, low 3)
- defer: 1: (high 0, medium 1, low 0)
- reject: 12
- addressed_findings:
  - `[low]` `[patch]` `Capture.swift::handleMicLost` — added a `guard !micLost` idempotency gate so repeated disconnect notifications, a fallback session's own runtime error, or repeated `simulateMicRemoval` no longer re-emit the warning or re-attempt fallback while already lost (a real sample still clears `micLost`, so a genuine second loss after recovery re-warns).
  - `[low]` `[patch]` `Capture.swift` — the mic `AVCaptureSession` runtime-error observer now lives in a dedicated `micSessionRuntimeObserver` slot that is replaced (not appended) on each fallback and removed at stop, so N fallbacks no longer leave N observers firing `handleMicLost` N times for one later error.
  - `[low]` `[patch]` `recording-audio-controls.tsx` / `recording-pane.tsx` — the pre-Start mic reconciliation effect is now gated on a new `active` prop (`active={!live}`), mirroring the source picker's documented pause-while-live contract so a mounted-but-inactive card can never silently reset the user's mic selection mid-session.

## Design Notes

**Why 19.4 introduces the warning slot (18.4 is still backlog).** The epic charts 19.4 as "raising into Epic 18's warning surface," but 18.4 (loud-failure triad) has not landed — the banner explicitly defers its warning variant to 18.4 and the tray/`RecordingStatusVm` have no warning concept. 19.4's own AC requires a persistent warning on tray + banner, so it adds the **minimal generic** slot (`RecordingStatusVm.warning: Option<String>`, an amber banner variant, a warning-marked tray line). This is a reusable seam: 18.4/18.5 populate the same slot for their faults. The one leg 19.4 deliberately leaves to 18.4 is the native notification — 19.4's AC names only tray + banner.

**Non-fatal is the crux.** Today `{"event":"error"}` is always paired with `exit(0)` and maps to the terminal `Failed` state. A mic-only fault must NOT take that path. The new `{"event":"warning",...}` line maps to `RecordingEvent::Warning`, which `apply` accepts only in non-terminal live states and which sets a sticky flag **without** changing `SessionState` — exactly the `SegmentClosed` precedent (orthogonal to the state, bumps a field). Whole-stream loss (`didStopWithError`) stays fatal; only mic loss is carved out.

**Testability boundary.** Per epic "simulated-signal testability" + the 16.6/19.3 pattern, the automated gates prove the *contract*, not the *hardware*: (1) a pure `MicHealth` decision fn unit-tested via `swift test`; (2) core `RecordingSession::apply` never-abort + stickiness tests; (3) a `simulateMicRemoval` RPC that drives the identical branch, smoke-checked as a clean no-op; (4) the banner + reconciliation vitest. Real silence samples in the file, real fallback A/V-sync, and real device churn are Story 20.6 — the dev host (macOS 26) only reaches the 15+ in-stream path, so the 13–14 branch is compile-verified only.

**Threading shape (sidecar → UI):**
```
// keeper-rec (on device-disconnect / simulateMicRemoval)
emitEvent(["event":"warning","code":"micLost","message":"microphone disconnected — using system default input"])
// recording.rs parse_event → RecordingEvent::Warning{code,message}
// RecordingSession::apply → self.warning = Some(message); state unchanged (stays Recording)
// ipc.rs sink → shared RecordingStatusVm.warning = Some(message)   // sticky, not gated on Failed
// → tray.rs status line (warning-marked) + active-recording-banner.tsx (amber, persistent)
```

**As-built notes (implementation).** Anchors drifted ~+30 lines in `recording.rs` after the `Warning` variant insert: variant ~106-118, `apply` warning arm ~226-243 (legal in `Recording`/`Rotating`/`Stopping` only; else `IllegalTransition`, which the ipc sink & drivers drop best-effort → "no state resurrection, never a panic"), `parse_event` warning arm ~330-350. Sticky reset lives only in the fresh `RecordingStatusVm` literal each `recording_start` builds (`ipc.rs` ~3546), so the sink is write-only and can never write `None` mid-session. **Silence-fill:** a 250 ms `DispatchSourceTimer` on `mediaQueue` pads mono 16-bit 48 kHz LPCM zeros from a shared "written-tail" cursor to host-clock now; that cursor doubles as the **lower-bound PTS trim** in `appendMicSample` (late/overlapping real samples below it are dropped, so the mic timeline never rewinds). **Fallback:** reuses the parallel `AVCaptureSession` path on every OS version (no mid-stream `microphoneCaptureDeviceID` swap); whichever source delivers first wins via the trim; a real sample clears `micLost` automatically; a failed fallback emits a second (no-input) warning (last-write-wins). Tray line: `⚠ Recording — 12:34 · segment 3, 412 MB — <message>` (facts first; no new icon asset; presence untouched). Deferred to 20.6: real silence audibility, fallback A/V-sync, real device churn, and the 13–14 branch (compile-verified only; dev host is macOS 26).

## Verification

**Commands:**
- `bun run check` -- expected: biome + tsc + vitest green (banner warning variant + persistence, mic device reconciliation).
- `bun run check:rust` -- expected: rustfmt clean + clippy `-D warnings` clean.
- `bun run test:rust` -- expected: cargo-nextest green incl. new warning/never-abort/parse tests; regenerated `RecordingStatusVm.ts` committed.
- `swift test --package-path tools/keeper-rec` -- expected: `MicHealth` decision tests pass.
- `bash scripts/build-keeper-rec.sh` -- expected: Swift release build succeeds and NDJSON smoke passes, including a `simulateMicRemoval` clean no-op.

## Auto Run Result

Status: done

**Summary.** Microphone hot-unplug resilience: a mic unplugged mid-recording no longer aborts the session. A new non-fatal, sticky warning slot threads sidecar → core → view model → tray + banner; the sidecar keeps video + system audio rolling, silence-fills the mic track, and falls back to the system default input. On the setup surface a vanished mic selection reconciles to System default input so Start never ships a dead device id.

**Files changed (one-line):**
- `keeper-core/src/recording.rs` — non-fatal `RecordingEvent::Warning{code,message}`, sticky `RecordingSession.warning`, live-only `apply` (no state change), tolerant `parse_event`; + never-abort/sticky/parse tests.
- `keeper-core/src/vm.rs` — `RecordingStatusVm.warning: Option<String>` (`#[ts(export)]`).
- `keeper/src/ipc.rs` — sink maps `Warning` → sticky snapshot slot (gated on `apply().is_ok()`), reset at session start.
- `keeper/src/tray.rs` — warning-marked status line (`⚠ …`); presence/`is_live` untouched.
- `keeper-rec/MicHealth.swift` (+ `MicHealthTests.swift`) — pure mic-loss decision policy + 8 XCTests.
- `keeper-rec/main.swift` — `simulateMicRemoval` RPC (clean no-op with no session).
- `keeper-rec/Capture.swift` — disconnect + runtime-error observers, `handleMicLost` (non-fatal warning, idempotent), silence-fill + PTS-lower-bound trim, default-input fallback, observer hygiene, `micSession` nil on stop.
- `scripts/smoke-keeper-rec.sh` — `simulateMicRemoval` no-op assertion.
- `active-recording-banner.tsx` — amber, persistent, non-dismissible warning variant.
- `recording-mic.ts` — `isMicSelectionAvailable`; `recording-audio-controls.tsx` / `recording-pane.tsx` — reconciliation effect gated on `active={!live}`.
- `src/lib/ipc/gen/RecordingStatusVm.ts` — regenerated with `warning`.

**Review findings:** intent_gap 0, bad_spec 0. Patches applied 3 (all low: Swift `handleMicLost` idempotency guard; Swift runtime-error observer dedup; TS reconciliation gated on `!live`). Deferred 1 (medium — the macOS 15+ on-hardware mic-loss seam: activeMicDeviceId guess, fallback double-feed, silence-fill clock/format assumptions — routed to Story 20.6). Rejected 12 (unreachable/already-scoped/no-defect, incl. the "warning invisible in Stopping" finding — `stopping` is a live banner state).

**Verification (all independently re-run and green):** `bun run check` (biome + tsc + vitest 1373), `bun run check:rust` (rustfmt + clippy `-D warnings`), `bun run test:rust` (cargo-nextest 905, incl. new warning/never-abort/parse tests + `dependency_firewall_holds`), `swift test` (32, incl. `MicHealthTests` 8), `bash scripts/build-keeper-rec.sh` (release build + smoke incl. `simulateMicRemoval` no-op).

**Residual risks:** The real-capture behavior (silence audibility, fallback A/V-sync, real device churn, the 13–14 branch) is verifiable only on hardware and is charted to Story 20.6 (SM-10) — see the deferred-work entry. No new network egress; `keeper-core` stays platform-free.

**Follow-up review recommended:** false — the review pass made only three low-severity, localized robustness fixes with no behavior/API/security/data impact.
