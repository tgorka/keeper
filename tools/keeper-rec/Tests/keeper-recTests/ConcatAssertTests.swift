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

    /// `camera-####` fixtures do not exist yet — Story 20.1 (Epic 20 webcam
    /// capture) generates them and enables the alignment test below.
    private let cameraFixturesAbsent = true

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
        let names = ["seg-c.mp4", "seg-b.mp4", "seg-a.mp4"]
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

    /// Row: empty / unreadable segment — a zero-byte `screen-0001.mp4` yields
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

    // MARK: - Epic-20 hook

    /// Row: screen↔camera one-frame alignment (FR-70) — the assertion
    /// function is implemented and ready
    /// (`assertScreenCameraAlignedWithinOneFrame`), but `camera-####` fixtures
    /// land with Story 20.1, so this stays skipped until then.
    func testScreenCameraAlignmentHook() throws {
        try XCTSkipIf(
            cameraFixturesAbsent,
            "camera-#### fixtures land with Story 20.1 (Epic 20 webcam capture); "
                + "assertScreenCameraAlignedWithinOneFrame is implemented and ready")
    }
}
