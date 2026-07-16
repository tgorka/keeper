// SPDX-License-Identifier: Apache-2.0
//
// keeper-rec — first-party screen-recording capture sidecar for keeper.
//
// This file is the NDJSON-RPC handshake seed (Story 16.1): one JSON object per
// line on stdio. Real capture (ScreenCaptureKit / AVFoundation) lands in 16.6;
// today the binary only answers `getCapabilities` and exits cleanly so the
// externalBin + codesign + sidecar-spawn wiring can be proven end-to-end.
//
// Cap #1722: macOS 15+ silently rejects ad-hoc-signed ScreenCaptureKit — real
// capture builds need an Apple Development certificate (a DevEx requirement, not
// a product blocker). The stub itself needs no special signing.

import Foundation

// If the parent closes the read end of our stdout before we finish writing, a
// SIGPIPE would terminate us with signal 13 (exit 141) — violating the invariant
// that this stub always exits cleanly. Ignore it so the write path can fail as a
// recoverable error instead of a signal.
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

// Read exactly one request line. EOF / closed stdin → clean exit, no output.
guard let line = readLine(strippingNewline: true),
      let data = line.data(using: .utf8),
      let msg = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
else {
    exit(0)
}

// Unknown / missing method → clean exit, no partial output, never a crash.
guard msg["method"] as? String == "getCapabilities" else {
    exit(0)
}

let version = ProcessInfo.processInfo.operatingSystemVersion
let macos = "\(version.majorVersion).\(version.minorVersion).\(version.patchVersion)"

let response: [String: Any] = [
    "id": msg["id"] ?? NSNull(),
    "result": [
        "protocolVersion": 1,
        "macos": macos,
    ],
]

_ = writeLine(response)
exit(0)
