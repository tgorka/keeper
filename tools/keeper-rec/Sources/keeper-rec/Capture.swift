// SPDX-License-Identifier: Apache-2.0
//
// The full-screen + system-audio capture engine (Story 16.6 → 17.1, FR-68,
// FR-69, FR-71, AD-37).
//
// One `SCStream` over an `SCContentFilter` for a whole display, with
// `capturesAudio` + `excludeCurrentProcessAudio` (keeper's own notification
// sounds are absent from the recording), feeding a **rotating chain of
// `AVAssetWriter`s** (dual-writer gapless rotation, Story 17.1). Each segment
// is a fragmented MP4 (H.264 video + one AAC system-audio track, ~4 s
// fragments via `movieFragmentInterval`) so a mid-session kill loses at most
// the last fragment; a clean `finishWriting` finalizes each segment's `moov`
// into an ordinary playable `.mp4`.
//
// Rotation (Story 17.1): when the current segment's **observed on-disk** size
// reaches the byte budget — or the duration-cap fallback fires first — the
// engine starts the next writer at the current complete frame's PTS, hands
// that frame and everything after it to the new writer, and finalizes the
// retired writer asynchronously. The `SCStream` itself is never touched: one
// capture source, PTS host-clock-anchored and continuous across the cut, no
// dropped frame or audio. Each closed segment is announced as
// `segmentClosed{index,path,bytes,track:"screen",ptsStart,ptsEnd}` bracketed by
// `state:"rotating"` / `state:"recording"`; the FINAL segment on a clean stop
// is closed by `finalized` instead (a `segmentClosed` while the host is
// Stopping would be an illegal host transition), so its PTS bounds are not
// reported — an accepted Story 17.4 limitation (the NFR-22 CI gate runs on
// generated fixtures that carry complete bounds).
//
// `ptsStart`/`ptsEnd` (Story 17.4, NFR-22, coordinator-authorized 17.1
// amendment) are the retiring segment's first/last appended video sample's
// PTS in **original capture-clock seconds** — recorded HERE, before
// `startSession(atSourceTime:)` rebases the file's own timeline to 0. Once
// muxed, a gapless and a gapped session read back bit-identically from the
// files; the host clock is the only place the boundary truth exists, so the
// manifest persists it and the concat gate asserts against it.
//
// Threading: every SCStream sample callback (both `.screen` and `.audio`) is
// delivered on the single serial `mediaQueue`, which also runs the rotation
// and finalize paths — writer state is queue-confined, no locks. Progress is
// reported as NDJSON event lines on stdout (the Story 16.2 contract):
// `{"event":"state","state":"preflight"|"recording"|"rotating"|"stopping"|"finalized"}`,
// `{"event":"segmentClosed",…}`, and `{"event":"error","message":"…"}` — the
// host's parser drops unknown extras, so event lines may carry a `path`.

import AVFoundation
import CoreGraphics
import CoreMedia
import Foundation
import ScreenCaptureKit

/// A capture failure with a human-readable, non-secret message.
struct CaptureError: Error, CustomStringConvertible {
    let message: String
    init(_ message: String) { self.message = message }
    var description: String { message }
}

/// One segment's writer stack: the `AVAssetWriter`, its inputs, the file it
/// writes, and its 0-based session index. Created by
/// `CaptureEngine.makeSegmentWriter` and touched only on `mediaQueue` after
/// capture is live.
private final class SegmentWriter {
    let writer: AVAssetWriter
    let videoInput: AVAssetWriterInput
    let audioInput: AVAssetWriterInput?
    let path: String
    let index: Int

    init(
        writer: AVAssetWriter, videoInput: AVAssetWriterInput,
        audioInput: AVAssetWriterInput?, path: String, index: Int
    ) {
        self.writer = writer
        self.videoInput = videoInput
        self.audioInput = audioInput
        self.path = path
        self.index = index
    }
}

/// The single capture session this process can run (one session per sidecar
/// spawn — AD-34; the host spawns a fresh `keeper-rec` per recording).
final class CaptureEngine: NSObject, SCStreamDelegate, SCStreamOutput {
    /// The one serial queue owning all writer state: SCStream delivers both
    /// output types here, and rotation/stop/finalize run here too.
    private let mediaQueue = DispatchQueue(label: "dev.tgorka.keeper-rec.media")

