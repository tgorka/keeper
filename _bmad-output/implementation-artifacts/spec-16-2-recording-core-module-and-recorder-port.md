---
title: 'recording Core Module & Recorder Port'
type: 'feature'
created: '2026-07-16'
status: 'done'
baseline_revision: '4db2954b5185989485b26234b09f659a27dfc520'
final_revision: '9d831f4588017324e720ae06ea0516928504477c'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-16-context.md'
warnings: [oversized]
---

<intent-contract>

## Intent

**Problem:** Recording logic has no home on the hexagonal seam. Every later story (16.3 gating, 16.4 RPC, 16.5 pre-flight, 16.6 capture) needs a platform-free session state machine and a shell-side `Recorder` port that spawns `keeper-rec` — without either, a capture crash could reach the messaging core and there is nowhere to hang the sidecar seam.

**Approach:** Add a `keeper-core::recording` module (platform-free, tauri-free, no Apple API) owning the session state machine `idle → preflight → recording → rotating → stopping → finalized | recovered | failed` with `thiserror → CoreError`, plus a `Recorder` trait port beside `Platform`. Add the macOS shell impl (`crates/keeper/src/recorder.rs`, `#[cfg(desktop)]`) that spawns `keeper-rec` via `Platform::sidecar_path` and streams its parsed NDJSON events into the machine; iOS returns `CoreError::Unsupported`. No real capture, no RPC field contract, no IPC/UI surface (16.4/16.6/16.3).

## Boundaries & Constraints

