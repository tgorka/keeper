// SPDX-License-Identifier: Apache-2.0
//
// keeper-rec — first-party screen-recording capture sidecar for keeper.
//
// NDJSON-RPC request loop (Story 16.4): one JSON object per line on stdio. The
// host writes id-correlated requests ({"id":<n>,"method":"..."}); this loop
// answers each with {"id":<n>,"result":{...}} or {"id":<n>,"error":{...}} and
// exits cleanly at EOF. Real capture (ScreenCaptureKit / AVFoundation) lands in
// 16.6 — today only the two read-only methods are served:
//
// - getCapabilities: protocol-version handshake + macOS version + feature flags
//   + per-TCC permission states. `screenRecording` is real (CoreGraphics
//   preflight — non-prompting, and authoritative because THIS process is the
//   one that will capture); `microphone` (Story 19.3) and `camera` (Story
//   20.1) are the real, non-prompting AVFoundation tri-states.
// - listSources: real active displays via CoreGraphics, real applications via
//   SCShareableContent (19.1), real microphones (19.3) and real cameras
//   (20.1) via AVFoundation.
//
// Cap #1722: macOS 15+ silently rejects ad-hoc-signed ScreenCaptureKit — real
// capture builds need an Apple Development certificate (a DevEx requirement, not
// a product blocker). This request loop itself needs no special signing.

import AppKit
import AVFoundation
import CoreGraphics
import Foundation
import ScreenCaptureKit

// If the parent closes the read end of our stdout before we finish writing, a
// SIGPIPE would terminate us with signal 13 (exit 141) — violating the invariant
// that this sidecar always exits cleanly. Ignore it so the write path can fail as
// a recoverable error instead of a signal.
signal(SIGPIPE, SIG_IGN)

/// Serializes every stdout write: RPC replies leave the main request loop while
/// capture events (Story 16.6) arrive from dispatch queues — interleaved bytes
/// would corrupt the NDJSON framing.
private let stdoutLock = NSLock()

/// Write a JSON object followed by a newline to stdout, safely (no forced unwrap).
/// Returns false if serialization or the write fails so callers can exit cleanly.
func writeLine(_ object: [String: Any]) -> Bool {
    guard let data = try? JSONSerialization.data(withJSONObject: object) else {
        return false
    }
    stdoutLock.lock()
    defer { stdoutLock.unlock() }
    let handle = FileHandle.standardOutput
    // The throwing `write(contentsOf:)` surfaces a broken pipe as a Swift error
    // (caught by `try?`) rather than raising an uncaught ObjC exception.
    guard (try? handle.write(contentsOf: data)) != nil else { return false }
    if let newline = "\n".data(using: .utf8) {
        _ = try? handle.write(contentsOf: newline)
    }
    return true
}

/// Emit one capture-progress NDJSON event line (Story 16.6). Best-effort: a
/// broken pipe here means the host is gone; the capture teardown paths handle
/// process exit themselves.
func emitEvent(_ object: [String: Any]) {
    _ = writeLine(object)
}

/// Map this process's AVFoundation authorization for `mediaType` onto the wire's
/// TCC tri-state string (Story 19.3). Unlike the Screen Recording preflight,
/// AVFoundation reports the authoritative granted / denied / notDetermined
/// directly and without prompting; `.restricted` counts as denied (the user
/// cannot grant it, so the honest surface is the denied fix-path).
private func avPermissionString(for mediaType: AVMediaType) -> String {
    switch AVCaptureDevice.authorizationStatus(for: mediaType) {
    case .authorized: return "granted"
    case .denied, .restricted: return "denied"
    case .notDetermined: return "notDetermined"
    @unknown default: return "notDetermined"
    }
}

