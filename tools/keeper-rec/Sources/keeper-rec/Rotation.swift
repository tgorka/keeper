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
