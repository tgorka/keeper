// SPDX-License-Identifier: Apache-2.0
//
// The Story 17.3 induced-kill assurance test (AC2, FR-73): a fragmented MP4
// written with Capture.swift's writer configuration (~4 s
// `movieFragmentInterval`) stays playable up to its last complete flushed
// fragment even when `finishWriting` NEVER runs — the crash-safety property
// keeper's startup recovery relies on (recovered segments play as-is, no
// remux). Mirrors 17.4's approach: fixtures generated on the runner via
// AVAssetWriter — no ScreenCaptureKit, no signing, no committed media.
//
// The crash shape is modeled by ABANDONING the writer: a clean
// `finishWriting` consolidates the fragments away, so it cannot model a
// crash. The snapshot bytes are copied only once the file is SETTLED — no
// appends pending, the on-disk box structure quiescent across polls, and the
// copy verified byte-identical against a re-read — so the copy never races an
// in-flight fragment flush. (`cancelWriting()` cannot run first: on current
// macOS it DELETES the abandoned output file, so the settle-verify-copy
// protocol is the "otherwise stop the writer" leg; the writer is cancelled
// afterwards purely as cleanup.) The snapshot is then truncated mid-fragment
// — asserted to actually drop bytes (`cut < fileSize`), so the "fewer frames
// than the clean control" assertion can never false-green on a no-op cut.
//
// On-disk shape of an abandoned fragmented .mov (observed, load-bearing for
// the cut): `ftyp`, then the FIRST ~4 s fragment as `mdat`+`moov`, then each
// subsequent complete fragment as `mdat`+`moof`, then a growing tail `mdat`
// holding samples no `moof` indexes yet — the un-flushed tail a crash drops.

import AVFoundation
import CoreMedia
import CoreVideo
import Foundation
import XCTest

/// One top-level ISO-BMFF box in a byte buffer: its file offset, its 4CC
/// type, and its total size in bytes.
private struct TopLevelBox: Equatable {
    let offset: Int
    let type: String
    let size: Int
}

/// Walk the buffer's TOP-LEVEL box structure (offset/type/size triples).
/// Standard ISO-BMFF framing: 32-bit big-endian size + 4CC, `size == 1` ⇒
/// 64-bit largesize follows, `size == 0` ⇒ the box runs to EOF. A torn tail
/// (fewer than 8 readable header bytes, or a size overrunning the buffer)
/// ends the walk — exactly the shape a truncated file presents.
private func topLevelBoxes(in data: Data) -> [TopLevelBox] {
    var boxes: [TopLevelBox] = []
    var offset = 0
    while offset + 8 <= data.count {
        let size32 = data.subdata(in: offset..<offset + 4).reduce(0) { ($0 << 8) | Int($1) }
        guard let type = String(data: data.subdata(in: offset + 4..<offset + 8), encoding: .ascii)
        else { break }
        var size = size32
        if size32 == 0 {
            size = data.count - offset
        } else if size32 == 1 {
            guard offset + 16 <= data.count else { break }
            size = data.subdata(in: offset + 8..<offset + 16).reduce(0) { ($0 << 8) | Int($1) }
        }
        guard size >= 8 || size32 == 0, offset + size <= data.count else { break }
        boxes.append(TopLevelBox(offset: offset, type: type, size: size))
        offset += size
    }
    return boxes
}

