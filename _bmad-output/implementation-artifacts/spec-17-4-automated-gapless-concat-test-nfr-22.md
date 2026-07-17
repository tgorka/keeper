---
title: 'Automated Gapless-Concat Test (NFR-22)'
type: 'feature'
created: '2026-07-17'
status: done
baseline_revision: 'ffc5889485ac0525b8bae6039a352eb006e39c78'
final_revision: '33765511d1410e891acdc2f71505e4aeb78deb39'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-17-context.md'
  - '{project-root}/docs/project-context.md'
warnings: [oversized]
---

<intent-contract>

## Intent

**Problem:** Story 17.1 rotates a recording into multiple `screen-####.mp4` segments with host-clock-anchored, keyframe-aligned handover, and 17.2 records them in a session `manifest.json`. Nothing yet *proves* the handover is gapless: an A/V-sync regression across a rotation cut (a dropped frame, a rewound clock, a keyframe misalignment) would ship silently and only surface as a hiccup in the user's playback. There is no automated gate.

**Approach:** Add a Swift concat-assert harness to the `keeper-rec` test target that, given a session's ordered segments, reads each segment's video-frame presentation timestamps and asserts the concatenated timeline is strictly monotonic with no boundary gap or overlap exceeding one frame duration (NFR-22). It runs in the existing `recording` CI job with **no capture hardware and no code-signing** — fixtures are generated on the runner via `AVAssetWriter` (AD-38) — and is recorded as a required release gate. It leaves a wired-but-skipped screen↔camera alignment hook for Epic 20.

## Boundaries & Constraints