    private var stream: SCStream?
    /// The writer currently receiving samples (segment N). Swapped on rotation.
    private var current: SegmentWriter?
    /// The pure rotation trigger (byte budget + duration cap), fixed at start.
    private var policy = RotationPolicy(
        segmentMB: RotationPolicy.defaultSegmentMB,
        maxSegmentSeconds: RotationPolicy.defaultMaxSegmentSeconds)
    /// Balanced around every writer's `finishWriting`; the clean-stop path
    /// waits on it so `exit` never abandons a segment mid-`moov`.
    private let finalizeGroup = DispatchGroup()

    // Writer-construction inputs, seeded once in `beginCapture` and reused for
    // every rotation target (same display, same tracks, every segment).
    private var pixelWidth = 2
    private var pixelHeight = 2
    private var wantsAudio = false

    /// Set on `mediaQueue` when the writer session has been anchored at the
    /// first complete video frame's PTS.
    private var sessionStarted = false
    /// Set on `mediaQueue` when a stop began — later samples are dropped.
    private var stopping = false
    /// PTS of the current segment's anchor frame — the elapsed-time base for
    /// the duration-cap fallback. Host-clock timeline, shared by all segments.
    private var segmentStartPTS = CMTime.invalid
    /// Whether the current segment has received a video frame yet (feeds the
    /// policy's `isFirstFrameOfSegment` guard — never rotate a just-opened
    /// segment, even under a tiny budget).
    private var segmentHasVideo = false
    /// The current segment's FIRST appended video sample's PTS in original
    /// capture-clock seconds (Story 17.4, NFR-22) — captured before the writer
    /// rebases the file timeline to 0. `nil` until the segment's first video
    /// frame is appended; reported as `ptsStart` on the retiring segment's
    /// `segmentClosed`.
    private var segmentFirstVideoPTS: Double?
    /// The current segment's LAST appended video sample's PTS in original
    /// capture-clock seconds — updated on every appended video frame; reported
    /// as `ptsEnd` on the retiring segment's `segmentClosed`.
    private var segmentLastVideoPTS: Double?
    /// True from cut-begin until the retired writer's finalize completion has
    /// emitted its `segmentClosed` + `recording` pair. No overlapping
    /// rotations, so the host always sees rotating → segmentClosed → recording
    /// strictly in order (anything else would be an illegal host transition).
    private var rotationInFlight = false

    /// Whether `start` was called (main-thread only; gates the EOF-as-stop path).
    private(set) var isActive = false

    /// Begin capturing to `path` (Story 17.1). The video target is one of
    /// (Story 19.1): a specific `applicationPid` (app-scoped, exclusionary — only
    /// that app's windows land in the file), a specific `displayId`, or the main
    /// display (`applicationPid == nil && displayId == nil`). Rotating segments
    /// per `segmentMB` / `maxSegmentSeconds`. Emits `preflight` immediately, then
    /// `recording` (with the path) once frames are flowing, or a single honest
    /// `error` line on any failure — including a vanished application pid.
    func start(
        path: String, displayId: UInt32?, applicationPid: Int32?, applicationBundleId: String?,
        systemAudio: Bool, segmentMB: Int, maxSegmentSeconds: Int
    ) {
        isActive = true
        policy = RotationPolicy(segmentMB: segmentMB, maxSegmentSeconds: maxSegmentSeconds)
        emitEvent(["event": "state", "state": "preflight"])
        Task {
            do {
                // Shareable-content enumeration is the real TCC/SCK gate: an
                // ungranted or (macOS 15+) ad-hoc-rejected process fails HERE,
                // which surfaces as the honest error event below (Cap #1722).
                let content = try await SCShareableContent.excludingDesktopWindows(
                    false, onScreenWindowsOnly: false)

                // Story 19.1: an application target is captured app-scoped. The
                // pid is re-resolved against LIVE shareable content — a pid that
                // vanished between the picker's last poll and Start is absent
                // here and fails cleanly (honest `error` → host `Failed`), never
                // a hung recording.
                if let wantedPid = applicationPid {
                    // Re-resolve against LIVE shareable content, matching BOTH
                    // pid and (when provided) bundle id — the OS recycles a pid to
                    // a different app within seconds, so pid alone could capture
                    // the wrong app. A vanished or rebound pid is absent here and
                    // fails cleanly (honest `error` → host `Failed`).
                    guard
                        let app = content.applications.first(where: {
                            $0.processID == wantedPid
                                && (applicationBundleId == nil
                                    || $0.bundleIdentifier == applicationBundleId)
                        })
                    else {
                        throw CaptureError(
                            "the selected application is no longer running")
                    }
                    // App-scoped capture lives on a display: anchor to the display
                    // hosting the app's frontmost on-screen window (falling back to
                    // the main display, then any). An app with NO on-screen window
                    // would yield an empty (black) filter, so fail honestly instead
                    // of recording nothing.
                    let appWindows = content.windows.filter {
                        $0.owningApplication?.processID == wantedPid && $0.isOnScreen
                    }
                    guard let anchor = appWindows.first else {
                        throw CaptureError(
                            "the selected application has no on-screen window to record")
                    }
                    let center = CGPoint(x: anchor.frame.midX, y: anchor.frame.midY)
                    guard
                        let display = content.displays.first(where: {
                            $0.frame.contains(center)
                        })
                            ?? content.displays.first(where: {
                                CGDisplayIsMain($0.displayID) != 0
                            })
                            ?? content.displays.first
                    else {
                        throw CaptureError("no recordable display found")
                    }
                    try self.beginCapture(
                        display: display, application: app, path: path,
                        systemAudio: systemAudio)
                    return
                }

                let display: SCDisplay
                if let wanted = displayId {
                    guard let match = content.displays.first(where: { $0.displayID == wanted })
                    else {
                        throw CaptureError("display \(wanted) is not recordable")
                    }
                    display = match
                } else if let main = content.displays.first(where: {
                    CGDisplayIsMain($0.displayID) != 0
                }) ?? content.displays.first {
                    display = main
                } else {
                    throw CaptureError("no recordable display found")
                }
                try self.beginCapture(
                    display: display, application: nil, path: path, systemAudio: systemAudio)
            } catch {
                emitEvent([
                    "event": "error",
                    "message": "capture start failed: \(String(describing: error))",
                ])
                exit(0)
            }
        }
    }