/// Write a fragmented `.mov` the way Capture.swift's real writers do (H.264,
/// 64×64, ~4 s `movieFragmentInterval`, host-clock `startSession` anchor) and
/// ABANDON it: no `markAsFinished`, no `finishWriting` — the on-disk bytes
/// are whatever fragments the writer flushed before the "crash", exactly the
/// shape a killed recorder leaves.
///
/// The snapshot protocol (a copy must never race an in-flight flush): after
/// the last append, poll — under a generous, non-flaky budget; flushes land
/// in milliseconds on any sane runner, the bound only guards a wedged writer
/// — until at least one complete `moof` fragment is on disk AND the file is
/// quiescent (size and box map identical across consecutive polls), then read
/// the bytes twice and require the reads identical (a settled snapshot,
/// proven, not assumed). Only then is the writer cancelled — cleanup only:
/// `cancelWriting` deletes the abandoned file, which is why it cannot be the
/// pre-copy "stop". Returns the settled crash-shape bytes.
private func abandonedFragmentedSegmentBytes(
    at url: URL, frames: Int, frameRate: Double
) async throws -> Data {
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
                // so the strict-monotonic check below stays meaningful.
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
        throw FixtureError("could not add the crash-fixture H.264 video track")
    }
    writer.add(input)
    guard writer.startWriting() else {
        throw FixtureError(
            "crash-fixture writer could not start: \(writer.error.map(String.init(describing:)) ?? "unknown")"
        )
    }
    let period = 1.0 / frameRate
    let firstPTS = 1000.0
    writer.startSession(
        atSourceTime: CMTime(value: CMTimeValue((firstPTS * 600).rounded()), timescale: 600))

    for frame in 0..<frames {
        guard let pool = adaptor.pixelBufferPool else {
            throw FixtureError("crash-fixture pixel-buffer pool unavailable")
        }
        var pixelBuffer: CVPixelBuffer?
        guard
            CVPixelBufferPoolCreatePixelBuffer(nil, pool, &pixelBuffer) == kCVReturnSuccess,
            let buffer = pixelBuffer
        else {
            throw FixtureError("could not create a crash-fixture pixel buffer")
        }
        CVPixelBufferLockBaseAddress(buffer, [])
        if let base = CVPixelBufferGetBaseAddress(buffer) {
            memset(
                base, Int32(frame % 251),
                CVPixelBufferGetBytesPerRow(buffer) * CVPixelBufferGetHeight(buffer))
        }
        CVPixelBufferUnlockBaseAddress(buffer, [])

        while !input.isReadyForMoreMediaData {
            try await Task.sleep(nanoseconds: 1_000_000)
        }
        let pts = CMTime(
            value: CMTimeValue(((firstPTS + Double(frame) * period) * 600).rounded()),
            timescale: 600)
        guard adaptor.append(buffer, withPresentationTime: pts) else {
            throw FixtureError(
                "crash-fixture frame append failed: \(writer.error.map(String.init(describing:)) ?? "unknown")"
            )
        }
    }

    // Deliberately NO `markAsFinished`/`finishWriting` — that clean finalize
    // would consolidate the fragments away and stop modeling a crash. Settle:
    // wait until a complete `moof` fragment exists and the file is quiescent
    // across two consecutive polls.
    let flushDeadline = Date().addingTimeInterval(60)
    var previous: (size: Int, boxes: [TopLevelBox])? = nil
    var settled = false
    while Date() < flushDeadline {
        try await Task.sleep(nanoseconds: 250_000_000)
        guard let bytes = try? Data(contentsOf: url) else { continue }
        let boxes = topLevelBoxes(in: bytes)
        let current = (size: bytes.count, boxes: boxes)
        if boxes.contains(where: { $0.type == "moof" }), let previous,
            previous.size == current.size, previous.boxes == current.boxes
        {
            settled = true
            break
        }
        previous = current
    }
    guard settled else {
        throw FixtureError(
            "crash fixture never settled with a flushed moof fragment within the flush budget")
    }

    // The settled snapshot, PROVEN stable: two reads must be byte-identical
    // (never a copy racing an in-flight flush).
    guard let snapshot = try? Data(contentsOf: url), !snapshot.isEmpty else {
        throw FixtureError("could not read the settled crash-fixture bytes")
    }
    try await Task.sleep(nanoseconds: 250_000_000)
    guard let reread = try? Data(contentsOf: url), reread == snapshot else {
        throw FixtureError("crash-fixture bytes changed under the snapshot — not settled")
    }

    // Cleanup only, AFTER the snapshot is secured: cancelling an abandoned
    // writer deletes its output file.
    writer.cancelWriting()
    return snapshot
}

final class RecoveryTests: XCTestCase {
    /// The fixture frame rate (the capture engine's 30 fps default).
    private let frameRate = 30.0

