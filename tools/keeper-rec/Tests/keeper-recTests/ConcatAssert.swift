// SPDX-License-Identifier: Apache-2.0
//
// The NFR-22 gapless-concat harness (Story 17.4, Option B) — pure
// AVFoundation + Foundation, no ScreenCaptureKit, so it runs in CI with no
// capture hardware and no code-signing (AD-38).
//
// Why the boundary check reads the MANIFEST, not the files: the capture
// engine's `startSession(atSourceTime:)` rebases every segment file's own
// media timeline to 0 with no edit list, so a gapless and a gapped session
// read back bit-identically from the `.mp4`s. The host-clock boundary truth
// exists only at capture time; Story 17.4's 17.1/17.2 amendment has the
// sidecar report each segment's original capture-clock `ptsStart`/`ptsEnd` on
// `segmentClosed` and the manifest persist them. The gate therefore asserts:
//
//   (a) per boundary, FROM MANIFEST BOUNDS: `delta = ptsStart(k+1) − ptsEnd(k)`
//       is strictly positive (else overlap) and `|delta − P| ≤ P + epsilon`
//       (else gap), where `P = 1/frameRate` — the literal "no gap or overlap
//       exceeding one frame duration";
//   (b) intra-file, FROM THE FILES: each segment's own video PTS (read via
//       `AVAssetReader`, passthrough) is strictly monotonic.
//
// The core check RETURNS `[ConcatViolation]` so the negative-control tests
// can assert a non-empty violation set without fighting `XCTFail`; the thin
// `assertGaplessConcat` wrapper turns each violation into an `XCTFail` naming
// the boundary and the measured seconds.

import AVFoundation
import CoreMedia
import Foundation
import XCTest

/// A harness fault (as opposed to a gate violation): an unreadable manifest or
/// a segment the reader cannot produce video frames from. Thrown, so it
/// surfaces as a test error — never a silent pass.
enum ConcatHarnessError: Error, CustomStringConvertible {
    case manifestUnreadable(folder: String, underlying: String)
    case noVideoTrack(segmentIndex: Int)
    case noVideoFrames(segmentIndex: Int)
    case readerFailed(segmentIndex: Int, underlying: String)

    var description: String {
        switch self {
        case .manifestUnreadable(let folder, let underlying):
            return "manifest.json missing/unparseable in \(folder): \(underlying)"
        case .noVideoTrack(let segmentIndex):
            return "no video track in segment \(segmentIndex)"
        case .noVideoFrames(let segmentIndex):
            return "no video frames in segment \(segmentIndex)"
        case .readerFailed(let segmentIndex, let underlying):
            return "AVAssetReader failed on segment \(segmentIndex): \(underlying)"
        }
    }
}

/// One segment as the session's `manifest.json` describes it: the manifest
/// `index`, the resolved file URL, and the host-clock PTS bounds the sidecar
/// reported at capture time (`nil` when the manifest recorded them as null —
/// e.g. the final segment of a real capture, or an older sidecar).
struct SessionSegment {
    let index: Int
    let url: URL
    let ptsStart: Double?
    let ptsEnd: Double?
}

/// The manifest slice the harness needs (the Rust `SessionManifest` writes
/// camelCase; unknown fields are ignored by `JSONDecoder`). `track` is read
/// tolerantly (Story 20.1: a dual-track manifest carries `"screen"` and
/// `"camera"` entries; an older manifest without the field reads as screen).
private struct ManifestDocument: Decodable {
    struct Segment: Decodable {
        let index: Int
        let file: String
        let ptsStart: Double?
        let ptsEnd: Double?
        let track: String?
    }
    let segments: [Segment]
}

/// Decode `<folder>/manifest.json` once — shared by the all-segments and
/// per-track readers. A missing or unparseable manifest throws.
private func manifestDocument(inFolder folder: URL) throws -> ManifestDocument {
    let manifestURL = folder.appendingPathComponent("manifest.json")
    do {
        let data = try Data(contentsOf: manifestURL)
        return try JSONDecoder().decode(ManifestDocument.self, from: data)
    } catch {
        throw ConcatHarnessError.manifestUnreadable(
            folder: folder.path, underlying: String(describing: error))
    }
}

/// Map manifest entries into `SessionSegment`s ordered by manifest `index`.
private func orderedSegments(
    _ entries: [ManifestDocument.Segment], inFolder folder: URL
) -> [SessionSegment] {
    entries
        .sorted { $0.index < $1.index }
        .map { segment in
            SessionSegment(
                index: segment.index,
                url: folder.appendingPathComponent(segment.file),
                ptsStart: segment.ptsStart,
                ptsEnd: segment.ptsEnd)
        }
}

/// Decode `<folder>/manifest.json` and return the session's segments ordered
/// by manifest `index` (NOT filesystem order), each with its manifest PTS
/// bounds. A missing or unparseable manifest throws — a surfaced test error.
func sessionSegments(inFolder folder: URL) throws -> [SessionSegment] {
    orderedSegments(try manifestDocument(inFolder: folder).segments, inFolder: folder)
}

