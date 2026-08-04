// SPDX-License-Identifier: Apache-2.0
//
// Unit tests for the pure acoustic-echo-cancellation decision policy (Story
// 22.7) — the whole fallback matrix at the sidecar boundary. No speakers, no
// microphone, no code-signing: `EchoCancellation.decide` is Foundation-only
// (the MicHealth.swift pattern), and every path the engine can take — the user
// turned it off, an aggregate input, the documented -66784 initialize failure,
// any other setup `OSStatus`, and the happy path — funnels through this one
// branch. The AudioUnit C API that produces these facts lives in
// VoiceProcessingMic.swift and is deliberately not exercised here.

import AudioToolbox
import XCTest

@testable import keeper_rec

final class EchoCancellationTests: XCTestCase {
    // MARK: - the user asked for it off

    func testNotRequestedRunsThePlainMicWithoutAWarning() {
        let decision = EchoCancellation.decide(
            requested: false, isAggregateInput: false,
            initStatus: EchoCancellation.okStatus)
        XCTAssertFalse(decision.useVoiceProcessing)
        // An honoured preference is not a fault — the host must see no warning.
        XCTAssertNil(decision.warningCode)
        XCTAssertNil(decision.warningMessage)
    }

    func testNotRequestedStaysQuietEvenWhenTheHardwareWouldHaveFailed() {
        // "Off" wins before any hardware fact is consulted: an off session must
        // be today's mic path verbatim, warnings included (there are none).
        let decision = EchoCancellation.decide(
            requested: false, isAggregateInput: true,
            initStatus: EchoCancellation.unexpectedInputChannelsStatus)
        XCTAssertFalse(decision.useVoiceProcessing)
        XCTAssertNil(decision.warningCode)
    }

    // MARK: - aggregate input: a permanent, honest refusal

    func testAggregateInputFallsBackWithTheUnavailableWarning() {
        let decision = EchoCancellation.decide(
            requested: true, isAggregateInput: true,
            initStatus: EchoCancellation.okStatus)
        XCTAssertFalse(decision.useVoiceProcessing)
        XCTAssertEqual(decision.warningCode, EchoCancellation.warningCode)
        XCTAssertEqual(decision.warningMessage, EchoCancellation.unavailableMessage)
    }

    // MARK: - initialize failures

    func testUnexpectedInputChannelsFallsBackWithTheUnavailableWarning() {
        // `kAUVoiceIOErr_UnexpectedNumberOfInputChannels` — the one status the
        // I/O matrix names by number.
        let decision = EchoCancellation.decide(
            requested: true, isAggregateInput: false,
            initStatus: EchoCancellation.unexpectedInputChannelsStatus)
        XCTAssertFalse(decision.useVoiceProcessing)
        XCTAssertEqual(decision.warningCode, EchoCancellation.warningCode)
        XCTAssertEqual(decision.warningMessage, EchoCancellation.unavailableMessage)
    }

    func testAnyOtherFailureStatusDegradesIdentically() {
        // A device-set failure, a format-set failure, a generic
        // `kAudioUnitErr_*` — one warning, one fallback, no special cases.
        for status: OSStatus in [-10_879, -50, -66_748, 1] {
            let decision = EchoCancellation.decide(
                requested: true, isAggregateInput: false, initStatus: status)
            XCTAssertFalse(
                decision.useVoiceProcessing, "status \(status) must fall back")
            XCTAssertEqual(decision.warningCode, EchoCancellation.warningCode)
            XCTAssertEqual(decision.warningMessage, EchoCancellation.unavailableMessage)
        }
    }

    // MARK: - the happy path

    func testRequestedOnASupportedInputRunsTheVoiceProcessingUnit() {
        let decision = EchoCancellation.decide(
            requested: true, isAggregateInput: false,
            initStatus: EchoCancellation.okStatus)
        XCTAssertTrue(decision.useVoiceProcessing)
        XCTAssertNil(decision.warningCode)
        XCTAssertNil(decision.warningMessage)
    }

    // MARK: - the wire-stable codes and messages

    func testWarningCodesAreTheDocumentedWireStrings() {
        // The host renders these verbatim; they are part of the sidecar's
        // observable contract, not incidental strings.
        XCTAssertEqual(EchoCancellation.warningCode, "echoCancellationUnavailable")
        XCTAssertEqual(EchoCancellation.staleReferenceCode, "echoReferenceStale")
        XCTAssertFalse(EchoCancellation.unavailableMessage.isEmpty)
        XCTAssertFalse(EchoCancellation.staleReferenceMessage.isEmpty)
        // The two codes must never collide — the host tells them apart.
        XCTAssertNotEqual(EchoCancellation.warningCode, EchoCancellation.staleReferenceCode)
    }

    func testTheUnexpectedInputChannelsConstantMatchesTheFramework() {
        // EchoCancellation.swift stays Foundation-only by spelling the status
        // as a literal; this pins it to the AudioToolbox constant it mirrors.
        XCTAssertEqual(
            EchoCancellation.unexpectedInputChannelsStatus,
            kAUVoiceIOErr_UnexpectedNumberOfInputChannels)
        XCTAssertEqual(EchoCancellation.okStatus, noErr)
    }
}
