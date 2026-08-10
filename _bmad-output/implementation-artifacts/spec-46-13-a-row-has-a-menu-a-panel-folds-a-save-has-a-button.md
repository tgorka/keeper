# Spec 46.13 — A file row has a menu, a panel can fold, and a save has a button

status: implemented
story: Epic 46, Story 46.13
bindings: FR-215 (the row's three "open"s), FR-216 (a Save control for a file), FR-217 (folding a
panel), AD-104 (one header shape, extracted on the second consumer), UX-DR77
crates: none — frontend only, no Rust read and no Rust edited
frontend: `src/lib/stores/panels.ts`, `src/components/layout/panel-strip.tsx`,
`src/components/layout/files-pane.tsx` (the ROW only), `src/components/viewers/text-file-frame.tsx`,
NEW `src/components/layout/pane-header.tsx`, and `src/components/notes/note-editor.tsx`'s header
element (migrated onto the extracted component)
tests: `src/lib/stores/panels.test.ts` (+13), `src/components/layout/panel-strip.test.tsx` (+4),
`src/components/layout/files-pane.test.tsx` (+8), NEW `src/components/layout/pane-header.test.tsx`
(8), NEW `src/components/viewers/text-file-frame.test.tsx` (10)
compiled and tested: fully, on Linux. `bun run test src/components/layout/ src/components/viewers/
src/lib/stores/ src/components/notes/note-editor.test.tsx` → **75 files, 1268 tests, EXIT=0, three
consecutive runs**; `bun run typecheck` clean on every file this story touches.

---

## Three reports, one surface

The owner filed three items about the space between the Files tree and the document. They are one
story because they are one surface and because two of the three are the same defect wearing
different clothes: **keeper could already do the thing, and had no way to say so.**

- **A.** Right-clicking a file did nothing. The two panel verbs existed as gestures nobody could
  discover, and the one verb with a label was called `Open` — which is the word all three deserve.
- **B.** A panel could be closed and not put away. Closing is destructive and the last panel refuses
  it, so "I want this out of the way for a minute" had no answer at all.
- **C.** Saving a file was `Mod-s` and nothing else, with no autosave (deliberately — `spec-45-6`),
  so the only feedback that an edit was still in the buffer was that nothing had happened, which is
  indistinguishable from a save that worked. `useTextFile` had exposed a `dirty` flag to nobody
  since 45.6.

---

## Report A — the three "open"s, named

`FILES_OPEN_HERE_LABEL` = "Open in this panel" → `setActiveTarget`, the single click's verb.
`FILES_OPEN_BESIDE_LABEL` = "Open in a new panel" → `openPanel`, the double click's verb.
`FILES_OPEN_LABEL` = "Open in the default app" → `syncOpenEntry`, the verb that leaves keeper.

**Wording is the deliverable; the menu is how a name becomes reachable.** A test asserts the three
are distinct AND that none is another plus a suffix, which is what "worded apart" actually means: a
mutant that renames the third back to `Open` is caught by that test and not by any assertion about
behaviour.

"Panel", not "tab". The owner's report said tab; the product has never had one. The thing beside the
tree is a panel, the store is `panels`, and inventing a second word would teach a vocabulary the
rest of keeper does not use.

**The pattern is copied verbatim, not invented.** One Radix `ContextMenu` whose trigger is the row
itself (`asChild`, so the DOM the tree and the virtualiser see is unchanged), plus `useLongPress`
for the phone tier — the fifth instance of the same construction as `chat-row`,
`favorites-section`, `networks-group` and `pins-strip`. The long-press dispatches the synthetic
`contextmenu` the Radix trigger already listens for, so there is one menu, not two.

**A folder and a profile root get no menu.** All three items are ways to open a *file*; a folder is
not a panel target (its gesture is expand/collapse), so its menu would hold none of them, and an
empty menu offered on a right-click is worse than the native one it suppressed. The predicate is
`rowTarget(node)` — one function, used both to decide the menu and to derive the row's click target,
so the two gestures cannot come to be about different things.

**The row's own button says `Open` and answers to `Open in the default app`.** The row already
carries three text buttons and a name that truncates before they do; spelling the whole verb across
it would cost the file name about a hundred pixels on every row in the tree. The accessible name
*contains* the visible label, which is the condition that makes this safe rather than a trap for
voice control (WCAG 2.5.3), and the full verb is also the `title`.

## Report B — folding, and the ruling about `v: 1`

