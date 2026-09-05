// SPDX-License-Identifier: Apache-2.0
//
// The bridge between Rust and ActivityKit (Epic 65, Story 65.5, AD-194).
//
// ActivityKit is Swift-only: `Activity` is a generic Swift class with no
// Objective-C surface, so the shell's objc2 crates cannot reach it and this
// file is the first Swift compiled into the app target. `crates/keeper/src/
// voice_island.rs` declares the four functions below in an `extern "C"`
// block; `libapp.a` is a static archive the Xcode project links, so its
// undefined symbols resolve against the `@_cdecl` names here at the app's
// link. Nothing is decided here — which word, and whether to start, update
// or end, is `keeper_core::voice::island` — and nothing here touches the
// network, the file system or the main thread.
//
// Threads: Rust calls `keeper_island_start` with its voice lock held, from
// whichever thread moved the turn. `Activity.request` is synchronous and
// documented for any thread, so the start answers in place — the refusal,
// if any, is the whole point of the return value. `update` and `end` are
// `async` in ActivityKit; they are queued on one `AsyncStream` consumed by
// one task, so two updates a few milliseconds apart land in the order Rust
// sent them and the caller never waits.
//
// Ownership: a refusal is returned as a C string allocated with `strdup`,
// which Rust reads and hands back to `keeper_island_free`. Rust never frees
// it with its own allocator and Swift never frees Rust's buffers.

import ActivityKit
import Foundation

/// One queued ActivityKit call, run by the island's single consumer.
private typealias IslandOp = @Sendable () async -> Void

/// The one activity keeper keeps, and the queue its updates go through.
@available(iOS 16.2, *)
private final class Island {
    static let shared = Island()

    private let lock = NSLock()
    private var activity: Activity<KeeperIslandAttributes>?
    private let enqueue: AsyncStream<IslandOp>.Continuation

    private init() {
        var continuation: AsyncStream<IslandOp>.Continuation!
        let ops = AsyncStream<IslandOp> { continuation = $0 }
        enqueue = continuation
        // One consumer for the life of the process: FIFO by construction.
        Task.detached {
            for await op in ops {
                await op()
            }
        }
    }

    /// `Activity.request`. `nil` when the card is up; otherwise the refusal
    /// in a sentence a person can read in Settings → Bots.
    func start(word: String, phrase: String) -> String? {
        guard ActivityAuthorizationInfo().areActivitiesEnabled else {
            return "Live Activities are off for keeper: Settings > keeper > Live Activities"
        }
        lock.lock()
        defer { lock.unlock() }
        // A card left from an earlier request goes first, so there is never
        // more than one of keeper's in the island.
        if let previous = activity {
            activity = nil
            enqueue.yield { await previous.end(nil, dismissalPolicy: .immediate) }
        }
        let content = ActivityContent(
            state: KeeperIslandAttributes.ContentState(word: word, detail: ""),
            staleDate: nil
        )
        do {
            activity = try Activity.request(
                attributes: KeeperIslandAttributes(phrase: phrase),
                content: content,
                pushType: nil
            )
            return nil
        } catch {
            return Island.describe(error)
        }
    }

    func update(word: String, detail: String) {
        lock.lock()
        let current = activity
        lock.unlock()
        guard let current else { return }
        let content = ActivityContent(
            state: KeeperIslandAttributes.ContentState(word: word, detail: detail),
            staleDate: nil
        )
        enqueue.yield { await current.update(content) }
    }

    func end(word: String, detail: String, lingerSeconds: UInt32) {
        lock.lock()
        let current = activity
        activity = nil
        lock.unlock()
        guard let current else { return }
        let content = ActivityContent(
            state: KeeperIslandAttributes.ContentState(word: word, detail: detail),
            staleDate: nil
        )
        // `.after` keeps the final content on the lock screen until the date
        // (the system caps it at four hours); zero is at once.
        let policy: ActivityUIDismissalPolicy = lingerSeconds == 0
            ? .immediate
            : .after(Date(timeIntervalSinceNow: TimeInterval(lingerSeconds)))
        enqueue.yield { await current.end(content, dismissalPolicy: policy) }
    }

    /// The refusal as one sentence: Apple's case name first (`unentitled`,
    /// `unsupportedTarget`, `visibility`, `denied`, …) because that name on
    /// a Personal Team is the measurement the epic asks for, then Apple's
    /// own reason and remedy where it gives them.
    private static func describe(_ error: Error) -> String {
        guard let auth = error as? ActivityAuthorizationError else {
            return "Live Activity refused: \(error.localizedDescription)"
        }
        var sentence = "Live Activity refused: \(auth)"
        if let why = auth.failureReason {
            sentence += " — \(why)"
        }
        if let fix = auth.recoverySuggestion {
            sentence += " \(fix)"
        }
        return sentence
    }
}

/// Request the card for `word` and `phrase`. Returns `NULL` when it is up,
/// else a `strdup`'d sentence the caller releases with `keeper_island_free`.
@_cdecl("keeper_island_start")
public func keeper_island_start(
    _ word: UnsafePointer<CChar>,
    _ phrase: UnsafePointer<CChar>
) -> UnsafeMutablePointer<CChar>? {
    guard #available(iOS 16.2, *) else {
        return strdup("Live Activity refused: this iPhone runs an iOS older than 16.2")
    }
    guard let reason = Island.shared.start(
        word: String(cString: word),
        phrase: String(cString: phrase)
    ) else {
        return nil
    }
    return strdup(reason)
}

/// Move the running card to `word`; nothing when there is none.
@_cdecl("keeper_island_update")
public func keeper_island_update(_ word: UnsafePointer<CChar>, _ detail: UnsafePointer<CChar>) {
    if #available(iOS 16.2, *) {
        Island.shared.update(word: String(cString: word), detail: String(cString: detail))
    }
}

/// End the running card, leaving `word` and `detail` on it for
/// `lingerSeconds` (zero: removed at once); nothing when there is none.
@_cdecl("keeper_island_end")
public func keeper_island_end(
    _ word: UnsafePointer<CChar>,
    _ detail: UnsafePointer<CChar>,
    _ lingerSeconds: UInt32
) {
    if #available(iOS 16.2, *) {
        Island.shared.end(
            word: String(cString: word),
            detail: String(cString: detail),
            lingerSeconds: lingerSeconds
        )
    }
}

/// Release a sentence `keeper_island_start` returned.
@_cdecl("keeper_island_free")
public func keeper_island_free(_ reason: UnsafeMutablePointer<CChar>?) {
    free(reason)
}
