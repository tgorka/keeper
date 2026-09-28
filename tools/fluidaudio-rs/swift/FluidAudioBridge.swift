// C ABI over FluidAudio for keeper's on-device transcription.
//
// Every entry point is synchronous: async FluidAudio calls run in a detached
// Task and the calling (Rust worker) thread waits on a semaphore. Results cross
// the boundary as JSON strings allocated with strdup and released through
// `fluidaudio_free_string`; failures return non-zero and hand back the Swift
// error's own text through `outError`.
//
// Models are only ever loaded from paths the caller names. Nothing here calls
// FluidAudio's download, prepare or cache helpers, so no network request can
// originate from this bridge and a failed load deletes nothing.

import AVFoundation
import CoreML
import FluidAudio
import Foundation

// MARK: - Plumbing

private enum BridgeError: Error, LocalizedError {
    case notLoaded(String)
    case missingFile(String)
    case invalidArgument(String)
    case media(String)

    var errorDescription: String? {
        switch self {
        case .notLoaded(let what): return "\(what) models are not loaded"
        case .missingFile(let path): return "missing model file: \(path)"
        case .invalidArgument(let message): return message
        case .media(let message): return message
        }
    }
}

private final class ResultBox<T>: @unchecked Sendable {
    var result: Result<T, Error>?
}

/// Carries a non-Sendable FluidAudio object into the task `blocking` starts;
/// the owning thread waits for that task, so the object is never shared.
private struct Exclusive<T>: @unchecked Sendable {
    let value: T
}

/// Runs `body` on the Swift concurrency pool and blocks the calling thread
/// until it finishes. The caller is a plain OS thread owned by Rust, never a
/// pool thread, so the wait cannot starve the task it waits for.
private func blocking<T>(_ body: @escaping @Sendable () async throws -> T) throws -> T {
    let semaphore = DispatchSemaphore(value: 0)
    let box = ResultBox<T>()
    Task.detached(priority: .userInitiated) {
        do {
            box.result = .success(try await body())
        } catch {
            box.result = .failure(error)
        }
        semaphore.signal()
    }
    semaphore.wait()
    guard let result = box.result else {
        throw BridgeError.invalidArgument("task finished without a result")
    }
    return try result.get()
}

private func describe(_ error: Error) -> String {
    if let localized = error as? LocalizedError, let text = localized.errorDescription {
        return text
    }
    return String(describing: error)
}

private func fail(_ error: Error, _ outError: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?) -> Int32 {
    outError?.pointee = strdup(describe(error))
    return 1
}

/// `object` with every NaN or infinity replaced by 0. `JSONSerialization`
/// raises an Objective-C exception on a non-finite number — Swift cannot
/// catch that, so keeper would abort — and one can come from an indefinite
/// track duration or a degenerate embedding.
private func finiteNumbers(_ object: Any) -> Any {
    switch object {
    case is Bool, is Int, is String:
        return object
    case let number as Double:
        return number.isFinite ? number : 0.0
    case let number as Float:
        return number.isFinite ? number : Float(0)
    case let dictionary as [String: Any]:
        return dictionary.mapValues(finiteNumbers)
    case let array as [Any]:
        return array.map(finiteNumbers)
    default:
        return object
    }
}

private func emitJSON(_ object: Any, _ out: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?) throws {
    let clean = finiteNumbers(object)
    guard JSONSerialization.isValidJSONObject(clean) else {
        throw BridgeError.invalidArgument("result cannot be written as JSON")
    }
    let data = try JSONSerialization.data(withJSONObject: clean, options: [])
    guard let text = String(data: data, encoding: .utf8) else {
        throw BridgeError.invalidArgument("result is not UTF-8")
    }
    out?.pointee = strdup(text)
}

private func samplesArray(_ samples: UnsafePointer<Float>?, _ count: Int) -> [Float] {
    guard let samples, count > 0 else { return [] }
    return Array(UnsafeBufferPointer(start: samples, count: count))
}

