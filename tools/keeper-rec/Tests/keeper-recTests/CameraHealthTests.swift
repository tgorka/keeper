// SPDX-License-Identifier: Apache-2.0
//
// Unit tests for the pure camera-loss decision policy (Story 20.1) — the
// simulated-signal contract at the sidecar boundary, the MicHealthTests twin.
// No capture hardware, no code-signing: `CameraHealth.decide` is
// Foundation-only (the Rotation.swift pattern), and every removal path in the
// engine — real disconnect, camera-session runtime error,
// `simulateCameraRemoval` — funnels through this one branch. Unlike the mic
// there is no fallback leg: the intent contract mandates the camera file
// simply finalizes early (no black-fill, no re-feed from a different camera).

import XCTest

@testable import keeper_rec

final class CameraHealthTests: XCTestCase {
    // MARK: - camera off: nothing to lose

    func testCameraDisabledNeverWarns() {
        let decision = CameraHealth.decide(
            cameraEnabled: false, removedDeviceId: "cam-A", activeDeviceId: "cam-A")
        XCTAssertFalse(decision.shouldWarn)
    }

    func testCameraDisabledIgnoresEvenAnUnspecifiedRemoval() {
        let decision = CameraHealth.decide(
            cameraEnabled: false, removedDeviceId: nil, activeDeviceId: nil)
        XCTAssertFalse(decision.shouldWarn)
    }

    // MARK: - unrelated device: no false warning

    func testUnrelatedDeviceRemovalIsIgnored() {
        let decision = CameraHealth.decide(
            cameraEnabled: true, removedDeviceId: "someone-elses-camera", activeDeviceId: "cam-A")
        XCTAssertFalse(decision.shouldWarn)
    }

    // MARK: - our camera lost: warn, finalize early, screen continues

    func testActiveCameraRemovalWarnsWithTheStableCode() {
        let decision = CameraHealth.decide(
            cameraEnabled: true, removedDeviceId: "cam-A", activeDeviceId: "cam-A")
        XCTAssertTrue(decision.shouldWarn)
        XCTAssertEqual(decision.code, "cameraLost")
        XCTAssertEqual(decision.message, CameraHealth.lostMessage)
    }

    func testTheMessageIsHonestAboutBothHalves() {
        // The AC's never-abort contract lives in this copy: the camera file
        // ends, the screen recording continues — both stated, no black-fill
        // or fallback claimed.
        XCTAssertTrue(CameraHealth.lostMessage.contains("camera file ends"))
        XCTAssertTrue(CameraHealth.lostMessage.contains("screen recording continues"))
    }

    // MARK: - unknown identities: conservative honesty

    func testUnspecifiedRemovalSignalWarns() {
        // A camera-session runtime error / the simulated removal names no
        // device — assume it was ours (a warning beats a silently frozen
        // camera file).
        let decision = CameraHealth.decide(
            cameraEnabled: true, removedDeviceId: nil, activeDeviceId: "cam-A")
        XCTAssertTrue(decision.shouldWarn)
        XCTAssertEqual(decision.code, "cameraLost")
    }

    func testUnknownActiveDeviceWarnsOnAnyVideoRemoval() {
        // The engine could not snapshot which device feeds the camera file —
        // a named video removal still warns.
        let decision = CameraHealth.decide(
            cameraEnabled: true, removedDeviceId: "cam-B", activeDeviceId: nil)
        XCTAssertTrue(decision.shouldWarn)
        XCTAssertEqual(decision.message, CameraHealth.lostMessage)
    }
}