`Panel` gains `folded: boolean` — the first display state this model has held. It is in the store
and not in `PanelStrip`'s `useState` for the reason 46.3 moved the Files tree's expansion out of
component state: `AppShell` unmounts the strip's host on every primary-view switch, so component
state would be a lens the reader rebuilt by hand each time they looked at Notes.

**A folded panel renders no body.** Not `hidden`: a body kept mounted keeps its listing, its
subscription and its editor buffer — exactly the cost the reader was reclaiming — and for a note
panel it would hold a document mirror open over a note nobody can see. W3Notes's
`two-notes-at-once.test.tsx` pins the lifecycle a fold produces (one unmount releases exactly one
subscription), so nobody can "optimise" this into a hidden div without a red.

**The fold is allowed on the last panel, where closing is refused**, and the asymmetry is the point:
closing the last panel is unrecoverable, where the control that unfolds is sitting exactly where the
panel was.

**A panel that is GIVEN something to show unfolds.** One function, `shown()`, applied at every site
that sets a target — `setActiveTarget`, `openPanel`'s retarget-and-focus branches, the pin branch,
the fill-an-empty-panel branch. Without it, clicking a file in the tree would load it into a panel
the reader cannot see, which is this epic's defect shape verbatim. Focus alone does NOT unfold:
clicking a folded panel's header is not a request to see anything.

### The `v: 1` ruling

`PersistedPanels` goes to `v: 2` with `f`, an array of indices into `t`. Indices, not a boolean per
panel: folding is the rare state, and a parallel boolean array would spend a fifth of the 3500-byte
budget saying "false" four times.

**A `v: 1` cookie is read, and restores with nothing folded.** This is a deliberate exception to the
module's own discard rule, and the reasoning is why it is written down rather than inherited:

| | why the rule exists | does it apply to `f`? |
| --- | --- | --- |
| discard an unknown `v` | a future version may change what a `t` entry *means*, and a panel pointing at something that no longer means what it meant is worse than no panel | **no** — `v: 2` only ADDS a field whose absence has one exact, safe reading: nothing is folded, which is both the state `v: 1` shipped with and the state every panel is reachable from |
| cost of applying it anyway | — | every existing reader loses their whole workspace on the first launch after an update, in exchange for nothing |

So `PANELS_READABLE_VERSIONS = [1, 2]`, and `f` is read **only** from a `v: 2` payload — a `v: 1`
payload carrying an `f` is a cookie somebody edited, and reading it would make the version number
decorative. Both halves are mutation-proven (B3, B4).

The price is paid in the other direction and it is the price the discard rule always charged: a
build older than 46.13 reading a `v: 2` cookie discards it and comes up clean. A downgrade costs one
arrangement, once — which is why the writer bumps rather than smuggling `f` into a `v: 1` payload. A
cookie that lies about its version to stay compatible is a cookie no future reader can trust.

**Budget rules follow `files-tree.ts`'s precedent** (wave 1): bounded on the way in as well as out,
dropped from the end that can be spared, never truncated silently, and one shared `encode` so the
first attempt and every trim cannot disagree about what `f` counts. `panelsCookie` narrows to a
`HoldingPanel` (target non-null) *before* encoding, so the encoder has nothing to decide about an
empty panel — see the mutation table's B6 entry for why that narrowing replaced a guard.

**On "fold panels other than the first":** there is no per-panel fold in the product today, so that
phrasing describes `sidebar-fold.ts`, which is restricted to two groups. Read as the general request
it plainly is — *let me put an open thing away without closing it* — and answered with a fold on
every panel. Said out loud here because the literal reading would have produced a second fold
mechanism for the sidebar and left the panels as they were.

## Report C — the Save button, in the frame and not the header

Main's ruling, not relitigated: `TextFileFrame`'s own chrome. `dirty` and `save` live in the hook
mounted *below* `PanelFrame`'s header, so a header button needs a registry of per-panel save
functions kept in step with mounts and unmounts — a worse thing to own than a button in the right
place. It also means a note embed of a file gets the control for free, where a panel header would
have given it nothing.

**The bar exists exactly when a Save could land**: a writable *format* (`entry.writable`) that was
not truncated on the way in (`!vm.oversize`). A header whose only reason to exist is a control that
is not there is chrome for its own sake. The LOCATION's verdict is deliberately not consulted — 45.2's
rule is that a location refusal arrives as a refused save carrying Rust's own sentence, in the
banner, and guessing it here would mean the frontend deciding which volumes are writable. (Confirmed
with W3Outside: after 46.14 a file inside a profile but outside the vault becomes `writable: true`,
which changes whether pressing the button succeeds and not whether it appears. Their `caveat`
paragraph renders in the banner region, above `error`, and is untouched by this story.)

