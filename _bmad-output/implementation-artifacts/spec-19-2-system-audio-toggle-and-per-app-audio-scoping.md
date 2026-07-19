---
title: 'System-Audio Toggle & Per-App Audio Scoping'
type: 'feature'
created: '2026-07-19'
baseline_revision: '84c7af46f3f20d2205576bf58c6fcad61471f7be'
final_revision: '9cfe098'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-19-context.md'
warnings: [oversized]
---

<intent-contract>

## Intent

**Problem:** System audio is captured but the toggle is hardcoded on — `recording_start` sets `system_audio: true` and the manifest `SessionDevices.system_audio: true` unconditionally (`ipc.rs:3461`/`:3488`), and the Audio setup card is a placeholder ("Configured in a later update"). A user cannot record a silent screen capture (no content audio), and nothing on the surface discloses that system audio is content-audio, is a separate AAC track (never a mix), excludes keeper's own notification sounds, and is scoped to the same capture target as the video.

**Approach:** Land the system-audio leg. Build the Audio card's System-audio `Switch` (default on) with content-audio labelling and the separate-tracks / keeper-excluded disclosure, and thread a per-session toggle from a new ephemeral store through `recording_start` (an added `system_audio: Option<bool>` param, `None` → `true`) into `SessionParams.system_audio` (already serialized to the sidecar as `systemAudio`) and the manifest `SessionDevices.system_audio`. The sidecar already scopes audio to the active `SCContentFilter` (app or display) with `excludesCurrentProcessAudio` and writes a separate AAC track (16.6 + 19.1); 19.2 activates the toggle end-to-end and discloses the scoping. Mic (19.3) and destination/fps (19.5) stay out of scope.

## Boundaries & Constraints

