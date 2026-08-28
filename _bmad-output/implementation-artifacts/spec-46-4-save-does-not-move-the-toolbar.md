# Spec 46.4 — A Save Does Not Move the Toolbar

status: implemented
story: Epic 46, Story 46.4
bindings: epic 46 "a save succeeds and the toolbar jumps"; the epic's header-structure paragraph
(three groups; 46.13 extracts the shared component with a second real consumer in hand)
crates: none — this story is frontend only, no Rust touched
frontend: `src/components/notes/note-editor.tsx` (header structure, save caption),
`src/components/notes/note-editor.test.tsx` (new)
compiled and tested: fully, on Linux — `bun run typecheck`, `bun run test`, `bunx biome check`
all run against the real files

---

## What the defect actually was

Not "the caption is too wide". **The caption was a width-variable member of the same
non-wrapping flex row as every control in the toolbar**, and in a flex row that is
over-constrained, one member's width is taken out of the others.

`saveStateWord` returns three strings of three different widths — `""` while dirty, `"Saving…"`,
then `"Saved · HH:MM"` — and the caption `<span>` sat directly in
`<header className="flex items-center gap-2">` upstream of `AttachFileButton`, three text
buttons, a conditional `Show in Files` and the `NoteActions` trigger. Every save cycle changed
the row's content width twice. In the 560px quick-capture window (`notes_window.rs` `CAPTURE_SIZE`)
there is no slack for the `flex-1` title to give back, so the shrink was distributed across the
shrinkable siblings, and the whole button cluster reflowed. `CaptureDocument` mounts this exact
header, which is where the owner was looking.

There is a second source of the same jump, found by ScoutNotesGaps and confirmed here:
`Show in Files` is gated on `vault !== null && path !== null && filePathForNote(...) !== null`,
and the vault list is hydrated by an effect. **Group 3's width therefore changed at least once per
note open, after first paint** — under the flat row that reflow landed on the caption too. A width
on the caption span alone would not have removed it. This is why the fix is the row's structure
and not a class on one span.

---

## (a) or (b): why a reserved slot, and not "move the caption out of the row"

The story offered two honest fixes. Main's ruling (three groups: identity `min-w-0 flex-1`;
status `shrink-0`; actions) is essentially (a) plus (b)'s wrapper, and after doing the flex
arithmetic that combination is the only one that holds in both regimes:

| regime | (b) alone: caption folded into the identity group | (a) inside a three-group row |
| --- | --- | --- |
| row fits (main pane, wide) | works — identity has `flex-basis: 0`, so its contents never reach the row's content width, and actions sit at `W − w₃` | works, same reason |
| row over-constrained (560px capture) | **fails** — identity collapses to zero width and the caption is clipped away entirely or overflows into the buttons | holds — the caption's box is a constant, so actions start at a constant offset and only *their* width absorbs the overflow |

(b) alone trades a visible jump for a caption that disappears exactly in the window where the
owner reported the bug. So: three groups, and the status group is `shrink-0` with a fixed box.

Group 3 is deliberately **not** `shrink-0` — Main's corrected ruling. If its contents ever outgrow
the window again, the row should squeeze them rather than push the last control (the `⋯`) off the
right-hand edge, which is how 46.5's defect happened. That asymmetry is safe precisely because
group 2's width is fixed: a constant offset plus a shrinkable tail cannot move when the save state
changes.

---

## The locale problem, and how it is handled

`saveStateWord` formats with `toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })`.
`"Saved · 14:32"` in `en-GB` is `"Saved · 2:32 PM"` in `en-US` and different again elsewhere. **A
`w-24` sized on this machine is a truncation bug on the owner's.**

Chosen: **reserve by measurement, not by a guessed width.** `SAVE_CAPTION_SIZERS` holds every
caption the slot can show, produced by `saveStateWord` itself, rendered inside the slot as
`invisible aria-hidden` children. The browser measures them in the font, the locale and the clock
format the machine actually has, and the widest is the slot's width. Nothing is guessed and
nothing is hard-coded.

Three sizers, not two:

- `"Saving…"`.
- `saveStateWord(… SIZER_INSTANT_MS …)` and `saveStateWord(… + 12 h …)`. Two instants twelve hours
  apart, because in every timezone one falls before noon and the other after, so a locale that
  appends a day period contributes **both** `AM` and `PM` to the measurement rather than whichever
  one this machine happened to save at. Without the second, a `PM` save in a 12-hour locale could
  be a hair wider than an `AM` reservation and would ellipsise a character.
