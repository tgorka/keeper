---
title: 'A hint you can read, on every row and every icon'
status: done
baseline_revision: a8eb9c2
final_revision: 'c162a1a12a86'
---

<intent-contract>

## Intent

**Problem:** Truncated note names have no hover preview, icon-only controls have inconsistent or absent pointer names, and the Files full-value path is click-only.

**Approach:** Add HoverHint and IconHint with a chosen 500ms hover delay and immediate keyboard focus disclosure; apply them to note/file names and the owned icon controls without replacing existing information routes.

**Always:** Full label; at most three visual lines of plain-text detail; max-w-xs; use existing accessible names; preserve row activation, roving focus, full-value popover and folded avatar rails.

**Block If:** A fix requires another slice's file without agreement, or removes a keyboard/touch route.

**Never:** Native title as the new hint, a frontend markdown renderer, interactive hint content, a tooltip on a folded avatar rail, or hints as the sole information route.

## I/O & Edge-Case Matrix

| Input/state | Output/behavior | Proof |
| --- | --- | --- |
| Mouse enters a row/control | Absent at 499ms; visible at 500ms | Fake-timer wrapper test |
| Keyboard focus reaches trigger | Full hint opens immediately without pointer input | `act(() => button.focus())`, following the repo's existing tests |
| Long title and multiline detail | Complete wrapping title, three-line bounded plain detail | Wrapper and note-row tests; actual layout measurement owed by coordinator |
| Empty detail | Label only | Wrapper test |
| Files name overflows | Hint plus existing click-to-open full value button | Files test |
| Icon-only control added | Guard names file:line if unwrapped/unexempted | Explicit swept-files guard |
| Folded avatar | Accessible name remains; no hint | Explicit 45.20 exemption |

</intent-contract>

## Code Map

- `src/components/ui/tooltip.tsx:6-49`: existing immediate provider and Radix primitives.
- `src/components/notes/note-row.tsx:216-379`: focusable row and truncated name/snippet.
- `src/components/layout/files-pane.tsx:1081-1098`: RowName preserves FullValueButton.
- `src/components/layout/files-pane.tsx:2936-2951,3190-3245`: promoted row/header icons.
- `src/components/layout/priority-actions.tsx:291-325`: shared promoted icons.
- `src/components/layout/sidebar-pane.tsx:322-340,470-510`: fold and folded view controls.
- `src/components/layout/spaces-group.tsx:70-99`: folded avatar carve-out.
- `src/components/layout/surface-row-floor.test.ts:37-73`: mechanical guard precedent.
- `_bmad-output/implementation-artifacts/spec-44-12-columns-you-can-size-content-you-can-read.md:24-26`: title is not a keyboard/touch/full-value route.
- `_bmad-output/implementation-artifacts/spec-45-20-chrome-that-makes-room.md:62`: folded avatar hint competes with long press.

## Tasks & Acceptance

- [x] Publish shared wrappers (first code change, to unblock sibling consumers).
- [x] Add row previews and sweep owned icons.
- [x] Preserve Files full-value path and folded-avatar exemption.
- [x] Add delay/focus/row/full-value proof and explicit-list guard.
- [x] Record verification and visual decisions.

**Acceptance:** a test proves the hint carries the full name of a truncated row and at most three lines of detail; a test proves it opens on keyboard focus and after the chosen delay (fake timers), not instantly; a mechanical guard fails on an icon-only control in the swept files that has neither `IconHint` nor an explicit exemption comment; a files-pane test proves the `FullValueButton` still exists (44.12 is answered, not replaced).

## Design Notes

