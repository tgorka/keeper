// SPDX-License-Identifier: Apache-2.0
//
// The full-screen + system-audio capture engine (Story 16.6 → 17.1, FR-68,
// FR-69, FR-71, AD-37).
//
// One `SCStream` over an `SCContentFilter` for a whole display, with
// `capturesAudio` + `excludeCurrentProcessAudio` (keeper's own notification
// sounds are absent from the recording), feeding a **rotating chain of
// `AVAssetWriter`s** (dual-writer gapless rotation, Story 17.1). Each segment
// is a fragmented MP4 (H.264 video + an optional AAC system-audio track + an
// optional, separate AAC microphone track — Story 19.3, never premixed; ~4 s
// fragments via `movieFragmentInterval`) so a mid-session kill loses at most
// the last fragment; a clean `finishWriting` finalizes each segment's `moov`
// into an ordinary playable `.mp4`.
//
// The optional webcam (Story 20.1, FR-70) is a SECOND, separate file per
// segment — `camera-####.mp4` from a dedicated AVCaptureSession + video-only
// AVAssetWriter, host-clock anchored and cut at the screen's rotation
// boundaries (screen is the rotation master; the camera never runs its own
// RotationPolicy), never a track inside `screen-####`, never composited.
// Camera loss mid-recording is non-fatal: a sticky `cameraLost` warning, an
// early camera-file finalize, and the screen keeps rolling.
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
// `{"event":"segmentClosed",…}`, the non-fatal
// `{"event":"warning","code":…,"message":…}` (Story 19.4 — mic loss never
// aborts the session), and `{"event":"error","message":"…"}` — the host's
// parser drops unknown extras, so event lines may carry a `path`.

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
    /// The microphone's own AAC track (Story 19.3, AD-36) — a second, unmixed
    /// audio input beside the system-audio one, present only when the mic
    /// source is enabled. Rebuilt per segment like every other input, so the
    /// mic track survives rotation exactly like the system-audio track.
    let micInput: AVAssetWriterInput?
    let path: String
    let index: Int

    init(
        writer: AVAssetWriter, videoInput: AVAssetWriterInput,
        audioInput: AVAssetWriterInput?, micInput: AVAssetWriterInput?, path: String, index: Int
    ) {
        self.writer = writer
        self.videoInput = videoInput
        self.audioInput = audioInput
        self.micInput = micInput
        self.path = path
        self.index = index
    }
}

