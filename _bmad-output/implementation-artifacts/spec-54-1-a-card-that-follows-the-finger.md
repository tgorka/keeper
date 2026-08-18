# Spec 54.1 — A card that follows the finger

story: 54.1
status: review
branch: `work/epic-54-card-follows`
baseline_revision: 9a95acf
final_revision: ''
binds: FR-323, FR-324; WCAG 2.1.1 / 2.5.7 (untouched), DW-37
sentinel: `MUT54-1`

<intent-contract>

**The ask, verbatim.** *"drop dziala teraz - lubie przerywane linie zeby widiec gdzie
drop zrobic, ale drag ma regresje - nie ma animacji ani animacji przesuwania oraz
czesto inne czesci aplikacji sa 'zaznaczane' przypadkowo"*

**What the drag has today, in full.** `cursor-grab` — static, never changes
(`task-board.tsx:325`). `data-[dragging=true]:opacity-50` on the pressed card
(`:325`, attribute at `:291`). The column's dashed border (`:443-445`). That is the
entire inventory: no transform, no translate, no ghost, no placeholder gap, no
transition, no cursor change. The card **does not move**. Only `drag.over` tracks
the pointer, and it feeds the column border alone.

**Why the selection leaks.** A native HTML5 drag never selects text: once
`dragstart` fires the browser is running a drag session and the selection machinery
is not. `draggable="true"` was therefore doing double duty — the drag image AND the
selection suppression — and story 53.1 removed both together, replacing the first
with nothing and the second with a `select-none` on the pressed `<li>` only
(`task-board.tsx:325`, intent stated at `:319-321`). Nothing suppresses selection on
the document, and **no `preventDefault()` is issued on any pointerdown or mousedown
in the drag path**, so a captured pointermove with the button held anchors a
selection at the nearest selectable position and extends it across everything the
pointer crosses.

**Always**
- The dragged card **follows the pointer**, 1:1, for the whole gesture. The idiom is
  this repo's own: `chat-row.tsx:461` `transform: translateX(${dx}px)` with
  `:459` withholding the transition **while** the gesture is live.
- It **settles** when it lands: a transition on release, so the card does not
  teleport into its new column.
- No text anywhere is selected by a drag. The press prevents its own default, the way
  `resizable-columns.tsx:202-203` already does, and the suppression covers the
  document for the gesture's duration rather than one element.
- The dashed column cue he named is **kept exactly as it is** — same element, same
  class, same condition (`task-board.tsx:443-445`). It marks a COLUMN; that is what
  he said he likes, and a slot-level line would be new work, not a restoration.
- `useReducedMotion()` cuts the **landing** transition. It does NOT cut the live
  follow: direct manipulation is not animation, which is exactly the distinction
  `chat-row.tsx:459` encodes.
- The pins strip gets the same treatment or a written reason why not — its desktop
  selection leak is identical (`pins-strip.tsx:344` suppresses selection on the
  phone only).

**Block if**
- The gesture is cancelled (`pointercancel`) or refused by Rust: the card returns to
  where its file says, and the return is the settle transition, not a jump.

**Never**
- Never restore `draggable`, `onDragStart`, `onDrop` or `dataTransfer` to buy the
  drag image back: `drop` cannot fire under Tauri on macOS, and
  `task-board.test.tsx:399` forbids it on purpose.
- Never suppress selection permanently on the document — only for the gesture, and
  released on every exit path including `pointercancel` and an unmount mid-drag.
- Never animate the live follow, and never leave the landing transition on under
  reduced motion.

</intent-contract>

## Code Map

| where | change |
|---|---|
| `src/hooks/use-pointer-drag.ts` | the press prevents its own default; the hook exposes the live pointer delta so a caller can translate; document-level selection suppression armed at the slop crossing and released on every exit |
| `src/components/notes/task-board.tsx:280-330` | the card translates by the delta, settles on release, and keeps `select-none`; `cursor-grab` gains its active state |
| `src/components/notes/task-board.tsx:443-445` | untouched — his cue |
| `src/components/layout/pins-strip.tsx` | the same follow and the same suppression, or a recorded reason |
| `src/hooks/use-reduced-motion.ts` | consumed, not changed |