/// The per-track reader (Story 20.1): only the segments whose manifest
/// `track` matches (an absent `track` reads as `"screen"` — pre-20.1
/// manifests carried the field implicitly). The gapless gate runs per track;
/// the alignment gate pairs the two lists by index.
func sessionSegments(inFolder folder: URL, track: String) throws -> [SessionSegment] {
    let entries = try manifestDocument(inFolder: folder).segments
        .filter { ($0.track ?? "screen") == track }
    return orderedSegments(entries, inFolder: folder)
}

/// The segment file URLs ordered by manifest `index` — the harness runs
/// identically on committed fixtures and on real signed-runner output.
func sessionSegmentURLs(inFolder folder: URL) throws -> [URL] {
    try sessionSegments(inFolder: folder).map { $0.url }
}

/// Reads one segment file's video-track sample PTS (seconds) — the intra-file
/// half of the gate.
enum SegmentTimeline {
    /// The segment's video sample PTS in seconds, in output order, via
    /// `AVAssetReader` + `AVAssetReaderTrackOutput` (nil output settings →
    /// passthrough, cheap — no decode). Throws a clear error when the asset
    /// has no video track, yields zero samples, or the reader fails.
    static func videoPTS(of url: URL, segmentIndex: Int) async throws -> [Double] {
        let asset = AVURLAsset(url: url)
        guard let track = try? await asset.loadTracks(withMediaType: .video).first else {
            throw ConcatHarnessError.noVideoTrack(segmentIndex: segmentIndex)
        }
        let reader: AVAssetReader
        do {
            reader = try AVAssetReader(asset: asset)
        } catch {
            throw ConcatHarnessError.readerFailed(
                segmentIndex: segmentIndex, underlying: String(describing: error))
        }
        let output = AVAssetReaderTrackOutput(track: track, outputSettings: nil)
        reader.add(output)
        guard reader.startReading() else {
            throw ConcatHarnessError.readerFailed(
                segmentIndex: segmentIndex,
                underlying: reader.error.map(String.init(describing:)) ?? "unknown")
        }
        var pts: [Double] = []
        while let sample = output.copyNextSampleBuffer() {
            // Passthrough readers interleave zero-sample marker buffers (e.g.
            // around fragment boundaries) with the real samples — only buffers
            // that actually carry a sample contribute a frame PTS.
            guard CMSampleBufferGetNumSamples(sample) > 0 else { continue }
            pts.append(CMSampleBufferGetPresentationTimeStamp(sample).seconds)
        }
        guard reader.status == .completed else {
            throw ConcatHarnessError.readerFailed(
                segmentIndex: segmentIndex,
                underlying: reader.error.map(String.init(describing:)) ?? "unknown")
        }
        guard !pts.isEmpty else {
            throw ConcatHarnessError.noVideoFrames(segmentIndex: segmentIndex)
        }
        return pts
    }
}

/// One NFR-22 breach, self-describing: which boundary (or segment), what
/// kind, and the measured seconds.
struct ConcatViolation: Equatable, CustomStringConvertible {
    enum Kind: String {
        /// Boundary delta exceeds one frame period beyond the expected `P`.
        case gap
        /// Boundary delta is zero/negative — segment k+1 rewinds into k.
        case overlap
        /// A segment's own file PTS are not strictly increasing.
        case nonMonotonic
        /// The manifest recorded null bounds — the boundary is unverifiable,
        /// which the gate treats as a failure, never a silent pass.
        case missingBounds
    }

    /// For `gap`/`overlap`: the boundary `k` (between manifest segments `k`
    /// and `k+1`, 0-based manifest order). For `nonMonotonic`/`missingBounds`:
    /// the offending segment's manifest index.
    let boundary: Int
    let kind: Kind
    /// The measured seconds: `delta − P` for a gap, `delta` (≤ 0) for an
    /// overlap, the non-positive step for nonMonotonic, 0 for missing bounds.
    let seconds: Double

    var description: String {
        switch kind {
        case .gap:
            return "boundary \(boundary)→\(boundary + 1): gap of \(seconds)s beyond one frame"
        case .overlap:
            return "boundary \(boundary)→\(boundary + 1): overlap (delta \(seconds)s ≤ 0)"
        case .nonMonotonic:
            return "segment \(boundary): file PTS not strictly monotonic (step \(seconds)s)"
        case .missingBounds:
            return "segment \(boundary): manifest ptsStart/ptsEnd are null — boundary unverifiable"
        }
    }
}