/// The single capture session this process can run (one session per sidecar
/// spawn — AD-34; the host spawns a fresh `keeper-rec` per recording).
final class CaptureEngine: NSObject, SCStreamDelegate, SCStreamOutput,
    AVCaptureAudioDataOutputSampleBufferDelegate, AVCaptureVideoDataOutputSampleBufferDelegate
{
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
    /// Whether the mic source is enabled (Story 19.3) — every segment writer
    /// then carries the second, unmixed mic AAC track.
    private var wantsMic = false
    /// The macOS 13–14 microphone path (Story 19.3, AD-36): an audio-only
    /// AVCaptureSession feeding the same per-segment `micInput` the 15+
    /// in-stream `.microphone` output would — one writer, user-invisible OS
    /// split. `nil` on macOS 15+ (the SCStream carries the mic) and while the
    /// mic is off. Also stood up on 15+ as the mic-loss *fallback* feed
    /// (Story 19.4) — see `attemptMicFallback`.
    private var micSession: AVCaptureSession?
    /// Best-effort uniqueID of the device actually feeding the mic track
    /// (Story 19.4): the picked device, else a snapshot of the system default
    /// input at start (re-snapshotted when a fallback attaches). Feeds
    /// `MicHealth.decide` so an unrelated device's removal never raises a
    /// false warning.
    private var activeMicDeviceId: String?
    /// Set on `mediaQueue` when the mic is lost (Story 19.4) — the
    /// silence-fill gate. Cleared when a real mic sample flows again (the
    /// fallback device came up).
    private var micLost = false
    /// The repeating silence-fill timer on `mediaQueue` (Story 19.4), armed on
    /// the first loss and cancelled at stop.
    private var silenceTimer: DispatchSourceTimer?
    /// The mic track's written tail — the next acceptable mic PTS (Story
    /// 19.4). Real samples and silence-fill both advance it; `appendMicSample`
    /// trims any sample landing below it (a late buffer from a dying device, a
    /// fallback overlapping an already-silence-filled span) so the track's
    /// timeline never rewinds.
    private var micPTSLowerBound = CMTime.invalid
    /// Cached LPCM format description for generated silence buffers (Story 19.4).
    private var silenceFormatDescription: CMFormatDescription?
    /// Device-removal / runtime-error observer tokens (Story 19.4), removed at
    /// stop so no late signal fires into a winding-down engine.
    private var micObservers: [NSObjectProtocol] = []
    /// The current mic `AVCaptureSession`'s runtime-error observer (Story 19.4),
    /// kept apart from `micObservers` so a fallback that re-arms the session
    /// REPLACES it rather than accumulating one observer per fallback (which
    /// would fire `handleMicLost` N times for a single later error). Removed at
    /// stop with the rest.
    private var micSessionRuntimeObserver: NSObjectProtocol?

    // MARK: - Webcam state (Story 20.1, FR-70, AD-37)

    /// Whether the camera source is enabled (Story 20.1) — the session then
    /// writes a SECOND, separate `camera-####.mp4` file per segment from a
    /// dedicated video-only writer, host-clock anchored and cut at the
    /// screen's rotation boundaries. Never a track inside `screen-####`.
    private var wantsCamera = false
    /// The camera `AVCaptureSession` (Story 20.1): an `AVCaptureVideoDataOutput`
    /// delivering on the same serial `mediaQueue` as every other sample —
    /// writer state stays queue-confined, no locks. `nil` while the camera is
    /// off or after a loss finalized the camera file early.
    private var cameraSession: AVCaptureSession?
    /// The camera writer currently receiving samples (video-only; the
    /// `SegmentWriter` audio/mic inputs stay nil). Swapped in lockstep with
    /// the SCREEN rotation — the camera never runs its own RotationPolicy.
    private var currentCamera: SegmentWriter?
    /// Set on `mediaQueue` once the camera writer session has been anchored
    /// at its first host-clock sample PTS (`startSession(atSourceTime:)`).
    private var cameraSessionStarted = false
    /// Set on `mediaQueue` when the camera is lost (Story 20.1) — terminal
    /// for the session's camera leg: the current `camera-####.mp4` finalizes
    /// early and no later segment carries a camera file (no black-fill, no
    /// fallback re-feed — the intent contract).
    private var cameraLost = false
    /// Best-effort uniqueID of the device feeding the camera file — feeds
    /// `CameraHealth.decide` so an unrelated device's removal never raises a
    /// false warning.
    private var activeCameraDeviceId: String?
    /// The camera's native pixel size (from the resolved device's active
    /// format), seeding every camera segment writer.
    private var cameraPixelWidth = 2
    private var cameraPixelHeight = 2
    /// The current camera segment's reported PTS bounds in original
    /// capture-clock seconds — `ptsStart`/`ptsEnd` on the camera
    /// `segmentClosed`, paired by index against the screen segment's bounds
    /// (the NFR-22 alignment gate). `ptsStart` is the SHARED screen
    /// anchor/boundary PTS, recorded when the camera segment is anchored —
    /// BEFORE any camera frame arrives — so a camera warming up seconds after
    /// the screen (or lagging a rotation cut) never shifts the reported
    /// boundary. `ptsEnd` is the last REAL appended camera frame.
    private var cameraSegmentFirstVideoPTS: Double?
    private var cameraSegmentLastVideoPTS: Double?
    /// Whether the current camera segment has received a REAL appended frame
    /// (the screen's `segmentHasVideo` twin). Deliberately distinct from
    /// `cameraSegmentFirstVideoPTS`, which is set at anchor time before any
    /// frame — the empty-vs-nonempty decision must never confuse "anchored"
    /// with "has content": a zero-frame camera file is dropped, never
    /// finalized (the concat reader throws `noVideoFrames` on it).
    private var cameraSegmentHasVideo = false
    /// Camera device-removal observer tokens, removed at stop so no late
    /// signal fires into a winding-down engine.
    private var cameraObservers: [NSObjectProtocol] = []
    /// The camera `AVCaptureSession`'s runtime-error observer (kept apart so
    /// removal at stop is cheap and final, the mic precedent).
    private var cameraSessionRuntimeObserver: NSObjectProtocol?
    /// The session folder (with trailing slash) derived from the host-supplied
    /// screen path — `camera-####.mp4` basenames ride beside it.
    private var sessionDirectory = ""

    /// Set on `mediaQueue` when the writer session has been anchored at the
    /// first complete video frame's PTS.
    private var sessionStarted = false
    /// Set on `mediaQueue` when a stop began — later samples are dropped.
    private var stopping = false
    /// PTS of the current segment's anchor frame — the elapsed-time base for
    /// the duration-cap fallback. Host-clock timeline, shared by all segments.
    private var segmentStartPTS = CMTime.invalid
    /// The SESSION's anchor PTS — the first complete screen frame, set once
    /// when `sessionStarted` flips true and never overwritten (unlike
    /// `segmentStartPTS`, which every rotation rebases). The camera's
    /// segment-0 `ptsStart` reports THIS shared boundary even when the camera
    /// warms up seconds later (NFR-22 same-index alignment).
    private var sessionAnchorPTS: CMTime?
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
    /// display (`applicationPid == nil && displayId == nil`). `micEnabled`
    /// (Story 19.3) adds the microphone as its own, unmixed AAC track
    /// (`micDeviceId` nil = the system default input). `cameraEnabled`
    /// (Story 20.1) adds the webcam as its own separate `camera-####.mp4`
    /// file per segment (`cameraDeviceId` nil = the system default camera;
    /// a vanished picked id falls back to the default, never an abort).
    /// Rotating segments per `segmentMB` / `maxSegmentSeconds`. `fps`
    /// (Story 19.5) selects the capture frame rate, normalized to {30, 60}
    /// before it reaches the stream configuration. Emits `preflight`
    /// immediately, then `recording` (with the path) once frames are flowing,
    /// or a single honest `error` line on any failure — including a vanished
    /// application pid.
    func start(
        path: String, displayId: UInt32?, applicationPid: Int32?, applicationBundleId: String?,
        systemAudio: Bool, micEnabled: Bool, micDeviceId: String?,
        cameraEnabled: Bool, cameraDeviceId: String?,
        segmentMB: Int, maxSegmentSeconds: Int, fps: Int
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
                        systemAudio: systemAudio, micEnabled: micEnabled,
                        micDeviceId: micDeviceId, cameraEnabled: cameraEnabled,
                        cameraDeviceId: cameraDeviceId, fps: fps)
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
                    display: display, application: nil, path: path, systemAudio: systemAudio,
                    micEnabled: micEnabled, micDeviceId: micDeviceId,
                    cameraEnabled: cameraEnabled, cameraDeviceId: cameraDeviceId, fps: fps)
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
    /// `micEnabled` (Story 19.3, AD-36) adds the second, unmixed mic AAC track:
    /// in-stream `captureMicrophone` + the `.microphone` output on macOS 15+, a
    /// parallel audio-only `AVCaptureSession` on 13–14 — same writer either way,
    /// invisible to the user and to the capability flag. `cameraEnabled`
    /// (Story 20.1, FR-70) stands up the SEPARATE camera leg: a dedicated
    /// `AVCaptureSession` + video-only writer producing `camera-####.mp4`
    /// beside each screen segment — never a track inside `screen-####`, never
    /// composited (no PiP/self-view).
    private func beginCapture(
        display: SCDisplay, application: SCRunningApplication?, path: String, systemAudio: Bool,
        micEnabled: Bool, micDeviceId: String?, cameraEnabled: Bool, cameraDeviceId: String?,
        fps: Int
    ) throws {
        // Capture at the display's true pixel size (SCDisplay reports points).
        pixelWidth = max(2, CGDisplayPixelsWide(display.displayID))
        pixelHeight = max(2, CGDisplayPixelsHigh(display.displayID))
        wantsAudio = systemAudio
        // Seed BEFORE `makeSegmentWriter` — segment 0's writer must already
        // carry the mic input when the mic source is enabled.
        wantsMic = micEnabled
        if micEnabled {
            // Story 19.4: best-effort snapshot of the device that will feed
            // the mic track — the picked device, else the current system
            // default input (whose concrete id the in-stream 15+ path never
            // reports). `nil` (no default input either) stays conservative:
            // any audio removal then warns rather than silently gapping.
            activeMicDeviceId = micDeviceId ?? AVCaptureDevice.default(for: .audio)?.uniqueID
            installMicDisconnectObserver()
        }

        let first = try makeSegmentWriter(path: path, index: 0)

        // Story 19.1: an application target is exclusionary — `including:[app]`
        // keeps only that app's windows in the file. The display-only branch
        // (16.6) is unchanged. `excludesCurrentProcessAudio` (set below when
        // `systemAudio`) keeps keeper's own sounds out either way. Story 19.2:
        // per-app/per-display audio scoping is active via this same shared
        // filter + `excludesCurrentProcessAudio` — no separate audio filter
        // exists; the toggle only decides whether the audio track is added.
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
        // Story 19.5: the selected frame rate, re-normalized defensively to
        // {30, 60} so a bad wire value can never yield a degenerate timescale.
        config.minimumFrameInterval = CMTime(value: 1, timescale: Int32(normalizeFps(fps)))
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
        if micEnabled {
            if #available(macOS 15.0, *) {
                // Story 19.3 (AD-36): on macOS 15+ the mic rides the SAME
                // SCStream — `.microphone` samples arrive through the one
                // capture source, keeping the timeline shared with video and
                // system audio. A picked device is addressed by its uniqueID;
                // absent, the OS uses the system default input.
                config.captureMicrophone = true
                if let micDeviceId {
                    config.microphoneCaptureDeviceID = micDeviceId
                }
            }
        }

        let stream = SCStream(filter: filter, configuration: config, delegate: self)
        try stream.addStreamOutput(self, type: .screen, sampleHandlerQueue: mediaQueue)
        if systemAudio {
            try stream.addStreamOutput(self, type: .audio, sampleHandlerQueue: mediaQueue)
        }
        if micEnabled {
            if #available(macOS 15.0, *) {
                // The `.microphone` output delivers on the same serial
                // `mediaQueue` as every other sample — writer state stays
                // queue-confined, no locks.
                try stream.addStreamOutput(self, type: .microphone, sampleHandlerQueue: mediaQueue)
            } else {
                // macOS 13–14 (AD-36): no in-stream mic — stand up the parallel
                // audio-only AVCaptureSession feeding the same per-segment
                // `micInput`. User-invisible: same writer, same second track.
                try startMicCaptureSession(deviceId: micDeviceId)
            }
        }

        // Story 20.1 (FR-70): the separate camera leg — a dedicated
        // AVCaptureSession + video-only writer producing `camera-0000.mp4`
        // beside the host-supplied screen basename. Seeded here so segment 0's
        // camera writer exists before the first sample can arrive.
        wantsCamera = cameraEnabled
        if cameraEnabled {
            try setupCamera(deviceId: cameraDeviceId, screenPath: path)
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

        var micInput: AVAssetWriterInput?
        if wantsMic {
            // The microphone's OWN track (Story 19.3, AD-36): a second AAC
            // input with the same shape as the system-audio one, never
            // premixed — stock players play the tracks together, editors can
            // separate them. The writer's internal converter adapts the
            // device's native format (e.g. a mono mic) to this output shape.
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
                throw CaptureError("could not add the microphone AAC track")
            }
            writer.add(input)
            micInput = input
        }

        guard writer.startWriting() else {
            throw CaptureError(
                "could not start writing: \(writer.error.map(String.init(describing:)) ?? "unknown")"
            )
        }

        return SegmentWriter(
            writer: writer, videoInput: videoInput, audioInput: audioInput, micInput: micInput,
            path: path, index: index)
    }

    /// Stand up the macOS 13–14 microphone path (Story 19.3, AD-36): an
    /// audio-only `AVCaptureSession` whose `AVCaptureAudioDataOutput` delivers
    /// samples on the same serial `mediaQueue` the SCStream uses, feeding the
    /// same per-segment `micInput` the 15+ in-stream `.microphone` output
    /// would — one writer, user-invisible OS split. macOS 15+ never calls this.
    /// A missing/unopenable device throws — surfaced by the caller as a single
    /// honest `error` event, never a hung recording.
    private func startMicCaptureSession(deviceId: String?) throws {
        let device: AVCaptureDevice?
        if let deviceId {
            device = AVCaptureDevice(uniqueID: deviceId)
        } else {
            device = AVCaptureDevice.default(for: .audio)
        }
        guard let device else {
            throw CaptureError("the selected microphone is not available")
        }
        let session = AVCaptureSession()
        let input = try AVCaptureDeviceInput(device: device)
        guard session.canAddInput(input) else {
            throw CaptureError("could not open the microphone input")
        }
        session.addInput(input)
        let output = AVCaptureAudioDataOutput()
        output.setSampleBufferDelegate(self, queue: mediaQueue)
        guard session.canAddOutput(output) else {
            throw CaptureError("could not attach the microphone output")
        }
        session.addOutput(output)
        micSession = session
        // Story 19.4: the concrete device now feeding the mic track (the
        // resolved default when `deviceId` was nil) — the identity
        // `MicHealth.decide` matches removals against.
        activeMicDeviceId = device.uniqueID
        // Story 19.4 (closes the 19.3 deferred gap): a yanked device on this
        // path surfaces as a *session runtime error*, not a stream teardown —
        // observe it and route it into the same non-fatal mic-loss branch a
        // device-disconnect notification takes. Never `error`/exit: mic-only
        // loss must not kill the session.
        // Replace (never accumulate) the prior session's runtime-error
        // observer — a fallback re-arms this session (Story 19.4), so one
        // observer must track only the current session.
        if let prior = micSessionRuntimeObserver {
            NotificationCenter.default.removeObserver(prior)
        }
        micSessionRuntimeObserver = NotificationCenter.default.addObserver(
            forName: AVCaptureSession.runtimeErrorNotification, object: session, queue: nil
        ) { [weak self] _ in
            guard let self else { return }
            self.mediaQueue.async {
                self.handleMicLost(removedDeviceId: self.activeMicDeviceId)
            }
        }
        // `startRunning` blocks while the session spins up — keep it off the
        // request loop and `mediaQueue`. Mic samples arriving before the video
        // anchor are dropped by the append guard, exactly like system audio.
        DispatchQueue.global(qos: .userInitiated).async {
            session.startRunning()
        }
    }

    /// Append one microphone sample to the current segment's mic track (Story
    /// 19.3) — on `mediaQueue`, from either OS path (the `.microphone` stream
    /// output on 15+, the parallel `AVCaptureSession` on 13–14). Mirrors the
    /// system-audio guard: samples before the video anchor (or after stop) are
    /// dropped, and the mic simply follows `current`, splitting at the same
    /// rotation handover boundary as system audio.
    private func appendMicSample(_ sampleBuffer: CMSampleBuffer) {
        guard sampleBuffer.isValid, !stopping, sessionStarted,
            let micInput = current?.micInput, micInput.isReadyForMoreMediaData
        else { return }
        let pts = CMSampleBufferGetPresentationTimeStamp(sampleBuffer)
        // Lower-bound PTS trim (Story 19.4): real samples, silence-fill, and a
        // fallback source all share the one written-tail cursor — a sample
        // landing below it (a late buffer from a dying device, a fallback
        // overlapping an already-silence-filled span) would rewind the track's
        // timeline, so it is dropped rather than appended out of order.
        if pts.isNumeric, micPTSLowerBound.isNumeric, CMTimeCompare(pts, micPTSLowerBound) < 0 {
            return
        }
        micInput.append(sampleBuffer)
        // A real mic sample flowing again ends the lost span — the
        // silence-fill (which only pads while `micLost`) yields to it.
        micLost = false
        if pts.isNumeric {
            let duration = CMSampleBufferGetDuration(sampleBuffer)
            micPTSLowerBound = duration.isNumeric ? CMTimeAdd(pts, duration) : pts
        }
    }

    // MARK: - Microphone hot-unplug resilience (Story 19.4)

    /// Drive the identical mic-loss branch a real hardware unplug takes — the
    /// NDJSON-RPC test hook (`simulateMicRemoval`). With no active session
    /// this is a clean no-op (the smoke asserts a clean exit 0); with a
    /// mic-off session `MicHealth.decide` makes it a no-op on `mediaQueue`.
    func simulateMicRemoval() {
        guard isActive else { return }
        mediaQueue.async { [self] in
            handleMicLost(removedDeviceId: activeMicDeviceId)
        }
    }

    /// Observe device disconnects for the mic-loss path (Story 19.4) — the
    /// notification fires for every `AVCaptureDevice`, so non-audio devices
    /// are filtered here; whether THIS removal matters is `MicHealth.decide`'s
    /// call (the same pure branch the simulated removal drives).
    private func installMicDisconnectObserver() {
        let token = NotificationCenter.default.addObserver(
            forName: AVCaptureDevice.wasDisconnectedNotification, object: nil, queue: nil
        ) { [weak self] note in
            guard let self,
                let device = note.object as? AVCaptureDevice, device.hasMediaType(.audio)
            else { return }
            let removedId = device.uniqueID
            self.mediaQueue.async { self.handleMicLost(removedDeviceId: removedId) }
        }
        micObservers.append(token)
    }

    /// The one mic-loss branch (Story 19.4) — on `mediaQueue`, fed by a device
    /// disconnect, a 13–14 session runtime error, or `simulateMicRemoval`
    /// (identical path for all three). NEVER emits `error` and never exits:
    /// mic loss is non-fatal — video + system audio keep rolling, the mic
    /// track is silence-filled, and a fallback to the system default input is
    /// attempted. The host renders the sticky warning; on-hardware silence /
    /// fallback A/V-sync correctness is Story 20.6.
    private func handleMicLost(removedDeviceId: String?) {
        guard !stopping else { return }
        // Idempotent while already lost (Story 19.4): a repeated disconnect
        // notification, the fallback session's own runtime error, or a repeated
        // `simulateMicRemoval` must not re-emit the warning or re-attempt
        // fallback. A real sample flowing again clears `micLost`
        // (`appendMicSample`), so a genuine second loss after recovery still
        // warns.
        guard !micLost else { return }
        let decision = MicHealth.decide(
            micEnabled: wantsMic,
            removedDeviceId: removedDeviceId,
            activeDeviceId: activeMicDeviceId,
            fallbackAvailable: AVCaptureDevice.default(for: .audio) != nil)
        guard decision.shouldWarn else { return }
        emitEvent([
            "event": "warning",
            "code": decision.code,
            "message": decision.message,
        ])
        micLost = true
        startSilenceFill()
        if decision.fallbackToDefault {
            attemptMicFallback()
        }
    }

    /// Attempt to re-feed the mic track from the system default input (Story
    /// 19.4) — on `mediaQueue`. Reuses the 13–14 parallel-AVCaptureSession
    /// machinery on every OS version: on 15+ the SCStream's own `.microphone`
    /// output stays attached, and whichever source delivers first wins via the
    /// PTS trim (real churn behavior is Story 20.6). Best-effort: a failure
    /// downgrades the warning to the honest no-input message (last-write-wins
    /// on the host) and the track stays silence-filled.
    private func attemptMicFallback() {
        // Retire a dead 13–14 session first — off the media queue
        // (`stopRunning` blocks, and its output delivers here).
        if let dead = micSession {
            micSession = nil
            DispatchQueue.global(qos: .userInitiated).async { dead.stopRunning() }
        }
        do {
            try startMicCaptureSession(deviceId: nil)
        } catch {
            emitEvent([
                "event": "warning",
                "code": MicHealth.warningCode,
                "message": MicHealth.noInputMessage,
            ])
        }
    }

    /// Arm the repeating silence-fill (Story 19.4) — on `mediaQueue`. While
    /// the mic is lost the track is padded with generated LPCM silence from
    /// the written tail forward, so the file carries an explicit silent span
    /// instead of a gap (the writer's AAC converter encodes it like any device
    /// format). Armed once; the handler self-gates on `micLost`.
    private func startSilenceFill() {
        guard silenceTimer == nil else { return }
        let timer = DispatchSource.makeTimerSource(queue: mediaQueue)
        timer.schedule(deadline: .now() + .milliseconds(250), repeating: .milliseconds(250))
        timer.setEventHandler { [weak self] in self?.appendSilenceChunk() }
        timer.resume()
        silenceTimer = timer
    }

    /// Append one chunk of silence up to "now" (host clock — the same
    /// timeline SCStream and AVCaptureSession stamp samples with), on
    /// `mediaQueue`. The fill cursor is the shared written-tail lower bound;
    /// a cursor that fell far behind (a stalled queue) is clamped so one tick
    /// never fabricates minutes of audio.
    private func appendSilenceChunk() {
        guard micLost, !stopping, sessionStarted,
            let micInput = current?.micInput, micInput.isReadyForMoreMediaData
        else { return }
        let sampleRate: Int32 = 48_000
        let now = CMClockGetTime(CMClockGetHostTimeClock())
        let maxFill = CMTime(value: CMTimeValue(sampleRate), timescale: sampleRate)  // 1 s
        let tick = CMTime(value: CMTimeValue(sampleRate / 4), timescale: sampleRate)  // 250 ms
        var cursor = micPTSLowerBound
        if !cursor.isNumeric || CMTimeCompare(CMTimeSubtract(now, cursor), maxFill) > 0 {
            cursor = CMTimeSubtract(now, tick)
        }
        let duration = CMTimeSubtract(now, cursor)
        guard duration.isNumeric, duration.seconds > 0 else { return }
        let frames = min(Int(sampleRate), Int((duration.seconds * Double(sampleRate)).rounded(.down)))
        guard frames > 0, let buffer = makeSilenceBuffer(at: cursor, frames: frames) else {
            return
        }
        micInput.append(buffer)
        micPTSLowerBound = CMTimeAdd(
            cursor, CMTime(value: CMTimeValue(frames), timescale: sampleRate))
    }

    /// Build a mono 16-bit 48 kHz LPCM buffer of `frames` zero samples at
    /// `pts` (Story 19.4). Nil on any CoreMedia failure — the tick simply
    /// retries; never a crash.
    private func makeSilenceBuffer(at pts: CMTime, frames: Int) -> CMSampleBuffer? {
        if silenceFormatDescription == nil {
            var asbd = AudioStreamBasicDescription(
                mSampleRate: 48_000, mFormatID: kAudioFormatLinearPCM,
                mFormatFlags: kAudioFormatFlagIsSignedInteger | kAudioFormatFlagIsPacked,
                mBytesPerPacket: 2, mFramesPerPacket: 1, mBytesPerFrame: 2,
                mChannelsPerFrame: 1, mBitsPerChannel: 16, mReserved: 0)
            var format: CMFormatDescription?
            guard
                CMAudioFormatDescriptionCreate(
                    allocator: kCFAllocatorDefault, asbd: &asbd, layoutSize: 0, layout: nil,
                    magicCookieSize: 0, magicCookie: nil, extensions: nil,
                    formatDescriptionOut: &format) == noErr
            else { return nil }
            silenceFormatDescription = format
        }
        guard let format = silenceFormatDescription else { return nil }
        let bytes = frames * 2
        var blockBuffer: CMBlockBuffer?
        guard
            CMBlockBufferCreateWithMemoryBlock(
                allocator: kCFAllocatorDefault, memoryBlock: nil, blockLength: bytes,
                blockAllocator: kCFAllocatorDefault, customBlockSource: nil, offsetToData: 0,
                dataLength: bytes, flags: 0, blockBufferOut: &blockBuffer) == noErr,
            let block = blockBuffer,
            CMBlockBufferFillDataBytes(
                with: 0, blockBuffer: block, offsetIntoDestination: 0, dataLength: bytes) == noErr
        else { return nil }
        var sampleBuffer: CMSampleBuffer?
        guard
            CMAudioSampleBufferCreateReadyWithPacketDescriptions(
                allocator: kCFAllocatorDefault, dataBuffer: block, formatDescription: format,
                sampleCount: frames, presentationTimeStamp: pts, packetDescriptions: nil,
                sampleBufferOut: &sampleBuffer) == noErr
        else { return nil }
        return sampleBuffer
    }

    // MARK: - AVCapture(Audio|Video)DataOutputSampleBufferDelegate (on mediaQueue)

    /// The two AVCapture data-output delegate protocols share this one
    /// selector: the mic's `AVCaptureAudioDataOutput` (macOS 13–14 path +
    /// the 19.4 fallback feed) and the camera's `AVCaptureVideoDataOutput`
    /// (Story 20.1) both deliver here — dispatch on the output type.
    func captureOutput(
        _ output: AVCaptureOutput, didOutput sampleBuffer: CMSampleBuffer,
        from connection: AVCaptureConnection
    ) {
        if output is AVCaptureVideoDataOutput {
            appendCameraSample(sampleBuffer)
        } else {
            appendMicSample(sampleBuffer)
        }
    }

    // MARK: - Webcam as a separate synchronized file (Story 20.1, FR-70)

    /// The `camera-####.mp4` path for segment `index`, beside the
    /// host-supplied screen basename in the same session folder. Same-index
    /// pairing with `screen-####` is the alignment contract (NFR-22).
    private func cameraSegmentPath(index: Int) -> String {
        sessionDirectory + String(format: "camera-%04d.mp4", index)
    }

    /// Stand up the camera leg (Story 20.1): resolve the device (a picked
    /// uniqueID that vanished falls back to the system default camera — the
    /// I/O-matrix contract, never an abort; NO camera at all is an honest
    /// start failure, the mic precedent), read its native pixel size, build
    /// segment 0's video-only writer, and attach an `AVCaptureVideoDataOutput`
    /// delivering on `mediaQueue`. A throw here surfaces as the caller's
    /// single honest `error` event — never a hung recording.
    private func setupCamera(deviceId: String?, screenPath: String) throws {
        let device: AVCaptureDevice?
        if let deviceId {
            device = AVCaptureDevice(uniqueID: deviceId) ?? AVCaptureDevice.default(for: .video)
        } else {
            device = AVCaptureDevice.default(for: .video)
        }
        guard let device else {
            throw CaptureError("the selected camera is not available")
        }
        // The camera file is encoded at the device's native active-format
        // size — the camera never inherits the display's dimensions.
        let dimensions = CMVideoFormatDescriptionGetDimensions(
            device.activeFormat.formatDescription)
        cameraPixelWidth = max(2, Int(dimensions.width))
        cameraPixelHeight = max(2, Int(dimensions.height))
        // `camera-####.mp4` rides beside the host-supplied screen basename.
        if let slash = screenPath.lastIndex(of: "/") {
            sessionDirectory = String(screenPath[...slash])
        } else {
            sessionDirectory = ""
        }
        // The identity `CameraHealth.decide` matches removals against.
        activeCameraDeviceId = device.uniqueID

        let session = AVCaptureSession()
        let input = try AVCaptureDeviceInput(device: device)
        guard session.canAddInput(input) else {
            throw CaptureError("could not open the camera input")
        }
        session.addInput(input)
        let output = AVCaptureVideoDataOutput()
        output.setSampleBufferDelegate(self, queue: mediaQueue)
        guard session.canAddOutput(output) else {
            throw CaptureError("could not attach the camera output")
        }
        session.addOutput(output)
        cameraSession = session
        currentCamera = try makeCameraSegmentWriter(index: 0)
        installCameraDisconnectObserver()
        // A yanked camera on this path can surface as a *session runtime
        // error* rather than a device disconnect — route it into the same
        // non-fatal camera-loss branch. Never `error`/exit: camera-only loss
        // must not kill the session (the 19.4 mic precedent).
        cameraSessionRuntimeObserver = NotificationCenter.default.addObserver(
            forName: AVCaptureSession.runtimeErrorNotification, object: session, queue: nil
        ) { [weak self] _ in
            guard let self else { return }
            self.mediaQueue.async {
                self.handleCameraLost(removedDeviceId: self.activeCameraDeviceId)
            }
        }
        // `startRunning` blocks while the session spins up — keep it off the
        // request loop and `mediaQueue`. Camera samples arriving before the
        // screen anchor are dropped by the append guard, like every other
        // non-video source.
        DispatchQueue.global(qos: .userInitiated).async {
            session.startRunning()
        }
    }

    /// Build one camera segment's video-only writer (fragmented MP4, H.264 at
    /// the device's native size) and start it writing — the `SegmentWriter`
    /// stack with the audio/mic inputs nil. Called for segment 0 from
    /// `setupCamera` and for each screen-driven rotation on `mediaQueue`.
    private func makeCameraSegmentWriter(index: Int) throws -> SegmentWriter {
        let path = cameraSegmentPath(index: index)
        let url = URL(fileURLWithPath: path)
        try FileManager.default.createDirectory(
            at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        let writer = try AVAssetWriter(outputURL: url, fileType: .mp4)
        // Same ~4 s fragment cadence as the screen writer (AD-37): a
        // mid-session kill loses at most the last fragment of either file.
        writer.movieFragmentInterval = CMTime(seconds: 4, preferredTimescale: 600)
        let videoInput = AVAssetWriterInput(
            mediaType: .video,
            outputSettings: [
                AVVideoCodecKey: AVVideoCodecType.h264,
                AVVideoWidthKey: cameraPixelWidth,
                AVVideoHeightKey: cameraPixelHeight,
                AVVideoCompressionPropertiesKey: [
                    // A webcam-sized H.264 stream, not the display's budget.
                    AVVideoAverageBitRateKey: 4_000_000,
                    AVVideoExpectedSourceFrameRateKey: 30,
                    AVVideoProfileLevelKey: AVVideoProfileLevelH264HighAutoLevel,
                ],
            ])
        videoInput.expectsMediaDataInRealTime = true
        guard writer.canAdd(videoInput) else {
            throw CaptureError("could not add the camera H.264 video track")
        }
        writer.add(videoInput)
        guard writer.startWriting() else {
            throw CaptureError(
                "could not start the camera writer: \(writer.error.map(String.init(describing:)) ?? "unknown")"
            )
        }
        return SegmentWriter(
            writer: writer, videoInput: videoInput, audioInput: nil, micInput: nil,
            path: path, index: index)
    }

    /// Append one camera sample to the current camera segment (Story 20.1) —
    /// on `mediaQueue`. Mirrors the mic guard: samples before the SCREEN
    /// anchor (or after stop / after a camera loss) are dropped. The camera
    /// writer is anchored at the SHARED screen session anchor, not its own
    /// first frame — `SCStream` and `AVCaptureSession` both stamp against
    /// `CMClockGetHostTimeClock`, so the two files share one timeline, and
    /// reporting the shared anchor as `ptsStart` keeps segment 0's same-index
    /// starts within one frame even when the camera warms up seconds after
    /// the screen (the NFR-22 alignment gate).
    private func appendCameraSample(_ sampleBuffer: CMSampleBuffer) {
        guard sampleBuffer.isValid, !stopping, !cameraLost, sessionStarted,
            let camera = currentCamera
        else { return }
        let pts = CMSampleBufferGetPresentationTimeStamp(sampleBuffer)
        if !cameraSessionStarted {
            // Anchor at the screen's session anchor, not this (possibly
            // warm-up-delayed) camera frame: appending at `pts` ≥ the anchor
            // is valid (pre-anchor camera samples were dropped above), and
            // the reported `ptsStart` must be the shared boundary or segment
            // 0 would be misaligned by the whole camera warm-up.
            let anchor = sessionAnchorPTS ?? pts
            camera.writer.startSession(atSourceTime: anchor)
            cameraSessionStarted = true
            if anchor.isNumeric { cameraSegmentFirstVideoPTS = anchor.seconds }
        }
        guard camera.videoInput.isReadyForMoreMediaData else { return }
        camera.videoInput.append(sampleBuffer)
        cameraSegmentHasVideo = true
        // Track the last REAL appended PTS (`ptsEnd`) in original
        // capture-clock seconds — the muxer rebases the file's own timeline
        // to 0, so the host-clock bounds exist only here (Story 17.4 rule).
        // `ptsStart` was fixed at anchor time and is never overwritten; the
        // first numeric frame only backfills it when the anchor itself was
        // non-numeric (a NaN would break the segment's JSON event line).
        if pts.isNumeric {
            let seconds = pts.seconds
            if cameraSegmentFirstVideoPTS == nil { cameraSegmentFirstVideoPTS = seconds }
            cameraSegmentLastVideoPTS = seconds
        }
    }

    /// Cut the camera writer at the screen's rotation keyframe PTS — called
    /// from `rotate(...)` inside the SAME `mediaQueue` critical section, so
    /// same-index `screen-####`/`camera-####` files share one boundary. The
    /// screen is the rotation master (AD-37); the camera never rotates on its
    /// own byte budget. Every camera-only failure here is NON-FATAL: the
    /// camera leg ends early with a `cameraLost` warning while the screen
    /// rotation proceeds untouched.
    private func rotateCameraAtScreenBoundary(keyframePTS: CMTime, nextIndex: Int) {
        guard wantsCamera, !cameraLost, let retiring = currentCamera else { return }
        // A camera segment that received no REAL frame cannot be finalized —
        // retire it empty (drop the file) and advance to the new index so
        // same-index pairing holds once frames flow. The replacement is
        // anchored at the shared boundary right away (and marked started, so
        // a later append never lands in an unstarted writer) — its reported
        // `ptsStart` is the boundary, like every rotated segment's.
        guard cameraSessionStarted, cameraSegmentHasVideo else {
            retiring.writer.cancelWriting()
            try? FileManager.default.removeItem(atPath: retiring.path)
            currentCamera = nil
            do {
                let next = try makeCameraSegmentWriter(index: nextIndex)
                next.writer.startSession(atSourceTime: keyframePTS)
                cameraSessionStarted = true
                cameraSegmentFirstVideoPTS = keyframePTS.isNumeric ? keyframePTS.seconds : nil
                cameraSegmentLastVideoPTS = nil
                cameraSegmentHasVideo = false
                currentCamera = next
            } catch {
                failCameraNonFatally(
                    message: "camera segment rotation failed — the camera file ends here; "
                        + "screen recording continues")
            }
            return
        }
        let next: SegmentWriter
        do {
            next = try makeCameraSegmentWriter(index: nextIndex)
        } catch {
            failCameraNonFatally(
                message: "camera segment rotation failed — the camera file ends here; "
                    + "screen recording continues")
            return
        }
        // The retiring camera segment's host-clock PTS bounds — read here,
        // before the swap resets them (the Story 17.4 screen rule).
        let retiringPTSStart = cameraSegmentFirstVideoPTS
        let retiringPTSEnd = cameraSegmentLastVideoPTS
        // The new camera file opens at the SAME screen keyframe boundary PTS,
        // and the BOUNDARY is its reported `ptsStart` — not the next camera
        // frame, which may lag or drop at the cut — keeping same-index starts
        // within one frame period (the NFR-22 alignment gate). A non-numeric
        // cut PTS leaves the bound nil rather than seeding a NaN (the screen
        // rotation's rule); the first numeric frame then backfills it.
        next.writer.startSession(atSourceTime: keyframePTS)
        currentCamera = next
        cameraSegmentFirstVideoPTS = keyframePTS.isNumeric ? keyframePTS.seconds : nil
        cameraSegmentLastVideoPTS = nil
        cameraSegmentHasVideo = false
        retiring.videoInput.markAsFinished()
        finalizeGroup.enter()
        retiring.writer.finishWriting { [self] in
            mediaQueue.async { [self] in
                if retiring.writer.status == .completed {
                    // While stopping, the host is in `Stopping` — a
                    // segmentClosed now would be an illegal transition; the
                    // stop path owns all remaining signalling.
                    if !stopping {
                        var closed: [String: Any] = [
                            "event": "segmentClosed",
                            "index": retiring.index,
                            "path": retiring.path,
                            "bytes": onDiskBytes(atPath: retiring.path),
                            "track": "camera",
                        ]
                        if let retiringPTSStart { closed["ptsStart"] = retiringPTSStart }
                        if let retiringPTSEnd { closed["ptsEnd"] = retiringPTSEnd }
                        emitEvent(closed)
                    }
                } else if !stopping {
                    // A camera-only finalize failure is non-fatal by contract
                    // — warn, never `error`, never exit (screen owns the
                    // session's fate). It is terminal for the CAMERA leg,
                    // though: by the time this retiring-writer completion runs
                    // the replacement writer is already live, so route through
                    // `failCameraNonFatally` to stop and drop it too. Otherwise
                    // the sticky `cameraLost` warning ("the camera file ends
                    // here") would fire while later camera segments keep
                    // recording — a lie. This mirrors every other camera-fault
                    // path (device loss, rotation-writer creation failure).
                    failCameraNonFatally(
                        message: "camera segment finalize failed — the camera file ends here; "
                            + "screen recording continues")
                }
                finalizeGroup.leave()
            }
        }
    }

    /// Drive the identical camera-loss branch a real hardware unplug takes —
    /// the NDJSON-RPC test hook (`simulateCameraRemoval`). With no active
    /// session this is a clean no-op; with a camera-off session
    /// `CameraHealth.decide` makes it a no-op on `mediaQueue`.
    func simulateCameraRemoval() {
        guard isActive else { return }
        mediaQueue.async { [self] in
            handleCameraLost(removedDeviceId: activeCameraDeviceId)
        }
    }

    /// Observe device disconnects for the camera-loss path — the notification
    /// fires for every `AVCaptureDevice`, so non-video devices are filtered
    /// here; whether THIS removal matters is `CameraHealth.decide`'s call
    /// (the same pure branch the simulated removal drives).
    private func installCameraDisconnectObserver() {
        let token = NotificationCenter.default.addObserver(
            forName: AVCaptureDevice.wasDisconnectedNotification, object: nil, queue: nil
        ) { [weak self] note in
            guard let self,
                let device = note.object as? AVCaptureDevice, device.hasMediaType(.video)
            else { return }
            let removedId = device.uniqueID
            self.mediaQueue.async { self.handleCameraLost(removedDeviceId: removedId) }
        }
        cameraObservers.append(token)
    }

    /// The one camera-loss branch (Story 20.1) — on `mediaQueue`, fed by a
    /// device disconnect, a camera-session runtime error, or
    /// `simulateCameraRemoval` (identical path for all three). NEVER emits
    /// `error` and never exits: camera loss is non-fatal — the screen (and
    /// audio) keep rolling, the current `camera-####.mp4` finalizes early
    /// (no black-fill), and the host renders the sticky `cameraLost` warning.
    private func handleCameraLost(removedDeviceId: String?) {
        guard !stopping, !cameraLost else { return }
        let decision = CameraHealth.decide(
            cameraEnabled: wantsCamera,
            removedDeviceId: removedDeviceId,
            activeDeviceId: activeCameraDeviceId)
        guard decision.shouldWarn else { return }
        emitEvent([
            "event": "warning",
            "code": decision.code,
            "message": decision.message,
        ])
        cameraLost = true
        finalizeCameraEarly()
    }

    /// Non-fatal camera fault with an honest, cause-specific message (a
    /// rotation writer failure rather than a device removal): same warning
    /// code, same early finalize — the wire consumer sees one `cameraLost`
    /// surface either way.
    private func failCameraNonFatally(message: String) {
        guard !cameraLost else { return }
        cameraLost = true
        emitEvent([
            "event": "warning",
            "code": CameraHealth.warningCode,
            "message": message,
        ])
        finalizeCameraEarly()
    }

    /// Finalize the current camera file early (Story 20.1) — on `mediaQueue`.
    /// The camera leg ends here for the session: the capture session winds
    /// down (off the media queue — `stopRunning` blocks), the writer's `moov`
    /// is written so the partial file stays playable, and its `segmentClosed
    /// {track:"camera"}` reports the host-clock bounds it accumulated. A
    /// writer without a REAL appended frame — un-anchored OR anchored at the
    /// boundary but frameless — is cancelled and its file dropped: a
    /// zero-frame `camera-####.mp4` would be unplayable noise in the ledger.
    private func finalizeCameraEarly() {
        if let session = cameraSession {
            cameraSession = nil
            DispatchQueue.global(qos: .userInitiated).async { session.stopRunning() }
        }
        guard let camera = currentCamera else { return }
        currentCamera = nil
        guard cameraSessionStarted, camera.writer.status == .writing,
            cameraSegmentHasVideo
        else {
            camera.writer.cancelWriting()
            try? FileManager.default.removeItem(atPath: camera.path)
            return
        }
        let ptsStart = cameraSegmentFirstVideoPTS
        let ptsEnd = cameraSegmentLastVideoPTS
        camera.videoInput.markAsFinished()
        finalizeGroup.enter()
        camera.writer.finishWriting { [self] in
            mediaQueue.async { [self] in
                if camera.writer.status == .completed, !stopping {
                    var closed: [String: Any] = [
                        "event": "segmentClosed",
                        "index": camera.index,
                        "path": camera.path,
                        "bytes": onDiskBytes(atPath: camera.path),
                        "track": "camera",
                    ]
                    if let ptsStart { closed["ptsStart"] = ptsStart }
                    if let ptsEnd { closed["ptsEnd"] = ptsEnd }
                    emitEvent(closed)
                }
                finalizeGroup.leave()
            }
        }
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

        // Story 20.1 (FR-70): cut the camera writer at the SAME keyframe PTS
        // in the same critical section — the screen is the rotation master;
        // same-index screen/camera segments share this one boundary.
        rotateCameraAtScreenBoundary(keyframePTS: keyframePTS, nextIndex: retiring.index + 1)

        retiring.videoInput.markAsFinished()
        retiring.audioInput?.markAsFinished()
        retiring.micInput?.markAsFinished()
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
        // Story 19.4: a device removal during teardown must not fire the
        // mic-loss path into a winding-down engine (the `stopping` flag also
        // guards, but removal is cheap and final).
        for token in micObservers {
            NotificationCenter.default.removeObserver(token)
        }
        micObservers.removeAll()
        if let token = micSessionRuntimeObserver {
            NotificationCenter.default.removeObserver(token)
            micSessionRuntimeObserver = nil
        }
        if let micSession {
            // Story 19.3: wind down the 13–14 mic session off the media queue
            // (`stopRunning` blocks, and its output delivers on `mediaQueue` —
            // stopping from there could deadlock on an in-flight callback).
            // Late mic samples are dropped by the `stopping` flag either way.
            DispatchQueue.global(qos: .userInitiated).async {
                micSession.stopRunning()
            }
            // Story 19.4 (closes the 19.3 deferred gap): drop the handle so no
            // later path can touch a wound-down session.
            self.micSession = nil
        }
        // Story 20.1: the camera teardown mirrors the mic's — observers gone
        // first (a removal during teardown must not fire the camera-loss path
        // into a winding-down engine), then the session wound down off the
        // media queue. Late camera samples are dropped by `stopping`.
        for token in cameraObservers {
            NotificationCenter.default.removeObserver(token)
        }
        cameraObservers.removeAll()
        if let token = cameraSessionRuntimeObserver {
            NotificationCenter.default.removeObserver(token)
            cameraSessionRuntimeObserver = nil
        }
        if let cameraSession {
            DispatchQueue.global(qos: .userInitiated).async {
                cameraSession.stopRunning()
            }
            self.cameraSession = nil
        }
        guard let stream else {
            // Stop before capture ever started: nothing to finalize.
            emitEvent(["event": "error", "message": "stop before capture started"])
            exit(0)
        }
        mediaQueue.async {
            self.stopping = true
            // Story 19.4: the silence-fill has nothing more to pad.
            self.silenceTimer?.cancel()
            self.silenceTimer = nil
        }
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
            // Story 20.1: the screen never anchored, so the camera never
            // appended either — cancel its empty writer and drop the file so
            // a no-frames stop leaves no orphan `camera-0000.mp4` behind.
            if let camera = currentCamera {
                currentCamera = nil
                camera.writer.cancelWriting()
                try? FileManager.default.removeItem(atPath: camera.path)
            }
            emitEvent([
                "event": "error",
                "message": "no frames were captured before stop",
            ])
            exit(0)
        }
        // Story 20.1: finalize the camera's FINAL segment alongside the
        // screen's — like the screen's final segment it emits no
        // `segmentClosed` (the host is Stopping; the terminal disk reconcile
        // lists it with honest null bounds). A camera writer without a REAL
        // appended frame (anchored at the boundary or not) is cancelled and
        // its file dropped instead — never finalized into a zero-frame file.
        // A camera already finalized early (loss) left `currentCamera` nil —
        // nothing to do.
        if let camera = currentCamera {
            currentCamera = nil
            if cameraSessionStarted, camera.writer.status == .writing,
                cameraSegmentHasVideo
            {
                camera.videoInput.markAsFinished()
                finalizeGroup.enter()
                camera.writer.finishWriting { self.finalizeGroup.leave() }
            } else {
                camera.writer.cancelWriting()
                try? FileManager.default.removeItem(atPath: camera.path)
            }
        }
        current.videoInput.markAsFinished()
        current.audioInput?.markAsFinished()
        current.micInput?.markAsFinished()
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
                // The write-once session anchor: the camera's segment-0
                // boundary (`segmentStartPTS` holds the same value now but is
                // rebased on every rotation, so the camera reads this field).
                sessionAnchorPTS = pts
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
            // `.microphone` (macOS 15+, Story 19.3) rides the same SCStream —
            // route it to the current segment's own mic track. Any other
            // future output type is dropped.
            if #available(macOS 15.0, *), type == .microphone {
                appendMicSample(sampleBuffer)
            }
        }
    }

    // MARK: - SCStreamDelegate

    func stream(_ stream: SCStream, didStopWithError error: Error) {
        // The OS tore the stream down (display unplugged, session revoked…):
        // surface honestly and salvage nothing here (recovery is Story 17.3).
        // Deliberately fatal ONLY for whole-stream loss (Story 19.4): a
        // mic-only fault never routes here — it takes the non-fatal
        // `handleMicLost` warning path instead.
        emitEvent([
            "event": "error",
            "message": "capture stopped: \(String(describing: error))",
        ])
        exit(0)
    }
}