## Tasks & Acceptance

| # | acceptance |
|---|---|
| 1 | while a drag is live the card's inline transform equals the pointer delta, asserted on the arithmetic the component computed |
| 2 | the live follow carries no transition, and the release does — and under reduced motion the release carries none either |
| 3 | the press prevents its default, so no selection is anchored |
| 4 | selection is suppressed for the gesture's duration and restored on release, on cancel, and when the pressed card unmounts mid-drag |
| 5 | the column's dashed cue still appears on exactly the column a release would be accepted by — the existing test stays green untouched |
| 6 | a press that does not move still opens the file, and the card's transform is never set |
| 7 | the pins strip either follows and suppresses too, or the spec's Design Notes name the reason it does not |
| 8 | no `draggable`/`dataTransfer` returns — the existing guard stays green |
| 9 | what jsdom cannot see is named in Verification: a transform reaching the screen, and a selection painted by WebKit. The click-through on hesperia is owed and written down |

## Design Notes

**The delta is state, and it is opt-in.** `usePointerDrag` gained
`delta: {x, y}` (`use-pointer-drag.ts:189-197`), set from `event.client* -
press.start*` on every move past the slop and returned to a shared frozen zero by
`begin` and by `forget`. It is published only when the caller passes
`trackDelta` (`:141-152`), because it costs a render per `pointermove`: the board
already re-rendered on every move (`dropAt` returns a fresh object, so `setOver`
never bails), but the pins strip's target is a slot *number* and React bails out
of every move that does not cross a slot. Publishing unconditionally would have
turned a strip drag into sixty renders a second for a surface with nothing to do
with them.

**The settle is the transition coming back, not a timer and not a FLIP.** The
pressed card carries `transition-transform duration-200 ease-out` at all times
*except* while it is the lifted card (`task-board.tsx:368-375`), which is
`chat-row.tsx:459` exactly. So one render does both halves of the landing: the
drag ends, the inline transform is removed, and the transition is back in the
after-change style — the browser therefore interpolates from the last
`matrix(…)` to `none` rather than teleporting. No settling flag, no timeout, no
second frame. Two landings, both correct: a release that lands **nowhere** (or is
cancelled, or is refused by Rust) keeps the same DOM node, so the card travels
back to where its file says; a release that lands in **another column** unmounts
the node from one `<ul>` and mounts a new one in the other, which appears at zero
with nothing to animate from. Animating that second case is a FLIP — measuring
the new rect and inverting — and the epic put it out of scope.

**The transform is on the lifted card only.** `style` is `undefined` for every
other card (`task-board.tsx:377-382`), so a board of twenty cards does not grow
twenty containing blocks and twenty compositor candidates because one of them is
moving. It also makes acceptance 6 observable: a press under the slop sets no
transform at all.

**The cursor rides `data-dragging`, not `:active`.** `data-[dragging=true]:
cursor-grabbing` sits beside the `opacity-50` that already keys off the same
attribute (`task-board.tsx:369`). `:active` would light on a plain click, which
is not a live gesture; the attribute is set at the slop crossing, which is exactly
when the gesture becomes one.

**Reduced motion cuts the landing and never the follow.** `useReducedMotion()`
(`task-board.tsx:256-258`) is ANDed into the transition class only. A card that
stopped tracking the pointer under the preference would be a card that ignores the
hand holding it: direct manipulation is not animation, which is the distinction
`chat-row.tsx:459` already encodes.

**The press cancels its own default, at the callsite.** Both desktop entries call
`preventDefault()` on the `pointerdown` after their gates and before `begin`
(`task-board.tsx:345`, `pins-strip.tsx:328`) — `resizable-columns.tsx:202-203`'s
shape, which is why a seam drag never leaked a selection. It is not inside the
hook because the two entries do not share an event: the pins strip's phone lift
reaches `begin` from a `use-long-press` detail, with no cancellable event left.
The hook's doc block (`use-pointer-drag.ts:53-75`) states the requirement so a
third caller reads it.