**Always:**
- The gate reads **video-track sample PTS** from each segment via `AVAssetReader` over an `AVURLAsset`, concatenates them in segment order, and asserts (a) **strictly monotonically increasing** PTS across the whole session and (b) at every segment boundary the timeline discontinuity `δ − P` stays within one frame period in magnitude (`|δ − P| ≤ P`, where `δ` = firstPTS(seg k+1) − lastPTS(seg k) and `P` = one frame period) — i.e. no gap or overlap exceeding one frame. `P` derives from the capture frame rate (30 fps default → `P = 1/30 s`) and is a parameter so a future 60 fps run tightens the bound.
- Segments are discovered from a session's `manifest.json` (`segments[]`, path `<folder>/<file>`, ordered by `index`) so the harness runs **identically** on committed fixtures and on real signed-runner output.
- Fixtures are **generated on the runner** by `AVAssetWriter` (H.264 video, `.mpeg4CMAFCompliant` fragmented MP4, `screen-####.mp4` naming, 30 fps, host-clock-anchored PTS where segment k+1 continues exactly one frame after segment k's last frame) — **no ScreenCaptureKit**, so no display, no capture hardware, no signing (AD-38). No binary media is committed (no git-LFS in the tree).
- The suite includes **negative controls** — a deliberately gapped session and an overlapping session — that make the assertion fail; a green run therefore proves the check has teeth, not that it is vacuous.
- The gate runs in the existing `recording` CI job through `swift test` (`scripts/test-keeper-rec.sh`), is documented as an NFR-22 enforcement point in `docs/performance.md` (extending AD-21's measurement-hook discipline), and is added to `docs/release.md`'s required status checks so it is an actual release gate.
- `keeper-rec` stays first-party Apache-2.0 linking only Apple system frameworks; **no ffmpeg, no third-party SPM/test dependency** (AD-38).

**Block If:**
- Making the assertion pass against faithful fixtures would require changing 17.1's shipped capture/rotation Swift or bumping the `keeper-rec` `PROTOCOL_VERSION` (i.e. the real handover is not actually gapless) — HALT `blocked` (that reopens 17.1's locked contract; this story is a test/CI/docs gate only, adds no production capture code).

**Never:**
- No change to `keeper-rec` capture/rotation source (`Capture.swift`, `Rotation.swift`, `main.swift`) or to `keeper-core` — this story adds test-target code, CI wiring, and docs only.
- No real ScreenCaptureKit capture, signing, or physical display in the test path — the gate must run unattended in CI.
- No committed binary MP4 fixtures / git-LFS.
- No **population** of the screen↔camera alignment assertion with real camera data — 17.4 only leaves the wired, skipped hook and the assertion function; Story 20.1 supplies `camera-####` fixtures and enables it.
- No settings/`keeper.db` work (17.5); no startup/crash recovery (17.3).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Gapless session | ≥3 `screen-####.mp4` fMP4 segments, PTS continuous across cuts (seg k+1 first frame = seg k last + `P`) | `assertGaplessConcat` passes: concat PTS strictly monotonic; every boundary `|δ − P| ≤ P` | No error expected |
| Gap at a boundary | seg k+1 first frame lands `> 2·P` after seg k's last frame | Fails, naming boundary `k→k+1` and the measured gap (`δ − P > P`) | Test fails loudly (negative control) |
| Overlap at a boundary | seg k+1 first PTS ≤ seg k last PTS (rewound/duplicated time) | Fails on non-monotonicity, naming boundary `k→k+1` and the overlap | Test fails loudly (negative control) |
| Single-segment session | one segment only | Passes — no boundary to check, intra-segment PTS still asserted strictly monotonic | No error expected |
| Manifest-driven discovery | folder whose `manifest.json` lists segments not in filesystem order | `sessionSegmentURLs` returns URLs ordered by manifest `index` | Missing file / unparseable manifest → surfaced test error |
| Empty / unreadable segment | a segment file with zero readable video samples | Fails with a clear "no video frames in segment N" message | Surfaced as failure, not a silent pass |
| Screen↔camera hook | no `camera-####` fixtures present | Alignment test is **skipped** (`XCTSkipIf`, documented for Story 20.1); the alignment function is implemented and ready | n/a |

</intent-contract>

## Code Map

> **Option B, as built (see the Coordinator scope decision below).** The
> original file-PTS-only approach was impossible: `AVAssetWriter.startSession
> (atSourceTime:)` rebases every segment file's media timeline to 0, so a
> gapless and a gapped session read back bit-identically from the files. The
> boundary check therefore reads the **manifest's host-clock PTS bounds**
> (captured by the sidecar before muxing); the file PTS feed only the
> intra-file monotonicity check.

- `tools/keeper-rec/Sources/keeper-rec/Capture.swift` -- 17.1 amendment (minimal, bounds tracking + event fields only): the engine tracks the current segment's first/last **appended video sample** PTS in original capture-clock seconds (`segmentFirstVideoPTS`/`segmentLastVideoPTS`, set on every appended `.screen` frame). `rotate(...)` snapshots the retiring segment's bounds before the cut, resets the trackers to the cut keyframe's PTS for the new segment (the cut keyframe lands in writer B, so segment k+1's first PTS = `keyframePTS`), and the retiring writer's finalize completion emits them as additive `ptsStart`/`ptsEnd` on `segmentClosed{index,path,bytes,track,ptsStart,ptsEnd}`. The FINAL segment (clean stop → `finalized`) still emits no `segmentClosed`, so its bounds are unreported — an accepted, documented limitation (the CI gate runs on generated fixtures with complete bounds).
- `tools/keeper-rec/Sources/keeper-rec/main.swift` -- The `protocolVersion` doc comment records the additive `ptsStart`/`ptsEnd` on `segmentClosed`; the value stays **1** per the additive-change precedent (16.5/16.6).
- `src-tauri/crates/keeper-core/src/recording.rs` -- 17.2 amendment: `RecordingEvent::SegmentClosed` gains `pts_start`/`pts_end: Option<f64>`; `parse_event` reads `ptsStart`/`ptsEnd` best-effort (`as_f64()`, absent/mistyped → `None`, bare index-only lines stay legal events); `SegmentEntry` gains `pts_start`/`pts_end: Option<f64>` (camelCase serde, `#[serde(default)]` so pre-17.4 manifests parse; `None` serializes as explicit `null`, never skipped); `reconcile_from_dir` snapshots the event-fed ledger's bounds by index before the disk rebuild and re-applies them ("disk is the truth for bytes, the host clock is the truth for time" — the rebased files can never yield the bounds back); the `PROTOCOL_VERSION` doc comment records the additive fields, version stays 1. `Eq` dropped from `RecordingEvent`/`SegmentEntry`/`SessionManifest` (f64 fields); unit tests updated/added: enriched + partial + mistyped bounds parsing, bounds-preserving reconcile, null serialization + pre-17.4 deserialization.
- `src-tauri/crates/keeper/src/ipc.rs` -- The `SegmentClosed` → live `SegmentEntry` mapping in `recording_start`'s event sink threads `pts_start`/`pts_end` through, so the live manifest carries them and the terminal reconcile can preserve them.
- `tools/keeper-rec/Tests/keeper-recTests/ConcatAssert.swift` -- The reusable harness (pure AVFoundation + Foundation, no ScreenCaptureKit): `sessionSegments(inFolder:)`/`sessionSegmentURLs(inFolder:)` decode `manifest.json` and return segments/URLs ordered by manifest `index` with their PTS bounds (missing/unparseable manifest → thrown `ConcatHarnessError`); `SegmentTimeline.videoPTS(of:segmentIndex:)` reads a segment's video PTS via `AVAssetReader` + passthrough `AVAssetReaderTrackOutput` (zero-sample marker buffers skipped; no track / zero frames / reader failure → clear thrown error naming the segment); `ConcatViolation` (boundary + kind {gap, overlap, nonMonotonic, missingBounds} + measured seconds); `gaplessConcatViolations(segments:frameRate:epsilon:)` — the **returning** core: per boundary from MANIFEST bounds `delta = ptsStart(k+1) − ptsEnd(k)` with `delta > 0` (else overlap) and `|delta − P| ≤ P + ε` (else gap), plus intra-file strict monotonicity, plus `missingBounds` for any null-bounds segment; `assertGaplessConcat(inFolder:frameRate:)` — the thin wrapper that `XCTFail`s per violation; `assertScreenCameraAlignedWithinOneFrame(screen:camera:frameRate:)` — the implemented Epic-20 hook (first-frame PTS per matching index within `P`).
- `tools/keeper-rec/Tests/keeper-recTests/FixtureSegments.swift` -- `AVAssetWriter`-based fixture generator: `writeFixtureSegment` (H.264 64×64, `movieFragmentInterval` ~4 s, frame reordering disabled so passthrough order == presentation order, host-clock-anchored `startSession` like the real engine), `writeFixtureManifest` (camelCase, Rust-`SessionManifest`-shape-compatible, null bounds as explicit `null`), `makeFixtureSession` + `makeGaplessSession`/`makeSessionWithGap` (≥4·P)/`makeSessionWithOverlap` (≤ ptsEnd(k)), and the `TempSessionDir` RAII temp-dir helper. `.mpeg4CMAFCompliant` is deliberately not set: on a file-URL writer that profile belongs to the segment-delegate API; the fixtures mirror the shipped Capture.swift configuration (`movieFragmentInterval` alone). No SCK, no signing, no committed media.
- `tools/keeper-rec/Tests/keeper-recTests/ConcatAssertTests.swift` -- One XCTest per I/O-matrix row: gapless passes (violations empty + wrapper clean); gap/overlap fail via the RETURNED violation set naming boundary 1→2 and the measured seconds (3·P gap excess / −P overlap); single-segment passes; manifest-`index`-order discovery beats both JSON-array and lexicographic order; missing manifest throws; zero-byte segment throws naming the segment; null manifest bounds → `missingBounds`; `testScreenCameraAlignmentHook` is `XCTSkipIf`'d awaiting Story 20.1's camera fixtures.
- `tools/keeper-rec/Package.swift` -- `linkerSettings` on the `keeper-recTests` target linking `AVFoundation`, `CoreMedia`, `CoreVideo`, `VideoToolbox` (explicit-linking parity with the executable target).
- `scripts/test-keeper-rec.sh` -- Header comment updated: the harness now runs the rotation unit tests AND the NFR-22 gapless-concat gate (fixtures generated on the runner; no capture hardware, no signing). Run step unchanged.
- `.github/workflows/ci.yml` -- `recording` job renamed "Recording sidecar (unit tests + gapless-concat gate)"; comment updated. Run step unchanged.
- `docs/performance.md` -- NFR-22 row in the gate table: "Segment handover gaplessness" → "concatenated manifest PTS bounds monotonic, no boundary gap/overlap > one frame" → the `keeper-rec` concat gate in the `recording` CI job (extends AD-21); note that NFR-22 is the one gate outside the cargo-nextest job.
- `docs/release.md` -- **Recording sidecar (gapless-concat, NFR-22)** added to the required status checks, mapped to the `recording` job in `ci.yml`.

