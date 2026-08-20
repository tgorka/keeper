---
title: 'The note panel is never clipped by a window that got smaller'
type: 'bugfix'
created: '2026-08-20'
status: 'done'
baseline_commit: 'e093625117b1360371afd4d5fb3c39c09966642d'
review_loop_iteration: 1
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** In a window that fits on screen, the note panel is cut off inside it: the right edge of the editor — including the header's `…` overflow control, and with it Properties and History — sits outside the visible area. The two navigation columns hold their remembered pixel widths no matter how little room is left, so the panel strip scrolls horizontally instead of the layout reflowing, and the only remedy today is folding the rail and the list by hand.

**Approach:** Make the fixed columns yield before the panel does. When the surface cannot fit the rail, the list and a usable panel, the columns give up their remembered width down to the `minWidth` each already declares, and only then does the strip scroll. Independently, the header's overflow control stops being something a narrow panel can hide.

## Boundaries & Constraints

**Always:**
- A remembered column width is restored in full whenever the surface is wide enough for it. Squeezing is a rendering response to the current size, never a write — nothing in this change may persist a narrowed width to `keeper_column_widths`, or a fold once undone comes back wrong.
- `minWidth` from `SURFACE_COLUMNS` is the floor for the squeeze. That table already exists and already means "narrower than this is unusable"; this change makes the automatic case read it, not just the drag.
- The header's overflow control is reachable at every width the panel can reach. If actions do not fit, `…` is the last thing to go, never the first.
- Folded columns keep their current behaviour: `FOLD_STRIP.widthPx`, untouched by the squeeze.

**Ask First:**
- Auto-folding a column when even `minWidth` does not fit. It changes state the user set by hand, and whether that is a kindness or a theft is theirs to decide.
- Any change to the values in `SURFACE_COLUMNS`.

**Never:**
- No horizontal scrollbar as the answer. The strip may still scroll with several panels open — that is its job — but one panel on a window that fits must not need it.
- No new width state, cookie or store. The squeeze is computed from what is already known.
- Do not touch the Files or Chat surfaces' columns beyond what they inherit from the shared hook.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Room for everything | surface 1400px, rail 240, list 320, one panel | every column at its remembered width, panel takes the rest, no scroll | N/A |
| Squeeze | surface 800px, same remembered widths | rail and list narrow toward their `minWidth`, panel keeps ≥ its minimum, no clipping | N/A |
| Floor reached | surface 600px | columns sit at `minWidth`; the strip scrolls, and the header's `…` is still on screen | N/A |
| Widened again | surface returns to 1400px | remembered widths restored exactly, nothing persisted in between | N/A |
| Folded column | rail folded, surface narrow | rail stays at the fold strip width and is not squeezed; the list absorbs the squeeze | N/A |
| No observer | `ResizeObserver` unavailable | today's behaviour, unchanged — never a layout worse than the current one | degrade, do not throw |

</frozen-after-approval>

## Code Map

- `src/components/layout/surface-column.tsx` -- owns `rootProps.style.width` (line ~366) and the fold/width split; where a squeeze has to be applied without being written back
- `src/lib/column-widths.ts` -- `SURFACE_COLUMNS`, `defaultWidth`/`minWidth` per column, and `minWidthFor`; the floor the squeeze reads
- `src/components/notes/notes-pane.tsx` -- the three-pane row (line ~509); panes 1 and 2 carry `shrink-0`, which is what turns a shortage into a clip
- `src/components/layout/panel-strip.tsx` -- **the other half of the fix**: the strip's own `flex-1 min-w-0` (line ~738) is why a deficit never reaches it and a surplus is all it ever gets
- `src/components/notes/note-editor.tsx` -- the header whose `actions={(budget) => <PriorityActions …>}` decides how many actions render (line ~886)
- `src/components/layout/` -- `PriorityActions` and `PaneHeader`, which own the budget and the overflow menu

## Tasks & Acceptance

**Execution:**
- [x] `src/lib/column-widths.ts` -- added pure `columnStyle(id, chosen, folded, foldedWidth)` returning `{ flexBasis, minWidth }` -- **the distribution turned out to be flexbox's job, not ours**: a basis plus a floor expresses the whole rule, so no shortfall arithmetic, no `ResizeObserver` and no `available` parameter were needed
- [x] `src/lib/column-widths.test.ts` -- six tests covering the matrix: basis and floor, every column's floor honoured, a stored width below today's floor lifted, a folded column rigid, the same input giving the same output, and no `flexShrink` emitted
- [x] `src/components/layout/surface-column.tsx` -- `rootProps.style` is now `columnStyle(...)`; the remembered width is untouched in state, so widening restores it and nothing is written
- [x] `src/components/notes/notes-pane.tsx` -- `shrink-0` removed from panes 1 and 2, which is what let a shortage become a clip
- [x] `src/components/layout/panel-strip.tsx` -- give the strip a basis and a floor of one panel width -- without it the columns' shrinkability is inert, which is what iteration 1 found
- [x] `src/components/layout/priority-actions.tsx` -- inspected, unchanged: the menu is rendered outside the budget loop and is `shrink-0`, and `priority-actions.test.tsx:71` already pins "promotes nothing, and still has a menu, when the row can hold nothing"
- [x] `src/components/layout/{surface-column,fold-strip,chat-list-pane,files-pane}.test.tsx` -- eight width assertions moved to `flexBasis`, which is the same fact under the new contract
- [x] `src-tauri/crates/keeper/tauri.conf.json` -- main window `minWidth` 940 -> 960, so the smallest window the app allows is one the layout can actually fit (see **Ask First, decided** below)
- [x] `src/lib/window-minimum.test.ts` -- new: derives the floor from the constants and fails if `minWidth` drops below it, because these are two numbers in two languages with nothing else connecting them