- The digits need no entry of their own: the slot is `tabular-nums`, so `09:15` and `23:41` are the
  same width by construction. This is what makes three strings enough to cover 1,440 captions.

Deduplicated with an `indexOf` filter so a locale that renders both instants identically cannot
hand React two children under one key.

**The error caption is the one string that cannot be reserved for**, because it is Rust's message
verbatim (`markSaveFailed` ← `use-notes-body.ts:120,218`) and therefore unbounded. It is taken out
of flow instead: the visible caption is `absolute inset-0 truncate text-right`, so it contributes
nothing to the slot's width however long it is. It is ellipsised on screen, kept whole in the DOM
for a screen reader, and put on the element's `title` for a pointer. Truncation is a change from
today — but today a long error simply blew the header out, and an unbounded string in a fixed slot
is the only remaining way to move the toolbar.

---

## I/O matrix

Store state → what the header renders. `saveStateWord` is unchanged; only where its output lives
changed.

| store | `saveStateWord` | slot box | visible element | `title` |
| --- | --- | --- | --- | --- |
| `dirty: true` (`editBuffer`) | `""` | fixed, unchanged | empty, out of flow | absent |
| `saving: true` (`beginSave`) | `"Saving…"` | fixed, unchanged | `Saving…` | `Saving…` |
| `savedAtMs` set, clean (`markSaved`) | `"Saved · HH:MM"` | fixed, unchanged | `Saved · HH:MM` | same |
| `error` set (`markSaveFailed`) | the message, verbatim | fixed, unchanged | ellipsised | full message |
| fresh open, never saved | `""` | fixed, unchanged | empty | absent |

Row geometry, which is the actual deliverable:

| header child | classes | may change width? | may move its neighbours? |
| --- | --- | --- | --- |
| group 1 identity | `flex min-w-0 flex-1 items-center gap-2` | yes, constantly (title, path) | no — `flex-basis: 0`, contributes nothing to the row's content width |
| group 2 status | `relative grid shrink-0 justify-items-end …` | **no** — sized by `SAVE_CAPTION_SIZERS` | no |
| group 3 actions | `flex items-center gap-2` | yes (46.5's business) | no — it is last; it can only take from itself |

---

## Edge cases

| case | behaviour |
| --- | --- |
| 12-hour locale (`en-US`) | both `AM` and `PM` renderings are reserved for; neither ellipsises |
| 24-hour locale (`en-GB`, `de-DE`) | the two instants render as two distinct same-shape strings; the reservation is one hour-width, correct |
| a locale whose two instants render identically | deduplicated to two sizers; still correct, and no duplicate React key |
| `savedAtMs` never set | slot is fully reserved and empty — the box exists before the first save, so the first save does not grow it |
| save error of any length | cannot widen the slot (out of flow); ellipsised, full text in DOM and `title` |
| very long note title | absorbed entirely by group 1's `truncate`; caption and actions do not move |
| very narrow window (below 560, restorable via `notes.capture_placement`) | group 1 → 0, group 2 holds its box, group 3 absorbs the squeeze. Nothing moves on a save |
| `noteId === null` | early return, no header — untouched |
| `Show in Files` appearing after hydration | now inside 46.5's menu, so group 3's width no longer changes at all; the structure would have absorbed it either way |
| header mounted by `CaptureDocument` | identical element tree; one fix, both hosts |

---

## Mutation table

Every mutant carried the sentinel `MUTANT-46-4`; each was applied to a pristine copy, tested, and
restored, and the restore was verified with `cmp` against the pre-sweep bytes (not from memory).
**8 applied, 8 caught, 0 survived.** Every one of the six tests is killed by at least one mutant.

| # | mutation | caught by | how it failed |
| --- | --- | --- | --- |
| M1 | drop `shrink-0` from the caption slot | *gives the slack to identity and to nothing else* | `expect(element).toHaveClass("shrink-0")` |
| M2 | add `flex-1` to the caption slot (two growers) | *gives the slack to identity and to nothing else* | growers ≠ `[identity]` |
| M3 | delete the actions wrapper, controls back as row siblings | *puts no control in the same shrink context as the caption* | row has 8 children, 5 of them controls |
| M4 | render no sizers | *reserves the box from strings this machine's own clock produced* | reservation `[]` ≠ `SAVE_CAPTION_SIZERS` |
| M5 | reserve for one reference instant only | *reserves the box …* | `SAVE_CAPTION_SIZERS` length 2 |
| M6 | put the visible caption back in flow | *cannot be widened by a save error* | `expect(element).toHaveClass("absolute")` |
| M7 | make the slot's class list depend on `body.saving` | *keeps the same box through dirty, saving and saved* | box differs between states |
| M8 | size the slot off the title's length (group 1) | *keeps the same box while the group beside it changes width* | box differs after the title grows |

M7 was independently observed red by `W1Delete`, who caught the `pl-2` mutant live in a
concurrent run at 17:20 and confirmed the file green again at 17:21 — outside corroboration of
both the kill and the restore, which a process cannot give itself.

---

## Deliberately NOT done

- **No shared header component.** `PanelFrame` is the same construction and 46.13's Save control is
  the same variable-width status element. Main's rule of two: 46.13 extracts it with two real
  consumers in hand. Building it now, while three agents edit this file for a consumer that does
  not exist, is speculative generality with a merge conflict attached. The intent is recorded in
  the epic spine so 46.13 reproduces the decision rather than the shape.
- **No `ResizeObserver`, container query, or host-conditional render.** All three would make the
  header's width a runtime question. The whole point is that it is a static one.
- **`saveStateWord`'s wording and its three-state logic are unchanged.** Shortening the caption to
  `"Saved"` would have made the box narrower and the reservation trivial, and would have thrown
  away the one piece of information the caption carries. The sizers are derived from the function
  rather than duplicated, so a future wording change cannot desynchronise them.
- **Nothing in group 3.** Its contents are 46.5's story; this change created its wrapper and
  re-indented its children, and moved, renamed and reclassed none of them.
- **The Files pane's missing save caption.** ScoutFilesGaps is right that `useTextFile` exposes a
  `dirty` flag rendered nowhere; Main ruled that goes in `TextFileFrame`'s own chrome in 46.13.
- **Not flipping `sprint-status.yaml`.** Several agents share that file this wave; the ledger is
  Main's.

---

## What I could not verify here, and why

**I could not measure the reflow, and no test in this repository can.** jsdom performs no layout —
every element reports a zero rect — and `src/test/setup.ts`'s shim answers a viewport only for
zero-sized elements and deliberately stops at the edge of a CodeMirror editor (an unscoped shim
told CM every line was a screen tall and virtualised the document away). A test asserting "the ⋯
did not move by N pixels" would be asserting the shim, not the product. So the six tests assert the
structural property that *causes* the shift — the caption is not a width-variable participant in
the flex row that holds the buttons — and each one fails on the code as it shipped.

Specifically **not** proven by any automated check here, and needing eyes:

1. that the reserved box is not visibly too wide in the main pane (the reservation is correct by
   construction, but "correct" and "tasteful" are different claims);
2. that in a 12-hour locale the `AM`/`PM` reservation actually prevents an ellipsis — the CI locale
   decides which branch runs, and M5 is only guaranteed to fail in a 12-hour locale;
3. that the row does not overflow at 560px after 46.5's collapse — that is 46.5's gate, but it
   shares this row.

No Rust was touched, so nothing here is blocked on the macOS gate. `keeper` (the Tauri shell crate)
was neither read nor edited.

### Ordered gate checks

Run on the macOS host, in the built app.

1. **`bun run test src/components/notes/note-editor.test.tsx src/components/capture/`** → EXIT=0.
   Run three times. *(Done here: 3/3 green, 47 tests. Also `bun run test src/components/notes/`
   → 38 files, 707 tests, all green — the blast radius of a DOM restructure.)*
2. **`bun run typecheck`** → clean. *(Done here.)*
3. Open a note in the main Notes pane. Type. Watch the caption go empty → `Saving…` →
   `Saved · HH:MM`. **The `⋯` must not move by a pixel.** Sight along the right-hand edge of the
   window; the trigger's own edge is the reference.
4. Open the quick-capture window (global hotkey). **Resize it narrower than its default** — not
   just to 560, because `notes.capture_placement.<label>` restores whatever width the owner last
   left it at, and that may be a width nobody picked. Type, wait for the autosave, watch the `⋯`.
   Nothing may move, and nothing may leave the right-hand edge.
5. In the same narrow window, keep typing until the title is long enough to truncate. The title
   must be the thing that gives ground; the caption and the actions must not.
6. Provoke a failed write (make the vault read-only, or unplug the volume) and save. The message
   must appear in the caption slot, ellipsised rather than pushing the toolbar, with the full text
   on hover.
7. On a machine set to a 12-hour locale (`System Settings → General → Language & Region → 24-Hour
   Time: off`), repeat step 3 both before and after noon. The caption must never show an ellipsis.