## Tasks & Acceptance

**Execution:**
- [x] `tools/keeper-rec/Sources/keeper-rec/Capture.swift` + `main.swift` -- 17.1 amendment (Option B): track per-segment first/last appended video PTS in original capture-clock seconds and emit them as additive `ptsStart`/`ptsEnd` on the retiring segment's `segmentClosed`; document the additive fields at the `protocolVersion` handshake (value stays 1). Minimal changes — bounds tracking + event fields only, no rotation-mechanics refactor.
- [x] `src-tauri/crates/keeper-core/src/recording.rs` + `src-tauri/crates/keeper/src/ipc.rs` -- 17.2 amendment: lift `ptsStart`/`ptsEnd` into `RecordingEvent::SegmentClosed` (best-effort), persist them on `SegmentEntry` (null when unknown), preserve event-fed bounds by index across the terminal `reconcile_from_dir` (disk stays authoritative for index/file/bytes/track), thread them through the shell's live-ledger mapping; unit tests for parsing, serde null/back-compat, and the bounds-preserving reconcile.
- [x] `tools/keeper-rec/Tests/keeper-recTests/ConcatAssert.swift` -- The harness: manifest-ordered discovery with bounds, `AVAssetReader` intra-file PTS, `ConcatViolation`, the violation-returning `gaplessConcatViolations` core + the `XCTFail`ing `assertGaplessConcat` wrapper, and the implemented `assertScreenCameraAlignedWithinOneFrame` Epic-20 hook.
- [x] `tools/keeper-rec/Tests/keeper-recTests/FixtureSegments.swift` -- The `AVAssetWriter` fixture generator (`makeGaplessSession`, `makeSessionWithGap`, `makeSessionWithOverlap`, RAII temp dir) producing real fMP4 `screen-####.mp4` segments + a Rust-shape-compatible `manifest.json` with host-clock-anchored bounds (AD-38).
- [x] `tools/keeper-rec/Tests/keeper-recTests/ConcatAssertTests.swift` -- XCTests for every I/O-matrix row; negative controls assert the RETURNED violation set (non-vacuousness without fighting `XCTFail`); the `XCTSkipIf` screen↔camera hook.
- [x] `tools/keeper-rec/Package.swift` -- Link `AVFoundation`/`CoreMedia`/`CoreVideo`/`VideoToolbox` to the test target.
- [x] `scripts/test-keeper-rec.sh` + `.github/workflows/ci.yml` -- Comments (and the job name) refreshed to state the harness enforces the NFR-22 gapless-concat gate; run steps unchanged.
- [x] `docs/performance.md` + `docs/release.md` -- NFR-22 gate-table row + the Recording sidecar job in the required-status-checks list.