/// The NFR-22 gate core: every violation in the session, or `[]` when the
/// concatenation is gapless. Boundary deltas come from the MANIFEST bounds
/// (the only place the host-clock truth survives muxing); intra-file
/// monotonicity comes from the files via `AVAssetReader`. Harness faults
/// (unreadable segment, zero frames) throw. `P = 1/frameRate`; `epsilon`
/// absorbs floating-point noise only, never a real frame.
func gaplessConcatViolations(
    segments: [SessionSegment], frameRate: Double, epsilon: Double = 1e-6
) async throws -> [ConcatViolation] {
    // A non-positive frame rate makes `period` infinite/NaN, which would let
    // every boundary comparison pass vacuously — a misuse, not a gate result.
    precondition(frameRate > 0, "frameRate must be positive, got \(frameRate)")
    let period = 1.0 / frameRate
    var violations: [ConcatViolation] = []

    // (b) intra-file: each segment's own video PTS strictly monotonic, and
    // every segment must have manifest bounds for its boundaries to be
    // checkable at all.
    for segment in segments {
        if segment.ptsStart == nil || segment.ptsEnd == nil {
            violations.append(
                ConcatViolation(boundary: segment.index, kind: .missingBounds, seconds: 0))
        }
        let pts = try await SegmentTimeline.videoPTS(of: segment.url, segmentIndex: segment.index)
        for i in 1..<pts.count where pts[i] <= pts[i - 1] {
            violations.append(
                ConcatViolation(
                    boundary: segment.index, kind: .nonMonotonic,
                    seconds: pts[i] - pts[i - 1]))
        }
    }

    // (a) per boundary, from the manifest bounds: the next segment's first
    // frame is expected exactly one frame period after this one's last.
    for k in 0..<max(0, segments.count - 1) {
        guard let end = segments[k].ptsEnd, let start = segments[k + 1].ptsStart else {
            continue  // already reported as missingBounds above
        }
        let delta = start - end
        if delta <= 0 {
            violations.append(ConcatViolation(boundary: k, kind: .overlap, seconds: delta))
        } else if abs(delta - period) > period + epsilon {
            violations.append(
                ConcatViolation(boundary: k, kind: .gap, seconds: delta - period))
        }
    }
    return violations
}

/// The thin XCTest-facing wrapper: reads the session from its `manifest.json`,
/// runs the gate, and `XCTFail`s once per violation with the boundary + the
/// measured seconds. Positive tests call this; negative-control tests call
/// `gaplessConcatViolations` directly and assert a non-empty set.
func assertGaplessConcat(
    inFolder folder: URL, frameRate: Double,
    file: StaticString = #filePath, line: UInt = #line
) async throws {
    let segments = try sessionSegments(inFolder: folder)
    let violations = try await gaplessConcatViolations(segments: segments, frameRate: frameRate)
    for violation in violations {
        XCTFail("NFR-22: \(violation)", file: file, line: line)
    }
}

/// The Epic-20 screen↔camera alignment core (FR-70, Story 20.1): for every
/// screen/camera segment pair sharing a manifest `index`, the first-frame
/// host-clock PTS must agree within one frame period. Pure — returns the
/// violations (misaligned pairs as `gap` carrying the measured skew,
/// unverifiable bounds as `missingBounds`, an index with no camera
/// counterpart as `missingBounds` too) so the negative controls can assert a
/// non-empty set without fighting `XCTFail` (the `gaplessConcatViolations`
/// split, mirrored).
func screenCameraAlignmentViolations(
    screen: [SessionSegment], camera: [SessionSegment], frameRate: Double,
    epsilon: Double = 1e-6
) -> [ConcatViolation] {
    precondition(frameRate > 0, "frameRate must be positive, got \(frameRate)")
    let period = 1.0 / frameRate
    // Tolerant of a duplicate manifest index (keep the first) rather than
    // trapping — a malformed camera manifest must surface as a gate
    // violation, never a fatalError in the harness.
    let cameraByIndex = Dictionary(
        camera.map { ($0.index, $0) }, uniquingKeysWith: { first, _ in first })
    var violations: [ConcatViolation] = []
    for screenSegment in screen {
        guard let cameraSegment = cameraByIndex[screenSegment.index],
            let screenStart = screenSegment.ptsStart,
            let cameraStart = cameraSegment.ptsStart
        else {
            violations.append(
                ConcatViolation(
                    boundary: screenSegment.index, kind: .missingBounds, seconds: 0))
            continue
        }
        let skew = cameraStart - screenStart
        // `+ epsilon` absorbs float noise only, mirroring the concat gate's
        // boundary tolerance — a skew within one frame period is aligned.
        if abs(skew) > period + epsilon {
            violations.append(
                ConcatViolation(boundary: screenSegment.index, kind: .gap, seconds: skew))
        }
    }
    return violations
}

/// The thin XCTest-facing alignment wrapper: runs the pure core and
/// `XCTFail`s once per violation naming the segment index and the measured
/// skew. Positive tests call this; negative controls call
/// `screenCameraAlignmentViolations` directly.
@discardableResult
func assertScreenCameraAlignedWithinOneFrame(
    screen: [SessionSegment], camera: [SessionSegment], frameRate: Double,
    file: StaticString = #filePath, line: UInt = #line
) -> [ConcatViolation] {
    let violations = screenCameraAlignmentViolations(
        screen: screen, camera: camera, frameRate: frameRate)
    for violation in violations {
        XCTFail("FR-70 screen↔camera alignment: \(violation)", file: file, line: line)
    }
    return violations
}
