// SPDX-License-Identifier: Apache-2.0
//
// The NFR-22 gapless-concat gate tests (Story 17.4) — one test per I/O-matrix
// row. Fixtures are generated on the runner via AVAssetWriter
// (FixtureSegments.swift): no capture hardware, no code-signing, so the whole
// suite runs in CI (scripts/test-keeper-rec.sh, the `recording` job). The
// negative controls assert against the RETURNED violation set
// (`gaplessConcatViolations`), proving a green run is non-vacuous without
// fighting `XCTFail`.

import XCTest

final class ConcatAssertTests: XCTestCase {
    /// The fixture frame rate (the capture engine's 30 fps default); `P = 1/30 s`.
    private let frameRate = 30.0
    private var period: Double { 1.0 / frameRate }

    // MARK: - I/O-matrix rows

    /// Row: gapless session — ≥3 segments whose manifest bounds continue
    /// exactly one frame across every cut → no violations; the XCTFail
    /// wrapper passes too.
    func testGaplessSessionPasses() async throws {
        let dir = try TempSessionDir(label: "gapless")
        try await makeGaplessSession(in: dir.url)
        let violations = try await gaplessConcatViolations(
            segments: sessionSegments(inFolder: dir.url), frameRate: frameRate)
        XCTAssertEqual(violations, [], "a faithful handover must produce zero violations")
        try await assertGaplessConcat(inFolder: dir.url, frameRate: frameRate)
    }

    /// Row (Story 21.1): the SAME gapless assertion holds for HEVC segments —
    /// the concat gate is codec-agnostic and both shipped codecs stay gapless.
    func testGaplessHevcSessionPasses() async throws {
        let dir = try TempSessionDir(label: "gapless-hevc")
        try await makeGaplessSession(in: dir.url, codec: .hevc)
        let violations = try await gaplessConcatViolations(
            segments: sessionSegments(inFolder: dir.url), frameRate: frameRate)
        XCTAssertEqual(violations, [], "a faithful HEVC handover must produce zero violations")
        try await assertGaplessConcat(inFolder: dir.url, frameRate: frameRate)
    }

    /// Row: gap at a boundary — segment 2 starts 4·P after segment 1's last
    /// frame → exactly one `gap` violation naming boundary 1→2 with the
    /// measured excess (`delta − P = 3·P`).
    func testGapFailsNamingBoundaryAndMeasuredGap() async throws {
        let dir = try TempSessionDir(label: "gap")
        try await makeSessionWithGap(in: dir.url, atBoundary: 1)
        let violations = try await gaplessConcatViolations(
            segments: sessionSegments(inFolder: dir.url), frameRate: frameRate)
        XCTAssertEqual(violations.count, 1, "exactly the injected defect, nothing else")
        let violation = try XCTUnwrap(violations.first)
        XCTAssertEqual(violation.kind, .gap)
        XCTAssertEqual(violation.boundary, 1)
        XCTAssertEqual(violation.seconds, 3 * period, accuracy: 1e-6)
        XCTAssertTrue(
            violation.description.contains("1→2"),
            "the failure must name the offending boundary: \(violation)")
    }

    /// Row: overlap at a boundary — segment 2's first PTS lands one frame
    /// BEFORE segment 1's last → exactly one `overlap` violation naming
    /// boundary 1→2 with the non-positive delta.
    func testOverlapFailsNamingBoundaryAndMeasuredOverlap() async throws {
        let dir = try TempSessionDir(label: "overlap")
        try await makeSessionWithOverlap(in: dir.url, atBoundary: 1)
        let violations = try await gaplessConcatViolations(
            segments: sessionSegments(inFolder: dir.url), frameRate: frameRate)
        XCTAssertEqual(violations.count, 1, "exactly the injected defect, nothing else")
        let violation = try XCTUnwrap(violations.first)
        XCTAssertEqual(violation.kind, .overlap)
        XCTAssertEqual(violation.boundary, 1)
        XCTAssertEqual(violation.seconds, -period, accuracy: 1e-6)
    }