    /// Build segment 0's writer + the stream and start capturing. Runs on the
    /// Task executor; writer state it seeds is only touched again on
    /// `mediaQueue`. When `application` is non-nil the filter is app-scoped
    /// (Story 19.1) — only that application's windows land in the file; keeper,
    /// other apps, and notification banners are absent because they are not the
    /// target app. When nil the whole display is captured (the 16.6 path).
    private func beginCapture(
        display: SCDisplay, application: SCRunningApplication?, path: String, systemAudio: Bool
    ) throws {
        // Capture at the display's true pixel size (SCDisplay reports points).
        pixelWidth = max(2, CGDisplayPixelsWide(display.displayID))
        pixelHeight = max(2, CGDisplayPixelsHigh(display.displayID))
        wantsAudio = systemAudio

        let first = try makeSegmentWriter(path: path, index: 0)

        // Story 19.1: an application target is exclusionary — `including:[app]`
        // keeps only that app's windows in the file. The display-only branch
        // (16.6) is unchanged. `excludesCurrentProcessAudio` (set below when
        // `systemAudio`) keeps keeper's own sounds out either way (audio behavior
        // is unchanged — 19.2 owns per-app audio scoping).
        let filter: SCContentFilter
        if let application {
            filter = SCContentFilter(
                display: display, including: [application], exceptingWindows: [])
        } else {
            filter = SCContentFilter(display: display, excludingWindows: [])
        }
        let config = SCStreamConfiguration()
        config.width = pixelWidth
        config.height = pixelHeight
        config.minimumFrameInterval = CMTime(value: 1, timescale: 30)
        config.showsCursor = true
        config.queueDepth = 8
        config.pixelFormat = kCVPixelFormatType_32BGRA
        if systemAudio {
            // System-audio capture with keeper's own sounds excluded (FR-69).
            config.capturesAudio = true
            config.excludesCurrentProcessAudio = true
            config.sampleRate = 48_000
            config.channelCount = 2
        }

        let stream = SCStream(filter: filter, configuration: config, delegate: self)
        try stream.addStreamOutput(self, type: .screen, sampleHandlerQueue: mediaQueue)
        if systemAudio {
            try stream.addStreamOutput(self, type: .audio, sampleHandlerQueue: mediaQueue)
        }

        self.current = first
        self.stream = stream

        stream.startCapture { error in
            if let error {
                emitEvent([
                    "event": "error",
                    "message": "capture start failed: \(String(describing: error))",
                ])
                exit(0)
            }
            // Capture is live (FR-68); the extra `path` names the output file
            // (the host's tolerant parser keeps or drops it freely).
            emitEvent(["event": "state", "state": "recording", "path": path])
        }
    }

