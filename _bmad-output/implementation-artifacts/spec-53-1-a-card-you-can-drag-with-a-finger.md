# Spec 53.1 — A card you can drag with a finger

story: 53.1
status: in-progress
branch: `work/epic-53-pointer-drag`
baseline_revision: 8c8a3eb
final_revision: ''
binds: FR-314; WCAG 2.1.1, 2.5.7; DW-37
sentinel: `MUT53-1`

<intent-contract>

**The ask, verbatim.** *"tasks inside tasks view are not possible to drop into the
other column and changing the property (only dropdown field works - i want drag
and drop instead) - think about that more - several times faild to finish it"*

**Why two attempts failed.** Both patched JavaScript. The defect is below it:
`tauri-runtime-wry-2.11.4/src/lib.rs:4862-4896` installs a drag-drop handler whose
closure always returns `true`, and `wry-0.55.1`'s macOS `WryWebView` implements
`NSDraggingDestination` on the WKWebView subclass itself
(`class/wry_web_view.rs:77-112`), forwarding to `super` **only** when that closure
answers `false` (`wkwebview/drag_drop.rs:88-95`). So `dragstart` and `dragover`
fire, and `performDragOperation:` is claimed in Rust before WebKit performs the
drop. The page's `drop` event cannot fire. `dragDropEnabled` defaults to `true`
and is per-window, config-time only.

**Why not simply set `dragDropEnabled: false`.** It works, and it silently breaks
Story 3.7: `conversation-pane.tsx:814-848` is the app's only
`onDragDropEvent` consumer, and drop-an-OS-file-to-attach would stop working. He
did not ask to lose it.

**Always**
- The gesture is **pointer events** — `pointerdown` + `setPointerCapture` +
  `pointermove` + `pointerup` — so nothing in the OS drag layer is involved. The
  idiom is the repo's own: `ui/resizable-columns.tsx:202`,
  `hooks/use-swipe-actions.ts:187`, `layout/phone-shell.tsx:355`,
  `layout/pins-strip.tsx:157`.
- A press-and-move on a card lifts it, a move over a column marks the column it
  would land in, and a release drops it there — writing the same `status:` and
  `order:` the dropdown writes, through the identical handler.
- The **whole column box** is a drop target: its padding, its header and the empty
  space below the last card. Today `<ul>` at `task-board.tsx:401` does not fill
  the box at `:364-366`, so most of a column is dead while the wrapper draws the
  highlight — a dead zone that looks live.
- The highlight is drawn on whatever is actually droppable, so the cue cannot lie.
- A press that does not move is a click: the card's title still opens the file, and
  a drag never fires it.
- The keyboard path is untouched. The dropdown stays, revealed on hover and
  focus-within as story 52.7 left it — WCAG 2.1.1 and 2.5.7, and DW-37.
- `HTML5` drag attributes and handlers are REMOVED, not left beside the new
  gesture. Two mechanisms for one verb is how the dead one survives unnoticed.

**Block if**
- A pointer press begins on the status control, the title button's own activation,
  or any interactive descendant that owns the gesture: those keep their behaviour.
- Rust refuses the move: the card returns to its column and the refusal is said,
  exactly as today.

**Never**
- Never set `dragDropEnabled: false` in this story; it would trade this bug for
  Story 3.7's.
- Never leave `draggable` / `onDragStart` / `onDrop` on the board once the pointer
  path lands.
- Never rely on a jsdom test as evidence of a drop again *for HTML5 DnD* — but DO
  test the pointer path, which jsdom can drive honestly.

</intent-contract>

## Code Map

| where | change |
|---|---|
| `src/components/notes/task-board.tsx:186-420` | the card's `draggable`/`onDragStart`/`onDragEnd` and both `onDragOver`/`onDrop` pairs are replaced by a pointer gesture; the drop target becomes the column box; the highlight moves onto it |
| `src/components/notes/task-board.tsx:364-366,401` | the `<ul>` fills its column so the empty space below the cards is droppable |
| a small hook beside the board, or `src/hooks/` | the press/move/release state machine, if it is worth naming — decide by whether the pins strip can share it, and say which you chose |
| `src/components/layout/pins-strip.tsx` | the same defect and the same cure, if the hook is shared; if not, say why and leave DW-37 open with the reason |
| `_bmad-output/implementation-artifacts/deferred-work.md` | DW-37 updated with what is now true on which surface |

## Tasks & Acceptance

| # | acceptance |
|---|---|
| 1 | a pointer press, a move onto another column and a release moves the card, and `sessions_task_move` is called with that column's status |
| 2 | a release over a column's EMPTY SPACE below the last card moves the card there — the case the dead `<ul>` broke |
| 3 | a release over the column HEADER lands in that column |
| 4 | a press with no movement does not move the card, and the title's click still opens the file |
| 5 | the drop cue is drawn only on a region that would accept the release |
| 6 | the dropdown still moves a card, writes the same `status:` and `order:`, and is still reachable by keyboard |
| 7 | no `draggable`, `onDragStart`, `onDrop` or `dataTransfer` remains anywhere in the board |
| 8 | a refused move returns the card and says the refusal |
| 9 | the pointer path is exercised by tests that fail if the state machine is broken — jsdom can drive `pointerdown`/`pointermove`/`pointerup`, and a mutation proves each |

## Design Notes