    /// Row: single-segment session — no boundary to check; the intra-file
    /// strict-monotonicity assertion still runs and passes.
    func testSingleSegmentSessionPasses() async throws {
        let dir = try TempSessionDir(label: "single")
        try await makeGaplessSession(in: dir.url, segments: 1)
        let violations = try await gaplessConcatViolations(
            segments: sessionSegments(inFolder: dir.url), frameRate: frameRate)
        XCTAssertEqual(violations, [])
    }

    /// Row: manifest-driven discovery — file names whose lexicographic
    /// (filesystem) order is the REVERSE of the manifest indices, and a
    /// shuffled manifest array: `sessionSegments` must return manifest-`index`
    /// order, and the timeline must still gate green in that order.
    func testDiscoveryFollowsManifestIndexOrderNotFilesystemOrder() async throws {
        let dir = try TempSessionDir(label: "order")
        // index 0 → "seg-c", 1 → "seg-b", 2 → "seg-a"; manifest array [2,0,1].
        let names = ["seg-c.mov", "seg-b.mov", "seg-a.mov"]
        try await makeFixtureSession(
            in: dir.url, fileName: { names[$0] }, manifestOrder: [2, 0, 1])
        let segments = try sessionSegments(inFolder: dir.url)
        XCTAssertEqual(segments.map { $0.index }, [0, 1, 2], "sorted by manifest index")
        XCTAssertEqual(
            segments.map { $0.url.lastPathComponent }, names,
            "index order wins over both the JSON-array and the lexicographic order")
        let violations = try await gaplessConcatViolations(
            segments: segments, frameRate: frameRate)
        XCTAssertEqual(violations, [], "in index order the session is gapless")
    }

    /// Row: missing/unparseable manifest — a folder without `manifest.json`
    /// surfaces a thrown, descriptive harness error, never a silent pass.
    func testMissingManifestThrows() throws {
        let dir = try TempSessionDir(label: "no-manifest")
        XCTAssertThrowsError(try sessionSegments(inFolder: dir.url)) { error in
            XCTAssertTrue(
                String(describing: error).contains("manifest.json"),
                "the error must name the manifest: \(error)")
        }
    }

    /// Row: empty / unreadable segment — a zero-byte `screen-0001.mov` yields
    /// no readable video, which must throw a clear error naming the segment.
    func testEmptySegmentFailsClearly() async throws {
        let dir = try TempSessionDir(label: "empty")
        let segments = try await makeGaplessSession(in: dir.url, segments: 2)
        // Truncate segment 1 to zero bytes; the manifest still lists it.
        try Data().write(to: dir.url.appendingPathComponent(segments[1].file))
        do {
            _ = try await gaplessConcatViolations(
                segments: sessionSegments(inFolder: dir.url), frameRate: frameRate)
            XCTFail("a zero-readable-video segment must throw, never pass silently")
        } catch let error as ConcatHarnessError {
            XCTAssertTrue(
                String(describing: error).contains("segment 1"),
                "the error must name the segment: \(error)")
        }
    }

    /// Null manifest bounds (an older sidecar / a real capture's final
    /// segment) make the boundary unverifiable — the gate reports a clear
    /// `missingBounds` violation rather than passing vacuously.
    func testNullManifestBoundsFailAsMissingBounds() async throws {
        let dir = try TempSessionDir(label: "null-bounds")
        try await makeFixtureSession(in: dir.url, nullBoundsForIndex: 1)
        let violations = try await gaplessConcatViolations(
            segments: sessionSegments(inFolder: dir.url), frameRate: frameRate)
        XCTAssertEqual(
            violations,
            [ConcatViolation(boundary: 1, kind: .missingBounds, seconds: 0)],
            "null bounds must fail loudly, and both adjacent boundary checks are skipped")
    }

    // MARK: - Screen↔camera alignment (Story 20.1, FR-70)