    /// Build one segment's writer stack (fragmented MP4, H.264 + optional AAC)
    /// and start it writing. Called for segment 0 from `beginCapture` and for
    /// each rotation target on `mediaQueue`. A failure (e.g. a non-writable
    /// directory) throws — the callers surface it as a single `error` event and
    /// a clean exit.
    private func makeSegmentWriter(path: String, index: Int) throws -> SegmentWriter {
        let url = URL(fileURLWithPath: path)
        try FileManager.default.createDirectory(
            at: url.deletingLastPathComponent(), withIntermediateDirectories: true)

        let writer = try AVAssetWriter(outputURL: url, fileType: .mp4)
        // Fragmented MP4 (~4 s fragments, AD-37): size is observable live and a
        // mid-segment kill loses at most the last fragment. `finishWriting`
        // writes the final `moov`, defragmenting into an ordinary playable mp4.
        writer.movieFragmentInterval = CMTime(seconds: 4, preferredTimescale: 600)

        let videoInput = AVAssetWriterInput(
            mediaType: .video,
            outputSettings: [
                AVVideoCodecKey: AVVideoCodecType.h264,
                AVVideoWidthKey: pixelWidth,
                AVVideoHeightKey: pixelHeight,
                AVVideoCompressionPropertiesKey: [
                    // Generous for a full retina display; H.264 High auto-level.
                    AVVideoAverageBitRateKey: 10_000_000,
                    AVVideoExpectedSourceFrameRateKey: 30,
                    AVVideoProfileLevelKey: AVVideoProfileLevelH264HighAutoLevel,
                ],
            ])
        videoInput.expectsMediaDataInRealTime = true
        guard writer.canAdd(videoInput) else {
            throw CaptureError("could not add the H.264 video track")
        }
        writer.add(videoInput)

        var audioInput: AVAssetWriterInput?
        if wantsAudio {
            let input = AVAssetWriterInput(
                mediaType: .audio,
                outputSettings: [
                    AVFormatIDKey: kAudioFormatMPEG4AAC,
                    AVSampleRateKey: 48_000,
                    AVNumberOfChannelsKey: 2,
                    AVEncoderBitRateKey: 192_000,
                ])
            input.expectsMediaDataInRealTime = true
            guard writer.canAdd(input) else {
                throw CaptureError("could not add the AAC audio track")
            }
            writer.add(input)
            audioInput = input
        }

        guard writer.startWriting() else {
            throw CaptureError(
                "could not start writing: \(writer.error.map(String.init(describing:)) ?? "unknown")"
            )
        }

        return SegmentWriter(
            writer: writer, videoInput: videoInput, audioInput: audioInput, path: path,
            index: index)
    }

    /// The observed **on-disk** size of `path` in bytes (0 when unreadable).
    /// This — not an in-memory appended-byte tally — feeds the rotation
    /// trigger: fMP4 buffering makes appended counts run ahead of what a crash
    /// would actually preserve on disk.
    private func onDiskBytes(atPath path: String) -> UInt64 {
        let attributes = try? FileManager.default.attributesOfItem(atPath: path)
        return (attributes?[.size] as? NSNumber)?.uint64Value ?? 0
    }

