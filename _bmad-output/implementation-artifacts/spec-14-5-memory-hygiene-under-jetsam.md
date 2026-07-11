---
title: 'Memory Hygiene Under Jetsam'
type: 'feature'
created: '2026-07-11'
status: 'done'
review_loop_iteration: 1
followup_review_recommended: false
context: []
warnings: ['oversized']
baseline_revision: 'cdcf3620b4300f75bb8f45309436eb93d1d10d63'
final_revision: '09f247053461baa444c3c2e2f76c895218d8eee1'
---

<intent-contract>

## Intent

**Problem:** When keeper is backgrounded on iOS it keeps holding droppable weight — decoded image bitmaps live in the WKWebView renderer, and any in-flight `keeper-media://` fetch keeps its byte buffer — so a suspended session is fatter than it needs to be and more likely to be jetsam-killed before the user returns. Nothing sheds that weight on the background transition today: the single lifecycle entry (14.1) pauses sync but frees no media memory, and there is no drop hook wired to the background signal.

**Approach:** On the background lifecycle transition, shed keeper's droppable media memory from the **frontend** (the only place that owns the WKWebView's decoded-image memory): a single reduced-tier lifecycle store — fed by the *existing* sole `visibilitychange` listener so there is no second lifecycle truth — flips a `shed` flag; media renderers drop their `src` while shed (releasing decoded bitmaps and aborting in-flight `keeper-media://` fetches, which frees their request-scoped byte buffers) and restore it on foreground. The **Rust** media byte-buffer posture stays the Story 12.4 Range cap — regression-tested here to hold under sustained seeking — with disk-backed streaming staying a deferred-work entry. The 24 h suspended soak and the Instruments memory-baseline check are the non-automated bars, ledgered as SM-8 dogfooding.

## Boundaries & Constraints

**Always:** Derive the shed strictly from the single lifecycle signal already owned by `use-app-lifecycle` — one visibility listener, one lifecycle truth (AD-30). Reduced-capability (iOS) tier only, gated on `useIsReducedCapabilityPlatform`; desktop must stay byte-identical (the store never leaves `foreground`, so `shed` is never true). All drop/restore work is best-effort and silent (no toast). Preserve the Story 12.4 Range cap unchanged. Rust: no `.unwrap()`/bare `.expect()` in prod paths, `?` + `thiserror`. TS: no `any`, `import type` for types.

**Block If:** Investigation shows dropping `<img>`/`<audio>` `src` does **not** release the WKWebView's decoded-image memory and the only effective shed requires a native Swift `didReceiveMemoryWarning` plugin — HALT (breaking the zero-native posture is a phase-level decision, as 14.1 deferred the plugin). Or if the shed cannot be made safe against the Story 14.4 resume-integrity restoration (media fails to re-hydrate on foreground / data loss) — HALT.

**Never:** Do not build a native memory-warning / `UIApplication` Swift plugin this story (deferred, AD-30 upgrade path). Do not introduce a JS blob/object-URL media cache to "have something to clear" — that violates the keeper-media:// / no-large-payload-over-IPC invariant. Do not drop core view-model state (room-list, timeline, nav mirrors) — only media/image memory; resume data comes back via snapshot-then-diff (AD-8) and the 14.4 nav restore. Do not regress desktop. Do not implement disk-backed streaming (stays deferred, DW-29). Do not turn the 24 h soak or Instruments check into a story-blocking automated gate.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Background (reduced tier) | `visibilitychange` → `hidden` | Store phase → `background`; media renderers drop `src` (release decoded images); in-flight `keeper-media://` fetches abort, freeing their byte buffers | Best-effort; swallow |
| Foreground (reduced tier) | `visibilitychange` → `visible` | Store phase → `foreground`; media `src` restored, reloads from cache; no data loss | Best-effort; swallow |
| Desktop / pre-hydration | any visibility | No listener attached; phase stays `foreground`; media `src` never dropped — byte-identical to today | No error expected |
| Sustained large-media seeking | repeated Range requests across offsets | Each clamped slice ≤ `MAX_MEDIA_RANGE_CHUNK` (8 MiB on iOS); no persistent buffer growth (request-scoped) | 12.4 cap holds |
| iOS memory warning | `didReceiveMemoryWarning` | No web signal reaches the webview today; the same shed path rides the deferred native seam — out of scope this story | N/A (deferred) |

</intent-contract>

## Code Map

