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
//   one that will capture); `microphone`/`camera` stay provisional
//   "notDetermined" until AVFoundation detection lands (16.6/19).
// - listSources: real active displays via CoreGraphics; applications (needs
//   SCShareableContent) and microphones/cameras (need AVFoundation) are empty
//   arrays — the shape is locked, enumeration is deferred (16.6/19).
//
// Cap #1722: macOS 15+ silently rejects ad-hoc-signed ScreenCaptureKit — real
// capture builds need an Apple Development certificate (a DevEx requirement, not
// a product blocker). This request loop itself needs no special signing.

import CoreGraphics
import Foundation

// If the parent closes the read end of our stdout before we finish writing, a
// SIGPIPE would terminate us with signal 13 (exit 141) — violating the invariant
// that this sidecar always exits cleanly. Ignore it so the write path can fail as
// a recoverable error instead of a signal.
signal(SIGPIPE, SIG_IGN)

/// Write a JSON object followed by a newline to stdout, safely (no forced unwrap).
/// Returns false if serialization or the write fails so callers can exit cleanly.
private func writeLine(_ object: [String: Any]) -> Bool {
    guard let data = try? JSONSerialization.data(withJSONObject: object) else {
        return false
    }
    let handle = FileHandle.standardOutput
    // The throwing `write(contentsOf:)` surfaces a broken pipe as a Swift error
    // (caught by `try?`) rather than raising an uncaught ObjC exception.
    guard (try? handle.write(contentsOf: data)) != nil else { return false }
    if let newline = "\n".data(using: .utf8) {
        _ = try? handle.write(contentsOf: newline)
    }
    return true
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
/// surface. `microphone`/`camera` are provisional "notDetermined" until
/// AVFoundation detection lands (16.6/19).
private func permissionsPayload() -> [String: Any] {
    let screenRecording = CGPreflightScreenCaptureAccess() ? "granted" : "notDetermined"
    return [
        "screenRecording": screenRecording,
        "microphone": "notDetermined",
        "camera": "notDetermined",
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
        "protocolVersion": 1,
        "macos": macos,
        "features": [
            // System-audio capture is supported on the macOS 13+ floor this
            // sidecar is built for (Package.swift platforms: .macOS(.v13)).
            "systemAudio": true,
            // Microphone / camera capture paths are not built yet (16.6 / 20.x).
            "microphone": false,
            "camera": false,
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

/// The `listSources` result: real displays; applications (SCShareableContent)
/// and microphones/cameras (AVFoundation) deferred as empty arrays — the shape
/// is the locked contract, enumeration lands with 16.6/19.
private func sourcesResult() -> [String: Any] {
    return [
        "displays": listDisplays(),
        "applications": [] as [[String: Any]],
        "microphones": [] as [[String: Any]],
        "cameras": [] as [[String: Any]],
    ]
}

// The request loop: read lines until EOF. A malformed line (not JSON, not an
// object, no string method) is skipped — never a crash, never garbage output. An
// unknown method gets an id-correlated {id,error} answer so the host can fail
// honestly instead of hanging. EOF (the host closed our stdin) → clean exit 0.
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
        response = ["id": id, "result": sourcesResult()]
    case "requestScreenRecording":
        // Story 16.5: ask TCC for Screen Recording access. Returns immediately
        // with the current grant; where the state is undetermined the OS posts
        // its one real prompt per app lifetime asynchronously (attributed to
        // keeper — this sidecar is keeper's own child process, and the usage
        // string lives in keeper's bundle Info.plist). A prior denial shows no
        // prompt and reads back false — the host resolves that to its honest
        // denied-with-fix-path (System Settings deep link).
        response = ["id": id, "result": ["granted": CGRequestScreenCaptureAccess()]]
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

exit(0)
