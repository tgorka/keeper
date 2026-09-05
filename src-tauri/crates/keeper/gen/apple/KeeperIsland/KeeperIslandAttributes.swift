// SPDX-License-Identifier: Apache-2.0
//
// What the island shows for keeper's ear (Epic 65, Story 65.5, AD-194).
//
// Compiled into BOTH targets — `keeper_iOS` (where `Sources/keeper/
// KeeperIsland.swift` requests the activity) and the `KeeperIsland`
// extension (which draws it) — because ActivityKit matches the two by this
// type's name and layout. `project.yml` lists this one file under the app
// target's sources as well as the extension's; a field added on one side
// only is a card that never appears.
//
// The words are `keeper_core::voice::island::Word::as_str` — the decision
// is Rust's, this is the wire.

import ActivityKit
import Foundation

@available(iOS 16.2, *)
struct KeeperIslandAttributes: ActivityAttributes {
    /// The part that changes with every turn state.
    struct ContentState: Codable, Hashable {
        /// One of `armed`, `listening`, `heard`, `thinking`, `answering`,
        /// `speaking`, `failed`, `off`.
        var word: String
        /// The sentence that goes with the word — a failure's reason — or
        /// empty.
        var detail: String
    }

    /// The phrase as armed (matching form), or empty when listening was
    /// started by hand rather than for a phrase.
    var phrase: String
}