/// Refuses a model directory before Core ML sees it: every `.mlmodelc` must
/// carry its spec, program and weights, and every plain file must exist.
private func requireFiles(in directory: URL, models: [String], files: [String]) throws {
    let fm = FileManager.default
    for model in models {
        for part in ["coremldata.bin", "model.mil", "weights/weight.bin"] {
            let path = directory.appendingPathComponent(model).appendingPathComponent(part).path
            guard fm.fileExists(atPath: path) else { throw BridgeError.missingFile(path) }
        }
    }
    for file in files {
        let path = directory.appendingPathComponent(file).path
        guard fm.fileExists(atPath: path) else { throw BridgeError.missingFile(path) }
    }
}

private func loadCompiledModel(_ directory: URL, _ name: String, _ units: MLComputeUnits) throws -> MLModel {
    let configuration = MLModelConfiguration()
    configuration.computeUnits = units
    return try MLModel(contentsOf: directory.appendingPathComponent(name), configuration: configuration)
}

// MARK: - Engine state

private let asrModelFiles = ["Preprocessor.mlmodelc", "Encoder.mlmodelc", "Decoder.mlmodelc", "JointDecisionv3.mlmodelc"]
private let asrPlainFiles = ["parakeet_vocab.json"]
private let diarizerModelFiles = ["Segmentation.mlmodelc", "FBank.mlmodelc", "Embedding.mlmodelc", "PldaRho.mlmodelc"]
private let diarizerPlainFiles = ["plda-parameters.json"]

final class FluidAudioEngine: @unchecked Sendable {
    // Accessed only from the one Rust thread that owns the handle; the Rust
    // type is `Send` but not `Sync` and every method takes `&mut self`.
    private var asrManager: AsrManager?
    private var diarizer: OfflineDiarizerManager?

    func loadAsr(directory: URL) throws {
        try requireFiles(in: directory, models: asrModelFiles, files: asrPlainFiles)
        // `loadLocal` reads exactly this directory and never touches ModelHub.
        let models = try AsrModels.loadLocal(from: directory, version: .v3, encoderPrecision: .int8)
        asrManager = AsrManager(config: .default, models: models)
    }

    func transcribe(_ samples: [Float], language: String?) throws -> [String: Any] {
        guard let manager = asrManager else { throw BridgeError.notLoaded("ASR") }
        var filter: Language?
        if let language {
            guard let parsed = Language(rawValue: language) else {
                throw BridgeError.invalidArgument("unsupported language: \(language)")
            }
            filter = parsed
        }
        let minimum = Int(ASRConstants.minimumAudioDurationSeconds * 16_000)
        if samples.count < minimum {
            return ["text": "", "confidence": 0.0, "tokens": [[String: Any]]()]
        }
        let result = try blocking { [filter] in
            // A fresh decoder state per call: state carried over biases the
            // next buffer's first tokens.
            var state = try TdtDecoderState()
            return try await manager.transcribe(samples, decoderState: &state, language: filter)
        }
        let tokens: [[String: Any]] = (result.tokenTimings ?? []).map { timing in
            // Parakeet v3's vocabulary spells the SentencePiece word start as a
            // leading space; the API promises the canonical '▁'.
            [
                "text": timing.token.hasPrefix(" ") ? "\u{2581}" + timing.token.dropFirst() : timing.token,
                "start": timing.startTime,
                "end": timing.endTime,
                "confidence": timing.confidence,
            ]
        }
        return ["text": result.text, "confidence": result.confidence, "tokens": tokens]
    }

    func loadDiarizer(directory: URL) throws {
        try requireFiles(in: directory, models: diarizerModelFiles, files: diarizerPlainFiles)
        let start = Date()
        // Same compute units FluidAudio's own loader picks; built by hand so
        // its download-and-purge fallback is never reachable.
        let models = OfflineDiarizerModels(
            segmentationModel: try loadCompiledModel(directory, "Segmentation.mlmodelc", .all),
            fbankModel: try loadCompiledModel(directory, "FBank.mlmodelc", .cpuOnly),
            embeddingModel: try loadCompiledModel(directory, "Embedding.mlmodelc", .all),
            pldaRhoModel: try loadCompiledModel(directory, "PldaRho.mlmodelc", .all),
            pldaPsi: try loadPldaPsi(directory.appendingPathComponent("plda-parameters.json")),
            compilationDuration: Date().timeIntervalSince(start)
        )
        let manager = OfflineDiarizerManager(config: .default)
        manager.initialize(models: models)
        diarizer = manager
    }