/// The per-TCC permission states, keyed exactly as the wire contract expects.
///
/// `screenRecording` uses the real, non-prompting CoreGraphics preflight of THIS
/// process's Screen Recording grant (keeper-rec is the process that captures in
/// 16.6, so its grant is the one that matters). The boolean preflight is
/// two-valued, so it can only confirm `granted`; a false is reported as
/// `notDetermined` rather than `denied`, because the preflight genuinely cannot
/// tell an explicit denial from a never-requested state — asserting `denied` for
/// a first-run user would wrongly steer 16.5 to a dead-end "open System Settings"
/// prompt. The authoritative granted / not-yet-requested / denied tri-state
/// (request, prompt, deep-link, live re-detection) is Story 16.5's pre-flight
/// surface. `microphone` is the real, non-prompting AVFoundation tri-state
/// (Story 19.3); `camera` stays provisional "notDetermined" until 20.x.
private func permissionsPayload() -> [String: Any] {
    let screenRecording = CGPreflightScreenCaptureAccess() ? "granted" : "notDetermined"
    return [
        "screenRecording": screenRecording,
        "microphone": avPermissionString(for: .audio),
        // Real since Story 20.1 (FR-70): the same non-prompting AVFoundation
        // tri-state as the microphone, for the `.video` media type.
        "camera": avPermissionString(for: .video),
    ]
}

/// The `getCapabilities` result: protocol version (handshake), macOS version,
/// feature flags, and permission states.
private func capabilitiesResult() -> [String: Any] {
    let version = ProcessInfo.processInfo.operatingSystemVersion
    let macos = "\(version.majorVersion).\(version.minorVersion).\(version.patchVersion)"
    return [
        // MUST stay in lockstep with `keeper_core::recording::PROTOCOL_VERSION`
        // (the Rust host). The two constants have no shared source of truth across
        // the language boundary; a drift is caught at runtime by the host's
        // handshake (→ a clean Unsupported), but bump both together on purpose.
        // Story 17.4 added the additive `ptsStart`/`ptsEnd` fields (original
        // capture-clock seconds) to the `segmentClosed` event — consumed
        // tolerantly by the host parser, so per the additive-change precedent
        // (16.5's requestScreenRecording, 16.6's startRecording/stop) the
        // version stays 1.
        // Story 19.5 added the additive `fps` field to `startRecording`
        // (always emitted by the host, decoded best-effort here with a default
        // of 30) — per the same additive precedent the version stays 1.
        // Story 20.1 added the additive `cameraEnabled`/`cameraDeviceId`
        // startRecording fields (absent = camera off, preserving the pre-20.1
        // wire), the `requestCamera` / `simulateCameraRemoval` methods, and
        // the camera `segmentClosed{track:"camera"}` lines — all additive, so
        // the version stays 1.
        "protocolVersion": 1,
        "macos": macos,
        "features": [
            // System-audio capture is supported on the macOS 13+ floor this
            // sidecar is built for (Package.swift platforms: .macOS(.v13)).
            "systemAudio": true,
            // Microphone capture is live (Story 19.3, AD-36): in-stream on
            // macOS 15+, a parallel AVCaptureSession on 13–14 — supported on
            // the whole floor either way, so the flag never leaks the split.
            "microphone": true,
            // Camera capture is live (Story 20.1, FR-70, AD-37): a separate
            // `camera-####.mp4` per segment from a second in-process
            // AVAssetWriter, supported on the whole macOS 13+ floor.
            "camera": true,
        ],
        "permissions": permissionsPayload(),
    ]
}

/// Enumerate the active displays via CoreGraphics — real values, no
/// ScreenCaptureKit needed. A CoreGraphics failure degrades to an empty list
/// (honest, never a crash).
private func listDisplays() -> [[String: Any]] {
    var count: UInt32 = 0
    guard CGGetActiveDisplayList(0, nil, &count) == .success, count > 0 else {
        return []
    }
    var ids = [CGDirectDisplayID](repeating: 0, count: Int(count))
    guard CGGetActiveDisplayList(count, &ids, &count) == .success else {
        return []
    }
    return ids.prefix(Int(count)).map { id in
        [
            "id": id,
            "width": CGDisplayPixelsWide(id),
            "height": CGDisplayPixelsHigh(id),
            "isMain": CGDisplayIsMain(id) != 0,
        ]
    }
}

