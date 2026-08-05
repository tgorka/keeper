// SPDX-License-Identifier: Apache-2.0
//
// Pure acoustic-echo-cancellation decision policy (Story 22.7, Epic 22).
//
// Foundation-only on purpose (the MicHealth.swift / Rotation.swift
// testable-policy pattern): no AudioToolbox / CoreAudio / AVFoundation import,
// so every "use the voice-processing unit or fall back to today's mic path"
// branch is unit-testable (Tests/keeper-recTests/EchoCancellationTests.swift)
// without capture hardware, speakers, or code-signing. The C API that produces
// the facts fed in here lives in exactly one file, VoiceProcessingMic.swift;
// `CaptureEngine.startVoiceProcessingMic` (Capture.swift) performs the side
// effects (the `warning` event, the fallback) this decision names.

import Foundation

/// Decides whether one capture session runs the `'vpio'` voice-processing
/// microphone producer, and which non-fatal `warning` (if any) the engine owes
/// the host (Story 22.7).
///
/// The inputs are plain facts the engine already holds: whether the user asked
/// for echo cancellation at all, whether the picked input device is an
/// aggregate (the one device class `kAudioUnitSubType_VoiceProcessingIO`
/// refuses — it builds its own private aggregate spanning input and output),
/// and the `OSStatus` an `AudioUnitInitialize` attempt returned. Echo
/// cancellation is **non-fatal by contract**: every failure degrades to today's
/// plain microphone path with a warning — never a failed, silent, or
/// half-written recording.
enum EchoCancellation {
    /// The stable wire code for "echo cancellation could not be applied"
    /// (`{"event":"warning",...}`) — the recording continues on the plain mic.
    static let warningCode = "echoCancellationUnavailable"

    /// The honest message behind [`warningCode`]: the session still records the
    /// microphone, just without the canceller.
    static let unavailableMessage =
        "echo cancellation is unavailable — recording the microphone unprocessed"

    /// The stable wire code for "the echo reference is no longer the device you
    /// are listening on" — emitted at most once per session.
    static let staleReferenceCode = "echoReferenceStale"

    /// The honest message behind [`staleReferenceCode`]. Re-initializing the
    /// unit mid-session would tear the mic track, so the recording keeps the
    /// reference it started with and says so.
    static let staleReferenceMessage =
        "the output device changed — echo cancellation keeps using the output it started with"

    /// The `AudioUnitInitialize` OSStatus that means "this input device does not
    /// present the channel layout the voice-processing unit expects"
    /// (`kAUVoiceIOErr_UnexpectedNumberOfInputChannels`). Spelled as a literal
    /// so this file stays Foundation-only; VoiceProcessingMic.swift passes the
    /// real framework constant's value straight through.
    static let unexpectedInputChannelsStatus: OSStatus = -66784

    /// The neutral "no failure yet" status (`noErr`), spelled locally for the
    /// same Foundation-only reason.
    static let okStatus: OSStatus = 0

    /// What the engine should do about one session's echo-cancellation request.
    struct Decision: Equatable {
        /// Whether to run the `'vpio'` producer as this session's microphone
        /// source (and therefore skip the SCStream's own mic leg entirely).
        let useVoiceProcessing: Bool
        /// The wire `code` for the non-fatal warning the engine owes the host,
        /// or `nil` when nothing is owed (the happy path, and an honest
        /// user-requested "off" — declining is not a fault).
        let warningCode: String?
        /// The wire `message` paired with [`warningCode`]; `nil` in lockstep.
        let warningMessage: String?
    }

    /// The "run the plain microphone path, quietly" decision.
    private static let plainMic = Decision(
        useVoiceProcessing: false, warningCode: nil, warningMessage: nil)

    /// The "run the plain microphone path, and say why" decision.
    private static let unavailable = Decision(
        useVoiceProcessing: false, warningCode: warningCode, warningMessage: unavailableMessage)

    /// Decide whether this session's microphone runs through the
    /// voice-processing unit.
    ///
    /// - `requested`: the persisted pre-recording switch (`true` by default,
    ///   including on the absent-key wire an older host sends).
    /// - `isAggregateInput`: whether the picked (or default) input device's
    ///   transport type is `kAudioDeviceTransportTypeAggregate` — the unit
    ///   cannot span one, so this is a fallback, not an error.
    /// - `initStatus`: the `AudioUnitInitialize` result, or [`okStatus`] when
    ///   no initialization has been attempted yet (the pre-flight call).
    ///   `kAUVoiceIOErr_UnexpectedNumberOfInputChannels` and every other
    ///   non-zero status degrade identically — one honest warning, one fallback.
    static func decide(
        requested: Bool, isAggregateInput: Bool, initStatus: OSStatus
    ) -> Decision {
        // The user turned it off: today's mic path verbatim, and no warning —
        // an honoured preference is not a fault.
        guard requested else { return plainMic }
        // An aggregate input is a known, permanent refusal — warn once and run
        // the plain mic rather than failing a start over an unsupported device.
        if isAggregateInput { return unavailable }
        // Any setup/initialize failure degrades the same way.
        if initStatus != okStatus { return unavailable }
        return Decision(useVoiceProcessing: true, warningCode: nil, warningMessage: nil)
    }
}
