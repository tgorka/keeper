---
title: 'Dual-Writer Gapless Size-Based Rotation in keeper-rec'
type: 'feature'
created: '2026-07-17'
status: 'done'
baseline_revision: '4ee39f402b744d0d94b1b4476ce3587c5df92fe1'
final_revision: 'd42b6a9'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-17-context.md'
  - '{project-root}/docs/project-context.md'
warnings: [oversized]
---

<intent-contract>

## Intent

**Problem:** `keeper-rec` writes each recording to a single fragmented MP4 (Story 16.6). An hours-long session grows into one unmanageable file, and a crash risks the whole recording — Epic 17 needs size-based rotation that costs at most the tail fragment.

**Approach:** Turn the sidecar's single `AVAssetWriter` into a dual-writer, keyframe-cut, gapless rotation: when the on-disk segment reaches its byte budget (or a duration-cap fallback fires), start writer B at the next video keyframe, hand over without dropping a frame or audio, finalize A asynchronously, and emit a `segmentClosed` event per closed segment. The rotation *trigger policy* and segment-path derivation are extracted into a pure, unit-tested Swift type; the AVFoundation handover is gated by compilation and (separately) Story 17.4's fixture concat test.

## Boundaries & Constraints

**Always:**
- Keep exactly **one** `SCStream` (one capture source, unchanged from 16.6). Rotation swaps `AVAssetWriter`s beneath it, never the stream.
- Cut only at a **video keyframe** (`SCFrameStatus.complete` frame with no `.notDisplayed`/dependency on prior frames) so each segment starts self-decodable; writer B is started at that keyframe's PTS and A stops at the frame just before it.
- All PTS stay **host-clock-anchored** (as 16.6 already anchors the session), so concatenated segments are timestamp-continuous — no gap or overlap beyond one frame at the cut.
- Every segment file stays **fragmented MP4** (`.mpeg4CMAFCompliant`-equivalent via `movieFragmentInterval` ~4 s) throughout recording; a clean `finishWriting` defragments each closed segment into an ordinary playable `.mp4`.
- Emit `{"event":"segmentClosed","index":<u32>,"path":<str>,"bytes":<u64>,"track":"screen"}` when a rotation closes a segment. `index` is the 0-based index of the segment that just closed and **must** be present (the shipped `keeper_core::recording::parse_event` requires it); `path`/`bytes`/`track` are additive for the Story 17.2 ledger.
- Bracket each rotation with the state events the host state machine already parses: `{"event":"state","state":"rotating"}` when the cut begins and `{"event":"state","state":"recording"}` once writer B is live.
- The rotation trigger is the **observed on-disk size** of the current segment (stat the file), not an in-memory appended-byte tally — fMP4 buffering makes the appended count run ahead of what a crash would actually preserve.
- Keep the sidecar's "always exits cleanly" invariant: any rotation/finalize failure surfaces as a single `{"event":"error","message":…}` line and a clean exit, never a panic or signal.

**Block If:**
- The dual-writer handover cannot be made gapless within AVFoundation's single-`SCStream` model without a second capture source or a container change away from fMP4 — HALT `blocked` (this would contradict AD-37's locked route).

**Never:**
- Do not add a session folder, `manifest.json`, or segment ledger (Story 17.2), settings persistence or reading `keeper.db` (Story 17.5), the concat-assert CI gate (Story 17.4), recovery of orphaned segments (Story 17.3), or any camera/microphone track (Epic 19/20). `track` is always `"screen"` here.
- Do not emit `segmentClosed` for the **final** segment on clean stop — its closure is signalled by `finalized` (segmentClosed while `Stopping` is an illegal host transition).
- Do not change the `keeper-rec` protocol version or the Rust request builders; new `start` params are optional and additive.
- No third-party dependencies or ffmpeg (license firewall); Apple system frameworks only.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Size trigger at keyframe | observed on-disk bytes ≥ byte budget, current frame is a keyframe, not the first frame of the segment | Policy returns "rotate now"; handover starts | No error expected |
| Below budget | observed bytes < budget, elapsed < duration cap | Policy returns "continue"; no rotation | No error expected |
| Low-motion / duration-cap fallback | observed bytes far below budget but elapsed ≥ duration cap, at a keyframe | Policy returns "rotate now" | No error expected |
| Budget reached mid-GOP | observed bytes ≥ budget but current frame is **not** a keyframe | Policy returns "continue" until the next keyframe | No error expected |
| First frame of a fresh segment | segment just opened, bytes ~0, at a keyframe | Policy returns "continue" (never rotate a just-opened segment) | No error expected |
| Segment path derivation | first path `…/screen-0000.mp4` | segment N path = zero-padded numeric run incremented (`screen-0001.mp4`, …); no numeric run → insert `-0001` before the extension | Non-writable dir surfaces as `error`, clean exit |
| `start` without `segmentMB`/`maxSegmentSeconds` | params omit the fields | Defaults applied (segment 500 MB, duration cap 1800 s) | No error expected |
| Enriched event vs shipped parser | `{"event":"segmentClosed","index":2,"path":"…","bytes":123,"track":"screen"}` | Rust `parse_event` → `SegmentClosed{index:2}` (extras dropped) | No error expected |

