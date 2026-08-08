// SPDX-License-Identifier: Apache-2.0
//
// Pure rotation-trigger policy + segment-path derivation (Story 17.1, Epic 17,
// AD-37).
//
// Foundation-only on purpose: no AVFoundation / ScreenCaptureKit import, so
// this logic is unit-testable (Tests/keeper-recTests/RotationTests.swift)
// without capture hardware or code-signing. `CaptureEngine` (Capture.swift)
// consults `RotationPolicy` at each complete video frame and performs the
// actual dual-writer handover.

import Foundation

/// Decides when the current segment should be cut and handed over to a fresh
/// writer (Story 17.1).
///
/// The decision inputs are deliberately *observed* facts, not intentions:
/// `observedBytes` is the segment's **on-disk** size (fMP4 buffering makes any
/// in-memory appended-byte tally run ahead of what a crash would actually
/// preserve — the budget must bound what survives), `elapsedSeconds` is time
/// since the segment's anchor frame, and the keyframe / first-frame flags gate
/// *where* a cut may land.
struct RotationPolicy: Equatable {
    /// Rotate once the segment's observed on-disk size reaches this many bytes.
    let byteBudget: UInt64
    /// Rotate once the segment has run this long even while far below the byte
    /// budget — the low-motion fallback, so idle screens still rotate.
    let durationCapSeconds: Double

    /// Default segment size in MB when `startRecording` omits `segmentMB`.
    static let defaultSegmentMB = 500
    /// Default duration cap in seconds when `maxSegmentSeconds` is omitted
    /// (30 minutes).
    static let defaultMaxSegmentSeconds = 1800
    /// The `segmentMB` → bytes factor: decimal MB (10^6). The exact factor and
    /// the defaults above are authored assumptions (epic context), adjustable
    /// on dogfooding evidence without a spec change.
    static let bytesPerMB: UInt64 = 1_000_000
    /// The largest accepted segment size in MB (1 TB). Caps `segmentMB` before
    /// the `segmentMB * bytesPerMB` product so an absurd host value can never
    /// overflow `UInt64` and trap — the sidecar must always exit cleanly, never
    /// crash.
    static let maxSegmentMB = 1_000_000

    /// Build a policy from the wire-level `segmentMB` / `maxSegmentSeconds`
    /// values. `segmentMB` is clamped to `1...maxSegmentMB`: a zero/negative
    /// budget would degenerate into rotating on every keyframe, and an absurdly
    /// large one would overflow the byte-budget product. `maxSegmentSeconds` is
    /// floored at 1 for the same degenerate-cap reason.
    init(segmentMB: Int, maxSegmentSeconds: Int) {
        let clampedMB = min(max(1, segmentMB), Self.maxSegmentMB)
        self.byteBudget = UInt64(clampedMB) * Self.bytesPerMB
        self.durationCapSeconds = Double(max(1, maxSegmentSeconds))
    }

    /// Decide rotate (`true`) vs continue (`false`) for the current video frame.
    ///
    /// Never rotates off a non-keyframe (each segment must start
    /// self-decodable) and never rotates a just-opened segment
    /// (`isFirstFrameOfSegment`), even when a tiny budget or cap is already
    /// exceeded — otherwise a pathological configuration could produce
    /// zero-frame segments forever.
    func shouldRotate(
        observedBytes: UInt64, elapsedSeconds: Double, isKeyframe: Bool,
        isFirstFrameOfSegment: Bool
    ) -> Bool {
        guard isKeyframe, !isFirstFrameOfSegment else { return false }
        return observedBytes >= byteBudget || elapsedSeconds >= durationCapSeconds
    }
}

