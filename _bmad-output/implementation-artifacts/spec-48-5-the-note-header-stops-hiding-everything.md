# Spec 48.5 — The note header stops hiding everything

story: 48.5
status: implemented; the pixels are gate checks
branch: `work/epic-48` (off `chore/the-epic-46-tail`)
binds: AD-104, UX-DR77/78 (revises 46.5's *scope*, not its taxonomy); FR-195
sentinel: `MUT48-5`

## What the defect actually was

46.5's arithmetic was right and its conclusion was too wide. Six controls plus
two truncating spans do not fit the 560px quick-capture window, the row does not
wrap, and `NoteActions` was the group's last child — so the verb nobody could
afford to lose was the first one off the screen. The repair was to put every
verb in a `⋯` menu and keep one control beside it.

That is the correct shape at 560px. **The same header is also mounted at
1400px**, where it hides six verbs behind a menu to save room it has in
abundance. Three separate 0.8.1 field reports are one cause:

| Report | The feature | Where it was |
|---|---|---|
| "I still see no way to delete notes" | shipped in 45.17 | menu item, last |
| "I don't see attachments" | shipped in 45.13/46.2 | menu item, first |
| tags on a recording note "missing" | shipped in 44.14/45.x | menu item → **and** a panel that renders only in `mode === "edit"` |

A menu that holds everything is a menu nobody opens. The third report has a
second half that the menu does not explain: `showProperties && mode === "edit"`,
so a person who pressed Properties while reading history got **nothing at all** —
no panel, no sentence, no hint that the mode was the reason.

## The change

**Priority-ordered overflow: show what fits, menu what does not.** One mechanism
at every width — no media query, no hard-coded breakpoint, no host-conditional
render, and nothing anywhere that asks "is this the capture window".

1. `PaneHeader` gains a **render-prop form** for `actions`. A caller that passes
   a function is handed the pixels the row can spare for group 3, measured with
   a `ResizeObserver` on the `<header>` itself.
2. `PriorityActions` (new) renders as many candidates as that budget buys, in a
   declared priority order, and hands the rest to a menu the caller renders.
3. The note editor declares four candidates and keeps two things out of the
   arithmetic entirely: `AttachFileButton` (always a control) and everything in
   the menu that never promotes.
4. Both mode-gated panels now **explain** their absence instead of vanishing.

### Priority order, and the argument for it

`attachments → properties → history → show-in-files`.

- **Attachments** and **Properties** first because they are literally two of the
  three things the owner reported as missing. Main's prior was the same and I
  did not find a reason to overturn it.
- **History** and **Show in Files** after, because both are journeys *away from*
  the note rather than facts *about* it.
- **Show in Files** last of the four because it is the only one with a second
  home: the Files pane reaches the same file without this header at all. 45.18's
  "burying a one-press navigation in a dropdown is a regression" is honoured at
  every width the window can afford, which is more than 46.5 gave it.
- **Delete is never promoted, at any width.** A destructive verb does not belong
  in a toolbar — 46.5's ruling, unchanged. It is findable now because the menu
  is the *short* list rather than the list of everything.
- **Export and "Open in a capture window" never promote either**, for a
  different and duller reason: they are components with their own flows, not a
  label and a handler, so there is no button form of them to promote. Recorded
  under "not done" rather than pretended away.

### AD-104's three groups are intact

Identity still absorbs slack (`min-w-0 flex-1` off a zero basis), status is
still a reserved measured box (`shrink-0`), and the overflow arithmetic concerns
group 3's **contents** only. One class did change and it is a rule with a
reason, not a second convention:

> Group 3 is squeezable when `actions` is a node (46.4's ruling: squeezing beats
> pushing the last control off the edge) and `shrink-0` when `actions` is a
> function. A group that drops its own last item needs neither, and a control
> squeezed until its word is clipped is worse than one fewer control.

The Files pane's Save bar and `PanelFrame` pass nodes and are byte-for-byte
unaffected.

### The identity floor is a reservation, not a breakpoint

`PANE_HEADER_IDENTITY_MIN_PX = 160`. Group 1's basis is zero, so left to the
flexbox it would surrender every pixel a control asked for and the pane would
end up saying what can be done to a note without saying which note. Nothing
asks "is the window narrower than N"; it asks "once the title has this much,
what is left". That is the one declared number in the mechanism, and it is
declared where the group it protects lives.

`PANE_HEADER_GAP_PX = 8` is `gap-2` as a number, because a Tailwind class cannot
be read from JavaScript and the arithmetic has to spend it. `PriorityActions`
lays its row out with `style={{ gap }}` from the same number, so the plan and
the layout cannot come to disagree; `PaneHeader`'s own `gap-2` and the constant
are one fact written twice and the doc says so.

### Widths are measured, never declared

A table of per-item widths is a guess about a font, a locale and a text-size
setting, wrong on the first machine that disagrees. The first commit renders
every candidate as a control, reads what the browser gave each one in a **layout
effect** (after the DOM is mutated, before paint — a measurement, not a
flicker), and re-plans. Each width is recorded **once**: a group that re-measured
what it had just re-laid out is the shape that oscillates, and a toolbar that
flickers at one particular window width is a worse defect than the one being
fixed. An item that appears later (`Show in Files`, when the vault list lands)
has no width yet, so the group takes one more measuring pass and re-plans —
which is why the map is keyed by id and not by index.

## I/O matrix

### `planPriorityActions({ available, reserved, widths, gap })` → promoted count

| available | reserved | widths | gap | → | why |
|---|---|---|---|---|---|
| 640 | 188 | [120,110,90,100] | 8 | 4 | every cost paid exactly |
| 4000 | 188 | same | 8 | 4 | cannot promote what is not there |
| 532 / 531 | 188 | same | 8 | 3 / 2 | the third item's boundary |
| 434 / 433 | 188 | same | 8 | 2 / 1 | the second item's boundary |
| 316 / 315 | 188 | same | 8 | 1 / 0 | **the first item's boundary** |
| 188 | 188 | same | 8 | 0 | nothing but the menu |
| 0 | 188 | same | 8 | 0 | nothing but the menu |
| −400 | 188 | same | 8 | 0 | never negative-promotes |
| 200 / 216 | 0 | [100,100] | 8 | 1 / 2 | the gap is charged per item |
| 500 | 0 | [100,**900**,20] | 8 | 1 | prefix: the 20 is NOT skipped ahead to |
| 4000 | 0 | [100,**NaN**,20] | 8 | 1 | unmeasured stops the walk |
| NaN | 0 | [10] | 8 | 0 | the row was never observed |
| 0 | 0 | [0,0,0,0] | 8 | 0 | a world with no layout at all |

### `paneHeaderActionsBudget({ header, status })` → px for group 3

| header | status | → | why |
|---|---|---|---|
| 1400 | 70 | 1154 | −160 identity −70 status −2×8 seams |
| 1400 | null | 1232 | two groups, therefore one seam |
| 1000 | 0 | 824 | the slot is in the DOM, so its seam is too |
| 100 | 70 | 0 | never negative |
| NaN | 70 | 0 | not yet observed |

### The note header (`note-editor.tsx`, group 3)

Widths below are the ones `note-editor.test.tsx` **declares**; the real ones are
a font's business. With them the group owes 464px before the first candidate.

| Row width | Controls | Menu (opened) |
|---|---|---|
| 1400 | Attach, Attachments, Properties, History, Show in Files, Actions | [capture], ─, Export…, ─, **Delete note** |
| 800 | Attach, Attachments, Properties, History, Actions | Show in Files, [capture], ─, Export…, ─, **Delete** |
| 700 | Attach, Attachments, Properties, Actions | History, Show in Files, … |
| 600 | Attach, Attachments, Actions | Properties, History, Show in Files, … |
| 572 / 571 | Attach, Attachments, Actions / Attach, Actions | the boundary |
| 500, 0, never observed | Attach, Actions | everything — **exactly 46.5's shape** |

`[capture]` is 48.3's `CaptureNoteItem`, present only when `capabilities.notes`.

### The mode-gated panels

| `mode` | Properties pressed | Attachments pressed |
|---|---|---|
| `edit` | the panel | the panel |
| `history` | sentence + **Back to the note** | sentence + **Back to the note** |
| `conflict` | sentence, **no** way out offered | sentence, **no** way out offered |

## Edge cases

- **Explained rather than rendered read-only.** `PropertiesPanel` takes a null
  `subscriptionId` to mean "editing disabled", and what that actually does
  (`properties-panel.tsx:397`) is drop the write on the floor: the fields stay
  enabled, the chips stay removable, and nothing says why the edit did not take.
  Between a panel that lies about being editable and a sentence that says when
  it will be, the sentence is the honest one. I do not own that file this wave
  and would not have changed the answer if I did.
- **No way out of a conflict.** `leaveMode` abandons the resolution as well as
  returning to the note. A control called "Back to the note" that quietly threw
  away a merge in progress would be far worse than the silence it replaces, so
  the button renders in history only; the resolver draws its own two exits, both
  named for what they do.
- **Attachments got the same treatment as Properties, and that is one line of
  scope I took deliberately.** The story names Properties. Attachments is the
  *first* promoted control and was reported missing in its own right; shipping a
  fix that makes it easy to find and leaves it silently doing nothing in two of
  three modes would be the same report again with a shorter path to it. Same
  mechanism, same sentence, one extra call site.
- **The menu is the row's overflow in the row's order, then the verbs that never
  leave.** One rule, so nothing has to reason about where an item lands when the
  list above it changes length. This moved 48.3's capture item from "between
  History and Show in Files" to "last among the things that show the note".
  E48Capture was asked and agreed; their ordering test reads
  `History < capture < Export < Delete` and holds at every width, including
  widths where History is not in the menu at all. Their comment in
  `note-editor.tsx` was amended to say where the item is now and why.
- **`inMenu(id)` answers `true` for an id it has never heard of.** That is the
  right answer for Export, Delete and the capture item, and it means the caller
  can ask about anything without a guard. `Show in Files` still needs its own
  predicate, because "not offered" and "in the menu" are different claims.
- **The measuring pass renders every candidate for one commit.** In a browser
  that commit never reaches the screen (layout effect precedes paint). In jsdom
  it is flushed inside `act`, so no suite can observe it. It is real, it is
  named, and mutation **M7** proves it load-bearing.
- **A machine that never delivers an observation degrades to 46.5.** Budget zero
  → nothing promoted → one control and a menu. This is not hypothetical: it is
  exactly what every other suite in this repository sees, because
  `src/test/setup.ts`'s `ResizeObserver` exists only so Radix's popper can mount
  and never delivers anything. Two tests assert that degradation on purpose.
- **`row.children` is still three, and still none of them a `BUTTON`.** 46.4's
  two structural tests were not touched and still pass; the whole mechanism
  lives inside group 3.

## Mutation table

Baseline established with the **same command and the same filter** as the sweep:
`bun run test src/components/layout/priority-actions.test.tsx src/components/notes/note-editor.test.tsx`
→ **2 files, 36 tests, 0 failed** before the first mutant.

Every mutant was applied with the editor, run, restored, and the restore
verified by `md5sum -c` against a baseline recorded before the sweep — for all
six files including the **two new ones `git diff` cannot see**. A repo-wide grep
for `MUT48-5` after the sweep returns nothing.

| # | Mutation | Caught by (named) | Result |
|---|---|---|---|
| M1 | `cost = width + gap` → `cost = width` | `moves the first item at exactly one width…`, `moves each later item…`, `charges the gap once per promoted item`, `promotes nothing in a world with no layout at all`, `moves the first control into the menu at exactly one width`, `moves the first verb into the menu at exactly one width` | ✅ 6 failed |
| M2 | prefix `break` → `continue` (pack by width) | `stops at the first item that does not fit rather than packing by width` + 4 boundary tests | ✅ 5 failed |
| M3 | budget drops `identityMin` | all four `paneHeaderActionsBudget` tests + `degrades one control at a time`, `degrades one verb at a time`, `keeps the menu's own order…`, `gives back what it promoted…`, both boundary tests | ✅ 10 failed |
| M4 | `observer.observe(row)` removed — header never observed | `shows every verb as a word when the row is wide`, `degrades…` ×2, both boundary tests, `keeps the menu's own order…`, `gives a promoted control and its menu item the same handler`, `gives back what it promoted…` | ✅ 8 failed |
| M5 | `inMenu` always `true` (menu keeps promoted items too) | `renders no verb twice at any width` ×2, `shows every verb as a word…`, `keeps the menu's own order…`, `gives back what it promoted…` | ✅ 5 failed |
| M6 | Properties back to `showProperties && mode === "edit"` | `explains itself in history mode, and comes back with one press` | ✅ 1 failed |
| M6b | Attachments back to `showAttachments && mode === "edit"` | `says the same thing for the other panel the header opens` | ✅ 1 failed |
| M7 | `measuring` forced `false` — no measuring pass | 8 tests across both files | ✅ 8 failed |

**7 mutants (8 rows), 8 caught, 0 survived.**

Two mutations were considered and deliberately **not** run:

- *Promote Delete.* There is no code path to mutate: Delete is not a candidate
  and is rendered by `NoteActions`. Making it one would be editing the diff into
  existence rather than testing the code. The invariant is asserted directly
  instead — `keeps Delete in the menu, and never in the row, at every width`,
  over six widths including 0.
- *Drop the write-once guard on the width map.* The failure it prevents is an
  oscillation across frames, and jsdom's `act` flushes to a fixed point; a test
  that "caught" it would be asserting the scheduler.

## What I could not verify here, and why

**jsdom performs no layout.** Every element reports a zero rect;
`src/test/setup.ts`'s shim answers one 1024×768 viewport for the zero-sized ones
and deliberately stops at CodeMirror's edge, because an unscoped shim once told
CodeMirror every line was a screen tall and it virtualised documents away. **No
test in this repository can measure a reflow**, and one that claimed to would be
asserting the shim.

So the policy is a pure function of numbers, proved to the pixel without a DOM,
and the component tests *arrange* a geometry rather than measure one:
`withActionWidths` declares a width for exactly the elements this mechanism
measures, and `withHandFiredResize` delivers the observation the shimmed
`ResizeObserver` never does. Both were appended to `src/test/layout.ts`, which is
this repository's declared home for "the crudest possible layout engine, for the
assertions jsdom cannot carry".

**The numbers in those tests are mine and not a browser's.** That is the one
thing a human on the Mac has to check, and it is the first gate below. In
particular I did **not** verify the claim that the 560px window shows Attach +
Attachments + Actions — with my declared widths it does, and with the real font
it may show one control fewer or one more. The mechanism is what is being
asserted; the specific width at which each verb moves is a fact about a font.

`bunx tsc --noEmit` is clean for all six files. The `keeper` shell crate was
never built: it does not link on Linux, and I touched no Rust at all.

### Gate checks, in order, none of which I performed

1. **Open a note in the main window at a wide size.** The header reads
   `<title> · <path> … <caption> | Attach a file | Attachments | Properties |
   History | Show in Files | Actions`. Words, not a lone `⋯`.
2. **Drag the window narrower, slowly.** Controls leave one at a time, from the
   right, in that order, and each one that leaves appears in the `Actions` menu
   in the same position it left from. Nothing flickers, nothing changes places,
   and the title never disappears entirely — it truncates and keeps ~160px.
3. **Drag it wider again.** They come back in the reverse order. Same widths, no
   hysteresis band.
4. **Quick capture (⌘⌥N) at the default 560px.** At least `Attach a file` and
   `Actions` are fully on screen with nothing clipped at the right edge — this
   is 46.5's original defect and it must still be fixed. Note whether
   `Attachments` also fits; if it does not, the row is still correct, and if it
   is clipped rather than absent, the mechanism is wrong and I want to know.
5. **Resize the capture window down to `CAPTURE_MIN_SIZE`.** The row degrades to
   `Attach | Actions` and never clips. 46.15's persisted placement means the
   owner may be running a width nobody chose.
6. **Delete.** At every width above, `Actions` → `Delete note` is present, red,
   last, below a rule, and behind its confirmation.
7. **Properties in history mode** — the reported half. Open a note, press
   `Properties`, then `History`. A sentence appears where the panel was, naming
   Properties and saying you are reading an older version, with **Back to the
   note** beside it. Press it: history closes and the panel is there. Repeat with
   `Attachments`.
8. **Properties in conflict mode.** Force a conflict, press `Properties`: the
   sentence appears and there is **no** Back button — only the resolver's own
   two exits.
9. **Keyboard and screen reader** (no AT on this box): tab reaches each promoted
   control in row order; `Actions` announces `Actions for <title>`; the menu's
   arrow-key roving still works, including 48.3's capture item, which is still
   an unwrapped `DropdownMenuItem`.

## Gate results on this box

- `bun run test src/components/notes/ src/components/layout/ src/components/capture/`
  — **71 files, 1496 tests, EXIT=0, three consecutive runs.**
- Recorded because a clean number that hid something would be worthless: I ran
  **six**. Runs 1, 2, 4, 5 and 6 were EXIT=0. Run 3 had exactly one red —
  `files-pane.test.tsx > FilesPane keyboard navigation > steps down and up one
  visible row at a time`, `expected 'Vault' to be 'Field'`. It is not mine and
  not a mutant: that file carries 307 changed lines from story 48.1 this wave,
  it passes 93/93 in isolation three runs running, and my change was live in the
  five green runs around it. `PaneHeader`'s node form — which is what the Files
  pane uses — produces byte-identical DOM to before this story, and the failing
  assertion is about a virtualised row window, not a header. Flagged to 48.1's
  owner rather than filed; the log is at `/tmp/gate3.log`.
- `bun run test src/components/layout/priority-actions.test.tsx src/components/notes/note-editor.test.tsx`
  — 2 files, 36 tests, EXIT=0. This is the sweep filter, and its pre-sweep green
  is the baseline every row of the mutation table is measured against.
- `bunx tsc --noEmit` — no error in any of the six files.
- `bunx biome check` on the six files — clean (formatter run on those files only;
  the repository-wide formatter and linter are Main's, once, at the end).
- No Rust was touched, so nothing needed the `keeper` shell crate, which does not
  link on this Linux box.

## Files changed

- `src/components/layout/priority-actions.tsx` — **new.** `planPriorityActions`
  (pure) and `PriorityActions` (measure + render).
- `src/components/layout/priority-actions.test.tsx` — **new.** 22 tests: the
  policy without a DOM, then the mechanism at seven widths.
- `src/components/layout/pane-header.tsx` — `PANE_HEADER_GAP_PX`,
  `PANE_HEADER_IDENTITY_MIN_PX`, `paneHeaderActionsBudget` (pure), the
  render-prop form of `actions`, the `ResizeObserver`, and the conditional
  `shrink-0` on group 3. Existing callers untouched.
- `src/components/notes/note-editor.tsx` — the four candidates and their order;
  the menu rendered through the predicate; `panelUnavailableReason` and
  `PanelUnavailable`; both mode-gated panels now explain themselves. 48.3's
  capture line is preserved verbatim, its comment amended to state its new
  position.
- `src/components/notes/note-editor.test.tsx` — +8 tests in two describes.
- `src/test/layout.ts` — appended `withActionWidths` and `withHandFiredResize`.
- `src/components/notes/note-actions.tsx` — **not changed.** It was in my
  ownership and needed nothing: Delete is already last, already behind a
  separator, already behind a confirmation, and the trigger is already a word.


## Deferred work

Two entries for Main to land; the highest number in `deferred-work.md` at the
time of writing is **DW-201**, so these want the next two free.

**DW-a — `AttachFileButton` is outside the overflow arithmetic.**
origin: story 48.5. location: `note-editor.tsx`'s `leading`, and
`attach-file-button.tsx`. reason: it is a fixed leading control, so below its
own cost the row can still overflow by the amount it exceeds the budget — the
one width band where 46.5's defect could recur, now bounded by a single control
instead of five. It cannot be a candidate as things stand: it is a dropdown with
two dialogs, not a label and a handler, and `PriorityActions` promotes labels.
fix: give it a menu form (its two existing `DropdownMenuItem`s, rendered into the
`⋯` menu when it does not fit) and add it to the candidate list as the lowest
priority. That is `attach-file-button.tsx`'s decision and it is not this story's
file. status: open.

**DW-b — `PanelFrame` and the Files pane's Save bar still pass action nodes.**
origin: story 48.5. location: `pane-header.tsx`'s two other consumers. reason:
both have action clusters that can outgrow a narrow pane the same way this one
did, and both are still on 46.4's squeeze-rather-than-push behaviour. Neither
has produced a field report, and AD-104's rule of two says a shared mechanism
waits for the second real consumer rather than guessing at it. fix: pass a
function instead of a node and declare a priority order; the mechanism needs no
change to accept them. status: open.