</intent-contract>

## Code Map

- `tools/keeper-rec/Sources/keeper-rec/Rotation.swift` -- **NEW.** Pure rotation-trigger policy + segment-path derivation. `import Foundation` only (no AVFoundation/ScreenCaptureKit) so it is unit-testable without capture hardware or code-signing.
- `tools/keeper-rec/Sources/keeper-rec/Capture.swift` -- Refactor `CaptureEngine` single-writer → dual-writer rotation: current/next `AVAssetWriter`, keyframe-driven handover, `segmentClosed` + `rotating`/`recording` events, async finalize of the retired writer.
- `tools/keeper-rec/Sources/keeper-rec/main.swift` -- `startRecording` handler reads optional `params.segmentMB` (Int) and `params.maxSegmentSeconds` (Int), passes them to `CaptureEngine.start`.
- `tools/keeper-rec/Package.swift` -- Add an XCTest test target for the pure logic.
- `tools/keeper-rec/Tests/keeper-recTests/RotationTests.swift` -- **NEW.** Unit tests for the policy and path derivation (the I/O matrix rows).
- `scripts/test-keeper-rec.sh` -- **NEW.** Runs `swift test --package-path tools/keeper-rec`.
- `package.json` -- Add `"rec:test"` script.
- `.github/workflows/ci.yml` -- Add a `recording` job running the sidecar unit tests (macos-latest, no signing needed for pure tests).
- `src-tauri/crates/keeper-core/src/recording.rs` -- Add one `parse_event` test asserting the enriched `segmentClosed` line still parses to `SegmentClosed{index}` (cross-language seam guard). No production Rust change.

## Tasks & Acceptance