**Disabled, with a sentence, rather than absent** — the opposite of this pane's usual idiom, and the
difference is that "nothing has changed" is a state the reader leaves by typing, where "keeper will
not write this format" is not. A control that vanished whenever the buffer matched the disk is a
control nobody could find on purpose. `FILE_SAVE_CLEAN_TITLE` is what keeps the disabled state
honest, which is the rule the absent-not-disabled idiom actually serves.

**The caption's polarity is the inverse of the note editor's, deliberately.** A note autosaves, so
the fact worth carrying is that the write landed and when (`Saved · HH:MM`, silence while typing). A
file does not autosave, so the fact worth carrying is the one the reader can still act on
(`Unsaved changes`, silence once it is on disk). Same slot, same reservation mechanism, opposite
polarity, because the two surfaces make opposite promises.

## The AD-104 extraction

`src/components/layout/pane-header.tsx` — `PaneHeader` plus `PANE_HEADER_IDENTITY_SLOT` /
`PANE_HEADER_STATUS_SLOT` / `PANE_HEADER_ACTIONS_SLOT`. Three consumers: `NoteEditor` (46.4's
header, migrated), `TextFileFrame` (the second real consumer, and the reason the rule of two is
satisfied), `PanelFrame` (the same identity/actions construction with no status).

**A caller supplies content, never classes.** The three wrappers, their classes and their order are
the component's, because they *are* the fix: a caller that could pass a class into the status slot
could pass `flex-1` into it and the jump would be back with the shape intact. The status is supplied
as `{ sizers, caption }` — strings — so the reservation-by-measurement mechanism cannot be
half-adopted. The outer `<header>`'s padding and border differ per surface and are the one class
hook.

**`status` is nullable, and that is the one thing 46.4 had no reason to decide.** A header with
nothing to report renders two groups rather than an empty reserved box: a zero-width slot in a
`gap-2` row is 8px of space held for nothing, and "there is no status here" is a different claim
from "the status is empty". `PanelFrame` is that consumer.

**`note-editor.tsx` stops exporting `HEADER_IDENTITY_SLOT` / `SAVE_CAPTION_SLOT` /
`HEADER_ACTIONS_SLOT`** — clean cutover, no aliases. `note-editor.test.tsx` and
`note-actions.test.tsx` import the canonical names. `saveStateWord` and `SAVE_CAPTION_SIZERS` keep
their names, signatures and module, so nothing about the note's caption logic moved.

**`PanelFrame`'s regime is the opposite of the capture window's, and the rules hold in both.**
Panels are `flex-1` with a 280px floor inside a horizontally scrolling strip, so a panel header gets
*narrower* than the note editor's rather than wider. Identity absorbs all slack off a zero basis in
both regimes; the status slot's width is a constant in both; the actions are last and shrinkable in
both.

---

## I/O matrix

### The row's menu

| gesture | store call | panels after | `syncOpenEntry` |
| --- | --- | --- | --- |
| single click a file | `setActiveTarget` | same length, active retargeted | not called |
| double click a file | `openPanel` | one more (or focus an existing) | not called |
| menu → Open in this panel | `setActiveTarget` | same length, active retargeted | not called |
| menu → Open in a new panel | `openPanel` | one more | not called |
| menu → Open in the default app | none | unchanged | `(profileId, subpath)` |
| row button `Open` | none | unchanged | `(profileId, subpath)` |
| right-click / long-press a folder or profile root | none | unchanged | not called |

### The fold

| before | act | `folded` | `activeId` | cookie |
| --- | --- | --- | --- | --- |
| unfolded | `toggleFold` | true | unchanged | `f` gains its index |
| folded | `toggleFold` | false | unchanged | `f` loses its index |
| folded | `setActiveTarget` into it | false | unchanged | rewritten |
| folded, holds X | `openPanel(X)` | false | becomes this panel | rewritten |
| folded | `focusPanel` | **true** | becomes this panel | rewritten |
| folded, only panel | `closePanel` | true (refused) | unchanged | unchanged |

### The cookie

