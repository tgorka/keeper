// SPDX-License-Identifier: Apache-2.0
//
// AVAssetWriter-based fixture sessions for the NFR-22 gapless-concat gate
// (Story 17.4, AD-38): real fMP4 `screen-####.mov` segments + a
// `manifest.json` shape-compatible with the Rust `SessionManifest`, generated
// on the runner — no ScreenCaptureKit, no display, no signing, no committed
// binary media.
//
// The writers mirror Capture.swift's real handover: each segment is written
// with `startSession(atSourceTime:)` at its host-clock-anchored first PTS, so
// the muxer rebases every file's own timeline to 0 — EXACTLY why the boundary
// check reads the manifest bounds, not the files (the file PTS feed only the
// intra-file monotonicity check). Frame reordering is disabled so passthrough
// sample order == presentation order (no B-frames; the strict-monotonic
// intra-file check stays meaningful). `.mpeg4CMAFCompliant` is deliberately
// NOT set: on a file-URL writer that profile belongs to the segment-delegate
// API; Capture.swift's real writers use `movieFragmentInterval` alone, and the
// fixtures mirror the shipped configuration.

import AVFoundation
import CoreMedia
import CoreVideo
import Foundation

/// A fixture-generation fault — thrown, so it surfaces as a test error.
struct FixtureError: Error, CustomStringConvertible {
    let message: String
    init(_ message: String) { self.message = message }
    var description: String { message }
}

/// RAII temp dir: a unique folder under the system temp dir, removed on
/// deinit. Keep the instance alive for the duration of the test.
final class TempSessionDir {
    let url: URL

