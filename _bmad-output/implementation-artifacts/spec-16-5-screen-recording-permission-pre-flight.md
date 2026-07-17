---
title: 'Screen Recording Permission Pre-flight'
type: 'feature'
created: '2026-07-16'
status: 'done'
baseline_revision: '206e13a00826f54cb7550622241519e2d7ed2025'
final_revision: '41ea8073ff6b912c3a02dea3f943cfc72e4263b4'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-16-context.md'
warnings: [oversized]
---

<intent-contract>

## Intent

**Problem:** Story 16.4 gave keeper a `getCapabilities` probe that reports Screen Recording as a *two-valued* preflight (`granted` / `notDetermined`), but there is no frontend consumer, no honest tri-state (granted / not-yet-requested / denied-with-fix-path), no way to trigger the OS prompt or deep-link to System Settings, and no gate on Start. Without them the Recording view can't tell a user why capture won't start, and the story's promise — "never a spinner waiting on a grant that will never come" — is unmet.

**Approach:** Add a `RecordingPermissionVm` (ts-rs) resolved by a pure `keeper-core` function that lifts the two-valued sidecar preflight into the tri-state using a host session "already requested" flag; add a `requestScreenRecording` sidecar RPC (`CGRequestScreenCaptureAccess`) and a `Recorder::request_screen_recording` port method; expose three Tauri commands (fetch / request / open-settings); and render an honest permission pre-flight row in the Recording view — live-detected at render, re-detected on focus/return, with a Start button disabled and naming the blocking permission until the grant is green. Real grant validation on hardware rides Story 16.6.

## Boundaries & Constraints

**Always:**
- Live-detect Screen Recording via the sidecar `CGPreflightScreenCaptureAccess` probe at render *and* re-detect on window focus/return — never cache the grant optimistically.
- Resolve the tri-state with a pure, unit-tested `keeper-core` function; `keeper-core` stays free of the firewall tokens (`dependency_firewall_holds` must pass).
- Spawn `keeper-rec` as a child process (never a LaunchAgent) so TCC attributes the request to keeper; surface keeper's usage string via the macOS bundle Info.plist.
- Bound every pre-flight round-trip with a timeout so a wedged sidecar resolves to a clean error, not an infinite spinner (closes deferred-work item, deferred-work.md:967).
- Regenerate and commit the ts-rs `.ts` bindings for the new VMs; recording voice everywhere (sentence case, no exclamation marks, honest local-only framing).

