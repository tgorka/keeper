// SPDX-License-Identifier: Apache-2.0
//
// The acoustic-echo-cancelling microphone producer (Story 22.7, Epic 22).
//
// A third microphone source behind the existing `micEnabled` branch: macOS's
// own `kAudioUnitSubType_VoiceProcessingIO` ('vpio') audio unit, whose echo
// reference is the OUTPUT DEVICE's mix rather than this process's render. That
// is the whole point — keeper renders no far end, so a canceller fed by our own
// render would have nothing to subtract; the unit builds a private aggregate
// spanning the input and output devices, which is why it can remove audio
// ANOTHER app put through the speakers (the ScreenCaptureKit system-audio track
// keeper is recording at that same moment).
//
// This is the ONE file in the sidecar that speaks the AudioUnit/CoreAudio C
// API. Every decision it feeds — run the unit or fall back, which warning is
// owed — lives in the Foundation-only `EchoCancellation.swift` so it stays
// unit-testable without hardware. Every `OSStatus` here becomes a thrown
// `VoiceProcessingError`, and every throw degrades to today's plain microphone
// path (`CaptureEngine.startVoiceProcessingMic`) — never a failed recording.
//
// Bus layout (the load-bearing detail): element **1** is the INPUT bus (the
// microphone; its output scope is what `AudioUnitRender` hands us) and element
// **0** is the OUTPUT bus (the speakers; its input scope is where the silence
// render callback sits). `kAudioOutputUnitProperty_CurrentDevice` is a
// Global-scope property whose ELEMENT selects which of the two buses is being
// bound, so the input device goes to element 1 and the output device — the echo
// reference — to element 0.
//
// Threading: the unit's input callback runs on a CoreAudio realtime thread. It
// renders into a preallocated buffer, wraps the frames in a `CMSampleBuffer`
// stamped from the SAME host clock the rest of the session uses
// (`CMClockMakeHostTimeFromSystemUnits`), and hands it to the sink, which
// hops onto the engine's serial `mediaQueue` — writer state stays queue-confined
// exactly as with every other source.

import AudioToolbox
import CoreAudio
import CoreMedia
import Foundation

/// One failed CoreAudio/AudioUnit call, carrying the stage that failed and the
/// raw `OSStatus` — `EchoCancellation.decide` maps the status (notably
/// `kAUVoiceIOErr_UnexpectedNumberOfInputChannels`, -66784) onto the fallback.
struct VoiceProcessingError: Error, CustomStringConvertible {
    /// The call that failed, in human words (never a secret, never a path).
    let stage: String
    /// The raw `OSStatus` the framework returned.
    let status: OSStatus
    var description: String { "\(stage) failed (OSStatus \(status))" }
}

/// The `'vpio'` microphone producer for one capture session.
///
/// Owned by `CaptureEngine`; `start()` throws on any setup failure (the engine
/// then falls back), `stop()` is idempotent and safe to call after a failed
/// start. A single instance is used per attempt — a mic-loss fallback builds a
/// fresh one rather than re-initializing this unit.
final class VoiceProcessingMic {
    /// Where finished mic sample buffers go. Called on the CoreAudio realtime
    /// input thread — the engine's sink hops to `mediaQueue` immediately.
    typealias SampleSink = (CMSampleBuffer) -> Void

    /// The requested client sample rate. The unit converts from the hardware
    /// rate for us when it can; when it refuses, `start()` retries at the
    /// device's native rate (the honest documented cost) rather than failing.
    private static let preferredSampleRate: Double = 48_000

    /// The render slice we size the capture buffer for. Set explicitly rather
    /// than trusting the platform default so the preallocated buffer can never
    /// be smaller than a slice the unit hands us.
    private static let maximumFramesPerSlice: UInt32 = 4096

    /// The picked input device's uniqueID (an `AVCaptureDevice` uniqueID, which
    /// on macOS is the CoreAudio device UID), or `nil` for the system default
    /// input.
    private let deviceUID: String?
    private let sink: SampleSink
    /// Fired (at most once, by the caller's own latch) when the DEFAULT OUTPUT
    /// DEVICE changes mid-session: the echo reference this unit started with is
    /// no longer what the user hears, and re-initializing would tear the mic
    /// track, so the engine warns instead.
    private let onOutputDeviceChanged: () -> Void