    /// Perform one gapless dual-writer handover at `keyframePTS` (Story 17.1),
    /// on `mediaQueue`: emit `rotating`, start the next writer anchored at the
    /// cut frame's PTS (the same host-clock timeline — continuous across the
    /// cut), append the cut keyframe to it, and finalize the retired writer
    /// asynchronously. `segmentClosed{index,path,bytes,track}` + `recording`
    /// follow from the retired writer's finalize completion, so the host sees
    /// the bracket strictly in order. Any failure surfaces as a single `error`
    /// line and a clean exit (the sidecar invariant).
    private func rotate(
        at keyframePTS: CMTime, keyframe sampleBuffer: CMSampleBuffer, retiring: SegmentWriter
    ) {
        rotationInFlight = true
        emitEvent(["event": "state", "state": "rotating"])

        let next: SegmentWriter
        do {
            next = try makeSegmentWriter(
                path: nextSegmentPath(from: retiring.path), index: retiring.index + 1)
        } catch {
            emitEvent([
                "event": "error",
                "message": "segment rotation failed: \(String(describing: error))",
            ])
            exit(0)
        }

        // The retiring segment's host-clock video PTS bounds (Story 17.4,
        // NFR-22): everything it accumulated BEFORE this cut. Read here — the
        // muxer rebases each file's own timeline to 0, so the original
        // capture-clock bounds exist only in this process, at this moment. The
        // cut keyframe goes to writer B below, so the NEW segment's first PTS
        // is exactly `keyframePTS`.
        let retiringPTSStart = segmentFirstVideoPTS
        let retiringPTSEnd = segmentLastVideoPTS

        // Hand over: writer B opens at the cut keyframe's PTS and takes this
        // keyframe and everything after it; A gets nothing more. Audio has no
        // keyframes — it simply follows `current`, splitting at the same
        // handover boundary (a boundary sample kept whole on one side is
        // within the one-frame tolerance; the writer trims pre-session
        // samples).
        next.writer.startSession(atSourceTime: keyframePTS)
        current = next
        segmentStartPTS = keyframePTS
        segmentHasVideo = false
        segmentFirstVideoPTS = nil
        segmentLastVideoPTS = nil
        // The cut keyframe MUST land in writer B, or the new segment would start
        // on a non-keyframe and be non-self-decodable (breaking the gapless /
        // independently-playable invariant, AD-37). A freshly `startWriting`+
        // `startSession`'d real-time input is ready immediately; if it somehow is
        // not, fail loudly rather than silently produce a corrupt segment.
        guard next.videoInput.isReadyForMoreMediaData else {
            emitEvent([
                "event": "error",
                "message": "rotation writer was not ready for the cut keyframe",
            ])
            exit(0)
        }
        next.videoInput.append(sampleBuffer)
        segmentHasVideo = true
        // The new segment opens on the cut keyframe: its first AND (so far)
        // last appended video PTS is the cut PTS itself. A non-numeric cut
        // PTS leaves the bounds nil (reported as null) rather than seeding a
        // NaN that would later break the segment's JSON event line.
        segmentFirstVideoPTS = keyframePTS.isNumeric ? keyframePTS.seconds : nil
        segmentLastVideoPTS = segmentFirstVideoPTS

        retiring.videoInput.markAsFinished()
        retiring.audioInput?.markAsFinished()
        finalizeGroup.enter()
        retiring.writer.finishWriting { [self] in
            mediaQueue.async { [self] in
                guard retiring.writer.status == .completed else {
                    emitEvent([
                        "event": "error",
                        "message":
                            "segment finalize failed: \(retiring.writer.error.map(String.init(describing:)) ?? "unknown")",
                    ])
                    exit(0)
                }
                rotationInFlight = false
                // While stopping, the host is in `Stopping` — a segmentClosed
                // or `recording` line now would be an illegal transition; the
                // stop path owns all remaining signalling (`finalized`).
                if !stopping {
                    // Additive `ptsStart`/`ptsEnd` (Story 17.4, NFR-22): the
                    // retiring segment's original capture-clock video bounds.
                    // A rotation only ever retires a segment that appended
                    // video (the policy never cuts a first frame), so the
                    // bounds are present in practice; the host parser reads
                    // them best-effort either way.
                    var closed: [String: Any] = [
                        "event": "segmentClosed",
                        "index": retiring.index,
                        "path": retiring.path,
                        "bytes": onDiskBytes(atPath: retiring.path),
                        "track": "screen",
                    ]
                    if let retiringPTSStart { closed["ptsStart"] = retiringPTSStart }
                    if let retiringPTSEnd { closed["ptsEnd"] = retiringPTSEnd }
                    emitEvent(closed)
                    emitEvent(["event": "state", "state": "recording", "path": next.path])
                }
                finalizeGroup.leave()
            }
        }
    }

    /// Stop cleanly: emit `stopping`, stop the stream, finish the current
    /// writer (which finalizes its `moov` — an ordinary playable `.mp4`), wait
    /// for any in-flight retired-writer finalize, emit `finalized`, and exit.
    /// Safe to call once; later samples are dropped via `stopping`.
    func stop() {
        emitEvent(["event": "state", "state": "stopping"])
        guard let stream else {
            // Stop before capture ever started: nothing to finalize.
            emitEvent(["event": "error", "message": "stop before capture started"])
            exit(0)
        }
        mediaQueue.async { self.stopping = true }
        stream.stopCapture { _ in
            // A stop error is irrelevant here — finalize whatever was written.
            self.mediaQueue.async { self.finishAndExit() }
        }
    }