- **Provider scope:** preserve `TooltipProvider`'s existing zero-delay default. Existing consumers in `account-footer.tsx`, `sidebar-group.tsx` and `surface-column.tsx` are out-of-slice disclosures with established behavior; the sidebar controls owned here were migrated. Each new hint owns a local provider with `skipDelayDuration={0}` and a root with `delayDuration={HOVER_HINT_DELAY_MS}`. Consequently the root provider's instant setting, and another hint having just opened, cannot skip this hint's 500ms pointer delay. Radix's focus opening remains immediate.
- **Hierarchy and placement:** full, wrapping label first, optional plain-text detail below at most three visual lines; `max-w-xs`, foreground/background tokens, no interactive children in content. Wide-pane rows/icons default to `top`; left-rail callers explicitly use `right`. A long unbroken label uses `overflow-wrap:anywhere`, not ellipsis. Empty detail adds no line.
- **Note rows:** attach the hint to the existing focusable row via its `ContextMenuTrigger`; do not add a span tab stop. The unread row still shows provenance in its second visible line, while the hint shows the body snippet verbatim. Opening the note is still the touch/full-content route. No markdown parsing was added.
- **Files:** `RowName` adds a hint around its name-bearing child while the existing sibling `FullValueButton` is untouched. The extended overflow test opens the delayed hint, then clicks the original full-value affordance and reads the whole name. This answers 44.12's keyboard/touch objection rather than replacing its solution.
- **45.20 carve-out:** expanded chat Space rows receive their full-name hint, but folded avatar rows remain unhinted with an explicit `icon-hint-exempt` comment. Accessible names and selection gestures remain intact. The folded view-navigation glyphs and fold control are not avatars and receive `IconHint`.
- **Shared actions:** promoted actions now use the same name for the hint and `aria-label`. At TasksHeader's request `PriorityAction.disabled?: boolean` also reaches the promoted button; a behavioral test proves a pending action cannot run, then runs once enabled. Overflow-menu disabled state remains the caller's responsibility.
- **Sweep scope:** `note-list.tsx`, `notes-pane.tsx`, `pane-header.tsx`, and `fold-strip.tsx` were inspected but need no direct edit: they contain no unhinted direct icon-only controls (shared promoted controls inherit `PriorityActions`). The guard lists ten files explicitly rather than walking the tree; it recognizes icon-sized controls, folded-rail controls, sole glyph children and the sync glyph trigger, and reports `file:line`.
- **Approved ownership additions:** Main granted `src/components/layout/sync-pane.tsx` (the assigned `src/components/sync/sync-*.tsx` path has no files) and `priority-actions.test.tsx`. Only the Sync delivery-detail glyph changed; the copy card and `copyStart` are untouched. No other outside-owned file was edited.
- **Old assertion removal:** deleted only the two native-`title` assertions in `files-pane.test.tsx` (baseline lines 2500 and 3613); the existing accessible-name and icon-content checks remain. No wording was re-pinned onto a tooltip. `priority-actions.test.tsx` had no such assertions—its `title` occurrences were fixture controls, not expectations.
- **Keyboard test tooling:** initial `user-event` imports failed because this repo does not depend on it. Main explicitly directed use of the existing `fireEvent`/`.focus()` style instead of adding a dependency. The final tests use actual DOM focus inside `act`, not a simulated pointer, and no package or lockfile changed.
- Shared exports preceded the spec only as explicitly requested to unblock sibling compilation. No formatter, repo-wide gate, or git operation was run by this slice.

## Verification

### Passing final commands

- `bunx vitest run src/components/ui/tooltip.test.tsx src/components/notes/note-row.test.tsx src/components/layout/files-pane.test.tsx src/components/layout/icon-hints.test.ts` — **4 files passed; 172 tests passed** (tooltip 3, note-row 18, files-pane 150, guard 1).
- `bunx vitest run src/components/layout/priority-actions.test.tsx src/components/layout/sidebar-pane.test.tsx src/components/layout/sync-pane.test.tsx src/components/layout/spaces-group.test.tsx src/components/layout/fold-strip.test.tsx src/components/notes/notes-pane.test.tsx src/components/notes/note-list.test.tsx` — **7 files passed; 295 tests passed**.
- `bunx vitest run src/components/layout/icon-hints.test.ts` — **1 file passed; 1 test passed**, after strengthening the recognizer to catch a new anonymous `<button><Plus /></button>` and a folded-rail control, with an exemption fixture.

### Regression sensitivity and intermediate results

- First execution of the required four-file command: two suites could not resolve `@testing-library/user-event`; Files and guard passed. Replaced those imports per Main's direction, without changing dependencies.
- `bunx vitest run src/components/ui/tooltip.test.tsx src/components/notes/note-row.test.tsx src/components/layout/files-pane.test.tsx src/components/layout/icon-hints.test.ts src/components/layout/priority-actions.test.tsx src/components/layout/sidebar-pane.test.tsx src/components/layout/sync-pane.test.tsx src/components/layout/spaces-group.test.tsx` — initially **345 passed, 1 failed**. The test left the preceding hoverable Radix tooltip open in its pointer grace region; fixed the test interaction by dismissing it with Escape before testing the second delayed hint.
- `bunx vitest run src/components/ui/tooltip.test.tsx src/components/notes/note-row.test.tsx src/components/layout/icon-hints.test.ts` — baseline **22 passed**.
- The same three-file command with the hint root's delay temporarily set to zero and `line-clamp-3` temporarily removed — **3 failed, 19 passed**, as intended: the hint incorrectly existed at 499ms; both wrapper and real note-row tests rejected the unclamped detail. Both mutations were restored; the tooltip source snapshot returned exactly to `ED1C`. The passing final commands above ran after restoration.

### Evidence boundary / coordinator probe

No Rust files changed. No browser was launched and no visual rendering was performed by this slice. jsdom proves focus/hover timing, escaping, classes, context-menu preservation, full-value popover operation and action behavior, not pixel geometry.

Main owns the epic-wide `dev/probe` pass. It must show: a pointer hint remains absent until 500ms; keyboard focus opens it; a truncated row's complete label wraps without ellipsis, detail occupies at most three rendered lines, and the hint does not overflow the viewport/pane; the Files full-value control remains operable; folded avatar rails remain unhinted. Formatting and repo-wide gates are also the coordinator's work.

## Shipped in

PR #362 of stack #364 (epic 72), branch `epic72/surfaces`. The macOS gate (`bun run check:rust:macos`) passed on hesperia over the stack tip, which is where the `keeper` shell crate compiles at all.