    private var unit: AudioUnit?
    /// The client format on bus 1's output scope — mono packed Int16 at
    /// whichever rate the unit accepted. Its `mSampleRate` drives the sample
    /// buffers' timescale, so it must be read back, never assumed.
    private var format = AudioStreamBasicDescription()
    private var formatDescription: CMFormatDescription?
    /// Preallocated single-buffer `AudioBufferList` + its backing storage: the
    /// realtime callback must not allocate its render target per slice.
    private var bufferList: UnsafeMutableAudioBufferListPointer?
    private var storage: UnsafeMutableRawPointer?
    private var storageBytes = 0
    private var running = false
    /// The default-output-device listener block, kept so `stop()` can remove
    /// exactly the block it added.
    private var outputDeviceListener: AudioObjectPropertyListenerBlock?
    private let listenerQueue = DispatchQueue(label: "dev.tgorka.keeper-rec.echo-reference")

    init(
        deviceUID: String?, sink: @escaping SampleSink,
        onOutputDeviceChanged: @escaping () -> Void
    ) {
        self.deviceUID = deviceUID
        self.sink = sink
        self.onOutputDeviceChanged = onOutputDeviceChanged
    }

    deinit { teardown() }

    // MARK: - CoreAudio device queries

    /// The system object's property address for `selector`, Global scope, main
    /// element — the shape every hardware-level query below uses.
    private static func systemAddress(_ selector: AudioObjectPropertySelector)
        -> AudioObjectPropertyAddress
    {
        AudioObjectPropertyAddress(
            mSelector: selector,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain)
    }

    /// Resolve a CoreAudio `AudioDeviceID` from an `AVCaptureDevice` uniqueID
    /// via `kAudioHardwarePropertyTranslateUIDToDevice`. `nil` when the id does
    /// not name a live CoreAudio device (a vanished pick) — the caller then
    /// uses the system default input, exactly like the plain mic path does.
    static func deviceID(forUID uid: String) -> AudioDeviceID? {
        var address = systemAddress(kAudioHardwarePropertyTranslateUIDToDevice)
        var device = AudioDeviceID(kAudioObjectUnknown)
        var size = UInt32(MemoryLayout<AudioDeviceID>.size)
        var cfUID = uid as CFString
        let status = withUnsafeMutablePointer(to: &cfUID) { uidPointer -> OSStatus in
            AudioObjectGetPropertyData(
                AudioObjectID(kAudioObjectSystemObject), &address,
                UInt32(MemoryLayout<CFString>.size), uidPointer, &size, &device)
        }
        guard status == noErr, device != AudioDeviceID(kAudioObjectUnknown) else { return nil }
        return device
    }

    /// The current system default device for `selector`
    /// (`kAudioHardwarePropertyDefaultInputDevice` /
    /// `…DefaultOutputDevice`), or `nil` when none exists.
    static func defaultDevice(_ selector: AudioObjectPropertySelector) -> AudioDeviceID? {
        var address = systemAddress(selector)
        var device = AudioDeviceID(kAudioObjectUnknown)
        var size = UInt32(MemoryLayout<AudioDeviceID>.size)
        let status = AudioObjectGetPropertyData(
            AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &size, &device)
        guard status == noErr, device != AudioDeviceID(kAudioObjectUnknown) else { return nil }
        return device
    }