**Execution:**
- [x] `tools/keeper-rec/Sources/keeper-rec/Rotation.swift` -- Add a pure `RotationPolicy` (fields: byte budget, duration cap; method deciding rotate/continue from observed on-disk bytes, elapsed seconds, `isKeyframe`, `isFirstFrameOfSegment`) and a `nextSegmentPath(from:)` deriver -- gives a hardware-free unit-test surface for the trigger + naming.
- [x] `tools/keeper-rec/Sources/keeper-rec/Capture.swift` -- Rework `CaptureEngine` to dual-writer rotation driven by `RotationPolicy`: track observed on-disk size of the current segment, at each complete keyframe ask the policy, and on "rotate" start writer B at that keyframe PTS, route the handover so no frame/audio is dropped, `markAsFinished` + async `finishWriting` on A, then emit `rotating` → `segmentClosed{index,path,bytes,track:"screen"}` (bytes = A's on-disk size) → `recording` -- the gapless dual-writer handover (AD-37).
- [x] `tools/keeper-rec/Sources/keeper-rec/main.swift` -- Read optional `segmentMB`/`maxSegmentSeconds` from `startRecording` params (defaults 500 / 1800) and pass to `CaptureEngine.start` -- lets Story 17.5 supply configured values without a further sidecar change.
- [x] `tools/keeper-rec/Package.swift` + `tools/keeper-rec/Tests/keeper-recTests/RotationTests.swift` -- Add a `.testTarget` and unit tests covering every I/O-matrix policy/path row (`@testable import keeper_rec`; if executable-target testing is unreliable on the toolchain, split the pure logic into a small library target the executable and tests share) -- proves the trigger and naming without capture.
- [x] `scripts/test-keeper-rec.sh` + `package.json` (`rec:test`) + `.github/workflows/ci.yml` -- Wire `swift test --package-path tools/keeper-rec` into a CI job so the policy tests gate merges (Story 17.4 later extends this harness with the concat gate) -- unit tests that never run guard nothing.
- [x] `src-tauri/crates/keeper-core/src/recording.rs` -- Add a `parse_event` test asserting `{"event":"segmentClosed","index":2,"path":"…","bytes":123,"track":"screen"}` → `SegmentClosed{index:2}` -- locks the sidecar↔host event compatibility.

**Acceptance Criteria:**
- Given a running capture whose current segment's on-disk size reaches the byte budget, when the next video keyframe arrives, then the sidecar hands over to a new writer with no dropped frame or audio and emits `rotating`, then `segmentClosed` (with the closed segment's index, path, and on-disk bytes), then `recording`.
- Given a low-motion capture whose size stays below budget, when the duration cap elapses, then a rotation still fires at the next keyframe (fallback), and every rotated segment file is an independently playable fragmented MP4.
- Given `keeper_core::recording::parse_event`, when it receives the enriched `segmentClosed` line, then it yields `SegmentClosed{index}` and the recording state machine bumps its segment counter unchanged.
- Given a clean `stop`, when the session finalizes, then only `stopping` → `finalized` are emitted for the last segment (no `segmentClosed` for it) and all prior segments remain untouched and playable.

## Spec Change Log

_None yet._

## Review Triage Log

### 2026-07-17 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 2: (high 0, medium 1, low 1)
- defer: 1: (high 0, medium 0, low 1)
- reject: 11
- addressed_findings:
  - `[medium]` `[patch]` The cut keyframe was appended to the new writer only `if isReadyForMoreMediaData`, so a not-ready fresh writer would silently start the next segment on a non-keyframe (non-self-decodable, breaking the gapless AC). Now a loud `error` + clean exit instead of silent corruption (`Capture.swift` `rotate`).
  - `[low]` `[patch]` `segmentMB * bytesPerMB` could overflow `UInt64` and trap (crash) on an absurd host value, violating the sidecar's always-exit-cleanly invariant. `segmentMB` is now clamped to `maxSegmentMB` (1 TB) before the product, with a unit test (`Rotation.swift`, `RotationTests.swift`).

## Design Notes

Rotation choreography (per cut), all on the existing serial `mediaQueue`:
```
on each complete video keyframe sample:
  observed = fileSize(currentSegmentPath)            // on-disk, not appended tally
  if policy.shouldRotate(observed, elapsed, isKeyframe:true, isFirst:false):
    emit state:"rotating"
    B = makeWriter(nextSegmentPath); B.startWriting(); B.startSession(at: kfPTS)
    // hand over: append this keyframe onward to B; stop feeding A
    A.video.markAsFinished(); A.audio?.markAsFinished()
    A.finishWriting { emit segmentClosed{index, path:A.path, bytes:fileSize(A.path), track:"screen"}; emit state:"recording" }
    current = B; segmentIndex += 1
```
Audio has no keyframes: split the audio stream at the same PTS boundary the video cuts on (samples before → A, at/after → B); a boundary sample kept whole on one side is within the one-frame tolerance. `isFirstFrameOfSegment` guards against rotating a just-opened segment when the budget is tiny. Keep `segmentMB` → bytes as a documented constant (`segmentMB * 1_000_000`); the exact factor and defaults are authored assumptions, adjustable on dogfooding without a spec change. Actual gapless correctness across the cut is verified on real signed hardware and by Story 17.4's fixture concat-assert — 17.1's automated gate is compile + the pure-policy unit tests.

## Verification

**Commands:**
- `bun run rec:build` -- expected: keeper-rec compiles (release, arm64), installs to `binaries/`, and the existing stdio smoke test still passes.
- `bun run rec:test` (`swift test --package-path tools/keeper-rec`) -- expected: all `RotationTests` pass (trigger + path-derivation rows green).
- `bun run test:rust` -- expected: the new `parse_event` compatibility test and all existing recording tests pass.
- `bun run check:rust` -- expected: rustfmt + clippy clean (no production Rust change, so this is a regression gate).

