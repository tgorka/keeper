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
            path: "Sources/keeper-rec"
        ),
    ]
)