    /// Whether `device`'s transport type is `kAudioDeviceTransportTypeAggregate`
    /// — the one input class the voice-processing unit cannot span (it builds
    /// its OWN private aggregate). Unknown/unreadable ⇒ `false`: a failed probe
    /// must not silently disable a feature the hardware would have supported;
    /// a genuinely unsupported device still fails cleanly at initialize and
    /// takes the identical fallback.
    static func isAggregate(device: AudioDeviceID) -> Bool {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioDevicePropertyTransportType,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain)
        var transport: UInt32 = 0
        var size = UInt32(MemoryLayout<UInt32>.size)
        let status = AudioObjectGetPropertyData(device, &address, 0, nil, &size, &transport)
        return status == noErr && transport == kAudioDeviceTransportTypeAggregate
    }

    /// The input device this producer would bind: the picked uniqueID when it
    /// still resolves, else the system default input. `nil` when there is no
    /// input device at all.
    static func resolveInputDevice(uid: String?) -> AudioDeviceID? {
        if let uid, let picked = deviceID(forUID: uid) { return picked }
        return defaultDevice(kAudioHardwarePropertyDefaultInputDevice)
    }

    /// Whether the input device this session would use is an aggregate — the
    /// pre-flight fact `EchoCancellation.decide` consumes before any unit is
    /// built (no TCC prompt, no hardware start).
    static func isAggregateInput(uid: String?) -> Bool {
        guard let device = resolveInputDevice(uid: uid) else { return false }
        return isAggregate(device: device)
    }

    // MARK: - Lifecycle

    /// Build, configure, initialize and start the `'vpio'` unit. Throws a
    /// `VoiceProcessingError` carrying the failing stage's `OSStatus` — the
    /// engine feeds that status to `EchoCancellation.decide` and falls back.
    func start() throws {
        guard let inputDevice = Self.resolveInputDevice(uid: deviceUID) else {
            throw VoiceProcessingError(stage: "resolving the input device", status: -1)
        }
        guard let outputDevice = Self.defaultDevice(kAudioHardwarePropertyDefaultOutputDevice)
        else {
            throw VoiceProcessingError(stage: "resolving the output device", status: -1)
        }

        var description = AudioComponentDescription(
            componentType: kAudioUnitType_Output,
            componentSubType: kAudioUnitSubType_VoiceProcessingIO,
            componentManufacturer: kAudioUnitManufacturer_Apple,
            componentFlags: 0, componentFlagsMask: 0)
        guard let component = AudioComponentFindNext(nil, &description) else {
            throw VoiceProcessingError(stage: "finding the voice-processing unit", status: -1)
        }
        var instance: AudioUnit?
        try check(
            AudioComponentInstanceNew(component, &instance),
            "instantiating the voice-processing unit")
        guard let instance else {
            throw VoiceProcessingError(stage: "instantiating the voice-processing unit", status: -1)
        }
        unit = instance

        // Enable BOTH buses before binding devices (the AUHAL ordering rule):
        // element 1 / input scope is the microphone, element 0 / output scope
        // is the speakers. The output bus must be enabled even though keeper
        // renders nothing — that bus IS the echo reference.
        var enable: UInt32 = 1
        try check(
            AudioUnitSetProperty(
                instance, kAudioOutputUnitProperty_EnableIO, kAudioUnitScope_Input, 1,
                &enable, UInt32(MemoryLayout<UInt32>.size)),
            "enabling the microphone bus")
        try check(
            AudioUnitSetProperty(
                instance, kAudioOutputUnitProperty_EnableIO, kAudioUnitScope_Output, 0,
                &enable, UInt32(MemoryLayout<UInt32>.size)),
            "enabling the echo-reference bus")

        // `kAudioOutputUnitProperty_CurrentDevice` is Global-scope; its ELEMENT
        // picks the bus. Element 1 ⇒ the microphone, element 0 ⇒ the output
        // device whose mix the canceller subtracts.
        var input = inputDevice
        try check(
            AudioUnitSetProperty(
                instance, kAudioOutputUnitProperty_CurrentDevice, kAudioUnitScope_Global, 1,
                &input, UInt32(MemoryLayout<AudioDeviceID>.size)),
            "binding the microphone device")
        var output = outputDevice
        try check(
            AudioUnitSetProperty(
                instance, kAudioOutputUnitProperty_CurrentDevice, kAudioUnitScope_Global, 0,
                &output, UInt32(MemoryLayout<AudioDeviceID>.size)),
            "binding the echo-reference device")

        // Noise suppression comes with the unit and is not defeatable; AGC is,
        // and must be OFF — a recorder must not ride the user's voice level up
        // and down under the far end.
        var agc: UInt32 = 0
        try check(
            AudioUnitSetProperty(
                instance, kAUVoiceIOProperty_VoiceProcessingEnableAGC, kAudioUnitScope_Global, 0,
                &agc, UInt32(MemoryLayout<UInt32>.size)),
            "disabling automatic gain control")
        // Explicitly NOT bypassed — belt and braces against a stale default.
        var bypass: UInt32 = 0
        try check(
            AudioUnitSetProperty(
                instance, kAUVoiceIOProperty_BypassVoiceProcessing, kAudioUnitScope_Global, 0,
                &bypass, UInt32(MemoryLayout<UInt32>.size)),
            "enabling voice processing")

        var slice = Self.maximumFramesPerSlice
        try check(
            AudioUnitSetProperty(
                instance, kAudioUnitProperty_MaximumFramesPerSlice, kAudioUnitScope_Global, 0,
                &slice, UInt32(MemoryLayout<UInt32>.size)),
            "setting the maximum render slice")

        try configureFormat(instance)
        try installCallbacks(instance)

        // The one OSStatus the I/O matrix names explicitly: -66784
        // (`kAUVoiceIOErr_UnexpectedNumberOfInputChannels`) surfaces here, and
        // takes the same fallback as every other initialize failure.
        try check(AudioUnitInitialize(instance), "initializing the voice-processing unit")
        try allocateCaptureBuffer()
        try check(AudioOutputUnitStart(instance), "starting the voice-processing unit")
        running = true
        installOutputDeviceListener()
    }

    /// Stop and dispose the unit and remove the output-device listener.
    /// Idempotent, and safe after a failed `start()`.
    func stop() { teardown() }

    // MARK: - Configuration

    /// Set the client format on bus 1's output scope (what `AudioUnitRender`
    /// hands us) and bus 0's input scope (what the silence callback fills).
    ///
    /// Mono packed Int16 — the same shape the silence-fill already writes, so
    /// the writer's AAC converter path is unchanged. 48 kHz is requested first;
    /// a unit that refuses the conversion gets the device's native rate instead
    /// (the documented cost), because a rate mismatch must never cost the user
    /// their echo cancellation.
    private func configureFormat(_ instance: AudioUnit) throws {
        var hardware = AudioStreamBasicDescription()
        var size = UInt32(MemoryLayout<AudioStreamBasicDescription>.size)
        let probe = AudioUnitGetProperty(
            instance, kAudioUnitProperty_StreamFormat, kAudioUnitScope_Output, 1, &hardware, &size)
        let nativeRate =
            probe == noErr && hardware.mSampleRate > 0
            ? hardware.mSampleRate : Self.preferredSampleRate

        var lastStatus: OSStatus = noErr
        for rate in [Self.preferredSampleRate, nativeRate] {
            var candidate = Self.monoInt16Format(sampleRate: rate)
            let outStatus = AudioUnitSetProperty(
                instance, kAudioUnitProperty_StreamFormat, kAudioUnitScope_Output, 1,
                &candidate, UInt32(MemoryLayout<AudioStreamBasicDescription>.size))
            if outStatus != noErr {
                lastStatus = outStatus
                continue
            }
            let inStatus = AudioUnitSetProperty(
                instance, kAudioUnitProperty_StreamFormat, kAudioUnitScope_Input, 0,
                &candidate, UInt32(MemoryLayout<AudioStreamBasicDescription>.size))
            if inStatus != noErr {
                lastStatus = inStatus
                continue
            }
            format = candidate
            var created: CMFormatDescription?
            let formatStatus = CMAudioFormatDescriptionCreate(
                allocator: kCFAllocatorDefault, asbd: &candidate, layoutSize: 0, layout: nil,
                magicCookieSize: 0, magicCookie: nil, extensions: nil,
                formatDescriptionOut: &created)
            guard formatStatus == noErr, let created else {
                throw VoiceProcessingError(
                    stage: "describing the microphone format", status: formatStatus)
            }
            formatDescription = created
            return
        }
        throw VoiceProcessingError(stage: "setting the microphone format", status: lastStatus)
    }

    /// One channel, 16-bit signed packed LPCM at `sampleRate` — the mic track's
    /// shape while echo cancellation is on (the canceller is a voice-band, mono
    /// processor; there is no stereo output to be had).
    private static func monoInt16Format(sampleRate: Double) -> AudioStreamBasicDescription {
        AudioStreamBasicDescription(
            mSampleRate: sampleRate, mFormatID: kAudioFormatLinearPCM,
            mFormatFlags: kAudioFormatFlagIsSignedInteger | kAudioFormatFlagIsPacked,
            mBytesPerPacket: 2, mFramesPerPacket: 1, mBytesPerFrame: 2,
            mChannelsPerFrame: 1, mBitsPerChannel: 16, mReserved: 0)
    }

    /// Register the input callback (bus 1 delivery notification) and the bus-0
    /// render callback that emits silence.
    private func installCallbacks(_ instance: AudioUnit) throws {
        var inputCallback = AURenderCallbackStruct(
            inputProc: voiceProcessingInputProc,
            inputProcRefCon: Unmanaged.passUnretained(self).toOpaque())
        try check(
            AudioUnitSetProperty(
                instance, kAudioOutputUnitProperty_SetInputCallback, kAudioUnitScope_Global, 0,
                &inputCallback, UInt32(MemoryLayout<AURenderCallbackStruct>.size)),
            "attaching the microphone callback")
        var renderCallback = AURenderCallbackStruct(
            inputProc: voiceProcessingRenderSilence, inputProcRefCon: nil)
        try check(
            AudioUnitSetProperty(
                instance, kAudioUnitProperty_SetRenderCallback, kAudioUnitScope_Input, 0,
                &renderCallback, UInt32(MemoryLayout<AURenderCallbackStruct>.size)),
            "attaching the silent render callback")
    }

    /// Allocate the realtime render target once, sized for the largest slice
    /// the unit may ask for.
    private func allocateCaptureBuffer() throws {
        let bytes = Int(Self.maximumFramesPerSlice) * Int(format.mBytesPerFrame)
        guard bytes > 0 else {
            throw VoiceProcessingError(stage: "sizing the capture buffer", status: -1)
        }
        let list = AudioBufferList.allocate(maximumBuffers: 1)
        let block = UnsafeMutableRawPointer.allocate(byteCount: bytes, alignment: 2)
        list[0] = AudioBuffer(mNumberChannels: 1, mDataByteSize: UInt32(bytes), mData: block)
        bufferList = list
        storage = block
        storageBytes = bytes
    }

    /// Watch `kAudioHardwarePropertyDefaultOutputDevice`. The unit is NEVER
    /// re-initialized on a change (that would tear the mic track mid-session);
    /// the engine emits one honest `echoReferenceStale` warning instead.
    private func installOutputDeviceListener() {
        var address = Self.systemAddress(kAudioHardwarePropertyDefaultOutputDevice)
        let block: AudioObjectPropertyListenerBlock = { [weak self] _, _ in
            self?.onOutputDeviceChanged()
        }
        let status = AudioObjectAddPropertyListenerBlock(
            AudioObjectID(kAudioObjectSystemObject), &address, listenerQueue, block)
        // A listener we could not install costs only the stale warning — never
        // the recording. Keep the handle only when it really was registered.
        if status == noErr { outputDeviceListener = block }
    }

    // MARK: - Realtime capture

    /// Render one slice from bus 1 and hand it to the sink as a
    /// `CMSampleBuffer` stamped on the shared host clock. Runs on the CoreAudio
    /// realtime input thread; never throws, never blocks.
    fileprivate func render(
        flags: UnsafeMutablePointer<AudioUnitRenderActionFlags>,
        timestamp: UnsafePointer<AudioTimeStamp>, frames: UInt32
    ) -> OSStatus {
        guard let unit, let list = bufferList, let storage, running else { return noErr }
        let bytes = Int(frames) * Int(format.mBytesPerFrame)
        guard bytes > 0, bytes <= storageBytes else { return noErr }
        list[0] = AudioBuffer(
            mNumberChannels: 1, mDataByteSize: UInt32(bytes), mData: storage)
        let status = AudioUnitRender(
            unit, flags, timestamp, 1, frames, list.unsafeMutablePointer)
        guard status == noErr else { return status }
        // The SAME host-clock domain as `CMClockGetTime(CMClockGetHostTimeClock())`
        // and as ScreenCaptureKit's PTS, so `micPTSLowerBound`, the silence fill
        // and the rotation split all keep working with no conversion.
        let hostTime =
            timestamp.pointee.mFlags.contains(.hostTimeValid)
            ? timestamp.pointee.mHostTime : mach_absolute_time()
        let pts = CMClockMakeHostTimeFromSystemUnits(hostTime)
        guard let buffer = makeSampleBuffer(frames: frames, bytes: bytes, pts: pts) else {
            return noErr
        }
        sink(buffer)
        return noErr
    }

    /// Copy `bytes` of rendered LPCM out of the reusable render target into a
    /// self-contained `CMSampleBuffer` (the target is overwritten by the next
    /// slice, so the buffer must own its memory). `nil` on any CoreMedia
    /// failure — one dropped slice, never a crash.
    private func makeSampleBuffer(frames: UInt32, bytes: Int, pts: CMTime) -> CMSampleBuffer? {
        guard let formatDescription, let storage else { return nil }
        var blockBuffer: CMBlockBuffer?
        guard
            CMBlockBufferCreateWithMemoryBlock(
                allocator: kCFAllocatorDefault, memoryBlock: nil, blockLength: bytes,
                blockAllocator: kCFAllocatorDefault, customBlockSource: nil, offsetToData: 0,
                dataLength: bytes, flags: 0, blockBufferOut: &blockBuffer) == noErr,
            let block = blockBuffer,
            CMBlockBufferReplaceDataBytes(
                with: storage, blockBuffer: block, offsetIntoDestination: 0, dataLength: bytes)
                == noErr
        else { return nil }
        var sampleBuffer: CMSampleBuffer?
        guard
            CMAudioSampleBufferCreateReadyWithPacketDescriptions(
                allocator: kCFAllocatorDefault, dataBuffer: block,
                formatDescription: formatDescription, sampleCount: CMItemCount(frames),
                presentationTimeStamp: pts, packetDescriptions: nil,
                sampleBufferOut: &sampleBuffer) == noErr
        else { return nil }
        return sampleBuffer
    }

    // MARK: - Teardown

    /// Stop, uninitialize and dispose the unit; remove the listener; free the
    /// render target. Idempotent — every field is cleared as it is released.
    private func teardown() {
        running = false
        if var address = outputDeviceListener.map({ _ in
            Self.systemAddress(kAudioHardwarePropertyDefaultOutputDevice)
        }), let block = outputDeviceListener {
            AudioObjectRemovePropertyListenerBlock(
                AudioObjectID(kAudioObjectSystemObject), &address, listenerQueue, block)
            outputDeviceListener = nil
        }
        if let unit {
            AudioOutputUnitStop(unit)
            AudioUnitUninitialize(unit)
            AudioComponentInstanceDispose(unit)
            self.unit = nil
        }
        if let storage {
            storage.deallocate()
            self.storage = nil
        }
        storageBytes = 0
        if let bufferList {
            free(bufferList.unsafeMutablePointer)
            self.bufferList = nil
        }
        formatDescription = nil
    }

    /// Throw on any non-`noErr` status, naming the stage that produced it.
    private func check(_ status: OSStatus, _ stage: String) throws {
        guard status == noErr else {
            throw VoiceProcessingError(stage: stage, status: status)
        }
    }
}