/// keeper's own bundle id — never offered as an application capture target
/// (Story 19.1: keeper can never record itself, and app-scoped capture already
/// excludes it from the file). Matches the bundle id in keeper's Info.plist.
private let keeperBundleId = "dev.tgorka.keeper"

/// Render one application's icon as a bounded (≤64×64px) PNG `data:image/png;
/// base64,…` URI (Story 19.1), or `nil` when no icon can be produced — the
/// picker then shows a generic glyph. Kept small so the polled source list never
/// becomes a large-payload-over-IPC violation. Nil-safe throughout: any failure
/// (no running app, no icon, encode failure) degrades to `nil`, never a crash.
private func iconDataURI(forPid pid: pid_t) -> String? {
    guard let running = NSRunningApplication(processIdentifier: pid),
        let icon = running.icon
    else {
        return nil
    }
    // Downscale into a 64×64 bitmap and PNG-encode it. Drawing into a fixed
    // NSBitmapImageRep bounds the payload regardless of the source icon size.
    let side = 64
    guard
        let rep = NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: side, pixelsHigh: side,
            bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
            colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)
    else {
        return nil
    }
    rep.size = NSSize(width: side, height: side)
    guard let context = NSGraphicsContext(bitmapImageRep: rep) else {
        return nil
    }
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = context
    icon.draw(
        in: NSRect(x: 0, y: 0, width: side, height: side),
        from: .zero, operation: .copy, fraction: 1.0)
    NSGraphicsContext.restoreGraphicsState()
    guard let png = rep.representation(using: .png, properties: [:]) else {
        return nil
    }
    return "data:image/png;base64,\(png.base64EncodedString())"
}

/// Enumerate the recordable applications via `SCShareableContent` (Story 19.1):
/// real running apps that own at least one on-screen window, keeper's own bundle
/// excluded (it can never be a target), deduped by pid, name-sorted, each with
/// name/pid/bundleId + an optional ≤64px PNG icon data-URI. An enumeration
/// failure (ungranted / ad-hoc-rejected process) degrades to an empty list —
/// honest, never a hang or crash (the pre-flight, Story 16.5, owns the fix path).
private func listApplications() async -> [[String: Any]] {
    guard
        let content = try? await SCShareableContent.excludingDesktopWindows(
            false, onScreenWindowsOnly: true)
    else {
        return []
    }
    // Only apps that own ≥1 on-screen window are recordable targets. The window
    // list is already on-screen-only above; collect the distinct owning pids.
    var pidsWithWindows = Set<pid_t>()
    for window in content.windows where window.isOnScreen {
        if let owner = window.owningApplication {
            pidsWithWindows.insert(owner.processID)
        }
    }
    var seen = Set<pid_t>()
    var apps: [[String: Any]] = []
    for app in content.applications {
        let pid = app.processID
        guard pidsWithWindows.contains(pid), !seen.contains(pid) else { continue }
        let bundleId = app.bundleIdentifier
        // keeper can never be a capture target (it also can never appear in an
        // app-scoped file); drop it from the offered list.
        guard bundleId != keeperBundleId else { continue }
        seen.insert(pid)
        var entry: [String: Any] = [
            "bundleId": bundleId,
            "name": app.applicationName,
            "pid": Int(pid),
        ]
        if let icon = iconDataURI(forPid: pid) {
            entry["icon"] = icon
        }
        apps.append(entry)
    }
    // Name-sorted (case-insensitive, locale-aware) for a stable picker order.
    apps.sort { lhs, rhs in
        let a = (lhs["name"] as? String) ?? ""
        let b = (rhs["name"] as? String) ?? ""
        return a.localizedCaseInsensitiveCompare(b) == .orderedAscending
    }
    return apps
}

