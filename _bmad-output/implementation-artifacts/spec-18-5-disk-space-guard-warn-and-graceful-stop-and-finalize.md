---
title: 'Story 18.5: Disk-Space Guard — Warn & Graceful Stop-and-Finalize'
type: 'feature'
created: '2026-07-19'
status: 'done'
baseline_revision: '8a67333eb57fd92ad10a64cef1930b845d9f3948'
final_revision: 'c45e6e91a783acc0ae2968224e0639068a63551e'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-18-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** A long recording can currently run its target volume to exhaustion or die mid-write. Pre-start free-space validation already exists (`evaluate_destination` blocks Start below the 2 GiB floor), but there is **no live guard**: once recording, nothing watches free space, so the disk can silently fill and a write eventually fails as an opaque fault instead of a graceful stop. The sidecar reports no free-space figure and the state machine has no low-disk policy.

**Approach:** Add a **live disk-space guard** as platform-free **policy in `keeper-core::recording`** (pure functions over a simulated free-space figure), driven by the shell probing the target volume on a ~1 Hz tick during an active session (reusing the same `fs4::available_space` the pre-start check uses — the sidecar is *not* extended). Below the warn threshold (authored **10 GB**) the guard raises a persistent sticky warning through the existing 19.4/18.4 warning surface (tray ⚠ line, in-app banner amber, one native notification). Below the hard floor (authored **2 GB**) it performs a **graceful stop-and-finalize** via the existing idempotent `stop_active_recording` (→ `Stopping` → `Finalized`, all segments written) plus a single "stopped — low disk" notification — never a `Failed` fault, never a mid-write death.

## Boundaries & Constraints

**Always:**
- All disk-guard **policy** — thresholds, warn/floor decision, one-shot latching, and the user-facing copy — lives in `keeper-core::recording` as pure, platform-free functions (AD-33/AD-39). The shell's only job is to *measure* free space and *execute* the returned action.
- Pre-start free-space validation (existing `evaluate_destination`, floor = `RECORDING_MIN_FREE_BYTES`) must keep blocking Start with an actionable error when free space is already below the floor.
- The live guard probes the target volume's free space on a ~1 Hz tick while the session is live; in the warn band it raises a **persistent** sticky warning (tray ⚠ line + banner amber + one native notification); below the hard floor it runs a graceful stop that reaches `Finalized`.
- Each distinct disk-guard event notifies **exactly once** — warn onset once, and the hard-floor stop once **even if a warn already fired** (they are two distinct events) — and never re-fires while a threshold stays crossed. Notifications reuse `notify_recording_warning` (bypasses DND / per-network mute, like the 18.4 warning leg).
- A free-space probe **error** is treated as "plenty" (fail-open): never warn or stop on a failed `stat`.
- Thresholds are **authored constants** (`RECORDING_WARN_FREE_BYTES` = 10 GB, `RECORDING_MIN_FREE_BYTES` = 2 GiB) — product-owner sign-off items at phase release (PRD §14.7), changeable in one edit; not a settings row.

**Block If:**
- The Epic 17 stop→finalize contract is absent or changed such that an idempotent graceful stop can no longer reach `Finalized` — the hard-floor leg would then have nothing safe to call (would risk a mid-write death instead of a clean stop).

