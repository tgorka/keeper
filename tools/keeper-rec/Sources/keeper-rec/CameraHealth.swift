// SPDX-License-Identifier: Apache-2.0
//
// Pure camera-loss decision policy (Story 20.1, Epic 20, FR-70).
//
// Foundation-only on purpose (the MicHealth.swift / Rotation.swift
// testable-policy pattern): no AVFoundation / CoreMedia / ScreenCaptureKit
// import, so the camera-loss decision is unit-testable
// (Tests/keeper-recTests/CameraHealthTests.swift) without capture hardware or
// code-signing. `CaptureEngine.handleCameraLost` (Capture.swift) feeds every
// removal signal — a real device disconnect, a camera-session runtime error,
// or the `simulateCameraRemoval` RPC — through this one branch and performs
// the side effects (warning event, early camera-file finalize) it decides.

import Foundation

/// Decides how the engine responds to a camera-removal signal (Story 20.1).
///
/// The inputs are deliberately plain facts the engine already holds: whether
/// this session records a camera file at all, the removed device's id (when
/// the signal names one), and the id of the device actually feeding the camera
/// file (best-effort; `nil` when unknown). Camera loss is **non-fatal by
/// contract**: the decision only ever yields a sticky warning — the engine
/// then finalizes the current `camera-####.mov` early (which simply ends
/// there; no black-fill, no fallback re-feed — unlike the mic, a talking-head
/// track from a *different* camera would be a silent lie) while the screen
/// recording continues. Never an `error` event, never a process exit.
enum CameraHealth {
    /// The stable wire code for a camera-loss warning (`{"event":"warning",...}`).
    static let warningCode = "cameraLost"

    /// The honest warning message: the camera file ends early, the screen
    /// recording keeps rolling. Continuity Camera drops (the phone locks or
    /// moves away) are the common case this copy must fit.
    static let lostMessage = "camera disconnected — the camera file ends here; screen recording continues"

    /// What the engine should do about one removal signal.
    struct Decision: Equatable {
        /// Whether to emit the non-fatal `warning` event (and finalize the
        /// camera file early). `false` = the removal does not affect this
        /// session's camera file (camera off, or an unrelated device) — do
        /// nothing.
        let shouldWarn: Bool
        /// The wire `code` for the warning event ([`warningCode`]).
        let code: String
        /// The wire `message` ([`lostMessage`]).
        let message: String
    }

    /// The do-nothing decision (no warning, no finalize).
    private static let ignore = Decision(shouldWarn: false, code: "", message: "")

    /// Decide the response to a device-removal signal.
    ///
    /// - `cameraEnabled`: whether this session records a camera file.
    /// - `removedDeviceId`: the removed device's uniqueID, or `nil` when the
    ///   signal names no device (a camera-session runtime error, or the
    ///   simulated removal) — treated as "assume it was ours" (an honest
    ///   warning beats a silently frozen camera file).
    /// - `activeDeviceId`: the uniqueID of the device feeding the camera
    ///   file, or `nil` when unknown (same conservative treatment).
    static func decide(
        cameraEnabled: Bool, removedDeviceId: String?, activeDeviceId: String?
    ) -> Decision {
        // No camera file — no camera to lose; every removal is someone else's.
        guard cameraEnabled else { return ignore }
        // A removal that names a device we know is NOT ours (another app's
        // camera, an unrelated video endpoint) must not raise a false
        // warning. When either side is unknown, warn — conservative honesty
        // (the MicHealth precedent).
        if let removedDeviceId, let activeDeviceId, removedDeviceId != activeDeviceId {
            return ignore
        }
        return Decision(shouldWarn: true, code: warningCode, message: lostMessage)
    }
}