**Acceptance Criteria:**
- Given a synthesized gapless session of ≥3 segments, when the concat gate runs under `swift test`, then it passes: every boundary's manifest-bounds delta is exactly one frame period and every segment's own file PTS are strictly monotonic (NFR-22). ✅
- Given the negative-control fixtures, when the gate core runs, then the gapped session yields a `gap` violation and the overlapping session an `overlap` violation, each naming boundary 1→2 and the measured seconds — a green suite is therefore non-vacuous. ✅
- Given `bash scripts/test-keeper-rec.sh` (the `recording` CI job) on `macos-latest` with no capture hardware and no code-signing, when it runs, then the concat gate executes and gates the build; `docs/release.md` lists the Recording sidecar job as a required status check and `docs/performance.md` maps NFR-22 to it (AD-38, AD-21). ✅
- Given no `camera-####` fixtures exist, when the suite runs, then the screen↔camera one-frame-alignment test is present but skipped and documented for Story 20.1, and `assertScreenCameraAlignedWithinOneFrame` is implemented and ready (NFR-22, FR-70). ✅

## Design Notes

**Why the boundary check reads the manifest, not the files (Option B).** The
original approach — read each segment file's video PTS and assert cross-cut
continuity — is impossible: `AVAssetWriter.startSession(atSourceTime:)`
rebases every segment file's media timeline to 0 with no edit list, so a
gapless and a gapped session are bit-identical on disk. The distinguishing
information exists only in the capture engine, at capture time, before the
mux. The coordinator-authorized amendment has the engine report each retiring
segment's first/last appended video PTS in **original capture-clock seconds**
on `segmentClosed` (`ptsStart`/`ptsEnd`), the manifest persist them (null when
unknown — older sidecar, recovered session, the final segment), and the gate
assert against them. AD-33's "disk is the truth" applies to bytes; the host
clock is the truth for time — `reconcile_from_dir` accordingly rebuilds
index/file/bytes/track from disk but carries the bounds over by index.