    /// Row: force-killed fragmented segment (AC2, FR-73). A writer abandoned
    /// mid-session (never finalized) and then truncated mid-fragment — the
    /// last complete fragment torn and everything past it dropped, as a real
    /// crash would — still opens via `AVAsset` and decodes strictly-monotonic
    /// video frames spanning up to the last complete ~4 s fragment, yet
    /// strictly fewer than the clean `writeFixtureSegment` control. No remux
    /// anywhere: recovery plays these bytes as-is.
    func testTruncatedAbandonedFragmentedSegmentDecodesUpToLastCompleteFragment() async throws {
        let dir = try TempSessionDir(label: "recovery-kill")
        // ~9 s of media at 30 fps: the first ~4 s fragment (mdat+moov), a
        // second complete ~4 s fragment (mdat+moof), and a >1 s tail past the
        // last fragment boundary, guaranteeing un-flushed bytes past the last
        // `moof` for the cut to drop.
        let frames = 270

        // The crash-shape snapshot: abandoned (never finalized), settled and
        // proven stable before the copy.
        let crashURL = dir.url.appendingPathComponent("screen-crash.mov")
        let snapshot = try await abandonedFragmentedSegmentBytes(
            at: crashURL, frames: frames, frameRate: frameRate)

        // The clean positive control: the SAME content, cleanly finalized —
        // the frame-count ceiling the truncated file must stay under.
        let controlURL = dir.url.appendingPathComponent("screen-control.mov")
        try await writeFixtureSegment(
            at: controlURL, firstPTS: 1000.0, frames: frames, frameRate: frameRate)
        let controlPTS = try await SegmentTimeline.videoPTS(of: controlURL, segmentIndex: 0)
        XCTAssertEqual(
            controlPTS.count, frames,
            "the clean control must decode every appended frame")

        // Truncate MID-FRAGMENT: cut halfway into the last complete
        // fragment's `moof`, tearing that fragment exactly as a mid-write
        // power cut would — the survivors are the fragments before it. The
        // cut MUST actually drop bytes (and the snapshot must carry a tail
        // past the last `moof`), otherwise the fewer-frames assertion below
        // could false-green on a no-op truncation.
        let boxes = topLevelBoxes(in: snapshot)
        guard let lastMoof = boxes.last(where: { $0.type == "moof" }) else {
            throw FixtureError("crash snapshot holds no moof fragment to truncate into")
        }
        guard snapshot.count > lastMoof.offset + lastMoof.size else {
            throw FixtureError("crash snapshot has no un-flushed tail past the last moof")
        }
        let cut = lastMoof.offset + lastMoof.size / 2
        XCTAssertLessThan(
            cut, snapshot.count,
            "the truncation must drop real bytes — a no-op cut voids the test")
        XCTAssertGreaterThan(cut, 0, "the cut must leave the leading fragments intact")

        let truncatedURL = dir.url.appendingPathComponent("screen-truncated.mov")
        try snapshot.prefix(cut).write(to: truncatedURL)

        // The truncated crash file still opens and decodes: strictly-monotonic
        // frames (the same reader idiom as the NFR-22 gate), a prefix of the
        // clean control's timeline, spanning up to the last complete fragment
        // — yet strictly fewer frames than the control (the torn fragment and
        // the never-flushed tail are honestly gone).
        let truncatedPTS = try await SegmentTimeline.videoPTS(of: truncatedURL, segmentIndex: 0)
        XCTAssertFalse(truncatedPTS.isEmpty, "the surviving fragments must decode")
        XCTAssertLessThan(
            truncatedPTS.count, controlPTS.count,
            "the truncated file must decode FEWER frames than the clean control")
        for (index, pair) in zip(truncatedPTS.dropFirst(), truncatedPTS).enumerated() {
            XCTAssertGreaterThan(
                pair.0, pair.1,
                "video PTS must be strictly monotonic at sample \(index + 1)")
        }
        for (index, pair) in zip(truncatedPTS, controlPTS).enumerated() {
            XCTAssertEqual(
                pair.0, pair.1, accuracy: 1e-9,
                "the truncated timeline must be a prefix of the control's (sample \(index))")
        }
        // "Up to the last complete ~4 s fragment": with the second fragment
        // torn, the survivor is the first ~4 s — the decoded span must reach
        // a full fragment interval (small tolerance for the muxer landing the
        // fragment boundary a frame or two around the interval), never
        // collapse to a token frame or two.
        let period = 1.0 / frameRate
        let decodedSpan = (truncatedPTS.last ?? 0) - (truncatedPTS.first ?? 0)
        XCTAssertGreaterThanOrEqual(
            decodedSpan, 4.0 - 3 * period,
            "the surviving fragments must span up to the last complete ~4 s fragment")
    }
}
