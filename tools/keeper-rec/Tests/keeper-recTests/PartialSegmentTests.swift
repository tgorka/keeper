// SPDX-License-Identifier: Apache-2.0
//
// `.partial` while writing, final on close (Story 41.3, FR-133, AD-69).
//
// The sidecar writes every segment as `<name>.<ext>.partial` and renames it
// onto `<name>.<ext>` the instant `finishWriting` returns, so a growing
// segment is invisible to sync (`keeper-sync` excludes `*.partial` at tier 0)
// and a closed one is an ordinary file the moment it is closed. Two properties
// carry the whole design and are proven here:
//
//   1. The publish is a RENAME, not a copy. A copy of a multi-gigabyte segment
//      is both slow and a second chance to be interrupted — the very failure
//      the suffix exists to prevent — so the tests assert inode identity
//      across the publish, which no copy can produce.
//   2. A writer that never finishes leaves exactly one `.partial` and no final
//      name. That is what a SIGKILL leaves, and what startup recovery is
//      handed.
//
// AVAssetWriter only (mirroring the Story 17.4 fixtures): no ScreenCaptureKit,
// no capture hardware, no code-signing, so this suite runs anywhere macOS
// Swift does.

import AVFoundation
import Foundation
import XCTest

@testable import keeper_rec

final class PartialSegmentTests: XCTestCase {
    /// The `st_ino` of a path — the identity POSIX `rename(2)` preserves and a
    /// copy cannot.
    private func fileID(_ path: String) throws -> UInt64 {
        let attributes = try FileManager.default.attributesOfItem(atPath: path)
        let number = try XCTUnwrap(
            attributes[.systemFileNumber] as? NSNumber, "no file number for \(path)")
        return number.uint64Value
    }

    private func entries(of folder: URL) throws -> [String] {
        try FileManager.default.contentsOfDirectory(atPath: folder.path).sorted()
    }

    /// The suffix goes on the end of the whole filename, so the final name is
    /// recovered by removing it — nothing downstream has to guess what the
    /// media extension was, and a `.mov` and an `.m4a` are treated alike.
    func testThePartialPathIsTheFinalPathPlusTheSuffix() {
        for final in [
            "/tmp/keeper-rec 2026-08-07/screen-0003.mov",
            "/tmp/keeper-rec 2026-08-07/camera-0003.mov",
            "/tmp/keeper-rec 2026-08-07/audio-0000.m4a",
        ] {
            let partial = partialSegmentPath(for: final)
            XCTAssertEqual(partial, final + ".partial")
            XCTAssertNotEqual(partial, final, "the written name is never the announced name")
            XCTAssertEqual(
                String(partial.dropLast(partialSegmentSuffix.count)), final,
                "stripping the suffix must recover the final name exactly")
        }
    }

    /// The publish moves the bytes without touching them: same inode, same
    /// length, same content, and the temporary name gone. Inode identity is
    /// the assertion that a copy could never satisfy.
    func testPublishingAFinishedSegmentIsAnAtomicRenameNotACopy() async throws {
        let dir = try TempSessionDir(label: "publish")
        let final = dir.url.appendingPathComponent("screen-0000.mov").path
        let partial = partialSegmentPath(for: final)
        try await writeFixtureSegment(
            at: URL(fileURLWithPath: partial), firstPTS: 1000, frames: 12, frameRate: 30)

        let before = try Data(contentsOf: URL(fileURLWithPath: partial))
        let inode = try fileID(partial)

        try publishSegment(from: partial, to: final)

        XCTAssertEqual(
            try entries(of: dir.url), ["screen-0000.mov"],
            "the published segment is the only file left — no temporary, no copy beside it")
        XCTAssertEqual(
            try fileID(final), inode,
            "the final name must be the SAME file, not a copy of it")
        XCTAssertEqual(
            try Data(contentsOf: URL(fileURLWithPath: final)), before,
            "publishing must not rewrite a single byte of the media")

        // And it is still a playable movie, not merely the right bytes.
        let asset = AVURLAsset(url: URL(fileURLWithPath: final))
        let tracks = try await asset.loadTracks(withMediaType: .video)
        XCTAssertEqual(tracks.count, 1, "the published segment still reads as a movie")
    }

    /// A recorder killed mid-segment leaves the in-progress file under its
    /// temporary name and nothing under the final one — so sync sees no
    /// segment at all rather than a torn one, and recovery has exactly one
    /// file to resolve.
    func testAKilledWriterLeavesExactlyOnePartialAndNoFinalName() async throws {
        let dir = try TempSessionDir(label: "killed")
        let final = dir.url.appendingPathComponent("screen-0007.mov").path
        let writer = try await abandonedSegmentWriter(atFinalPath: final, frames: 12)

        XCTAssertEqual(
            try entries(of: dir.url), ["screen-0007.mov.partial"],
            "a killed writer leaves exactly one in-progress file and no final name")
        XCTAssertFalse(
            FileManager.default.fileExists(atPath: final),
            "nothing may exist under the final name until the rename lands")

        // Cleanup only, and last: `cancelWriting` deletes the output file, so
        // it can never be the "stop" the assertions above observe.
        writer.cancelWriting()
    }