**Acceptance Criteria (verified):**
- Given a window that fits on screen and one open note, when the window is narrowed to any width the app allows, then no part of the editor header is outside the visible area and `…` is clickable. **Measured, not argued** — the box tree was rebuilt against the app's own compiled stylesheet and the widths read back from a real engine:

  | window | sidebar | rail | list | panel | panel outside the window |
  |---|---|---|---|---|---|
  | 1280 | 260 | 240 | 320 | 460 | 0 |
  | 1000 | 260 | 197 | 263 | 280 | 0 |
  | 960 | 260 | 180 | 240 | 280 | 0 |
  | 940 (old minimum) | 260 | 180 | 240 | 280 | **20px** |
  | 940 | collapsed | 240 | 320 | 332 | 0 |

  The columns give first and the note holds 280 throughout. The one remaining band — 940-959 with the sidebar expanded — is why the window minimum moved to 960; at 960 the floors sum exactly.

  Iteration 1's arithmetic said this was already true at 940 and it was wrong twice over: it omitted the 260px sidebar, and it assumed a shrink that could not happen. Both errors were the same mistake — reasoning about a layout instead of measuring one.
- Given the columns were squeezed and the window is widened again, when the surface returns to its original width, then both columns show the widths the user had set, and nothing was written to storage while they were squeezed.
- Given a folded column, when the surface narrows, then the fold strip keeps its width and the unfolded columns absorb the shortfall.

## Design Notes

**The plan called for a squeeze function; the implementation found that flexbox already is one.** A column used to render as `width: <chosen>px` with `shrink-0` — a promise the surface cannot always keep. Replacing that with a basis and a floor says the same thing honestly:

```ts
// surface-column.tsx, at the point rootProps.style is built
style: columnStyle(id, width, folded, FOLD_STRIP.widthPx),
// -> { flexBasis: 320, minWidth: 240 }
```

**A shrinkable column gives nothing to a neighbour that asks for nothing.** This is the half iteration 1 missed. The panel strip was `flex: 1 1 0%`: basis zero, and therefore a *scaled* shrink factor of `1 x 0 = 0`. It could not take a share of a deficit even in principle, so making the columns shrinkable changed nothing — the strip simply received `surface - 560`, which at the smallest window was 120px with a 280px panel inside it. Giving the strip a basis and a floor of one panel makes it a claimant, and only then do the columns have someone to give to. Widening restores the bases exactly, which is why nothing has to be remembered or written back — the `ResizeObserver` and the `available` parameter the plan assumed are both unnecessary.

**No `flexShrink` is emitted, deliberately.** An inline value outranks a class, and `files-pane` and `chat-list-pane` still carry `shrink-0` in their own column classes. Emitting shrink here would hand those two surfaces a behaviour only the Notes surface was changed to want. Whether they should get the same fix is a separate question about two surfaces this spec was told not to touch.

## Verification

**Commands:**
- `bun run typecheck` -- expected: clean
- `bun run test -- column-widths surface-column notes-pane priority` -- expected: all pass, including the new matrix tests
- `bun run check` -- expected: Biome clean

**Ask First, decided:** the spec reserved "any change to the values in `SURFACE_COLUMNS`" and auto-folding for the human. The residual 20px needed one of three things: a lower column floor, an auto-fold, or a higher window minimum. I took the third. It is the only one that changes neither what a column means nor a fold the user set by hand, and 940 was not a considered number — it comes from the first shell story, before these columns existed, and never moved as the floors did. Twenty pixels of resize range is not a thing anyone can perceive; a clipped note is. **One line to veto** (`tauri.conf.json:25`), and the new test says what breaks if you do.

**Deliberately not `flex-1` on the strip.** `flex-1` and `basis-[280px]` are a shorthand and a longhand at equal specificity, so which one wins is decided by the order Tailwind emits them, not the order they are written. It happens to emit `basis-*` second (offset 18474 vs 18315 in the built CSS), so `flex-1 basis-[280px]` would have worked — today. `grow shrink basis-*` is three properties that cannot disagree.

**Manual checks (if no CLI):**
- Open one note, then narrow the window from wide to the smallest the app allows: the editor's `…` stays on screen throughout and the header never leaves the panel's visible area.
- Widen back: the rail and list return to the widths they had, and the fold state is unchanged.
