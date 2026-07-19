---
title: 'Microphone Picker & Separate Track'
type: 'feature'
created: '2026-07-19'
baseline_revision: '7915d63bde82a6976ab44bf15377be6c118c3186'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
final_revision: '3941d26'
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-19-context.md'
warnings: [oversized]
---

<intent-contract>

## Intent

**Problem:** The recorder can capture screen + system audio, but never the user's voice: `SessionDevices.microphone`/`camera` are hardcoded `false` (`ipc.rs:3464`), `SessionParams` has no mic field, the sidecar has zero microphone capture/enumeration/permission code (`microphones:[]`, `"microphone":false/notDetermined` are placeholders), and the Audio card has no mic control. So a screencast can't carry narration, and there's no honest lazy-permission path for the mic.

**Approach:** Land the microphone leg end to end. Add a mic device-picker to the Audio card (System default input first; **off by default** so permission is never requested preemptively); enumerate real input devices in the sidecar (`AVCaptureDevice.DiscoverySession`) surfaced through the already-shaped `RecordingSourcesVm.microphones: RecordingDeviceVm[]`; request mic TCC access **lazily, only when the user enables the source**, via a new `request_microphone_permission` command → sidecar `requestMicrophone` RPC; capture the mic as its **own, unmixed AAC track** (`captureMicrophone` in-stream on macOS 15+, a parallel `AVCaptureSession` on 13–14 — same writer, invisible to the user); and thread the mic selection through `recording_start` into `SessionParams` (sidecar wire) + `SessionDevices.microphone` (manifest). Also reconcile the 19.2-deferred Source-picker "…and audio are recorded" copy with the system-audio toggle. Hot-unplug resilience (19.4) and the rich TCC pre-flight rows (20.2) stay out of scope.

## Boundaries & Constraints

**Always:**
- The Audio card gains a **microphone device-picker** (shadcn `Select`) with **"System default input" always the first option and the default selection**, plus each enumerated `RecordingDeviceVm`. The mic source is **disabled by default**; enabling it is what triggers the permission request. The picker is disabled/greyed with a helper caption while the mic is off. Copy voice: sentence case, no exclamation, no "please"; "Recording Session"/"segment" capitalized where they appear.
- **Microphone permission is requested only when the mic source is enabled — never preemptively** (FR-69, AD-36). Enabling the source calls `request_microphone_permission`; the outcome is surfaced with an honest inline caption (granted → mic records; denied → the mic track will be silent, resolve in System Settings). No permission is probed or requested on setup-surface render.
- The mic is written as its **own AAC track, never premixed** with system audio (separate `AVAssetWriterInput`), using in-stream `SCStreamConfiguration.captureMicrophone` + `.microphone` stream-output on macOS 15+, and a parallel audio-only `AVCaptureSession`/`AVCaptureAudioDataOutput` on macOS 13–14 — same `AVAssetWriter`, invisible to the user and to the capability flag (FR-69, AD-36). The second track survives segment rotation like the system-audio track.
- The selection is **per-session**, threaded through `recording_start` like 19.1's target and 19.2's `system_audio`: new params `microphone_enabled: Option<bool>` (`None`→`false`) and `microphone_device_id: Option<String>` (`None`→system default). They flow to **both** `SessionParams.microphone: Option<MicSelection>` (wire `micEnabled`/`micDeviceId`) **and** `SessionDevices.microphone` (manifest = enabled), so an off session writes `devices.microphone=false` and the sidecar adds no mic track.
- Wire/state logic stays **platform-free** in `keeper-core::recording` — the mic-selection struct + wire builder + the new `Recorder::request_microphone` trait method are pure/`impl Future`; all AVFoundation/ScreenCaptureKit tokens live only in the sidecar and `keeper/src/recorder.rs` under `#[cfg(desktop)]`. `dependency_firewall_holds` must stay green.
- `NSMicrophoneUsageDescription` is added to `src-tauri/crates/keeper/Info.plist` (mirroring `NSScreenCaptureUsageDescription`) so the OS mic prompt is legal; the `keeper-rec.entitlements` empty dict is unchanged (mic is TCC-gated at runtime, not entitlement-gated). The Tauri command args are snake_case in Rust ↔ camelCase in JS `invoke` (`microphone_enabled`↔`microphoneEnabled`).
- The new `request_microphone_permission` command is registered in `keeper/src/lib.rs` `generate_handler!`. Any new/changed VM derives `serde` + `ts_rs::TS` (camelCase, `#[ts(export)]`) with the regenerated `.ts` in `src/lib/ipc/gen/` committed, never hand-edited (AD-7). (This story reuses the existing `RecordingDeviceVm`/`TccPermission`/`RecordingSourcesVm` types — no new exported VM is expected.)

