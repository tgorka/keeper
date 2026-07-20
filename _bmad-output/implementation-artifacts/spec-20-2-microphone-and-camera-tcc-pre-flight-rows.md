---
title: 'Story 20.2: Microphone & Camera TCC Pre-flight Rows'
type: 'feature'
created: '2026-07-19'
status: 'done'
baseline_revision: 'ddd9cec7af8c942adc3303311fdc1f393c75822a'
final_revision: 'f1890e8'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-20-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** The recording pre-flight (Story 16.5) tracks only Screen Recording. When a user enables the microphone (19.3) or webcam (20.1), those sources request TCC lazily on enable but have **no honest, fixable permission surface** and **do not gate Start** — so a user can start a recording whose enabled mic/camera is denied, silently getting no track. FR-67/AD-36/UX-DR33 require the pre-flight to cover mic and camera the moment they are turned on.

**Approach:** Add a Microphone row and a Camera row to the existing Permissions surface, each shown **only when its source is enabled**, live-detected from the *same* `getCapabilities` probe the screen row already runs (no new sidecar RPC — `PROTOCOL_VERSION` stays `1`), lifted to the existing `granted / notYetRequested / denied` tri-state, with a deep-link fix-path (`Privacy_Microphone` / `Privacy_Camera`) when only manual granting remains. Extend `can_start` so an **enabled source that is not granted becomes a blocking permission**: Start is disabled and names it. All resolution lives in a pure, unit-tested `keeper-core` function.

## Boundaries & Constraints

**Always:**
- Mic/Camera legs are resolved from the **existing** `recorder.get_capabilities()` probe (a fresh child `keeper-rec` per call — live at render, re-detected on focus/return and whenever the enabled state changes; never cached optimistically). `AVCaptureDevice.authorizationStatus` and `CGPreflightScreenCaptureAccess` are both **non-prompting**.
- The Microphone row renders **only when mic is enabled**; the Camera row **only when webcam is enabled** (camera row absent when webcam off). Enabled state is passed from the frontend stores (`useMicEnabled`/`useWebcamEnabled`) to the `recording_permission` command as `mic_enabled`/`camera_enabled`.
- Mic/Camera map directly (no session flag, unlike screen's 2-valued preflight): `Granted→Granted`, `Denied→Denied`, `NotDetermined→NotYetRequested`.
- `can_start` = screen `Granted` **and** (mic disabled **or** `Granted`) **and** (camera disabled **or** `Granted`). Start is disabled and names the highest-priority blocker (Screen Recording → Microphone → Camera). Logic lives in a pure `keeper-core` fn (unit-tested), not the shell.
- The OS prompt is issued only on source **enable** (existing 19.3/20.1) or the row's explicit **Request permission** action — **never** from the probe/render, never preemptively.
- Deep-link to the exact pane via `platform.open_url` (through the Rust opener, bypassing JS scope), mirroring `open_screen_recording_settings`: `x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone` and `…?Privacy_Camera`.
- `keeper-core` stays firewall-clean (no tauri/Apple/tokio/process token); regenerate and commit the ts-rs binding for `RecordingPermissionVm`. Recording voice everywhere (sentence case, no exclamation marks, honest local-only framing). Never use the `recording-red` token on any row/pill/button.
- `NSMicrophoneUsageDescription` / `NSCameraUsageDescription` remain present in keeper's bundle `Info.plist` (already added in 19.3/20.1, merged by Tauri's colocated-`Info.plist` convention next to `tauri.conf.json`) — verify they land, add nothing new.

**Block If:**
- Making mic/camera pre-flight honest appears to require a **new sidecar RPC or a `PROTOCOL_VERSION` bump** (it must not — `getCapabilities` already reports `microphone`/`camera` TCC), or caching a grant across sessions.
- Real hardware TCC state that unit tests + the fake sidecar cannot simulate — do not fake a grant; HALT.