/// Enumerate the audio-input devices via `AVCaptureDevice.DiscoverySession`
/// (Story 19.3): real microphones as `{id, name}` rows for the Audio card's
/// device picker ("System default input" is the picker's own first row — the
/// host never needs a device id for the default). The macOS-14 device-type
/// rename is handled per-availability; a host with no input devices degrades to
/// an empty list — honest, never a crash.
private func listMicrophones() -> [[String: Any]] {
    let deviceTypes: [AVCaptureDevice.DeviceType]
    if #available(macOS 14.0, *) {
        deviceTypes = [.microphone, .external]
    } else {
        deviceTypes = [.builtInMicrophone, .externalUnknown]
    }
    let discovery = AVCaptureDevice.DiscoverySession(
        deviceTypes: deviceTypes, mediaType: .audio, position: .unspecified)
    return discovery.devices.map { device in
        ["id": device.uniqueID, "name": device.localizedName]
    }
}

/// Enumerate the video-input devices via `AVCaptureDevice.DiscoverySession`
/// (Story 20.1, FR-70): real cameras as flat `{id, name}` rows for the Webcam
/// card's device picker ("System default camera" is the picker's own first
/// row — the host never needs a device id for the default). No device-class
/// grouping on purpose: `localizedName` already distinguishes built-in /
/// external / Continuity Camera. The macOS-14 type-name split is handled
/// per-availability (the `listMicrophones` precedent): `.external` and the
/// explicit `.continuityCamera` type exist on 14+ only — on 13 a Continuity
/// Camera still enumerates through the legacy `.externalUnknown` type, so no
/// device class is lost. A host with no camera degrades to an empty list —
/// honest, never a crash.
private func listCameras() -> [[String: Any]] {
    let deviceTypes: [AVCaptureDevice.DeviceType]
    if #available(macOS 14.0, *) {
        deviceTypes = [.builtInWideAngleCamera, .external, .continuityCamera, .deskViewCamera]
    } else {
        deviceTypes = [.builtInWideAngleCamera, .externalUnknown, .deskViewCamera]
    }
    let discovery = AVCaptureDevice.DiscoverySession(
        deviceTypes: deviceTypes, mediaType: .video, position: .unspecified)
    return discovery.devices.map { device in
        ["id": device.uniqueID, "name": device.localizedName]
    }
}

/// The `listSources` result (Story 16.4 → 20.1): real displays (CoreGraphics),
/// real applications (SCShareableContent), real microphones and real cameras
/// (AVFoundation). `async` because application enumeration awaits shareable
/// content.
private func sourcesResult() async -> [String: Any] {
    return [
        "displays": listDisplays(),
        "applications": await listApplications(),
        "microphones": listMicrophones(),
        "cameras": listCameras(),
    ]
}

// The one capture session this process can host (Story 16.6). Created lazily on
// the first `startRecording`; main-thread only.
let captureEngine = CaptureEngine()

