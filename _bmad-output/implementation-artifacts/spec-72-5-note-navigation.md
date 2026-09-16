---
status: review
baseline_revision: a8eb9c2
final_revision: ''
---

# Back and forward through the notes you followed

<intent-contract>
Problem: Following a note replaces the panel with no way back. Revision history is not navigation history.
Approach: Keep bounded transient target stacks per panel, record in setActiveTarget, expose navigation through note headers and the Rust action registry.
Always: Two panels have independent histories; new navigation clears forward; empty directions are no-ops; history is never persisted.
Block If: Navigation would bypass the panel store or mutate document contents.
Never: Add a second history in the editor or confuse revisions with navigation.

| Input / edge | Output |
|---|---|
| Four notes followed | Back and forward traverse the chain |
| Back then new target | Forward is cleared |
| More than 50 targets | Oldest history entries dropped |
| Second panel | Independent empty history |
| Named history entry | Jump directly, preserving intervening entries in opposite direction |
| File then note | File participates: history means where the viewer was |
| Empty stack or same target | No-op |
| Reload arrangement | Targets restored, histories empty |
| Double-click preview | Original target and original history restored |
</intent-contract>

## Code Map
- `src/lib/stores/panels.ts:565` (baseline): single target mutation funnel; `:613` opens independent panels.
- `src/components/notes/note-editor.tsx:1120` (baseline): header frame controls; now mounts NoteNavigation.
- `src/components/layout/panel-strip.tsx:486` (baseline): note editor host.
- `src/components/notes/notes-phone-pane.tsx:386` (baseline): phone editor host.
- `src-tauri/crates/keeper-core/src/palette.rs:650` (baseline): notes registry.
- `src/components/command-palette/actions.ts:141` (baseline): notes handlers.

## Tasks & Acceptance
- [x] Implement bounded panel history and direct jumps.
- [x] Render hinted controls and named history menus on desktop and phone.
- [x] Register Rust verbs and frontend handlers.
- [x] Run scoped verification: frontend 126 passed; Rust palette 48 passed.

**Acceptance:** store tests prove a follow-chain of four notes walks back and forward, that a new target truncates the forward stack, that the cap drops the oldest entry, and that two panels keep independent stacks; a component test proves the controls disable at the ends of the stack and that the menu opens a named entry directly; a palette test proves the two verbs are registered with the notes category and appear in the cheat sheet.

## Design Notes
Non-note targets participate because the gesture means where the viewer just was. Both navigation verbs accept an optional step count for menu jumps; stacks store nearest entry last. No shortcut chips are invented for unbound keys.

Required arrays buy the invariant that every panel has two valid stacks. The single makePanel factory initializes them, including restored panels; consumers never normalize an optional history. The two app-shell.test.tsx fixture literals are the sole external ripple, assigned to Main by agreement.

Double-click preview restoration restores the history snapshot as well as its target. Otherwise opening the preview beside the original would leave the original as its own Back entry. History menus resolve titles through the existing notesBodyRead API while open; missing notes retain their stable ID rather than an invented title. Desktop link callbacks focus their owning panel before retargeting, including keyboard-driven link follows.

Deleted two implementation assertions, preserving surrounding observable behavior: panels.test.ts's exact replaced object equality and panel-strip.test.tsx's native title attribute assertion (Main confirmed ownership). Neither warrants re-pinning onto the new internal structure.

The first Rust run exposed a real regression: alphabetic empty-query ranking put Back/Forward in the capped top 20 and displaced Today's Journal. Main rejected dropping that assertion. Navigation now receives score -1 only for an empty query, below every pre-existing action, while search retains ordinary fuzzy scoring. The original six notes verbs remain asserted in the default list; an additional regression asserts the exact prior 20 action IDs and proves Back/Forward remain searchable. The uncapped cheat-sheet projection includes all eight notes IDs; the tray keeps its original three.

## Verification
- `bunx vitest run src/lib/stores/panels.test.ts src/components/notes/note-editor.test.tsx src/components/notes/note-navigation.test.tsx src/components/notes/notes-phone-pane.test.tsx`: first attempt had 3 suites pass and new suite fail import of unavailable user-event; replaced it with the repo's fireEvent. Next run: **4 files, 92 tests passed**.
- Adding the preview regression and panel-strip suite exposed two stale implementation assertions above. Removed them; final command `bunx vitest run src/lib/stores/panels.test.ts src/components/notes/note-editor.test.tsx src/components/notes/note-navigation.test.tsx src/components/notes/notes-phone-pane.test.tsx src/components/layout/panel-strip.test.tsx`: **5 files, 126 tests passed**, repeated after final source edits. jsdom prints its existing HTMLMediaElement pause/load notices.
- `cargo nextest run --manifest-path src-tauri/Cargo.toml -p keeper-core -E 'test(palette)'`: rustup failed EXDEV before running tests.
- `RUSTUP_TOOLCHAIN=stable-x86_64-unknown-linux-gnu cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core palette`: first attempt timed out at 180 seconds during cold compilation; retry produced **46 passed, 1 failed** (default-list displacement). A 47-pass intermediate run after removing that assertion was rejected by Main; it is not the acceptance result. Ranking fixed and stronger default-list regression added; final rerun: **48 passed, 0 failed**. Main explicitly authorized cargo test because nextest is absent here.
- Production reachability: NoteEditor mounts NoteNavigation for the panel ID passed by PanelStrip and NotesPhoneNote; palette handlers call the same store verbs. Real Radix menu + store interaction exercised by the new component test; browser pixels and physical-phone interaction were not verified here.
- No shell-crate changes or Rust view-model type changes in this slice. Formatting and full gates belong to Main, per shared-worktree contract.

## Shipped in

PR #362 of stack #364 (epic 72), branch `epic72/surfaces`. The macOS gate (`bun run check:rust:macos`) passed on hesperia over the stack tip, which is where the `keeper` shell crate compiles at all.