**Block If:**
- Enabling the microphone or writing a second AAC track cannot be done without raising the `recording` capability floor / `minimumSystemVersion` below the app's existing floor. Surface, do not raise the floor. (Mic rides the macOS 13 floor: `captureMicrophone` gates behind `if #available(macOS 15,*)`, `AVCaptureSession` covers 13–14.)
- Microphone capture requires a TCC permission or entitlement **beyond** `NSMicrophoneUsageDescription` + the runtime `AVCaptureDevice.requestAccess(for: .audio)` grant (e.g. a hardened-runtime entitlement the ad-hoc sign flow can't provide). Surface rather than proceed.

**Never:**
- No microphone **hot-unplug/fallback/warning** behavior (that is 19.4, which depends on this story and Epic 18's warning surface); no rich mic/camera **TCC pre-flight rows** with persistent status + re-check (that is 20.2). 19.3's denied-permission handling is a single honest inline caption, not a preflight surface.
- No webcam/camera capture (20.1) — `SessionDevices.camera` stays hardcoded `false`. No destination-folder chooser or fps (19.5).
- **No premixing** of mic with system audio — separate AAC tracks. No change to the system-audio track, the video track, or 19.1's source list/polling.
- **No DB persistence of the mic selection and no Settings → Recording mirror** — like 19.1's source and 19.2's system-audio toggle, the mic is ephemeral/per-session (default off, System default input). (Stated deliberately so it is not read as an intent gap.)
- No new network destination, upload, telemetry, transcription, or preview (local-only, FR-76). No hand-edited generated `.ts`. No `.unwrap()`/bare `.expect()` in Rust production paths; no `any` in TS.
- No claim that mic audio is sample-verified here — the "the mic track actually contains the user's voice, as a separate stream" check folds into SM-9/SM-10 on-hardware acceptance (Story 20.6), like every real-capture leg since 16.6. 19.3's automated gates are compile + Rust/TS unit tests + the Swift release build + NDJSON smoke.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Default start (mic off) | `recording_start` with `microphone_enabled` omitted (`None`) | `SessionParams.microphone = None`; wire has no `micEnabled`; manifest `devices.microphone = false`; sidecar adds no mic track — 19.2 path unchanged | No error |
| Enable mic, system default | mic Switch on, picker "System default input" → Start | `recording_start(microphone_enabled: Some(true), microphone_device_id: None)`; wire `"micEnabled": true`, no `micDeviceId`; manifest `devices.microphone = true`; sidecar captures default input as a second AAC track | No error |
| Enable mic, specific device | Switch on, picker device `X` → Start | wire `"micEnabled": true, "micDeviceId": "X"`; manifest `devices.microphone = true` | No error |
| Lazy permission on enable | user flips mic Switch on | `request_microphone_permission` fires exactly once → sidecar `requestMicrophone`; caption reflects granted/denied; **nothing fires on render while off** | denied → honest caption, mic track silent (no crash) |
| Enumerate mics | `recording_list_sources` | `RecordingSourcesVm.microphones` lists real input devices (`id`+`name`); picker renders them under "System default input" | empty list → only "System default input" shown |
| macOS 13–14 host | mic enabled at Start | parallel `AVCaptureSession` audio path feeds the same second AAC track; user-invisible | No error |
| iOS / sidecar absent | `recording_start`/`request_microphone_permission` on non-desktop | `Unsupported`, no spawn/panic; mic params accepted and ignored; `dependency_firewall_holds` passes | honest |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/recording.rs` -- add `MicSelection { device_id: Option<String> }` (pure data, near `ApplicationTarget` ~line 367) and `SessionParams.microphone: Option<MicSelection>` (~line 385). In `start_recording_request` (~line 412) emit `wire["micEnabled"]=true` + optional `wire["micDeviceId"]` when `microphone` is `Some` (additive, mirroring the `application` branch; `PROTOCOL_VERSION` unchanged). Add `request_microphone_request(id)` builder (`method:"requestMicrophone"`, ~line 360) and a `Recorder::request_microphone(&self) -> impl Future<Output = Result<TccPermission, CoreError>>` trait method (~line 1168). Extend `dependency_firewall_holds` stays green. New unit tests (see Tasks).
- `src-tauri/crates/keeper/src/ipc.rs` -- `recording_start` (~line 3409) gains `microphone_enabled: Option<bool>` + `microphone_device_id: Option<String>`; resolve `let mic_on = microphone_enabled.unwrap_or(false)`, build `SessionParams.microphone = mic_on.then(|| MicSelection { device_id: microphone_device_id })` and set `SessionDevices.microphone = mic_on` (replace the hardcoded `false` at ~line 3465). Add `request_microphone_permission(state) -> Result<TccPermission, IpcError>` calling `state.recorder.request_microphone()` (mirror `request_screen_recording_permission` ~line 3275).
- `src-tauri/crates/keeper/src/recorder.rs` -- implement `request_microphone` on the desktop `SidecarRecorder` (send `request_microphone_request`, parse the `{status}` reply into `TccPermission`) under `#[cfg(desktop)]`; the iOS/non-desktop recorder returns `Unsupported`/`NotDetermined`. AVFoundation tokens (if any) stay here, never in core.
- `src-tauri/crates/keeper/src/lib.rs` -- register `ipc::request_microphone_permission` in `generate_handler!` (~line 364, beside the other recording commands).
- `src-tauri/crates/keeper/Info.plist` -- add `NSMicrophoneUsageDescription` (mirror the existing `NSScreenCaptureUsageDescription` string, ~lines 11-12).
- `tools/keeper-rec/Sources/keeper-rec/main.swift` -- decode `micEnabled` (Bool, default false) + `micDeviceId` (String?, nil = default) in the `startRecording` case (~line 292-328) and thread into `captureEngine.start(...)`; add `listMicrophones()` (`AVCaptureDevice.DiscoverySession` audio devices → `{id: uniqueID, name: localizedName}`) and use it in `sourcesResult()` (~line 239, replacing `[]`); make `permissionsPayload()` report the real `authorizationStatus(for: .audio)` and set `features.microphone = true`; add a `requestMicrophone` RPC method (mirror `requestScreenRecording` ~line 279, calling `AVCaptureDevice.requestAccess(for: .audio)`).
- `tools/keeper-rec/Sources/keeper-rec/Capture.swift` -- `CaptureEngine.start` (~line 143) + `beginCapture` (~line 241) gain mic params; add a `micInput: AVAssetWriterInput?` slot to `SegmentWriter` (~line 62) and a parallel AAC block in `makeSegmentWriter` (~line 338); on macOS 15+ set `config.captureMicrophone=true` (+ `microphoneCaptureDeviceID` when a device is picked) and register/route the `.microphone` stream-output (the `default:` branch at ~line 605 currently drops it); on 13–14 stand up an audio-only `AVCaptureSession`+`AVCaptureAudioDataOutput` on `mediaQueue` feeding `micInput`; start/stop it with the capture lifecycle.
- `src/lib/stores/recording-mic.ts` -- NEW ephemeral zustand store mirroring `recording-audio.ts`: `{ micEnabled: boolean (default false), micDeviceId: string | null (default null) }`; hooks `useMicEnabled()`/`useMicDeviceId()`, imperative reads `micEnabled()`/`micDeviceId()` (for header Start), setters, `resetRecordingMicForTest()`.
- `src/components/recording/recording-audio-controls.tsx` -- append a mic row under the system-audio row: a `Switch` (default off) + a `Select` device-picker ("System default input" first, then `useRecordingSources()?.microphones`), the picker disabled while off; on enable, call `requestMicrophonePermission()` and surface an inline granted/denied caption; separate-track disclosure; exported label/testid constants.
- `src/lib/ipc/client.ts` -- `recordingStart(target?, systemAudio?, micEnabled?, micDeviceId?)` (~line 1695) passes `microphoneEnabled`/`microphoneDeviceId` (`?? null`); add `requestMicrophonePermission(): Promise<TccPermission>` → `invoke("request_microphone_permission")`.
- `src/hooks/use-recording-session.ts` -- `start(target?, systemAudio?, micEnabled?, micDeviceId?)` (~line 135) forwards the two mic args to `recordingStart`.
- `src/components/layout/recording-pane.tsx` -- header Start (~line 92) threads `micEnabled()` + `micDeviceId()` as the 3rd/4th args to `start(...)`.
- `src/components/recording/recording-source-picker.tsx` -- fix the 19.2-deferred contradiction: `appScopeDisclosure(appName, systemAudioOn)` drops the "and audio" clause when system audio is off; read `useSystemAudioEnabled()` at the render site (~line 239).
- Tests: `recording.rs` unit tests; `recording-mic.test.ts`; `recording-audio-controls.test.tsx`; update `recording-pane.test.tsx` + `recording-source-picker.test.tsx` + any `client.ts` test.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/recording.rs` -- add `MicSelection` + `SessionParams.microphone`; emit `micEnabled`/`micDeviceId` in `start_recording_request`; add `request_microphone_request` + `Recorder::request_microphone` -- platform-free mic wire + permission contract.
- [x] `src-tauri/crates/keeper-core/src/recording.rs` (tests) -- add wire tests: mic-off (no `micEnabled`), mic-on default (`micEnabled:true`, no `micDeviceId`), mic-on device (`micDeviceId:"X"`); assert `dependency_firewall_holds` still passes -- no-hardware wire coverage.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- `recording_start` gains the two mic params threaded into `SessionParams.microphone` + `SessionDevices.microphone` (un-hardcode `false`); add `request_microphone_permission` command -- activates the mic end to end in Rust.
- [x] `src-tauri/crates/keeper/src/recorder.rs` -- implement `request_microphone` on the desktop recorder; iOS returns `Unsupported` -- shell/platform port for the mic RPC.
- [x] `src-tauri/crates/keeper/src/lib.rs` -- register `request_microphone_permission` in `generate_handler!` -- exposes the command to JS.
- [x] `src-tauri/crates/keeper/Info.plist` -- add `NSMicrophoneUsageDescription` -- legal OS mic prompt.
- [x] `tools/keeper-rec/Sources/keeper-rec/main.swift` -- decode `micEnabled`/`micDeviceId`; `listMicrophones()`; real mic permission status + `requestMicrophone` RPC; `features.microphone=true` -- enumeration + lazy permission in the sidecar.
- [x] `tools/keeper-rec/Sources/keeper-rec/Capture.swift` -- second AAC `micInput` track; macOS 15+ `captureMicrophone`/`.microphone` output + 13–14 `AVCaptureSession` path; rotation-safe; lifecycle start/stop -- the actual mic capture, never premixed.
- [x] `src/lib/stores/recording-mic.ts` (+ `.test.ts`) -- NEW ephemeral mic store (enabled default false, device default null; hooks + imperative reads + setters + reset) -- shared mic state between the Audio card and header Start.
- [x] `src/components/recording/recording-audio-controls.tsx` (+ `.test.tsx`) -- mic Switch + device Select ("System default input" first, off by default, greyed when off), lazy `requestMicrophonePermission` on enable + inline caption, exported labels -- the live mic picker.
- [x] `src/lib/ipc/client.ts` -- `recordingStart(..., micEnabled?, micDeviceId?)` + `requestMicrophonePermission()` -- carries mic to Rust.
- [x] `src/hooks/use-recording-session.ts` -- `start(..., micEnabled?, micDeviceId?)` forwards to `recordingStart` -- threads the value from the pane.
- [x] `src/components/layout/recording-pane.tsx` (+ `.test.tsx`) -- header Start passes `micEnabled()`/`micDeviceId()` -- wires the picker into Start.
- [x] `src/components/recording/recording-source-picker.tsx` (+ `.test.tsx`) -- condition the app-scope "and audio" clause on `useSystemAudioEnabled()` -- closes the 19.2-deferred copy contradiction.

**Acceptance Criteria:**
- Given the idle setup surface, when the Audio card renders, then the mic device-picker shows "System default input" as the first/default option, the mic source is **off**, and **no** microphone permission has been requested; turning the source on requests mic permission exactly once and surfaces the granted/denied result inline.
- Given the mic source is enabled (System default input or a specific device) and a recording starts, then `recording_start` receives `microphone_enabled: true` (+ `microphone_device_id` when a device is chosen), the sidecar wire carries `"micEnabled": true` (+ `"micDeviceId"`), the manifest records `devices.microphone = true`, and the mic is written as its own AAC track separate from the system-audio track; leaving the mic off records `devices.microphone = false` and no mic track.
- Given an application target is selected with system audio **off**, when the Source card renders its disclosure, then it no longer claims the app's "audio" is recorded (the "and audio" clause is dropped), staying consistent with the Audio card's off-state note.
- Given the sidecar is unavailable or the platform is iOS, when `recording_start` or `request_microphone_permission` is called, then it returns `Unsupported` with no spawn/panic, and `keeper-core::recording` still carries no tauri/Apple/process token (`dependency_firewall_holds` passes).
- Given `bun run check`, `bun run check:rust`, `bun run test:rust`, and `bash scripts/build-keeper-rec.sh`, then biome/tsc/vitest, clippy (`-D warnings`), cargo-nextest (with regenerated committed `.ts`), and the Swift release build + NDJSON smoke all pass.

## Spec Change Log

_Empty — no bad_spec loopback was triggered._

## Review Triage Log

### 2026-07-19 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 1: (high 0, medium 0, low 1)
- defer: 2: (high 0, medium 2, low 0)
- reject: 13
- addressed_findings:
  - `[low]` `[patch]` The mic permission caption could show a stale granted/denied outcome under rapid on→off→on toggling: `onMicToggle` fired `requestMicrophonePermission()` on every enable with no generation guard, so a late/out-of-order resolution from a superseded enable overwrote the caption for the current state. Added a `micRequestSeq` ref bumped on every toggle; a resolution only writes `micPermission` when its `requestId` is still current, and disabling now also clears any prior outcome. Added a deterministic regression test (two deferred permission promises: the superseded first enable's late `granted` no longer overwrites the second enable's `denied`).

_Deferred:_
- `[medium]` The macOS 13–14 `AVCaptureSession` mic path (new in this story) carries several robustness gaps that only manifest on 13–14 hardware — the dev host is macOS 26 (uses the 15+ in-stream `captureMicrophone` path), so none are reachable by the automated gates: (1) the mic capture clock is never reconciled with the SCStream/video anchor (A/V-sync drift risk); (2) `appendMicSample` has no lower-bound PTS trim, so a mic sample whose PTS precedes a rotated segment's session start could fail writer B; (3) an `AVCaptureSessionRuntimeError` after `startRunning` is not observed, so a post-start device failure yields a silent mic track with no event; (4) `startRunning`/`stopRunning` run on unsynchronized global-queue tasks and `micSession` is never nil'd, a benign start/stop race. Graceful mic degradation + the loud warning surface for these is Story 19.4's chartered scope (Microphone Hot-Unplug Resilience, depends on this story + Epic 18); on-hardware A/V-sync + rotation verification is Story 20.6. Logged to the deferred-work ledger.
- `[medium]` Setup-surface mic device-selection reconciliation: when the selected `micDeviceId` disappears from the enumerated `microphones` list before Start (a pre-Start unplug), the `Select` renders a stale/empty value and Start still ships the dead id (15+ silent fallback / 13–14 clean error). Device churn + re-enumeration is Story 19.4's scope. Logged to the ledger.

_Rejected (noise / not this story / not reachable):_ the `microphoneEnabled↔microphone_enabled` "silent mismatch" (the codebase-wide Tauri camelCase↔snake_case convention, proven by 19.2's `systemAudio`); a `notDetermined` caption branch (`AVCaptureDevice.requestAccess` always resolves granted/denied post-request, so it is unreachable); the `"__default__"` sentinel colliding with a real `AVCaptureDevice.uniqueID` (uniqueIDs are long hardware strings, not that token); "capability ≠ permission" (both are reported honestly and independently, as designed); the manifest recording `microphone: mic_on` as intent-not-outcome (identical shape to 19.2's `system_audio`, deliberate); an empty-string `micDeviceId` reaching `AVCaptureDevice(uniqueID:)` (our client sends `null` or a real id, never `""`); `parse_request_microphone_result`'s serde coupling to `TccPermission` (the same tested pattern as the screen-recording parse); the app-scope disclosure "understating" capture when mic is on + system audio off (the source-scope line describes what leaks from the app source, not a total-audio manifest — the mic is the user's own voice, disclosed separately in the Audio card's mic row); a leftover `micDeviceId` on the wire when mic is off (Rust discards it; harmless); an old-sidecar capability pre-check (keeper + keeper-rec ship in lockstep); first-segment mic samples before the video anchor being dropped (a fractional leading clip, identical to system audio, acceptable); a zero-sample mic AAC track when enabled-but-silent (`markAsFinished` on an empty input is tolerated by `finishWriting`); and the mic-across-rotation / 13–14-path test-coverage gaps (Swift live-capture is unit-test-hostile, folded into the 20.6 on-hardware verification like every real-capture leg since 16.6).

## Design Notes

**Ephemeral, off by default — the lazy-permission hinge.** The mic mirrors 19.1/19.2's ephemeral pattern (small vanilla-zustand store, per-session, never persisted, never mirrored to Settings — DB persistence is reserved for 17.5 segmentation and 19.5 folder/fps). The crux is that the mic source is **off by default**: the AC "defaults to System default input" is the picker's default *device*, not an enabled state. Off-by-default is exactly what makes "permission requested only when enabled, never preemptively" true — enabling the Switch is the sole trigger for `request_microphone_permission`. Requesting at enable-time (setup surface), not at Start, matches AD-36's "probed and requested only when the source is enabled."

**Two OS paths, one writer (AD-36).** The user and the capability flag never see the macOS split. On 15+ the mic rides the existing `SCStream` via `captureMicrophone` + a `.microphone` output type (the sample-routing `default:` branch already anticipated this); on 13–14 a separate audio-only `AVCaptureSession` feeds the same second `AVAssetWriterInput`. Either way it's a second, unmixed 48 kHz AAC track that rotates with segments like the system-audio track. This is the largest, least-unit-testable slice — consistent with 16.6, its correctness gate here is compile + Swift release build + NDJSON smoke; the "voice is actually in the file, as its own stream" verification is Story 20.6, not this loop.

**Threading shape (JS → Rust):**
```
// recording-pane.tsx (header Start)
void start(selectedRecordingTarget(), systemAudioEnabled(), micEnabled(), micDeviceId());
// client.ts
invoke("recording_start", { target, systemAudio, microphoneEnabled: micEnabled ?? null, microphoneDeviceId: micDeviceId ?? null });
// ipc.rs
let mic_on = microphone_enabled.unwrap_or(false);
let microphone = mic_on.then(|| MicSelection { device_id: microphone_device_id });
// → SessionParams.microphone (wire micEnabled/micDeviceId) + SessionDevices.microphone = mic_on
```

**Why no new VM.** `RecordingDeviceVm {id,name}` (picker rows), `RecordingSourcesVm.microphones` (already round-trips, currently empty), and `TccPermission` (permission result) already exist and are `#[ts(export)]`ed — 19.3 fills them with real data rather than introducing types, so no `ts_rs` regen churn beyond what those already emit.

## Verification

**Commands:**
- `cd src-tauri && cargo test -p keeper-core recording` -- expected: the new mic wire tests + `dependency_firewall_holds` pass.
- `bun run test:rust` -- expected: cargo-nextest green (regenerate + commit any `.ts` if a VM changed — none expected).
- `bun run check:rust` -- expected: `cargo fmt --check` + clippy `--all-targets -D warnings` clean.
- `bun run check` -- expected: biome + tsc + vitest pass (new mic store/controls tests + updated pane/source-picker tests).
- `bash scripts/build-keeper-rec.sh` -- expected: `swift build -c release --arch arm64` + NDJSON smoke green (compiles the new mic capture + `listMicrophones` + `requestMicrophone`).
- `cd src-tauri && cargo check --workspace --target aarch64-apple-ios` -- expected: compiles; `recording_start`/`request_microphone_permission` accept mic input and the iOS recorder returns `Unsupported`.

**Manual checks:**
- Confirm `keeper-core/src/recording.rs` imports no tauri/Apple-framework/process API; the mic picker defaults to "System default input" with the source off and no permission requested until enabled; and the Audio-card + Source-card copy is honest in every combination of system-audio on/off × mic on/off × display/application target.

## Auto Run Result

Status: done

**Summary.** Implemented Story 19.3 — the microphone leg, end to end. `recording_start` gains `microphone_enabled: Option<bool>` (`unwrap_or(false)` — off by default, the lazy-permission hinge) + `microphone_device_id: Option<String>` (`None` → system default input); `mic_on.then_some(MicSelection { device_id })` threads into **both** `SessionParams.microphone` (new field; wire additive `micEnabled`/`micDeviceId`) and the manifest `SessionDevices.microphone` (un-hardcoded from `false`). A new `request_microphone_permission` command → `Recorder::request_microphone` trait → sidecar `requestMicrophone` RPC (`AVCaptureDevice.requestAccess(for: .audio)`) requests mic TCC **lazily, only when the user enables the source**. The Swift sidecar now enumerates real input devices (`AVCaptureDevice.DiscoverySession`, surfaced via the pre-shaped `RecordingSourcesVm.microphones`), reports the real mic TCC state + `features.microphone = true`, and captures the mic as its **own, unmixed 48 kHz AAC track** — in-stream `captureMicrophone` + `.microphone` output on macOS 15+, a parallel audio-only `AVCaptureSession` on 13–14, one writer either way, rotation-safe. The Audio card gained a mic `Switch` (default off) + a `Select` device-picker ("System default input" first/default, greyed while off) with honest lazy-permission captions. `NSMicrophoneUsageDescription` was added to keeper's Info.plist. The 19.2-deferred Source-picker copy contradiction is closed — `appScopeDisclosure` drops the "and audio" clause when system audio is off. No DB persistence / Settings mirror (deliberate, per epic scoping); 19.4 (hot-unplug) and 19.5 (destination/fps) stay out of scope.

**Files changed.**
- `src-tauri/crates/keeper-core/src/recording.rs` — `MicSelection` + `SessionParams.microphone`; additive `micEnabled`/`micDeviceId` wire; `request_microphone_request` builder + `parse_request_microphone_result`; `Recorder::request_microphone` trait method + `FakeRecorder` impl; new unit tests (mic-off/on-default/on-device wire, request builder, status parse + faults); firewall stays green.
- `src-tauri/crates/keeper/src/recorder.rs` — desktop `request_microphone` (RPC id 6) + `MIC_PROMPT_TIMEOUT` (120 s, since `requestAccess` blocks on the user); iOS returns `Unsupported`; 2 fake-sidecar tests.
- `src-tauri/crates/keeper/src/ipc.rs` — `recording_start` mic params threaded into `SessionParams.microphone` + `SessionDevices.microphone`; new `request_microphone_permission` command.
- `src-tauri/crates/keeper/src/lib.rs` — command registered.
- `src-tauri/crates/keeper/Info.plist` — `NSMicrophoneUsageDescription` (honest local-only framing).
- `tools/keeper-rec/Sources/keeper-rec/main.swift` — real mic TCC state, `features.microphone = true`, `listMicrophones()`, `requestMicrophone` RPC, `micEnabled`/`micDeviceId` decode + threading.
- `tools/keeper-rec/Sources/keeper-rec/Capture.swift` — `SegmentWriter.micInput` + per-segment AAC mic track (never premixed, rotation-safe); macOS 15+ `captureMicrophone`/`.microphone` routing; macOS 13–14 `AVCaptureSession` path.
- `src/lib/stores/recording-mic.ts` (+ `.test.ts`, NEW) — ephemeral mic store (enabled default false, device default null).
- `src/components/recording/recording-audio-controls.tsx` (+ `.test.tsx`) — mic Switch + device Select + lazy-permission captions; **review patch**: `micRequestSeq` generation guard against stale out-of-order permission resolutions.
- `src/lib/ipc/client.ts` — `recordingStart(target?, systemAudio?, micEnabled?, micDeviceId?)` + `requestMicrophonePermission()`.
- `src/hooks/use-recording-session.ts` — `start` forwards the mic args.
- `src/components/layout/recording-pane.tsx` (+ `.test.tsx`) — header Start threads `micEnabled()`/`micDeviceId()`.
- `src/components/recording/recording-source-picker.tsx` (+ `.test.tsx`) — app-scope disclosure conditions on `useSystemAudioEnabled()` (closes the 19.2-deferred contradiction).
- `src/lib/ipc/client.test.ts` — mic threading tests.

**Review findings breakdown.** 1 patch applied (low: a `micRequestSeq` generation guard so a late/out-of-order permission resolution from a superseded rapid on→off→on toggle can no longer overwrite the current caption; added a deterministic regression test). 2 deferred (both medium): (a) the macOS 13–14 `AVCaptureSession` mic robustness cluster — clock reconciliation / rotation PTS lower-bound / runtime-error surfacing / start-stop race — which only manifests on 13–14 hardware (dev host is macOS 26; 15+ in-stream path) and whose graceful-degradation + warning surface is Story 19.4's chartered scope, with on-hardware A/V-sync in 20.6; (b) setup-surface mic device-selection reconciliation when a picked device unplugs pre-Start (device churn is 19.4's scope). Both logged to the deferred-work ledger. 13 rejected (Tauri camelCase convention, unreachable `notDetermined` branch, impossible sentinel collision, capability≠permission, manifest-intent-not-outcome matching 19.2, unreachable empty-string device id, serde coupling = tested pattern, source-scope disclosure vs orthogonal mic row, harmless off-state wire id, lockstep-sidecar assumption, acceptable leading-clip drop, tolerated empty AAC track, and Swift live-capture test-coverage gaps folded into 20.6). 0 intent_gap, 0 bad_spec (no repair loopback). Details in the Review Triage Log.

**Follow-up review recommended:** false — the final pass made one low-severity, localized, test-covered frontend patch (a permission-race guard, no API/data/behavior-at-Start impact) and deferred the rest; not significant enough to warrant an independent follow-up.

**Verification.** All gates independently re-run green after the review patch: `bun run check:rust` (fmt + clippy `-D warnings`) clean; `bun run test:rust` → 898/898 nextest (incl. 8 new mic tests + `dependency_firewall_holds`); `bun run check` → biome + tsc + vitest **1364/1364** + `check:core-tauri-free` clean; `bash scripts/build-keeper-rec.sh` → Swift release build + NDJSON smoke green (`features.microphone: true` + real mic TCC state); `cargo check --workspace --target aarch64-apple-ios` → compiles (only pre-existing dead-code warnings; iOS recorder returns `Unsupported`). Manual: `keeper-core/src/recording.rs` carries no tauri/Apple/process token; mic picker defaults to "System default input" with the source off and no permission probed until enabled.

**Residual risks.** (1) The macOS 13–14 `AVCaptureSession` mic path is compile-verified only — its A/V-sync, rotation PTS handling, and runtime-error behavior are unverified until 13–14 hardware (Story 20.6) and its graceful-degradation/warning surface lands in 19.4; the dev host (macOS 26) exercises only the 15+ in-stream path. (2) Sample-level "the voice is actually present as a separate stream" verification folds into SM-9/SM-10 on-hardware acceptance (Story 20.6), like every real-capture leg since 16.6. (3) The pre-Start device-vanish picker reconciliation is deferred to 19.4. 19.3's automated gates are compile + Rust/TS unit tests + the Swift release build + NDJSON smoke.