**Always:**
- The Audio card renders a shadcn `Switch` for **System audio, default on each session** (ephemeral — the store defaults to `true` on load, never remembered), labelled as content-audio ("the audio the recorded content plays"), not a device pick. Copy voice: sentence case, no exclamation, no "please"; "Recording Session"/"segment" capitalized where they appear.
- Inline disclosure states plainly that **system audio and microphone are separate tracks, not a mix**, and that **keeper's own notification sounds are excluded**. When the toggle is **off**, the card honestly states the recording will have no content audio.
- The toggle is **per-session**, threaded through `recording_start` exactly like 19.1's source target (a new `system_audio: Option<bool>` command param; `None` preserves the 16.6 default-on path). The threaded value flows to **both** `SessionParams.system_audio` (the sidecar wire) **and** the manifest `SessionDevices.system_audio`, so an off session writes `devices.systemAudio = false`.
- **Per-app / per-display audio scoping is the same `SCContentFilter` that scopes video (19.1) plus `excludesCurrentProcessAudio` (16.6)** — keeper's own audio, and for an application target other apps' audio, never enter the file. No new Swift capture logic; 19.2 confirms and discloses this. When system audio is off the sidecar already adds no audio track and does not set `capturesAudio` (16.6 made those conditional on `systemAudio`).
- Wire logic stays **platform-free** in `keeper-core::recording` — `systemAudio` is already serialized; `dependency_firewall_holds` must stay green. Any Apple/SCK code stays in the sidecar and `keeper/src/recorder.rs` under `#[cfg(desktop)]`.
- New/changed VMs (if any) derive `serde` + `ts_rs::TS` with camelCase + `#[ts(export)]`; regenerated `.ts` in `src/lib/ipc/gen/` is committed, never hand-edited (AD-7). The Tauri command arg is `system_audio` in Rust ↔ `systemAudio` in JS `invoke` (the codebase's `account_id`↔`accountId` convention).

**Block If:**
- Toggling system audio off, or scoping audio to the selected application, cannot be expressed without raising the `recording` capability floor / `minimumSystemVersion`. Surface, do not raise the floor.
- Per-app audio scoping requires a TCC permission or entitlement **beyond** the existing Screen Recording grant. Surface rather than proceed. (ScreenCaptureKit audio rides the Screen Recording grant; there is no microphone permission here — mic is 19.3.)

**Never:**
- No microphone toggle/picker/permission (19.3), no destination-folder chooser or fps (19.5), no webcam (20.1). 19.2 changes the **system-audio** leg only.
- **No DB persistence of the system-audio toggle and no Settings → Recording mirror for it** — the epic scopes DB persistence + Settings mirroring to segmentation (17.5) and folder/fps (19.5); system audio is "default on" per session, mirroring 19.1's ephemeral source selection. (Documented deliberately so it is not read as an intent gap.)
- No premixing of system audio with any other source — separate AAC tracks (already the case; do not change the writer). No modification to 19.1's source-picker disclosure.
- No new network destination, upload, telemetry, or preview (local-only, FR-76). No hand-edited generated `.ts`. No `.unwrap()`/bare `.expect()` in Rust production paths; no `any` in TS.
- No claim that on-hardware audio scoping is sample-verified here — the sample-level "only the target's audio is in the file / keeper's sounds absent" check folds into SM-9/SM-10 acceptance (Story 20.6), like every real-capture leg since 16.6. 19.2's automated gates are compile + unit + Swift release build.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Default start | `recording_start` with `system_audio` omitted (`None`) | `SessionParams.system_audio = true`; wire `"systemAudio": true`; manifest `devices.systemAudio = true` — the 16.6 path unchanged | No error |
| Toggle off then start | Audio Switch off → Start | `recording_start(system_audio: Some(false))`; wire `"systemAudio": false`; manifest `devices.systemAudio = false`; sidecar adds no audio track | No error |
| Toggle on (explicit) | Audio Switch on → Start | `system_audio: Some(true)`; wire/manifest `true` | No error |
| App-scoped + audio on | application target selected, system audio on | same app-scoped `SCContentFilter` scopes both video and audio; `excludesCurrentProcessAudio` drops keeper's sounds | No error |
| Card default render | setup surface idle, first render | Switch is **checked** (store default `true`); disclosure copy visible | No error |
| iOS / sidecar absent | `recording_start` on non-desktop | `Unsupported`, no spawn/panic; `system_audio` param accepted and ignored downstream | honest |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper/src/ipc.rs` -- `recording_start` (~line 3409): add param `system_audio: Option<bool>`; resolve `let system_audio = system_audio.unwrap_or(true);` and use it for **both** `SessionDevices { system_audio, … }` (~line 3461) and `SessionParams { … system_audio, … }` (~line 3488), replacing the two hardcoded `true`s. No other command change; `recording_settings_get/set` are untouched (audio is not persisted).
- `src-tauri/crates/keeper-core/src/recording.rs` -- **no production change needed**: `SessionParams.system_audio` exists (line 396) and `start_recording_request` already emits `"systemAudio"` (line 415). Add/extend a unit test for the `system_audio: false` wire shape (see Tasks). `dependency_firewall_holds` (line ~2642) stays green — no new tokens.
- `tools/keeper-rec/Sources/keeper-rec/Capture.swift` -- **no functional change**: `capturesAudio`/`excludesCurrentProcessAudio` are already conditional on `systemAudio` (lines 270-276) and the AAC track is already conditional + separate (lines 336-363), scoped by the shared `SCContentFilter` (lines 251-282). Update only the stale comment (~lines 253-255) that says "19.2 owns per-app audio scoping" to state that per-app audio scoping is now active via the shared filter + `excludesCurrentProcessAudio`. Doc-only.
- `src/lib/stores/recording-audio.ts` -- NEW vanilla zustand store mirroring `recording-source.ts` conventions: `{ systemAudioEnabled: boolean }` defaulting to `true`; `useSystemAudioEnabled()` hook, `systemAudioEnabled()` imperative read (for the header Start click), `setSystemAudioEnabled(enabled)`, and `resetRecordingAudioForTest()`. Ephemeral UI state — never persisted, never mirrored to a Rust stream.
- `src/components/recording/recording-audio-controls.tsx` -- NEW setup-surface component (sibling of `recording-source-picker.tsx`): a `Switch` bound to the store (checked by default), a "System audio" label + content-audio caption ("the audio the recorded content plays"), the separate-tracks / keeper-excluded disclosure, and an honest off-state line. Export label/testid constants. This lives on the setup surface only (unlike `RecordingSettingsControls`, which is shared with Settings because it is persisted).
- `src/lib/ipc/client.ts` -- `recordingStart(target?, systemAudio?: boolean)` (~line 1695): pass `{ target: target ?? null, systemAudio: systemAudio ?? null }` to `invoke`. Update the doc comment.
- `src/hooks/use-recording-session.ts` -- `start(target?, systemAudio?: boolean)` (~line 133): forward `systemAudio` to `recordingStart`.
- `src/components/layout/recording-pane.tsx` -- add a `title === "Audio"` branch in the `SETUP_CARDS.map` (mirror the `"Source"`/`"Segmenting"` specialization) rendering `<RecordingAudioControls />`; the header Start (~line 93) passes `systemAudioEnabled()` as the second arg to `start(...)`.
- Tests: `recording.rs` unit test (`system_audio: false` wire); `recording-audio.test.ts`; `recording-audio-controls.test.tsx`; update `recording-pane.test.tsx` (Audio card renders the controls; Start threads the audio value).

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- `recording_start` gains `system_audio: Option<bool>`; `unwrap_or(true)` threads it into `SessionDevices.system_audio` and `SessionParams.system_audio` (replace both hardcoded `true`s) -- activates the toggle end-to-end in Rust.
- [x] `src-tauri/crates/keeper-core/src/recording.rs` (tests) -- extend the `start_recording_request` coverage with a `system_audio: false` case asserting wire `"systemAudio": false` (and keep the default-`true` case); confirm `dependency_firewall_holds` still passes -- no-hardware wire coverage.
- [x] `tools/keeper-rec/Sources/keeper-rec/Capture.swift` -- update the stale "19.2 owns per-app audio scoping" comment to reflect that per-app/per-display audio scoping is now active via the shared `SCContentFilter` + `excludesCurrentProcessAudio` -- keeps the codebase honest; doc-only, no behavior change.
- [x] `src/lib/stores/recording-audio.ts` (+ `.test.ts`) -- NEW ephemeral store (`systemAudioEnabled` default `true`, hook + imperative read + setter + `resetRecordingAudioForTest`) -- shared toggle state between the Audio card and header Start.
- [x] `src/components/recording/recording-audio-controls.tsx` (+ `.test.tsx`) -- NEW Audio card body: default-on `Switch`, content-audio label/caption, separate-tracks + keeper-excluded disclosure, honest off-state line, exported labels -- the live Audio card.
- [x] `src/lib/ipc/client.ts` -- `recordingStart(target?, systemAudio?)` passes `systemAudio` to `invoke` -- carries the toggle to Rust.
- [x] `src/hooks/use-recording-session.ts` -- `start(target?, systemAudio?)` forwards to `recordingStart` -- threads the value from the pane.
- [x] `src/components/layout/recording-pane.tsx` -- mount `<RecordingAudioControls />` in the `"Audio"` branch; Start passes `systemAudioEnabled()` (update `recording-pane.test.tsx`) -- wires the card into the surface.

**Acceptance Criteria:**
- Given the idle setup surface, when the Audio card renders, then it shows a System-audio `Switch` **on by default**, labelled as content-audio (not a device), with inline disclosure that system audio and microphone are separate tracks (not a mix) and keeper's own notification sounds are excluded.
- Given the user turns System audio off and starts a recording, then `recording_start` receives `system_audio: false`, the sidecar wire carries `"systemAudio": false`, the session manifest records `devices.systemAudio = false`, and no audio track is written; turning it back on (or leaving the default) records system audio as a separate AAC track.
- Given an application target is selected with system audio on, when a recording starts, then audio is scoped to that application via the same `SCContentFilter` and keeper's own process audio is excluded (`excludesCurrentProcessAudio`) — sample-level isolation folds into 20.6.
- Given the sidecar is unavailable or the platform is iOS, when `recording_start` is called with any `system_audio`, then it returns `Unsupported` with no spawn/panic, and `keeper-core::recording` still carries no tauri/Apple/process token (`dependency_firewall_holds` passes).
- Given `bun run check`, `bun run check:rust`, `bun run test:rust`, and `bash scripts/build-keeper-rec.sh`, then biome/tsc/vitest, clippy (`-D warnings`), cargo-nextest (with any regenerated committed `.ts`), and the Swift release build all pass.

## Spec Change Log

_Empty — no bad_spec loopback was triggered._

## Review Triage Log

### 2026-07-19 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 1: (high 0, medium 0, low 1)
- defer: 1: (high 0, medium 1, low 0)
- reject: 1
- addressed_findings:
  - `[low]` `[patch]` The `recording-audio` store test's "exposes a hook selector" case was vacuous — it only asserted `typeof useSystemAudioEnabled === "function"` then read the store directly, never rendering the hook. Rewrote it to `renderHook(() => useSystemAudioEnabled())` and assert `result.current` tracks store flips (true → false → true) under `act`, so the hook's reactive subscription is directly covered.

_Deferred:_ `[medium]` the Source-picker app-scope disclosure ("Only {App}'s windows **and audio** are recorded") is unconditional, so an application target + the new system-audio-off toggle contradicts the Audio card's honest "no content audio" off-note on the same screen. Real and exposed by 19.2, but the read-only intent-contract explicitly forbade modifying 19.1's source-picker disclosure ("Never"), so the coherent fix (conditioning the "and audio" clause on the toggle) was out of scope this story — logged to the deferred-work ledger for Story 19.3 (which reworks the Audio card) or a focused copy pass. Recording behavior is correct (session writes `devices.systemAudio=false`, no audio track); only the sibling copy is stale.

_Rejected (noise / pre-existing):_ the new `system_audio: Option<bool>` param's iOS/non-desktop "accept and ignore" path lacks a dedicated test — the `cargo check --target aarch64-apple-ios` gate passes (the new signature compiles) and the `IosRecorder` returns `Unsupported` via the `Recorder` port, exactly the pre-existing honest-`Unsupported` pattern Story 19.1 rejected the equivalent finding for; no new test warranted.

## Design Notes

**Ephemeral toggle, not a persisted setting.** The system-audio Switch mirrors 19.1's source selection: a small vanilla zustand store, default-on each session, read imperatively at the header Start click and threaded through `recording_start` as a new `Option<bool>` param (`None` → `true`, preserving the 16.6 default). It is deliberately **not** persisted to `keeper.db` and **not** mirrored into Settings → Recording — the epic reserves DB persistence + Settings mirroring for segmentation (17.5) and folder/fps (19.5). "Default on" (no "remembered" language) is the epic's wording for this toggle. Keeping it out of `RecordingSettingsVm`/`recording_settings_*` avoids conflating a per-session content choice with the persisted segmentation settings.

**The sidecar half is already done.** 16.6 made `capturesAudio`, `excludesCurrentProcessAudio`, and the separate AAC track conditional on the `systemAudio` request field (default `true`), and 19.1's app-scoped `SCContentFilter` already scopes the audio it captures. So per-app audio scoping is emergent from filter + `excludesCurrentProcessAudio`, and off-means-no-track already works. 19.2's only sidecar touch is a comment correction — the real work is un-hardcoding the value in Rust and building the UI. Sample-level verification (only the app's audio, keeper's sounds absent) is 20.6, consistent with every real-capture leg since 16.6.

**Threading shape (JS → Rust):**
```
// recording-pane.tsx (header Start)
void start(selectedRecordingTarget(), systemAudioEnabled());
// client.ts
invoke("recording_start", { target: target ?? null, systemAudio: systemAudio ?? null });
// ipc.rs
let system_audio = system_audio.unwrap_or(true); // → SessionDevices + SessionParams
```

## Verification

**Commands:**
- `cd src-tauri && cargo test -p keeper-core recording` -- expected: the `system_audio: false` wire test + `dependency_firewall_holds` pass.
- `bun run test:rust` -- expected: cargo-nextest green (regenerate + commit any `.ts` if a VM changed — none expected in 19.2).
- `bun run check:rust` -- expected: `cargo fmt --check` + clippy `--all-targets -D warnings` clean.
- `bun run check` -- expected: biome + tsc + vitest pass (new store/controls tests + updated recording-pane test).
- `bash scripts/build-keeper-rec.sh` -- expected: `swift build -c release --arch arm64` + smoke green (comment-only Swift change).
- `cd src-tauri && cargo check --workspace --target aarch64-apple-ios` -- expected: compiles; `recording_start` accepts `system_audio` and the iOS recorder still returns `Unsupported`.

**Manual checks:**
- Confirm `keeper-core/src/recording.rs` still imports no tauri/Apple-framework/process API, and the Audio card copy is honest in both on and off states (no "no audio" claim while on, no "audio recorded" claim while off).

## Auto Run Result

Status: done

**Summary.** Implemented Story 19.2 — the System-audio toggle. The `recording_start` Tauri command gains a `system_audio: Option<bool>` param (`unwrap_or(true)`, preserving the 16.6 default-on path) threaded into **both** `SessionParams.system_audio` (already serialized to the sidecar as `systemAudio`) and the manifest `SessionDevices.system_audio` — un-hardcoding the two `true`s so an off session writes `devices.systemAudio=false` and the sidecar adds no audio track. A new ephemeral vanilla-zustand store (`recording-audio.ts`, default on, never persisted) feeds a new Audio setup card (`RecordingAudioControls`): a default-on `Switch` labelled as content-audio, with an on-state disclosure (system audio + mic are separate AAC tracks, never mixed; keeper's own notification sounds excluded) and an honest off-state note (no content audio). The header Start reads the toggle imperatively and threads it through `recordingStart(target?, systemAudio?)` → `use-recording-session.start`. The Swift sidecar already scoped audio to the shared `SCContentFilter` + `excludesCurrentProcessAudio` and wrote a separate AAC track (16.6/19.1), so per-app/per-display audio scoping is emergent — only a stale comment was corrected there. No DB persistence / Settings mirror (deliberate, per epic scoping); mic (19.3) and destination/fps (19.5) stay out of scope.

**Files changed.**
- `src-tauri/crates/keeper/src/ipc.rs` — `recording_start` gains `system_audio: Option<bool>`; `unwrap_or(true)` threaded into `SessionDevices.system_audio` + `SessionParams.system_audio` (both hardcoded `true`s replaced).
- `src-tauri/crates/keeper-core/src/recording.rs` — new unit test `start_recording_request_carries_system_audio_off` (wire `"systemAudio": false`); no production change (`start_recording_request` already emits the field); firewall stays green.
- `tools/keeper-rec/Sources/keeper-rec/Capture.swift` — doc-only: the stale "19.2 owns per-app audio scoping" comment now states scoping is active via the shared filter + `excludesCurrentProcessAudio`.
- `src/lib/stores/recording-audio.ts` (+ `.test.ts`) — new ephemeral toggle store (default on; hook + imperative read + setter + `resetRecordingAudioForTest`); test strengthened in review to render the hook and assert reactivity.
- `src/components/recording/recording-audio-controls.tsx` (+ `.test.tsx`) — new Audio card body (default-on Switch, content-audio label/caption, on/off disclosure, exported labels/testid).
- `src/lib/ipc/client.ts` — `recordingStart(target?, systemAudio?)` passes `{ target, systemAudio: systemAudio ?? null }`.
- `src/hooks/use-recording-session.ts` — `start(target?, systemAudio?)` forwards to `recordingStart`.
- `src/components/layout/recording-pane.tsx` (+ `.test.tsx`) — mounts `<RecordingAudioControls />` in the `"Audio"` branch; Start threads `systemAudioEnabled()`.

**Review findings breakdown.** 1 patch applied (low: strengthened a vacuous store-hook test into a real `renderHook` reactivity assertion). 1 deferred (medium: the 19.1 Source-picker's unconditional "…and audio are recorded" disclosure contradicts the new audio-off state for an application target — the read-only intent-contract forbade touching that disclosure, so it was logged to the deferred-work ledger for 19.3/a copy pass; recording behavior is correct). 1 rejected (iOS `system_audio` accept-and-ignore path — the iOS `cargo check` gate passes and the `Recorder` port returns `Unsupported`, the pre-existing pattern 19.1 rejected the equivalent for). 0 intent_gap, 0 bad_spec (no repair loopback). Details in the Review Triage Log.

**Follow-up review recommended:** false — the final pass made only one low-severity, localized test-only change (no behavior/API/data impact) and deferred one medium copy issue; not significant enough to warrant an independent follow-up.

**Verification.** All gates independently re-run green: `bun run check:rust` (fmt + clippy `-D warnings`) clean; `bun run test:rust` → 891/891 (incl. `start_recording_request_carries_system_audio_off` + `dependency_firewall_holds`; one flaky palette latency test failed once under CPU contention and passed cleanly in isolation and on a contention-free rerun); `bun run check` → biome + tsc + vitest 1345/1345 + `check:core-tauri-free` clean (re-run after the review patch); `bash scripts/build-keeper-rec.sh` → Swift release build + all smoke checks green; `cargo check --workspace --target aarch64-apple-ios` → compiles (only pre-existing dead-code warnings). Manual: `keeper-core/src/recording.rs` carries no tauri/Apple/process token; Audio-card copy is honest in both on and off states.

**Residual risks.** (1) The deferred Source-picker "…and audio" copy contradiction in the app-target + audio-off state (behavior correct, copy stale) — logged to the ledger. (2) Sample-level audio scoping (only the target's audio in the file, keeper's sounds absent) is not automatically verified here — it folds into SM-9/SM-10 on-hardware acceptance (Story 20.6), like every real-capture leg since 16.6. 19.2's automated gates are compile + unit + Swift release build.