/// The bus-0 render callback: emit NOTHING.
///
/// keeper is a recorder — it renders no far end. Zeroing the buffers and
/// setting `kAudioUnitRenderAction_OutputIsSilence` is exactly what Chromium's
/// `kMacSystemEchoCanceller` does: the unit still takes its echo reference from
/// the OUTPUT DEVICE's mix, so it cancels what other apps are playing, without
/// keeper putting a single sample into the room.
private let voiceProcessingRenderSilence: AURenderCallback = {
    _, ioActionFlags, _, _, _, ioData in
    ioActionFlags.pointee.insert(.unitRenderAction_OutputIsSilence)
    guard let ioData else { return noErr }
    for buffer in UnsafeMutableAudioBufferListPointer(ioData) {
        if let data = buffer.mData {
            memset(data, 0, Int(buffer.mDataByteSize))
        }
    }
    return noErr
}

/// The bus-1 input callback: the unit tells us a slice is ready; we pull it
/// with `AudioUnitRender` (the buffer list handed in here is always NULL by
/// contract) and forward it as a `CMSampleBuffer`.
private let voiceProcessingInputProc: AURenderCallback = {
    inRefCon, ioActionFlags, inTimeStamp, _, inNumberFrames, _ in
    let producer = Unmanaged<VoiceProcessingMic>.fromOpaque(inRefCon).takeUnretainedValue()
    return producer.render(
        flags: ioActionFlags, timestamp: inTimeStamp, frames: inNumberFrames)
}
