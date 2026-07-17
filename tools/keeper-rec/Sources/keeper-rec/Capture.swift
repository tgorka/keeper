// SPDX-License-Identifier: Apache-2.0
//
// The full-screen + system-audio capture engine (Story 16.6, FR-68, FR-69,
// FR-71, AD-37).
//
// One `SCStream` over an `SCContentFilter` for a whole display, with
// `capturesAudio` + `excludeCurrentProcessAudio` (keeper's own notification
// sounds are absent from the recording), feeding one `AVAssetWriter` writing a
// **single fragmented MP4** (H.264 video + one AAC system-audio track, ~4 s
// fragments via `movieFragmentInterval`) so a mid-session kill loses at most
// the last fragment. A clean stop finishes the writer, which finalizes the
// file's `moov` — the result is an ordinary playable `.mp4`.
//
// Threading: every SCStream sample callback (both `.screen` and `.audio`) is
// delivered on the single serial `mediaQueue`, which also runs the finalize
// path — writer state is queue-confined, no locks. Progress is reported as
// NDJSON event lines on stdout (the Story 16.2 contract):
// `{"event":"state","state":"preflight"|"recording"|"stopping"|"finalized"}`
// and `{"event":"error","message":"…"}` — the host's parser drops unknown
// extras, so the `recording` line also carries the output `path`.

import AVFoundation
import CoreGraphics
import CoreMedia
import Foundation
import ScreenCaptureKit

/// A capture-start failure with a human-readable, non-secret message.
struct CaptureError: Error, CustomStringConvertible {
    let message: String
    init(_ message: String) { self.message = message }
    var description: String { message }
}

/// The single capture session this process can run (one session per sidecar
/// spawn — AD-34; the host spawns a fresh `keeper-rec` per recording).
final class CaptureEngine: NSObject, SCStreamDelegate, SCStreamOutput {
    /// The one serial queue owning all writer state: SCStream delivers both
    /// output types here, and stop/finalize runs here too.
    private let mediaQueue = DispatchQueue(label: "dev.tgorka.keeper-rec.media")

    private var stream: SCStream?
    private var writer: AVAssetWriter?
    private var videoInput: AVAssetWriterInput?
    private var audioInput: AVAssetWriterInput?
    /// Set on `mediaQueue` when the writer session has been anchored at the
    /// first complete video frame's PTS.
    private var sessionStarted = false
    /// Set on `mediaQueue` when a stop began — later samples are dropped.
    private var stopping = false
    /// Whether `start` was called (main-thread only; gates the EOF-as-stop path).
    private(set) var isActive = false

    /// Begin capturing `displayId` (or the main display) with system audio to
    /// `path`. Emits `preflight` immediately, then `recording` (with the path)
    /// once frames are flowing, or a single honest `error` line on any failure.
    func start(path: String, displayId: UInt32?, systemAudio: Bool) {
        isActive = true
        emitEvent(["event": "state", "state": "preflight"])
        Task {
            do {
                // Shareable-content enumeration is the real TCC/SCK gate: an
                // ungranted or (macOS 15+) ad-hoc-rejected process fails HERE,
                // which surfaces as the honest error event below (Cap #1722).
                let content = try await SCShareableContent.excludingDesktopWindows(
                    false, onScreenWindowsOnly: false)
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
                try self.beginCapture(display: display, path: path, systemAudio: systemAudio)
            } catch {
                emitEvent([
                    "event": "error",
                    "message": "capture start failed: \(String(describing: error))",
                ])
                exit(0)
            }
        }
    }

    /// Build the writer + stream and start capturing. Runs on the Task executor;
    /// writer state it seeds is only touched again on `mediaQueue`.
    private func beginCapture(display: SCDisplay, path: String, systemAudio: Bool) throws {
        let url = URL(fileURLWithPath: path)
        try FileManager.default.createDirectory(
            at: url.deletingLastPathComponent(), withIntermediateDirectories: true)

        // Capture at the display's true pixel size (SCDisplay reports points).
        let pixelWidth = max(2, CGDisplayPixelsWide(display.displayID))
        let pixelHeight = max(2, CGDisplayPixelsHigh(display.displayID))

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
        if systemAudio {
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

        let filter = SCContentFilter(display: display, excludingWindows: [])
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

        self.writer = writer
        self.videoInput = videoInput
        self.audioInput = audioInput
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

    /// Stop cleanly: emit `stopping`, stop the stream, finish the writer (which
    /// finalizes the `moov` — an ordinary playable `.mp4`), emit `finalized`,
    /// and exit. Safe to call once; later samples are dropped via `stopping`.
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

    /// Finish the writer on `mediaQueue` and exit. No frames captured → an
    /// honest error (an empty writer cannot produce a playable file).
    private func finishAndExit() {
        guard let writer, sessionStarted, writer.status == .writing else {
            self.writer?.cancelWriting()
            emitEvent([
                "event": "error",
                "message": "no frames were captured before stop",
            ])
            exit(0)
        }
        videoInput?.markAsFinished()
        audioInput?.markAsFinished()
        writer.finishWriting {
            if writer.status == .completed {
                emitEvent(["event": "state", "state": "finalized"])
            } else {
                emitEvent([
                    "event": "error",
                    "message":
                        "finalize failed: \(writer.error.map(String.init(describing:)) ?? "unknown")",
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
        guard sampleBuffer.isValid, !stopping, let writer else { return }
        switch type {
        case .screen:
            // Only complete frames carry image data (idle/suspended frames do not).
            guard
                let attachments = CMSampleBufferGetSampleAttachmentsArray(
                    sampleBuffer, createIfNecessary: false) as? [[SCStreamFrameInfo: Any]],
                let statusRaw = attachments.first?[.status] as? Int,
                statusRaw == SCFrameStatus.complete.rawValue
            else { return }
            // Anchor the writer session at the first complete video frame's PTS
            // so video and audio share one host-clock timeline (NFR-22 seed).
            if !sessionStarted {
                writer.startSession(
                    atSourceTime: CMSampleBufferGetPresentationTimeStamp(sampleBuffer))
                sessionStarted = true
            }
            if let videoInput, videoInput.isReadyForMoreMediaData {
                videoInput.append(sampleBuffer)
            }
        case .audio:
            // Audio before the session anchor is dropped (the anchor is video).
            guard sessionStarted, let audioInput, audioInput.isReadyForMoreMediaData else {
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
        // surface honestly and salvage nothing here (recovery is Epic 17).
        emitEvent([
            "event": "error",
            "message": "capture stopped: \(String(describing: error))",
        ])
        exit(0)
    }
}