    /// Capture never degrades. A rename that cannot land is reported as a
    /// typed error and leaves the bytes exactly where they are — the segment
    /// is recoverable at next start rather than lost to a failed publish.
    func testARenameThatCannotLandIsTypedAndLeavesTheBytesOnDisk() async throws {
        try XCTSkipIf(
            getuid() == 0,
            "root ignores directory permissions, so the rename cannot be made to fail")
        let dir = try TempSessionDir(label: "unwritable")
        let final = dir.url.appendingPathComponent("screen-0001.mov").path
        let partial = partialSegmentPath(for: final)
        try await writeFixtureSegment(
            at: URL(fileURLWithPath: partial), firstPTS: 1000, frames: 12, frameRate: 30)
        let before = try Data(contentsOf: URL(fileURLWithPath: partial))

        // A directory that cannot be written is what `rename(2)` needs to
        // fail: the entry it would create lives in the directory, not in the
        // file.
        try FileManager.default.setAttributes(
            [.posixPermissions: 0o500], ofItemAtPath: dir.url.path)
        defer {
            try? FileManager.default.setAttributes(
                [.posixPermissions: 0o700], ofItemAtPath: dir.url.path)
        }

        XCTAssertThrowsError(try publishSegment(from: partial, to: final)) { error in
            guard let failure = error as? SegmentPublishError else {
                return XCTFail("expected a SegmentPublishError, got \(error)")
            }
            XCTAssertNotEqual(failure.code, 0, "the errno the rename failed with is carried")
            XCTAssertTrue(
                failure.description.contains("screen-0001.mov"),
                "the message names the segment: \(failure.description)")
            XCTAssertFalse(
                failure.description.contains(dir.url.path),
                "…and only the basename, never the directory: \(failure.description)")
        }

        XCTAssertEqual(
            try Data(contentsOf: URL(fileURLWithPath: partial)), before,
            "the bytes stay on disk under the temporary name")
        XCTAssertFalse(
            FileManager.default.fileExists(atPath: final),
            "a failed publish must not leave a half-named file behind")
    }
}

/// Open a writer exactly as `Capture.swift` does — at the `.partial` path,
/// H.264 64×64, ~4 s `movieFragmentInterval` — append `frames` frames and
/// ABANDON it: no `markAsFinished`, no `finishWriting`. That is the shape a
/// SIGKILL leaves; a clean finalize would consolidate the fragments away and
/// model nothing.
///
/// Returns the still-live writer so the caller can assert the on-disk state
/// FIRST and cancel afterwards as cleanup — `cancelWriting` deletes the
/// abandoned output file, so it can never be the pre-assertion "stop".
private func abandonedSegmentWriter(atFinalPath finalPath: String, frames: Int) async throws
    -> AVAssetWriter
{
    let url = URL(fileURLWithPath: partialSegmentPath(for: finalPath))
    let writer = try AVAssetWriter(outputURL: url, fileType: .mov)
    writer.movieFragmentInterval = CMTime(seconds: 4, preferredTimescale: 600)
    let input = AVAssetWriterInput(
        mediaType: .video,
        outputSettings: [
            AVVideoCodecKey: AVVideoCodecType.h264,
            AVVideoWidthKey: 64,
            AVVideoHeightKey: 64,
            AVVideoCompressionPropertiesKey: [
                AVVideoAverageBitRateKey: 200_000,
                AVVideoExpectedSourceFrameRateKey: 30,
                AVVideoAllowFrameReorderingKey: false,
            ],
        ])
    input.expectsMediaDataInRealTime = false
    let adaptor = AVAssetWriterInputPixelBufferAdaptor(
        assetWriterInput: input,
        sourcePixelBufferAttributes: [
            kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
            kCVPixelBufferWidthKey as String: 64,
            kCVPixelBufferHeightKey as String: 64,
        ])
    guard writer.canAdd(input) else {
        throw FixtureError("could not add the abandoned writer's video track")
    }
    writer.add(input)
    guard writer.startWriting() else {
        throw FixtureError(
            "abandoned writer could not start: \(writer.error.map(String.init(describing:)) ?? "unknown")"
        )
    }
    writer.startSession(atSourceTime: CMTime(value: 0, timescale: 600))
    for frame in 0..<frames {
        guard let pool = adaptor.pixelBufferPool else {
            throw FixtureError("abandoned writer's pixel-buffer pool unavailable")
        }
        var pixelBuffer: CVPixelBuffer?
        guard
            CVPixelBufferPoolCreatePixelBuffer(nil, pool, &pixelBuffer) == kCVReturnSuccess,
            let buffer = pixelBuffer
        else {
            throw FixtureError("could not create a pixel buffer")
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
        guard
            adaptor.append(
                buffer,
                withPresentationTime: CMTime(value: CMTimeValue(frame * 20), timescale: 600))
        else {
            throw FixtureError(
                "append failed: \(writer.error.map(String.init(describing:)) ?? "unknown")")
        }
    }
    // The output file exists from `startWriting`; wait for it rather than
    // assume, so the caller's directory assertion cannot race the muxer.
    for _ in 0..<200 where !FileManager.default.fileExists(atPath: url.path) {
        try await Task.sleep(nanoseconds: 10_000_000)
    }
    return writer
}
