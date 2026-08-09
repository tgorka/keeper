# Spec 44.10 — Lists That Render What You Can See

status: implemented
epic: 44 (The vocabulary is the space, and the note is a document)
binds: FR-165, AD-84
supersedes: the `@tanstack/react-virtual` window Story 37.2 put on the note list

## What this story is

Three lists — the notes list, the recordings archive, the Files tree — rendered
every row they had. AD-84 says render what is on screen. This adds one window
and uses it three times.

One window, not three. Three virtualisers is three bugs, and the interesting
bugs here are not in the arithmetic — they are in what happens to the roving
tabindex, to focus and to selection when a row that logically exists is not in
the DOM. Solving that once is the whole point.

## Where the code is

| File | What changed |
| --- | --- |
| `src/components/ui/window-list.tsx` | **New.** `useWindowedRows`, the one window. |
| `src/components/ui/window-list.test.tsx` | **New.** The window's own tests. |
| `src/test/layout.ts` | Added `withListGeometry` — the jsdom scrolling box. |
| `src/test/setup.ts` | Deleted the `offsetWidth`/`offsetHeight` prototype shim. |
| `src/components/notes/note-list.tsx` | Ported off `@tanstack/react-virtual`. |
| `src/components/recordings/recordings-pane.tsx` | Windowed; `ScrollArea` → owned viewport. |
| `src/components/recordings/recording-row.tsx` | Root `<li>` → `<div>`; the window owns the `<li>`. |
| `src/components/layout/files-pane.tsx` | Windowed; `ScrollArea` → owned viewport. |
| `package.json`, `bun.lock` | `@tanstack/react-virtual` removed. |

Row markup is untouched in all three. The window owns the scroll container, the
positioned wrapper and which indices exist; the list owns what a row looks like,
what the arrow keys mean and which row is selected.

## Decisions, and the failure each one prevents

### Hand-rolled, and the existing virtualisation dependency deleted

The house position is that this repo hand-rolls, and the dependency posture is
not a story's to renegotiate. `@tanstack/react-virtual` was already here for
Story 37.2's note list, and it was that list's only consumer. Keeping it while
hand-rolling a window beside it would have left two answers to one question —
exactly the second convention the house style forbids. It is gone from
`package.json` and the lockfile, and the `offsetWidth`/`offsetHeight` prototype
shim in `src/test/setup.ts` that existed solely to make it measurable went with
it (nothing in `src/` or in any dependency reads either property; verified by
grep before deleting, and the surfaces most likely to care were re-run green).

### Rows are measured, not fixed — because two of the three wrap

Fixed is far simpler and is a lie the moment a row wraps. Checked before
choosing:

- **Notes row** — `h-16` on the row element, every text node `truncate`d. Cannot
  wrap. Genuinely fixed at 64 px.
- **Recordings row** — `flex-wrap` tag badges (twelve tags is three lines), plus
  a monospace path line that only exists where `revealInFileManager` is off.
  Varies by content AND by platform.