    /// Row: a faithful dual-track session — same-index `screen-####` /
    /// `camera-####` pairs cut at the same boundaries. Each track is
    /// independently gapless AND every same-index pair starts within one
    /// frame period; the XCTFail wrapper passes too. This populates the
    /// Story 17.4 alignment hook.
    func testDualTrackAlignedSessionPasses() async throws {
        let dir = try TempSessionDir(label: "dual-aligned")
        try await makeDualTrackSession(in: dir.url)
        let screen = try sessionSegments(inFolder: dir.url, track: "screen")
        let camera = try sessionSegments(inFolder: dir.url, track: "camera")
        XCTAssertEqual(screen.count, 3, "the per-track reader must find every screen segment")
        XCTAssertEqual(camera.count, 3, "the per-track reader must find every camera segment")
        // Each track holds the NFR-22 gapless gate on its own timeline…
        let screenViolations = try await gaplessConcatViolations(
            segments: screen, frameRate: frameRate)
        XCTAssertEqual(screenViolations, [], "the screen track must be gapless")
        let cameraViolations = try await gaplessConcatViolations(
            segments: camera, frameRate: frameRate)
        XCTAssertEqual(cameraViolations, [], "the camera track must be gapless")
        // …and every same-index pair is aligned within one frame period.
        XCTAssertEqual(
            screenCameraAlignmentViolations(screen: screen, camera: camera, frameRate: frameRate),
            [], "same-index boundaries must align within one video frame")
        assertScreenCameraAlignedWithinOneFrame(
            screen: screen, camera: camera, frameRate: frameRate)
    }

    /// A sub-frame skew is still aligned — the tolerance is one frame period,
    /// not exactness (the two capture sessions anchor on their own first
    /// samples, which land within a frame of each other on real hardware).
    func testSubFrameCameraSkewStillPasses() async throws {
        let dir = try TempSessionDir(label: "dual-subframe")
        try await makeDualTrackSession(
            in: dir.url, cameraSkewSeconds: 0.5 * period, skewIndex: 1)
        let screen = try sessionSegments(inFolder: dir.url, track: "screen")
        let camera = try sessionSegments(inFolder: dir.url, track: "camera")
        XCTAssertEqual(
            screenCameraAlignmentViolations(screen: screen, camera: camera, frameRate: frameRate),
            [], "a skew within one frame period is aligned by contract (NFR-22)")
    }

    /// Regression pin (the segment-0 warm-up fix): a camera whose first REAL
    /// frame lands 3·P after the screen anchor still reports the SHARED
    /// anchor as segment 0's `ptsStart` — the alignment gate passes, and the
    /// camera track's own concat gate stays green (the anchored `ptsStart`
    /// never perturbs the boundary chain; `ptsEnd` is still the last real
    /// frame). The pre-fix engine reported the camera's own late first frame
    /// instead — that shape is the negative control below.
    func testCameraWarmupAnchoredAtScreenBoundaryPasses() async throws {
        let dir = try TempSessionDir(label: "dual-warmup-anchored")
        let planned = try await makeDualTrackSession(in: dir.url, cameraWarmupFrames: 3)
        XCTAssertEqual(
            planned.camera[0].ptsStart, planned.screen[0].ptsStart,
            "the fixture models the fixed semantics: segment 0 reports the shared anchor")
        let screen = try sessionSegments(inFolder: dir.url, track: "screen")
        let camera = try sessionSegments(inFolder: dir.url, track: "camera")
        XCTAssertEqual(
            screenCameraAlignmentViolations(screen: screen, camera: camera, frameRate: frameRate),
            [], "an anchored segment-0 ptsStart must pass regardless of camera warm-up")
        let cameraViolations = try await gaplessConcatViolations(
            segments: camera, frameRate: frameRate)
        XCTAssertEqual(cameraViolations, [], "the camera track stays gapless under warm-up")
    }

