// swift-tools-version:6.0
// SPDX-License-Identifier: Apache-2.0
import PackageDescription

let package = Package(
    name: "keeper-rec",
    platforms: [
        .macOS(.v13),
    ],
    targets: [
        .executableTarget(
            name: "keeper-rec",
            path: "Sources/keeper-rec",
            // CoreGraphics is used for the Screen Recording preflight
            // (`CGPreflightScreenCaptureAccess`) and active-display enumeration
            // (`CGGetActiveDisplayList`). SwiftPM auto-links it via the SDK
            // umbrella on macOS, but link it explicitly so the build stays
            // reproducible under stricter/explicit-linking toolchains.
            // ScreenCaptureKit / AVFoundation are NOT linked here — they land
            // with real capture (16.6 / 19).
            linkerSettings: [
                .linkedFramework("CoreGraphics"),
            ]
        ),
    ]
)
