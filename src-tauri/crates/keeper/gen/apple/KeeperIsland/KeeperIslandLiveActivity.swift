// SPDX-License-Identifier: Apache-2.0
//
// The card (Epic 65, Story 65.5, FR-453–FR-455, AD-194): what the Dynamic
// Island and the lock screen show while keeper's ear is doing something.
//
// Four presentations, all mandatory for a Live Activity: the lock-screen
// card, the expanded island, the compact leading/trailing pair, and the
// minimal circle. Each is the same three things — a symbol, a state line,
// and the failure's sentence when there is one — cut to the space. No
// animation beyond what the system does when the island opens and closes.
//
// Nothing is decided here: the word and the sentence arrive from Rust
// (`keeper_core::voice::island`), and an unknown word is shown as itself
// rather than hidden, so a mismatch between the two sides is visible.

import ActivityKit
import SwiftUI
import WidgetKit

/// DESIGN.md's tokens as SwiftUI colours, by name, so a retheme is one
/// edit here beside the one in `src/index.css`. The card is drawn in the
/// dark set whatever the phone's appearance: the Dynamic Island is black,
/// and the lock screen is the workroom at night.
private enum Ink {
    /// `ground` — the lock-screen card's tint.
    static let ground = rgb(0x0d, 0x12, 0x10)
    /// `text` — the state line and the app's name.
    static let text = rgb(0xe4, 0xe9, 0xe4)
    /// `text-dim` — the phrase, the failure's sentence, a model thinking.
    static let textDim = rgb(0x98, 0xa4, 0x9b)
    /// `accent` — lichen: the ear is open (armed, listening, speaking).
    static let accent = rgb(0x8f, 0xc6, 0x59)
    /// `ok` — teal: something was heard, an answer is arriving.
    static let ok = rgb(0x2f, 0xb8, 0xa0)
    /// `danger` — the turn stopped on an error.
    static let danger = rgb(0xe0, 0x62, 0x5a)

    private static func rgb(_ red: Int, _ green: Int, _ blue: Int) -> Color {
        Color(
            red: Double(red) / 255,
            green: Double(green) / 255,
            blue: Double(blue) / 255
        )
    }
}

/// One word, cut for the card: the SF Symbol, its tint, the line, and the
/// short form for the compact trailing slot.
private struct Face {
    let symbol: String
    let tint: Color
    let line: String
    let short: String

    init(word: String, phrase: String) {
        switch word {
        case "armed":
            symbol = "ear"
            tint = Ink.accent
            line = phrase.isEmpty ? "Listening for your phrase" : "Listening for “\(phrase)”"
            short = phrase.isEmpty ? "Listening" : phrase
        case "listening":
            symbol = "waveform"
            tint = Ink.accent
            line = "Listening"
            short = "Listening"
        case "heard":
            symbol = "text.bubble"
            tint = Ink.ok
            line = "Heard you"
            short = "Heard"
        case "thinking":
            symbol = "ellipsis.circle"
            tint = Ink.textDim
            line = "Thinking"
            short = "Thinking"
        case "answering":
            symbol = "text.alignleft"
            tint = Ink.ok
            line = "Answering"
            short = "Answering"
        case "speaking":
            symbol = "speaker.wave.2"
            tint = Ink.accent
            line = "Speaking"
            short = "Speaking"
        case "failed":
            symbol = "exclamationmark.triangle"
            tint = Ink.danger
            line = "Stopped"
            short = "Stopped"
        case "off":
            symbol = "ear"
            tint = Ink.textDim
            line = "Not listening"
            short = "Off"
        default:
            // A word this extension does not know: shown as itself, so a
            // bridge and an extension that disagree are caught by eye.
            symbol = "questionmark.circle"
            tint = Ink.textDim
            line = word
            short = word
        }
    }
}

struct KeeperIslandLiveActivity: Widget {
    var body: some WidgetConfiguration {
        ActivityConfiguration(for: KeeperIslandAttributes.self) { context in
            LockScreenCard(
                face: Face(word: context.state.word, phrase: context.attributes.phrase),
                detail: context.state.detail
            )
            .activityBackgroundTint(Ink.ground)
            .activitySystemActionForegroundColor(Ink.text)
        } dynamicIsland: { context in
            let face = Face(word: context.state.word, phrase: context.attributes.phrase)
            return DynamicIsland {
                DynamicIslandExpandedRegion(.leading) {
                    Image(systemName: face.symbol)
                        .font(.title2)
                        .foregroundStyle(face.tint)
                        .frame(width: 44, height: 44)
                }
                DynamicIslandExpandedRegion(.center) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("keeper")
                            .font(.caption)
                            .foregroundStyle(Ink.textDim)
                        Text(face.line)
                            .font(.headline)
                            .foregroundStyle(Ink.text)
                            .lineLimit(1)
                    }
                }
                DynamicIslandExpandedRegion(.bottom) {
                    if !context.state.detail.isEmpty {
                        Text(context.state.detail)
                            .font(.footnote)
                            .foregroundStyle(Ink.textDim)
                            .lineLimit(2)
                    }
                }
            } compactLeading: {
                Image(systemName: face.symbol)
                    .foregroundStyle(face.tint)
            } compactTrailing: {
                Text(face.short)
                    .font(.caption2)
                    .foregroundStyle(Ink.text)
                    .lineLimit(1)
            } minimal: {
                Image(systemName: face.symbol)
                    .foregroundStyle(face.tint)
            }
            .keylineTint(face.tint)
        }
    }
}

/// The lock-screen card: the symbol, keeper's name, the state line, and the
/// failure's sentence when there is one.
private struct LockScreenCard: View {
    let face: Face
    let detail: String

    var body: some View {
        HStack(alignment: .center, spacing: 12) {
            Image(systemName: face.symbol)
                .font(.title)
                .foregroundStyle(face.tint)
                .frame(width: 40)
            VStack(alignment: .leading, spacing: 2) {
                Text("keeper")
                    .font(.caption)
                    .foregroundStyle(Ink.textDim)
                Text(face.line)
                    .font(.headline)
                    .foregroundStyle(Ink.text)
                    .lineLimit(1)
                if !detail.isEmpty {
                    Text(detail)
                        .font(.footnote)
                        .foregroundStyle(Ink.textDim)
                        .lineLimit(2)
                }
            }
            Spacer(minLength: 0)
        }
        .padding(16)
    }
}