    private func loadPldaPsi(_ url: URL) throws -> [Double] {
        let data = try Data(contentsOf: url)
        guard
            let root = try JSONSerialization.jsonObject(with: data) as? [String: Any],
            let tensors = root["tensors"] as? [String: Any],
            let psi = tensors["psi"] as? [String: Any],
            let base64 = psi["data_base64"] as? String,
            let decoded = Data(base64Encoded: base64, options: [.ignoreUnknownCharacters]),
            decoded.count >= MemoryLayout<Float>.size
        else {
            throw BridgeError.invalidArgument("plda-parameters.json has no psi tensor")
        }
        var floats = [Float](repeating: 0, count: decoded.count / MemoryLayout<Float>.size)
        _ = floats.withUnsafeMutableBytes { decoded.copyBytes(to: $0) }
        return floats.map { Double($0) }
    }

    /// Diarizes `samples`; audio with no speech is an empty result, not an error.
    private func runDiarizer(_ samples: [Float]) throws -> DiarizationResult? {
        // `process` would fall back to `prepareModels()` — a download — when
        // no models are set, so it is only reached once `initialize` ran.
        guard let manager = diarizer else { throw BridgeError.notLoaded("diarizer") }
        let exclusive = Exclusive(value: manager)
        do {
            return try blocking { try await exclusive.value.process(audio: samples) }
        } catch OfflineDiarizationError.noSpeechDetected {
            return nil
        }
    }

    /// Per-speaker mean embeddings: the pipeline's own speaker database, or the
    /// mean of each speaker's segment embeddings when it has none.
    private static func speakerEmbeddings(_ result: DiarizationResult) -> [String: [Float]] {
        if let database = result.speakerDatabase, !database.isEmpty { return database }
        var sums: [String: [Float]] = [:]
        var counts: [String: Int] = [:]
        for segment in result.segments where !segment.embedding.isEmpty {
            if var sum = sums[segment.speakerId], sum.count == segment.embedding.count {
                for i in sum.indices { sum[i] += segment.embedding[i] }
                sums[segment.speakerId] = sum
            } else {
                sums[segment.speakerId] = segment.embedding
            }
            counts[segment.speakerId, default: 0] += 1
        }
        return sums.reduce(into: [:]) { out, entry in
            let n = Float(counts[entry.key] ?? 1)
            out[entry.key] = entry.value.map { $0 / n }
        }
    }

    func diarize(_ samples: [Float]) throws -> [String: Any] {
        guard let result = try runDiarizer(samples) else {
            return ["segments": [[String: Any]](), "speakers": [[String: Any]]()]
        }
        let segments: [[String: Any]] = result.segments.map {
            ["speaker": $0.speakerId, "start": Double($0.startTimeSeconds), "end": Double($0.endTimeSeconds)]
        }
        let speakers: [[String: Any]] = Self.speakerEmbeddings(result)
            .sorted { $0.key.localizedStandardCompare($1.key) == .orderedAscending }
            .map { ["speaker": $0.key, "embedding": $0.value] }
        return ["segments": segments, "speakers": speakers]
    }

    func embed(_ samples: [Float]) throws -> [String: Any] {
        guard let result = try runDiarizer(samples) else { return ["embedding": NSNull()] }
        var spoken: [String: Float] = [:]
        for segment in result.segments {
            spoken[segment.speakerId, default: 0] += segment.durationSeconds
        }
        let embeddings = Self.speakerEmbeddings(result)
        // A vector with a NaN in it would recognize nobody; zeroing it (as
        // `emitJSON` would) would recognize the wrong person.
        guard
            let dominant = spoken.max(by: { $0.value < $1.value })?.key,
            let embedding = embeddings[dominant],
            embedding.allSatisfy(\.isFinite)
        else {
            return ["embedding": NSNull()]
        }
        return ["embedding": embedding]
    }
}

// MARK: - Media (AVAssetReader: AVAudioFile cannot open a .mov with video)

private let targetSampleRate: Double = 16_000

private func audioTracks(of url: URL) throws -> (AVURLAsset, [AVAssetTrack]) {
    guard FileManager.default.fileExists(atPath: url.path) else {
        throw BridgeError.media("no such file: \(url.path)")
    }
    let asset = AVURLAsset(url: url)
    let tracks = try blocking { try await asset.loadTracks(withMediaType: .audio) }
    return (asset, tracks)
}