**The one-frame band.** In a continuous 30 fps stream consecutive frames are
one period `P = 1/frameRate` apart, so the expected boundary delta
`δ = ptsStart(k+1) − ptsEnd(k)` is `≈ P` (the cut keyframe goes to the NEXT
writer, so segment k+1 starts exactly one frame after segment k's last frame).

```swift
// boundary check between manifest segments k and k+1 (host-clock seconds)
let period = 1.0 / frameRate            // 30 fps → 0.0333…
let delta = start - end                 // ptsStart(k+1) − ptsEnd(k), expected ≈ P
if delta <= 0 {                         // rewind/duplicate → overlap
    violations.append(.init(boundary: k, kind: .overlap, seconds: delta))
} else if abs(delta - period) > period + epsilon {  // > one frame → gap
    violations.append(.init(boundary: k, kind: .gap, seconds: delta - period))
}
```

`frameRate` is a parameter (30 default) so a 60 fps capture path reuses the
harness with a tighter bound; `epsilon` (1e-6) absorbs float noise only, never
a real frame. Intra-file, each segment's own (rebased) PTS must still be
strictly monotonic — read via passthrough `AVAssetReader` (zero-sample marker
buffers, which passthrough readers interleave around fragment boundaries, are
skipped). Null manifest bounds are a `missingBounds` **failure**, never a
silent pass.

**Why generate fixtures instead of committing them.** AD-38 allows "committed
fixture segments (or output produced on the signed runner)." The repo has no
git-LFS and no committed media; committing opaque binaries would be
unreviewable, unregenerable, and could not express the negative controls.
Generating on the runner with `AVAssetWriter` needs no ScreenCaptureKit and
therefore no display/hardware/signing (only *capture* — `SCStream` — is gated
by macOS TCC + the ad-hoc-signing rejection, Cap #1722; muxing is not). The
fixture writer mirrors 17.1's real handover (`startSession(atSourceTime:)` at
the host-clock anchor; the files come out rebased, exactly like production)
and disables frame reordering so passthrough sample order equals presentation
order. The manifest-driven discovery keeps the harness source-agnostic, so a
future signed-runner capture folder drops straight in.

**Returning violations instead of asserting.** The negative controls are an
explicit acceptance criterion: the gate must demonstrably fail on broken
input. `gaplessConcatViolations` returns `[ConcatViolation]` (throwing only on
harness faults like an unreadable segment), so positive tests assert an empty
set, negative tests assert the exact expected violation — and the production
entry point `assertGaplessConcat` stays a two-line `XCTFail` wrapper.

**Protocol + schema compatibility.** `ptsStart`/`ptsEnd` are additive on an
existing event, read tolerantly (`None` when absent/mistyped, per-field);
`PROTOCOL_VERSION` stays 1 (the 16.5/16.6 additive precedent — keeper and
keeper-rec ship in lockstep). `SegmentEntry` serializes missing bounds as
explicit `null` (no `skip_serializing_if`) and deserializes pre-17.4 manifests
via `#[serde(default)]`. `Eq` was dropped from `RecordingEvent`/`SegmentEntry`/
`SessionManifest` (f64 fields); nothing relied on it beyond `assert_eq!`
(PartialEq).

**Accepted limitation — the final segment of a real capture.** A clean stop
closes the last segment with `finalized` (a `segmentClosed` while the host is
Stopping would be an illegal transition), so the final segment's bounds are
recorded as null in real sessions and its last boundary is reported as
`missingBounds` if a real session folder is ever fed to the gate. The CI gate
runs on generated fixtures carrying complete bounds; wiring final-segment
bounds through `finalized` would destabilize the 17.1 state machine for no CI
benefit and is deliberately out of scope.

## Verification

**Commands:**
- `swift test --package-path tools/keeper-rec` (== `bun run rec:test`) -- **run 2026-07-17, green:** 24 tests — the existing 15 `RotationTests` + 9 `ConcatAssertTests` (8 executed, 1 skipped hook), 0 failures. Gapless/single-segment/discovery pass; gap, overlap, null-bounds, missing-manifest, and zero-byte-segment cases prove the gate fails/throws on broken input; the screen↔camera hook is skipped for Story 20.1. Clean rebuild shows zero compiler warnings.
- `bash scripts/test-keeper-rec.sh` -- identical result, exercised exactly as the `recording` CI job runs it on `macos-latest` with no capture hardware and no signing.
- In `src-tauri`: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo nextest run --no-tests=pass` -- **run 2026-07-17, green:** clippy clean, 864/864 tests passed (including the new recording/manifest bounds tests).

**Manual checks (if no CLI):**
- The Swift capture change is event-field-only (no rotation-mechanics change), and the protocol change is additive with `PROTOCOL_VERSION` still 1 on both sides — no re-sign or `rec:build` needed beyond what CI already does; a live-capture smoke of the enriched `segmentClosed` requires signed hardware (tracked with 12.6/15.6's physical-device work).
- Confirm `docs/release.md` lists the Recording sidecar job among required status checks and `docs/performance.md` has the NFR-22 row pointing at the `recording` job.

## Coordinator scope decision (2026-07-17) — Option B AUTHORIZED

The Block-If is resolved by the spec owner: **implement Option B.** The frozen
17.1/17.2 contracts are hereby amended for this story:

1. **`keeper-rec` (17.1 amendment):** the capture engine reports each segment's
   real host-clock bounds in its `segmentClosed` event —
   `{"event":"segmentClosed","index":N,"ptsStart":<seconds>,"ptsEnd":<seconds>}`
   (the first/last appended video sample's original capture PTS, before the
   writer rebases the file timeline). Segments stay zero-based, standalone
   playable, ordinary files (AD-37 unchanged).
2. **Protocol:** this is an **additive** field on an existing event, consumed
   tolerantly by the host parser (unknown/missing fields already tolerated both
   directions). Per the additive-change precedent (16.5's
   `requestScreenRecording`, 16.6's `startRecording`/`stop`),
   `PROTOCOL_VERSION` stays **1** — keeper and keeper-rec ship in lockstep.
   Update the PROTOCOL_VERSION doc comment to record this addition.
3. **`keeper-core` + manifest (17.2 amendment):** `parse_event` lifts the new
   optional fields into `RecordingEvent::SegmentClosed`; the session ledger /
   `manifest.json` persists per-segment `ptsStart`/`ptsEnd`. Missing bounds
   (older sidecar, recovered sessions) are recorded as null — recovery paths
   stay tolerant.
4. **The gate (17.4 proper):** `assertGaplessConcat` asserts, per boundary,
   `ptsStart(k+1) − ptsEnd(k) ≈ P` (one frame period, with the spec's
   tolerance) from the manifest bounds, plus intra-file PTS monotonicity via
   `AVAssetReader`. The negative controls (gap ≥ 4·P, overlap) must now be
   distinguishable — non-vacuousness is measurable again.

Rationale: the distinguishing information must be captured at the only moment
it exists (in the capture engine, before the muxer rebases); persisting it in
the manifest is exactly the manifest's job (AD-33 "disk is the truth" applies
to bytes, host clock is the truth for time). Option A is rejected — it breaks
AD-37 standalone playability.

Review budget note: keep changes to 17.1's Swift minimal (bounds tracking +
event fields only); do not refactor rotation mechanics.

## Review Triage Log

### 2026-07-17 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 3: (high 0, medium 0, low 3)
- defer: 2: (high 0, medium 1, low 1)
- reject: 14: (high 0, medium 0, low 14)
- addressed_findings:
  - `[low]` `[patch]` A non-numeric capture `CMTime` would make `.seconds` NaN; a NaN in the `segmentClosed` JSON payload makes `JSONSerialization` throw and silently drops the whole event line (index and all), and a non-finite bound could also fail a terminal manifest serialization. Guarded the bounds with `pts.isNumeric` in `Capture.swift` (append path + rotation reset) and reject non-finite bounds in `parse_event` (`recording.rs`), keeping persisted bounds provably finite.
  - `[low]` `[patch]` `gaplessConcatViolations` / `assertScreenCameraAlignedWithinOneFrame` with `frameRate ≤ 0` make `period` infinite so every boundary check passes vacuously; added `precondition(frameRate > 0, …)` to both.
  - `[low]` `[patch]` The Epic-20 alignment hook trapped with a `fatalError` on a duplicate camera manifest index (`Dictionary(uniqueKeysWithValues:)`) and used a strict `> period` tolerance inconsistent with the gate's float-noise epsilon; switched to a tolerant `Dictionary(_:uniquingKeysWith:)` and `> period + epsilon`.

Reviewer findings judged **reject** (faithful to the read-only `<intent-contract>` or not reachable), for the record: the `|δ − P| ≤ P` band tolerating a delta up to `2P` and the `δ ≤ 0` overlap rule are the intent-contract's literal definitions of "no gap or overlap exceeding one frame"; appended-sample bounds tracking is exactly what the Coordinator decision specifies; the intra-file monotonicity check is a deliberate secondary assertion (orthogonal to the boundary check by design); rotation never retires a zero-video segment (`RotationPolicy` forbids rotating a just-opened segment, so its bounds are always non-nil); a faithful gapless boundary's delta ≈ P is nowhere near the `δ ≤ 0` overlap edge, so the missing overlap epsilon is not reachable; `reconcile_from_dir` joins bounds on the segment's canonical `index` identity; hardcoded `frameRate = 30` matches the fixed capture rate. The two **defer** items (real-capture final-segment null bounds; the un-guarded cross-language manifest seam) are logged in `deferred-work.md`.

## Auto Run Result

Status: done

**Implemented change.** Story 17.4 adds the automated NFR-22 gapless-concat release gate, implemented as **Option B** (coordinator-authorized): the `keeper-rec` capture engine now reports each segment's original host-clock `ptsStart`/`ptsEnd` on `segmentClosed` (captured before the muxer rebases the file timeline — the only moment the boundary truth exists); `keeper-core` lifts and persists those bounds on the manifest (null when unknown, preserved across the terminal disk reconcile); and a new pure-AVFoundation Swift harness asserts, per segment boundary, `ptsStart(k+1) − ptsEnd(k) ≈ P` from the manifest bounds plus intra-file PTS monotonicity from the files — with `AVAssetWriter`-generated fixtures (gapless + gap + overlap negative controls) so the gate runs in CI with no capture hardware and no signing (AD-38). `PROTOCOL_VERSION` stays 1 (additive field).

**Files changed.**
- `tools/keeper-rec/Sources/keeper-rec/Capture.swift` — track per-segment first/last appended video PTS (numeric-guarded) and emit `ptsStart`/`ptsEnd` on the retiring segment's `segmentClosed`.
- `tools/keeper-rec/Sources/keeper-rec/main.swift` — document the additive fields at the `protocolVersion` handshake (value unchanged).
- `src-tauri/crates/keeper-core/src/recording.rs` — `pts_start`/`pts_end` on `RecordingEvent::SegmentClosed` + `SegmentEntry` (finite-guarded parse, null serde, index-preserving reconcile), `PROTOCOL_VERSION` doc note, unit tests.
- `src-tauri/crates/keeper/src/ipc.rs` — thread the bounds into the live segment-ledger entry.
- `tools/keeper-rec/Tests/keeper-recTests/ConcatAssert.swift` — the harness (manifest-ordered discovery with bounds, `AVAssetReader` intra-file PTS, `ConcatViolation`, violation-returning core + `XCTFail` wrapper, Epic-20 alignment hook). *(new)*
- `tools/keeper-rec/Tests/keeper-recTests/FixtureSegments.swift` — `AVAssetWriter` fixture generator + Rust-shape-compatible `manifest.json` writer + RAII temp dir. *(new)*
- `tools/keeper-rec/Tests/keeper-recTests/ConcatAssertTests.swift` — one XCTest per I/O-matrix row; negative controls assert the returned violation set; `XCTSkipIf` camera hook. *(new)*
- `tools/keeper-rec/Package.swift` — link AVFoundation/CoreMedia/CoreVideo/VideoToolbox to the test target.
- `scripts/test-keeper-rec.sh`, `.github/workflows/ci.yml` — comments + `recording` job name reflect the NFR-22 gate (run steps unchanged).
- `docs/performance.md`, `docs/release.md` — NFR-22 gate-table row + the Recording sidecar job added to the required status checks.

**Review findings breakdown.** 3 patches applied (all low-severity robustness guards — NaN-PTS, `frameRate > 0`, tolerant alignment-index map); 2 items deferred (real-capture final-segment null bounds; un-guarded cross-language manifest seam) → `deferred-work.md`; 14 findings rejected as faithful-to-contract or unreachable. No `intent_gap`, no `bad_spec`, so no re-derivation loopback.

**Follow-up review recommendation:** false — the final pass made only a few localized, low-consequence defensive fixes; no behavior/API/security/data surface of the shipped change was altered.

**Verification performed.**
- `bash scripts/test-keeper-rec.sh` → 24 tests, 0 failures, 1 skipped (the Epic-20 hook). Clean rebuild, zero Swift warnings.
- `src-tauri`: `cargo fmt --all -- --check` clean, `cargo clippy --all-targets -- -D warnings` clean, `cargo nextest run` 864/864 passed (783/783 for keeper-core after the parse-guard patch).

**Residual risks.** (1) On real (non-fixture) captures the final segment has null bounds, so the last boundary is unverifiable — accepted for this CI/fixtures gate, deferred for real-runner gating. (2) The Swift manifest decoder and the Rust manifest serde are hand-maintained on two sides of an un-guarded language seam — deferred. (3) The one-frame tolerance and 30 fps default are authored bars pending owner sign-off at phase release (epic context), not blockers for the mechanism.
