# Spec 52.7 — A card you can actually drop

story: 52.7
status: review
branch: `work/epic-52-drag-drops` (on top of `work/epic-52-dialog-scrolls`)
baseline_revision: c873fa6
final_revision: ''
binds: FR-311; WCAG 2.1.1, 2.5.7; DW-37
sentinel: `MUT52-7`

<intent-contract>

**The ask, verbatim.** *"session tasks wciaz drag and drop nie dziala i jest
brzydki dropdown w zamian"*.

**The prior triage was stale, and the new one is measured.** The board's
`shape === "flat"` guard is GONE — story 51.7 removed it, and
`session-detail.tsx:555` renders `<SessionBoard>` unconditionally for both shapes.
He IS seeing a board, and the handlers ARE on the rows he sees
(`task-board.tsx:191-207`). The drag fails for one reason: `onDragStart` at `:193`
is `() => setDragging(card.key)` — it never receives the event and never calls
`event.dataTransfer.setData(...)`. A repo-wide grep for `dataTransfer` returns
zero matches. WebKit draws the ghost and then fires no `drop`, so `onMove` is
never called. keeper ships a WKWebView. `pins-strip.tsx:283` has the identical
hole, so pin reordering is dead on macOS too.

**The dropdown is not a leftover.** `task-board.tsx:35-39` and `spec-51-7:41,51`
record it as the keyboard path — "this repo does not ship a pointer-only
affordance". Removing it re-opens DW-37 on a second surface. It is demoted, not
deleted.

**Always**
- A dragged card writes its identity to the drag data store and declares
  `effectAllowed = "move"`; the drop targets set `dropEffect = "move"`, so WebKit
  paints a move cursor rather than a no-drop badge.
- Dropping a card on another column moves it, on macOS, in the real app.
- The keyboard path keeps working, unchanged, and reaches the identical write.
- The select is revealed on hover and on `focus-within` — the
  `session-tree.tsx:421` idiom — so it is always reachable by keyboard and no
  longer four permanent boxes of chrome.
- The card's whole surface is draggable, not only the grip: the title button no
  longer swallows the gesture.

**Block if**
- Rust refuses the move: the card returns and the refusal is said, as today.

**Never**
- Never remove the dropdown, or hide it from a keyboard or assistive-tech user.
- Never rely on a jsdom drag test as evidence again: jsdom has no drag data store,
  which is exactly why `session-board.test.tsx:63-67` passed over this defect.

</intent-contract>

## Code Map

| where | change |
|---|---|
| `src/components/notes/task-board.tsx:186-278` | `onDragStart` takes the event and calls `setData` + `effectAllowed`; `onDragOver` sets `dropEffect`; the `<li>` gains `group`; the select gains the reveal classes; the title stops eating the drag |
| `src/components/layout/pins-strip.tsx:283-285` | the same three lines — the same defect, named in DW-37 |
| `src/components/sessions/session-board.test.tsx` | a test that asserts `setData` was called with the card's identity, which is the half jsdom CAN see |

## Tasks & Acceptance

| # | acceptance |
|---|---|
| 1 | `onDragStart` writes the card's key to `dataTransfer` and sets `effectAllowed`, asserted with a fake `DataTransfer` |
| 2 | `onDragOver` sets `dropEffect = "move"` on both the card and the column |
| 3 | the pins strip gets the identical treatment and its own assertion |
| 4 | the select is present in the DOM at all times and carries the focus-within reveal classes |
| 5 | dragging by the card body — not only the grip — starts a drag |
| 6 | the keyboard move still writes `status:` and `order:` exactly as before |
| 7 | DW-37's ledger entry is updated to say which surface is now correct and why the entry stays open or closes |

## Design Notes

_(filled at review)_

## Verification

_(filled at review)_