**Never:**
- Never preemptively request mic/camera from the probe or from row render.
- Never bump `PROTOCOL_VERSION`; never add a sidecar RPC (no wire change needed).
- Never use `recording-red` on the permission rows/pills/buttons.
- Never add a network destination, upload, or share affordance.
- Never persist mic/camera permission state across sessions (they are live-probed; only screen keeps its per-lifetime "already requested" flag).
- No real-hardware grant validation here — live TCC on a Development-signed Mac is SM-9/SM-10 (Story 20.6). This story ships code + unit/fake-sidecar/frontend tests only.
- Never put a tauri / Apple-framework / process / tokio token into `keeper-core`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Mic off (default) | `mic_enabled=false` | `microphone: null`; no Mic row; mic never affects `can_start` | No error |
| Camera off (default) | `camera_enabled=false` | `camera: null`; no Camera row | No error |
| Mic on, granted | `mic_enabled=true`, caps.microphone=`granted` | `microphone: "granted"`; Mic row green, no action; not blocking | No error |
| Mic on, denied | caps.microphone=`denied` | `microphone: "denied"`; Mic row shows **Open System Settings** (`Privacy_Microphone`); `can_start=false`, Start names Microphone | No error |
| Mic on, not determined | caps.microphone=`notDetermined` | `microphone: "notYetRequested"`; Mic row shows **Request permission** (`request_microphone_permission`); `can_start=false`, Start names Microphone | No error |
| Camera on, denied | caps.camera=`denied` | `camera: "denied"`; Camera row shows **Open System Settings** (`Privacy_Camera`); Start names Camera | No error |
| Screen denied + mic granted | screen=`denied`, mic enabled+granted | `can_start=false`; Start names **Screen Recording** (highest priority) | No error |
| Screen granted + mic granted + camera denied | all enabled | `can_start=false`; Start names **Camera** | No error |
| `resolve_source_access` (pure) | `granted` / `denied` / `notDetermined` | `Granted` / `Denied` / `NotYetRequested` | Total fn, no panic |
| `open_microphone_settings` / `open_camera_settings` | invoked | `platform.open_url(Privacy_Microphone / Privacy_Camera)` | maps `to_ipc_error`; caller best-effort |
| Sidecar unavailable / hung / iOS | `get_capabilities`→Unsupported/timeout | clean error; frontend swallows → safe default (Start disabled, no rows claimed granted) | timeout → error |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/vm.rs` -- extend `RecordingPermissionVm` (vm.rs:2703) with `microphone: Option<ScreenRecordingAccess>` and `camera: Option<ScreenRecordingAccess>` (ts-rs, camelCase, `Some` iff that source is enabled; refresh the "Screen Recording alone" doc comment). `ScreenRecordingAccess` is reused as the shared tri-state.
- `src-tauri/crates/keeper-core/src/recording.rs` -- NEW pure `resolve_source_access(tcc: vm::TccPermission) -> vm::ScreenRecordingAccess` (`Granted→Granted`, `Denied→Denied`, `NotDetermined→NotYetRequested`) and NEW pure `resolve_recording_permission(screen: ScreenRecordingAccess, microphone: Option<ScreenRecordingAccess>, camera: Option<ScreenRecordingAccess>) -> vm::RecordingPermissionVm` computing `can_start` (screen Granted ∧ each enabled leg Granted). Unit-test every `resolve_source_access` branch and the `can_start` matrix (each leg None/Granted/Denied/NotYetRequested). Firewall guard (`dependency_firewall_holds`) must still pass.
- `src-tauri/crates/keeper/src/ipc.rs` -- `recording_permission` (ipc.rs:3268) and `request_screen_recording_permission` (ipc.rs:3300) gain `mic_enabled: bool` / `camera_enabled: bool` args; both resolve all three legs from the `get_capabilities` probe via `resolve_recording_permission(screen, mic_enabled.then(|| resolve_source_access(caps.microphone)), camera_enabled.then(|| resolve_source_access(caps.camera)))`. Replace the shell `recording_permission_vm` helper (ipc.rs:3251) with the core fn. Add `open_microphone_settings` and `open_camera_settings` commands mirroring `open_screen_recording_settings` (ipc.rs:3329) with the `Privacy_Microphone` / `Privacy_Camera` URLs.
- `src-tauri/crates/keeper/src/lib.rs` -- register `open_microphone_settings` and `open_camera_settings` in `generate_handler!` (near lib.rs:364).
- `src/lib/ipc/client.ts` -- `recordingPermission(micEnabled, cameraEnabled)` and `requestScreenRecordingPermission(micEnabled, cameraEnabled)` pass the two flags; add `openMicrophoneSettings()` / `openCameraSettings()` wrappers (mirror `openScreenRecordingSettings`, client.ts:1683). `requestMicrophonePermission`/`requestCameraPermission` already exist.
- `src/hooks/use-recording-permission.ts` -- subscribe to `useMicEnabled()`/`useWebcamEnabled()`, thread them into `recordingPermission`/`requestScreenRecordingPermission`, and re-fetch when either flag changes (add to the deps alongside the existing mount + focus/visibility re-detect and monotonic `seq` guard). Expose `requestMicrophone`/`requestCamera` (call the existing request commands, then `refresh()`) and `openMicrophoneSettings`/`openCameraSettings`.
- `src/components/recording/recording-permission-row.tsx` -- generalize: accept `name: string`, `access: ScreenRecordingAccess`, `notes?: string[]`, `onRequest`, `onOpenSettings` (keep the pill/action/label logic and exported constants). Screen passes its existing note-lines; mic/camera pass their own honest note-lines.
- `src/components/layout/recording-pane.tsx` -- in the Permissions card, render `<RecordingPermissionRow name="Microphone" .../>` when `permission.microphone != null` and `name="Camera"` when `permission.camera != null`; extend the disabled-Start note to name the highest-priority blocking permission (Screen Recording → Microphone → Camera) instead of the hard-coded "Screen Recording".
- `src/components/recording/recording-audio-controls.tsx` + `recording-webcam-controls.tsx` -- reconcile the denied captions (which currently promise a silent/absent track) with the new reality that an enabled+denied source blocks Start; keep the fix instruction, drop the "will be silent / no camera file" claim.
- `src/lib/ipc/gen/RecordingPermissionVm.ts` -- GENERATED by `bun run test:rust`; commit, never hand-edit.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- add `microphone`/`camera` `Option<ScreenRecordingAccess>` legs to `RecordingPermissionVm` (ts-rs, camelCase, doc comments) -- the code-owned three-class contract (FR-67/AD-36).
- [x] `src-tauri/crates/keeper-core/src/recording.rs` -- add pure `resolve_source_access` and `resolve_recording_permission` (can_start over three legs) -- platform-free multi-source gate logic, firewall intact.
- [x] `src-tauri/crates/keeper-core/src/recording.rs` (tests) -- unit-test every `resolve_source_access` branch and the `can_start` matrix (each leg None / Granted / Denied / NotYetRequested, plus blocker-priority ordering) -- hardware-free coverage of the I/O matrix.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- thread `mic_enabled`/`camera_enabled` into `recording_permission` + `request_screen_recording_permission`, resolve all three legs via the core fn, and add `open_microphone_settings` / `open_camera_settings` (Privacy_Microphone / Privacy_Camera) -- the shell wiring, no new sidecar round-trip.
- [x] `src-tauri/crates/keeper/src/lib.rs` -- register the two new deep-link commands.
- [x] `src/lib/ipc/client.ts` -- pass the enabled flags on the two existing wrappers; add `openMicrophoneSettings` / `openCameraSettings`.
- [x] `src/hooks/use-recording-permission.ts` -- subscribe to the enabled stores, re-fetch on enabled change, expose mic/camera request + openSettings actions (preserve the stale-probe `seq` guard) -- live, never-cached mic/camera legs.
- [x] `src/components/recording/recording-permission-row.tsx` -- generalize to `name`/`access`/`notes`/`onRequest`/`onOpenSettings` -- one honest row shape reused by all three permissions.
- [x] `src/components/layout/recording-pane.tsx` -- render the Mic/Camera rows only when enabled and name the blocking permission on the disabled Start note.
- [x] `src/components/recording/recording-audio-controls.tsx` + `recording-webcam-controls.tsx` -- reconcile the denied captions with the new blocking behavior.
- [x] Tests: extend `recording-permission-row.test.tsx` (mic/camera name + notes + actions), `use-recording-permission.test.tsx` (mic/camera legs, re-fetch on enabled change, request/openSettings, can_start blocking), `recording-pane.test.tsx` (rows appear only when enabled; Start disabled naming Microphone/Camera), and the audio/webcam controls tests for the caption change. Mock the new client fns via `vi.mock("@/lib/ipc/client")`.

**Acceptance Criteria:**
- Given the pre-flight surface renders, when microphone and/or webcam is enabled, then a Microphone row and/or a Camera row appears, each live-detected at render from the `getCapabilities` probe (never cached) and re-detected on focus/return and on enabled-state change, with the Camera row absent whenever webcam is off (FR-67, AD-36, UX-DR33).
- Given an enabled source whose permission is not granted, when the row renders, then it deep-links to the exact System Settings pane (`Privacy_Microphone` / `Privacy_Camera`) when only manual granting remains, and offers **Request permission** (the existing on-enable request command) when the state is not-yet-requested — never issuing a prompt from render itself (FR-67, UX-DR33).
- Given a blocking source permission (an enabled mic or camera that is not granted, or Screen Recording), then Start is disabled and names the highest-priority blocking permission, computed by the pure `keeper-core` resolver; and `NSMicrophoneUsageDescription` / `NSCameraUsageDescription` are present in keeper's bundle `Info.plist` (FR-67, AD-36).
- Given the additive change, then no sidecar RPC is added and the NDJSON `PROTOCOL_VERSION` stays `1`; `keeper-core` remains firewall-clean and the `RecordingPermissionVm` ts-rs binding is regenerated and committed.
- Given a wedged or unavailable sidecar (or iOS), then the pre-flight resolves a clean error within the timeout and the frontend falls back to a safe default (Start disabled, no row claimed granted) with no crash and no infinite spinner.

## Design Notes

**One probe, three legs.** `get_capabilities()` already returns `screen_recording`, `microphone`, and `camera` `TccPermission` live (16.4/19.3/20.1). The existing `recording_permission` command already spawns that probe for the screen row — 20.2 just resolves the other two legs from the same result. No new RPC, no `PROTOCOL_VERSION` change, no extra sidecar spawn per render.

**Mic/camera need no session flag.** `CGPreflightScreenCaptureAccess` is 2-valued, so screen needs the host "already requested" flag to distinguish denied from never-asked. `AVCaptureDevice.authorizationStatus` is a true tri-state, so `resolve_source_access` maps it directly — honest without any persisted flag.

**Start-gating is the new behavior.** 19.3/20.1 deliberately left mic/camera non-blocking (their captions promised a silent/absent track). 20.2's contract ("a blocking source permission → Start disabled naming it"; epic: "until all required grants are green") makes an *enabled* source that isn't granted a blocker: `can_start = screen Granted ∧ (mic None ∨ Granted) ∧ (camera None ∨ Granted)`. Because Start now blocks, the control-row denied captions must stop promising a silent/absent track and instead point at the fix — otherwise the two surfaces contradict.

**Post-prompt refresh comes free.** Enabling a source fires the OS prompt (in the control), which blurs keeper; answering it refocuses keeper → the hook's existing focus/visibility re-detect re-probes and the row + Start reflect the fresh grant. The enabled-change re-fetch makes the row appear immediately when toggled on.

## Verification

**Commands:**
- `bun run test:rust` -- expected: cargo-nextest green incl. new `resolve_source_access` + `resolve_recording_permission` tests; regenerates `src/lib/ipc/gen/RecordingPermissionVm.ts` (commit it).
- `bun run check:rust` -- expected: `cargo fmt --check` + clippy `--all-targets -D warnings` clean across both crates (no `.unwrap()` in production paths).
- `bun run check` -- expected: biome + tsc + vitest green (permission-row / hook / pane / audio / webcam tests).
- `cd src-tauri && cargo check --workspace --target aarch64-apple-ios` -- expected: compiles; iOS commands stay `Unsupported`/no-op.

**Manual checks:**
- Confirm `keeper-core/src/recording.rs` imports no tauri/Apple/process/tokio token (firewall test green).
- Confirm the built app's `Info.plist` carries `NSMicrophoneUsageDescription` and `NSCameraUsageDescription` (already merged; real TCC attribution is Story 20.6 hardware).
- Confirm no change under `tools/keeper-rec/` and that `getCapabilities` still reports `protocolVersion: 1`.

## Review Triage Log

### 2026-07-19 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 3: (high 0, medium 1, low 2)
- defer: 1: (high 1)
- reject: 7
- addressed_findings:
  - `[medium]` `[patch]` Enable-time race: enabling a source fired the OS prompt *and* a concurrent pre-flight probe that read `NotDetermined`; the prompt's answer updated only the setup card's local caption, so the Microphone/Camera pre-flight row and the Start gate stayed stale (blocked) after a successful grant until an incidental window focus/visibility event. The spec's "post-prompt refresh comes free from the focus listeners" assumption was fragile (OS-dependent). Fixed by (a) making the hook's `refresh`/`request` stable and reading the *live* store value imperatively (`micEnabled()`/`webcamEnabled()`) so no callback captures a stale-flag closure, (b) splitting the enabled-change re-probe into its own skip-first effect, and (c) wiring an explicit `onPermissionSettled` re-sync from the Audio/Webcam cards to the pre-flight the moment the enable-request resolves. Added a `recording-pane` test modeling the real ordering (enable-time probe reads `notYetRequested` while the prompt is pending → post-prompt re-sync reads `granted`, with no focus event) — also closing the prior test's pre-seeded-grant coverage gap.
  - `[low]` `[patch]` `request_screen_recording_permission`'s second `getCapabilities` leg-probe `?`-propagated, so a transient probe failure discarded an already-successful screen-recording grant and collapsed the whole request to the safe default. Made it non-fatal: on the leg-probe's `Err`, degrade the unconfirmed enabled legs to `NotYetRequested` (Start stays honestly blocked, never falsely unlocked) and still return the resolved screen outcome; a later live probe reconciles.
  - `[low]` `[patch]` `blockingPermissionName` could return `null` while `can_start` is `false` (a latent divergence between the TS name-derivation and the Rust `can_start`), leaving a disabled Start with no note. Added a `?? SCREEN_RECORDING_PERMISSION_NAME` fallback so a disabled Start always names what to fix.

_Deferred (1, tracked to 20.6 — see deferred-work.md):_ the Start gate is UI-only — `recording_start` re-enforces neither screen, mic, nor camera permission server-side, so a session reaching `recording_start` with an enabled-but-ungranted source (a revoke between the last probe and the click, or a direct IPC invoke) still produces a silent mic track / missing camera file. This matches the existing 16.5 UI-only screen gate and is beyond this UI story's scope; the induced permission-revoke-mid-record loud-failure is SM-10 hardware acceptance (Story 20.6).

_Rejected (7, by-design / mandated / noise):_ the enabled+denied Start block "reverses" 19.3/20.1 non-blocking behavior (spec/epic/AC-mandated — "until all required grants are green"; the reconciled captions give the fix path: grant in Settings or turn the source off); a duplicate `requestMicrophone` from the card enable + the row's Request button (`AVCaptureDevice.requestAccess` is idempotent and the OS coalesces a pending prompt; the row Request is the spec'd not-yet-requested fallback); the `access: ScreenRecordingAccess` prop name being a misnomer on a shared enum + the note-string React key (cosmetic; notes are distinct static constants); `permissionRowTestId` deriving from display copy (breaks loudly at test time, no user consequence); the pre-flight note describing the eventual track/file (honest explanatory copy — the row also shows the live grant state and blocks Start when denied); the hardcoded `Privacy_Microphone`/`Privacy_Camera` deep-link URLs being Rust-untested (mirrors the accepted 16.5 pattern; on-hardware pane verification rides 20.6); and the re-fetch/listener churn on toggles (the re-probe on enabled-change *is* the intended live detection — and the stable-`refresh` rework from the medium patch already removed the listener rebind churn).

### 2026-07-19 — Review pass (follow-up, hook-rework re-review)
- intent_gap: 0
- bad_spec: 0
- patch: 0
- defer: 0
- reject: 12
- addressed_findings:
  - none

_Independent follow-up review (Blind Hunter + Edge Case Hunter, fresh context) requested because the prior pass's medium patch reworked the timing-sensitive `use-recording-permission` hook without re-review. Both reviewers independently traced the timing paths and converged: the `keeper-core` resolver and the `seq` last-initiated-wins guard are **sound**; the reworked hook is correct on every path they walked. All 12 distinct findings were rejected after code-verification:_
- _the `seq` guard cannot invert (the post-prompt re-sync is always initiated after the enable-time probe **and** reads newer truth — a fresh `getCapabilities` probe, not the request's return value), so the "double-probe last-writer hazard" is not real; the extra spawn is redundant-but-harmless live detection (prior-pass reject, re-confirmed);_
- _the `request_screen_recording_permission` failed-leg degrade to `NotYetRequested` is transient and self-heals (answering the screen prompt refocuses → the focus listener re-probes live); the proposed "unknown/error" leg state is a spec-level design change beyond this UI story;_
- _the `blockingPermissionName ?? SCREEN_RECORDING_PERMISSION_NAME` fallback is **unreachable** — `can_start` is computed from the identical leg condition `blockingPermissionName` checks, so they cannot drift with the current resolver (defensive dead-code, documented);_
- _the `can_start` server-side-enforcement / trust-boundary finding duplicates the already-owned deferred entry (Start gate is UI-only → 20.6) — not a new finding;_
- _`notes` defaulting to `[]`, the `.finally` lacking a local mounted guard (the wired `refresh` already guards `mounted.current`), the Tauri command-arg shape change (a bundled Tauri app ships JS+Rust as one artifact — no mixed-bundle path), the `requestMicrophone`/`requestCamera` seq-timing and screen-request/enabled-change interleavings (transient, self-correcting via `seq` + the next probe), the disable-path redundant probe, the rapid-toggle spawn burst, the deep-link reliance on focus/visibility (accepted 16.5 pattern), and the "tautological" post-prompt-refresh test (the imperative-live-read design makes the old stale-closure bug unrepresentable; the test still guards that the re-sync fires and threads correct flags) — all speculative future-misuse, cosmetic, or by-design._

## Auto Run Result

Status: done

### Summary
Implemented Story 20.2 — Microphone & Camera TCC pre-flight rows. The recording Permissions surface now grows a Microphone row and/or a Camera row that appear only when their source is enabled, live-detected from the *same* `getCapabilities` probe the screen row already runs (no new sidecar RPC — `PROTOCOL_VERSION` stays `1`), lifted to the shared `granted / notYetRequested / denied` tri-state with a deep-link fix-path (`Privacy_Microphone` / `Privacy_Camera`). The core new behavior: an *enabled* source that is not granted is now a blocking permission — Start is disabled and names the highest-priority blocker (Screen Recording → Microphone → Camera), computed by a pure, unit-tested `keeper-core` resolver (`resolve_source_access` + `resolve_recording_permission`). `NSMicrophoneUsageDescription`/`NSCameraUsageDescription` were already present in the bundle Info.plist. Three review patches (1 medium, 2 low) were applied; one high-flagged item (server-side Start enforcement) was deferred to the 20.6 hardware acceptance as out of this UI story's scope.

### Files changed
- `src-tauri/crates/keeper-core/src/vm.rs` — `RecordingPermissionVm` gains `microphone`/`camera` `Option<ScreenRecordingAccess>` legs (`Some` iff enabled).
- `src-tauri/crates/keeper-core/src/recording.rs` — pure `resolve_source_access` (AV tri-state → access) + `resolve_recording_permission` (can_start over three legs) with unit tests (branch + 3×4×4 can_start matrix + blocker scenarios).
- `src-tauri/crates/keeper/src/ipc.rs` — `recording_permission` + `request_screen_recording_permission` take `mic_enabled`/`camera_enabled` and resolve all legs from one probe; `recording_permission_vm` shell helper removed; `open_microphone_settings`/`open_camera_settings` added; **[review patch]** the request path's leg-probe failure is now non-fatal to a successful screen grant.
- `src-tauri/crates/keeper/src/lib.rs` — registered the two deep-link commands.
- `src/lib/ipc/client.ts` — enabled-flag args on the two wrappers; `openMicrophoneSettings`/`openCameraSettings`.
- `src/hooks/use-recording-permission.ts` — mic/camera legs; **[review patch]** stable `refresh`/`request` reading the live store value imperatively (no stale-flag closure), a dedicated skip-first enabled-change re-probe effect, listeners bound once.
- `src/components/recording/recording-permission-row.tsx` — generalized to `name`/`access`/`notes`/`onRequest`/`onOpenSettings`.
- `src/components/layout/recording-pane.tsx` — renders Mic/Camera rows when present; names the blocking permission on the disabled-Start note; **[review patch]** `?? SCREEN_RECORDING_PERMISSION_NAME` fallback so a blocked Start always shows a note; passes `onPermissionSettled={refresh}` to the cards.
- `src/components/recording/recording-audio-controls.tsx` / `recording-webcam-controls.tsx` — denied captions reconciled with the new blocking behavior; **[review patch]** `onPermissionSettled` re-sync fired once the enable-triggered prompt resolves.
- `src/lib/ipc/gen/RecordingPermissionVm.ts` — regenerated (mic/camera legs).
- Tests: `recording-permission-row`, `use-recording-permission`, `recording-pane` (incl. the new post-prompt-refresh ordering test), audio/webcam control tests.

### Review findings breakdown
- Patches applied: 3 (1 medium, 2 low) — enable-time pre-flight-stale race (hook rework + explicit post-prompt re-sync + new test), non-fatal leg-probe failure in the screen request, blocked-Start note fallback.
- Deferred: 1 (tracked to 20.6) — the Start gate is UI-only; `recording_start` does not re-enforce TCC permission server-side (matches the 16.5 screen gate; revoke-mid-record loud-failure is SM-10 hardware acceptance).
- Rejected: 7 — spec/epic-mandated blocking reversal, benign duplicate prompt, cosmetic type-name/react-key, test-id copy coupling, honest explanatory copy, deep-link URL Rust-test gap (mirrors accepted 16.5), toggle re-fetch churn (intended; churn already reduced by the medium patch).

### Verification
- `bun run check:rust` — PASS (rustfmt + clippy `--all-targets -D warnings`).
- `bun run check` — PASS (biome + tsc + 1437 vitest across 133 files + keeper-core tauri-free firewall check).
- `bun run test:rust` — PASS (959 nextest, incl. the new resolver tests; `RecordingPermissionVm.ts` regenerated).
- `cargo check --workspace --target aarch64-apple-ios` — PASS (only the pre-existing `parse_macos_major` dead-code warning; iOS commands stay `Unsupported`/no-op).
- Manual: `NSMicrophoneUsageDescription`/`NSCameraUsageDescription` present in `src-tauri/crates/keeper/Info.plist`; zero changes under `tools/keeper-rec/`; `getCapabilities` still reports `protocolVersion: 1`.

### Residual risks
- Real end-to-end TCC behavior (live prompt/grant/relaunch, the `Privacy_Microphone`/`Privacy_Camera` pane anchors across macOS 13–15, and an actual mid-record revoke) is validated only by unit/fake-sidecar/frontend tests here — hardware validation is the SM-9/SM-10 acceptance (Story 20.6, AD-38 posture).
- The Start gate is UI-only (deferred item above): a revoke between probe and click, or a direct IPC bypass, can still start an ungranted enabled source with a silent/absent track.
- **A follow-up independent review is recommended:** the medium review patch reworked the timing-sensitive `use-recording-permission` hook (stable `refresh`, imperative live-store reads, split effects, cross-component `onPermissionSettled` re-sync) and was not itself re-run through the adversarial reviewers.

### Follow-up review (2026-07-19)
The recommended independent re-review of the reworked `use-recording-permission` hook was run (Blind Hunter + Edge Case Hunter, fresh context, at session capability). **Outcome: no changes — the implementation stands.** Both reviewers independently traced the timing-critical paths and converged that the `keeper-core` resolver and the `seq` last-initiated-wins guard are sound. All 12 distinct surfaced findings were rejected after code-verification (see the follow-up entry in the Review Triage Log): the seq guard cannot invert; the failed-leg degrade is transient and self-heals via the focus re-probe; the `blockingPermissionName` fallback is unreachable given the resolver; the server-side-enforcement concern duplicates the already-owned 20.6 deferred entry; and the remainder are speculative future-misuse, cosmetic, or by-design. No patch applied, no spec loopback, no new deferred entry. `followup_review_recommended` cleared to `false`.
