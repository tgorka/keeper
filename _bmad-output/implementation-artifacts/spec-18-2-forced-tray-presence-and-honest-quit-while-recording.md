---
title: 'Story 18.2: Forced Tray Presence & Honest Quit-While-Recording'
type: 'feature'
created: '2026-07-17'
status: 'done'
baseline_revision: 'b568a42f92492f4129456b0dee2fe634aa1a5e2a'
final_revision: '72dd5fdd9b8adb3a2a4e4bfdd14882eee30a190d'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-18-context.md'
warnings: ['multiple-goals', 'oversized']
---

<intent-contract>

## Intent

**Problem:** Two honesty gaps remain after Story 18.1. (1) When the user's opt-in tray toggle (`system.menu_bar_presence`, FR-53) is **off**, the tray slot is empty, so the ~1 Hz recording tick is a silent no-op — a live capture runs with **no menu-bar indicator at all**. (2) `RunEvent::ExitRequested` never inspects `recording_run`: quitting (⌘Q) while recording neither warns nor stops the sidecar, so the tail segment is lost and `keeper-rec` can be orphaned mid-write.

**Approach:** (1) **Forced presence** — drive the forced tray entirely from the existing tick's `apply_recording_state`: when a session is live and the tray slot is empty, build the tray and mark it *forced*; on any terminal/idle state, if it was forced, drop it — restoring the exact prior configuration **without ever mutating the persisted `system.menu_bar_presence` flag** (that flag stays the source of truth for "prior config"). (2) **Honest quit** — in `ExitRequested`, if a recording is live, show a blocking confirm; on confirm, fire the graceful `stop` trigger, await the shared status reaching a terminal state under a **kill-timeout**, and on timeout abort the (now-stored) driver `JoinHandle` so `kill_on_drop(true)` force-terminates the sidecar rather than orphaning it — then continue the existing bounded `shutdown_all` and exit. On cancel, `api.prevent_exit()` keeps the app (and recording) running.

## Boundaries & Constraints

