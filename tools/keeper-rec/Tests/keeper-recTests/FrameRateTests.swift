// SPDX-License-Identifier: Apache-2.0
//
// Unit tests for the pure frame-rate normalization policy (Story 19.5). No
// capture hardware, no code-signing: `normalizeFps` is Foundation-only (the
// Rotation.swift pattern). The contract: {10, 15, 30, 60} pass through,
// everything else — absent-field default, corruption, hostile values —
// becomes 30, so a degenerate `CMTime` timescale can never reach
// `SCStreamConfiguration`.

import XCTest

@testable import keeper_rec

final class FrameRateTests: XCTestCase {
    func testLegalValuesPassThrough() {
        for fps in [10, 15, 30, 60] {
            XCTAssertEqual(normalizeFps(fps), fps, "fps \(fps) must pass through")
        }
    }

    func testOutOfSetValuesNormalizeToThirty() {
        // Near-misses, zero, negatives, and absurd values all collapse to the
        // default — never a pass-through, never a crash.
        for fps in [0, 1, 9, 11, 14, 16, 29, 31, 45, 59, 61, 120, -1, -60, Int.max, Int.min] {
            XCTAssertEqual(normalizeFps(fps), 30, "fps \(fps) must normalize to 30")
        }
    }
}