    init(label: String) throws {
        url = FileManager.default.temporaryDirectory
            .appendingPathComponent("keeper-rec-concat-\(label)-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    }

    deinit {
        try? FileManager.default.removeItem(at: url)
    }
}

/// One fixture segment as planned and as written into the manifest: the
/// manifest `index`, file basename, host-clock PTS bounds (`nil` bounds are
/// written as JSON `null`, mirroring a real capture's final segment /
/// older-sidecar entries), and — since Story 20.1 — the `track` it belongs
/// to (`"screen"` by default; `"camera"` for the dual-track fixtures).
struct FixtureSegment {
    let index: Int
    let file: String
    let ptsStart: Double?
    let ptsEnd: Double?
    var track: String = "screen"
}

/// Which discontinuity to inject at one boundary (the negative controls).
enum BoundaryDefect {
    /// `ptsStart(k+1) = ptsEnd(k) + P` — the faithful gapless handover.
    case none
    /// `ptsStart(k+1) = ptsEnd(k) + 4·P` — a dropped-frames gap.
    case gap
    /// `ptsStart(k+1) = ptsEnd(k) − P` — a rewound/duplicated cut.
    case overlap
}

/// Write one fMP4 segment: H.264, 64×64, ~4 s fragments, frames at
/// `firstPTS + i·P` on the host-clock timeline (which the muxer then rebases
/// to 0 in the file, like the real capture engine).
func writeFixtureSegment(
    at url: URL, firstPTS: Double, frames: Int, frameRate: Double
) async throws {
    let width = 64
    let height = 64
    let writer = try AVAssetWriter(outputURL: url, fileType: .mov)
    writer.movieFragmentInterval = CMTime(seconds: 4, preferredTimescale: 600)

    let input = AVAssetWriterInput(
        mediaType: .video,
        outputSettings: [
            AVVideoCodecKey: AVVideoCodecType.h264,
            AVVideoWidthKey: width,
            AVVideoHeightKey: height,
            AVVideoCompressionPropertiesKey: [
                AVVideoAverageBitRateKey: 200_000,
                AVVideoExpectedSourceFrameRateKey: Int(frameRate),
                // No B-frames: passthrough sample order == presentation order,
                // so the harness's strict-monotonic intra-file check holds for
                // a faithful file.
                AVVideoAllowFrameReorderingKey: false,
            ],
        ])
    input.expectsMediaDataInRealTime = false
    let adaptor = AVAssetWriterInputPixelBufferAdaptor(
        assetWriterInput: input,
        sourcePixelBufferAttributes: [
            kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
            kCVPixelBufferWidthKey as String: width,
            kCVPixelBufferHeightKey as String: height,
        ])
    guard writer.canAdd(input) else {
        throw FixtureError("could not add the fixture H.264 video track")
    }
    writer.add(input)
    guard writer.startWriting() else {
        throw FixtureError(
            "fixture writer could not start: \(writer.error.map(String.init(describing:)) ?? "unknown")"
        )
    }
    // Host-clock anchoring, timescale 600 (P = 1/30 s ↦ exactly 20 units).
    let period = 1.0 / frameRate
    writer.startSession(
        atSourceTime: CMTime(
            value: CMTimeValue((firstPTS * 600).rounded()), timescale: 600))

    for frame in 0..<frames {
        guard let pool = adaptor.pixelBufferPool else {
            throw FixtureError("fixture pixel-buffer pool unavailable")
        }
        var pixelBuffer: CVPixelBuffer?
        guard
            CVPixelBufferPoolCreatePixelBuffer(nil, pool, &pixelBuffer) == kCVReturnSuccess,
            let buffer = pixelBuffer
        else {
            throw FixtureError("could not create a fixture pixel buffer")
        }
        // Deterministic, frame-varying content so the encoder emits real
        // samples (a constant screen could collapse to nothing measurable).
        CVPixelBufferLockBaseAddress(buffer, [])
        if let base = CVPixelBufferGetBaseAddress(buffer) {
            memset(
                base, Int32(frame % 251),
                CVPixelBufferGetBytesPerRow(buffer) * CVPixelBufferGetHeight(buffer))
        }
        CVPixelBufferUnlockBaseAddress(buffer, [])

        // Not real-time: poll readiness (tiny frames drain fast).
        while !input.isReadyForMoreMediaData {
            try await Task.sleep(nanoseconds: 1_000_000)
        }
        let pts = CMTime(
            value: CMTimeValue(((firstPTS + Double(frame) * period) * 600).rounded()),
            timescale: 600)
        guard adaptor.append(buffer, withPresentationTime: pts) else {
            throw FixtureError(
                "fixture frame append failed: \(writer.error.map(String.init(describing:)) ?? "unknown")"
            )
        }
    }

    input.markAsFinished()
    await writer.finishWriting()
    guard writer.status == .completed else {
        throw FixtureError(
            "fixture finalize failed: \(writer.error.map(String.init(describing:)) ?? "unknown")"
        )
    }
}

/// Write `<folder>/manifest.json` shape-compatible with the Rust
/// `SessionManifest` serialization (camelCase; `version`, `session`,
/// `status`, `captureTarget`, `devices`, `segments[]`), with missing PTS
/// bounds as explicit JSON `null` (the Rust side never skips them either).
/// `manifestOrder` controls the ORDER OF THE JSON ARRAY only — discovery must
/// sort by `index` regardless.
func writeFixtureManifest(
    inFolder folder: URL, segments: [FixtureSegment], manifestOrder: [Int]? = nil
) throws {
    let ordered: [FixtureSegment]
    if let manifestOrder {
        ordered = manifestOrder.map { position in segments[position] }
    } else {
        ordered = segments
    }
    let entries: [[String: Any]] = ordered.map { segment in
        let bytes =
            (try? FileManager.default.attributesOfItem(
                atPath: folder.appendingPathComponent(segment.file).path))?[.size] as? UInt64
        return [
            "index": segment.index,
            "file": segment.file,
            "bytes": bytes ?? 0,
            "track": segment.track,
            "ptsStart": segment.ptsStart.map { $0 as Any } ?? NSNull(),
            "ptsEnd": segment.ptsEnd.map { $0 as Any } ?? NSNull(),
        ]
    }
    let hasCamera = segments.contains { $0.track == "camera" }
    let manifest: [String: Any] = [
        "version": 1,
        "session": folder.lastPathComponent,
        "status": "finalized",
        "captureTarget": ["kind": "display", "displayId": NSNull()],
        "devices": ["systemAudio": true, "microphone": false, "camera": hasCamera],
        "segments": entries,
    ]
    let data = try JSONSerialization.data(
        withJSONObject: manifest, options: [.prettyPrinted, .sortedKeys])
    try data.write(to: folder.appendingPathComponent("manifest.json"))
}

/// Generate a whole fixture session: `segmentCount` segments of
/// `framesPerSegment` frames each on one host-clock timeline anchored at
/// `basePTS`, with an optional injected defect at one boundary, optional
/// per-index file naming (for the discovery test), optional manifest-array
/// order, and optionally one segment whose manifest bounds are written null.
/// Returns the planned segments (manifest `index` order).
@discardableResult
func makeFixtureSession(
    in folder: URL,
    segmentCount: Int = 3,
    framesPerSegment: Int = 8,
    frameRate: Double = 30,
    basePTS: Double = 1000.0,
    defect: (boundary: Int, kind: BoundaryDefect)? = nil,
    fileName: (Int) -> String = { String(format: "screen-%04d.mov", $0) },
    manifestOrder: [Int]? = nil,
    nullBoundsForIndex: Int? = nil
) async throws -> [FixtureSegment] {
    let period = 1.0 / frameRate
    var segments: [FixtureSegment] = []
    var start = basePTS
    for index in 0..<segmentCount {
        let end = start + Double(framesPerSegment - 1) * period
        let file = fileName(index)
        try await writeFixtureSegment(
            at: folder.appendingPathComponent(file),
            firstPTS: start, frames: framesPerSegment, frameRate: frameRate)
        let nullBounds = nullBoundsForIndex == index
        segments.append(
            FixtureSegment(
                index: index, file: file,
                ptsStart: nullBounds ? nil : start,
                ptsEnd: nullBounds ? nil : end))
        // The next segment's anchor: gapless is exactly one frame after this
        // segment's last; the negative controls skew this one boundary.
        switch defect?.boundary == index ? (defect?.kind ?? .none) : .none {
        case .none: start = end + period
        case .gap: start = end + 4 * period
        case .overlap: start = end - period
        }
    }
    try writeFixtureManifest(inFolder: folder, segments: segments, manifestOrder: manifestOrder)
    return segments
}

/// A faithful gapless session (`ptsStart(k+1) = ptsEnd(k) + P` at every cut).
@discardableResult
func makeGaplessSession(
    in folder: URL, segments: Int = 3, framesPerSegment: Int = 8, frameRate: Double = 30
) async throws -> [FixtureSegment] {
    try await makeFixtureSession(
        in: folder, segmentCount: segments, framesPerSegment: framesPerSegment,
        frameRate: frameRate)
}

/// Negative control: a 4-frame gap at `boundary` (default: between segments
/// 1 and 2).
@discardableResult
func makeSessionWithGap(
    in folder: URL, atBoundary boundary: Int = 1, segments: Int = 3,
    framesPerSegment: Int = 8, frameRate: Double = 30
) async throws -> [FixtureSegment] {
    try await makeFixtureSession(
        in: folder, segmentCount: segments, framesPerSegment: framesPerSegment,
        frameRate: frameRate, defect: (boundary: boundary, kind: .gap))
}

/// Negative control: a one-frame rewind/overlap at `boundary` (default:
/// between segments 1 and 2).
@discardableResult
func makeSessionWithOverlap(
    in folder: URL, atBoundary boundary: Int = 1, segments: Int = 3,
    framesPerSegment: Int = 8, frameRate: Double = 30
) async throws -> [FixtureSegment] {
    try await makeFixtureSession(
        in: folder, segmentCount: segments, framesPerSegment: framesPerSegment,
        frameRate: frameRate, defect: (boundary: boundary, kind: .overlap))
}

/// Generate a dual-track fixture session (Story 20.1, FR-70): `screen-####` +
/// `camera-####` pairs on ONE host-clock timeline, both tracks cut at the
/// same segment boundaries — the capture engine's screen-master rotation
/// mirrored in fixture form. One manifest lists both tracks disambiguated by
/// `track`, exactly as the Rust `SessionManifest` persists a real dual-track
/// session. Negative-control knobs:
///
/// - `cameraSkewSeconds` at `skewIndex`: that camera segment starts skewed
///   from its screen twin — file AND manifest `ptsStart` both shifted, the
///   pre-fix engine's "report the camera's own first frame" shape (beyond one
///   frame period → an alignment violation);
/// - `cameraWarmupFrames` (> 0): segment 0's camera FILE starts that many
///   frame periods after the shared anchor (the real camera warm-up shape),
///   while its manifest `ptsStart` still reports the ANCHOR — the fixed
///   engine's segment-0 semantics; `ptsEnd` stays the last real frame;
/// - `nullCameraBoundsForIndex`: that camera segment's manifest bounds are
///   written null (unverifiable → `missingBounds`);
/// - `dropCameraIndex`: no camera counterpart is written for that index
///   (a lost camera's tail — `missingBounds` for the unpaired screen entry).
///
/// Returns the planned segments per track (manifest `index` order).
@discardableResult
func makeDualTrackSession(
    in folder: URL,
    segmentCount: Int = 3,
    framesPerSegment: Int = 8,
    frameRate: Double = 30,
    basePTS: Double = 1000.0,
    cameraSkewSeconds: Double = 0,
    skewIndex: Int? = nil,
    cameraWarmupFrames: Int = 0,
    nullCameraBoundsForIndex: Int? = nil,
    dropCameraIndex: Int? = nil
) async throws -> (screen: [FixtureSegment], camera: [FixtureSegment]) {
    let period = 1.0 / frameRate
    var screen: [FixtureSegment] = []
    var camera: [FixtureSegment] = []
    var start = basePTS
    for index in 0..<segmentCount {
        let end = start + Double(framesPerSegment - 1) * period
        let screenFile = String(format: "screen-%04d.mov", index)
        try await writeFixtureSegment(
            at: folder.appendingPathComponent(screenFile),
            firstPTS: start, frames: framesPerSegment, frameRate: frameRate)
        screen.append(
            FixtureSegment(index: index, file: screenFile, ptsStart: start, ptsEnd: end))
        if dropCameraIndex != index {
            let cameraStart = start + (skewIndex == index ? cameraSkewSeconds : 0)
            // Segment-0 warm-up (the fixed-engine shape): the file's first
            // REAL frame lands `cameraWarmupFrames·P` after the anchor, but
            // the manifest still reports the anchor as `ptsStart`.
            let warmupFrames = index == 0 ? min(cameraWarmupFrames, framesPerSegment - 1) : 0
            let cameraFrames = framesPerSegment - warmupFrames
            let cameraFileStart = cameraStart + Double(warmupFrames) * period
            let cameraEnd = cameraFileStart + Double(cameraFrames - 1) * period
            let cameraFile = String(format: "camera-%04d.mov", index)
            try await writeFixtureSegment(
                at: folder.appendingPathComponent(cameraFile),
                firstPTS: cameraFileStart, frames: cameraFrames, frameRate: frameRate)
            let nullBounds = nullCameraBoundsForIndex == index
            camera.append(
                FixtureSegment(
                    index: index, file: cameraFile,
                    ptsStart: nullBounds ? nil : cameraStart,
                    ptsEnd: nullBounds ? nil : cameraEnd,
                    track: "camera"))
        }
        // The next boundary: gapless on the screen-master timeline.
        start = end + period
    }
    try writeFixtureManifest(inFolder: folder, segments: screen + camera)
    return (screen: screen, camera: camera)
}
