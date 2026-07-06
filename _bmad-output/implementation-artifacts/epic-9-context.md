# Epic 9 Context: Command Palette, Hotkeys & Keyboard Mastery

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

This epic makes keeper fully operable from the keyboard, in the Texts/Beeper tradition: a ⌘K command palette that fuzzy-finds any Chat, contact, or action; a complete shortcut set that lets a user run the whole triage loop (walk unreads → archive → reply → next) without touching the mouse; a cheat sheet and native macOS menu bar generated from a single action registry so the reference can never drift from reality; and a system-wide global hotkey that summons or hides keeper from any app. It matters because speed and keyboard-first mastery are the product's core differentiator (the "40 chats in four minutes" triage promise), and because palette parity is a release gate that guarantees every feature is reachable.

## Stories

- Story 9.1: Command Palette
- Story 9.2: Keyboard Navigation and Quick-Switcher
- Story 9.3: Cheat Sheet and Native Menu Bar from the Action Registry
- Story 9.4: Global Hotkey

## Requirements & Constraints

- **Palette search performance:** typing ≥ 2 characters filters across Chats (all Accounts), contacts, and a registered action list; results must render within 100 ms per keystroke at 10k Chats. Enter executes any result.
- **Palette parity is a release gate:** every MVP feature with a UI surface must be reachable through at least one palette action. A checklist or automated test must verify this at release. Every MVP feature is required to register at least one action.
- **Full keyboard operability:** the UJ-3 triage loop (walk unreads → archive → reply → next) must complete with zero pointer use. This is a superset of the general accessibility bar — all MVP flows operable via keyboard alone.
- **Accessibility baseline:** interactive controls carry accessibility labels for VoiceOver; the native menu bar gives full-keyboard-access and VoiceOver users standard command discovery. (Full VoiceOver timeline-navigation polish is out of MVP scope; the MVP bar is "operable and labeled.")
- **Single source of truth:** the cheat sheet and native menu bar are generated from the same action registry the palette consumes — no hand-maintained shortcut list.
- **Global hotkey:** works while keeper is backgrounded or hidden (given macOS permissions), raising the main window with focus in the Unified Inbox chat list; pressed while focused, it hides the window. Reassignment must detect conflicts with existing system shortcuts at assignment time and warn. If permission is not granted, the setting must explain what to enable rather than fail silently.

## Technical Decisions

- **Palette/Quick-Switcher index lives in Rust.** The chat/action index is an in-memory structure in `keeper-core` (co-located with the inbox projection), queried via a command; the ≤ 100 ms-at-10k-Chats budget is met there. Ordering and filtering are never re-derived in TypeScript. Quick-Switcher rides this same index.
- **Action registry is the architectural spine of the epic.** A single action-registry module is the sole source for palette actions; palette (9.1), cheat sheet and menu bar (9.3) all consume it. Context-aware ranking: actions on the currently open Chat rank first.
- **Settings live in Rust.** Hotkey configuration and any shortcut preferences persist via `keeper-core::settings` in `keeper.db`, exposed through commands + a settings stream. No JS-writable config store (tauri-plugin-store / tauri-plugin-sql are not used).
- **Global hotkey uses the Tauri global-shortcut plugin** (desktop-only; a known non-portable capability accessed only through the platform port). Related plugins in the set: notification, autostart, window-state.
- **Command / event / DTO conventions:** commands are `domain_verb` snake_case; events `keeper://kebab-case`; view-model DTOs live in `keeper-core::vm` with `Vm` suffix, serde `camelCase`, ts-rs exported. Timestamps are integer ms since epoch. No `unwrap`/bare `expect` in production paths; `tracing` only (no `println!`); logs carry ids, never content or tokens.
- **TS conventions:** Biome-enforced (no `any`, `import type`); zustand stores as `use<Domain>Store`; `src/components/ui/` is shadcn-generated only. Tooling is bun only; quality gates `bun run check`, `check:rust`, `test:rust`.

## UX & Interaction Patterns

- **Palette visuals:** shadcn `Command` in a 640 px rounded panel (transient layer, gets shadow). Results show a type glyph (chat/contact/action), a network badge + account hue dot for chats, and a right-aligned kbd chip (SF Mono) for actions with shortcuts. Active row uses stock shadcn accent. `>` prefix switches to action mode (Archive, Toggle Incognito, Open Approval Pane, Start Export, Bridge/Signal operations, …). `Enter` executes; `⌘Enter` on a Chat result peeks (opens without closing the palette). No-matches shows the top registered actions plus a `>` hint. The palette closes anything below it (modal discipline: one dialog level).
- **Core shortcut set:** `⌘K` palette; `⌘1–4` switch views (Inbox / Archive / Approval Pane / Bridges); `⌃Tab` / `⌃⇧Tab` cycle Chats; `⌥⌘↓` / `⌥⌘↑` jump next/previous unread; `↑`/`↓` and `j`/`k` move list selection; `Enter` opens with composer focused.
- **Single-key list verbs (chat row focused):** `e` archive/unarchive, `u` toggle read/unread, `p` pin/unpin, `f` favorite/unfavorite, `m` mute menu.
- **Esc chain (universal):** Esc walks up exactly overlay → composer → timeline → clear filter → chat list.
- **Timeline focus:** `↑`/`↓` select message, `r` reply, `e` edit own, `⌫` opens the delete dialog.
- **Focus model:** visible focus ring on every focusable; roving tabindex in chat list, timeline, and Approval Pane; focus returns to the invoking element when overlays close.
- **Cheat sheet:** `⌘?` opens a searchable Dialog overlay listing all shortcuts, generated from the action registry.
- **Menu bar:** native macOS menu bar mirrors every registered command with its shortcut.
- **Global hotkey default:** `⌃⌥Space`, reassignable in Settings → Shortcuts.
- Banned: hover-only affordances without a keyboard/focus equivalent, modal stacks deeper than one.

## Cross-Story Dependencies

- **9.1 (Palette)** depends on the surfaces and actions built in Epics 4–8 being registerable actions; it establishes the action registry and Rust index that the rest of the epic builds on.
- **9.2 (Keyboard nav / Quick-Switcher)** depends on 9.1 — the Quick-Switcher rides the palette's Rust index.
- **9.3 (Cheat sheet / menu bar)** depends on 9.1 and 9.2 — both surfaces are generated from the shared action registry established there.
- **9.4 (Global hotkey)** depends on 9.2 (window-focus and inbox-focus behavior) and requires macOS accessibility permissions.
- The palette parity release gate (9.3) transitively depends on every MVP epic having registered its actions.