/// Derive segment N+1's path from segment N's (Story 17.1).
///
/// The trailing decimal run of the filename's stem is incremented with its
/// zero-padding width preserved (`…/screen-0000.mov` → `…/screen-0001.mov`,
/// `…/screen-9999.mov` → `…/screen-10000.mov` — the width grows rather than
/// wrap). A stem with no trailing numeric run (or one too long to represent)
/// gets `-0001` inserted before the extension (`…/screen.mov` →
/// `…/screen-0001.mov`). POSIX paths only (macOS); pure string logic, the
/// filesystem is never touched.
func nextSegmentPath(from path: String) -> String {
    // Split directory / filename on the last "/" (directory keeps the slash).
    let directory: String
    let filename: String
    if let slash = path.lastIndex(of: "/") {
        directory = String(path[...slash])
        filename = String(path[path.index(after: slash)...])
    } else {
        directory = ""
        filename = path
    }

    // Split the extension on the last "." — a leading dot is a hidden file's
    // name, not an extension.
    let stem: String
    let ext: String
    if let dot = filename.lastIndex(of: "."), dot != filename.startIndex {
        stem = String(filename[..<dot])
        ext = String(filename[dot...])
    } else {
        stem = filename
        ext = ""
    }

    // Find the stem's trailing ASCII-decimal run.
    var runStart = stem.endIndex
    while runStart > stem.startIndex {
        let previous = stem.index(before: runStart)
        guard stem[previous].isASCII, stem[previous].isNumber else { break }
        runStart = previous
    }
    let run = stem[runStart...]

    if !run.isEmpty, let value = UInt64(run), value < UInt64.max {
        let next = String(value + 1)
        let padded =
            next.count >= run.count
            ? next
            : String(repeating: "0", count: run.count - next.count) + next
        return directory + stem[..<runStart] + padded + ext
    }
    return directory + stem + "-0001" + ext
}

/// The suffix a segment carries for the whole of the time it is being written
/// (Story 41.3, FR-133, AD-69).
///
/// A segment is a half-written file until `finishWriting` returns, and a
/// half-written file committed once is a corrupt file forever. Rather than
/// teach the sync engine what a recording is, the writer marks the file
/// itself: `keeper-sync`'s tier-0 name corpus already excludes `*.partial`, so
/// an in-progress segment is invisible to every part of sync — the pending
/// list, the commit path, the activity feed — wherever the destination
/// happens to resolve. Nothing else marks it: no lock file, no state file,
/// nothing that can fall out of step with the bytes.
let partialSegmentSuffix = ".partial"

/// The path a segment is WRITTEN to, given the path it will carry once closed
/// (`…/screen-0003.mov` → `…/screen-0003.mov.partial`).
///
/// The suffix goes on the END of the whole filename rather than replacing the
/// extension, so the final name is recovered by removing it and nothing has to
/// guess what the media extension was.
func partialSegmentPath(for finalPath: String) -> String {
    finalPath + partialSegmentSuffix
}

/// A segment whose bytes are complete but which could not be published under
/// its final name (Story 41.3).
///
/// Carries the `errno` the rename failed with. The bytes are still on disk
/// under the temporary name — capture never degrades, and startup recovery
/// finalises or discards what is left.
struct SegmentPublishError: Error, CustomStringConvertible {
    /// The stable wire code for the non-fatal `{"event":"warning",…}` this
    /// raises (Story 41.3). Non-fatal by contract: the capture is unharmed and
    /// keeps rolling — only the segment's *publication* failed.
    static let warningCode = "segmentUnpublished"

    /// The `errno` `rename(2)` set.
    let code: Int32
    /// The final basename that could not be taken (never the directory).
    let name: String

    var description: String {
        "could not publish \(name): \(String(cString: strerror(code)))"
    }

    /// The honest warning line: nothing was lost, the bytes are simply still
    /// under their temporary name, and startup recovery finalises or discards
    /// them on the next run.
    var warningMessage: String {
        "\(description) — the segment is on disk under its temporary name and is "
            + "resolved at next start; recording continues"
    }
}

/// Publish a finished segment: move `<name>.<ext>.partial` onto
/// `<name>.<ext>` (Story 41.3, FR-133).
///
/// **`rename(2)`, deliberately, and not `FileManager`.** The two paths differ
/// only in the suffix, so they are always in the same directory on the same
/// filesystem, where POSIX `rename` is atomic and inode-preserving: no reader
/// can observe a partly-published file, and a multi-gigabyte segment is
/// published in constant time. `FileManager.moveItem` promises neither, and a
/// copy fallback would be both slow and a second chance to be interrupted —
/// which is the very failure this whole suffix exists to prevent. There is
/// therefore no fallback at all: a failure is reported and the bytes stay put.
func publishSegment(from partialPath: String, to finalPath: String) throws {
    var failure: Int32 = 0
    let published = partialPath.withCString { source in
        finalPath.withCString { destination in
            if rename(source, destination) == 0 { return true }
            failure = errno
            return false
        }
    }
    guard published else {
        throw SegmentPublishError(
            code: failure, name: (finalPath as NSString).lastPathComponent)
    }
}
