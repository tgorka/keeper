// SPDX-License-Identifier: Apache-2.0
//
// Pure frame-rate normalization policy (Story 19.5, Epic 19).
//
// Foundation-only on purpose (the Rotation.swift / MicHealth.swift
// testable-policy pattern): no AVFoundation / CoreMedia / ScreenCaptureKit
// import, so the fps policy is unit-testable
// (Tests/keeper-recTests/FrameRateTests.swift) without capture hardware or
// code-signing. `CaptureEngine.beginCapture` (Capture.swift) feeds the
// wire-decoded `fps` through this before building the
// `SCStreamConfiguration.minimumFrameInterval` timescale.

import Foundation

/// Normalize a wire-level `fps` value to the legal set {30, 60} (Story 19.5):
/// anything that is not exactly 60 becomes the default of 30. The host's
/// registry read normalizes with the identical rule, but the sidecar
/// re-normalizes defensively so a corrupted or hostile wire value can never
/// produce a degenerate (zero/negative/absurd) `CMTime` timescale — the
/// sidecar must always capture sanely, never crash or stall on a bad knob.
func normalizeFps(_ fps: Int) -> Int {
    fps == 60 ? 60 : 30
}
