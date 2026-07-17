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
            // Language mode 5: the capture engine (Story 16.6) is a classic
            // delegate + serial-DispatchQueue design (SCStreamOutput callbacks
            // append into AVAssetWriter inputs on one media queue); Swift 6
            // strict concurrency has no non-invasive way to express that
            // queue-confined ownership yet.
            swiftSettings: [
                .swiftLanguageMode(.v5),
            ],
            // CoreGraphics is used for the Screen Recording preflight
            // (`CGPreflightScreenCaptureAccess`), active-display enumeration
            // (`CGGetActiveDisplayList`), and pixel-size lookup. ScreenCaptureKit
            // + AVFoundation drive real capture (Story 16.6): SCStream delivers
            // screen + system-audio sample buffers, AVAssetWriter writes the
            // fragmented MP4. SwiftPM auto-links via the SDK umbrella on macOS,
            // but link explicitly so the build stays reproducible under
            // stricter/explicit-linking toolchains.
            linkerSettings: [
                .linkedFramework("CoreGraphics"),
                .linkedFramework("ScreenCaptureKit"),
                .linkedFramework("AVFoundation"),
                .linkedFramework("CoreMedia"),
            ]
        ),
        // Unit tests for the pure, Foundation-only logic (Rotation.swift,
        // Story 17.1): no capture hardware, no code-signing needed, so they
        // run in CI (`bun run rec:test` → scripts/test-keeper-rec.sh). The
        // test target depends on the executable target directly (supported
        // since Swift 5.5) and uses `@testable import keeper_rec`.
        .testTarget(
            name: "keeper-recTests",
            dependencies: ["keeper-rec"],
            path: "Tests/keeper-recTests",
            swiftSettings: [
                .swiftLanguageMode(.v5),
            ]
        ),
    ]
)