**Always:**
- The state machine is **platform-free**: no `tauri`, no Apple framework/`objc`, no process handle, no `keeper-rec` spawn — it lives entirely in `keeper-core`. It never holds a process handle; the port parses sidecar events and feeds them in. Enforce with a unit-test source guard (mirror `signals::tests::presence_is_withheld_everywhere`: forbidden tokens built by concatenation so they don't self-match).
- Illegal state transitions are **rejected** (`RecordingError::IllegalTransition`), never silently adopted. Errors flow `RecordingError` → `CoreError::Recording(#[from] …)` (AD-21). The port's not-available paths return the existing `CoreError::Unsupported` (matching `Platform::sidecar_path` honesty), not a new variant.
- `Recorder` is a native-`async fn` trait dispatched **statically** (`impl Future + Send`, no `async-trait`, no trait object) exactly like `bridges::bbctl::BbctlRunner`. `is_available()` returns `false` (never an error) when the sidecar can't be resolved.
- The macOS `Recorder` impl spawns `keeper-rec` (logical sidecar name `"keeper-rec"`) via `Platform::sidecar_path`, reads its stdout NDJSON line-by-line (byte-level; a non-UTF-8 line is skipped, never a false EOF), parses each with the pure core parser, and forwards `RecordingEvent`s; it never panics on absent/garbage output. Reader tasks are torn down on drop (reuse the `AbortOnDrop` idiom).
- Add the `to_ipc_error` arm for `CoreError::Recording(_)` to keep that funnel exhaustive (map to `IpcErrorCode::Internal`, non-retriable — recording does not cross the IPC command surface in this story; a dedicated surface arrives in 16.3+).
- Sidecar→host event field names are **provisional** and code-owned (like bbctl's provisional prose markers); the typed RPC contract is finalized in 16.4/16.6. Keep the parser tolerant: unrecognized lines return `None` and are dropped.

**Block If:**
- Retiring recording logic to the core would force a `tauri`/Apple dependency into `keeper-core` (it must not — surface this rather than adding the dep).

**Never:**
- No real capture (SCK stream, AVAssetWriter, sample buffers, `start`/`stop` capture config), no `getCapabilities`/`listSources` typed VMs, no `CapabilitiesVm.recording` flag, no permission pre-flight, no IPC command, no frontend surface — all owned by 16.3/16.4/16.5/16.6.
- No new network destination, no upload/telemetry (local-only invariant, FR-76).
- No ts-rs / VM export from this module (no `#[ts(export)]`) — the state enum becomes a VM only when a later story surfaces it.
- No storing a live `keeper-rec` process handle inside `keeper-core`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Full lifecycle | Fake `Recorder` scripts `Preflight → Recording → Rotating → Recording → Stopping → Finalized` | `RecordingSession` walks each state in order; terminal state is `Finalized` | No error |
| Failure branch | script reports `Failed` while `Recording` | session transitions to `Failed` (terminal) | surfaced, not panicked |
| Recovered branch | script reports `Recovered` from `Stopping` | session transitions to `Recovered` (terminal) | No error |
| Illegal transition | apply `SegmentClosed` while `Idle` | transition rejected; state unchanged | `RecordingError::IllegalTransition` |
| Parse `state`/`segmentClosed`/`error` line | recorded NDJSON fixture stream | each recognized line → the matching `RecordingEvent`; unknown line → `None` | malformed line → `None`, never a panic |
| macOS port, no bundled sidecar | `sidecar_path("keeper-rec")` is `Unsupported` (dev/CI) | `is_available()` == `false`; `run_session` returns `CoreError::Unsupported` | honest, no spawn, no panic |
| iOS port | any call | `is_available()` == `false`; `run_session` returns `CoreError::Unsupported` | honest, no panic |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/recording.rs` -- NEW: the whole platform-free module — `SessionState` (8 states), `RecordingEvent`, `RecordingSession::apply` transition table, pure `parse_event`, the `Recorder` trait, the `drive_session<R: Recorder>` orchestrator, and all unit tests (full-lifecycle fake, parse fixtures, dep-firewall guard).
- `src-tauri/crates/keeper-core/src/error.rs` -- add `RecordingError` (thiserror) + `CoreError::Recording(#[from] RecordingError)`.
- `src-tauri/crates/keeper-core/src/lib.rs` -- add `pub mod recording;`.
- `src-tauri/crates/keeper/src/recorder.rs` -- NEW: `DesktopRecorder` (`#[cfg(desktop)]`) spawns `keeper-rec` via `Platform::sidecar_path` and streams parsed events (`tokio::process` + `AbortOnDrop`, mirroring `DesktopBbctlRunner`); `IosRecorder` (`#[cfg(target_os = "ios")]`) returns `CoreError::Unsupported`. Both hold `Arc<dyn Platform>`.
- `src-tauri/crates/keeper/src/lib.rs` -- register `mod recorder;`.
- `src-tauri/crates/keeper/src/ipc.rs` -- add the `CoreError::Recording(_)` arm to `to_ipc_error`.
- `src-tauri/crates/keeper/tauri.ios.conf.json` -- NEW (amendment): per-platform Tauri override scoping `bundle.externalBin` to `[]` for iOS, so the required iOS CI gate (`cargo check --workspace --target aarch64-apple-ios`) stops failing on the missing `keeper-rec-aarch64-apple-ios` resource. See Spec Change Log 2026-07-16.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/error.rs` -- add `RecordingError` enum (`IllegalTransition { from: SessionState, event: String }`, `SidecarFailed(String)`) with `#[derive(Debug, Error)]`; add `CoreError::Recording(#[from] RecordingError)` -- rolls recording errors into the hexagon root (AD-21). (Review patch: a speculative `Protocol(String)` variant was dropped — no code path constructs it in this story; 16.4 adds protocol errors when it owns the typed wire contract.)
- [x] `src-tauri/crates/keeper-core/src/recording.rs` -- define `SessionState { Idle, Preflight, Recording, Rotating, Stopping, Finalized, Recovered, Failed }`; `RecordingEvent` (sidecar-reported facts driving transitions: preflight started, capture started, segment rotating, segment closed, stopping, finalized, recovered, failed); `RecordingSession` holding current state with `apply(&mut self, RecordingEvent) -> Result<(), RecordingError>` implementing the explicit transition table and rejecting illegal transitions; a pure `parse_event(&str) -> Option<RecordingEvent>` for `state`/`segmentClosed`/`error` NDJSON lines (provisional shape, tolerant); the `Recorder` trait (`is_available(&self) -> bool` + `run_session(&self, on_event: Box<dyn FnMut(RecordingEvent) + Send>) -> impl Future<Output = Result<(), CoreError>> + Send`); and `drive_session<R: Recorder>(recorder, on_state) -> Result<SessionState, CoreError>` that feeds parsed events into a `RecordingSession` and returns the terminal state -- the platform-free heart of the story.
- [x] `src-tauri/crates/keeper-core/src/lib.rs` -- `pub mod recording;` -- register the module.
- [x] `src-tauri/crates/keeper/src/recorder.rs` -- `DesktopRecorder { platform: Arc<dyn Platform> }` (`#[cfg(desktop)]`): `is_available()` = `sidecar_path("keeper-rec").is_ok()`; `run_session` resolves the path (→ `CoreError::Unsupported` if absent), spawns via `tokio::process::Command` (stdin null, stdout piped), reads lines byte-level, parses with `recording::parse_event`, forwards each event, tears readers down on drop, and reaps exit; on a `keeper-rec` I/O/spawn failure returns `CoreError::Recording(RecordingError::SidecarFailed(...))`. `IosRecorder` (`#[cfg(target_os = "ios")]`): `is_available()` == `false`, `run_session` returns `CoreError::Unsupported` -- the shell port seam (AD-24, AD-27).
- [x] `src-tauri/crates/keeper/src/lib.rs` -- `mod recorder;` -- compile the port impls.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- add `CoreError::Recording(_) => (IpcErrorCode::Internal, false)` to `to_ipc_error`, with a comment that recording errors don't cross the IPC surface yet -- keeps the funnel exhaustive.
- [x] `src-tauri/crates/keeper-core/src/recording.rs` (tests) -- `#[cfg(test)]`: a `FakeRecorder` scripting an event sequence (mirror `FakeBbctlRunner`); a full-lifecycle test driving `drive_session` through `idle→preflight→recording→rotating→recording→stopping→finalized` asserting the walk; a failure-branch test (`→Failed`); a recovered-branch test (`→Recovered`); illegal-transition tests; `parse_event` fixture tests for `state`/`segmentClosed`/`error` + unknown/malformed → `None`; and a dep-firewall guard test asserting no Apple-framework/`tauri`/`objc` token appears in `recording.rs` -- covers the I/O matrix and AD-33 invariants without hardware.
- [x] `src-tauri/crates/keeper/tauri.ios.conf.json` (amendment) -- per-platform Tauri override setting `bundle.externalBin: []` for iOS -- un-breaks the required iOS CI gate that Story 16.1's unconditional `externalBin` had left red (the shell-crate iOS `cargo check` failed on the missing `keeper-rec-aarch64-apple-ios` resource before rustc ran). See Spec Change Log 2026-07-16.

**Acceptance Criteria:**
- Given the module lands, when `cargo tree`/inspection and the dep-firewall unit test run, then `keeper-core::recording` carries no `tauri` and no Apple API and the `RecordingSession` holds no process handle (AD-33).
- Given the port seam, when the shell is built for iOS (`cargo check --target aarch64-apple-ios`), then `IosRecorder::run_session` returns `CoreError::Unsupported` and the crate compiles (AD-27).
- Given no bundled sidecar (dev/CI), when `DesktopRecorder::is_available()` is queried, then it is `false` and `run_session` returns `CoreError::Unsupported` — never a panic.
- Given the exhaustive `to_ipc_error` funnel, when the new `CoreError::Recording` variant is added, then the shell still compiles (the new arm keeps the match exhaustive).
- Given `bun run check:rust` and `bun run test:rust`, when they run, then clippy is clean (`-D warnings`) and every new unit test passes.

## Spec Change Log

### 2026-07-16 — iOS CI-gate regression surfaced during implementation
- **Finding:** implementation-time verification (`cargo check --workspace --target aarch64-apple-ios`, the required iOS CI gate) failed before rustc ran: `resource path binaries/keeper-rec-aarch64-apple-ios doesn't exist`. Root cause is **not** this story's Rust — it is Story 16.1's unconditional `bundle.externalBin: ["binaries/keeper-rec"]`. 16.1's Design Notes claimed "the iOS CI gate is `cargo check` … so `externalBin` is never consulted there"; that claim is false — `tauri-build`'s build script validates `externalBin` for the iOS target (which has no sidecar) on plain `cargo check`, leaving the required gate red.
- **Amendment:** added `src-tauri/crates/keeper/tauri.ios.conf.json` (a Tauri v2 per-platform config override) setting `bundle.externalBin: []` for iOS only. Desktop bundling is untouched (the base config still declares the sidecar). Verified: `cargo check --workspace --target aarch64-apple-ios` now exits 0.
- **Known-bad state avoided:** shipping 16.2 (or any story) on a permanently-red required iOS gate; and the tempting-but-wrong "fix" of committing/stubbing an iOS sidecar (iOS never records — FR-76 / epic invariant).
- **KEEP:** iOS recording honesty stays a Rust concern (`IosRecorder` → `CoreError::Unsupported`); the config override only scopes the *bundle input*, never adds an iOS capture path.

## Review Triage Log

### 2026-07-16 — Follow-up review pass
- intent_gap: 0
- bad_spec: 0
- patch: 1: (high 0, medium 0, low 1)
- defer: 2: (high 0, medium 0, low 2)
- reject: 15
- addressed_findings:
  - `[low]` `[patch]` The dependency-firewall guard banned only shell/Apple-framework tokens (`tauri`/`objc`/`objc2`/`ScreenCaptureKit`/`AVFoundation`/`CoreGraphics`) — nothing caught a raw process spawn, which is the actual AD-33 "never holds a process handle / no sidecar spawn" invariant. Added `tokio::process` and `std::process` as concatenation-built forbidden tokens so an accidental process-API import into `recording.rs` now fails the guard; reworded the surrounding comment to avoid the literal banned words (`recording.rs`). Verified: 18 recording tests pass, clippy clean.

### 2026-07-16 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 6: (high 0, medium 4, low 2)
- defer: 8: (high 0, medium 4, low 4)
- reject: 6
- addressed_findings:
  - `[medium]` `[patch]` Orphaned live sidecar when the `run_session` future is dropped mid-stream (only the reader was torn down) — added `Command::kill_on_drop(true)` and corrected the teardown doc comment (`recorder.rs`).
  - `[medium]` `[patch]` A non-zero `keeper-rec` exit with no `error` line resolved as a clean success — `run_session` now inspects `ExitStatus` and returns `SidecarFailed` on `!success()` (`recorder.rs`).
  - `[medium]` `[patch]` The dep-firewall test read a CWD-relative `src/recording.rs` (could panic spuriously or scan the wrong file and pass vacuously) — anchored on `env!("CARGO_MANIFEST_DIR")` like `signals::tests` (`recording.rs`).
  - `[medium]` `[patch]` An `error` NDJSON line with a missing/mistyped `message` was dropped to `None` (lost failure → stuck session) — `parse_event` now surfaces `Failed` with a non-secret fallback; added a regression test (`recording.rs`).
  - `[low]` `[patch]` Dead `RecordingError::Protocol` variant (defined, documented, never constructed — a false contract) — removed; 16.4 adds protocol errors with the typed wire contract (`error.rs`, spec task).
  - `[low]` `[patch]` `drive_session` discarded buffered `on_state` transitions when the run errored, and its doc over-claimed live streaming — now flushes transitions before propagating the error and documents the batched delivery (`recording.rs`).

## Design Notes

**Event-driven machine (AD-33).** The state machine advances **only** on `RecordingEvent`s the port feeds in — host intent (start/stop) becomes a command the sidecar acts on and then *reports back* as a `state` event, so the core never needs a process handle or a command side-channel in this story. Transition table (terminal: `Finalized`, `Recovered`, `Failed`):

```
Idle      → Preflight
Preflight → Recording | Failed
Recording → Rotating | Stopping | Failed
Rotating  → Recording | Stopping | Failed
Stopping  → Finalized | Recovered | Failed
```
`SegmentClosed` is legal only in `Recording`/`Rotating` (updates a counter, no state change). Any other input → `RecordingError::IllegalTransition`. Full crash-recovery *entry* semantics are Epic 17; here `Recovered` is a reachable terminal (a salvaged partial finalize) so the skeleton enumerates all 8 states.

**Port mirrors `BbctlRunner`.** Static generic dispatch, `is_available()` + streaming `run_session`; the `DesktopRecorder` body reuses the `DesktopBbctlRunner` spawn/stream/`AbortOnDrop` shape (stdout only — `keeper-rec` speaks NDJSON on stdout). The `FakeRecorder` scripts events for the lifecycle test, exactly as `FakeBbctlRunner` scripts lines. Illustrative driver core:

```rust
pub async fn drive_session<R: Recorder>(
    recorder: &R,
    mut on_state: impl FnMut(SessionState) + Send,
) -> Result<SessionState, CoreError> {
    let session = std::sync::Arc::new(std::sync::Mutex::new(RecordingSession::new()));
    let (s, errs) = (session.clone(), std::sync::Arc::new(std::sync::Mutex::new(Vec::new())));
    let sink = { let (s, errs) = (s.clone(), errs.clone()); Box::new(move |ev| {
        let mut g = s.lock().expect("session");
        match g.apply(ev) { Ok(()) => on_state(g.state()), Err(e) => errs.lock().unwrap().push(e) }
    }) };
    recorder.run_session(sink).await?;
    if let Some(e) = errs.lock().unwrap().drain(..).next() { return Err(e.into()); }
    Ok(session.lock().expect("session").state())
}
```
(Final impl may differ; keep it lock-simple and panic-free.)

## Verification

**Commands:**
- `cd src-tauri && cargo test -p keeper-core recording` -- expected: full-lifecycle, failure, recovered, illegal-transition, parse-fixture, and dep-firewall tests pass.
- `bun run check:rust` -- expected: clippy clean (`-D warnings`) across both crates (new `to_ipc_error` arm keeps the funnel exhaustive).
- `bun run test:rust` -- expected: whole Rust suite green; ts-rs export step unaffected (no VM added).
- `cd src-tauri && cargo check --target aarch64-apple-ios -p keeper` -- expected: compiles; `IosRecorder` returns `Unsupported`.
- `cd src-tauri && cargo deny check licenses bans sources` -- expected: passes (no dependency added).

**Manual checks:**
- Confirm `keeper-core/src/recording.rs` imports no `tauri`/Apple framework and the `Recorder` trait's macOS impl lives only in `keeper/src/recorder.rs` under `#[cfg(desktop)]`.

## Auto Run Result

Status: done

**Summary:** Landed the platform-free `keeper-core::recording` module and the `Recorder` shell port so recording logic sits on the hexagonal seam and a capture crash can never reach the messaging core. The core owns the session state machine (`idle → preflight → recording → rotating → stopping → finalized | recovered | failed`) with a tolerant NDJSON event parser and a static-dispatch `Recorder` trait (mirroring `bridges::bbctl::BbctlRunner`); the macOS shell impl spawns `keeper-rec` via `Platform::sidecar_path` and streams its parsed events in, while iOS returns `CoreError::Unsupported`. No real capture, RPC VMs, permission pre-flight, IPC, or frontend (owned by 16.3–16.6).

**Files changed:**
- `src-tauri/crates/keeper-core/src/recording.rs` (new) — `SessionState`, `RecordingEvent`, `RecordingSession::apply` transition table + segment counter, pure `parse_event`, the `Recorder` trait, `drive_session`, and 18 unit tests (full-lifecycle fake, failure/recovered branches, illegal transitions, parse fixtures incl. never-swallow-error, dependency firewall).
- `src-tauri/crates/keeper-core/src/error.rs` — `RecordingError` (`IllegalTransition`, `SidecarFailed`) + `CoreError::Recording(#[from] …)`.
- `src-tauri/crates/keeper-core/src/lib.rs` — `pub mod recording;`.
- `src-tauri/crates/keeper/src/recorder.rs` (new) — `#[cfg(desktop)] DesktopRecorder` (spawns `keeper-rec`, `kill_on_drop`, stdout NDJSON, exit-status-checked) and `#[cfg(target_os="ios")] IosRecorder` (`Unsupported`); plus a no-sidecar `is_available()==false` test.
- `src-tauri/crates/keeper/src/lib.rs` — `mod recorder;`.
- `src-tauri/crates/keeper/src/ipc.rs` — `CoreError::Recording(_)` arm in `to_ipc_error` (keeps the funnel exhaustive).
- `src-tauri/crates/keeper/tauri.ios.conf.json` (new) — per-platform override scoping `bundle.externalBin` to `[]` on iOS; un-breaks the required iOS CI gate that Story 16.1's unconditional `externalBin` had left red.

**Review findings breakdown:** 6 patches applied (medium: `kill_on_drop` for the orphan-sidecar risk, non-zero-exit surfaced as `SidecarFailed`, dep-firewall test anchored on `CARGO_MANIFEST_DIR`, `error`-line-without-`message` no longer swallowed; low: dead `Protocol` variant removed, `drive_session` flushes transitions before erroring + honest doc). 8 deferred to `deferred-work.md` (read-error-vs-EOF, unbounded channel/line cap, `wait()` timeout, crash-recovery entry paths [Epic 17], segment-index validation [Epic 17], sidecar-message capping, live `on_state` streaming, single-session guard). 6 rejected as noise/by-design (Preflight→Stopping cancel, idempotent re-report strictness, concatenated-JSON tolerance, `to_ipc_error` Internal collapse, `is_available` path-only probe, prefer-terminal-over-early-error policy). No intent_gap, no bad_spec, no spec loopback.

**Verification performed:**
- `cargo test -p keeper-core recording` → 18 passed.
- `cargo nextest run` (whole Rust suite) → 814 passed, 0 failed.
- `cargo clippy --workspace --all-targets -- -D warnings` → clean.
- `cargo check --workspace --target aarch64-apple-ios` → exit 0 (required iOS gate green after the `tauri.ios.conf.json` fix; the pre-fix failure was the build-script `keeper-rec-aarch64-apple-ios` resource error, before rustc, reproduced independently).
- `cargo deny check licenses bans sources` → bans/licenses/sources ok (no dependency added).

**Residual risks:** The macOS `run_session` spawn/stream path is exercised by unit-tested parsing and the no-sidecar availability test, but a real `keeper-rec` capture stream is not driven until Story 16.6 (human-in-the-loop, dev-signed hardware); the deferred process-lifecycle hardening (read-error surfacing, backpressure, `wait()` timeout) and the crash-recovery/segment-index transitions (Epic 17) are the main follow-ups.

### 2026-07-16 — Follow-up review pass

The recommended independent follow-up review ran (two adversarial/edge-case reviewers, opus, on the full 4db2954..HEAD diff). Outcome: the process-lifecycle patches confirmed sound. One low-severity **patch** applied — the dependency-firewall guard enforced only shell/Apple-framework tokens, so the actual AD-33 "never holds a process handle / no sidecar spawn" invariant went unchecked; added `tokio::process` + `std::process` forbidden tokens (18 recording tests pass, clippy clean). Two **new** findings **deferred** to `deferred-work.md`: (1) a non-zero `keeper-rec` exit following an already-reported terminal masks that terminal (reconcile with 16.4/16.6's exit-code contract); (2) the `run_session` spawn/stream/reap body remains untested (add a fake-executable NDJSON harness ahead of 16.6). The remaining reviewer findings re-surfaced items already deferred by the prior pass (wait() timeout, unbounded channel, segment-index, read-error, live streaming, single-session) — dropped to avoid ledger duplication — or were rejected as by-design/noise. Given only one localized low-consequence patch, `followup_review_recommended` is now `false`.