- `src/hooks/use-app-lifecycle.ts` -- sole `visibilitychange` listener + single Rust lifecycle entry caller (14.1); extend its `dispatch` to also feed the lifecycle store.
- `src/lib/stores/capabilities.ts` -- `useIsReducedCapabilityPlatform` predicate (the tier gate); zustand store conventions live in `src/lib/stores/`.
- `src/components/chat/media-attachment.tsx` -- timeline media renderer; `<img src={thumbSrc ?? fullSrc}>`, video poster `<img>`, `<audio src={fullSrc}>` — the primary decoded-image holders to shed.
- `src/components/chat/media-preview-overlay.tsx` -- full-screen Quick-Look overlay; full-res image/video is the heaviest single decoded holder.
- `src-tauri/crates/keeper/src/media_protocol.rs` -- `MAX_MEDIA_RANGE_CHUNK` (8 MiB iOS / `u64::MAX` desktop) + `capped_range_end` (12.4 cap); host tests here.
- `_bmad-output/implementation-artifacts/deferred-work.md` -- DW-29 (disk-backed streaming still deferred); append the 14.5 SM-8 dogfooding bars.

## Tasks & Acceptance

**Execution:**
- [x] `src/lib/stores/lifecycle.ts` -- NEW zustand `useLifecycleStore` holding `phase: "foreground" | "background"` (default `"foreground"`) with a `setPhase` action, plus a `useMediaShed()` selector returning `phase === "background"` -- the single frontend lifecycle truth the shed derives from; default keeps desktop `shed` permanently false. Do not over-claim in the doc that this is "forward-compatible" for a future memory-warning-while-foregrounded state — the `phase === "background"` derivation cannot express "shed while visible"; note plainly that a future memory-warning path would extend this store, not that it is already covered.
- [x] `src/hooks/use-app-lifecycle.ts` -- in `dispatch`, when the mapped `phase` is non-null, call `useLifecycleStore.getState().setPhase(phase)` alongside the existing IPC call -- feed the shed from the one listener; leave the reduced-tier gate, mount-time dispatch, and IPC best-effort behavior unchanged.
- [x] `src/components/chat/media-attachment.tsx` -- gate ONLY the `kind === "image"` `<img src>` on `useMediaShed()`: when shed, drop that image `src` (render the existing skeleton) and reset its `loaded` so the skeleton covers the re-load on restore. **Do NOT shed the inline `<audio>` element, the `<video>` poster, or any playback element** — audio/video `src` must stay put: dropping them resets playback position, forces a large re-download, and (with `autoPlay`) restarts from 0 on foreground, and the poster `<img>` drop makes a postered video morph to its placeholder across a background round-trip. Image thumbnails are the bulk of decoded-bitmap memory and shed cleanly. Implement the `loaded` reset without a dead lint-suppression statement (no `void shed;`): read `shed` genuinely (e.g. key the `<img>` on it, or fold shed into the skeleton condition) so no no-op is needed.
- [x] `src/components/chat/media-preview-overlay.tsx` -- gate the full-res `src` on `useMediaShed()` ONLY when `media.kind === "image"` (the single heaviest decoded holder). Leave video/audio preview `src` untouched (same playback-reset / large-re-download reason as above). A brief re-load of the full-res image on resume of an already-open overlay is the accepted cost; do not claim in comments that the bytes are guaranteed cache-served (the URL is restored, not proven cached).
- [x] `src-tauri/crates/keeper/src/media_protocol.rs` -- add a host regression test that asserts the **actually-materialized** slice length is bounded by the iOS cap: for a sweep of seek start offsets over a large synthetic byte buffer, slice `bytes.get(start..=capped_range_end(start, last, IOS_CAP))` (the real 206 path shape) and assert `slice.len() <= IOS_CAP` per request. Use saturating arithmetic for any offset math (defend against `end < start`). Name it for what it verifies — a per-request cap on the materialized buffer (buffers are request-scoped, so there is no cross-seek accumulation to model) — NOT "no cumulative growth". Extend the module doc for the 14.5 posture. Do not change the cap. (This must add value beyond the existing `capped_range_end_clamps_saturates_and_noops` pure-arithmetic test — it exercises the real slice materialization.)
- [x] `src/lib/stores/lifecycle.test.ts` (NEW) + `src/hooks/use-app-lifecycle.test.ts` (extend) + `src/components/chat/media-attachment.test.tsx` + `media-preview-overlay.test.tsx` (extend) -- unit-test the I/O matrix: store transitions, that a background transition drops the **image** `src` (and foreground restores it), that **audio/video `src` is NOT dropped** across a shed cycle (regression guard for finding #1), and that the desktop tier never sheds. Assert the drop hook *fires* (the AC-1 automated bar). Every suite that mutates the shared `lifecycleStore` singleton must reset it to `"foreground"` in `afterEach` (the store is a module-load singleton — leave it clean to avoid order-dependent cross-contamination).
- [x] `_bmad-output/implementation-artifacts/deferred-work.md` -- append the 14.5 SM-8 non-automated bars: (a) 24 h suspended-soak with a large account survives without a jetsam kill, (b) Instruments-on-Simulator confirmation that memory returns near baseline after backgrounding; cross-reference DW-29 for disk-backed streaming (do not duplicate it).

**Acceptance Criteria:**
- Given the reduced (iOS) tier, when the app backgrounds, then the drop hook fires — every **image** `src` is released and the lifecycle store reads `background` — while any playing `<audio>`/`<video>` keeps its `src` (no playback reset); on foreground the images re-hydrate (AC-1 automated portion; memory-near-baseline is the Instruments/SM-8 bar).
- Given the desktop tier (or before capabilities hydrate), when visibility changes, then no shed occurs and media rendering is byte-identical to today.
- Given large-media playback with sustained seeking, when successive Range requests stream, then every slice stays within the 12.4 cap with no unbounded growth, and disk-backed streaming remains a deferred-work entry (AC-2).
- Given the 24 h suspended soak with a large account, then it is recorded as an SM-8 dogfooding item (not a story-blocking device step), survival without a jetsam kill being the bar (AC-3).

## Spec Change Log

### 2026-07-11 — bad_spec repair (review pass 1)
- **Triggering findings:** (1) [medium] The shed dropped the `src` of *all* media including inline `<audio>`, `<video>`, and video poster `<img>` — resetting playback position to 0, forcing a large re-download, restarting `autoPlay` from the start on foreground, and morphing a postered video to its placeholder across a background round-trip. (2) [low] The Rust `sustained_seek…no_cumulative_growth` test asserted only pure per-slice arithmetic on `capped_range_end`, modelled no accumulation, hardcoded the cap instead of exercising the real materialization path, and duplicated the existing `capped_range_end_clamps_saturates_and_noops` test under a name that overstated coverage.
- **Amended:** Scoped the shed to **image holders only** (inline image thumbnails + the full-res preview image); audio/video/poster playback surfaces are explicitly exempt (Tasks + new "Images only — audio/video playback is exempt" Design Note + AC-1). Reworked the Rust test task to slice a real synthetic buffer and assert the **materialized** slice length ≤ cap with saturating arithmetic, renamed off the misleading "no cumulative growth", required it to add value beyond the pure-arithmetic test. Folded in patch-level guidance: no `void shed;` lint no-op, correct the "reloads from cache"/"no data loss"/audio "aborts fetch" comments, soften the store's forward-compat doc claim, and require every test suite to reset the `lifecycleStore` singleton in `afterEach`.
- **Known-bad state avoided:** A memory-hygiene story that visibly *harms* playback (voice notes and videos jumping to 0:00 and re-downloading on every background round-trip) to shed memory that audio/video barely hold; and a regression test whose name promises accumulation coverage it does not provide.
- **KEEP (must survive re-derivation):** the single-lifecycle-truth design (one `visibilitychange` listener in `use-app-lifecycle` feeding a vanilla-zustand `lifecycleStore`; no second listener); reduced-tier gating via `useIsReducedCapabilityPlatform` with desktop byte-identical by the default `"foreground"`; the `useMediaShed()` selector shape; feeding `setPhase` alongside the existing best-effort IPC; the 12.4 cap left unchanged; the two SM-8 deferred-work entries (24 h soak + Instruments near-baseline); and the desktop-invariance tests.

## Review Triage Log

### 2026-07-11 — Review pass
- intent_gap: 0
- bad_spec: 2: (medium 1, low 1)
- patch: 4: (low 4)
- defer: 0
- reject: 4: (low 4)
- addressed_findings:
  - `[medium]` `[bad_spec]` Shed dropped audio/video/poster `src` mid-playback (reset position, re-download, `autoPlay` restart, poster morph) — scoped the shed to image holders only; audio/video/poster exempt; re-derivation loopback.
  - `[low]` `[bad_spec]` Rust sustained-seek test asserted only pure `capped_range_end` arithmetic under an overstated "no cumulative growth" name, duplicating existing coverage — reworked to assert the materialized slice length ≤ cap over a real buffer with saturating math and an honest name; re-derivation loopback.
  - `[low]` `[patch]` (folded into the loopback) `void shed;` lint no-op + misleading comment; misleading "reloads from cache"/"no data loss"/audio "aborts fetch" comments; `lifecycleStore` singleton had no enforced test reset (cross-contamination risk); overlay/poster resume flicker — encoded as spec guidance so re-derivation avoids them.

### 2026-07-11 — Review pass (post-loopback re-review)
- intent_gap: 0
- bad_spec: 0
- patch: 3: (low 3)
- defer: 1: (low 1)
- reject: 15: (low 15)
- addressed_findings:
  - `[low]` `[patch]` Foreground restore un-hid the image before the remounted `<img>` re-decoded (`imageLoaded = loaded && !shed` with stale `loaded=true`) → brief blank at full opacity — reset `loaded` when entering shed so the skeleton covers the re-load in both directions; corrected the comment.
  - `[low]` `[patch]` Rust test comment overclaimed it exercised "the REAL 206 path (not just arithmetic)" though it mirrors the slice inline rather than calling `partial_or_full` — rewrote the comment to state honestly what it covers (clamp helper + materialized-slice bound) and that handler wiring is covered by the existing arithmetic test + SM-8.
  - `[low]` `[patch]` `media-attachment` comment + Design Note claimed dropping `src` "aborts the in-flight fetch"; the Rust handler spawns a detached task, so the buffer frees request-scoped (not an instant abort) — softened both.
  - `[low]` `[defer]` Exempted `<audio>`/`<video>` playback means a backgrounded open preview overlay with an autoplaying video leaves its decoded buffer un-shed — ledgered as an SM-8 residual to quantify (deferred-work.md).

## Design Notes

**Why the shed is frontend, not Rust.** The "image memory cache" the AC names is the WKWebView renderer's decoded-image memory — Rust cannot free it. Rust's media byte buffer (`media_protocol.rs`) is already request-scoped and 12.4-capped; there is no persistent Rust in-memory media cache to drop, and inventing an SDK cache-drop call would be fantasy. So the effective, honest drop hook lives where the memory does: the frontend drops the **image** `src`. Dropping an image `src` while `hidden` is invisible to the user; it prompts WebKit to release the decoded bitmap, and the request-scoped `keeper-media://` byte buffer frees when its (possibly-detached) fetch task completes — scheme-task invalidation is tolerated per 12.4, so the freeing is request-scoped rather than an instant abort.

**Images only — audio/video playback is exempt.** The shed targets the decoded-**image** bitmaps (inline image thumbnails + the full-res preview image), which are the bulk of the droppable memory and re-load cheaply behind the existing skeleton. It must NOT drop the `src` of an inline `<audio>`, a `<video>`, or a video poster `<img>`: dropping a playback element resets `currentTime` to 0, forces a full re-download (large for video), and with `autoPlay` restarts from the beginning on foreground — a real UX regression for the negligible memory a `preload="metadata"` audio or a single paused video frame holds. Shedding the poster `<img>` would also flip `hasPoster` and morph a postered video to its placeholder across a background round-trip. So the memory win comes entirely from images; playback surfaces are left untouched.

**One lifecycle truth.** The shed must not add a second `visibilitychange` listener. `use-app-lifecycle` already owns the sole listener and the mount-time seed; it just also writes the store. A future native `didReceiveMemoryWarning` plugin (deferred, AD-30) would *extend* this same store to express a "shed while foregrounded" state — the current `phase === "background"` derivation does not cover that case, so the store is a clean seam for that path, not already-complete coverage of it.

**Safe against resume integrity (14.4).** Only media `src` is shed — never room-list/timeline/nav mirrors. On foreground the src restores and reloads from the browser/Rust cache; message data returns via snapshot-then-diff re-subscribe (AD-8) and nav via the 14.4 Rust-held restore. No overlap with the 14.4 `paused_at`/nav state.

## Verification

**Commands:**
- `bun run check` -- expected: biome + tsc + vitest green (new store + media shed tests pass).
- `bun run check:rust` -- expected: rustfmt clean + clippy `-D warnings` clean.
- `bun run test:rust` -- expected: `media_protocol` sustained-seek cap test passes.

**Manual checks (SM-8 / not story-blocking):**
- Instruments on the iOS Simulator: background the app with media loaded; memory returns near baseline. 24 h suspended soak with a large account survives without a jetsam kill. Both ledgered as SM-8 dogfooding, findings recorded.

## Auto Run Result

Status: done

**Summary:** Story 14.5 sheds keeper's droppable **decoded-image** memory when the app backgrounds on the reduced (iOS) tier. A new vanilla-zustand `lifecycleStore` — fed by the *existing* sole `visibilitychange` listener in `use-app-lifecycle` (one lifecycle truth, AD-30) — flips a `useMediaShed()` flag; image renderers drop their `<img src>` while backgrounded (releasing the decoded bitmap; the request-scoped `keeper-media://` byte buffer frees when its fetch task completes) and restore it on foreground behind the existing skeleton. Audio/video/poster playback surfaces are deliberately exempt (dropping their `src` would reset playback and force re-downloads). The Story 12.4 Range cap is unchanged and gets a regression test that materialises a real per-request slice and asserts it stays within the cap. Desktop is byte-identical (the store never leaves `foreground` off the reduced tier). The 24 h jetsam soak and the Instruments memory-baseline check are ledgered as SM-8 dogfooding, not automated gates.

**Files changed:**
- `src/lib/stores/lifecycle.ts` (new) — `lifecycleStore` (`phase` default `foreground`) + `setPhase`, `useLifecycleStore` selector, `useMediaShed()`.
- `src/lib/stores/lifecycle.test.ts` (new) — store default, transitions, `useMediaShed` semantics.
- `src/hooks/use-app-lifecycle.ts` — `dispatch` also writes `setPhase(phase)` from the one existing listener; gate/mount-seed/IPC behavior unchanged.
- `src/components/chat/media-attachment.tsx` — sheds only `kind === "image"` `<img src>`; resets `loaded` on shed so the skeleton covers the re-load in both directions; audio/video/poster untouched.
- `src/components/chat/media-preview-overlay.tsx` — sheds the full-res `src` only when `kind === "image"`; video/audio preview `src` untouched.
- `src-tauri/crates/keeper/src/media_protocol.rs` — `capped_slice_len_bounded_per_request_across_seek_sweep` test (materialised slice ≤ cap over a real buffer, saturating math); 14.5 module-doc note. Cap unchanged.
- `src/hooks/use-app-lifecycle.test.ts`, `src/components/chat/media-attachment.test.tsx`, `src/components/chat/media-preview-overlay.test.tsx` — shed fires on background / restores on foreground; audio & video `src` NOT dropped; desktop never sheds; each suite resets the store in `afterEach`.
- `_bmad-output/implementation-artifacts/deferred-work.md` — three SM-8 residuals: Instruments near-baseline, 24 h jetsam soak, and the exempted-playback (backgrounded autoplaying overlay video) buffer.

**Review findings breakdown:**
- Pass 1: 2 bad_spec (1 medium: shed reset audio/video playback → scoped to images-only via re-derivation loopback; 1 low: weak Rust test → reworked), patches folded into the loopback, 4 rejected.
- Pass 2 (post-loopback): 0 intent_gap, 0 bad_spec; 3 low patches applied (restore-flash `loaded` reset; two honesty-comment corrections), 1 low defer (exempted-playback residual), ~15 rejected (by-design/spec-acknowledged/pedantic).
- Follow-up review recommended: **false** — the final pass made only a few localized, low-consequence fixes.

**Verification:**
- `bun run check` — PASS (biome clean, tsc clean, 1235 vitest tests / 116 files).
- `bun run check:rust` — PASS (`cargo fmt --check` clean, `clippy --all-targets -D warnings` clean).
- `bun run test:rust` — PASS (784 nextest tests, incl. the new per-request cap test).

**Residual risks:**
- The shed's actual memory reclamation (does WKWebView free decoded-image memory when `src` clears?) and the 24 h jetsam soak are on-device facts only — jsdom/host builds cannot verify them; carried as SM-8 dogfooding items (spec Block If: if Instruments shows no release, the only effective shed needs the deferred native `didReceiveMemoryWarning` plugin).
- A backgrounded open preview overlay with an autoplaying video keeps its decoded buffer (playback-continuity exemption) — ledgered for SM-8 quantification.
- No shed on a foreground iOS memory warning (no web signal reaches the webview today) — rides the same deferred native seam.
