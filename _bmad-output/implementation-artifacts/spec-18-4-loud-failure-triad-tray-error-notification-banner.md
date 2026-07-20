---
title: 'Story 18.4: Loud-Failure Triad — Tray Error + Notification + Banner'
type: 'feature'
created: '2026-07-19'
status: 'done'
baseline_revision: 'f5d8830c246b60db66802b004b24ad09662a81c5'
final_revision: '8997c36224d0faa395fe01bfa951ac2b359a7758'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-18-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** A recording fault can currently die quietly. The Rust state machine already reaches terminal `Failed` and exposes `RecordingStatusVm.error`, but nothing acts on it loudly: the tray *drops* on a terminal state (masking the failure instead of flipping to error), no native notification is ever posted for a recording fault, and the in-app banner returns `null` on `failed` (only a small pane-header note shows the reason). Story 19.4 wired the persistent tray/banner surfaces for non-fatal warnings but **deliberately left the native-notification leg to this story**. Result: a session can fail with no loud, cross-surface signal, and there is no one-click way to recover.

**Approach:** Wire the **loud-failure triad** on top of the existing `error`/`warning` snapshot fields — no new state machine. On a fault (transition into `Failed` with an `error`) all three surfaces fire within 5 s: the tray **holds visible and flips to an error badge** (new recording-red asset) naming the reason, a **native notification** is posted through the existing AD-18 pipeline, and the **banner shows a filled recording-red error variant** with a one-click **Restart recording** action. A single notification also fires on **warning onset** (closing 19.4's deferred leg). Add a `recording_acknowledge` command to clear a terminal-failed session back to idle (so the held tray/banner can be dismissed) and a hook-level `restart()` that replays the session's start params.

## Boundaries & Constraints

**Always:**
- All three surfaces render **purely from the Rust-owned snapshot** (`state`, `error`, `warning`) — never invent or duplicate fault state in TS. The banner error variant renders exactly when `state === "failed" && error !== null`.
- The tray **must not drop on `Failed`**: while `state == Failed` with an `error`, hold the current tray (forced or user-owned) in an error rendering — error badge + a status line naming the reason. It restores its prior configuration (existing 18.2 restore/drop path) **only** once the fault is cleared (acknowledge → idle, or a restart moves it back to a live state).
- The native notification fires **exactly once per fault onset** (snapshot `error` transitions None→Some) and **once per warning onset** (`warning` transitions None→Some) — event-driven, never re-posted by the 1 Hz tray tick, never re-fired for a sticky warning that merely repeats. It routes through the existing `Platform::notify`/AD-18 pipeline and **bypasses global DND and per-network mute** (a recording fault is a local loss-risk event and a mandated triad leg, not a chat notification).
- The one-click **Restart recording** replays the session's captured start params (target / system-audio / mic selection) via the existing `recording_start` path; it lives in the banner (and the fault notification/tray draw attention to it). `recording_acknowledge` clears a **terminal** session slot back to idle (dropping `error`/`warning`); it is a **no-op on a live session** (never a silent stop).
- Recording-red stays reserved for the record dot, the banner edge/fill, and the tray error badge only — never on the Restart/Dismiss buttons (destructive-outline / neutral), and it must stay visually distinct from destructive-delete red. Reduced-motion keeps any dot steady.
- Accessibility: the banner error is announced **assertively** (role="alert" / assertive live region) as a loss-risk event; `Esc` never stops or restarts a recording; Restart/Dismiss are explicit focusable controls. Tray items are real labelled menu items.
- Existing non-recording tray and notification behavior (Stories 10.2–10.4, 18.1–18.3, 19.4) must remain unchanged.

**Block If:**
- The sidecar/state machine cannot deliver a terminal `Failed` with a human-meaningful `error` message for a fault (i.e. the foundational 16.x/17.x/19.4 `RecordingEvent::Failed`/`Warning` contract is absent or changed).
- Firing a native notification for a recording fault is impossible without regressing the AD-18 message-notification gating (i.e. no way to bypass DND/mute for a recording fault without breaking `should_notify`).

**Never:**
- Never claim a notification **action button** ("Restart" tapped from the notification): `tauri-plugin-notification` 2.x has no per-notification action/click callback (deferred to Epic 11). The notification is the loud *alert*; the one-click restart is the banner button (and tray "Show Recording" brings it forward).
- Never add live writer-stall / device-loss / permission-revoke **detection** here — 18.4 *surfaces* any `Failed` fault the sidecar reports; live-permission-revoke validation is Story 20.6, and the disk-space **monitor policy** (warn 10 GB / hard-floor stop 2 GB) is Story 18.5 (which reuses these surfaces).
- Never re-implement or alter 19.4's sticky-warning tray/banner surfaces; never touch or duplicate macOS's purple recording pill; never add a Pause affordance.
- Never let `recording_acknowledge` clear a live session; never post a fault notification more than once per onset.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| fault onset | live session → `RecordingEvent::Failed{message:"keeper-rec exited"}` | state→failed, `error` set; notification posted once; tray flips to error badge + reason line (held); banner error variant shows reason | message never swallowed |
| fault via run_session Err | recorder task returns `Err` while state live | state→failed + `error`; notification fires **once** (not double with sink path) | guarded on not-already-terminal |
| notification dedup | second `Failed`/`error`-equal event after failure | no new notification; state stays failed | `apply` from terminal rejected |
| warning onset | live → `Warning{code:"micLost",message}` (None→Some) | one notification posted; tray ⚠ line + banner amber persist (19.4 unchanged) | — |
| warning repeat | subsequent `Warning` while `warning` already Some | no new notification; sticky message updates only | last-write-wins (19.4) |
| tray hold on fault | tick observes state=failed+error | tray stays visible, error badge + `Recording failed — <reason>` disabled line + Restart-guidance menu | never drop while failed+error |
| acknowledge (terminal) | state=failed, `recording_acknowledge` | slot cleared → snapshot idle (error/warning gone); next tick restores/drops tray per 18.2 | returns idle snapshot |
| acknowledge (live) | state=recording, `recording_acknowledge` | no-op; session untouched; snapshot unchanged | never silent-stops |
| restart | banner Restart on failed | `recording_start` replays captured target/audio/mic; state→preflight→recording; surfaces clear | start failure → failed+error again |
| banner error hidden | state live, or terminal with `error==null` (finalized/recovered/idle) | error variant renders `null` (live variant or nothing) | — |
| DND on + fault | global DND enabled, fault occurs | notification still posted (bypasses DND); tray+banner fire | — |
| reduced motion | `prefers-reduced-motion` + error banner | any dot steady; no pulsing | — |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper/src/ipc.rs` -- event sink (~3616) and run_session-Err fallback (~3729): detect `error` None→Some and `warning` None→Some transitions and dispatch a fault/warning notification exactly once via a new recording-notify entry; add `#[tauri::command] recording_acknowledge` that clears a **terminal** session slot to idle (no-op if live) and returns the fresh snapshot; `RecordingRun` already captures start params for replay (verify target/audio/mic are retained; extend if not).
- `src-tauri/crates/keeper-core/src/notify.rs` -- add a recording-fault entry (e.g. `notify_recording_fault`/`notify_recording_warning`) that calls `platform.notify(title, body, target)` **bypassing** `should_notify` (DND/mute), mirroring the `notify_bridge_disconnected` bypass precedent; concise copy naming the reason and pointing to the app to restart.
- `src-tauri/crates/keeper/src/tray.rs` -- `decide_presence`/apply (~254, ~395): add an **error-hold** branch so `Failed`+`error` renders an error badge (`Image::from_bytes(ERROR_ICON_PNG)`) + a `Recording failed — <reason>` disabled status line and a menu with "Show Recording", "Open Recordings Folder", "Dismiss Error" (→ acknowledge) rather than dropping; restore/drop only after acknowledge/idle. Reuse the 19.4 warning-line helper for warnings (unchanged).
- `src-tauri/crates/keeper/icons/tray-error.png` -- NEW recording-red **filled** error badge asset (base dimensions on existing `tray-recording.png`), embedded via `include_bytes!`; visually distinct from the record-dot.
- `src/components/recording/active-recording-banner.tsx` -- extend the render guard to also show an **error variant** when `state==="failed" && error!==null`: filled recording-red banner, `role="alert"` reason line, destructive-outline **Restart recording** and a **Dismiss** control; hidden otherwise. Warning/live variants unchanged.
- `src/hooks/use-recording-session.ts` -- remember the last `start(...)` args in a ref; expose `restart()` (replays them) and `acknowledge()` (invokes `recording_acknowledge`, sets the returned idle snapshot). Polling already stops on terminal; ensure the failed snapshot is retained for the banner.
- `src/components/layout/recording-pane.tsx` -- relocate the header `"Recording failed: <error>"` note into the banner error variant (single surface, mirroring 18.3's header→banner consolidation); wire `onRestart`/`onDismiss`.
- `src/lib/ipc/client.ts` -- add `recordingAcknowledge(): Promise<RecordingStatusVm>` binding.
- `src/hooks/use-reduced-motion.ts`, `src/components/ui/alert.tsx`, `src/components/ui/button.tsx` -- reuse for the error banner chrome and buttons.

## Tasks & Acceptance

**Execution (dependency order):**
- [x] `src-tauri/crates/keeper-core/src/notify.rs` -- add the recording-fault/warning notify entry that bypasses DND/mute; unit-test that it posts regardless of DND and does not touch message-notification gating.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- fire the notification once on `error`/`warning` onset from both the event sink and the run_session-Err fallback (dedup so a fault never double-notifies); add `recording_acknowledge` (terminal→idle clearing error/warning; live→no-op) returning the snapshot; verify start-param replay is available for restart. Unit-test: fault-once, warning-once + no-refire-on-sticky, acknowledge-terminal-clears, acknowledge-live-noop.
- [x] `src-tauri/crates/keeper/icons/tray-error.png` -- add the recording-red filled error badge asset.
- [x] `src-tauri/crates/keeper/src/tray.rs` -- add the error-hold rendering + menu; keep the tray up on `Failed`+`error`; restore/drop on acknowledge/idle. Unit-test `decide_presence`/status-line for the error state (holds, names the reason) and that non-error terminals still restore/drop as before.
- [x] `src/lib/ipc/client.ts` + `src/hooks/use-recording-session.ts` -- `recordingAcknowledge` binding; `restart()` (replay last start args) and `acknowledge()` in the hook; retain the failed snapshot for the banner.
- [x] `src/components/recording/active-recording-banner.tsx` (+ `active-recording-banner.test.tsx`) -- add the error variant (filled recording-red, reason `role="alert"`, Restart/Dismiss, reduced-motion steady, recording-red not on buttons); cover the I/O-matrix banner rows (error visible on failed+error, hidden on live/finalized/idle/error==null, Restart→onRestart, Dismiss→onDismiss, assertive announcement).
- [x] `src/components/layout/recording-pane.tsx` (+ `recording-pane.test.tsx`) -- relocate the failed note into the banner; wire onRestart/onDismiss; update fixtures.
- [x] Induced-fault coverage -- Rust tests driving synthetic `error` events for the **recorder-kill / writer-stall / device-loss** legs (fake recorder, following `drive_session_failure_branch_returns_failed`) asserting the triad fires (state failed + error set + notification dispatched via a fake `Platform` capturing calls). Frontend test asserts the banner error variant renders for each.

**Acceptance Criteria:**
- Given a live recording, when the sidecar reports a fatal fault, then within 5 s the tray flips to the error badge naming the reason (and stays visible), a native notification is posted once, and the in-app banner shows the filled recording-red error variant with the reason and a Restart action — all reading the same `RecordingStatusVm.error`.
- Given a failed session, when the user clicks Restart, then a new session starts with the same target/system-audio/mic settings and all three error surfaces clear; when the user clicks Dismiss (banner) or "Dismiss Error" (tray), then `recording_acknowledge` returns the session to idle and the tray restores its prior configuration.
- Given a non-fatal warning first appears (e.g. mic hot-unplug), then a single native notification fires while the tray ⚠ line and banner amber warning persist (19.4 behavior unchanged) and no notification re-fires while the warning remains sticky.
- Given global Do-Not-Disturb is on, then a recording-fault notification is still delivered (it bypasses DND/mute); given message notifications, their gating is unchanged.
- Given `prefers-reduced-motion` or a screen reader, then any error dot is steady, the error is announced assertively, and `Esc` never stops or restarts the recording.
- Given macOS's purple recording pill, then keeper never touches or duplicates it; given `tauri-plugin-notification` cannot show action buttons, then the notification is the alert and the one-click restart is the banner button (documented, not a missing feature).

## Design Notes

- **No new state — surface what exists.** `Failed` + `error` and sticky `warning` already live on the snapshot (16.6/19.4). This story is wiring: tray render, notification dispatch, banner variant. Avoid adding a parallel fault model in Rust or TS.
- **Fire once, at the transition.** Both the event sink and the run_session-Err fallback can set `Failed`. Centralize "did `error` just become Some?" (compare prior snapshot under the lock) so the notification fires exactly once; a second `Failed` event is rejected by the terminal-state `apply`, and the fallback is guarded on not-already-terminal. Same None→Some rule for `warning`.
- **Why the tray must hold.** `decide_presence` treats terminals as drop/restore; that would hide the fault the instant it happens. Failed+error is a *hold-in-error* state until the user acknowledges or restarts — then the existing 18.2 restore/drop path runs. This is the only change to the tray lifecycle; Finalized/Recovered restore immediately as before.
- **Notification honesty under platform limits.** The desktop `tauri-plugin-notification` has no action buttons/click callback (Epic 11). So the notification names the reason and says to open keeper; the actual one-click Restart is the always-present banner button. Bypass DND/mute like `notify_bridge_disconnected` bypasses per-network mute — a failed recording must not be silenced by chat-notification settings, and the tray+banner legs fire regardless anyway.
- **Restart re-invokes Start with the live selection.** Restart is a frontend `recording_start` call reading the same module-level capture stores the Start button uses (source / system-audio / mic) — **not** a per-mount arg replay, so it stays honest across a Recording-view remount (which resets local hook state but never the stores). `recording_acknowledge` is the clean terminal→idle reset that lets the held tray restore. Keeping restart in the frontend avoids Rust-side param replay and a tray→frontend event channel. (Review pass hardened this: the initial per-mount `restart()` ref reverted a remounted view's Restart to the defaults.)
- **Scope fences.** Detection of writer-stall/device-loss/permission-revoke is not built here (sidecar/20.6); disk-floor *policy* is 18.5. 18.4 proves the surfacing with synthetic `error` events for the recorder-kill/writer-stall/device-loss legs.

## Verification

**Commands:**
- `cd src-tauri && cargo test -p keeper-core notify:: recording::` -- expected: recording-notify bypass + induced-fault surfacing tests pass.
- `cd src-tauri && cargo test -p keeper ipc:: tray::` -- expected: notify-once/dedup, acknowledge terminal-clear/live-noop, tray error-hold + reason line tests pass; existing tray tests green.
- `cd src-tauri && cargo build -p keeper && cargo clippy -p keeper -p keeper-core --all-targets -- -D warnings` -- expected: builds (new `tray-error.png` embeds), no warnings.
- `bun run check` -- expected: biome + tsc + vitest (banner error-variant + pane + hook restart/acknowledge tests) all green.

**Manual checks (if no CLI):**
- Tray error badge appearance/hold, notification delivery + copy, banner error variant + Restart/Dismiss, and VoiceOver assertive announcement are GUI behaviors validated on real hardware at a later acceptance milestone (SM-10 / Epic 20); automated coverage here is the Rust surfacing/dispatch/acknowledge tests and the presentational banner tests.

## Review Triage Log

### 2026-07-19 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 2: (high 0, medium 1, low 1)
- defer: 0
- reject: 18: (high 0, medium 0, low 18)
- addressed_findings:
  - `[medium]` `[patch]` Restart replayed a **per-mount** arg ref (`lastStartArgs`), so after a fault the user could leave and reopen the Recording view (the hook remounts, its ref resets) and click Restart — silently reverting their chosen source/mic to the main-display/mic-off defaults and recording the wrong thing. The capture selection lives in remount-surviving module-level stores (the same ones the Start button reads), so removed the hook's `restart`/`lastStartArgs` and routed the pane's Restart through `start(selectedRecordingTarget(), systemAudioEnabled(), micEnabled(), micDeviceId())`; added a pane **remount regression test** (non-default display + mic-on, adopt a failed session with no start this mount, click Restart → asserts the chosen selection reaches `recording_start`, not the defaults). Dropped the 3 now-obsolete hook `restart` tests.
  - `[low]` `[patch]` The sidecar `error`-event parse defaulted only a **missing** message, not a present-but-**blank** one (unlike the `warning` arm), so an empty/whitespace `message` reached all three loud surfaces reasonless ("Recording failed — "). Added the warning path's `.filter(|m| !m.trim().is_empty())` blank-filter at the source and extended `parse_error_without_message_still_fails_never_swallows` to assert empty/whitespace messages surface a `Failed` with a **non-blank** reason.
- notes: All 18 rejects are low, non-manifesting, or correct-by-construction. Rejected — the fallback guard's `|| Idle` arm (intentional early-pre-capture-failure surfacing; single-fire holds via the shared mutex + `None→Some` onset + terminal guard; only the doc's "not-already-terminal" wording is loose); a zombie/double notification after acknowledge (the orphaned driver Arc stays terminal so its guard blocks a re-fire); sub-second tray flicker on restart (self-healing via ForcePresent); three-surface fault-predicate "duplication" (the predicates agree and the `error`/`state` fields are the single source); Restart without an in-flight debounce and restart-while-live (banner error variant only renders on `failed`, so unreachable; idempotent re-starts are harmless); the TS-fabricated failed snapshot on an IPC **reject** (pre-existing 16.6 dead-channel error fallback; Rust stays authoritative whenever reachable); `error_rendered`/`status_item` mutual-exclusion by caller convention and the acknowledge two-lock monotonicity (both correct today — terminal is a sink state); pure-core-only dedup tests (exactly-once holds by construction); `render_error` partial icon/menu/flag mismatch (embedded validated PNG, best-effort, self-healing); the 5 s bound under a stalled main thread (the notification + banner legs fire event-driven immediately; the tray is best-effort GUI); warning resolve-then-recur not re-notified (19.4's sticky-warning model by design); `failed`+`error===null` rendering nothing (unreachable — `Failed` always carries a non-blank message, now doubly hardened by the parse patch — and a deliberately tested defensive choice); a later distinct `Failed` message not updating the tray line (a second `Failed` is rejected by the terminal `apply`, so the reason is immutable post-failure); Dismiss racing a just-finalized session (the Dismiss-Error item only exists on the `failed` menu, which a finalized session never shows); and unbounded notification copy length (the sidecar `error.message` is a controlled short RPC field, not raw stderr, and macOS clamps). The Story 18.4 spec Design Note "one-click restart lives in the banner" and the `tauri-plugin-notification` no-action-button limit were confirmed accurate, not gaps.

## Auto Run Result

Status: done

**Summary:** Wires the **loud-failure triad** on top of the recording state machine's existing `error`/`warning` snapshot fields (no new state model): on a fault (transition into terminal `Failed` with an `error`) all three surfaces fire — the menu-bar **tray holds visible and flips to a recording-red error badge** naming the reason (it no longer drops on `Failed`, which would have masked the fault), a **native notification** is posted through the existing AD-18 pipeline (bypassing DND/mute as a mandated triad leg), and the in-app **banner shows a filled recording-red error variant** with a one-click **Restart** and a **Dismiss**. A single notification also fires on **warning onset** (closing Story 19.4's deliberately-deferred native-notification leg). `recording_acknowledge` clears a terminal-failed session back to idle (no-op on a live session), and Restart re-invokes `recording_start` from the live capture stores. The notification leg is the loud *alert* (the plugin has no action buttons — Epic 11); the one-click restart is the banner button.

**Files changed:**
- `src-tauri/crates/keeper-core/src/notify.rs` — `notify_recording_fault`/`notify_recording_warning` (consult no `NotifyConfig` → bypass DND + per-Network mute; `NotifyTarget::None`; swallowed failures) + tests.
- `src-tauri/crates/keeper-core/src/recording.rs` — `error`-event parse now blank-filters its message (review patch), mirroring the `warning` arm, so the triad always names a reason; parse test extended for empty/whitespace.
- `src-tauri/crates/keeper/src/ipc.rs` — `fold_recording_event` (onset-deduped fault/warning notify under the snapshot mutex, dispatched after release) + `fail_recording_snapshot` fallback + `acknowledge_recording_slot`/`acknowledge_recording` + `#[tauri::command] recording_acknowledge`; driver task threads `Arc<dyn Platform>`; induced-fault + dedup + acknowledge tests with a `CapturingPlatform`.
- `src-tauri/crates/keeper/icons/tray-error.png` — new 44×44 recording-red filled error badge, embedded via `include_bytes!` and decode-validated in a test.
- `src-tauri/crates/keeper/src/tray.rs` — hold-in-error branch (`decide_presence(state, has_error, present, forced)` + `RenderError`/`render_error`/`build_error_menu`/`format_error_line`/`dismiss_recording_error`); holds on `Failed`+`error`, restores/drops on acknowledge/idle; warnings unchanged; tests.
- `src-tauri/crates/keeper/src/lib.rs` — registered `recording_acknowledge`.
- `src/lib/ipc/client.ts` — `recordingAcknowledge()` binding.
- `src/hooks/use-recording-session.ts` — `acknowledge()` (adopts the Rust-returned idle snapshot); removed the per-mount `restart`/`lastStartArgs` (review patch — restart now lives in the pane, reading the stores).
- `src/hooks/use-recording-session.test.ts` — acknowledge tests (obsolete restart-ref tests removed in the review patch).
- `src/components/recording/active-recording-banner.tsx` (+ test) — error variant (`data-variant="error"`, recording-red fill/edge + always-steady dot, `role="alert"` reason, destructive Restart + outline Dismiss, recording-red never on buttons); guarded on `state==="failed" && error!==null`.
- `src/components/layout/recording-pane.tsx` (+ test) — mounts the banner with `onRestart`/`onDismiss`; the failed note moved from the header into the banner; Restart reads the live capture stores (review patch) with a remount-safety regression test.

**Review findings breakdown:** 2 patches applied (1 medium: remount-safe Restart; 1 low: blank error-message filter at the source); 0 intent gaps; 0 bad-spec loopbacks; 0 deferred; 18 rejected (all low — correct-by-construction dedup/lock/acknowledge concurrency, an intentional early-failure guard, unreachable degenerate states, a pre-existing dead-IPC fallback, cosmetic sub-second flicker, agreeing cross-surface predicates, and test-thoroughness on provably-correct logic).

**Verification:** `cargo fmt --check` clean; `cargo clippy -p keeper -p keeper-core --all-targets -- -D warnings` clean; `cargo nextest run -p keeper-core -p keeper` → **933 passed**; `bun run check` → biome (346 files) clean, tsc clean, **1397 vitest passed / 131 files**, core zero-egress check passed. All gates re-run green after the two review patches.

**Residual risks:** GUI/VoiceOver behaviors (tray error-badge appearance + hold, native-notification delivery/copy, banner error variant, assertive announcement) are not automatable here — deferred to real-hardware acceptance (SM-10 / Epic 20). Live fault **detection** of writer-stall / device-loss / permission-revoke is out of scope (sidecar-emitted `error` events / Story 20.6); 18.4 surfaces any `Failed` fault and is proven with synthetic error events for the recorder-kill/writer-stall/device-loss legs. The disk-floor guard **policy** (warn 10 GB / hard-floor stop 2 GB) is Story 18.5, which reuses these surfaces. `oversized` spec warning retained (cross-layer, multi-file story).