    /// Negative control (the pre-fix engine's segment-0 shape): reporting the
    /// camera's OWN warm-up-delayed first frame as `ptsStart` misaligns the
    /// pair by the whole warm-up (3·P here) → exactly one `gap` violation at
    /// index 0. Pins that the anchoring fix is load-bearing: un-anchoring
    /// segment 0 fails CI, the gate is not weakened.
    func testCameraWarmupReportedAsOwnFirstFrameFails() async throws {
        let dir = try TempSessionDir(label: "dual-warmup-unanchored")
        try await makeDualTrackSession(
            in: dir.url, cameraSkewSeconds: 3 * period, skewIndex: 0)
        let screen = try sessionSegments(inFolder: dir.url, track: "screen")
        let camera = try sessionSegments(inFolder: dir.url, track: "camera")
        let violations = screenCameraAlignmentViolations(
            screen: screen, camera: camera, frameRate: frameRate)
        XCTAssertEqual(violations.count, 1, "exactly the warm-up misalignment, nothing else")
        let violation = try XCTUnwrap(violations.first)
        XCTAssertEqual(violation.kind, .gap)
        XCTAssertEqual(violation.boundary, 0, "segment 0 is the warm-up casualty")
        XCTAssertEqual(violation.seconds, 3 * period, accuracy: 1e-6)
    }

    /// Negative control: a camera segment starting 4·P after its screen twin
    /// → exactly one `gap` violation naming the pair's index with the
    /// measured skew.
    func testCameraSkewBeyondOneFrameFails() async throws {
        let dir = try TempSessionDir(label: "dual-skew")
        try await makeDualTrackSession(
            in: dir.url, cameraSkewSeconds: 4 * period, skewIndex: 1)
        let screen = try sessionSegments(inFolder: dir.url, track: "screen")
        let camera = try sessionSegments(inFolder: dir.url, track: "camera")
        let violations = screenCameraAlignmentViolations(
            screen: screen, camera: camera, frameRate: frameRate)
        XCTAssertEqual(violations.count, 1, "exactly the injected defect, nothing else")
        let violation = try XCTUnwrap(violations.first)
        XCTAssertEqual(violation.kind, .gap)
        XCTAssertEqual(violation.boundary, 1, "the violation names the misaligned pair's index")
        XCTAssertEqual(violation.seconds, 4 * period, accuracy: 1e-6)
    }

    /// Negative control: an index with no camera counterpart (a camera lost
    /// mid-session leaves the tail unpaired) is unverifiable → `missingBounds`
    /// for that pair, never a silent pass.
    func testMissingCameraCounterpartFailsAsMissingBounds() async throws {
        let dir = try TempSessionDir(label: "dual-dropped")
        try await makeDualTrackSession(in: dir.url, dropCameraIndex: 1)
        let screen = try sessionSegments(inFolder: dir.url, track: "screen")
        let camera = try sessionSegments(inFolder: dir.url, track: "camera")
        XCTAssertEqual(camera.count, 2, "index 1 has no camera counterpart")
        XCTAssertEqual(
            screenCameraAlignmentViolations(screen: screen, camera: camera, frameRate: frameRate),
            [ConcatViolation(boundary: 1, kind: .missingBounds, seconds: 0)])
    }

    /// Negative control: null camera manifest bounds make the pair
    /// unverifiable → `missingBounds`, mirroring the concat gate's rule.
    func testNullCameraBoundsFailAsMissingBounds() async throws {
        let dir = try TempSessionDir(label: "dual-null-bounds")
        try await makeDualTrackSession(in: dir.url, nullCameraBoundsForIndex: 2)
        let screen = try sessionSegments(inFolder: dir.url, track: "screen")
        let camera = try sessionSegments(inFolder: dir.url, track: "camera")
        XCTAssertEqual(
            screenCameraAlignmentViolations(screen: screen, camera: camera, frameRate: frameRate),
            [ConcatViolation(boundary: 2, kind: .missingBounds, seconds: 0)])
    }
}