**Always:**
- Forced presence is decided **only** from the authoritative `RecordingStatusVm` snapshot + current slot/forced state — a pure `decide_presence(state, present, forced) -> action` seam; the tick applies the action. Force-build when live+absent; drop when terminal/idle+forced; otherwise fall through to Story 18.1's existing render/restore.
- The persisted `system.menu_bar_presence` setting is **never written** by this story. "Restore exact prior configuration" = if recording forced the tray into existence, drop it on stop; if the user had it on, leave it (idle menu) exactly as 18.1 does.
- An explicit user `set_tray_presence(app, enabled)` clears the forced flag (the user now owns the tray's presence); if they toggle off mid-recording, the next tick re-forces it (an invisible live recording is a bug, self-healing within ~1 s).
- Quit-while-recording **warns first**; only on confirm does it run `stop` → await-finalize → kill-timeout. Quit is never blocked indefinitely — the total added delay is bounded by the kill-timeout (extends FR-53's bounded-quit honesty).
- On kill-timeout the driver `JoinHandle` is aborted so the sidecar is force-terminated (`kill_on_drop(true)`) — a hung sidecar is killed, never left orphaned.
- Everything tray-related stays best-effort (`warn` + continue); the recording state machine, sidecar transport, and graceful-stop trigger are reused unchanged.

**Block If:**
- The `stop` graceful-stop trigger, the shared status snapshot, or `kill_on_drop`-based sidecar termination cannot be reached (a foundational 18.1/16.x assumption is false).

**Never:**
- Never touch, hide, or duplicate macOS's own purple screen-recording pill.
- Never mutate the persisted `system.menu_bar_presence` value; never add a new VM field or any TypeScript/frontend surface (the in-app quit/banner surfaces are Stories 18.3/18.4).
- Never add warning/error tray variants (18.4), a Pause affordance, or the disk-guard (18.5).
- Never force-kill on a normal Stop — force-kill fires only when the quit kill-timeout elapses.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Force on | live state, slot absent, forced=false | `ForcePresent` — build tray, forced:=true, then render recording menu | best-effort; build fail ⇒ retry next tick |
| Already present, live | live state, slot present | `RenderRecording` (unchanged 18.1); forced unchanged | — |
| Stop restores (was forced) | terminal/idle, slot present, forced=true | `DropTray` — drop slot, forced:=false | — |
| Stop, user opted in | terminal/idle, slot present, forced=false | `RestoreIdle` (unchanged 18.1) | — |
| No tray, idle | terminal/idle, slot absent | `Noop` | — |
| Finalize before timeout | status → `Finalized` within kill-timeout | `Finalized`; no abort | — |
| Already terminal | status terminal at entry | `Finalized` immediately | — |
| Sidecar hangs | status stuck `Stopping`/`Rotating` past kill-timeout | `TimedOut`; caller aborts driver → sidecar killed | log `warn`, exit anyway |
| is_live/is_terminal | each `RecordingUiState` variant | live = {Preflight,Recording,Rotating,Stopping}; terminal = {Idle,Finalized,Recovered,Failed} | — |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/vm.rs` -- `RecordingUiState` (2672), `RecordingStatusVm` (2704). Add `RecordingUiState::is_live()` / `is_terminal()` classifier methods (unit-tested); read-only otherwise.
- `src-tauri/crates/keeper/src/tray.rs` -- `TrayState` (57), `static TRAY` (69), `build_tray` (188), `set_tray_presence` (217), `apply_recording_state` (245, live guard 246-259). Add `static FORCED_PRESENCE: AtomicBool`, pure `decide_presence` (matrix-tested), force-build/drop wiring in `apply_recording_state`, and clear forced in `set_tray_presence`. Reuse `RecordingUiState::is_live/is_terminal`.
- `src-tauri/crates/keeper/src/ipc.rs` -- `RecordingRun` (125, add `driver: Option<JoinHandle<()>>`), `recording_start` spawn (3437, store the `JoinHandle`), `stop_active_recording` (3577), `recording_snapshot` (3591). Add `finalize_within(status, timeout, poll) -> FinalizeOutcome` (paused-clock unit test) and `pub(crate) fn finalize_recording_for_quit(state)` (stop → finalize_within → abort driver on `TimedOut`).
- `src-tauri/crates/keeper/src/lib.rs` -- `RunEvent::ExitRequested` (378, body 380-392). If recording live: blocking confirm via `tauri_plugin_dialog` (40); cancel → `api.prevent_exit()`; confirm → `finalize_recording_for_quit` (bounded) then existing `shutdown_all` + exit. Bind `{ api, .. }`.
- `src-tauri/crates/keeper/src/recorder.rs` -- `kill_on_drop(true)` (360) is the force-terminate the abort relies on (reference; unchanged).

## Tasks & Acceptance

**Execution (dependency order):**
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- add `RecordingUiState::is_live()` and `is_terminal()` (live = Preflight/Recording/Rotating/Stopping; terminal = Idle/Finalized/Recovered/Failed) + exhaustive unit test over all variants.
- [x] `src-tauri/crates/keeper/src/tray.rs` -- add `static FORCED_PRESENCE: AtomicBool`; pure `decide_presence(state: RecordingUiState, present: bool, forced: bool) -> PresenceAction` (matrix rows) + unit tests; in `apply_recording_state` apply the action (ForcePresent → `build_tray` + store + set forced; DropTray → clear slot + clear forced; else existing 18.1 render/restore). Reuse `is_live/is_terminal`. In `set_tray_presence`, clear `FORCED_PRESENCE` on any explicit call. Best-effort (`warn`+continue); build failure retries next tick.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- add `driver: Option<tauri::async_runtime::JoinHandle<()>>` to `RecordingRun` and store the `recording_start` driver handle into it; add `finalize_within(status: &Arc<Mutex<RecordingStatusVm>>, timeout: Duration, poll: Duration) -> FinalizeOutcome` polling until `is_terminal` or timeout, and `pub(crate) fn finalize_recording_for_quit(state: &AppState)` that fires `stop_active_recording`, `block_on`s `finalize_within` under `QUIT_FINALIZE_TIMEOUT` (authored 10 s), and on `TimedOut` aborts the stored `driver` handle (force-terminates the sidecar) with a `warn`. Add a paused-clock (`tokio::time::pause`) unit test covering the finalize-before-timeout and hung-sidecar-timeout legs.
- [x] `src-tauri/crates/keeper/src/lib.rs` -- in `RunEvent::ExitRequested { api, .. }`: read the recording snapshot; if `is_live`, show an OkCancel confirm ("Recording in progress — quit will stop and finalize the current segment"). On cancel → `api.prevent_exit()` and return. On confirm → `finalize_recording_for_quit(&state)` before the existing bounded `shutdown_all` + exit. Non-recording quit path unchanged. (Implemented as the async two-phase `prevent_exit` + re-request pattern rather than `blocking_show`, which deadlocks on the main event-loop thread — AC semantics preserved.)

**Acceptance Criteria:**
- Given the FR-53 tray toggle is off (empty slot), when a recording becomes live, then within ~1 s the tick force-builds the tray showing the record-dot recording menu, and the persisted `system.menu_bar_presence` value is unchanged.
- Given a recording forced the tray visible, when the session reaches a terminal state (Stop/finalize), then the forced tray is dropped and the menu bar returns to exactly its prior configuration (no tray); given the user had the tray on, the tray remains with the idle menu (18.1 behavior).
- Given ⌘Q while a recording is live, when the confirm is dismissed with Cancel, then `prevent_exit` keeps the app running and the recording continues untouched.
- Given ⌘Q while live and confirmed, when the sidecar finalizes within the kill-timeout, then the current segment reaches `finalized` before the process exits and no `keeper-rec` is orphaned.
- Given ⌘Q while live and confirmed, when the sidecar hangs past `QUIT_FINALIZE_TIMEOUT`, then the driver task is aborted (sidecar force-terminated via `kill_on_drop`) and the app still exits — never a hung quit, never an orphaned recorder.
- Given a quit while not recording, then behavior is unchanged from Story 10.3 (no dialog, bounded `shutdown_all`, exit).

## Design Notes

- **Forced presence lives in the tick, not in start/stop.** Routing it through `apply_recording_state` makes it self-healing: a mid-recording toggle-off (or a transient build failure) is re-forced on the next tick. The `decide_presence` pure function is the unit-testable seam; the GUI build/drop is the best-effort side effect. The `FORCED_PRESENCE` flag is a module `static AtomicBool` (survives the TrayState build/drop cycle, unlike a `TrayState` field).
- **Prior configuration = the persisted flag, left untouched.** Because we never write `system.menu_bar_presence`, "restore" is just: drop the tray iff *we* created it. No config snapshot object is needed.
- **Kill-timeout reuses `kill_on_drop`.** The sidecar child is a local in `run_session`; the only external lever is the driver `JoinHandle` (today discarded). Storing it and calling `.abort()` drops the `run_session` future → drops the child → `kill_on_drop(true)` kills it. This is the force-terminate with zero new sidecar-handle plumbing.
- **Quit stays bounded.** `finalize_within` polls the shared status (kept current by the driver on runtime worker threads while the main thread `block_on`s — the established `ExitRequested` pattern) at a short `poll` interval, capped by `QUIT_FINALIZE_TIMEOUT`; the subsequent `shutdown_all` keeps its own 3 s bound. `QUIT_FINALIZE_TIMEOUT` (10 s) is an authored default, product-sign-off at phase release, not an architecture blocker.

## Verification

**Commands:**
- `cd src-tauri && cargo test -p keeper-core vm::` -- expected: `is_live`/`is_terminal` variant tests pass.
- `cd src-tauri && cargo test -p keeper tray::` -- expected: `decide_presence` matrix tests pass.
- `cd src-tauri && cargo test -p keeper ipc::` -- expected: `finalize_within` finalize-before-timeout + hung-timeout tests pass.
- `cd src-tauri && cargo build -p keeper` -- expected: desktop build succeeds (dialog + ExitRequested + stored JoinHandle compile).
- `cd src-tauri && cargo clippy -p keeper -p keeper-core --all-targets -- -D warnings` -- expected: no warnings.

**Manual checks (if no CLI):**
- Forced-tray appearing on record with the toggle off, disappearing on stop, and the ⌘Q warn→finalize→exit flow (incl. hung-sidecar force-kill) are GUI/event-loop behaviors validated on real hardware at a later acceptance milestone; automated coverage here is `decide_presence`, `is_live`/`is_terminal`, and `finalize_within`.

## Review Triage Log

### 2026-07-17 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 2: (high 0, medium 1, low 1)
- defer: 0
- reject: 6: (high 0, medium 2, low 4)
- addressed_findings:
  - `[medium]` `[patch]` `finalize_recording_for_quit`'s force-kill on the kill-timeout path was racy: `driver.abort()` only *schedules* cancellation, so the `run_session` future (and its `kill_on_drop` SIGKILL) might not be dropped before the process exited — orphaning the very hung `keeper-rec` the story promises to kill. Fixed in `ipc.rs`: after `abort()`, bounded-`block_on` the driver handle (`QUIT_KILL_JOIN_TIMEOUT` = 2 s) so the drop, and thus the kill, actually completes before quit proceeds.
  - `[low]` `[patch]` `PresenceAction::DropTray` took the tray slot unconditionally without re-checking ownership under the lock (unlike `force_present`), so a concurrent main-thread `set_tray_presence(true)` in the narrow recording-end window could drop a freshly-installed user-owned tray (non-self-healing, since the next terminal tick is `Noop`). Fixed in `tray.rs`: swap-check `FORCED_PRESENCE` under the tray lock and only `take()` a tray we still own.
- notes: Rejected — the up-to-13 s main-thread quit freeze (bounded, only on a hung sidecar; `QUIT_FINALIZE_TIMEOUT` is an authored owner-sign-off threshold the spec already flags); latched `QUIT_CONFIRMED` tearing down a new recording unwarned (sub-ms window on a busy main thread, and the second exit pass is guaranteed to run — the process exits); `QUIT_CONFIRM_IN_FLIGHT` stranding quit (the main window is never destroyed — `CloseRequested`→hide — and the macOS confirm reliably fires its completion on either button, so the flag always resets); plus the reviewers' own verified-safe TOCTOU / lock-discipline / `driver: None`-informational notes.

## Auto Run Result

Status: done

**Summary:** Closes Epic 18's two remaining tray-honesty gaps on top of Story 18.1's tray plumbing. (1) **Forced presence** — the existing ~1 Hz tick now force-builds the menu-bar tray when a recording is live but the FR-53 opt-in toggle is off (an invisible live recording is a bug), tracked by a module `FORCED_PRESENCE` flag and dropped on the terminal tick to restore the exact prior configuration; the persisted `system.menu_bar_presence` setting is never written. All routed through a pure, matrix-tested `decide_presence(state, present, forced)` seam. (2) **Honest quit** — `RunEvent::ExitRequested` now warns first when a recording is live (async two-phase confirm, since `blocking_show` deadlocks the main event-loop thread), then on confirm fires the graceful `stop` trigger, awaits the shared status reaching a terminal state under a 10 s kill-timeout, and on timeout aborts the now-stored driver `JoinHandle` — dropping `run_session` so `kill_on_drop(true)` force-terminates a hung `keeper-rec` rather than orphaning it — before the existing bounded `shutdown_all`. `is_live`/`is_terminal` classifiers were added to `RecordingUiState` as the single source of truth reused by tray + quit.

**Files changed:**
- `src-tauri/crates/keeper-core/src/vm.rs` — `RecordingUiState::is_live()`/`is_terminal()` (exhaustive complements) + partition test.
- `src-tauri/crates/keeper/src/tray.rs` — `FORCED_PRESENCE` static, `PresenceAction` + pure `decide_presence` (matrix test), forced-build/drop wiring extracting 18.1's render/restore into `render_recording`/`restore_idle`; `set_tray_presence` clears the forced flag; ownership re-check under lock on `DropTray` (review patch).
- `src-tauri/crates/keeper/src/ipc.rs` — `RecordingRun.driver` handle stored from `recording_start`; `finalize_within` (+3 paused-clock tests), `finalize_recording_for_quit`, `QUIT_FINALIZE_TIMEOUT`/`QUIT_FINALIZE_POLL`, and `QUIT_KILL_JOIN_TIMEOUT` bounded post-abort join (review patch).
- `src-tauri/crates/keeper/src/lib.rs` — recording-aware `ExitRequested`: async two-phase confirm (`QUIT_CONFIRMED`/`QUIT_CONFIRM_IN_FLIGHT`), `prevent_exit` on cancel, `finalize_recording_for_quit` before `shutdown_all` on confirm; non-recording quit path unchanged.
- `src-tauri/crates/keeper/Cargo.toml` — `tokio` `test-util` dev-dependency (paused-clock tests only).
- `src/lib/ipc/gen/RecordingStatusVm.ts` — ts-rs-regenerated doc comment only (syncs 18.1's `output_path`-is-a-folder wording); no type change.

**Review findings breakdown:** 2 patches applied (1 medium: racy force-kill → bounded post-abort join; 1 low: `DropTray` ownership re-check); 0 intent gaps; 0 bad-spec loopbacks; 0 deferred; 6 rejected (bounded/authored quit-freeze, non-manifesting quit-static edges, reviewer-verified-safe notes). One implementation deviation (async two-phase confirm instead of `blocking_show`) was correct and AC-preserving — `blocking_show` deadlocks the main event-loop thread.

**Verification:** `cargo test -p keeper-core vm::` → 183 passed; `cargo test -p keeper tray::` → 5 passed; `cargo test -p keeper ipc::` → 48 passed (incl. all 3 `finalize_within` legs); `cargo build -p keeper` → success; `cargo clippy -p keeper -p keeper-core --all-targets -- -D warnings` → clean; `cargo fmt --check` → clean. (The machine hit a disk-full condition mid-verification; reclaimed via `cargo clean -p keeper -p keeper-core` + removing stale duplicate artifacts — no source impact.)

**Residual risks:** GUI/event-loop behaviors (the forced tray appearing/disappearing, the ⌘Q confirm sheet, and the live hung-sidecar force-kill) are not automatable here — deferred to real-hardware acceptance (SM-10 / Story 20.6); automated coverage is the pure `decide_presence`, `is_live`/`is_terminal`, and `finalize_within` seams. Worst-case quit freeze is bounded at ~12 s and only on a wedged sidecar. `multiple-goals` + `oversized` spec warnings retained (two separable goals, cross-layer story). The developer's machine is low on free disk.