*What the cancel costs, checked rather than assumed.* Pointer Events (mapping for
devices that support hover, steps 5–6) says a cancelled `pointerdown` sets the
PREVENT MOUSE EVENT flag, so `mousedown`/`mousemove`/`mouseup` are not fired —
and the spec's own worked sequence for a cancelled press still ends in `click`.
So: the title button still opens the file (proved by the existing
`is one handle` and `keeps a press that does not move a click` tests, both
untouched and green); keyboard activation never went near a pointer; and the one
real cost is focus-on-mousedown, which is why the `<select>` gate returns
*before* the cancel (`task-board.tsx:328-335`, asserted) and why a secondary
button does too. On macOS a click never focused a `button` anyway.

**The document-level suppression is a class, and it is Tailwind's own.**
`DRAG_SELECTION_CLASS = "select-none"` (`use-pointer-drag.ts:91-104`) is added to
`document.body` at the slop crossing and removed by `forget`, which every exit
path runs — release, cancel, the lost-capture-for-good branch — plus the unmount
cleanup (`:307-317`) for the surface that dies mid-gesture. A class, not
`body.style.userSelect`: `classList.remove` of an absent class is a no-op, so
every exit path can run unconditionally, where an inline write would have to
remember and restore a previous value. Tailwind's `select-none` and not a new
`@utility`: it compiles to `-webkit-user-select: none; user-select: none` (probed
against the repo's own Tailwind), it is already in the emitted sheet because the
card and the pin both use it, and a second name for one declaration would be a
second convention. Adding it imperatively from TS is
`conversation-pane.tsx:1240-1247`'s idiom; naming the constant is
`csv-table.ts:50`'s. An `armedRef` guard means the hook only ever releases a
suppression it armed itself, so the other surface's unmount cannot strip it
mid-gesture.

**Why the pins strip gets the suppression and not the follow** (acceptance 7).
Its drag preview is a real DOM reorder (`pins-strip.tsx:254-266`) — the pressed
avatar is *already* carried into its target slot, and that reorder is what the
file itself calls the desktop's drop cue. A translate by the delta from the press
origin would be added on top of a displacement the DOM already applied: the pin
would draw a slot ahead of the finger, and further ahead the further it travelled.
Following from there means re-measuring the moved element every step, which is a
FLIP. So the strip takes the other half of this story — the press cancel and the
shared document suppression, which is what the desktop strip was actually missing
(its only suppression was the phone-gated `select-none`, baseline `:344`, now
`:371`) — and the decision is recorded
both in the code (`:238-253`) and by a test that asserts the pressed pin moves by
reorder and never by a transform.

**Untouched, deliberately.** The dashed COLUMN cue (`task-board.tsx:494-502`) and
its test are byte-for-byte what they were; the stray row's permanent
`border-dashed` still carries no `data-board-column`, so the test helper still
does not see it; the dropdown, the keyboard move and the
`draggable`/`dataTransfer` guard are all unchanged and green.

## Verification

**Commands.** `npx tsc --noEmit` — clean (the one error in the tree,
`text-file-viewer.test.tsx(88,3)`, is story 54.2's file mid-flight, not this
story's). `npx vitest run src/components/notes/task-board.test.tsx
src/components/sessions/session-board.test.tsx
src/components/layout/pins-strip.test.tsx src/components/chat/chat-row.test.tsx`
— **140 passed**, up from 125 on the baseline: 4 files, 0 failed.
`npx biome check --write` on the six touched files — clean.

**The fifteen new tests, and the mutation that proves each.** Every mutation was
applied to the source, run, and reverted with a hash check against the
pre-mutation file.

| test | mutation | result |
|---|---|---|
| `translates the pressed card by the pointer's delta from where it pressed` | transform → `undefined` | fails (also fails the reduced-motion and session-board tests) |
| " | delta measured from `event.clientX` instead of `− press.startX` | fails |
| " | `lifted` drops its `dragging` term | fails (also fails `keeps a press that does not move a click`) |
| `withholds the settle transition while the card follows, and restores it at the release` | drop `!lifted(card) &&` from the transition class | fails |
| " | delete `data-[dragging=true]:cursor-grabbing` | fails |
| `cuts the landing transition under reduced motion, and never the live follow` | drop `!reducedMotion &&` | fails |
| `keeps a press that does not move a click…` (extended) | `lifted` drops its `dragging` term | fails |
| `cancels the press's own default, so no selection is anchored` | delete `event.preventDefault()` in the board | fails |
| `leaves the column menu's press and a secondary press to the platform` | move the cancel above the strip's phone gate (analogue) | fails |
| `holds the document unselectable from the slop crossing to the release` (board **and** strip) | delete `suppressSelection()` | both fail |
| " | arm at `begin` instead of the slop crossing | both fail |
| `gives the document back when the gesture is cancelled` | drop `restoreSelection()` from `forget` | fails |
| `gives the document back when the pressed card unmounts mid-drag` | drop `restoreSelection()` from `forget` | fails |
| `gives the document back when the whole board unmounts mid-drag` | drop `restoreSelection()` from the unmount cleanup — `forget` alone does **not** cover it | fails |
| `carries the follow and the selection suppression through to the sessions surface` | any of the transform, transition or cancel mutations | fails |
| `cancels the desktop press's own default, so a strip drag anchors no selection` | delete `e.preventDefault()` in the strip | fails |
| `leaves a secondary press to the platform, which is what opens the menu` | cancel moved above the strip's gate | fails |
| `leaves the phone's own press to the platform, and still suppresses its drag` | cancel moved above the gate; and `suppressSelection()` deleted | fails on both |
| `moves the pressed pin by reordering the strip, and never by a transform` | add a `translateX` to the lifted pin | fails |

The board's own 19 pre-existing tests, including the dashed-cue test
(`task-board.test.tsx:231` after the additions, helper unchanged) and the
HTML5-forbidding guard, are green with no edits. The one pre-existing test that
was edited is `keeps a press that does not move a click`, which gained two
assertions on its own subject (no transform before or after a sub-slop press).

**What jsdom proved, and what it cannot.** It computed the arithmetic: the inline
`transform` string the component derived from coordinates the test handed it
(`translate(260px, 205px)` from a press at 40,45 to a move at 300,250), the class
that decides whether that string is animated into, the `data-dragging` attribute
both the opacity and the cursor key off, `defaultPrevented` on the press, and the
class on `document.body` across all four exit paths. It cannot see any of this:

- **A transform reaching the screen.** No layout, no compositor. jsdom never
  paints, so "the card is under the cursor" is unobserved here.
- **A selection painted by WebKit.** jsdom has no selection engine worth the name,
  and — importantly — it generates **no compatibility mouse events at all**, so it
  cannot demonstrate that cancelling `pointerdown` suppresses the `mousedown` that
  anchors a selection. That link is the Pointer Events spec's, quoted in the hook,
  not this suite's.
- **The `click` surviving the cancel.** Same reason: the suite dispatches `click`
  itself, so it proves the handler runs, not that WebKit still fires it.
- **The cursor glyph, the 200 ms easing, and the reduced-motion preference** as the
  OS reports it.

**Owed on hesperia — the click-through.** In the installed build, on the sessions
board and on a note's board widget:

1. Press a card and move slowly across two columns. The card must stay **under the
   cursor** the whole way with no lag, and the cursor must read as a closed hand.
   The dashed border must follow the column under the pointer.
2. Release over a column: the card lands there. Release over dead space outside
   every column: the card must **glide back** to its slot (~200 ms), not snap.
3. During a drag, sweep the pointer across the session tree and a pane of prose.
   **Nothing anywhere may turn blue.** Repeat with the press starting on the card's
   own title text — the case he reported.
4. Release, then select a paragraph of text somewhere. It must select: the
   suppression was armed for the gesture, not for the session.
5. Click a card title without moving: the file opens. This is the one the press
   cancel could have broken.
6. Press a card's column menu: the dropdown opens and takes focus. Then Tab to it
   and change a column by keyboard.
7. Start a drag and Cmd-Tab away mid-gesture; come back and select text. Still
   selectable (the cancel path released it).
8. System Settings ▸ Accessibility ▸ Display ▸ Reduce motion **on**: repeat 1 and
   2. The follow must still be 1:1; the return in 2 must be instant, not eased.
9. Pins strip, desktop: drag an avatar across the strip. The strip previews the
   reorder, nothing anywhere is selected, and the click afterwards still opens the
   room.
