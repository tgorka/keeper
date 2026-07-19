// SPDX-License-Identifier: Apache-2.0
//
// Pure microphone-loss decision policy (Story 19.4, Epic 19).
//
// Foundation-only on purpose (the Rotation.swift testable-policy pattern): no
// AVFoundation / CoreMedia / ScreenCaptureKit import, so the mic-loss decision
// is unit-testable (Tests/keeper-recTests/MicHealthTests.swift) without capture
// hardware or code-signing. `CaptureEngine.handleMicLost` (Capture.swift) feeds
// every removal signal — a real device disconnect, a 13–14 session runtime
// error, or the `simulateMicRemoval` RPC — through this one branch and performs
// the side effects (warning event, silence-fill, fallback) it decides.

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
    static func decide(
        micEnabled: Bool, removedDeviceId: String?, activeDeviceId: String?,
        fallbackAvailable: Bool
    ) -> Decision {
        // No mic track — no mic to lose; every removal is someone else's.
        guard micEnabled else { return ignore }
        // A removal that names a device we know is NOT ours (another app's
        // mic, a camera-adjacent audio endpoint) must not raise a false
        // warning. When either side is unknown, warn — conservative honesty.
        if let removedDeviceId, let activeDeviceId, removedDeviceId != activeDeviceId {
            return ignore
        }
        return Decision(
            shouldWarn: true,
            code: warningCode,
            message: fallbackAvailable ? fallbackMessage : noInputMessage,
            fallbackToDefault: fallbackAvailable)
    }
}