- **Files tree** — treeitems are single-line, but the tree interleaves prose
  rows that are not treeitems: "this folder is empty", and the sentence Rust
  composes for a drive that is not plugged in ("/Volumes/merope/Field is not
  there. This folder lives on removable media — reattach the volume, then open
  it again."). That wraps to three lines in a narrow pane.

So: `rowHeight` is the height a row is ASSUMED to be until it has been mounted
once; the real `clientHeight` replaces it on first mount and on any resize.
Where every row is uniform — the notes list — the model collapses to exactly the
fixed-height behaviour it replaces, at no cost.

Measurements are stored **by key, not by index**. 44.4 makes a space carry a
sort, so the note list receives rows already ordered by the backend and that
order changes under it; an index-keyed store would leave the previous
occupant's height at each position after every re-sort.

### The total height is a running correction, and that is admitted

Only rows that have been mounted have been measured, so the scrollbar's claim
about how tall the list is starts as `count × estimate` and converges as the
list is scrolled. This is inherent to measuring, not a defect to fix by
measuring everything — measuring everything is the thing this hook exists not to
do. The recordings test scrolls to the bottom in a loop for exactly this reason,
which is what a person with a scrollbar actually does.

### Two rows outside the viewport stay mounted on purpose

This is the part of the story that goes wrong quietly, and each pin prevents a
specific silent failure:

- **`pinnedIndex`, the roving tab stop.** 43.8 built a tree where exactly one row
  carries `tabIndex=0`. Window it naively and scroll away from that row and the
  surface has NO row in the tab order — Tab skips the entire Files pane and
  nothing visible is wrong. Costs one node.
- **The last revealed row.** Focus lives on a DOM node. Unmount a focused row and
  focus falls to `<body>`, where the list's key handler never sees the next
  arrow press. The row stays mounted until focus moves elsewhere.

### `reveal` forces the row into the render; it does not wait a frame

The old note list did `virtualizer.scrollToIndex(index)` then
`requestAnimationFrame(() => rowRefs.current[index]?.focus())`. That is a race:
it passes on a fast machine and loses on a loaded one, and the loss is silent —
focus simply does not move. `reveal` instead sets the scroll offset, forces the
target index into the render outright, and runs the caller's `onReveal` in the
effect after that commit. The row is mounted by construction, not by timing.
(The mutation table below shows this is not theoretical: restoring the
`requestAnimationFrame` form fails eight tests, five of them 43.8's own.)

`reveal` also does not read `scrollTop` back after writing it. jsdom's setter is
a no-op and a browser silently clamps, so reading back would give two different
answers to one request; the offset is clamped here and the state set optimistically,
and a real scroll event corrects it if the browser disagrees.

### The window owns its scroll container, so `ScrollArea` is gone from two panes

The Files and Recordings panes scrolled inside Radix's `ScrollArea`. The window
needs the scroll element itself, and Radix's `Viewport` wraps its children in a
`display: table` div — an interaction with `position: absolute; width: 100%`
children that I cannot verify here, because there is no browser in this
environment. A plain `overflow-y-auto` div has no such uncertainty and is what
the notes list — the one list that was already windowed — has always used. The
visible consequence is a native scrollbar on those two panes instead of Radix's
styled one, now matching the notes list.

### `role="presentation"` on the positioning wrapper

The window needs a box to position. A box between a `tree` and its `treeitem`s
is a box with no role, so it declares that. The existing `getAllByRole("treeitem")`
queries are unaffected.

## I/O matrix — `useWindowedRows`

| Input | Output |
| --- | --- |
| `count: 0` | `rows: []`, `lastVisible: -1`, `totalSize: 0`. No spacer height, no rows. |
| `count: n`, nothing measured | `totalSize = n × (rowHeight + gap)`; window = rows under the viewport ± `overscan`. |
| A row mounts, `clientHeight > 0` and differs | Stored under the row's key; offsets and `totalSize` recomputed. |
| A row mounts, `clientHeight === 0` | Ignored. Zero is not a measurement, it is "nothing was laid out" — the estimate stands and the list behaves as fixed-height. |
| Viewport `clientHeight === 0` | Falls back to 640 px, about one screen. A freshly mounted list is never empty, and jsdom is never a zero-row window. |
| `pinnedIndex` outside the window | Appended to `rows` in index order; still exactly one extra node. |
| `pinnedIndex === anchor.index`, both outside | Deduplicated; one node, not two. |
| `pinnedIndex` out of range (`< 0`, `>= count`) | Ignored. |
| `reveal(i)` where `i` is already fully visible | No scroll. `onReveal(i)` still runs — the caller asked for focus, not for a scroll. |
| `reveal(i)` above the viewport | Scrolls so row `i`'s top is at the viewport top. |
| `reveal(i)` below the viewport | Scrolls so row `i`'s bottom is at the viewport bottom — one row, not a jump to the top. |
| `reveal(i)`, `i < 0` or `i >= count` | Returns. No scroll, no anchor, no `onReveal`, focus stays where it was. |
| `reveal(i)` twice with the same `i` | `onReveal` runs both times (the anchor carries a monotonic token, so a repeated request is not swallowed as "no state change"). |
| `getKey` identity changes (rows re-sorted or re-filtered) | Offsets rebuilt; each row keeps the height measured under its own key. |
| A mounted row is resized (pane narrows, row re-wraps) | The shared `ResizeObserver` re-measures it. One observer for the whole list, not one per row. |
| Viewport resized | Re-measured; the window widens or narrows. |
| Scroll past the end (offset > total) | Binary search clamps to the last index. |

## Edge cases

| Case | Behaviour |
| --- | --- |
| Notes: `↑` with no cursor | Moves to the LAST note — 3 999 rows past anything in the DOM. Mounts it, focuses it. |
| Notes: pagination (`onGrow`) | Driven by `lastVisible`, which excludes overscan AND the pinned rows. Using the tail of `rows` would read "the viewport reached the end" from the top whenever the tab stop sat near the bottom. |
| Files: `End` from the root | Crosses 3 000 unrendered rows, mounts the last, focuses it. `Home` returns. |
| Files: `←`/`→` toggling a folder | Unchanged — the flat row array is rebuilt and the remembered key survives, exactly as in 43.8. |
| Files: remembered row collapsed away | `activeKey` falls back to the first row, so the tree is never without a tab stop; `pinnedIndex` follows it. |
| Files: prose rows ("Reading…", absent drive) | Windowed like any other row, and still not `treeitem`s — they remain outside the arrow path. |
| Recordings: error or empty state | Replaces the list entirely; `totalSize` is 0 and no spacer renders. |
| Recordings: `gap` between cards | Folded into each row's box as bottom padding. Flex `gap` does not apply to absolutely positioned children, so a list that kept `gap-2` would have overlapped every card by 8 px. |
| Selection scrolled out and back | Untouched. Selection is the caller's state keyed by id; the window neither reads nor writes it, and never moves the scroll on its own. |

## Tests, and the mutation each one caught

Reverted, watched fail, restored. Five mutations:

| # | Mutation | Tests that failed |
| --- | --- | --- |
| 1 | **Render everything** — window becomes `[0, count-1]` | 13. All four node-count tests (window / notes / files / recordings), all four scroll-to-end tests, both focus-across-unrendered-rows tests, both tab-order tests, the selection test. The suite also went from 4 s to 43 s, which is the story in one number. |
| 2 | **Fixed height** — ignore every measurement | 1: `lets a measured row correct the estimate, and still reaches the last row`. |
| 3 | **Drop `pinnedIndex`** from the window | 4: `keeps the pinned row mounted however far away it is scrolled`, both `keeps exactly one … in the tab order` tests, and the notes scroll-to-end test. |
| 4 | **`requestAnimationFrame` instead of post-commit focus** | 8, and this is the important row: three 44.10 tests, and **five of 43.8's own pre-existing keyboard tests** — `puts exactly one row in the tab order and moves it with focus`, `steps down and up one visible row at a time`, `jumps to the first and last visible rows with Home and End`, and both expand/collapse tests. The rAF form does not merely fail the new assertions; it destroys the tree's keyboard model. |
| 5 | **Drive `scrollTop` from state on every commit** (keep the tab stop in view) | Exactly 2, and only the two that should: `keeps the open note selected across scrolling it out of view and back` and `keeps the remembered row focused after it is scrolled out and back`. This is the "returning to it does not scroll the list somewhere else" half of the AC, isolated. |

Commands run:

```
bun run test src/components/notes/note-list.test.tsx \
             src/components/layout/files-pane.test.tsx \
             src/components/ui/window-list.test.tsx \
             src/components/recordings/
→ 5 files, 77 tests, all passing
```

Also run: `bunx tsc --noEmit` (findings filtered to this story's files — clean)
and `bunx biome check` over this story's files — clean.

### `withListGeometry`, and why the tests would otherwise assert nothing

jsdom is worse than merely unlaid-out here. `clientHeight` is hard-coded zero,
`scrollTop`'s setter is a no-op that always reads back zero, and no scroll event
is ever dispatched. Without a geometry model the scroll offset can never leave
zero, so "scrolling reaches the last row" passes having never scrolled, and
"only a bounded number of rows mount" passes on a list that would mount all ten
thousand in a browser. Both would be assertions about jsdom.

`withListGeometry` answers the three properties the window reads, and only for
the two kinds of element the window marks (`data-window-viewport`,
`data-window-row`), so nothing else on screen has its geometry redefined
underneath it. It lives in `src/test/layout.ts` beside 44.12's `withTextLayout`,
because a second jsdom geometry model is the same mistake as a second window.

## What could not be proved here

Said plainly, in the spirit of this epic's own lesson.

- **No browser was available in this environment.** Everything below is
  unverified against a real engine.
- **Whether the estimates are right.** `NOTE_ROW_HEIGHT = 64` is pinned by `h-16`
  on the row and is safe. `FILES_ROW_ESTIMATE = 32` and
  `RECORDING_ROW_ESTIMATE = 60` are read off the Tailwind classes, not measured
  at the real font. A wrong estimate is self-correcting — the first mount
  replaces it — but it makes the initial scrollbar and the first frame's window
  wrong by that ratio.
- **Whether removing `ScrollArea` looks right.** The two panes now show a native
  scrollbar. That is a deliberate consistency call with the notes list, not a
  measured one.
- **Whether the browser's scroll anchoring fights the measurement pass.** When a
  measurement changes the total height, content above the viewport shifts and
  browsers may or may not compensate. With near-uniform rows the drift is small;
  it is not zero, and jsdom cannot show it.
- **Whether the `ResizeObserver` re-measure actually fires on a pane resize.**
  jsdom's `ResizeObserver` is a no-op stub, so that path is written and reasoned
  about but not exercised by any test here.
- **The 10 000-note vault on the owner's machine.** The fixtures here are 5 000,
  4 000, 3 000 and 2 000 rows in jsdom. The 100 ms first-paint budget (NFR-28) is
  not measured by any of them.

## Deliberately NOT done

- **No virtualisation library.** Not added, and the one that was already here was
  removed. See the decision above.
- **No horizontal windowing.** Every one of these lists scrolls in one axis.
- **No `scrollToOffset`, no `scrollToTop`, no imperative handle beyond `reveal`.**
  Nothing needs them. An API surface added on speculation is a contract frozen
  before anyone knows its shape.
- **No sticky headers, no grouping, no sections.** None of the three lists has
  them today, and building a grouped window for a list with no groups is
  designing for an imagined caller.
- **No keyboard model added to the recordings list.** It has none today; giving
  it one is a different story and would need its own vocabulary decision. The
  window supports it the moment someone wants it — that is what `pinnedIndex`
  and `reveal` are.
- **No gallery windowing.** AD-84 says "every list and every gallery". This story
  says lists; the gallery is 44.16's, and it will use this hook.
- **No change to what any list SHOWS.** No counts (44.11), no columns (44.12), no
  order column (44.5), no sync marks (44.17). Row markup is byte-identical apart
  from `recording-row.tsx`'s root element, which had to stop being an `<li>` so
  the window could own the positioned one.
- **No pre-measuring pass.** The total height converges as the list is scrolled
  rather than being made exact up front. Making it exact means mounting every
  row once, which is the thing this story exists to stop.
