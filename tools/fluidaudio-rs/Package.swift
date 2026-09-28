// swift-tools-version:6.2
import PackageDescription

let package = Package(
    name: "FluidAudioBridge",
    platforms: [
        .macOS(.v14)
    ],
    products: [
        .library(
            name: "FluidAudioBridge",
            type: .static,
            targets: ["FluidAudioBridge"]
        )
    ],
    dependencies: [
        // `traits: []` drops the NeMo text-normalization engine: it is a
        // prebuilt Rust staticlib whose runtime symbols would collide with the
        // Rust binary this library links into (FluidAudio #880, #888), and
        // keeper uses no TTS or ITN.
        .package(url: "https://github.com/FluidInference/FluidAudio.git", exact: "0.17.4", traits: [])
    ],
    targets: [
        .target(
            name: "FluidAudioBridge",
            dependencies: [
                .product(name: "FluidAudio", package: "FluidAudio")
            ],
            path: "swift"
        )
    ],
    swiftLanguageModes: [.v5]
)