// The request loop: read lines until EOF. A malformed line (not JSON, not an
// object, no string method) is skipped — never a crash, never garbage output. An
// unknown method gets an id-correlated {id,error} answer so the host can fail
// honestly instead of hanging. EOF (the host closed our stdin) → clean exit 0,
// or — when a capture is live — a graceful stop-and-finalize (a vanished host
// must not leave an unplayable file when a clean finalize is still possible).
while let line = readLine(strippingNewline: true) {
    guard !line.isEmpty,
          let data = line.data(using: .utf8),
          let msg = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
    else {
        continue
    }
    let id = msg["id"] ?? NSNull()
    guard let method = msg["method"] as? String else {
        continue
    }
    let response: [String: Any]
    switch method {
    case "getCapabilities":
        response = ["id": id, "result": capabilitiesResult()]
    case "listSources":
        // Story 19.1: application enumeration is async (`SCShareableContent`),
        // so answer off a `Task` and serialize the reply through the same
        // `writeLine`/`stdoutLock` the request loop uses — never interleaving
        // bytes with a concurrent reply. A failed write means the host is gone;
        // there is nothing to exit for here (the loop exits at EOF).
        Task {
            _ = writeLine(["id": id, "result": await sourcesResult()])
        }
        continue
    case "requestScreenRecording":
        // Story 16.5: ask TCC for Screen Recording access. Returns immediately
        // with the current grant; where the state is undetermined the OS posts
        // its one real prompt per app lifetime asynchronously (attributed to
        // keeper — this sidecar is keeper's own child process, and the usage
        // string lives in keeper's bundle Info.plist). A prior denial shows no
        // prompt and reads back false — the host resolves that to its honest
        // denied-with-fix-path (System Settings deep link).
        response = ["id": id, "result": ["granted": CGRequestScreenCaptureAccess()]]
    case "requestMicrophone":
        // Story 19.3 (FR-69, AD-36): ask TCC for microphone access — the host
        // only ever sends this lazily, when the user enables the mic source.
        // Where the state is undetermined the OS posts its one real prompt per
        // app lifetime (attributed to keeper — this sidecar is keeper's child
        // process; the usage string is keeper's NSMicrophoneUsageDescription)
        // and `requestAccess` resolves once the user answers; an already-decided
        // state resolves immediately with no prompt. The reply carries the
        // authoritative post-request tri-state, serialized through the same
        // `writeLine`/`stdoutLock` as every concurrent event line.
        Task {
            _ = await AVCaptureDevice.requestAccess(for: .audio)
            _ = writeLine(["id": id, "result": ["status": avPermissionString(for: .audio)]])
        }
        continue
    case "requestCamera":
        // Story 20.1 (FR-70, AD-36): ask TCC for camera access — the host
        // only ever sends this lazily, when the user enables the Webcam
        // switch (never preemptively; the mic precedent verbatim). Where the
        // state is undetermined the OS posts its one real prompt per app
        // lifetime (attributed to keeper — this sidecar is keeper's child
        // process; the usage string is keeper's NSCameraUsageDescription) and
        // `requestAccess` resolves once the user answers; an already-decided
        // state resolves immediately with no prompt. The reply carries the
        // authoritative post-request tri-state.
        Task {
            _ = await AVCaptureDevice.requestAccess(for: .video)
            _ = writeLine(["id": id, "result": ["status": avPermissionString(for: .video)]])
        }
        continue
    case "startRecording":
        // Story 16.6: begin the one capture session this process can host.
        // Progress flows as NDJSON *events* (preflight → recording → … ), not
        // through this reply — the reply only acknowledges the request shape.
        let params = msg["params"] as? [String: Any]
        guard let path = params?["path"] as? String, !path.isEmpty else {
            response = [
                "id": id,
                "error": ["code": "badParams", "message": "startRecording needs params.path"],
            ]
            break
        }
        guard !captureEngine.isActive else {
            response = [
                "id": id,
                "error": ["code": "busy", "message": "a capture session is already active"],
            ]
            break
        }
        let displayId = (params?["displayId"] as? NSNumber)?.uint32Value
        // Story 19.1: an optional application capture target. When present the
        // engine scopes capture to that app's windows (exclusionary) and ignores
        // `displayId`; the app is re-resolved live against SCShareableContent at
        // start, so a vanished pid fails with an honest `error` event (never a
        // hung recording). `bundleId` is informational for the engine.
        let applicationPid = (params?["applicationPid"] as? NSNumber)?.int32Value
        // The app's bundle id (when app-scoped): the engine re-resolves the pid
        // against live shareable content matching BOTH pid and bundle id, so a
        // recycled pid can't capture the wrong app.
        let applicationBundleId = params?["bundleId"] as? String
        let systemAudio = (params?["systemAudio"] as? Bool) ?? true
        // Story 19.3: the optional microphone leg — off unless explicitly
        // enabled (absent `micEnabled` means off, preserving the pre-19.3
        // wire); `micDeviceId` nil = the system default input. The mic is
        // written as its own AAC track, never premixed (AD-36).
        let micEnabled = (params?["micEnabled"] as? Bool) ?? false
        let micDeviceId = params?["micDeviceId"] as? String
        // Story 20.1: the optional webcam leg — off unless explicitly enabled
        // (absent `cameraEnabled` means off, preserving the pre-20.1 wire);
        // `cameraDeviceId` nil = the system default camera. The camera is
        // written as its own separate `camera-####.mp4` file per segment,
        // never a track inside `screen-####` (FR-70, AD-37).
        let cameraEnabled = (params?["cameraEnabled"] as? Bool) ?? false
        let cameraDeviceId = params?["cameraDeviceId"] as? String
        // Story 17.1: optional segmenting knobs, additive to the v1 protocol
        // (Story 17.5 later feeds configured values from keeper.db). Missing
        // fields fall back to the authored defaults; non-positive values are
        // clamped inside RotationPolicy.
        let segmentMB =
            (params?["segmentMB"] as? NSNumber)?.intValue
            ?? RotationPolicy.defaultSegmentMB
        let maxSegmentSeconds =
            (params?["maxSegmentSeconds"] as? NSNumber)?.intValue
            ?? RotationPolicy.defaultMaxSegmentSeconds
        // Story 19.5: the capture frame rate — additive like `segmentMB`, the
        // host always emits it, but decode best-effort with the 30 default so
        // an older host stays compatible. The engine normalizes to {30, 60}
        // via `normalizeFps` before it reaches SCStreamConfiguration.
        let fps = (params?["fps"] as? NSNumber)?.intValue ?? 30
        response = ["id": id, "result": ["starting": true]]
        _ = writeLine(response)
        captureEngine.start(
            path: path, displayId: displayId, applicationPid: applicationPid,
            applicationBundleId: applicationBundleId, systemAudio: systemAudio,
            micEnabled: micEnabled, micDeviceId: micDeviceId,
            cameraEnabled: cameraEnabled, cameraDeviceId: cameraDeviceId,
            segmentMB: segmentMB, maxSegmentSeconds: maxSegmentSeconds, fps: fps)
        continue
    case "simulateMicRemoval":
        // Story 19.4: drive the IDENTICAL mic-loss branch a real hardware
        // unplug takes (CaptureEngine.handleMicLost → MicHealth.decide →
        // non-fatal warning + silence-fill + fallback). A test/simulation
        // hook only — the host never sends it in production. With no active
        // session (or a mic-off one) it is a clean no-op: the reply below
        // still answers, and EOF still exits 0 (the smoke asserts this).
        response = ["id": id, "result": ["simulated": captureEngine.isActive]]
        _ = writeLine(response)
        captureEngine.simulateMicRemoval()
        continue
    case "simulateCameraRemoval":
        // Story 20.1: drive the IDENTICAL camera-loss branch a real hardware
        // unplug (a Continuity Camera walking away) takes —
        // `CaptureEngine.handleCameraLost` → `CameraHealth.decide` →
        // non-fatal `cameraLost` warning + early camera-file finalize. A
        // test/simulation hook only, the `simulateMicRemoval` twin: with no
        // active session (or a camera-off one) it is a clean no-op.
        response = ["id": id, "result": ["simulated": captureEngine.isActive]]
        _ = writeLine(response)
        captureEngine.simulateCameraRemoval()
        continue
    case "stop":
        // Story 16.6: stop-and-finalize. The engine emits `stopping` /
        // `finalized` events and exits the process itself once the file's
        // `moov` is written — this loop just keeps draining stdin until then.
        response = ["id": id, "result": ["stopping": true]]
        _ = writeLine(response)
        captureEngine.stop()
        continue
    default:
        response = [
            "id": id,
            "error": [
                "code": "unknownMethod",
                "message": "unknown method: \(method)",
            ],
        ]
    }
    // A failed write means stdout broke (the host closed the read end): stop the
    // loop and exit cleanly rather than spinning on a dead pipe — upholds the
    // "keeper-rec always exits cleanly" invariant.
    if !writeLine(response) {
        break
    }
}

// EOF. With a live capture, the host vanished mid-recording (crash / kill of the
// pipe, not a clean `stop`): finalize what we have so the file stays playable,
// then let the engine's completion path exit. `dispatchMain()` parks the main
// thread so the media queue can finish; the engine always exits the process.
if captureEngine.isActive {
    captureEngine.stop()
    dispatchMain()
} else {
    exit(0)
}