| payload | targets | folded |
| --- | --- | --- |
| `{v:2, t:[A,B], f:[1]}` | `[A,B]` | `[1]` |
| `{v:1, t:[A,B]}` | `[A,B]` | `[]` — the documented ruling |
| `{v:1, t:[A,B], f:[0]}` | `[A,B]` | `[]` — `f` is a `v:2` field |
| `{v:2, t:[junk,A,B], f:[2]}` | `[A,B]` | `[1]` — re-counted after the drop |
| `{v:2, t:[A,B], f:["1",1.5,-1,9,null]}` | `[A,B]` | `[]` |
| `{v:9,…}`, non-JSON, absent | `[]` | `[]` |

### The Save bar

| hook / registry state | bar | button | caption |
| --- | --- | --- | --- |
| `dirty: false` | drawn | disabled, `title` says why | empty |
| `dirty: true` | drawn | enabled, no `title` | `Unsaved changes` |
| `entry.writable: false` | absent | absent | — |
| `vm.oversize: true` | absent | absent | — |
| `loading` / `vm === null` / `vm.binary` | absent (early return, unchanged) | absent | — |
| `error` set | drawn | as above | unchanged — the sentence is a banner |

---

## Edge cases

| case | behaviour |
| --- | --- |
| right-click a row that is not selected | the menu opens about that row; the menu never mutates the selection |
| long press on the phone tier | dispatches the synthetic `contextmenu`; the same menu, one visual language, and the following click is suppressed |
| long press on a folder | no handlers spread at all, so the press is not swallowed by a suppressor for a menu that never opens |
| menu item on a fresh keeper (one empty panel) | "in this panel" and "in a new panel" both fill the empty panel — the store's documented branch. The test opens something first, because otherwise the two verbs are indistinguishable and the test would pass for either |
| folding the focused panel | stays focused, comes back unfolded the moment a target arrives |
| folding every panel | legal; each keeps its unfold control |
| a folded panel whose target is deleted | `closeTarget` closes it like any other; the last survivor is emptied, and an empty panel is never persisted, fold or no fold |
| a `v: 2` cookie over the budget | trims from the right; the fold indices are re-counted after every trim, so no index can point past the end |
| every panel folded and the budget bites | the survivors all come back folded; no index survives its panel |
| a save error of any length | banner, as before. It is not the caption: the status slot is a fixed box and everything to its right stands on it |
| a read-only or oversize file | no bar at all, so the caption slot cannot reserve width for a state it can never show |
| two panels over one file | two independent buffers; the second save wins silently. **Deferred, see below** |

---

## Mutation table

Every mutation carried the sentinel `// MUT46-13 …` as the file's first line, so a red anywhere in
the tree during the sweep was greppable by a sibling. **28 applied, 27 caught, 1 equivalent, 0
survived.** Each was reverted by reversing the exact substring (never by restoring a whole file),
because ten agents share this worktree; after the sweep every file was `diff`ed against its
pre-sweep bytes and the sentinel grep run again. One cell timed out mid-sweep and stranded A7 — the
grep caught it immediately, which is the reason the sentinel exists.

### 46.4's own set, re-run against the extracted component

| # | mutation | file | caught by |
| --- | --- | --- | --- |
| 46.4-M1 | drop `shrink-0` from the status slot | pane-header | *gives the slack to identity and to nothing else* |
| 46.4-M2 | add `flex-1` to the status slot | pane-header | *gives the slack to identity and to nothing else* |
| 46.4-M3 | delete the actions wrapper | pane-header | *puts no control in the same shrink context as the caption* |
| 46.4-M4 | render no sizers | pane-header | *reserves the box from strings this machine's own clock produced* |
| 46.4-M5 | reserve for one reference instant | note-editor | *reserves the box from strings this machine's own clock produced* |
| 46.4-M6 | visible caption back in flow | pane-header | *cannot be widened by a save error, and does not swallow one either* |
| 46.4-M7 | make the box depend on the caption | pane-header | *keeps the same box through dirty, saving and saved* |
| 46.4-M8 | size the slot off group 1's content | note-editor | *keeps the same box while the group beside it changes width* |

All eight are killed by `note-editor.test.tsx` — 46.4's own suite, unchanged in substance — and M1–M4
and M6 are independently killed by `pane-header.test.tsx` and `text-file-frame.test.tsx`, i.e. by
both consumers.

### Report A