private func listTracks(_ url: URL) throws -> [[String: Any]] {
    let (_, tracks) = try audioTracks(of: url)
    return try tracks.enumerated().map { index, track in
        let (descriptions, range) = try blocking { try await track.load(.formatDescriptions, .timeRange) }
        var channels: UInt32 = 0
        if let description = descriptions.first,
            let basic = CMAudioFormatDescriptionGetStreamBasicDescription(description)
        {
            channels = basic.pointee.mChannelsPerFrame
        }
        return ["index": index, "channels": channels, "duration": range.duration.seconds]
    }
}

private typealias SampleSink = @convention(c) (UnsafeMutableRawPointer?, UnsafePointer<Float>?, Int) -> Void

/// Decodes one audio track (`trackIndex >= 0`) or a mix of every audio track
/// to 16 kHz mono Float32, streaming each decoded buffer into `sink`.
private func decode(
    _ url: URL, trackIndex: Int32, start: Double, end: Double,
    context: UnsafeMutableRawPointer?, sink: SampleSink
) throws {
    let (asset, tracks) = try audioTracks(of: url)
    guard !tracks.isEmpty else { throw BridgeError.media("\(url.lastPathComponent) has no audio track") }
    let settings: [String: Any] = [
        AVFormatIDKey: kAudioFormatLinearPCM,
        AVSampleRateKey: targetSampleRate,
        AVNumberOfChannelsKey: 1,
        AVLinearPCMBitDepthKey: 32,
        AVLinearPCMIsFloatKey: true,
        AVLinearPCMIsBigEndianKey: false,
        AVLinearPCMIsNonInterleaved: false,
    ]
    let reader = try AVAssetReader(asset: asset)
    let output: AVAssetReaderOutput
    if trackIndex < 0 {
        output = AVAssetReaderAudioMixOutput(audioTracks: tracks, audioSettings: settings)
    } else {
        guard Int(trackIndex) < tracks.count else {
            throw BridgeError.media("audio track \(trackIndex) out of range (\(tracks.count) tracks)")
        }
        output = AVAssetReaderTrackOutput(track: tracks[Int(trackIndex)], outputSettings: settings)
    }
    output.alwaysCopiesSampleData = false
    guard reader.canAdd(output) else { throw BridgeError.media("cannot read audio from \(url.lastPathComponent)") }
    reader.add(output)
    if start >= 0, end > start {
        let timescale: CMTimeScale = 48_000
        reader.timeRange = CMTimeRange(
            start: CMTime(seconds: start, preferredTimescale: timescale),
            end: CMTime(seconds: end, preferredTimescale: timescale))
    }
    guard reader.startReading() else {
        throw BridgeError.media(reader.error.map(describe) ?? "cannot start reading \(url.lastPathComponent)")
    }
    while let sampleBuffer = output.copyNextSampleBuffer() {
        guard let block = CMSampleBufferGetDataBuffer(sampleBuffer) else { continue }
        var length = 0
        var pointer: UnsafeMutablePointer<CChar>?
        let status = CMBlockBufferGetDataPointer(
            block, atOffset: 0, lengthAtOffsetOut: nil, totalLengthOut: &length, dataPointerOut: &pointer)
        if status == kCMBlockBufferNoErr, let pointer, CMBlockBufferIsRangeContiguous(block, atOffset: 0, length: length) {
            pointer.withMemoryRebound(to: Float.self, capacity: length / 4) {
                sink(context, $0, length / 4)
            }
        } else {
            var copy = [Float](repeating: 0, count: length / 4)
            let copied = copy.withUnsafeMutableBytes {
                CMBlockBufferCopyDataBytes(block, atOffset: 0, dataLength: length, destination: $0.baseAddress!)
            }
            guard copied == kCMBlockBufferNoErr else { throw BridgeError.media("cannot copy decoded audio") }
            copy.withUnsafeBufferPointer { sink(context, $0.baseAddress, $0.count) }
        }
    }
    if reader.status == .failed {
        throw BridgeError.media(reader.error.map(describe) ?? "decoding \(url.lastPathComponent) failed")
    }
}

// MARK: - C ABI

