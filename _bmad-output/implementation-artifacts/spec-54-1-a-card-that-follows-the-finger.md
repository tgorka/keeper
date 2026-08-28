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
second frame. Two landings, and the second claim below was **WRONG** — see
*Corrections after review*, defect 3. A release that lands **nowhere** (or is
cancelled) keeps the same DOM node, so the card travels back to where its file
says: correct, and still the behaviour. A release that lands in **another column**
was claimed to unmount the node from one `<ul>` and mount a new one in the other,
"which appears at zero with nothing to animate from". It does not: `onPointerUp`
runs `forget()` and then `onRelease`, whose `move` is an `await` on a Tauri round
trip, so the release commit paints the card still in its SOURCE column with the
transform removed and the transition restored — and the browser interpolated it
from the drop point BACK to its original slot, the opposite direction, until the
re-read relocated it a round trip later. The transition is now withheld from a
card whose write is in flight (`task-board.tsx:349-364`, `:493-496`). Animating
the arrival itself is still a FLIP, and still out of scope.

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
untouched and green); keyboard activation never went near a pointer; and the cost
is focus-on-mousedown, which is why the `<select>` gate returns
*before* the cancel (`task-board.tsx:328-335`, asserted) and why a secondary
button does too. On macOS a click never focused a `button` anyway.

*This inventory was INCOMPLETE, and the gap was the ancestor.* It accounted for
the card's own controls and missed `panel-strip.tsx`'s `onMouseDown` — the panel
`<section>` whose own comment states the contract that clicking anywhere in a
panel focuses it. Suppressing `mousedown` suppressed that too, and it also
suppresses the default focus action, so `onFocusCapture` could not cover: no focus
event fires either. The panel now takes focus on `pointerdown` as well
(`panel-strip.tsx:653-655`). See *Corrections after review*, defect 2.

**The document-level suppression is a class, and it is Tailwind's own.**
`DRAG_SELECTION_CLASS = "select-none"` (`use-pointer-drag.ts:91-131`) is added to
`document.body` at the slop crossing and removed by `forget`, which every exit
path runs — release, cancel, the lost-capture-for-good branch — plus `begin`, for
a press whose release was never seen, plus the unmount cleanup (`:336-346`) for the
surface that dies mid-gesture. A class, not
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
mid-gesture — and, since review, it bounds each instance to at most ONE of a
module-level reference count (`:112-131`), because `armedRef` alone did not stop an
instance that DID arm from stripping the class from under a second surface still
dragging. See *Corrections after review*, defect 5.

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
2. Release over a column: the card lands there — and it must **not** glide anywhere
   on the way. Watch the moment of release: before review, a card that landed
   successfully was animated from the drop point back to the slot it came from, and
   only then relocated by the re-read. Release over dead space outside
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
10. **The slot, not just the column** — the check this list did not have, and the
    one that would have caught the review's first defect. Drag a card to the
    **bottom of its OWN column** and confirm it lands **last**, then reopen the file
    and confirm its `order:` agrees. Do it twice: once **grabbed near the card's top
    edge**, once **grabbed near its bottom edge**. Then drag the bottom card of a
    three-card column to the **top** of that same column, grabbed near its bottom
    edge, and confirm it lands **first**. Where the press landed inside the card must
    make no difference to any of it: it was the whole answer before the fix, because
    the tally measured the dragged card's transformed box.
11. **Panel focus from a card press.** Open two panels, both showing notes with a
    task board. With panel 2 unfocused, press a card in panel 2 — press only, no
    drag — and confirm the panel-2 ring appears. Then click the card's title and
    confirm the note opens in **panel 2** and that panel 1 still shows what it
    showed. Before the fix the press fired no `mousedown`, panel 1 kept `activeId`,
    and the note replaced panel 1's document.
12. **The card is not clipped by its pane.** In session detail, drag a card toward
    the bottom and the right edges of the pane. The card must stay visible the whole
    way — no part of it may disappear behind the pane edge — and **the scrollbar
    thumb must not shrink** while the drag is live. Repeat inside a note panel,
    whose box is `overflow-hidden`, dragging toward its right edge. jsdom implements
    no overflow clipping at all, so this is measured nowhere in the suite: only the
    capped number is (`caps the follow at the board's own box`).
13. **The suppression cannot strand.** Press a card and move it past the slop, then
    without releasing press a second card with a second finger (or press, drag off
    the window, and release outside it). Then try to select a paragraph of prose:
    it must select. Then start a drag on the board and, while it is live, drag a pin
    in the pins strip; end the pin's drag first and confirm nothing turns blue for
    the rest of the board's drag.

## Corrections after review

Story 54.1's own review found two P1s, two P2s and a P3. All six changes below are
in this branch, each with a test that fails without it (mutation-proved, table
below). Two of them were **caused by this story's own fix**, which is why the
owed-checks list above grew items 10–13: the previous list checked which COLUMN a
card landed in and never which SLOT, so no step on it could have caught defect 1.