**Block If:**
- Making the tri-state honest appears to require caching a grant across sessions, or real hardware/TCC state that unit tests + a stub sidecar cannot simulate — do not fake a grant; HALT.
- Adding `NSScreenCaptureUsageDescription` would require restructuring the Tauri bundle in a way that alters codesigning/notarization (16.1's concern) rather than a plain Info.plist merge — HALT.

**Never:**
- Never implement actual capture / Start behavior — Start is only *gated* here; capture is Story 16.6.
- Never use the `recording-red` token on the permission row or any button — it is reserved for the live record dot (16.6).
- Never add a network destination, upload, or share affordance.
- Never bump `PROTOCOL_VERSION` (the new method is additive; keeper and keeper-rec ship in lockstep) and never claim real-hardware grant validation — that is 16.6.
- Never put a tauri / Apple-framework / process token into `keeper-core`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Granted at render | preflight `granted` | `RecordingPermissionVm { screen_recording: granted, can_start: true }`; no action shown; Start enabled | No error |
| Not-yet-requested | preflight `notDetermined`, session `requested=false` | `{ notYetRequested, can_start:false }`; row shows "Request permission"; Start disabled, names Screen Recording | No error |
| Request → prompt → granted | user clicks Request; `request_screen_recording` returns `true` | session flag set; `{ granted, can_start:true }`; Start enabled | No error |
| Request → not granted / prior denial | `request_screen_recording` returns `false` (denied or no prompt shown) | `{ denied, can_start:false }`; row shows "Open System Settings" deep-link | No error |
| Re-detect on focus/return | window becomes visible/focused after a grant in System Settings | pre-flight re-fetched; row flips to `granted` where the OS allows, else the relaunch note-line covers it | No error |
| `resolve_screen_recording_access` (pure) | `(granted,*)` / `(denied,*)` / `(notDetermined,false)` / `(notDetermined,true)` | `Granted` / `Denied` / `NotYetRequested` / `Denied` | Total function, no panic |
| Sidecar unavailable / hung / iOS | `sidecar_path`→Unsupported, or sidecar hangs past the timeout, or `IosRecorder` | command resolves a clean error; frontend swallows → safe default (Start disabled, "Request permission"); never a crash or infinite spinner | timeout → error |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/vm.rs` -- NEW `#[ts(export)]` `ScreenRecordingAccess` enum (`Granted | NotYetRequested | Denied`, camelCase) and `RecordingPermissionVm { screen_recording: ScreenRecordingAccess, can_start: bool }`; follow the exact ts-rs pattern used by `TccPermission`/`RecordingCapabilitiesVm` (vm.rs:2506-2558) with story/AD doc comments.
- `src-tauri/crates/keeper-core/src/recording.rs` -- NEW pure `resolve_screen_recording_access(preflight: vm::TccPermission, requested: bool) -> vm::ScreenRecordingAccess`; pure wire fns `request_screen_recording_request(id: u64) -> String` (method `"requestScreenRecording"`) and `parse_request_screen_recording_result(line: &str) -> Result<bool, RecordingError>` (reads `result.granted`; malformed/`error`→`Protocol`); extend the `Recorder` trait (recording.rs:444-473) with `request_screen_recording(&self) -> impl Future<Output = Result<bool, CoreError>> + Send`; update `FakeRecorder` (recording.rs:989) with a canned impl; add unit tests (all tri-state branches, request-line shape, result parse happy/malformed). Firewall guard (`dependency_firewall_holds`, recording.rs:1191) must still pass.
- `src-tauri/crates/keeper/src/recorder.rs` -- implement `DesktopRecorder::request_screen_recording` via the shared `request_response` helper (recorder.rs:86) + `parse_request_screen_recording_result`; wrap the pre-flight round-trips (`fetch_capabilities` and the new request) in a bounded `tokio::time::timeout` so a hung sidecar yields a clean error; `IosRecorder`→`Unsupported`. Extend the `#[cfg(all(test, desktop))]` fake-sidecar harness (recorder.rs:413) with a request-permission echo case and a **hangs-past-timeout** case.
- `src-tauri/crates/keeper/src/ipc.rs` -- add a cfg-selected `recorder: Arc<PlatformRecorder>` (`#[cfg(desktop)] type PlatformRecorder = DesktopRecorder; #[cfg(target_os="ios")] = IosRecorder`) and a `recording_permission_requested: AtomicBool` to `AppState` (ipc.rs:48-84); construct both in `AppState::new` (ipc.rs:252) from the existing platform. Add three commands: `recording_permission` (async; `get_capabilities` → `resolve_screen_recording_access(caps.screen_recording, flag)`), `request_screen_recording_permission` (async; set flag, call port, re-resolve), and `open_screen_recording_settings` (mirror `ios_open_app_settings` ipc.rs:3148 → `platform.open_url("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")`). Map errors with `to_ipc_error`.
- `src-tauri/crates/keeper/src/lib.rs` -- register the three new commands in `tauri::generate_handler!` (lib.rs:175+).
- `tools/keeper-rec/Sources/keeper-rec/main.swift` -- add a `"requestScreenRecording"` case to the RPC switch (main.swift:145-158) → `CGRequestScreenCaptureAccess()` → `{id, result:{granted: Bool}}`; keep SIGPIPE/no-forced-unwrap safety. No `PROTOCOL_VERSION`/`getCapabilities` change.
- `src-tauri/crates/keeper/Info.plist` -- NEW Tauri macOS Info.plist merge file colocated with `tauri.conf.json` (src-tauri/crates/keeper/tauri.conf.json), adding `NSScreenCaptureUsageDescription` (honest local-only string). Verify it lands in the built app's Info.plist.
- `src/lib/ipc/client.ts` -- add `recordingPermission()`, `requestScreenRecordingPermission()` (→`RecordingPermissionVm`), and `openScreenRecordingSettings()` (→`void`) wrappers, mirroring `capabilities()` (client.ts:195).
- `src/hooks/use-recording-permission.ts` -- NEW hook: fetch-on-mount + re-fetch on `visibilitychange`→visible and window `focus` (pattern: use-capabilities-hydrate.ts + use-app-lifecycle.ts); expose `{ permission, request, openSettings, refresh }`; swallow errors to a safe default.
- `src/components/recording/recording-permission-row.tsx` -- NEW: permission name + live status pill (`Badge`) + right-aligned action (`Button`: Request / Open System Settings) + honest `note-line`s (relaunch, macOS 15+ monthly re-confirm, dev-build caveat as a muted line). No `recording-red`.
- `src/components/layout/recording-pane.tsx` -- insert a "Permissions" `Card` section (hosting the row) above the setup cards, plus a Start `Button` `disabled={!can_start}` that names the blocking permission when disabled. Capture onClick is an inert placeholder (16.6 wires it).
- `src/lib/ipc/gen/{ScreenRecordingAccess,RecordingPermissionVm}.ts` -- GENERATED by `bun run test:rust`; commit, never hand-edit.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- add `ScreenRecordingAccess` + `RecordingPermissionVm` (ts-rs, camelCase, doc comments) -- the code-owned tri-state contract (AD-7/AD-36).
- [x] `src-tauri/crates/keeper-core/src/recording.rs` -- add `resolve_screen_recording_access`, the request/parse wire fns, the `Recorder::request_screen_recording` method, and the `FakeRecorder` impl -- platform-free pre-flight logic (AD-33/AD-36), firewall intact.
- [x] `src-tauri/crates/keeper-core/src/recording.rs` (tests) -- unit-test every tri-state branch of `resolve_screen_recording_access`, the request-line shape, and `parse_request_screen_recording_result` happy/malformed/`error` -- hardware-free coverage.
- [x] `src-tauri/crates/keeper/src/recorder.rs` -- implement `DesktopRecorder::request_screen_recording`, add the bounded timeout around the pre-flight round-trips, `IosRecorder`→`Unsupported`, and extend the fake-sidecar harness (request echo + hang-past-timeout) -- the shell seam + spinner guard (deferred-work.md:967).
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- wire `recorder` + `recording_permission_requested` into `AppState`/`AppState::new`; add the three commands with `to_ipc_error` mapping -- the frontend consumer 16.4 deferred.
- [x] `src-tauri/crates/keeper/src/lib.rs` -- register the three commands in `generate_handler!`.
- [x] `tools/keeper-rec/Sources/keeper-rec/main.swift` -- add the `requestScreenRecording` case (`CGRequestScreenCaptureAccess`) returning `{granted}` -- the OS-prompt half.
- [x] `src-tauri/crates/keeper/Info.plist` -- add `NSScreenCaptureUsageDescription` via the Tauri macOS Info.plist merge -- keeper's TCC usage string (AD-36).
- [x] `src/lib/ipc/client.ts` -- add the three typed command wrappers.
- [x] `src/hooks/use-recording-permission.ts` -- fetch-on-mount + re-detect on focus/return, error-safe defaults.
- [x] `src/components/recording/recording-permission-row.tsx` -- the honest permission row with status pill, action, and note-lines (recording voice).
- [x] `src/components/layout/recording-pane.tsx` -- host the permission section + gated Start button naming the blocking permission.
- [x] Tests: `src/hooks/use-recording-permission.test.ts(x)`, `src/components/recording/recording-permission-row.test.tsx`, and update the Recording-pane test -- mock the client fns (`vi.mock("@/lib/ipc/client")`), assert render/re-fetch-on-visibility, the three states' actions, and Start disabled+named until granted.

**Acceptance Criteria:**
- Given the Recording setup renders, when the pre-flight probes through the `Recorder` port, then a `RecordingPermissionVm` reports Screen Recording distinctly as granted / not-yet-requested / denied-with-fix-path, detected live via `CGPreflightScreenCaptureAccess` (never cached optimistically) and re-detected on focus/return (FR-67, AD-36).
- Given a missing grant, when the user acts, then keeper requests via `CGRequestScreenCaptureAccess` (one real prompt per app lifetime) where the OS allows and otherwise deep-links to `x-apple.systempreferences:…Privacy_ScreenCapture`, with honest note-lines stating the relaunch, macOS 15+ monthly re-confirm, and ad-hoc-dev-build caveats (FR-67, UX-DR33).
- Given an ungranted permission, then Start is disabled with the blocking permission named (FR-67); the sidecar is spawned as a child (never a LaunchAgent) so TCC attributes it to keeper using keeper's usage string (AD-36). *(Real grant validation on hardware rides 16.6.)*
- Given a wedged or unavailable sidecar (or iOS), then the pre-flight resolves a clean error within the timeout and the frontend falls back to a safe default with no crash and no infinite spinner.
- Given `bun run check`, `bun run check:rust`, `bun run test:rust`, and `bash scripts/build-keeper-rec.sh`, then biome/tsc/vitest, clippy (`-D warnings`), cargo-nextest (with the new `.ts` regenerated and committed), and the Swift release build all pass, and `keeper-core` stays firewall-clean.

## Design Notes

**Two-valued preflight → tri-state, purely.** `CGPreflightScreenCaptureAccess` cannot distinguish an explicit denial from a never-requested state (confirmed in 16.4's honesty fix). 16.5 lifts it with a *host session* "already requested this app lifetime" flag (`AtomicBool` in `AppState`, matching the "one real prompt per app lifetime" reality): `granted → Granted`; `denied → Denied`; `notDetermined → requested ? Denied : NotYetRequested`. The mapping is a pure `keeper-core` fn so it is unit-tested without a Mac, and the flag never persists across sessions (no optimistic caching).

**One spawn per pre-flight, now bounded.** Detection reuses 16.4's one-spawn-per-request round-trip (a persistent session is 16.6). Because 16.5 re-detects on every focus/return, an unbounded `request_response` (deferred-work.md:967) would turn a wedged sidecar into the exact "spinner waiting on a grant that will never come" the story exists to prevent — so the shell wraps these calls in a bounded `tokio::time::timeout` (timeout lives in the shell; `keeper-core` stays tokio-free). A first-launch prior-session denial is honest-by-construction: the user clicks Request, `CGRequestScreenCaptureAccess` returns false without a visible prompt, and the row flips to Denied → System Settings deep-link.

**recording-red is not used here.** The permission row's status pills use neutral/positive/destructive semantics from existing tokens; `recording-red` (#E5322D) stays reserved for the live record dot (16.6). Token plumbing for the active-recording indicator is out of 16.5 scope.

**Usage string now, attribution validated later.** `NSScreenCaptureUsageDescription` is added to keeper's bundle so TCC shows keeper's own string; whether a fresh child `keeper-rec` correctly attributes the grant to keeper is validated on dev-signed hardware in 16.6 (per the epic), not fakeable in this unattended run.

## Verification

**Commands:**
- `cd src-tauri && cargo test -p keeper-core recording` -- expected: tri-state + wire-fn + firewall tests pass.
- `bun run test:rust` -- expected: cargo-nextest green; regenerates `src/lib/ipc/gen/ScreenRecordingAccess.ts` and `RecordingPermissionVm.ts` (commit them).
- `bun run check:rust` -- expected: `cargo fmt --check` + clippy `--all-targets -D warnings` clean across both crates.
- `bun run check` -- expected: biome + tsc + vitest pass (new hook/row/pane tests green).
- `bash scripts/build-keeper-rec.sh` -- expected: `swift build -c release --arch arm64` succeeds with the `requestScreenRecording` case.
- `cd src-tauri && cargo check --workspace --target aarch64-apple-ios` -- expected: compiles; `IosRecorder::request_screen_recording` returns `Unsupported`.

**Manual checks:**
- Confirm `keeper-core/src/recording.rs` imports no tauri/Apple-framework/process/tokio token (only the pre-flight timeout lives in `keeper/src/recorder.rs`).
- Confirm the built app's Info.plist carries `NSScreenCaptureUsageDescription` (real TCC attribution deferred to 16.6 hardware).

## Review Triage Log

### 2026-07-16 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 3: (high 0, medium 2, low 1)
- defer: 1
- reject: 6
- addressed_findings:
  - `[medium]` `[patch]` `request_screen_recording_permission` latched the session "already requested" flag *before* the sidecar round-trip, so a request that errored (sidecar unavailable/hung — `CGRequestScreenCaptureAccess` never ran, no prompt shown) still pinned the user to `Denied` on a later successful `notDetermined` probe, sending them to System Settings for a permission never actually requested. Moved the `store(true)` to *after* a confirmed `Ok(..)` round-trip; an errored request now leaves the flag clear so a later probe stays honestly "not yet requested."
  - `[medium]` `[patch]` The `use-recording-permission` hook re-detected on both `focus` and `visibilitychange` (macOS fires them back-to-back on window return) with only an unmount guard — overlapping probes could resolve out of order and a slower stale probe would clobber a newer result (e.g. a stale pre-grant read overwriting a fresh `granted`, spuriously re-disabling Start). Added a monotonic `seq` token shared across `refresh()` and `request()`; only the most-recently-initiated probe writes state (last-initiated wins).
  - `[low]` `[patch]` No test exercised the request→not-granted→`Denied` re-resolution (the Rust `FakeRecorder` hardcodes `Ok(true)` and the frontend always mocked `granted`). Added a hook test asserting `request` that re-resolves `denied` adopts the denied-with-fix-path (Start disabled), plus an out-of-order-probe race test asserting a late stale result never clobbers a newer one.

_Deferred (1, tracked to 16.6 — see deferred-work.md):_ the request/grant/relaunch UX rides `CGRequestScreenCaptureAccess`'s synchronous bool, which on a real first grant returns false while the OS prompt is still on screen (grant often invisible until relaunch on macOS 15+), so the row briefly reads "Denied" and the `requested+notDetermined → Denied` mapping labels an "awaiting/needs-relaunch" state with a hard "Denied" pill. The relaunch note-line discloses the quirk and re-detect partly rescues it; the honest awaiting-vs-denied labeling and real behavior are validated on hardware in 16.6 (the epic's own deferral).

_Rejected (6, noise / by-design / not this story):_ permission detection riding the full `getCapabilities` handshake couples the gate to protocol-version/shape validation (spec-dictated Code Map; keeper and keeper-rec ship in lockstep so skew is unreachable in a released bundle); the System Settings deep-link URL anchor (spec-dictated verbatim; it is the standard `Privacy_ScreenCapture` string, on-hardware verification rides 16.6); the 5s pre-flight timeout collapsing an already-granted user to the safe default on a transient re-detect timeout (spec-prescribed safe-default-on-error; transient and self-correcting, and preserving a stale "granted" when the sidecar is unreachable has its own honesty cost); the sync vs async command mix in `generate_handler!` (no defect — Tauri tolerates it); the Start gate trusting stale `canStart` at click time (the onClick is inert this story — capture re-verification is 16.6's job); and the missing capability sanity check before gating on `screen_recording` (the whole surface is gated behind the `recording` capability at the app-shell level and `get_capabilities` returns `Unsupported` off-platform).

## Auto Run Result

Status: done

**Summary.** Implemented Story 16.5 — the honest Screen Recording permission pre-flight. A new pure `keeper-core` resolver (`resolve_screen_recording_access`) lifts the sidecar's two-valued `CGPreflightScreenCaptureAccess` probe into the tri-state (granted / not-yet-requested / denied-with-fix-path) using a host session "already requested" flag; new ts-rs VMs (`ScreenRecordingAccess`, `RecordingPermissionVm`) carry it to the frontend. The sidecar gained a `requestScreenRecording` RPC (`CGRequestScreenCaptureAccess`) and the `Recorder` port a `request_screen_recording` method; three Tauri commands (`recording_permission`, `request_screen_recording_permission`, `open_screen_recording_settings`) expose fetch / request / deep-link, with a bounded pre-flight timeout so a wedged sidecar resolves a clean error instead of an infinite spinner. The Recording view now renders a live permission row (status pill + Request / Open System Settings action + honest relaunch / macOS-15-monthly / dev-build note-lines) and a Start button disabled and naming the blocking permission until the grant is green — live-detected at render and re-detected on focus/return. Actual capture and real-hardware grant validation stay in 16.6.

**Files changed.**
- `src-tauri/crates/keeper-core/src/vm.rs` — `ScreenRecordingAccess` enum + `RecordingPermissionVm` (ts-rs).
- `src-tauri/crates/keeper-core/src/recording.rs` — pure `resolve_screen_recording_access`, `request_screen_recording_request`/`parse_request_screen_recording_result`, the `Recorder::request_screen_recording` method + `FakeRecorder` impl, and unit tests (all tri-state branches, request shape, parse happy/malformed/error); firewall test green.
- `src-tauri/crates/keeper/src/recorder.rs` — `DesktopRecorder::request_screen_recording`, a bounded `tokio::time::timeout` (`PREFLIGHT_TIMEOUT=5s`) around both pre-flight round-trips, `IosRecorder`→`Unsupported`, and fake-sidecar harness cases (request echo + hang-past-timeout).
- `src-tauri/crates/keeper/src/ipc.rs` — `PlatformRecorder` cfg alias, `recorder` + `recording_permission_requested` in `AppState`/`new`, the three commands (flag now latched only after a confirmed round-trip — review patch).
- `src-tauri/crates/keeper/src/lib.rs` — registered the three commands.
- `tools/keeper-rec/Sources/keeper-rec/main.swift` — `requestScreenRecording` RPC case; no `PROTOCOL_VERSION` change.
- `src-tauri/crates/keeper/Info.plist` — new Tauri macOS merge with `NSScreenCaptureUsageDescription`.
- `src/lib/ipc/client.ts` — three typed command wrappers.
- `src/hooks/use-recording-permission.ts` — fetch-on-mount + re-detect on focus/visibility, monotonic `seq` guard against out-of-order clobber (review patch), error-safe defaults.
- `src/components/recording/recording-permission-row.tsx` — the honest permission row (no `recording-red`).
- `src/components/layout/recording-pane.tsx` — Permissions section + gated Start button (inert onClick — 16.6 wires capture).
- `src/lib/ipc/gen/{ScreenRecordingAccess,RecordingPermissionVm}.ts` — generated & committed.
- Tests: `use-recording-permission.test.tsx`, `recording-permission-row.test.tsx`, `recording-pane.test.tsx`.

**Review findings.** 1 pass. 3 patches applied (2 medium, 1 low): flag-latch timing, frontend out-of-order probe guard, request→denied + race test coverage. 1 deferred to 16.6 (real request/grant/relaunch UX + awaiting-vs-denied labeling — hardware-validated). 6 rejected (spec-dictated / by-design / not-this-story). No intent gaps, no bad-spec loopbacks.

**Verification.** `bun run check` — PASS (biome + tsc + 1283 vitest + keeper-core tauri-free). `bun run check:rust` — PASS (fmt + clippy `-D warnings`). `bun run test:rust` — PASS (847 nextest). `bash scripts/build-keeper-rec.sh` — PASS (Swift release, confirmed by the implementation session). `cargo check --workspace --target aarch64-apple-ios` — PASS (`IosRecorder` returns `Unsupported`; one pre-existing unrelated `dead_code` warning on the iOS target only). Built-app `Info.plist` carried the usage string (implementation session).

**Residual risks.** Real TCC grant attribution (that a fresh child `keeper-rec` attributes the grant to keeper), the true `CGRequestScreenCaptureAccess` request/grant/relaunch behavior, and the deep-link pane across macOS 13–15 are all validated on a dev-signed Mac in Story 16.6 (the epic's stated human-in-the-loop gate); note-lines disclose the relaunch and monthly-re-confirm quirks in the interim. The deferred labeling nuance is tracked in deferred-work.md.