private func engine(_ handle: UnsafeMutableRawPointer?) throws -> FluidAudioEngine {
    guard let handle else { throw BridgeError.invalidArgument("null engine handle") }
    return Unmanaged<FluidAudioEngine>.fromOpaque(handle).takeUnretainedValue()
}

private func path(_ raw: UnsafePointer<CChar>?) throws -> URL {
    guard let raw else { throw BridgeError.invalidArgument("null path") }
    return URL(fileURLWithPath: String(cString: raw))
}

@_cdecl("fluidaudio_engine_create")
public func fluidaudio_engine_create() -> UnsafeMutableRawPointer {
    Unmanaged.passRetained(FluidAudioEngine()).toOpaque()
}

@_cdecl("fluidaudio_engine_destroy")
public func fluidaudio_engine_destroy(_ handle: UnsafeMutableRawPointer?) {
    guard let handle else { return }
    Unmanaged<FluidAudioEngine>.fromOpaque(handle).release()
}

@_cdecl("fluidaudio_free_string")
public func fluidaudio_free_string(_ text: UnsafeMutablePointer<CChar>?) {
    free(text)
}

@_cdecl("fluidaudio_load_asr")
public func fluidaudio_load_asr(
    _ handle: UnsafeMutableRawPointer?, _ directory: UnsafePointer<CChar>?,
    _ outError: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> Int32 {
    do {
        try engine(handle).loadAsr(directory: try path(directory))
        return 0
    } catch {
        return fail(error, outError)
    }
}

@_cdecl("fluidaudio_transcribe")
public func fluidaudio_transcribe(
    _ handle: UnsafeMutableRawPointer?, _ samples: UnsafePointer<Float>?, _ count: Int,
    _ language: UnsafePointer<CChar>?,
    _ outJson: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?,
    _ outError: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> Int32 {
    do {
        let result = try engine(handle).transcribe(
            samplesArray(samples, count), language: language.map { String(cString: $0) })
        try emitJSON(result, outJson)
        return 0
    } catch {
        return fail(error, outError)
    }
}

@_cdecl("fluidaudio_load_diarizer")
public func fluidaudio_load_diarizer(
    _ handle: UnsafeMutableRawPointer?, _ directory: UnsafePointer<CChar>?,
    _ outError: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> Int32 {
    do {
        try engine(handle).loadDiarizer(directory: try path(directory))
        return 0
    } catch {
        return fail(error, outError)
    }
}

@_cdecl("fluidaudio_diarize")
public func fluidaudio_diarize(
    _ handle: UnsafeMutableRawPointer?, _ samples: UnsafePointer<Float>?, _ count: Int,
    _ outJson: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?,
    _ outError: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> Int32 {
    do {
        try emitJSON(try engine(handle).diarize(samplesArray(samples, count)), outJson)
        return 0
    } catch {
        return fail(error, outError)
    }
}

@_cdecl("fluidaudio_embed")
public func fluidaudio_embed(
    _ handle: UnsafeMutableRawPointer?, _ samples: UnsafePointer<Float>?, _ count: Int,
    _ outJson: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?,
    _ outError: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> Int32 {
    do {
        try emitJSON(try engine(handle).embed(samplesArray(samples, count)), outJson)
        return 0
    } catch {
        return fail(error, outError)
    }
}

@_cdecl("fluidaudio_audio_tracks")
public func fluidaudio_audio_tracks(
    _ media: UnsafePointer<CChar>?,
    _ outJson: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?,
    _ outError: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> Int32 {
    do {
        try emitJSON(try listTracks(try path(media)), outJson)
        return 0
    } catch {
        return fail(error, outError)
    }
}

@_cdecl("fluidaudio_decode")
public func fluidaudio_decode(
    _ media: UnsafePointer<CChar>?, _ trackIndex: Int32, _ start: Double, _ end: Double,
    _ context: UnsafeMutableRawPointer?,
    _ sink: @convention(c) (UnsafeMutableRawPointer?, UnsafePointer<Float>?, Int) -> Void,
    _ outError: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> Int32 {
    do {
        try decode(try path(media), trackIndex: trackIndex, start: start, end: end, context: context, sink: sink)
        return 0
    } catch {
        return fail(error, outError)
    }
}