| # | defect | fix | test that fails without it |
|---|---|---|---|
| 1 | **P1.** `getBoundingClientRect` returns the TRANSFORMED box, so the midpoint tally measured the dragged card at the pointer. Its contribution reduced to `height / 2 < grabOffsetY` — constant for the gesture — and a card dragged to the bottom of its own column was written to the TOP; grabbed on its lower half and dragged up, it landed one slot too far. Only within-column reorders, which is exactly what `dropIndex`'s compensation existed for. | The tally skips the lifted card — `[data-card-key]:not([data-dragging="true"])` (`task-board.tsx:248-253`) — which makes `at` already an index into the column WITHOUT the card, so the `dropIndex` compensation is **retired** rather than kept beside it (`:406-418`) | `lands a card last when it is dragged to the bottom of its own column`, `lands a card first when it is dragged up to the top of its own column`, `lands it last however deep in the card the press began`, `sends a card dragged to the bottom of its own column to the end of it` (sessions) |
| 2 | **P1.** The press's `preventDefault()` sets PREVENT MOUSE EVENT, so no `mousedown` is dispatched — including at `panel-strip.tsx`'s panel `<section>`, whose `onMouseDown` is what makes the next single click replace THIS panel. `onFocusCapture` cannot cover: the cancel suppresses the default focus action too. Pressing a card in an unfocused panel therefore opened its note into whichever panel `activeId` still pointed at, destroying that panel's document. | The panel takes focus on `pointerdown` as well as `mousedown` (`panel-strip.tsx:653-655`). The press cancel stays — dropping it reopens the selection this story exists to fix | `focuses a panel from the pointerdown, which a cancelled press is all there is` |
| 3 | **P2.** The Design Note claiming a cross-column release "appears at zero with nothing to animate from" was false: `move` awaits a Tauri round trip, so the release commit paints the card still in its SOURCE column with the transform removed and the transition restored — the browser glided it BACKWARDS from the drop point to its original slot. Correct for the three returning cases, wrong for every successful drop, and it reads exactly like "the animation is broken" | A `landing` gate the release sets when `target !== null` and the settled write clears (`task-board.tsx:349-364`, `:406-418`, `:493-496`). A refusal is only known a round trip later, so a refused card teleports back rather than gliding — the sentence is what reports it | `holds a landed card still while keeper writes the drop, rather than gliding it back` |
| 4 | **P2.** The follow was an inline transform on an in-flow `<li>` inside two clipping ancestors (`session-detail.tsx:457` `overflow-y-auto`, `panel-strip.tsx` `overflow-hidden`). A transformed descendant is clipped by an overflow ancestor and joins its scrollable overflow, so the card vanished behind the pane edge and every downward drag grew the scroll range. `opacity-50` makes the card a stacking context, so `z-index` could not help | The delta is capped to the board's own rect (`task-board.tsx:257-325`, `:461`, `:502`), measured at the press — the offsets are card-inside-board, so a mid-gesture scroll moves both equally and leaves them unchanged. Chosen over a `position: fixed` proxy, which needs either a second node carrying this card's `data-card-key` (which the tally counts) or the real card out of flow mid-drag, collapsing the hole and re-laying the column out under the pointer | `caps the follow at the board's own box, so the pane cannot clip the card` |
| 5 | **P3.** Two strand paths for the body class. (a) `begin` recovers from a press whose release was never seen and called `detach()` without `restoreSelection()`; the replaced press's `pointerup` is dropped by the pointerId guard, so nothing else could ever release it. (b) Board and strip are two hook instances over one shared class with no reference count, so whichever gesture ended first stripped it from under the other; `armedRef` cannot see that | `restoreSelection()` beside the `detach()` at the top of `begin` (`use-pointer-drag.ts:400-401`), and a module-level count that removes the class only at zero (`use-pointer-drag.ts:112-131`) | `gives the document back when a second press replaces one whose release never came`, `keeps the document unselectable while a second surface's drag is still live` |
| 6 | **P3 (this document).** The owed-checks list named no step that could see the slot, and the Design Notes stated two things that were not true (defects 3 and 5's premises) | Items 10–13 above; the two Design Notes corrected in place rather than only recorded here — a wrong *why* in the spec is what the next reader trusts | — |

**The suite was blind by construction, and that is fixed first.** `layout()` in both
board suites assigned each card a frozen `getBoundingClientRect` closure that no
transform could move, so every slot assertion measured pre-54.1 geometry. Both
fixtures now model the browser: a card's rect is its laid-out box **moved by the
`translate()` its own inline style carries** (`task-board.test.tsx:107-114`,
`session-board.test.tsx:99-106`), and the board's own `<section>` gets a rect so the
cap is measured against the board rather than against `src/test/setup.ts`'s viewport
shim. Defect 1's mutation is caught only because of this: with the frozen fixture,
`loses the vacated slot…` passed both with and without the fix.

**Mutation results.** Each mutation was applied to the source, run, and reverted
from a byte-exact backup (`cmp` clean), with the full diff of all three touched
files read line by line afterwards to confirm no mutant survived anywhere.

| mutation | caught by |
|---|---|
| the full pre-fix slot arithmetic: tally without `:not([data-dragging])` **and** the `dropIndex` compensation restored | 3 tests (`lands a card last…`, `lands a card first…`, sessions `sends a card…`) |
| the compensation kept **beside** the fix (retire missed) | 3 tests (`lands a card last…`, `lands it last however deep…`, sessions `sends a card…`) |
| the tally measures the lifted card, compensation retired | 3 tests (`lands it last however deep…`, `lands a card first…`, sessions `sends a card…`) |
| `onPointerDown` removed from the panel `<section>` | `focuses a panel from the pointerdown…` |
| the `landing !== card.key` term dropped from the transition class | `holds a landed card still while keeper writes the drop…` |
| `followTransform` returns the raw delta | `caps the follow at the board's own box…` |
| `restoreSelection()` removed from `begin` | `gives the document back when a second press replaces one whose release never came` |
| the reference count removes the class on every release | `keeps the document unselectable while a second surface's drag is still live` |

One measured `equivalent`, reported rather than argued: `lands it last however deep
in the card the press began` survives the full pre-fix mutation. For a DOWNWARD
drag with a bottom grab the phantom self-count and the vacated-slot subtraction
cancel exactly, so the old code returned the right answer for that grab offset. It
is kept as the other half of the pair — it is the test that catches the retire being
missed (mutation 2), and it pins the reviewer's owed check 10.