**The capture has to be taken back, not only taken.** The move that crosses the
slop takes the pointer capture, and on the pins strip it is the same move that
paints the reorder preview. A preview that reorders a keyed list makes React move
the pressed node; `insertBefore` on a node already in that parent REMOVES it
first, and the removing steps are exactly what Pointer Events hooks for the
*implicit release* of pointer capture. So the strip released its own capture on
the drag's first step and every later move and the release were dropped by the
`pressRef === null` guard — the third consecutive attempt to be green in jsdom and
dead on WebKit. `hooks/use-pointer-drag.ts:219-253` listens for the release
**natively on the captured element**, not through React's
`onLostPointerCapture`: React delegates at the root container, so an element
removed for good never reaches a delegated handler at all. `isConnected`
discriminates the two causes, which want opposite answers — *moved* takes the
capture back (still mounted, handlers intact one step later), *unmounted* ends the
gesture and clears the swallowed-click flag with it. The fix is in the hook and
not in the strip: the board is safe from the preview shape only by accident, since
an external `order:` edit from Obsidian, an agent or the watcher moves a pressed
card mid-drag exactly as a preview does.

**The move and the release are heard on the surface, not on the item.** Before the
crossing there is no capture, so a move is delivered only to the element under the
pointer and its ancestors. A press 3 px from the edge of a 28 px card — or 4 px
from a 44 px avatar with a 10 px tolerance — leaves the pressed element before it
has travelled the slop, and the move then lands on a box the pressed element sits
*below*: the drag silently never starts. The handlers therefore live once, on the
board's `<section>` (`notes/task-board.tsx:404-417`) and the strip's `<ul>`
(`layout/pins-strip.tsx:257-269`), which are ancestors of every item and of the
captured element after the crossing. They are not *also* on each item: a duplicate
copy would only re-run the same hit-test a second time on every move of a live
drag. The press stays on the item, because the press is what names it, and so does
the click swallow, because that click lands on the item.

**The swallowed-click flag has more than one clearing site, and needs them.** It is
cleared by the click it eats, by `begin`, by `allowNextClick` at the top of each
surface's `onPointerDown`, and by the unmount branch above. A finger's drag ends
with no synthesised click at all, and neither surface reaches `begin` on every
press — the strip returns before it for *every* phone press, the board for a press
on a card's own menu — so without the unconditional reset the tap after a
long-press reorder was eaten and the room did not open until the second.

**Tier limit: on the task board a finger lifts a card only above the phone tier.**
`notes/task-board.tsx` deliberately leaves `touch-action` alone, because claiming
it would stop a phone scrolling the board by starting on a card. Where an ancestor
can pan in the direction the finger travels the browser claims the gesture at about
the 6 px slop and the board receives `pointercancel`, which returns the card. At
phone width the four columns stack (`grid-cols-1`), so every cross-column move
travels along the page's own pan axis and the touch lift does not survive; above
that tier the columns sit side by side, cross-column travel is horizontal, nothing
pans that way, and a finger does move a card. The non-gesture path on the phone is
the per-card column menu, which stays in the DOM, in the tab order and in the
accessibility tree at all times. The pins strip has no such limit: its phone entry
is a long-press lift and its avatars are `touch-none`. The cure for the board is
the same route — gate the touch press behind `hooks/use-long-press` and pass
`captureNow`, which the hook already supports — and it is a UX decision about what
a press on a card means on a phone, so this story did not make it. Recorded in
DW-37.

**Acceptance row 7 is asserted as behaviour, not as source text.** `onDragStart`,
`onDragOver` and `onDrop` render no DOM attribute, so `[draggable]` is the only one
of the four that a rendered tree can be searched for. Rather than grep the source,
both suites drive the dead mechanism — `dragStart`, `dragOver`, `drop`, `dragEnd` —
and assert the surface does nothing: no move, no cue, no open.

## Verification

`npx tsc --noEmit` clean. `npx vitest run src/components/layout/pins-strip.test.tsx
src/components/notes/task-board.test.tsx src/components/sessions/session-board.test.tsx`
— 64 passed.

Ten mutations, each killing at least one named test (`MUT53-1`):

| mutation | test that fails |
|---|---|
| the mid-drag release does not take the capture back (the shipped behaviour) | `takes the capture back when the preview moves the pressed pin, and still lands the reorder`; `takes the capture back when a re-read moves the pressed card, and still lands the move` |
| the unmount branch leaves the swallowed-click flag set | `ends the gesture and frees the next click when the pressed pin unmounts mid-drag`; `…the pressed card…` |
| the release is heard through React's delegated prop instead of natively | all four of the above |
| the capture is taken on the move's `currentTarget` rather than the pressed element | the four above, plus both `keeps a drag whose pointer left the … before the slop` |
| the phone lift's capture is taken twice | `opens the room on the tap after a long-press reorder` |
| `begin` inherits the previous press's capture hold | `hands the capture to a second press that begins while the first is in flight` |
| the strip does not reset the flag on press | `opens the room on the tap after a long-press reorder` |
| the board does not reset the flag on press | `frees the click of the next press when the drag ended without one` |
| the strip's `<ul>` carries no move/release/cancel | 11 tests, including the headline `reorders by pointer on the desktop, and persists the new full order` |
| the board's `<section>` carries no move/release/cancel | 17 tests, including `moves the card to the column the release landed in` |
