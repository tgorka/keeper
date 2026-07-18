---
title: 'Story 18.3: In-App Active-Recording Banner & Segment Meter'
type: 'feature'
created: '2026-07-17'
status: 'done'
baseline_revision: 'b2efb9ac2dade6d934da4044d777feba2007b05e'
final_revision: '5105481b7ab21b995a6f7cce46c66dac143decbd'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-18-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** The menu-bar tray (Stories 18.1/18.2) is the only in-product truth about a live screen recording. A user looking at the app itself — specifically the Recording view (⌘5) — has no persistent, pinned indicator of a running capture and no sense of how full the current segment is. The frontend `RecordingStatusVm` also carries no on-disk size and no session segment cap, so it cannot honestly render size without inventing state (forbidden by the architecture invariant).

**Approach:** Build the in-app twin of the tray: a persistent (never-a-toast) **active-recording banner** pinned to the top of the Recording view — recording-red left edge, reduced-motion-aware record dot, "Recording", a monospace `elapsed · segment · size` line, and a Stop button — plus a **segment meter** (progress bar filling toward the session's segment-size cap, captioned `segment N · 412 / 500 MB`, resetting at each gapless rotation). To keep the UI a pure renderer of Rust-owned state, enrich the shared `recording_snapshot` read path with three fields — `on_disk_bytes` (total session), `current_segment_bytes` (open segment), `segment_cap_mb` (session-captured cap) — consumed identically by the tray and the banner.

## Boundaries & Constraints

**Always:**
- The banner + meter render **only** from the enriched Rust-owned `RecordingStatusVm` snapshot plus the reused client-computed `formatElapsed` — never invent, estimate (e.g. elapsed × bitrate), or duplicate recording state in TypeScript. Size and cap come from the VM.
- The banner renders only when `isLiveRecording(status)` (preflight/recording/rotating/stopping); on any terminal/idle state it renders nothing.
- The meter denominator is the **session-captured** `segment_cap_mb` (from the VM), never the mutable settings store — mid-session cap edits ("applies to the next session") must not skew a running meter.
- Record dot honors `prefers-reduced-motion` (steady, never pulsing) via the existing `useReducedMotion` hook.
- Stop reuses the existing idempotent `recordingStop` graceful-stop path; disabled while `stopping`.
- Recording-red is used **only** for the record dot, the banner's left edge, and the meter fill; the Stop button is destructive-styled (never recording-red) and the two reds stay visually distinct.
- Accessibility: recording state is announced **assertively** on start/stop/segment-change via an `sr-only` live region; the per-second ticking elapsed is **not** in a live region (never announced once per second); `Esc` never stops — Stop is an explicit focusable button.
- `recording_snapshot` fills the three fields **best-effort** (0 on missing/unreadable folder or no session); the tray consumes `on_disk_bytes` from the snapshot, dropping its now-duplicate `session_bytes_on_disk` call — visible behavior unchanged.

**Block If:**
- The enriched snapshot cannot expose on-disk bytes + the session segment cap because a foundational 16.x/17.x assumption is false (session folder path, `screen-####.mp4` naming, or the start-time segment-size settings read is unavailable).

**Never:**
- Never add the warning/error banner variants, the loud-failure notification triad, or a "Restart recording" action — those are Story 18.4; this story covers only the live/recording state.
- Never add a Pause affordance (deferred this phase); never show it disabled.
- Never touch, hide, or duplicate macOS's own purple screen-recording pill.
- Never make elapsed announce per second; never let `Esc` stop a recording.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| current-segment bytes, none | folder holds no `screen-####.mp4` | `current_segment_bytes_on_disk` → 0 | 0 on missing/unreadable |
| current-segment bytes, one growing | `screen-0000.mp4` (123) | 123 | — |
| current-segment bytes, post-rotation | `screen-0000.mp4` (500 MB), `screen-0001.mp4` (40) | 40 (highest index) | — |
| current-segment bytes, foreign files | also `camera-0000.mp4`, `manifest.json` | only `screen-####.mp4` considered | ignore foreign |
| snapshot enrichment, idle | no session/run | on_disk=0, current=0, cap=0 | no session ⇒ zeros |
| formatSize whole MB | 431_800_000 | `"412 MB"` (decimal MB) | — |
| formatSize ≥ 1000 MB | 1_290_000_000 | `"1.2 GB"` | — |
| bytesToWholeMb | 412_000_000 | `412` | — |
| meter fill | current 250 MB, cap 500 | bar at 0.5, caption `segment N · 250 / 500 MB` | — |
| meter over cap | current 520 MB, cap 500 | bar clamps to 1.0, caption `520 / 500 MB` | no overflow |
| meter cap 0 (defensive) | `segmentCapMb === 0` while live | meter hidden (no NaN/∞ fraction) | guarded |
| banner hidden | state idle/finalized/recovered/failed | component renders `null` | — |
| banner live line | recording, 2 closed, elapsed 12:34, 412 MB total | `12:34 · segment 3 · 412 MB` | — |
| reduced motion | `prefers-reduced-motion: reduce` | dot steady (no `animate-pulse`) | — |
| Stop click | live, not stopping | calls `onStop`; label→"Stopping…" & disabled when `stopping` | — |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/vm.rs` -- `RecordingStatusVm` (~2724) + `RecordingStatusVm::idle()`. Add `on_disk_bytes: u64`, `current_segment_bytes: u64`, `segment_cap_mb: u32` (doc: read-time/session-captured, not driver-maintained; 0 when no session). Update all constructors. ts-rs (`#[derive(TS)]`, camelCase) regenerates the TS type.
- `src-tauri/crates/keeper-core/src/recording.rs` -- `session_bytes_on_disk` (987), `SEGMENT_STEM_PREFIX`/`segment_index_from_stem`. Add `current_segment_bytes_on_disk(folder: &Path) -> u64` (length of the highest-index `screen-####.mp4`; 0 if none/unreadable).
- `src-tauri/crates/keeper/src/ipc.rs` -- `RecordingRun` (add `segment_cap_mb: u32`), `recording_start` (~3414 reads `segment_mb`; store it into the run), `recording_snapshot` (3600 — fill the three fields from disk + run when a session exists), `recording_status` command (3716 unchanged wrapper).
- `src-tauri/crates/keeper/src/tray.rs` -- status-line byte source (486) + `use ...session_bytes_on_disk` (35). Read `snapshot.on_disk_bytes` instead of re-summing; drop the now-unused import (simplification; behavior identical).
- `src/hooks/use-recording-session.ts` -- `IDLE_RECORDING_STATUS` (32), reused `formatElapsed` (46), `isLiveRecording` (41). Add the three new fields to the idle literal.
- `src/lib/recording-format.ts` -- NEW: `formatSize(bytes)` (decimal MB whole; one-decimal GB at ≥1000 MB) + `bytesToWholeMb(bytes)` (rounded decimal MB) — mirrors Rust `format_size`.
- `src/components/recording/active-recording-banner.tsx` -- NEW presentational banner + segment meter. Props `{ status, elapsed, onStop }`; renders `null` when not live.
- `src/components/layout/recording-pane.tsx` -- mount `<ActiveRecordingBanner>` pinned (shrink-0) between `<header>` and `<ScrollArea>`; relocate the header's live dot/elapsed/Stop cluster (79–99) into the banner (keep idle Start + terminal notes).
- `src/hooks/use-reduced-motion.ts`, `src/components/ui/alert.tsx`, `src/components/ui/button.tsx` -- reuse for the record dot, banner chrome, and Stop button.

## Tasks & Acceptance

**Execution (dependency order):**
- [x] `src-tauri/crates/keeper-core/src/recording.rs` -- add `current_segment_bytes_on_disk(folder: &Path) -> u64` reusing the segment-file ownership rule; unit-test the four current-segment matrix rows.
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- add `on_disk_bytes`, `current_segment_bytes`, `segment_cap_mb` to `RecordingStatusVm` (documented, defaulting to 0 in `idle()` and every other constructor); a test asserting `idle()` yields zeros. Let ts-rs regenerate `src/lib/ipc/gen/RecordingStatusVm.ts`.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- add `segment_cap_mb: u32` to `RecordingRun`, set from the `segment_mb` already read at `recording_start`; in `recording_snapshot`, when a session exists fill `on_disk_bytes = session_bytes_on_disk(path)`, `current_segment_bytes = current_segment_bytes_on_disk(path)`, `segment_cap_mb = run.segment_cap_mb` (best-effort, 0 on missing folder). Optional unit test over the fill path.
- [x] `src-tauri/crates/keeper/src/tray.rs` -- consume `snapshot.on_disk_bytes` in the status-line size formatting; remove the direct `session_bytes_on_disk` call and its now-unused import.
- [x] `src/lib/recording-format.ts` -- add `formatSize` + `bytesToWholeMb`; colocated `recording-format.test.ts` covering the format matrix rows.
- [x] `src/hooks/use-recording-session.ts` -- extend `IDLE_RECORDING_STATUS` with `onDiskBytes: 0, currentSegmentBytes: 0, segmentCapMb: 0` (the `start()` failed-snapshot spread inherits them).
- [x] `src/components/recording/active-recording-banner.tsx` -- build the presentational banner: recording-red 3px left edge, reduced-motion-aware record dot, "Recording", monospace `elapsed · segment · size` line (`segment = segmentsClosed + 1`, `size = formatSize(onDiskBytes)`), destructive-styled Stop (reusing `onStop`, "Stopping…"+disabled on `stopping`), and the segment meter (fraction `clamp(currentSegmentBytes / (segmentCapMb·10^6), 0, 1)`, caption `segment N · {bytesToWholeMb(currentSegmentBytes)} / {segmentCapMb} MB`, hidden when `segmentCapMb === 0`); an `sr-only aria-live="assertive"` node announcing state/segment changes (not per-second); returns `null` when `!isLiveRecording(status)`.
- [x] `src/components/recording/active-recording-banner.test.tsx` -- cover the banner/meter matrix rows (visible-when-live, hidden-when-terminal, live line text, meter caption + fraction + over-cap clamp + cap-0 hidden, reduced-motion steady dot, Stop→onStop/stopping-disabled).
- [x] `src/components/layout/recording-pane.tsx` -- mount the pinned banner and relocate the header live cluster into it; update `recording-pane.test.tsx` and any `RecordingStatusVm` test fixtures/literals to include the three new fields.

**Acceptance Criteria:**
- Given the Recording view with a live session, when the banner renders, then it is pinned to the top of the view (persistent, not a toast) showing the record dot, "Recording", a `elapsed · segment · size` line that ticks each second, and a Stop button — and it disappears on any terminal/idle state.
- Given a live session, when the current segment grows toward the session's cap, then the segment meter fills proportionally with caption `segment N · used / cap MB` and resets toward empty at each gapless rotation; the denominator is the session-captured cap, unaffected by editing the segment-size setting mid-recording.
- Given the in-app banner and the menu-bar tray are both visible, then both render identical elapsed / segment / size figures because both read the same enriched `RecordingStatusVm` snapshot.
- Given `prefers-reduced-motion: reduce`, then the record dot is steady (never pulsing); given a screen reader, recording state is announced assertively on start/stop/segment-change while the ticking elapsed is never announced once per second, and `Esc` never stops the recording.
- Given the user clicks the banner's Stop, then the same idempotent graceful-stop path fires (the session finalizes and the banner clears) — identical to the tray's Stop and the pane's prior Stop.
- Given macOS's own purple recording pill, then keeper never touches or duplicates it — the banner only adds elapsed/segment/size/meter/Stop that the pill lacks.

## Design Notes

- **One enriched read path, two surfaces.** `recording_snapshot` is the single shared reader (tray tick, quit check, frontend poll). Filling the byte/cap fields there means the tray and the in-app banner render byte-identical figures and the tray drops its duplicate `session_bytes_on_disk` call — no second source of truth. The fields are read-time (bytes) / session-captured (cap), not maintained by the sidecar-driven state machine; document that so the zeros on the stored snapshot are understood.
- **Why the cap lives in the VM.** The meter denominator must be the *running* session's cap. The settings store is mutable and explicitly "applies to the next session", so reading the cap from the store could show a wrong denominator after a mid-session edit. Capturing `segment_mb` into `RecordingRun` at start and surfacing it as `segment_cap_mb` keeps the meter honest and store-independent.
- **Total vs current bytes.** `on_disk_bytes` (all `screen-####.mp4`) drives the banner's `size` line — mirroring the tray. `current_segment_bytes` (the highest-index open segment) drives the meter numerator, which naturally falls back toward ~0 at each gapless rotation as a fresh segment file starts.
- **Consolidation, not duplication.** The pane header already carries an ad-hoc live dot/elapsed/Stop cluster (16.5/16.6); relocating it into the banner avoids two live surfaces and two Stop buttons. Elapsed reuses the hook's existing `formatElapsed` (no new elapsed formatter).
- **Live regions.** Keep the ticking mono line out of any `aria-live` region; announce state via a dedicated `sr-only aria-live="assertive"` node keyed on `state` + `segmentsClosed` (so it fires on start/stop/rotation, never per second).

## Verification

**Commands:**
- `cd src-tauri && cargo test -p keeper-core recording:: vm::` -- expected: `current_segment_bytes_on_disk` matrix + `idle()`-zeros tests pass; ts-rs regenerates `RecordingStatusVm.ts`.
- `cd src-tauri && cargo test -p keeper ipc:: tray::` -- expected: snapshot-fill (if added) + existing tray tests pass.
- `cd src-tauri && cargo build -p keeper && cargo clippy -p keeper -p keeper-core --all-targets -- -D warnings` -- expected: builds, no warnings (unused `session_bytes_on_disk` import removed).
- `bun run check` -- expected: biome + tsc (regenerated `RecordingStatusVm.ts` with `onDiskBytes`/`currentSegmentBytes`/`segmentCapMb`) + vitest (banner + format tests) all green.

**Manual checks (if no CLI):**
- Banner appearance/pinning, live meter fill and rotation reset, reduced-motion steadiness, and VoiceOver announcement cadence are GUI behaviors validated on real hardware at a later acceptance milestone (SM-10 / Epic 20); automated coverage here is the presentational component + formatter tests and the Rust byte helper.

## Review Triage Log

### 2026-07-17 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 5: (high 0, medium 0, low 5)
- defer: 0
- reject: 17: (high 0, medium 0, low 17)
- addressed_findings:
  - `[low]` `[patch]` The meter caption could read `1000 / 1000 MB` (full) while its bar sat at 99.9% — `bytesToWholeMb` **rounded** while `formatSize`/the tray size line **truncate**. Switched `bytesToWholeMb` to truncate, aligning the whole `recording-format` module on the documented "never overstate disk" convention (test + module doc updated).
  - `[low]` `[patch]` The segment progressbar's `aria-valuenow` could exceed `aria-valuemax` when the open segment momentarily overshot the cap (a screen reader would announce ">100%"). Clamped `aria-valuenow` to the cap and added `aria-valuetext` (`used / cap MB`) so assistive tech gets the same unit'd figure the visible caption shows.
  - `[low]` `[patch]` `recording_snapshot` held the `recording_run` slot lock across the best-effort `read_dir`/`stat` disk I/O — now on the 1 Hz frontend poll, the tray tick, and the quit guard. Restructured to clone the snapshot + capture the cap under the lock, **release it**, then do the disk I/O, so a slow/unreadable volume can't stall `start`/`stop`/quit.
  - `[low]` `[patch]` Added a banner test for the fresh-rotation state (current segment 0 bytes, cap > 0) asserting a clean `0%` bar + `0 / cap` caption — the meter's most frequent real runtime state, previously untested.
- notes: All 17 rejects are low/non-intolerable. Rejected — negative/NaN/Infinity into `formatSize` (impossible from the `u64` VM field); duplicate/oversized foreign `screen-####` indices, symlink-as-segment, and highest-index tie-break non-determinism (intended byte-for-byte parity with `session_bytes_on_disk`/manifest reconcile, established in the 18.1 review — the recorder writes zero-padded unique names); best-effort "0 MB" masking an unreadable folder mid-record (fault surfacing is Story 18.4's loud-failure triad, explicitly out of scope); assertive live region on rotation (spec-mandated — recording state announces assertively as a loss-risk event); preflight announcement (preflight is part of the live surface by design, sub-second); `Esc`-inert holds by construction (no key handler added); the header→banner relocation dropping the old `role="status"` landmark (intended relocation; the banner is strictly more informative; pane test updated); the two-`read_dir` sweep + non-atomicity (small local folders at 1 Hz, self-healing; matches 18.1's accepted per-tick re-stat); cross-module tray/enrichment coupling and three idle-literal copies (no current path violates the invariant; `tsc` enforces field presence; the new Rust test asserts idle zeros); `+ 1` vs `saturating_add(1)` for the segment index (unreachable overflow); ownership-predicate duplication (behaviorally identical today; refactor is out of scope). The read-only `<intent-contract>` I/O-matrix example `431_800_000 → "412 MB"` is a copy typo (431.8 MB truncates to `431 MB`); the binding requirement ("mirror Rust `format_size`") is unambiguous and correctly implemented — parity verified byte-for-byte — so it is not an intent gap.

## Auto Run Result

Status: done

**Summary:** Adds the in-app twin of the menu-bar tray (Stories 18.1/18.2): a persistent **active-recording banner** pinned to the top of the Recording view — recording-red left edge, reduced-motion-aware record dot, "Recording", a monospace `elapsed · segment · size` line, and a Stop button — plus a **segment meter** (a bar filling toward the session's segment-size cap, captioned `segment N · used / cap MB`, resetting at each gapless rotation). To keep the UI a pure renderer of Rust-owned state (no invented/estimated size in TS), the shared `recording_snapshot` read path is enriched with three fields — `on_disk_bytes` (total session), `current_segment_bytes` (open segment), `segment_cap_mb` (session-captured cap) — consumed identically by the tray and the banner, so the two surfaces render byte-identical figures. The tray drops its now-duplicate on-disk read; the Recording pane's ad-hoc header live cluster is relocated into the banner (no two Stop buttons).

**Files changed:**
- `src-tauri/crates/keeper-core/src/recording.rs` — new `current_segment_bytes_on_disk` (highest-index open segment; same ownership rule as `session_bytes_on_disk`) + 4 matrix tests.
- `src-tauri/crates/keeper-core/src/vm.rs` — `RecordingStatusVm` gains `on_disk_bytes`/`current_segment_bytes`/`segment_cap_mb` (documented read-time/session-captured; `#[ts(type="number")]` on the `u64`s); `idle()` zeroes them + a test.
- `src-tauri/crates/keeper/src/ipc.rs` — `RecordingRun.segment_cap_mb` captured at `recording_start`; `recording_snapshot` enriches the snapshot best-effort after releasing the slot lock (review patch).
- `src-tauri/crates/keeper/src/tray.rs` — status line reads `snapshot.on_disk_bytes`; dropped the duplicate `session_bytes_on_disk` call + unused imports.
- `src/lib/ipc/gen/RecordingStatusVm.ts` — ts-rs-regenerated with the three new `number` fields.
- `src/hooks/use-recording-session.ts` — three new fields added to `IDLE_RECORDING_STATUS`.
- `src/lib/recording-format.ts` (+ test) — `formatSize` (mirrors Rust `format_size` byte-for-byte) + `bytesToWholeMb` (truncating, review patch).
- `src/components/recording/active-recording-banner.tsx` (+ test) — new presentational banner + segment meter (reduced-motion dot, `sr-only` assertive announcer, clamped `aria-valuenow` + `aria-valuetext`, review patches).
- `src/components/layout/recording-pane.tsx` (+ test) — mounts the pinned banner; relocates the header live cluster.

**Review findings breakdown:** 5 patches applied (all low: meter/size truncation consistency, progressbar ARIA clamp + valuetext, Rust lock-across-I/O release, fresh-rotation meter test); 0 intent gaps; 0 bad-spec loopbacks; 0 deferred; 17 rejected (non-manifesting `u64` inputs, intended parity with `session_bytes_on_disk`/manifest, out-of-scope 18.4 fault surfacing, spec-conformant a11y, and verified-safe coupling/duplication notes). One spec I/O-matrix example value is a cosmetic copy typo inside the read-only intent-contract; the binding "mirror Rust `format_size`" requirement is correctly implemented (parity verified).

**Verification:** `cargo test -p keeper-core recording::` → 54 passed; `vm::` → 184 passed; `cargo test -p keeper ipc::` → 48 passed; `tray::` → 5 passed; `cargo build -p keeper` → success; `cargo clippy -p keeper -p keeper-core --all-targets -- -D warnings` → clean; `bun run check` → biome clean, tsc clean, **1316 vitest passed / 123 files**, zero-egress core check passed. Gates re-run green after the review patches (both frontend and Rust).

**Residual risks:** GUI/VoiceOver behaviors (banner pinning/appearance, live meter fill + rotation reset, reduced-motion steadiness, announcement cadence) are not automatable here — deferred to real-hardware acceptance (SM-10 / Epic 20); automated coverage is the presentational component + formatter tests and the Rust byte helpers. `recording_snapshot` now does a best-effort directory scan on each 1 Hz poll/tick (small local session folders; accepted as in 18.1). Mid-record fault surfacing (a suddenly-unreadable folder reading as "0 MB") is intentionally deferred to Story 18.4's loud-failure triad. `oversized` spec warning retained (cross-layer, multi-file story).