| # | mutation | caught by |
| --- | --- | --- |
| A1 | first item opens beside instead of here | *replaces the active panel from the first item* |
| A2 | second item replaces instead of appending | *opens a second panel from the second item* |
| A3 | third item opens a panel instead of leaving keeper | *leaves keeper entirely from the third item* |
| A4 | folders get the menu too | *does not make a folder a panel target* |
| A5 | the third verb goes back to being called `Open` | *names all three ways to open a file, and words them apart* |
| A6 | the short button stops answering to the long verb | *says the short word and answers to the long one* |
| A7 | the long-press bridge is not spread on the row | *opens the same menu on a phone-tier long press* |

**A1 survived its first run and the test was wrong, not the code.** The original test started from a
fresh keeper, where `openPanel` fills the empty active panel rather than appending — so the two
verbs are indistinguishable there and the assertion held for either. Rewritten to open something
else first; A1 is now caught. This is the mutant that justified the sweep.

### Report B

| # | mutation | caught by |
| --- | --- | --- |
| B1 | a target arriving does not unfold | *unfolds a panel it is given something to show* (+2 more, incl. the strip) |
| B2 | a fold is never written down | *round-trips which panel was folded, and only that one* |
| B3 | a `v: 1` cookie is read for folds it never had | *refuses to read a fold out of a version that had none* |
| B4 | a `v: 1` cookie is discarded instead of restored | *restores a cookie written before folding existed* (+3) |
| B5 | fold indices not re-counted over surviving targets | *counts the fold over the targets that survived the way back in* |
| B6 | count folds before dropping empty panels | **equivalent — see below** |
| B7 | a folded panel keeps its share of the width | *folds a panel away, and the body goes with it* |
| B8 | a folded panel keeps its body mounted | *folds a panel away, and the body goes with it* |
| B9 | the fold control's name stops saying which way it goes | *offers the way back in, and takes it* |
| B10 | `aria-expanded` does not follow the fold | *offers the way back in, and takes it* |
| B11 | the last panel is offered no fold | *offers to fold the last panel, which it refuses to close* (+3) |

**B6 is an equivalent mutant and the code changed because of it.** The encoder could not observe an
empty panel at all — `panelsCookie` filters them out before calling it — so its `target === null`
guard was unreachable, and no mutation of the counting order was observable through the public
surface. Unreachable code is where an off-by-one waits, so the filter now *narrows* to a
`HoldingPanel` and the guard is gone: the invariant lives in one place and the encoder has nothing to
decide. B8 also failed to be caught on its first run — the test looked for the file's name, which a
viewer that happened to draw nothing would satisfy — and now asserts the section's shape (one child,
a `<header>`).

### Report C

| # | mutation | caught by |
| --- | --- | --- |
| C1 | Save is always pressable | *is reachable, and says why it cannot act when it cannot* |
| C2 | Save says nothing about why it is disabled | *is reachable, and says why it cannot act when it cannot* |
| C3 | Save is offered over a truncated read | *is not offered over a file only the first part of which was read* |
| C4 | Save is offered over a format keeper will not write | *is not offered over a format keeper will not write* |
| C5 | the caption's polarity is inverted | four tests, incl. *keeps the same box across a save* |
| C6 | the reservation stops coming from the caption's own words | *reserves the box from the string the caption can actually show* |
| C7 | the button no longer calls `save` | *saves the buffer the hook is holding, and nothing else* |
| C8 | the bar is drawn with no Save in it | *is not offered over a format keeper will not write* (+1) |

---

## Deliberately NOT done

- **No Delete, Rename, Reveal or Copy path in the row's menu.** The three items are the three ways to
  open, which is the report. Duplicating the row's visible buttons into the menu is a decision about
  what a menu is *for*, and 45.3's Delete has a confirmation flow whose entry points are its own
  story.
- **No fold for the sidebar's groups.** `sidebar-fold.ts` is restricted to two groups by its own
  design; the owner's "fold panels other than the first" is answered by the panel fold, and the
  literal reading is recorded above rather than silently implemented.
- **A folded panel is not a vertical tab.** No rotated text, no writing-mode: it is the unfold
  control and the section's `aria-label`. Appearance is a gate check, and inventing a second visual
  idiom for a panel would need one.
- **No `saving` state for a file.** `useTextBuffer` exposes no in-flight flag and this story did not
  add one: a third caption state with no store behind it would be a caption that lies.
- **`PanelFrame` gets no status group.** It has nothing to report; an empty reserved box would be 8px
  of gap held for nothing. The nullable `status` is what makes that expressible.
- **No autosave for files.** Still deliberate (`spec-45-6`). The button is the answer to the missing
  feedback, not to the missing autosave.
