// SPDX-License-Identifier: Apache-2.0
//
// Unit tests for the pure mic-loss decision policy (Story 19.4) — the
// simulated-signal contract at the sidecar boundary. No capture hardware, no
// code-signing: `MicHealth.decide` is Foundation-only (the Rotation.swift
// pattern), and every removal path in the engine — real disconnect, 13–14
// runtime error, `simulateMicRemoval` — funnels through this one branch.

import XCTest

@testable import keeper_rec

final class MicHealthTests: XCTestCase {
    // MARK: - mic off: nothing to lose

    func testMicDisabledNeverWarns() {
        let decision = MicHealth.decide(
            micEnabled: false, removedDeviceId: "mic-A", activeDeviceId: "mic-A",
            fallbackAvailable: true)
        XCTAssertFalse(decision.shouldWarn)
        XCTAssertFalse(decision.fallbackToDefault)
    }

    func testMicDisabledIgnoresEvenAnUnspecifiedRemoval() {
        let decision = MicHealth.decide(
            micEnabled: false, removedDeviceId: nil, activeDeviceId: nil,
            fallbackAvailable: false)
        XCTAssertFalse(decision.shouldWarn)
    }

    // MARK: - unrelated device: no false warning

    func testUnrelatedDeviceRemovalIsIgnored() {
        let decision = MicHealth.decide(
            micEnabled: true, removedDeviceId: "someone-elses-mic", activeDeviceId: "mic-A",
            fallbackAvailable: true)
        XCTAssertFalse(decision.shouldWarn)
        XCTAssertFalse(decision.fallbackToDefault)
    }

    // MARK: - our mic lost: warn, silence-fill, fallback

    func testActiveMicRemovalWarnsAndFallsBackWhenADefaultExists() {
        let decision = MicHealth.decide(
            micEnabled: true, removedDeviceId: "mic-A", activeDeviceId: "mic-A",
            fallbackAvailable: true)
        XCTAssertTrue(decision.shouldWarn)
        XCTAssertEqual(decision.code, "micLost")
        XCTAssertEqual(decision.message, MicHealth.fallbackMessage)
        XCTAssertTrue(decision.fallbackToDefault)
    }

    func testActiveMicRemovalWithNoRemainingInputIsHonestAboutIt() {
        let decision = MicHealth.decide(
            micEnabled: true, removedDeviceId: "mic-A", activeDeviceId: "mic-A",
            fallbackAvailable: false)
        XCTAssertTrue(decision.shouldWarn)
        XCTAssertEqual(decision.code, "micLost")
        XCTAssertEqual(decision.message, MicHealth.noInputMessage)
        XCTAssertFalse(decision.fallbackToDefault, "no default input — nothing to fall back to")
    }

    func testTheTwoMessagesHonestlyDiffer() {
        // The AC's fallback-succeeded vs no-input distinction lives in these
        // two constants — they must never collapse into one wording.
        XCTAssertNotEqual(MicHealth.fallbackMessage, MicHealth.noInputMessage)
        XCTAssertTrue(MicHealth.fallbackMessage.contains("using system default input"))
        XCTAssertTrue(MicHealth.noInputMessage.contains("no microphone input"))
    }

    // MARK: - unknown identities: conservative honesty

    func testUnspecifiedRemovalSignalWarns() {
        // A 13–14 session runtime error / the simulated removal names no
        // device — assume it was ours (a warning beats a silent gap).
        let decision = MicHealth.decide(
            micEnabled: true, removedDeviceId: nil, activeDeviceId: "mic-A",
            fallbackAvailable: true)
        XCTAssertTrue(decision.shouldWarn)
        XCTAssertTrue(decision.fallbackToDefault)
    }

    func testUnknownActiveDeviceWarnsOnAnyAudioRemoval() {
        // The engine could not snapshot which device feeds the default-input
        // track — a named audio removal still warns.
        let decision = MicHealth.decide(
            micEnabled: true, removedDeviceId: "mic-B", activeDeviceId: nil,
            fallbackAvailable: false)
        XCTAssertTrue(decision.shouldWarn)
        XCTAssertEqual(decision.message, MicHealth.noInputMessage)
    }
}