    /// Finish the current writer on `mediaQueue` and exit once every segment's
    /// finalize has completed. The FINAL segment intentionally emits no
    /// `segmentClosed` — while the host is Stopping that would be an illegal
    /// transition; `finalized` is its closure signal (Story 17.1 contract). No
    /// frames captured → an honest error (an empty writer cannot produce a
    /// playable file).
    private func finishAndExit() {
        guard let current, sessionStarted, current.writer.status == .writing else {
            self.current?.writer.cancelWriting()
            emitEvent([
                "event": "error",
                "message": "no frames were captured before stop",
            ])
            exit(0)
        }
        current.videoInput.markAsFinished()
        current.audioInput?.markAsFinished()
        finalizeGroup.enter()
        current.writer.finishWriting { self.finalizeGroup.leave() }
        // Wait for the final writer AND any still-running retired-writer
        // finalize before reporting — `exit` must never abandon a segment
        // mid-`moov`.
        finalizeGroup.notify(queue: mediaQueue) {
            if current.writer.status == .completed {
                emitEvent(["event": "state", "state": "finalized"])
            } else {
                emitEvent([
                    "event": "error",
                    "message":
                        "finalize failed: \(current.writer.error.map(String.init(describing:)) ?? "unknown")",
                ])
            }
            exit(0)
        }
    }

    // MARK: - SCStreamOutput (on mediaQueue)

    func stream(
        _ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer,
        of type: SCStreamOutputType
    ) {
        guard sampleBuffer.isValid, !stopping, let current else { return }
        switch type {
        case .screen:
            // Only complete frames carry image data (idle/suspended frames do not).
            guard
                let attachments = CMSampleBufferGetSampleAttachmentsArray(
                    sampleBuffer, createIfNecessary: false) as? [[SCStreamFrameInfo: Any]],
                let statusRaw = attachments.first?[.status] as? Int,
                statusRaw == SCFrameStatus.complete.rawValue
            else { return }
            let pts = CMSampleBufferGetPresentationTimeStamp(sampleBuffer)
            // Anchor the writer session at the first complete video frame's PTS
            // so video and audio share one host-clock timeline (NFR-22 seed).
            if !sessionStarted {
                current.writer.startSession(atSourceTime: pts)
                sessionStarted = true
                segmentStartPTS = pts
            }
            // Rotation decision at every complete frame. SCStream delivers raw
            // (unencoded) frames, so every complete frame is a valid keyframe
            // cut point: the next writer's H.264 encoder opens its stream with
            // an IDR keyframe, making the new segment self-decodable from
            // sample one — hence `isKeyframe: true` here.
            if !rotationInFlight {
                let elapsed = CMTimeSubtract(pts, segmentStartPTS).seconds
                if policy.shouldRotate(
                    observedBytes: onDiskBytes(atPath: current.path),
                    elapsedSeconds: elapsed.isFinite ? elapsed : 0,
                    isKeyframe: true,
                    isFirstFrameOfSegment: !segmentHasVideo)
                {
                    rotate(at: pts, keyframe: sampleBuffer, retiring: current)
                    return
                }
            }
            if current.videoInput.isReadyForMoreMediaData {
                current.videoInput.append(sampleBuffer)
                segmentHasVideo = true
                // Track the segment's appended-video PTS bounds in original
                // capture-clock seconds (Story 17.4, NFR-22) — the muxer is
                // about to rebase the file timeline, so this is the only
                // moment the host-clock bounds can be observed. Only a numeric
                // PTS is recorded: a non-numeric CMTime's `.seconds` is NaN,
                // and a NaN in the `segmentClosed` payload would make
                // JSONSerialization throw and silently drop the whole event
                // line (index and all), not just the bound.
                if pts.isNumeric {
                    let seconds = pts.seconds
                    if segmentFirstVideoPTS == nil { segmentFirstVideoPTS = seconds }
                    segmentLastVideoPTS = seconds
                }
            }
        case .audio:
            // Audio before the session anchor is dropped (the anchor is video).
            guard sessionStarted, let audioInput = current.audioInput,
                audioInput.isReadyForMoreMediaData
            else {
                return
            }
            audioInput.append(sampleBuffer)
        default:
            // `.microphone` (macOS 15+) and any future output types are not
            // captured in this story (microphone capture is Epic 19/20).
            break
        }
    }

    // MARK: - SCStreamDelegate

    func stream(_ stream: SCStream, didStopWithError error: Error) {
        // The OS tore the stream down (display unplugged, session revoked…):
        // surface honestly and salvage nothing here (recovery is Story 17.3).
        emitEvent([
            "event": "error",
            "message": "capture stopped: \(String(describing: error))",
        ])
        exit(0)
    }
}
