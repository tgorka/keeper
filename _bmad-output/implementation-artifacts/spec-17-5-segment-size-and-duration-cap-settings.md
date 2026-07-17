---
title: 'Segment-Size & Duration-Cap Settings'
type: 'feature'
created: '2026-07-17'
status: 'done'
baseline_revision: 'edf59e2aca06f13f13404d6370e8f30e10858f4c'
final_revision: '774eb00ed5c844a260d68248bd7914f235cd096f'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-17-context.md'
  - '{project-root}/docs/project-context.md'
warnings: [oversized]
---

<intent-contract>

## Intent

**Problem:** Story 17.1 rotates recordings at a segment size and a duration-cap fallback, but those two knobs are hardcoded: the sidecar receives no `segmentMB`/`maxSegmentSeconds` today (the Rust `SessionParams` omits them), and the user can neither see nor change them. FR-72 requires them to be user-configurable, persisted, and applied to future sessions.

**Approach:** Persist `recording.segment_mb` (default 500) and `recording.duration_cap_minutes` (default 30) in `keeper.db` via `keeper_core::registry` (the existing `settings` k/v table — the epic's "keeper-core::settings"), expose them through get/set Tauri commands plus a `RecordingSettingsVm`, thread the stored values into the sidecar `start` params (already-supported wire fields), and render a shared segment-size + duration-cap control in both the Settings → Recording section and the pre-record "Segmenting" setup card. A single frontend store keeps the two surfaces mirrored; edits affect the next session only.

## Boundaries & Constraints

**Always:**
- Both values persist in the `settings` k/v table via new `keeper_core::registry` getters/setters that mirror the `undo_send.window` precedent: absent/unparsable ⇒ default; a stored value is clamped defensively on read AND on write. Defaults: segment 500 MB, duration cap 30 min. Clamps (authored, adjustable on dogfooding without a spec change): segment `100..=5000` MB, duration cap `1..=600` min.
- `SessionParams` (`recording.rs`) gains `segment_mb: u32` and `max_segment_seconds: u32`; `start_recording_request` emits them as `segmentMB` and `maxSegmentSeconds` in the `start` wire params. These are **additive** fields the sidecar already reads (17.1) — `PROTOCOL_VERSION` stays **1** (16.5/16.6/17.4 additive precedent).
- `recording_start` reads the two settings from the registry at start time and populates `SessionParams`, converting duration-cap minutes → seconds (30 min → 1800 s). Values are read at start, so a running session is never mutated — edits apply to the next session only.
- Get/set exposed as Tauri commands returning/accepting a `RecordingSettingsVm { segmentMb, durationCapMinutes }` (ts-rs `#[ts(export)]`, camelCase, mirroring `RecordingStatusVm`). The setter clamps, persists, and returns the effective (clamped) VM so the UI never displays an unsaved value.
- Both surfaces (Settings → Recording, and the recording-pane "Segmenting" card) render the **same** shared control component bound to one frontend store, so editing either writes the same value and both reflect it live (the `incognito` store precedent). Copy is sentence case; "Recording Session" and "segment" follow the glossary; a helper line states edits apply to the next session.
- Recording UI stays capability-gated (`capabilities.recording`), as the existing Recording surfaces already are.

**Block If:**
- Making the sidecar honor the configured values would require changing 17.1's shipped rotation Swift or bumping `PROTOCOL_VERSION` (i.e. the wire fields are NOT already consumed) — HALT `blocked` (this story adds no sidecar capture/rotation code).

**Never:**
- No `tauri-plugin-store` / `tauri-plugin-sql` — settings live in `keeper.db` behind `keeper_core::registry` only.
- No change to `keeper-rec` capture/rotation Swift (`Capture.swift`, `Rotation.swift`, `main.swift`) — the sidecar already reads `segmentMB`/`maxSegmentSeconds`; this story only starts sending them.
- No live segment-fill meter ("segment N · 412 / 500 MB") — that is Epic 18 (18.3). No Destination folder chooser — that is Epic 19 (the "Destination" setup card stays a placeholder here).
- No mutation of an in-flight session's segment size or duration cap.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Defaults on fresh install | no `recording.*` rows in `settings` | `get` returns `{segmentMb: 500, durationCapMinutes: 30}` | No error |
| Set within range | set `{800, 45}` | persisted; `get` returns `{800, 45}`; next `start` sends `segmentMB:800, maxSegmentSeconds:2700` | No error |
| Set below floor | set `{segmentMb: 10, durationCapMinutes: 0}` | clamped to `{100, 1}` on write; setter returns the clamped VM | Clamp, not reject |
| Set above ceiling | set `{segmentMb: 99999, durationCapMinutes: 5000}` | clamped to `{5000, 600}` | Clamp, not reject |
| Corrupt stored value | `settings` has `recording.segment_mb = "abc"` | `get` returns default 500 (unparsable ⇒ default) | Tolerant fallback |
| Start passes config | settings `{800, 45}`, `recording_start` fires | `start` NDJSON params include `segmentMB:800` and `maxSegmentSeconds:2700` | No error |
| Cross-surface mirror | edit segment size in Settings dialog | recording-pane "Segmenting" card shows the new value (one store) | No error |
| Edit during recording | active session + user edits value | store/db update; the running session's params are unchanged | No error |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/registry.rs` -- Add `RECORDING_SEGMENT_MB_KEY`/`RECORDING_DURATION_CAP_MINUTES_KEY` consts + `*_DEFAULT`/`*_MIN`/`*_MAX` consts and clamp-on-read/clamp-on-write get/set fns, mirroring `get_undo_send_window`/`set_undo_send_window` (lines 729–756). Unit tests.
- `src-tauri/crates/keeper-core/src/vm.rs` -- New `RecordingSettingsVm { segment_mb: u32, duration_cap_minutes: u16 }` next to `RecordingStatusVm` (line 2703): same `#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]` + `#[serde(rename_all="camelCase")]` + `#[ts(export)]`.
- `src-tauri/crates/keeper-core/src/recording.rs` -- `SessionParams` (line 372) gains `segment_mb: u32` + `max_segment_seconds: u32`; `start_recording_request` (line 386) adds `wire["segmentMB"]`/`wire["maxSegmentSeconds"]`. Unit test the wire JSON.
- `src-tauri/crates/keeper/src/ipc.rs` -- New `recording_settings_get`/`recording_settings_set` commands (registry get/set → `RecordingSettingsVm`; setter clamps + returns effective VM). In `recording_start` (SessionParams construction, ~line 3400) read both settings from `state.platform.data_dir()` via the new registry fns and populate the new fields (minutes → seconds).
- `src-tauri/crates/keeper/src/lib.rs` -- Register `recording_settings_get`/`recording_settings_set` in `generate_handler!` (near line 320).
- `src/lib/ipc/gen/RecordingSettingsVm.ts` -- ts-rs-generated binding (emitted by the export test).
- `src/lib/ipc/client.ts` -- `recordingSettingsGet()` / `recordingSettingsSet(vm)` wrappers (mirror `notifyGetPreviewEnabled`/`notifySetPreviewEnabled`).
- `src/lib/stores/recording-settings.ts` -- Zustand vanilla store (the `incognito.ts` precedent): `ensureHydrated()` (lazy `recordingSettingsGet`), `apply(next)` (optimistic + `recordingSettingsSet` → replace with effective VM, revert on failure). Both surfaces bind to it.
- `src/components/settings/recording-settings-controls.tsx` -- Shared control: segment-size stepper (MB) + duration-cap field (min) from `@/components/ui` primitives; clamp/validate on blur; helper copy "Applies to the next Recording Session." Colocated `*.test.tsx`.
- `src/components/settings/settings-dialog.tsx` -- Replace the Recording-section placeholder (~line 992) with `<RecordingSettingsControls/>`; hydrate the store on open.
- `src/components/layout/recording-pane.tsx` -- Render `<RecordingSettingsControls/>` in the "Segmenting" setup card (SETUP_CARDS ~line 51); "Destination" card stays a placeholder (Epic 19).

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/registry.rs` -- Add the two numeric settings (consts, clamp-on-read/write get/set) mirroring `undo_send.window`; unit-test default, round-trip, and both clamp edges + unparsable fallback.
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- Add `RecordingSettingsVm` with the `RecordingStatusVm` derive/attr set.
- [x] `src-tauri/crates/keeper-core/src/recording.rs` -- Add `segment_mb`/`max_segment_seconds` to `SessionParams`; emit `segmentMB`/`maxSegmentSeconds` in `start_recording_request`; unit-test the wire params.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- Add `recording_settings_get`/`recording_settings_set`; populate the new `SessionParams` fields in `recording_start` from the registry (minutes→seconds).
- [x] `src-tauri/crates/keeper/src/lib.rs` -- Register the two new commands.
- [x] `src/lib/ipc/client.ts` (+ regenerated `gen/RecordingSettingsVm.ts`) -- Add `recordingSettingsGet`/`recordingSettingsSet`.
- [x] `src/lib/stores/recording-settings.ts` -- Shared store (hydrate + optimistic apply with revert).
- [x] `src/components/settings/recording-settings-controls.tsx` (+ `.test.tsx`) -- Shared segment-size + duration-cap control; test clamp-on-blur, persist call, and store-driven mirroring.
- [x] `src/components/settings/settings-dialog.tsx` -- Mount the control in the Recording section; hydrate on open.
- [x] `src/components/layout/recording-pane.tsx` -- Mount the control in the "Segmenting" card.

**Acceptance Criteria:**
- Given a fresh install (no `recording.*` settings), when the Recording settings load, then segment size shows 500 MB and duration cap 30 min, and a `recording_start` sends `segmentMB:500`, `maxSegmentSeconds:1800`.
- Given a user sets segment size to 800 MB and duration cap to 45 min in either surface, when the other surface is shown, then it displays 800 MB / 45 min, and the next `recording_start` sends `segmentMB:800`, `maxSegmentSeconds:2700`.
- Given a user enters an out-of-range value, when it is committed, then it is clamped to the authored bounds and the displayed value equals the persisted (clamped) value — never an unsaved value.
- Given a recording is in progress, when the user edits a value, then the running session is unaffected and the change applies only to the next session.

## Review Triage Log

### 2026-07-17 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 3: (high 0, medium 1, low 2)
- defer: 1: (high 0, medium 1, low 0)
- reject: 8: (high 0, medium 1, low 7)
- addressed_findings:
  - `[medium]` `[patch]` The `recording-settings` store reverted a failed write to the *live* (possibly still-optimistic) store value, and the control's `commit` built the full VM from the closed-over render snapshot — so two racing edits could restore a never-confirmed value or clobber the co-field. Fixed: the store now tracks the last Rust-confirmed VM and reverts to it; `commit` reads `recordingSettingsStore.getState()` at commit time and derives the co-field from live state (dropping the misleading `?? 0` sentinels).
  - `[low]` `[patch]` `Number.parseInt` silently truncated partial-numeric input (`"500abc"→500`, `"1e3"→1`, `"3.9"→3`) into a wrong-but-valid persisted value. Switched to `Number()` with trim + empty-string + `Number.isFinite` guards and `Math.round`, so junk is discarded like a non-numeric entry.
  - `[low]` `[patch]` The `"Segmenting"` card mounts the live control via a brittle `title === "Segmenting"` string match with no test covering it. Added a `recording-pane` regression test asserting the segment-size + duration-cap fields render inside the card.

Findings judged **defer** (1): the `recording_settings_set` "clamp, not reject" contract is false for out-of-`u16`/`u32` values from a non-UI IPC caller (serde rejects before the clamp) — no user-facing impact (the control clamps first) → `deferred-work.md`.
Findings judged **reject** (faithful to contract, by-design, or not reachable): the two settings rows read/written non-atomically (no invariant links two independent scalars; a torn read yields one valid session that self-heals next start); the `minutes × 60` multiply is overflow-safe under the `≤600` clamp (well below `u32::MAX`); non-numeric silent-discard is the spec'd, tested UX; editing during recording applying only next session is the spec'd behavior (FR-72), no UI signal required; the module-global `writeId`/`hydration` singletons match the `incognito.ts` precedent; the `u16`/`u32` VM field-width asymmetry is cosmetic (both serialize to TS `number`); the hydration-strands-mirror scenario is unreachable (inputs are `disabled` and `commit` guards on `settings !== null`, so `writeId` stays 0 during hydration).

## Design Notes

**"Settings stream" ⇒ a shared frontend store, not a backend Channel.** The epic asks the two surfaces to "mirror each other." The codebase realizes cross-surface setting sync with a frontend zustand store (`incognito.ts`), and has **no** backend Tauri-Channel-for-settings precedent (Channels stream genuinely live data — bridge health, timeline). With a single webview and no external mutator, a backend broadcast would be dead weight. One `recording-settings` store hydrated once via `recordingSettingsGet` and updated on every `apply` keeps both surfaces mirrored; `keeper.db` remains the source of truth, re-read at each `recording_start`.

**Units.** Segment size is stored and shown in MB (matches the sidecar `segmentMB`, 1 MB = 1,000,000 bytes). Duration cap is stored and shown in **minutes** (the UX field), converted to seconds only when building `start` params — `duration_cap_minutes * 60` → `maxSegmentSeconds` (30 → 1800, the sidecar's own default).

**Clamp on both read and write** (the `undo_send.window` discipline): the setter clamps before persisting AND the getter clamps on read, so a hand-edited DB row or a future bound change can never surface an out-of-range value. Host clamps sit inside the sidecar's defensive `1..=1_000_000` MB / `≥1` s backstop.

## Verification

**Commands:**
- `bun run check:rust` -- expected: rustfmt clean, clippy `-D warnings` clean.
- `bun run test:rust` -- expected: green, including the new registry clamp/default tests and the `start_recording_request` wire test.
- `bun run check` -- expected: biome + tsc + vitest green, including the `recording-settings-controls` test and regenerated `RecordingSettingsVm.ts`.

**Manual checks (if no CLI):**
- Confirm `RecordingSettingsVm.ts` was regenerated and imported by `client.ts`.
- Confirm the Recording section and the "Segmenting" card show identical values and that editing one updates the other.

## Auto Run Result

Status: done

**Implemented change.** Story 17.5 makes recording segment-size and duration-cap user-configurable (FR-72). Two numeric settings persist in `keeper.db` behind `keeper_core::registry` (the `settings` k/v table — the epic's "keeper-core::settings") mirroring the `undo_send.window` precedent: `recording.segment_mb` (default 500, clamp `100..=5000`) and `recording.duration_cap_minutes` (default 30, clamp `1..=600`), clamped on both read and write. A `RecordingSettingsVm` is exposed through `recording_settings_get`/`recording_settings_set` commands (the setter clamps then returns the effective VM), and `recording_start` re-reads both settings at start time and threads them into the sidecar `start` params as `segmentMB` / `maxSegmentSeconds` (minutes → seconds; `PROTOCOL_VERSION` stays 1 — additive fields the 17.1 sidecar already reads). A shared React control (segment-size + duration-cap fields, clamp-on-blur) renders in BOTH the Settings → Recording section and the pre-record "Segmenting" setup card, bound to one `recording-settings` zustand store so the two surfaces mirror each other live; edits apply to the next session only.

**Files changed.**
- `src-tauri/crates/keeper-core/src/registry.rs` — the two clamped numeric settings + consts + unit tests (defaults, round-trip, both clamp edges, garbage/hand-edited-row fallback).
- `src-tauri/crates/keeper-core/src/vm.rs` — `RecordingSettingsVm { segment_mb: u32, duration_cap_minutes: u16 }` (ts-rs `#[ts(export)]`, camelCase).
- `src-tauri/crates/keeper-core/src/recording.rs` — `SessionParams` gains `segment_mb`/`max_segment_seconds`; `start_recording_request` emits `segmentMB`/`maxSegmentSeconds`; wire unit test.
- `src-tauri/crates/keeper/src/ipc.rs` — `recording_settings_get`/`recording_settings_set`; `recording_start` populates the params from the registry (minutes→seconds).
- `src-tauri/crates/keeper/src/lib.rs` — the two commands registered in `generate_handler!`.
- `src/lib/ipc/gen/RecordingSettingsVm.ts` *(new)* + `src/lib/ipc/client.ts` — generated binding + `recordingSettingsGet`/`recordingSettingsSet`.
- `src/lib/stores/recording-settings.ts` *(new)* — shared store (lazy dedup hydration + optimistic apply, revert to the last Rust-confirmed VM, monotonic write token).
- `src/components/settings/recording-settings-controls.tsx` *(new)* + `.test.tsx` *(new)* — the shared control.
- `src/components/settings/settings-dialog.tsx` — Recording section mounts the control + hydrates on open.
- `src/components/layout/recording-pane.tsx` (+ `.test.tsx`) — the "Segmenting" card mounts the control.

**Review findings breakdown.** 3 patches applied (1 medium: shared-store revert-target + control co-field read-modify-write hazard; 2 low: `parseInt` partial-numeric truncation → `Number()`+round+guards, and a `recording-pane` regression test for the brittle `Segmenting` string match); 1 deferred (clamp-not-reject contract is false for out-of-type IPC input → `deferred-work.md`); 8 rejected (faithful-to-contract, by-design, cosmetic, or unreachable). No `intent_gap`, no `bad_spec` — the captured intent was complete, so no re-derivation loopback.

**Follow-up review recommendation:** false — the review pass made only localized, low-to-medium-consequence robustness fixes to the frontend store/control, fully covered by the existing suite plus one added test; no behavior/API/security/data surface of the shipped feature changed.

**Verification performed.**
- `bun run check` → biome clean, tsc clean, vitest **1296/1296** passed (121 files; +1 new `recording-pane` test), core-tauri-free check clean — run 2026-07-17 after the review patches.
- `bun run check:rust` (fmt + clippy `-D warnings`) and `bun run test:rust` (cargo-nextest **868/868**, incl. the new registry clamp/default + `start_recording_request` wire tests) — green at implementation; Rust was untouched by the review pass.

**Residual risks.** (1) The `recording_settings_set` "clamp, not reject" contract holds only for in-`u16`/`u32` input; an out-of-type value from a future non-UI caller hard-fails serde rather than clamping (deferred — the UI clamps first, so no user impact today). (2) The default thresholds and clamp bounds are authored assumptions per the epic, adjustable on dogfooding evidence without a spec change. (3) A live-capture smoke that the configured `segmentMB`/`maxSegmentSeconds` actually reach the sidecar requires signed hardware (tracked with the 12.6/15.6 physical-device work); the wire assembly is unit-tested.