## Auto Run Result

Status: **done**

### Summary
Turned the `keeper-rec` sidecar's single-`AVAssetWriter` capture (16.6) into dual-writer, keyframe-cut, gapless **size-based rotation**. One `SCStream` still drives one serial media queue; at each complete video frame the engine stats the current segment's **on-disk** size and consults a pure `RotationPolicy` (byte budget from `segmentMB × 1 MB`, duration-cap fallback, keyframe + first-frame guards). On "rotate" it emits `state:"rotating"`, opens writer B anchored at the cut frame's PTS (host-clock-continuous), hands that keyframe and all later samples to B, and finalizes A asynchronously — A's completion emits `segmentClosed{index,path,bytes,track:"screen"}` then `state:"recording"`. A `finalizeGroup` makes clean stop wait for every in-flight finalize; the final segment is closed by `finalized` (no `segmentClosed`, which would be an illegal host transition while Stopping). The rotation trigger + segment-path derivation are pure, Foundation-only, and unit-tested; the AVFoundation handover is gated by compilation and (later) Story 17.4's fixture concat test — 17.1 is not human-in-the-loop.

### Files changed
- `tools/keeper-rec/Sources/keeper-rec/Rotation.swift` (new) — pure `RotationPolicy` (size/duration trigger, keyframe/first-frame guards, overflow-safe budget) + `nextSegmentPath` deriver; Foundation-only.
- `tools/keeper-rec/Sources/keeper-rec/Capture.swift` — single-writer → dual-writer gapless rotation; `segmentClosed` + `rotating`/`recording` bracket; stop waits on `finalizeGroup`.
- `tools/keeper-rec/Sources/keeper-rec/main.swift` — `startRecording` reads optional `segmentMB`/`maxSegmentSeconds` (defaults 500/1800).
- `tools/keeper-rec/Package.swift` + `tools/keeper-rec/Tests/keeper-recTests/RotationTests.swift` (new) — XCTest target; 15 unit tests over the I/O matrix + edges.
- `scripts/test-keeper-rec.sh` (new) + `package.json` (`rec:test`) + `.github/workflows/ci.yml` (`recording` job) — wire `swift test` into CI.
- `src-tauri/crates/keeper-core/src/recording.rs` — one test asserting the enriched `segmentClosed` still parses to `SegmentClosed{index}` (no production Rust change).

### Review findings breakdown
- Two reviewers (adversarial + edge-case) → 14 deduped findings.
- **Patches applied (2):** cut keyframe silently dropped if writer B not ready → now a loud error + clean exit; `segmentMB × bytesPerMB` overflow-trap → `segmentMB` clamped to `maxSegmentMB` (1 TB) + test.
- **Deferred (1):** a `stop` landing during an in-flight rotation can suppress one middle segment's `segmentClosed` (illegal while Stopping); no 17.1 consequence — recorded for Story 17.2's ledger reconciliation.
- **Rejected (11):** finalize-failure/budget-overshoot (bounded by fMP4 fragments + Story 17.3 recovery + sub-second finalize), idle-screen cap (file doesn't grow with no frames), per-frame stat (µs on local APFS), path collision (17.2's fresh session folder), `[self]` retain cycle (one-shot process, AD-34), CI pinning (matches repo conventions), documented final-segment counter seam, and reviewer-withdrawn items.

### Verification
- `bun run rec:test` — PASS (15/15 RotationTests).
- `bun run rec:build` — PASS (release arm64 build + install + all stdio smoke checks).
- `bun run test:rust` — PASS (850/850, incl. the new `parse_enriched_segment_closed_still_yields_index_only`).
- `bun run check:rust` — PASS (rustfmt + clippy `-D warnings`).

### Residual risks
- True gapless-handover correctness (no dropped frame/audio, ≤1-frame concat boundary) can only be proven on dev-signed hardware and by Story 17.4's fixture concat gate — 17.1's automated gate is compile + pure-policy unit tests, as the epic intends.
- The deferred stop-during-rotation ledger race is left for Story 17.2.
