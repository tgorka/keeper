// SPDX-License-Identifier: Apache-2.0
//
// Unit tests for the pure rotation-trigger policy + segment-path derivation
// (Story 17.1, Rotation.swift). Foundation-only logic under test — no capture
// hardware, no code-signing, so this suite runs anywhere macOS Swift does
// (including CI, via scripts/test-keeper-rec.sh). Each test maps to a row of
// the spec's I/O & edge-case matrix.

import XCTest

@testable import keeper_rec

final class RotationTests: XCTestCase {
    /// The authored defaults: 500 MB byte budget, 1800 s duration cap.
    private let policy = RotationPolicy(
        segmentMB: RotationPolicy.defaultSegmentMB,
        maxSegmentSeconds: RotationPolicy.defaultMaxSegmentSeconds)

    // MARK: - Trigger policy (I/O-matrix rows)

    /// Row: size trigger at keyframe — observed on-disk bytes ≥ budget at a
    /// keyframe (not the segment's first frame) → rotate now.
    func testSizeTriggerAtKeyframeRotates() {
        XCTAssertTrue(
            policy.shouldRotate(
                observedBytes: 500_000_000, elapsedSeconds: 60, isKeyframe: true,
                isFirstFrameOfSegment: false))
        // Well past the budget rotates too, not only the exact boundary.
        XCTAssertTrue(
            policy.shouldRotate(
                observedBytes: 623_000_000, elapsedSeconds: 60, isKeyframe: true,
                isFirstFrameOfSegment: false))
    }

    /// Row: below budget — bytes < budget and elapsed < cap → continue.
    func testBelowBudgetContinues() {
        XCTAssertFalse(
            policy.shouldRotate(
                observedBytes: 499_999_999, elapsedSeconds: 1799, isKeyframe: true,
                isFirstFrameOfSegment: false))
        XCTAssertFalse(
            policy.shouldRotate(
                observedBytes: 0, elapsedSeconds: 0, isKeyframe: true,
                isFirstFrameOfSegment: false))
    }

    /// Row: low-motion / duration-cap fallback — bytes far below budget but
    /// elapsed ≥ cap, at a keyframe → rotate now.
    func testDurationCapFallbackRotatesLowMotionSegments() {
        XCTAssertTrue(
            policy.shouldRotate(
                observedBytes: 12_000_000, elapsedSeconds: 1800, isKeyframe: true,
                isFirstFrameOfSegment: false))
        XCTAssertTrue(
            policy.shouldRotate(
                observedBytes: 0, elapsedSeconds: 5000, isKeyframe: true,
                isFirstFrameOfSegment: false))
    }

    /// Row: budget reached mid-GOP — bytes ≥ budget but the current frame is
    /// not a keyframe → continue until the next keyframe.
    func testBudgetReachedMidGopWaitsForKeyframe() {
        XCTAssertFalse(
            policy.shouldRotate(
                observedBytes: 700_000_000, elapsedSeconds: 3600, isKeyframe: false,
                isFirstFrameOfSegment: false))
    }

    /// Row: first frame of a fresh segment — never rotate a just-opened
    /// segment, even when a tiny budget/cap is already exceeded.
    func testFirstFrameOfFreshSegmentNeverRotates() {
        XCTAssertFalse(
            policy.shouldRotate(
                observedBytes: 900_000_000, elapsedSeconds: 9999, isKeyframe: true,
                isFirstFrameOfSegment: true))
    }

    /// Row: `start` without `segmentMB`/`maxSegmentSeconds` — the authored
    /// defaults (500 MB, 1800 s) and the documented decimal-MB byte factor.
    func testDefaultsAndByteFactor() {
        XCTAssertEqual(RotationPolicy.defaultSegmentMB, 500)
        XCTAssertEqual(RotationPolicy.defaultMaxSegmentSeconds, 1800)
        XCTAssertEqual(RotationPolicy.bytesPerMB, 1_000_000)
        XCTAssertEqual(policy.byteBudget, 500_000_000)
        XCTAssertEqual(policy.durationCapSeconds, 1800)
    }

    /// Non-positive wire values are clamped to 1, never a degenerate
    /// rotate-every-frame budget of zero.
    func testNonPositiveKnobsAreClampedNotDegenerate() {
        let clamped = RotationPolicy(segmentMB: 0, maxSegmentSeconds: -5)
        XCTAssertEqual(clamped.byteBudget, RotationPolicy.bytesPerMB)
        XCTAssertEqual(clamped.durationCapSeconds, 1)
    }

    /// An absurdly large `segmentMB` is capped at `maxSegmentMB` so the
    /// `segmentMB * bytesPerMB` product can never overflow `UInt64` and trap —
    /// the sidecar must exit cleanly, never crash, on a bad host value.
    func testHugeSegmentMBIsCappedWithoutOverflow() {
        let capped = RotationPolicy(segmentMB: .max, maxSegmentSeconds: .max)
        XCTAssertEqual(
            capped.byteBudget, UInt64(RotationPolicy.maxSegmentMB) * RotationPolicy.bytesPerMB)
    }

    // MARK: - Segment-path derivation (I/O-matrix row)

    /// Row: segment path derivation — the zero-padded trailing numeric run
    /// increments, padding width preserved.
    func testNumericRunIncrementsWithZeroPadding() {
        XCTAssertEqual(
            nextSegmentPath(from: "/rec/keeper/screen-0000.mp4"),
            "/rec/keeper/screen-0001.mp4")
        XCTAssertEqual(
            nextSegmentPath(from: "/rec/keeper/screen-0041.mp4"),
            "/rec/keeper/screen-0042.mp4")
        XCTAssertEqual(
            nextSegmentPath(from: "/rec/keeper/screen-0009.mp4"),
            "/rec/keeper/screen-0010.mp4")
    }

    /// The padding width grows past its last representable value rather than
    /// wrapping back to zero.
    func testNumericRunWidthGrowsWhenExhausted() {
        XCTAssertEqual(nextSegmentPath(from: "/rec/screen-9999.mp4"), "/rec/screen-10000.mp4")
    }

    /// Row: no numeric run → `-0001` inserted before the extension.
    func testNoNumericRunInsertsSuffixBeforeExtension() {
        XCTAssertEqual(nextSegmentPath(from: "/rec/screen.mp4"), "/rec/screen-0001.mp4")
    }

    /// A filename without an extension still derives (suffix appended / run
    /// incremented at the end).
    func testNoExtensionStillDerives() {
        XCTAssertEqual(nextSegmentPath(from: "/rec/screen"), "/rec/screen-0001")
        XCTAssertEqual(nextSegmentPath(from: "/rec/screen-0001"), "/rec/screen-0002")
    }

    /// A bare filename (no directory component) keeps working.
    func testBareFilenameWithoutDirectory() {
        XCTAssertEqual(nextSegmentPath(from: "screen-0000.mp4"), "screen-0001.mp4")
    }

    /// Only the TRAILING numeric run increments — earlier digit groups (e.g. a
    /// local-time-stamped filename) are untouched.
    func testTimestampedStemIncrementsTrailingRunOnly() {
        XCTAssertEqual(
            nextSegmentPath(from: "/rec/2026-07-17 10-30-00.mp4"),
            "/rec/2026-07-17 10-30-01.mp4")
    }

    /// A trailing run too long to represent as a UInt64 falls back to the
    /// suffix-insert path instead of crashing or wrapping.
    func testOverlongNumericRunFallsBackToSuffix() {
        XCTAssertEqual(
            nextSegmentPath(from: "/rec/screen-99999999999999999999.mp4"),
            "/rec/screen-99999999999999999999-0001.mp4")
    }
}