- **Two panels over one file still get two independent buffers**, and the second save wins silently.
  `useTextBuffer` is per-mount — the exact mirror image of the singleton W3Notes deleted for notes in
  46.12. This story makes the condition *visible* (each bar shows its own `dirty`) rather than fixing
  it; a per-file document mirror is the same lift 46.12 did for notes. **Ledger candidate, DW-197 or
  later** (DW-195 is W3Tray's, DW-196 is W3Recording-2's).
- **Not flipping `sprint-status.yaml`.** Several agents share it this wave; the ledger is Main's.

---

## What I could not verify here, and why

**No layout can be measured, and this is the same limit 46.4 wrote down.** jsdom performs no layout
— every element reports a zero rect, and `src/test/setup.ts`'s shim answers a viewport only for
zero-sized elements and deliberately stops at the edge of a CodeMirror editor. Every claim about
width in this story is asserted as the structure that *causes* the shift, never as a pixel. A test
asserting "the Save button did not move" would be asserting the shim.

Specifically **not** proven by any check here, and needing eyes on the macOS host:

1. that a folded panel is *visibly* narrow and that the strip's neighbours visibly take its width —
   the classes say so, jsdom cannot;
2. that a folded panel showing only its unfold control is legible as a panel and not as debris. This
   is the one piece of this story most likely to need a second pass on taste;
3. that the frame's Save bar does not read as a duplicate of the panel header's file name in a Files
   panel (the note editor has the same relationship between `PanelFrame`'s "Note" and its own `h1`,
   deliberately, but two file names in two rows is a judgement a screenshot settles);
4. that the reserved `Unsaved changes` box is not visibly too wide beside the Save button;
5. that the row's three text buttons plus the name still fit a narrow Files pane after nothing about
   their widths changed (the `Open` button's visible text is unchanged, which is why it was left
   short);
6. that the phone tier's long press on a tree row does not fight the tree's own scroll — the hook
   cancels past a 10px move, and only a real finger settles it.

No Rust was read or edited, so **nothing in this story is blocked on the macOS gate for
correctness**; the `keeper` shell crate was neither read nor touched, and no command was added, so
`src/test/command-registration.test.ts` is unaffected.

### Ordered gate checks

1. **`bun run test src/components/layout/ src/components/viewers/ src/lib/stores/
   src/components/notes/note-editor.test.tsx`** → EXIT=0, three times. *(Done here: 3/3, 75 files,
   1268 tests.)*
2. **`bun run typecheck`** → clean. *(Done here for every file this story touches. Note it also
   required adding `caveat: null` to the `FilesWriteVm` fixtures in `files-pane.test.tsx` and
   `panel-strip.test.tsx`, and `folded: false` to `app-shell.test.tsx`'s one `Panel` literal, as
   W3Outside's and this story's type changes landed.)*
3. In the built app, open the Files pane. **Right-click a file.** Three items, and press each in
   turn: the first must replace what is beside the tree, the second must add a panel, the third must
   open the file in the OS app and leave the panels alone.
4. Right-click a **folder** and a **profile root**: the native menu, or nothing — never an empty
   keeper menu.
5. Open three panels. **Fold the middle one.** Its neighbours must visibly take the width; only the
   unfold control may remain; hovering it must name the file. Unfold it — the document must come
   back.
6. Fold a panel, then **single-click a different file in the tree**. The panel must unfold and show
   the new file. This is the one behaviour whose absence would reproduce the epic's own defect.
7. **Fold the only panel.** It must fold, and Close must not be offered.
8. Fold one panel, **quit and relaunch**. The same panel must come back folded and the others open.
9. **Downgrade check, if a previous build is to hand:** launch the older build once. It must come up
   with a clean workspace rather than a broken one — that is the documented price of `v: 2`.
10. Open a `.md` in a Files panel. The bar must show the name, an empty caption and a **disabled
    Save** whose tooltip says nothing has changed. Type: the caption must read `Unsaved changes`,
    Save must enable, **and the Save button must not move**. Press it; the caption must empty.
11. `Mod-s` must still save, and must leave the caption in the same state the button does.
12. Open a **PDF** and a **very large text file**: no bar at all in either.
13. Make the volume read-only and press Save: Rust's sentence in the banner, and the bar must not
    move.
14. Open a note and repeat 46.4's step 3 (type, watch the caption cycle, the `⋯` must not move by a
    pixel) — the header is now a shared component, so this is a regression check on the extraction
    rather than on 46.4.
15. On a phone-tier window, **long-press a file row**: the same three-item menu, and the row must
    not also activate under the finger.
