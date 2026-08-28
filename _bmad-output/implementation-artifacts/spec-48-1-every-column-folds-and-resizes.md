# Spec 48.1 — Every column folds, and every column resizes

<intent-contract>

**The ask, twice.** *"daj mozliwosc folding innych paneli nie tylko pierwszego"* — give me the
ability to fold panels other than the first. Then, after Story 47.3 shipped:
**"wciaz tylko pierwsza kolumna jest mozliwa do foldowania"** — still only the first column can be
folded. Plus, from the same report and never built at all: *"w panelach daj mozliwosc
zawezenia/rozszezenia kolumn"* (let me narrow and widen the columns) and *"staraj sie dopasowac
kolumny do szerokosci - nie tnij tresci"* (fit the columns to the width; do not cut the content).

**The reading that shipped, and the reading that was meant.** Story 46.13 read the first ask as the
Files panel strip and shipped a per-panel fold. Story 47.3 read it as the notes rail's SECTIONS and
shipped three section folds. Both are real features and neither is what was asked. The owner means
the **COLUMNS** — the vertical slabs the shell is made of. 47.3 even wrote the gap down in its own
spec (`spec-47-3-every-rail-section-folds.md:253-255`: *"No fold for the notes rail as a whole …
Adding one is a layout story, not this one."*). **This is that story, and it supersedes that
refusal.**

**What was actually there.** One column in the entire app folded, and no column anywhere resized.

| surface | column | before this story |
| --- | --- | --- |
| every surface | app sidebar (`sidebar-pane.tsx`) | folds to `w-12`, persisted — Story 45.20. **The only one.** |
| Notes | scope rail (`notes-pane.tsx:379-381`) | hand-written `w-[240px]`, no fold, no seam |
| Notes | note list (`notes-pane.tsx:419-421`) | hand-written `w-[320px]`, no fold, no seam |
| Notes | panel strip (`notes-pane.tsx:513`) | `flex-1` — the flexible column, correctly |
| Files | tree (`files-pane.tsx:1749`) | `min-w-0 flex-1` — **fighting the strip for half the window** |
| Files | panel strip | `min-w-0 flex-1` |
| Inbox | chat list (`chat-list-pane.tsx:590`) | hand-written `w-[320px]`, no fold, no seam |

And the resize machinery already existed. `useResizableColumn(id, label)`
(`resizable-columns.tsx:93`, Story 44.12) is fully general — it takes an id and a label and nothing
else — and in the entire application it was wired to **one** boundary: the Properties panel's key
column. This is the same defect shape as Story 45.15's unmounted `CaptureNoteItem`: a complete
mechanism, built, tested, and connected to almost nothing.

**Delivered.** All four fixed surface columns fold to a 48px strip that carries the control that
undoes it, persisted in one new cookie and restored at the shell. All four carry a draggable,
keyboard-operable seam sharing the existing `keeper_column_widths` cookie, each with a floor chosen
for that column rather than the shared 72px. Folding never spends a remembered width.

</intent-contract>

## The four things that had to be got right

### 1. There was no owner of column layout, so this is four call sites and one new part

`app-shell.tsx` is a bare flex row; each surface hand-writes its own columns with its own Tailwind
width literal. There was nothing to extend. The new part is `useSurfaceColumn(id)`
(`src/components/layout/surface-column.tsx`), and it is a **hook returning nodes** rather than a
wrapper component, because the four column roots are:

| surface | root element | what it carries that a wrapper would have to forward |
| --- | --- | --- |
| notes rail | `<nav aria-label="Notes">` | a landmark role and its name |
| note list | `<div onKeyDown>` | the Esc chip-walk handler, at the column so it works from a row |
| Files tree | `<section aria-label="Files">` | a region role and its name |
| chat list | `<div ref tabIndex={-1} onKeyDown>` | the summon-hotkey focus target and the Esc filter clear |

A wrapper owning the root would forward a role, a name, a ref, a `tabIndex` and two key handlers,
and would still be wrong for the fifth column. So the root stays the surface's and spreads
`rootProps`; the hook hands back the two pieces that must be identical everywhere — `chrome` (the
fold control, the column's first child) and `seam` (the resizer, the column's next **sibling**).

### 2. A column fold and a section fold are different facts, so they get different cookies

`keeper_column_fold` is new, keyed `notes-rail` / `notes-list` / `files-tree` / `chat-list`.
`SIDEBAR_GROUPS` was not widened and 47.3's `keeper_notes_rail_fold` was not reused. The reason is
concrete rather than tidy: folding the notes rail COLUMN must leave the Spaces/Tags/Files SECTIONS
exactly as the user left them, so that unfolding gives back the rail they had. One namespace makes
that impossible to express. 47.3 refused to share with 45.20 for the same reason and this follows
it — the ENCODING is shared (`fold-cookie.ts`), the namespaces are not.

The key set is **imported** from `SURFACE_COLUMN_IDS` in `column-widths.ts` rather than restated,
because the same four ids key the width cookie. A column that could be folded but not resized would
be a typo nobody would ever find.

### 3. The floor is per column, and 72 was never a floor for a column

`MIN_COLUMN_WIDTH = 72` was chosen in Story 44.12 for a property KEY, and its own doc comment says
why it is safe: *"The floor is wide enough to keep an ellipsis and the overflow trigger on screen,
which is the escape hatch."* A surface column has no overflow trigger. What is past its edge is
simply gone. So `clampColumnWidth(px, min?)` takes a floor, `columnMinWidth(id)` supplies it, and it
is applied on the drag, on the write **and on the read** — a floor enforced only on write leaks the
moment a build lowers one, or a person edits the jar.

| id | default | floor | why that floor |
| --- | --- | --- | --- |
| `notes-rail` | 240 (unchanged) | **180** | the New note button: 16px icon + gap + the words + 8px padding either side. Below it the label the button exists to advertise clips, and a space row's trailing `+` lands on its name. |
| `notes-list` | 320 (unchanged) | **240** | a row is a title line and a meta line of tag chips, under a filter bar holding a search field. Narrower and the chips wrap one per line — which makes the list *taller*, not narrower. |
| `files-tree` | **360** (was `flex-1`) | **220** | tree rows indent 16px per level. The floor keeps three levels and a short name, which is where the tree stops being navigable rather than merely tight. |
| `chat-list` | 320 (unchanged) | **240** | a row is a 40px avatar, a name, a preview line and a timestamp. The floor is where the timestamp starts eating the name. |

`MAX_COLUMN_WIDTH = 640` is shared and unchanged: past it the flexible column is the one that has
vanished.

**`files-tree`'s default is the one visible behaviour change.** It was `min-w-0 flex-1` — the same
class as the panel strip beside it — so the surface split evenly between a folder list and the
document that list opens. That was never a decision; it was two panes with the same class. 360 makes
the strip the flexible column, which is the arrangement Notes has had since Story 46.12, and the
seam plus `Home`/double-click make any other split one gesture away.

### 4. Fold and resize interact, and the rule is deliberate

**A fold suspends a width; it never spends one.**

Nothing on the fold path writes `keeper_column_widths`, and nothing on the width path reads the
fold. Fold and unfold and the column comes back exactly as wide as it was left. The seam is
**unmounted** while folded rather than disabled: there is nothing to size, and a drag on a 48px
strip would write a width the fold is not showing.

The alternative — letting the strip's 48px reach the width cookie, or clearing the width on fold —
turns one accidental fold into a lost layout, and it is the shape that gets this wrong *silently*,
because every intermediate state looks plausible. That is not hypothetical this week: Story 48.2 is
fixing exactly this failure one layer down, where `notes_capture_set_locked` merged
`live.size.or(stored.size)` and so overwrote a remembered window size with the normalised one on the
unlock click. Mutation M6 below models the same bug in this layer and is killed.

## I/O matrix

### `useSurfaceColumn(id, options?)` — `surface-column.tsx`

| state | `rootProps.style.width` | `chrome` | `seam` | body |
| --- | --- | --- | --- | --- |
| fresh, no cookie | `SURFACE_COLUMNS[id].defaultWidth` | `Collapse <label>`, `aria-expanded=true` | rendered, `aria-valuenow` = default | rendered |
| width remembered | the remembered px | as above | `aria-valuenow` = remembered | rendered |
| folded | `48` | `Expand <label>`, `aria-expanded=false` | **null** | **not rendered** |
| folded, width remembered | `48` | `Expand …` | null | not rendered; the width cookie is untouched |
| `enabled: false` (phone) | default | **null** | **null** | rendered, whatever the fold store says |

`rootProps` also carries `id="column-<id>"` (the fold button's `aria-controls`) and
`data-folded="true"` while folded.

### `readColumnFold(cookie)` / `columnFoldCookie(fold)` — `column-fold.ts`

| in | out |
| --- | --- |
| jar without `keeper_column_fold` | every column showing |
| `keeper_column_fold=notes-rail%3A1%7Cchat-list%3A0` | `{notes-rail: true, chat-list: false, …open}` |
| key this build has no column for (`nope:1`) | dropped |
| value not `0`/`1` (`notes-rail:yes`) | dropped, that column stays showing |
| entry with no `:` (`chat-list`) | dropped |
| only `keeper_notes_rail_fold` in the jar | every column showing — no cross-read |
| write | **every** column, folded or not (a cookie write replaces the whole value) |

### `columnMinWidth(id)` / `clampColumnWidth(px, min?)` — `column-widths.ts`

| in | out |
| --- | --- |
| `columnMinWidth("notes-rail")` | 180 |
| `columnMinWidth("properties-key")` | 72 (`MIN_COLUMN_WIDTH`) — not a surface column |
| `clampColumnWidth(80, 180)` | 180 |
| `clampColumnWidth(NaN, 180)` | 180 |
| `clampColumnWidth(9999, 180)` | 640 (`MAX_COLUMN_WIDTH`) |
| `readColumnWidths("…=notes-rail:80\|properties-key:80")` | `{notes-rail: 180, properties-key: 80}` |
| `columnWidthCookie(jar, "chat-list", 100)` → read back | 240 |

### The seam — `ColumnResizer`, two new props

| prop | before | now |
| --- | --- | --- |
| `min` | absent; `aria-valuemin` hard-coded `MIN_COLUMN_WIDTH` | supplied by the hook as `columnMinWidth(id)`. A slider announcing a minimum the drag will not honour is a slider that lies. |
| `className` | absent; placement hard-coded `col-start-2 [grid-row:1/-1]` | optional override, defaulting to exactly that. The Properties grid passes nothing and is byte-identical in behaviour; a surface column passes `shrink-0` because its host is a flex row where the grid properties are inert. |

## Edge cases

- **A folded column with no handle is a column the user deleted by accident.** Folded, the strip
  renders the expand button and nothing else. It is a real `<button>` in the tab order with a name
  that says which way it goes and `aria-expanded` saying where it is now — the shape
  `sidebar-pane.tsx:180-200` already uses, copied deliberately so a person who has met one fold has
  met all of them.
- **The phone tier.** `PhoneShell` mounts `ChatListPane` in a single-pane stack. `useSurfaceColumn`
  is called with `enabled: !phone` there, so the phone gets neither control — a fold would hide the
  whole screen behind a 48px strip, and a seam would be a drag target with nothing beside it to
  trade width with. A fold made on the desktop is **ignored** on the phone rather than obeyed, so a
  remembered fold cannot follow the user onto an arrangement with no way to undo it. `NotesPane` and
  `FilesPane` are not mounted by `PhoneShell` at all.
- **Below 1080px.** `app-shell.tsx:284` withdraws the *sidebar's* fold because the viewport has
  already forced it and the control would lie. A surface column is the opposite case: nothing has
  decided for the user, and it is precisely where room is short that putting a column away is worth
  offering. So the column folds stay available at every desktop width, and there is a test at 1000px
  asserting both halves — the sidebar's control absent, the chat list's present.
- **A folded column unmounts its body**, following `PanelFrame`'s rule (`panel-strip.tsx:302-311`):
  a body kept behind `hidden` keeps its subscriptions and its `sync_browse` alive, which is the cost
  folding reclaims. For `FilesPane` and `ChatListPane` this is an early return placed after every
  hook, so the data effects above it keep running and unfolding shows what happened while the column
  was away rather than a cold scan.
- **Two columns on one screen need two names.** `Resize Notes` would name both the rail and the
  list. Every label is the column, not the surface, and a test asserts the four are distinct.
- **A jar written by an older build** loses nothing: an unknown fold key is dropped, and a width
  recorded below today's floor is lifted on read rather than at one call site.
- **`Home` and double-click on a seam** forget the width entirely, so the column returns to its
  default. That is inherited from 44.12 and is the door out of a regrettable drag.
- **The summon hotkey over a folded inbox does nothing, safely.** Story 9.4's cold-start fallback
  focuses `containerRef` when the Inbox has no row to land on. While the column is folded that ref
  is null, and both call sites are already null-safe (`containerRef.current?.focus()` at
  `chat-list-pane.tsx:402`, and the pending-request effect abandons because `document.activeElement`
  cannot equal `null`). So the raise switches to Inbox and leaves focus where it was rather than
  throwing. Whether the hotkey should *unfold* the column instead is a real question and is not this
  story's — see the DW note below.

## Mutation table

Every fix proved by removing it and watching a **named** test fail. Sentinel `MUT48-1`.

Baseline established **in the same command and the same filter as the sweep**, immediately before
it: `bun run test src/lib/column-widths.test.ts src/lib/stores/column-fold.test.ts
src/components/layout/surface-column.test.tsx src/components/layout/app-shell.test.tsx
src/components/layout/chat-list-pane.test.tsx src/components/layout/files-pane.test.tsx
src/components/notes/notes-pane.test.tsx` → **7 files / 271 tests / 0 failed**. (The wider
`src/components/layout/ src/components/notes/` filter could not be the sweep baseline: it was red
throughout on `priority-actions.test.tsx` and `note-editor.test.tsx`, both mid-flight in a sibling
agent's story. Scoring kills against a filter that is already red proves nothing, so the sweep used
a filter that was green.)

Restore verified three ways, because `git diff` is blind to the files I created:
`sha256sum -c` against a pre-sweep manifest of all seven touched sources, a repo-wide
`grep -rn MUT48-1 src/`, and **reading** the `git diff` of each tracked file that was mutated.

**Eleven mutations, eleven kills, zero survivors.** Kill counts are out of 271 and were re-verified
green between every mutant, not only at the end.

| # | mutation | kills | named tests that went red |
| --- | --- | --- | --- |
| M1 | `useSurfaceColumn` returns `folded: false` always | **28**, across 5 files | `the <id> column › folds to a strip that still holds the way back` ×4; `… › comes back folded after a reload` ×4; `… › comes back showing…` ×4; `NotesPane columns › folds the rail without taking the list with it`; `… › folds the list without taking the rail with it`; `FilesPane — the tree is a column › folds to a strip…`; `ChatListPane — the inbox is a column › folds to a strip…`; `AppShell › brings the chat list back folded…` |
| M2 | `toggleColumn` stops calling `persistFold` | **10** | `column fold store › toggles one column and writes the whole set out`; `the <id> column › comes back folded after a reload` ×4; `… › survives a reload folded AND resized…` ×4; `surface columns as a set › folds one column without touching another` |
| M3 | drop `hydrateColumnFold(document.cookie)` in `AppShell` | **1** | `AppShell › brings the chat list back folded when the last run left it folded` — **and nothing else**, which is the DW-172 point exactly: the store's own suite is blind to it |
| M4 | `seam` is always `null` | **23** | `the <id> column › resizes from the keyboard and remembers it across a reload` ×4; `… › resizes from a drag` ×4; `… › stops at its own floor…` ×4; `FilesPane … › carries a seam that remembers the width across a remount`; `ChatListPane … › resizes and remembers it across a remount`; `NotesPane columns › puts a seam on each column…` |
| M5 | per-column floor removed: `clampColumnWidth(px)` on read and write, `clampColumnWidth(next)` in the hook | **5** | `surface column floors › lifts a width recorded below today's floor on the way back in`; `the <id> column › stops at its own floor rather than the shared one` ×4 |
| M6 | **the fold spends the width**: the fold click also writes the strip's 48px through `onWidth` (Story 48.2's bug, one layer up) | **8** | `the <id> column › keeps the width it was given while folded, and gives it back on unfold` ×4; `… › survives a reload folded AND resized, then unfolds to the chosen width` ×4 |
| M7 | `ChatListPane` ignores `phone` and always enables the column | **1** | `ChatListPane — the inbox is a column › offers neither control on the phone, and ignores a fold made on the desktop` |
| M8 | `NotesPane` drops `{rail.chrome}` | **2** | `NotesPane columns › folds the rail without taking the list with it`; `… › does not disturb which rail sections are folded` |
| M9 | `NotesPane` drops `{list.chrome}` | **2** | `NotesPane columns › folds the list without taking the rail with it`; `… › puts a seam on each column, and takes the folded one's away` |
| M10 | `FilesPane` drops `{tree.chrome}` from both frames | **2** | `FilesPane — the tree is a column › folds to a strip…`; `… › comes back with the tree it had` |
| M11 | `ChatListPane` drops `{column.chrome}` from both frames | **4** | `ChatListPane — the inbox is a column › folds to a strip…`; `AppShell › brings the chat list back folded…`; `… › brings it back showing…`; `… › keeps offering the column folds below the sidebar's collapse breakpoint` |

M8–M11 exist because M1 proves the *mechanism* works and says nothing about whether a given surface
wired it. That distinction is the whole of Story 45.15's defect and the whole of this story's: the
machinery was there and the columns were not connected to it. M3 and M7 are the two mutants that
kill exactly one test each, and in both cases that is the point — they are the assertions no other
layer can make.

**The reload tests are the cookie's, not the store's.** Each unmounts, **wipes the module-level
store**, hydrates from `document.cookie`, and remounts — which is the only reload jsdom has. An
in-process remount would pass on a memory-only store.

## The `FilesPane keyboard navigation` red, investigated rather than filed

A sibling agent's gate run saw
`FilesPane keyboard navigation › steps down and up one visible row at a time` fail once in six —
`expected 'Vault' to be 'Field'`, i.e. `ArrowDown` did not move focus — while three agents were
running suites concurrently on a six-core box. It was handed to me because `files-pane.tsx` has 307
changed lines this wave and they are mine. **They are 39 real insertions and 4 deletions; the other
264 are the formatter re-indenting the body after the root was wrapped in a fragment**
(`git diff -w --stat` = 39/4). The verdict below is the reason I did not close it as flake, and the
reason I do not believe it is this story's.

**It is not the `getBoundingClientRect` shim.** That was the leading hypothesis, and it is the right
one to check — it is what virtualised CodeMirror's middle away in epic 45, and `setup.ts` still
answers a full 1024×768 viewport for any zero-sized element outside `.cm-editor`. But
`window-list.tsx` **never calls `getBoundingClientRect`** (grep: zero occurrences), and neither does
`files-pane.tsx`. It measures with `clientHeight` at `window-list.tsx:267` and `:292`, and treats
zero as "nothing was laid out" rather than as a measurement: the viewport falls back to
`ASSUMED_VIEWPORT_HEIGHT = 640` and each row to `FILES_ROW_ESTIMATE = 32`. That is a twenty-row
window over a **two-row** tree. There is no index in it to be off by one.

**It is not the fold or the seam.** The fold control is a sibling rendered *above* the scroll
container (`files-pane.tsx:1783` vs the viewport at `:1846`), and the seam is outside the
`<section>` entirely (`:1950`). Neither is inside the box whose `clientHeight` is measured, and in
jsdom that height is zero for all of them anyway.

**It reproduced once more, on a different test, and that is what identified it.** Across 30 runs of
the acceptance filter and its neighbours I saw exactly one further red:
`FilesPane › re-reads only the folders that are open when Refresh is pressed` —
`expected syncBrowse to be called 1 times, but got 2 times`, again with an
`An update to FilesPane inside a test was not wrapped in act(...)` warning beside it. Three
different tests, one shape.

**The mechanism, named.** `files-pane.tsx:757-775` restores the remembered expansion once per
mount, and it reads the store **live**:
`for (const key of reachableNodeKeys(filesTreeStore.getState().expanded)) load(...)`. It is a
passive effect keyed on `[profiles, load]`, so it fires when `syncProfiles()` settles — and
`load` is documented at `:696-702` as *always* re-asking, deliberately, because Refresh means "ask
again". Testing Library's `findBy*` resolves off a MutationObserver, which is a microtask; React's
passive effects are scheduled work. So on a contended box a test can observe the committed DOM,
proceed, and act on it **before that effect has run**. Then:

- the user (or the test) expands a folder → `toggle` writes the store and calls `load` — one call;
- the pending `restored` effect finally runs, reads the store live, now sees the just-opened folder
  in it, and calls `load` again — **two calls**. That is the Refresh red exactly.
- and in the nav test, `press()`'s `act(...)` is what flushes that pending effect; the effect calls
  `retainProfiles`, the store write re-renders the tree, and the row element captured *before* the
  `act` is replaced — so the `keyDown` dispatched at it never reaches React, nothing moves, and
  `document.activeElement` is still the first row. **`expected 'Vault' to be 'Field'`, exactly.**

One cause, three symptoms, and all of it in code this story does not touch: the effect, `load`, the
`toggle` and the nav suite are unchanged since Story 46.3. My change adds a zustand subscription and
a lazy `useState` above them and no additional render.

**It did not reproduce in thirty runs on an idle box.** 6× and then 3× more of
`src/components/layout/ src/components/notes/ src/lib/` (133 files / 2194 tests); 6× the sibling's
exact filter `src/components/notes/ src/components/layout/ src/components/capture/` (71 files /
1496 tests); 4× `files-pane.test.tsx` against a second full suite running concurrently; 8× the
`keyboard navigation` describe with all six cores saturated. One red in thirty, and it was the
Refresh one above. Every clean run logged **zero** `not wrapped in act` warnings; both failing runs
logged them. That warning is the tell, and it is a load signal rather than a code signal.

**What I did about it rather than nothing.** Added
`FilesPane — the tree is a column › leaves the tree's keyboard stepping intact across a fold and an
unfold`: two profiles, fold, unfold, then assert both rows are back in the window and that
`ArrowDown` lands on the second. That is the exact interaction the question was about, asserted
deterministically instead of argued. Green, three runs.

**What happened next: it became Story 48.6, in this branch.** I proposed deferring the fix, because
`load`'s "always re-ask" is a documented contract Refresh depends on and changing it from inside a
story about columns is how the next silent defect gets in. Main overruled that on the facts — three
lines, in a file already open, against a real double directory read on a 91,000-file tree — and
asked for it as its own story rather than a stray hunk. Done: `spec-48-6-a-folder-is-re-read-once.md`,
with its own mutation table, including the mutation that proves the *obvious* fix (memoise `load`)
is the wrong one. **DW-D is withdrawn** — a deferred entry for something fixed in the same wave is
noise. `files-pane.tsx` therefore carries two named changes on this branch: the column, and the
duplicate read.

## Acceptance

`bun run test src/components/layout/ src/components/notes/ src/lib/` — **EXIT=0, three consecutive
runs**. `bunx tsc --noEmit` clean of anything in this story's files.

| requirement | where |
| --- | --- |
| per column: it folds | `surface-column.test.tsx` `describe.each(SURFACE_COLUMN_IDS)` › `folds to a strip…` |
| … stays folded across a remount | `… › comes back folded after a reload` (store wiped, cookie re-read) × 4, plus both arms |
| … its expand control is reachable while folded | `… › folds to a strip that still holds the way back` asserts the button, its `aria-expanded`, that it takes focus, and that clicking it brings the body back |
| … it resizes and remembers | `… › resizes from the keyboard and remembers it across a reload` and `… › resizes from a drag` (real pointer events) × 4 |
| a test for the fold/resize interaction rule | `… › keeps the width it was given while folded, and gives it back on unfold` and `… › survives a reload folded AND resized, then unfolds to the chosen width` |
| the four surfaces actually wired it | `notes-pane.test.tsx` (× 2 columns), `files-pane.test.tsx`, `chat-list-pane.test.tsx`, `app-shell.test.tsx` |
| the restore is mounted where it cannot be forgotten | `app-shell.test.tsx › brings the chat list back folded…` (DW-172 shape) |
| the two fold namespaces stay separate | `NotesPane columns › does not disturb which rail sections are folded`; `column fold cookie › reads its own cookie and not another fold's` |
| mutation-prove each fold, the persistence, and the seam | table above |

## Deliberately NOT done

- **The recorded refusals nearby are about a DIFFERENT set and are not overridden.**
  - `spec-44-12-…md:254-260` refuses seams **inside** the Files tree and the note list — between a
    name and its trailing group, where there is no boundary between two columns. Still true. This
    story's seams are between the tree and the panels beside it, and between the list and the rail:
    boundaries that have always existed.
  - `spec-45-1-…md:239-240` refuses per-**panel** resize inside the strip. Still true; the strip's
    panels remain `flex-1` with a 280px floor. The strip *as a column* is the flexible one on both
    surfaces and correctly has neither a fold nor a seam — a surface where every column has a fixed
    width is a surface with a gap in it.
  - `spec-47-3-…md:253-255` refuses a fold for the notes rail as a whole and calls it "a layout
    story, not this one". **This is that story; that refusal is superseded.**
- **No fold or seam for the panel strip, the conversation pane or the detail panel.** The first two
  are the flexible column on their surfaces. The detail panel already has a toggle of its own
  (`toggleDetail`) and floats as a Sheet below 1280px, which is a different and older contract.
- **No drag to reorder columns, no "fold all", no keyboard chord for a fold.** Not asked for.
- **No per-column max.** 640 is shared. A per-column ceiling would need a reason and none of the
  four has one.
- **The 44.12 fitted-width path is untouched.** A surface column is never `fit-content`; the
  Properties key column still is, and `columnTemplate` is unchanged.
- **`MIN_COLUMN_WIDTH` was not changed**, only defaulted-to. The Properties key column's floor is
  still 72 and its behaviour is byte-identical.
- **The formatter and linter were not run repo-wide** (Main runs them once, per the wave rule).
  `bunx biome check --write` was run on this story's files only, so Main's pass is a no-op over them.
- **No new dependency.** No `localStorage`.

### Proposed deferred-work entries (numbers for Main to land; I did not edit `deferred-work.md`)

- **DW-A — the global summon hotkey does not unfold the inbox.** With the chat list folded, ⌘-summon
  switches to Inbox and lands focus nowhere (safely — see Edge cases). Arguably a raise should undo
  a fold that hides the thing being raised. It is a decision about what a summon means, not a bug in
  this story, and it applies equally to any future "focus the note list" chord.
- **DW-B — the two resize idioms are documented in one place and implemented as two hosts.** The
  Properties key column is a CSS grid with a zero-width middle track; a surface column is a flex row
  with a zero-width sibling. `ColumnResizer` now serves both through a `className` override, which
  is honest but is a hinge that will need a third case eventually. Worth revisiting if a third host
  appears, not before.
- **DW-C — `columnTemplate`'s fitted branch still hard-codes `MIN_COLUMN_WIDTH`.** Correct today
  (its only caller is the Properties grid, whose floor is 72), and wrong the day a surface column
  wants a fitted mode. Left alone rather than parameterised for a caller that does not exist.
- **DW-D is WITHDRAWN.** It was the duplicate `sync_browse`; it is fixed in this branch as Story
  48.6 (`spec-48-6-a-folder-is-re-read-once.md`). Nothing to land.

## What I could not verify here, and why

1. **Nothing Rust, nothing packaged.** Entirely frontend. The `keeper` shell crate does not build on
   Linux and was not touched; no `cargo` command was run and none is needed for this change.
2. **Layout is asserted by inline style and by presence, not by geometry.** jsdom has no layout
   engine. "The rail is 240px and the strip takes the rest" is proved as *the root carries
   `width: 240px` and the strip carries `flex-1`* — necessary and sufficient in this flex row — and
   not as measured pixels. Whether 180px is genuinely enough for the New note button **in the real
   font** is a browser fact and this is not a browser; the floors are reasoned from the elements in
   each row and need a human to confirm.
3. **The drag is exercised with synthetic pointer events** against jsdom's stubbed pointer capture
   (`src/test/setup.ts`). The arithmetic is real and the cookie write is real; pointer capture
   behaviour under a fast real drag is not.
4. **Cookie persistence across a real app restart** is exercised as string → parse → store, not as
   WebKit actually retaining `keeper_column_fold` for a year.
5. **`columnTemplate`'s `minmax(72px, fit-content(50%))` is still dropped by jsdom's CSS parser**
   (44.12 documented this). Irrelevant here — surface columns use an inline `width`, not the grid
   template — but it is why the seam's placement class is asserted by nothing and read by a human.

### Ordered gate checks

Run in this order; each assumes the previous passed. **Everything below marked *(ran)* was run by
me in this worktree. Step 6 was not, and is the one that decides whether the story is actually
delivered.**

1. `bunx tsc --noEmit` → clean of anything in this story's files. *(ran; the only errors in the tree
   were `alwaysOnTop` on `CaptureWindowVm`, mid-flight in Story 48.4's files.)*
2. `bun run test <the seven files above>` → 271 passed, EXIT=0. *(ran, and it is the mutation
   baseline.)*
3. `bun run test src/components/layout/ src/components/notes/ src/lib/` → EXIT=0, three consecutive
   runs. *(ran — see the run log at the end of this section.)*
4. `bun run test` (full frontend suite) → EXIT=0. **Not run here**; the wave rule reserves it for
   Main. The blast radius outside those three directories is `resizable-columns.tsx`, whose only
   other consumer is `properties-panel.tsx` in `src/components/notes/` — inside the filter.
5. `bun run lint` / formatter → Main's, once.
6. **On the real app (hesperia), the smoke test this story is actually about. I have not performed
   any of this — it needs a build on the Mac, and nothing below is a check I have run.**
   1. Open Notes. **Three** fold controls are visible, not one: the app sidebar's, the scope rail's
      and the note list's.
   2. Fold the scope rail. It becomes a 48px strip carrying one button. The list and the panels take
      the width. Press the button — the rail comes back, and **the SPACES/TAGS/FILES sections are
      folded exactly as they were**, not reset.
   3. Drag the seam between the rail and the list. The rail follows the cursor and stops at 180px;
      it does not go to 72. Press `Home` on the seam — back to 240.
   4. Drag the rail to 400px, fold it, unfold it. **It is 400px, not 240 and not 48.** This is the
      one that would fail silently if the interaction rule were wrong.
   5. Switch to Chats and back to Notes, then quit and relaunch keeper. Every fold and every width
      is where it was left.
   6. Open Files. The tree is a fixed 360px column with a seam, not half the window. Fold it — the
      panel strip takes the whole surface and the tree's strip is still there to bring it back.
   7. Narrow the window to about 1000px. The **app sidebar's** fold control disappears (the viewport
      has forced it, unchanged behaviour) while the **column** fold controls stay.
   8. On a phone-width window (below 768px), the chat list has neither a fold control nor a seam.
