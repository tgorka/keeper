// SPDX-License-Identifier: Apache-2.0
//
// Pure microphone-loss decision policy (Story 19.4, Epic 19).
//
// Foundation-only on purpose (the Rotation.swift testable-policy pattern): no
// AVFoundation / CoreMedia / ScreenCaptureKit import, so the mic-loss decision
// is unit-testable (Tests/keeper-recTests/MicHealthTests.swift) without capture
// hardware or code-signing. `CaptureEngine.handleMicLost` (Capture.swift) feeds
// every removal signal — a real device disconnect, a 13–14 session runtime
// error, a stalled producer (the mic watchdog), or the `simulateMicRemoval`
// RPC — through this one branch and performs the side effects (warning event,
// silence-fill, fallback) it decides. The watchdog's and the silence check's
// thresholds live here too, for the same reason.

import Foundation

/// Decides how the engine responds to a device-removal signal (Story 19.4).
///
/// The inputs are deliberately plain facts the engine already holds: whether
/// this session records a mic track at all, the removed device's id (when the
/// signal names one), the id of the device actually feeding the mic track
/// (best-effort; `nil` when unknown), and whether a system default input still
/// exists to fall back to. Mic loss is **non-fatal by contract**: the decision
/// only ever yields a warning + silence-fill + optional fallback — never an
/// `error` event or a process exit.
enum MicHealth {
    /// The stable wire code for a mic-loss warning (`{"event":"warning",...}`).
    static let warningCode = "micLost"

    /// The honest warning message when a system default input exists — the
    /// engine falls back to it (the mic track keeps recording from the
    /// default device after a silence-filled seam).
    static let fallbackMessage = "microphone disconnected — using system default input"

    /// The honest warning message when NO input device remains — the mic
    /// track stays silence-filled for the rest of the session.
    static let noInputMessage = "microphone disconnected — no microphone input"

    /// The honest warning message when the microphone did not disappear but
    /// simply stopped delivering audio — the watchdog's finding, measured on
    /// macOS 27: a ScreenCaptureKit microphone bound to the system default
    /// input goes quiet for good when that device (AirPods leaving) vanishes,
    /// and no disconnect notification reaches this process. The engine falls
    /// back to the system default input.
    static let stalledMessage = "microphone stopped sending sound — using system default input"

    /// What made the engine suspect the microphone, which only changes the
    /// sentence: every cause takes the same warn + silence-fill + fallback path.
    enum Cause: Equatable {
        /// A device removal signal (disconnect, runtime error, simulation).
        case removed
        /// The watchdog saw no microphone sample for [`stallSeconds`].
        case stalled
    }

    /// How long the live microphone may go without delivering a single sample
    /// before the watchdog treats it as lost. Every producer keeper uses
    /// delivers a buffer every ~10–20 ms while it runs, so two seconds of
    /// nothing is not a quiet room, it is a dead feed.
    static let stallSeconds: Double = 2

    /// Whether a producer whose last sample arrived at `lastSampleUptime`
    /// has stalled at `now` (both `ProcessInfo.systemUptime` seconds).
    static func isStalled(lastSampleUptime: Double, now: Double) -> Bool {
        now - lastSampleUptime >= stallSeconds
    }

    /// The wire code for "the microphone is delivering digital silence".
    static let silentCode = "micSilent"

    /// How long the microphone must deliver nothing but zero samples before
    /// the engine says so. A real microphone never produces exact digital
    /// zero — even a silent room has a noise floor — so a stretch this long
    /// means a muted input, an input volume of zero, or a device that is not a
    /// microphone at all (a virtual audio device that became the default).
    static let silenceSeconds: Double = 8

    /// The warning shown while the microphone records digital silence. Names
    /// the device when it is known, because the fix is to pick another one.
    static func silentMessage(deviceName: String?) -> String {
        let device = deviceName.map { "“\($0)”" } ?? "the microphone"
        return "\(device) is recording silence — check the microphone in "
            + "System Settings › Sound › Input"
    }

    /// Whether `bytes` — one buffer of PCM samples in any layout — is digital
    /// silence. All-zero bytes are zero for every integer and IEEE float
    /// sample format, so the check needs no format description. An empty
    /// buffer says nothing and counts as not silent.
    static func isDigitalSilence(_ bytes: UnsafeRawBufferPointer) -> Bool {
        guard !bytes.isEmpty else { return false }
        return !bytes.contains { $0 != 0 }
    }

    /// What the engine should do about one removal signal.
    struct Decision: Equatable {
        /// Whether to emit the non-fatal `warning` event (and start the
        /// silence-fill). `false` = the removal does not affect this session's
        /// mic track (mic off, or an unrelated device) — do nothing.
        let shouldWarn: Bool
        /// The wire `code` for the warning event ([`warningCode`]).
        let code: String
        /// The wire `message` — honestly distinguishes fallback-succeeded
        /// ([`fallbackMessage`]) from no-input ([`noInputMessage`]).
        let message: String
        /// Whether the engine should attempt to re-feed the mic track from
        /// the system default input.
        let fallbackToDefault: Bool
    }

    /// The do-nothing decision (no warning, no fallback).
    private static let ignore = Decision(
        shouldWarn: false, code: "", message: "", fallbackToDefault: false)

    /// Decide the response to a device-removal signal.
    ///
    /// - `micEnabled`: whether this session records a microphone track.
    /// - `removedDeviceId`: the removed device's uniqueID, or `nil` when the
    ///   signal names no device (a 13–14 session runtime error, or the
    ///   simulated removal) — treated as "assume it was ours" (an honest
    ///   warning beats a silently gapped track).
    /// - `activeDeviceId`: the uniqueID of the device feeding the mic track,
    ///   or `nil` when unknown (same conservative treatment).
    /// - `fallbackAvailable`: whether a system default input device exists.
    /// - `cause`: why the engine is asking — only the fallback sentence
    ///   differs; a stall names no device, so it is always "ours".
    static func decide(
        micEnabled: Bool, removedDeviceId: String?, activeDeviceId: String?,
        fallbackAvailable: Bool, cause: Cause = .removed
    ) -> Decision {
        // No mic track — no mic to lose; every removal is someone else's.
        guard micEnabled else { return ignore }
        // A removal that names a device we know is NOT ours (another app's
        // mic, a camera-adjacent audio endpoint) must not raise a false
        // warning. When either side is unknown, warn — conservative honesty.
        if let removedDeviceId, let activeDeviceId, removedDeviceId != activeDeviceId {
            return ignore
        }
        let recovering = cause == .stalled ? stalledMessage : fallbackMessage
        return Decision(
            shouldWarn: true,
            code: warningCode,
            message: fallbackAvailable ? recovering : noInputMessage,
            fallbackToDefault: fallbackAvailable)
    }
}