**Never:**
- Never extend the Swift sidecar or its NDJSON schema to report free space, and never measure disk from within `keeper-core` — measurement stays in the shell (`fs4`), consistent with the pre-start check (see Design Notes).
- Never add a user-facing settings control for the thresholds — no `registry.rs` / `RecordingSettingsVm` / UI change.
- Never run the volume to exhaustion, die mid-write, or surface the low-disk stop as a `Failed` fault; never re-issue the stop after it fires once.
- Never auto-clear the sticky low-disk warning when space recovers (consistent with 19.4's sticky-warning model); ending/acknowledging the session clears it.
- Never re-implement 18.4/19.4 warning/notification surfaces, touch macOS's purple recording pill, or add a Pause affordance.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| pre-start insufficient | Start request, free < 2 GiB floor | Start rejected with actionable free-space error before any capture begins | existing `evaluate_destination` |
| pre-start sufficient | free ≥ 2 GiB (other checks pass) | Start proceeds | — |
| warn crossing | live session, tick probes free in [2 GiB, 10 GB) | sticky warning raised ("Low disk space — N GB free"): tray ⚠ + banner amber + **one** notification; recording continues | — |
| warn sticky | later ticks still in warn band | message may refresh; **no** new notification; recording continues | latched `warned` |
| hard-floor crossing | live, tick probes free < 2 GiB | graceful stop: `stop_active_recording` → `Stopping` → `Finalized`; **one** "stopped — low disk" notification; sticky reason set; issued exactly once | latched `stopped`; never `Failed` |
| sudden drop | live, free jumps from ≥ 10 GB to < 2 GiB in one tick | Stop action only (warn skipped); one stop notification | — |
| space recovers | free rises back ≥ 10 GB after a warn | sticky warning persists (19.4 model); not auto-cleared | — |
| probe failure | `fs4::available_space` errs | free treated as `u64::MAX` (plenty); no warn/stop | fail-open |
| user stop races guard | guard Stop + user/⌘Q Stop | idempotent (one-shot taken once); single finalize | — |
| session already terminal | tick after Finalized/Failed or slot cleared | guard task exits; no probe, no action | — |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/recording.rs` -- add `RECORDING_WARN_FREE_BYTES` (10 GB) next to the existing `RECORDING_MIN_FREE_BYTES` (2 GiB, ~L428); add the pure policy: `DiskGuardDecision { Ok, Warn, Stop }` + `evaluate_disk_guard(free_bytes, warn_bytes, floor_bytes)` (free < floor ⇒ Stop; floor ≤ free < warn ⇒ Warn; else Ok), and the latched action layer `DiskGuardLatch { warned, stopped }` + `DiskGuardAction { None, Warn{message}, Stop{message} }` + `plan_disk_guard_action(free_bytes, warn_bytes, floor_bytes, &mut latch)` (returns each event's action at most once, formats the low-disk copy). All platform-free.
- `src-tauri/crates/keeper/src/ipc.rs` -- capture the resolved destination dir (from `effective_destination_dir`, ~L3616) into `RecordingRun` (~L3839) so the guard knows which volume to probe; at session start (driver-spawn region ~L3731) spawn a **~1 Hz disk-guard task** that, while the session is live, probes `fs4::available_space(dir)` (reusing the pre-start probe idiom, ~L3624), calls `plan_disk_guard_action`, and executes the action via a small `apply_disk_guard_action` helper: Warn/Stop → set `status.warning` under `status_lock` (drop lock before notifying) + `notify_recording_warning` once; Stop → additionally fire `stop_active_recording` (~L3855). Task exits on terminal state or a cleared slot. Pre-start free-space validation (~L3616-3641) already present — leave intact, add/confirm coverage.
- `src-tauri/crates/keeper-core/src/notify.rs` -- the **warn** leg reuses the existing `notify_recording_warning` (DND/mute bypass); the **hard-floor stop** leg uses a dedicated sibling `notify_recording_stopped` (same DND/mute-bypass + `NotifyTarget::None` + swallowed-failure posture) because the warning entry unconditionally appends "the recording is still running" — which would contradict a stop notification (review F1).

## Tasks & Acceptance

**Execution (dependency order):**
- [x] `src-tauri/crates/keeper-core/src/recording.rs` -- add `RECORDING_WARN_FREE_BYTES` + `DiskGuardDecision`/`evaluate_disk_guard` + `DiskGuardLatch`/`DiskGuardAction`/`plan_disk_guard_action` with the low-disk copy. Unit-test over **simulated** `free_bytes`: Ok / Warn / Stop bands incl. exact-boundary (`free == warn` ⇒ Ok, `free == floor` ⇒ Warn), warn-then-stop yields two actions once each, sudden drop yields Stop only, warn-sticky yields no repeat, post-stop always `None`.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- thread the destination dir into `RecordingRun`; spawn the ~1 Hz guard task; add `apply_disk_guard_action`. Unit-test `apply_disk_guard_action` with a `CapturingPlatform` (18.4) + a captured stop flag + a `status` mutex: Warn sets `warning` + notifies once; Stop sets `warning` + notifies once + requests stop; a probe of `u64::MAX` (error) is a no-op; a second same-band tick does not re-notify.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- confirm (add if missing) a pre-start test asserting Start is rejected with the free-space reason when the probed `free_bytes` is below `RECORDING_MIN_FREE_BYTES`.

**Acceptance Criteria:**
- Given the authored thresholds, when free space is evaluated, then the warn/floor decision, latching, and copy are produced entirely by `keeper-core` pure functions and are unit-tested via a simulated low-free-space signal (no disk fill).
- Given a running recording whose target-volume free space falls into the warn band, then within ~1 s a persistent low-disk warning shows on the tray line and in-app banner and a single native notification is posted (bypassing DND), and recording continues uninterrupted.
- Given free space then falls below the hard floor, when the guard fires, then the session gracefully stops and finalizes (reaches `Finalized`, all segments written), a single "stopped — low disk" notification is posted, and the stop is never surfaced as a `Failed` fault and is never re-issued.
- Given a Start request on a volume already below the hard floor, then Start is rejected with an actionable free-space error before any capture begins.
- Given a free-space probe failure mid-session, then no warning or stop is triggered (fail-open to "plenty").

## Design Notes

- **Why the shell measures, not the sidecar.** AD-39 says the guard is "driven by free-space on `state` events," but the sidecar today emits no free-space figure and its `state` events are transition-only (too sparse — segment rotations are ~500 MB apart). Rather than rebuild the Swift sidecar and change the NDJSON schema, the shell probes `fs4::available_space(dir)` on a ~1 Hz tick — the **same** probe the pre-start `evaluate_destination` check already uses. This keeps `keeper-core` platform-free (measurement outside core, policy inside core — the real intent of the AD-33/AD-39 split) and makes both legs of the guard use one consistent measurement path.
- **Two distinct notifications, each once.** Warn onset and the hard-floor stop are separate events; a user who ignored the 10 GB warn must still be pinged when it actually stops. The `DiskGuardLatch` (`warned`, `stopped`) guarantees each action is emitted at most once, so notifications don't rely on — and don't collide with — 18.4's warning-onset dedup. A session that plunges straight past both thresholds in one tick emits only the Stop (skips the warn). The two legs post through **different** notify entries: the warn reuses `notify_recording_warning` (whose copy asserts "the recording is still running"), the stop uses `notify_recording_stopped` (no such suffix) — same DND/mute bypass on both, but the stop must not tell the user the recording is still running (review F1).
- **Graceful stop reuses the normal Stop.** The hard-floor leg calls the exact idempotent `stop_active_recording` the tray/⌘Q use, so the session runs the normal finalize path to `Finalized` — the low-disk stop is honest cleanup, not a fault. The sticky `warning` carries the reason through the live phase; the notification is the durable signal for the auto-stop.
- **No frontend change.** The tray ⚠ line (19.4) and banner amber warning variant (19.4/18.4) already render `RecordingStatusVm.warning`; the guard only feeds that field. `Failed`/error surfaces are untouched.
- **Thresholds are constants.** 10 GB / 2 GiB are authored defaults pending PO sign-off (PRD §14.7); a one-line edit changes them. Testability comes from the pure `plan_disk_guard_action` taking a simulated `free_bytes`, so no disk is ever filled.

## Verification

**Commands:**
- `cd src-tauri && cargo test -p keeper-core recording::` -- expected: `evaluate_disk_guard` + `plan_disk_guard_action` band/boundary/latch tests pass.
- `cd src-tauri && cargo test -p keeper ipc::` -- expected: `apply_disk_guard_action` (warn/stop notify-once + stop-request + fail-open) and the pre-start free-space rejection tests pass; existing recording tests green.
- `cd src-tauri && cargo build -p keeper && cargo clippy -p keeper -p keeper-core --all-targets -- -D warnings` -- expected: builds, no warnings.
- `bun run check` -- expected: still green (no frontend change; reuses the 19.4 warning render).

**Manual checks (if no CLI):**
- Live tray ⚠ line + banner amber under a low-disk warn, the graceful stop-and-finalize copy, and native-notification delivery are GUI behaviors validated on real hardware at a later acceptance milestone (SM-10 / Epic 20); automated coverage here is the Rust policy + action-executor tests driven by a simulated free-space signal.

## Review Triage Log

### 2026-07-19 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 1: (high 0, medium 1, low 0)
- defer: 1: (high 0, medium 1, low 0)
- reject: 12: (high 0, medium 0, low 12)
- addressed_findings:
  - `[medium]` `[patch]` The hard-floor **stop** notification reused `notify_recording_warning`, whose body unconditionally appends "— the recording is still running" — so the auto-stop alert read "Recording stopped — low disk — the recording is still running", a self-contradiction on the one event the epic wants unmistakable. Added a dedicated `notify_recording_stopped` entry (title "Recording stopped", reason verbatim, identical DND/mute-bypass + `NotifyTarget::None` + swallowed-failure posture) and routed the Stop leg of `apply_disk_guard_action` to it (the Warn leg still uses `notify_recording_warning`). Strengthened the two shell stop tests to assert the exact title/body and the absence of "still running", and added three `notify_recording_stopped` core tests (copy-verbatim, DND/mute bypass, swallowed failure). The intent-contract's "notifications reuse `notify_recording_warning`" invariant is preserved in spirit — its captured intent is *one DND-bypassing notification per event*, which both legs honor; only the baked warn-copy was unfit for the stop leg (Code Map + Design Notes reconciled outside the read-only contract).
- notes: **Deferred (1):** F2 — `RecordingStatusVm.warning` is a single `Option<String>` with two now-independent writers (the sidecar sink for 19.4 mic-unplug warnings, and the disk guard), so a co-occurring mic-unplug and low-disk warning clobber each other last-write-wins. Real, but the single-slot warning model is pre-existing (19.4) and this story deliberately scoped out reworking it ("reuse the 19.4 sticky-warning surface"); reconciling multiple concurrent warnings is a focused warning-model change, not a disk-guard patch — logged to deferred-work. **Rejected (12, all low):** guard can act during `Preflight` (needs free < 2 GiB within ~1 s of a Start that just validated ≥ 2 GiB — near-impossible; stop is idempotent regardless); 10 GiB constant vs decimal-GB copy (the GiB idiom mirrors the existing 2 GiB floor and is an authored PO-sign-off value; the copy shows honest *actual* decimal free space); guard probes the parent destination dir (same volume — free space is per-volume, and the parent is guaranteed to exist unlike a transient session subfolder); `fs4::available_space` runs inline on the runtime (fail-open, one worker at most, and identical to the existing pre-start probe idiom; process exit reaps the task on quit); guard keeps probing during the `Stopping`→`Finalized` window (latch blocks any re-stop; ≤1 benign extra probe); `is_terminal()` treats `Idle` as terminal (guard's own Arc never becomes `Idle`; correct-by-construction); fail-open `u64::MAX` sentinel is a doc-contract not a type (current shell caller is correct; a hardening suggestion, not a live bug); notify posted microseconds before `request_stop` (sub-tick ordering; disk *is* low so the message is true even in a user-stop race); no `debug_assert!(floor ≤ warn)` (constants are correct: 10 GiB > 2 GiB); test `.expect_err`/`.expect` (allowed in `#[cfg(test)]` per project rules); ~1 s first-probe latency (inherent to the ~1 Hz cadence the spec authorizes; 2 GiB floor is ample headroom); theoretical `Arc::ptr_eq` ABA (the exiting guard holds the old status `Arc`, so its address cannot be freed+reused while it still ticks — correct-by-construction).

## Auto Run Result

Status: done

**Summary:** Adds the **live disk-space guard** for Recording Sessions. Pre-start free-space validation already existed (`evaluate_destination` blocks Start below the 2 GiB floor); this story adds the during-recording half as platform-free **policy in `keeper-core::recording`** — `evaluate_disk_guard` (bands: `free < floor` ⇒ Stop, `floor ≤ free < warn` ⇒ Warn, else Ok) and a latched `plan_disk_guard_action` that emits each distinct event at most once and owns the user copy — with the shell (`keeper/src/ipc.rs`) *measuring* free space (the same `fs4::available_space` the pre-start gate uses) on a ~1 Hz guard task per session and *executing* the returned action. Below the warn threshold (`RECORDING_WARN_FREE_BYTES`, 10 GiB) a persistent sticky warning is raised through the existing 19.4/18.4 surface (tray ⚠ line + banner amber + one native notification); below the hard floor (`RECORDING_MIN_FREE_BYTES`, 2 GiB) the guard fires the existing idempotent `stop_active_recording`, so the session finalizes on the normal graceful path (`Stopping` → `Finalized`, never a `Failed` fault) with one "stopped — low disk" notification. Fail-open on probe error; the guard exits when its session goes terminal or its run slot is cleared/replaced (`Arc::ptr_eq`). Thresholds are authored constants (PO sign-off), and the whole guard unit-tests via a simulated free-space figure — no disk is ever filled. No frontend change (the warning surfaces already render `RecordingStatusVm.warning`).

**Files changed:**
- `src-tauri/crates/keeper-core/src/recording.rs` — `RECORDING_WARN_FREE_BYTES` (10 GiB) beside the 2 GiB floor; `DiskGuardDecision`/`evaluate_disk_guard`; `DiskGuardLatch`/`DiskGuardAction`/`plan_disk_guard_action` (pure, platform-free, latched, owns the copy) + 6 unit tests over simulated `free_bytes`.
- `src-tauri/crates/keeper-core/src/error.rs` — `format_gb` promoted to `pub(crate)` so the warn copy names free space with the pre-start rejection's formatting.
- `src-tauri/crates/keeper-core/src/notify.rs` — `notify_recording_stopped` (review F1 patch): a dedicated stop-notification entry (title "Recording stopped", reason verbatim, same DND/mute-bypass + `NotifyTarget::None` + swallowed-failure posture as the warn/fault legs) so the auto-stop alert never inherits the warning entry's "still running" suffix; + 3 tests.
- `src-tauri/crates/keeper/src/ipc.rs` — `RecordingRun.destination_dir` (session-captured volume); `DISK_GUARD_POLL` (1 s); `apply_disk_guard_action` (sets sticky `warning` under the lock, notifies after releasing it, Stop additionally fires the stop closure — Warn routes to `notify_recording_warning`, Stop to `notify_recording_stopped`); `recording_start` gained a `tauri::AppHandle` param and spawns the ~1 Hz guard task (fail-open `fs4` probe → `plan_disk_guard_action` → execute; exits on terminal/cleared/replaced slot); 5 tests (warn-once, warn-then-stop, sudden-drop, fail-open no-op, pre-start free-space rejection) hardened to assert exact stop title/body and absence of "still running".
- `spec-18-5-…md` — task checkboxes; Code Map + Design Notes reconciled to the dedicated stop-notify entry; Review Triage Log; this result.

**Review findings breakdown:** 1 patch applied (medium: the self-contradicting stop notification — dedicated `notify_recording_stopped` entry + strengthened tests); 1 deferred (the single-slot `warning` collision between 19.4 mic-unplug and low-disk warnings — pre-existing warning-model limitation, logged to `deferred-work.md`); 12 rejected (all low: correct-by-construction latch/`Arc` identity, fail-open consistent with the existing pre-start probe idiom, authored GiB thresholds vs honest decimal-GB copy, near-impossible Preflight race, and hardening suggestions on already-correct code). 0 intent gaps, 0 bad-spec loopbacks.

**Verification:** `cargo fmt --check` clean; `cargo clippy -p keeper -p keeper-core --all-targets -- -D warnings` clean; `cargo nextest run -p keeper-core -p keeper` → **947 passed** (incl. 14 new disk-guard/notify tests); `bun run check` → biome (346 files) clean, tsc clean, **1397 vitest passed / 131 files**, core zero-egress (tauri-free) check passed.

**Residual risks:** GUI/notification behaviors (tray ⚠ line + banner amber under a live low-disk warn, native-notification delivery/copy, the graceful stop-and-finalize reveal) are not automatable here — deferred to real-hardware acceptance (SM-10 / Epic 20). The warn/floor thresholds (10 GiB / 2 GiB) are authored PO-sign-off defaults, changeable in one edit. The deferred single-`warning`-slot collision (F2) can transiently hide a co-occurring mic-unplug or low-disk warning until the warning model is reconciled. `oversized` spec warning retained (cross-layer core+shell story).
