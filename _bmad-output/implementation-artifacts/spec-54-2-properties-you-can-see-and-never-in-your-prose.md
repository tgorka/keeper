# Spec 54.2 — Properties you can see, and never in your prose

story: 54.2
status: review
branch: `work/epic-54-properties-stay` (on top of `work/epic-54-card-follows`)
baseline_revision: 9a95acf
final_revision: ''
binds: FR-325, FR-326, FR-327; spec-52-3's request (restored), spec-53-3 (two clauses overridden), AD-102 (untouched)
sentinel: `MUT54-2`

<intent-contract>

**The ask, verbatim.** *"note folding - dziala folding i unfolding tekstu ale mialo
byc i z czescia properties ktorej teraz brakuje (za to w notes jest prefix z
properisami ktorego nie chcialem w notes)"*

**Both halves are one defect.** `file-frame-fold.ts:61` returns
`{ properties: true, caveat: true }`, so on every fresh install the properties fold
is CLOSED. Two consequences:

1. The grid he had on screen — `id`, `created`, `tags` — is gone, and the only thing
   left is an unlabelled 32px `SlidersHorizontal` ghost icon in the merged bar next
   to Save (`text-file-frame.tsx:711-722`). The region is **unmounted**, not hidden
   (`:830-841`), so there is no residue: no count, no chevron, no summary. Nothing on
   screen says properties exist.
2. Folded ⇒ `FileProperties` never mounts ⇒ `formBlock` stays `null` ⇒
   `frontmatterInForm={propertiesOpen ? formBlock : null}` (`:889`) hands the panes
   nothing ⇒ `raw-rendered-view.tsx:723` hides nothing ⇒ the Note tab draws
   `---\ntitle: …\n---` as the first lines of his document. That is the "prefix z
   properties", and it is the exact thing story 52.3 was asked for and delivered.

**Two clauses of spec 53.3 are overridden here, and both are marked SUPERSEDED at
the clause in that file** —
`spec-53-3-one-title-bar-and-two-folds.md:69-79` (the Never list),
`:97` (acceptance row 1) and `:129-143` (the Design Notes paragraph the second
clause lives in). Marked at the clause and not in a footnote, because the reader
this protects is one who has scrolled to the clause and stopped there — which is
exactly how 53.3's own antecedent drift produced this regression.
- *"Never default the properties panel open on the file surface … the notes surface
  defaults closed"* — the symmetry does not exist. On a note the frontmatter is a
  separate store field (`notes-editor.ts:16-19`) so a closed panel hides nothing; on
  a file the buffer IS the whole file, so a closed form puts YAML in the prose.
  `raw-rendered-view.tsx:199-201` already says this.
- *"with no form on screen the document has to draw the `---` block or a file's
  `tags:` would be on screen nowhere at all"* — answered by the fold CONTROL, which
  is named Properties. **Folded is not absent.**

**Always**
- The properties band is **open by default** on the file surface. He asked for an
  option to fold; an option to fold presumes the thing is there.
- The control reads as a disclosure and not as another toolbar verb: a visible name
  or the chevron pair the caveat fold already uses (`text-file-frame.tsx:797-801`),
  so a reader can tell properties exist. This costs **zero** vertical pixels — the
  header row is a fixed 40px.
- **A folded form still hides the block.** Folding is a display choice about the
  form, never an instruction to put frontmatter into the document. The frame knows
  the block whether or not the form is on screen.
- A user's own fold answer still wins and still survives a restart, through the same
  cookie and the same keys.
- The caveat band's default is untouched: AD-102's narrowing stays exactly as story
  53.3 shipped it.

**Block if**
- The file has no properties address (`propertiesPanel === null`): no control, no
  band, and nothing hidden from the panes — unchanged.

**Never**
- Never put the `---` block into the reader's prose because a form is folded.
- Never paraphrase or truncate the caveat: AD-102 is not in scope here.
- Never make the fold per-file. It is a standing preference, as shipped.

</intent-contract>

## Code Map

| where | change |
|---|---|
| `src/lib/stores/file-frame-fold.ts:53-72` | `properties` defaults OPEN; the docstring's notes-surface analogy is replaced by the asymmetry that disproves it |
| `src/components/viewers/text-file-frame.tsx:830-841` | the form's block is known while folded — keep it mounted and hidden, or hoist the frontmatter read out of the mount; whichever is chosen, say why in Design Notes |
| `src/components/viewers/text-file-frame.tsx:889` | `frontmatterInForm` no longer depends on the fold |
| `src/components/viewers/text-file-frame.tsx:711-722` | the control becomes a disclosure a reader can recognise |
| `src/components/viewers/text-file-frame.test.tsx:896` | re-anchored: fold→block-returns is the bug; the fold itself is still right |
| `src/components/viewers/text-file-frame.test.tsx:723`, `text-file-viewer.test.tsx:1559` | the two suites 53.3 disarmed with a `beforeEach` that arranges the fold open — restored to test the default a fresh install actually has |
| `src/components/viewers/text-file-viewer.test.tsx:1317-1326` | a THIRD disarm, found while doing the two: the Story 52.2 rename block carried the same `hydrateFileFrameFold({properties: false})`. Restored the same way — the rename fields are inside the form, and the form is on screen by default now |
| `src/components/viewers/text-file-viewer.test.tsx:1557` | the 52.3 block also clears the fold cookie between tests, because its new folded case presses the disclosure and a fold is persisted — one test's press must not be the next one's arrangement |
| `src/lib/stores/file-frame-fold.test.ts:65-68` and four fixtures below it | re-anchored to the new default, and each fixture flipped so it still differs from the default it is testing against |
| new tests | `text-file-frame.test.tsx`: *"names itself on the bar, in a row whose height it does not change"*, *"keeps hiding the block from a form the reader folded away by hand"*; `text-file-viewer.test.tsx`: *"keeps the block out of the Note tab after the reader folds the form away"* |

**Added by the code review.**

| where | change |
|---|---|
| `_bmad-output/implementation-artifacts/spec-53-3-one-title-bar-and-two-folds.md:69-79,97,129-143` | **P1.** The two overridden clauses were never actually marked, while this spec, the commit message and `epic-54:44` all said they were. Each is now struck through AT THE CLAUSE with a `SUPERSEDED by story 54.2` line and its reason — the notes/file asymmetry for the first, "folded is not absent, the control names it" for the second. `:214-219` also notes that 53.3's 81px is now a gain a reader collects by pressing rather than on arrival |
| `src/components/viewers/text-file-frame.tsx:216-283` | **P2.** `FILE_ACTIONS_NARROW_PX`, `PROPERTIES_WORD_PX` and `PROPERTIES_WORD_BUDGET_PX`, with the arithmetic and the rejected alternatives in the docstring |
| `src/components/viewers/text-file-frame.tsx:788,841-849` | **P2.** `actions` becomes `PaneHeader`'s render-prop form, and the word is `sr-only` below the budget. The comment at the old `:743` that claimed group 1 could absorb the width is replaced by what group 1 actually does |
| `src/components/viewers/text-file-viewer.test.tsx:1313-1330` | **P3.** The 52.2 rename block clears `FILE_FRAME_FOLD_COOKIE` as well as resetting the store, so its open form is arranged rather than inherited |
| `src/components/viewers/text-file-frame.test.tsx:237-253` | `glyphsOf`, the repo's lucide-class idiom (`files-pane.test.tsx:1481-1497`), so the chevron's DIRECTION is asserted rather than the svg count |
| new tests | `text-file-frame.test.tsx`: *"hides its word rather than the file name when the row cannot afford both"*; and the chevron direction added to *"names itself on the bar…"* and to *"folds the form away and back…"* |

## Tasks & Acceptance

| # | acceptance |
|---|---|
| 1 | a fresh install shows the properties grid on a savable markdown file, with no cookie and no press |
| 2 | the Note tab shows the document's prose and NOT the `---` block, on a fresh install |
| 3 | with the properties band folded BY HAND, the Note tab still shows no `---` block |
| 4 | the Source tab still shows every byte, and a save still writes the whole file — byte-compared |
| 5 | the control is recognisable as a disclosure and names itself visibly; the header's height is unchanged |
| 6 | a folded answer survives a remount and a restart |
| 7 | the caveat band's default and AD-102's short form are untouched |
| 8 | the two disarmed suites test the real default again, and `text-file-frame.test.tsx:896` is re-anchored with a comment naming this story |
| 9 | the vertical cost of defaulting open is stated from a real measurement, against the 153px story 53.3 claimed |

## Design Notes

**The default is one line and it is not the fix.** `file-frame-fold.ts:83`
returns `{ properties: false, caveat: true }`. On its own that puts the grid back
on screen and takes the `---` block out of the Note tab — and the block comes
straight back the first time he uses the fold he asked for. Which is why the rest
of this story is about the second half.

**Option (a): the form stays MOUNTED and is hidden while folded.**
`text-file-frame.tsx:900-915` renders `<div id={propertiesRegionId}
className="shrink-0" hidden={!propertiesOpen}>` — the `hidden` attribute, the way
`sidebar-group.tsx:215` folds a section body and `tag-combobox.tsx:388` folds its
listbox: `display: none`, out of the tab order, out of the accessibility tree, no
height in the column. `text-file-frame.tsx:962` is then
`frontmatterInForm={formBlock}`, with no reference to the fold.

*What it costs.* One `sync_read_frontmatter` per file while the form is folded —
the panel's own read (`properties-panel.tsx:955-975`), which already runs for
every file whose form is open. That read is not overhead here, it is the
requirement: this frame cannot hide bytes it does not know, so SOMETHING has to
read the block whether the form is on screen or not. Plus one
`recording_note_targets` for a block that carries a session id
(`properties-panel.tsx:621-641`). Nothing else: the tag vocabulary is fetched
only when somebody presses Add a tag (`properties-panel.tsx:1302-1310`), and the
key-column width is a store selector, not IPC.

*Why not (b), hoisting the read into the frame.* The frame would call
`syncReadFrontmatter` itself and the panel would go on calling its own — two
reads per open file, on a reader who is on a pendrive as often as not — unless
`FileProperties` grew a `block` prop, which is a new contract on a component with
three hosts and a second definition of "the block this form is holding" for the
two to disagree over. Either shape also needs its own failure path for a refused
read, in step with the one the panel already has, and 52.3's whole lesson is that
two recognisers over one file is how a form and a pane come to disagree. (b) buys
one avoided IPC on a recording note, folded. It is not worth a second source of
truth.

**The control is a disclosure now** (`text-file-frame.tsx:750-767`). It was a
32px `icon-sm` ghost carrying `SlidersHorizontal` and an `aria-label`, sitting
next to Save, identical whether the file had three properties or none — with the
region unmounted, nothing on screen said properties existed. It now carries the
glyph (the app's spelling of Properties, `note-editor.tsx:794`), the visible word
`PROPERTIES_LABEL` — which is also its accessible name, by its own text, so there
is no second copy to drift and no `title` repeating a word already on screen —
and the `ChevronDown`/`ChevronRight` pair the caveat fold uses ten lines below.
`aria-controls` is unconditional now, because the region exists in both states.

**Zero vertical pixels for that.** `PaneHeader` is a fixed 40px row
(`pane-header.tsx:323`, `h-10`) and the control is `size="xs"` → `h-6`, the same
height as the Save button beside it.

**HORIZONTALLY it costs the actions budget, and the first draft of this story got
that backwards.** The sentence that stood here said the width came out of group 1
because group 1 is the only member of the row allowed to give ground. Group 1 is
the only member allowed to give ground and it is not able to PAY: `flex-1` off a
zero basis (`pane-header.tsx:328`) contributes nothing to the row's own content
width, so in a shortage it does not shrink proportionally — it simply sits at 0px.
The file name disappears first and the row then overflows past group 4's fold and
close, which is the 46.5 defect `pane-header.tsx:29-33` exists to refuse. Nothing
else in the row can absorb it either: every `Button` carries `shrink-0
whitespace-nowrap` in `buttonVariants`' base (`button.tsx:28`), group 2's width is
a constant by construction and group 4 is `shrink-0` on purpose.

*The decision, and why this one.* The word is spent out of the ACTIONS BUDGET:
`actions` is `PaneHeader`'s render-prop form now (`text-file-frame.tsx:788`), and
below `PROPERTIES_WORD_BUDGET_PX` (164 = `FILE_ACTIONS_NARROW_PX` 98 + the word's
66, `text-file-frame.tsx:216-283`) the word renders `sr-only` — absolutely
positioned, so out of the row's content width and out of its `gap-1`, and still in
the accessibility tree, so the control keeps ONE accessible name, its own text, at
every width (`:841-849`). The glyph and the chevron are the affordance that
survives, and `aria-expanded` still says which state the fold is in.

Three candidates were considered and two rejected:
- **`hidden sm:inline`** — rejected, and not merely as coarser. This repo has no
  such idiom to reach for (`sm:` appears only on dialog `max-w`s), and the
  instrument is wrong: `sm:` asks about the WINDOW, while a strip of four panels in
  a 1600px window satisfies every viewport breakpoint there is and still gives each
  panel 400px. The panel is what this row is inside.
- **drop the word, keep the chevron pair** — rejected: the invisible affordance is
  half of what the owner reported, and an `aria-label` no eye reads is what this
  story replaced.
- **give the name a truncating container that yields** — rejected because it cannot
  work: the name already truncates (`text-file-frame.tsx:542-546`) and its problem
  is not truncation but a zero basis, and giving group 1 a `min-w-*` would make the
  OVERFLOW worse rather than better.

Honouring the budget keeps both halves at once: `paneHeaderActionsBudget` already
reserves `PANE_HEADER_IDENTITY_MIN_PX` (160) for the name and charges groups 2 and
4 in full, so a group 3 that stays inside its budget cannot push anything off the
right edge. A budget of zero — a machine that never delivers a `ResizeObserver`
observation — renders the narrow shape, which is `PriorityActions`' own safe
direction.

**One benign residue, since this story claimed "nothing beyond that".** Open the
Add-a-tag chooser and then fold the form: `TagCombobox` stays mounted with its
capture-phase listeners live (`tag-combobox.tsx:187-203`, `:249`), because `hidden`
on the region unmounts nothing. It heals itself on the reader's next click.
`outside` (`:182-185`) closes on `click`, `auxclick` or `contextmenu` unless the
target is inside `root`, and a root inside a `display: none` subtree contains
nothing anywhere else on the page — so the first click that lands anywhere calls
`setOpen(false)`, and the effect's own cleanup removes all six listeners. Escape is
harmless in the meantime for the mirror-image reason: `claim` (`:244-248`) only
`preventDefault`s for a target INSIDE `root`, and a hidden root can hold neither
focus nor a target, so the Radix layer around it dismisses normally rather than
being vetoed by a list nobody can see. Recorded rather than fixed: the fix would be
unmounting the region, which is the thing this story exists to stop.

**Untouched:** AD-102's band, its default, and its short form. The caveat's
describe in `text-file-frame.test.tsx` is unchanged and green.

## Verification

### Commands

| command | result |
|---|---|
| `npx tsc --noEmit` | clean |
| `npx vitest run src/components/viewers/text-file-frame.test.tsx src/components/viewers/text-file-viewer.test.tsx src/components/viewers/raw-rendered-view.test.tsx src/lib/stores/file-frame-fold.test.ts` | **156 passed** (41 + 48 + 59 + 8) |
| `npx vitest run src/components/viewers src/components/layout/panel-strip.test.tsx src/components/notes/file-properties.test.tsx` | **398 passed** (14 files) — the neighbours a changed default reaches |
| `npx biome check` on the five touched source files | clean |

**After the code review (P1/P2/P3 and two coverage gaps):**

| command | result |
|---|---|
| `npx tsc --noEmit` | clean |
| `npx vitest run src/components/viewers/text-file-frame.test.tsx src/components/viewers/text-file-viewer.test.tsx src/lib/stores/file-frame-fold.test.ts` | **98 passed** (42 + 49 + 7) |
| `npx vitest run src/components/viewers src/components/layout/pane-header.test.tsx` | **363 passed** (13 files) — the neighbours the render-prop `actions` reaches, `PaneHeader`'s own suite included |
| `npx biome check` on `text-file-frame.tsx`, `text-file-frame.test.tsx`, `text-file-viewer.test.tsx` | clean |

### `MUT54-2` — the review's three fixes fail without themselves

Each mutation applied in place, the suite run, then restored, and the restore
verified by reading the file back rather than by remembering what was typed.

| mutation | result |
|---|---|
| D: the word always visible — `<span>{PROPERTIES_LABEL}</span>` with no budget test | **1 failed** — *"hides its word rather than the file name when the row cannot afford both"* |
| E: `ChevronDown` and `ChevronRight` swapped | **2 failed** — *"names itself on the bar, in a row whose height it does not change"* and *"folds the form away and back, and never hands the block to the prose"*. Both passed on the pre-review build, which only counted two svgs |
| F: the 52.2 block's `beforeEach` back to `resetFileFrameFoldForTest` alone, with one probe describe above it persisting `{ properties: true }` the way a test that presses the disclosure does | **5 failed** — every test in the rename block, each on a 5s timeout, because the rename fields live inside a form the jar had folded away. With the cookie clear restored and the probe still in place: **49 passed**. The probe was then removed |

### `MUT54-2` — the tests fail without the fix

Each change reverted in place, the suites run, then restored; the restore
verified by reading `git diff` rather than by remembering what was typed.

| mutation | result |
|---|---|
| A: `frontmatterInForm={propertiesOpen ? formBlock : null}` — re-couple the seam to the fold | **3 failed** — *"keeps hiding the block from a form the reader folded away by hand"*, *"folds the form away and back, and never hands the block to the prose"*, *"keeps the block out of the Note tab after the reader folds the form away"*. This is the mutation that matters: the default alone leaves all three green |
| B: drop `hidden={!propertiesOpen}` back to an unmounted region | **6 failed**, the three above plus *"are offered for a writable markdown file…"*, *"survives a remount…"*, *"comes up folded when the cookie the last run left says so"* |
| C: `fileFrameFolded()` back to `{ properties: true, caveat: true }` | **16 failed** across all three suites, including the three store fixtures and every one of the tests whose 53.3 `beforeEach` this story deleted — which is the disarm, demonstrated: with the arrangement gone, the shipped default cannot pass them |

### The nine rows

| # | where it is proven |
|---|---|
| 1 | `text-file-frame.test.tsx` — *"are offered for a writable markdown file the surface can address"*: no cookie, no press, `aria-expanded="true"` and the region on screen |
| 2 | `text-file-viewer.test.tsx` — *"keeps the block out of Note mode and puts it back in the bytes a save writes"*, now running at the real default with its `beforeEach` arrangement deleted |
| 3 | `text-file-frame.test.tsx` — *"keeps hiding the block from a form the reader folded away by hand"*; and end to end in `text-file-viewer.test.tsx` — *"keeps the block out of the Note tab after the reader folds the form away"* |
| 4 | `text-file-viewer.test.tsx` — *"still shows every byte on the Source tab"*, and the folded case above asserts `syncWriteEntry` was called with `BLOCK + BODY + "beta\n"`, byte for byte |
| 5 | `text-file-frame.test.tsx` — *"names itself on the bar, in a row whose height it does not change"*: the visible word, no `aria-label`, and the glyph pair asserted BY NAME (`lucide-sliders-horizontal`, then `lucide-chevron-down` open / `lucide-chevron-right` folded — also in *"folds the form away and back…"*, so the direction is pinned in both states). **The height half of this row is a className check and NOT a measurement**: `expect(bar()?.className).toContain("h-10")` and `expect(control.className).toContain("h-6")`, with no Tailwind loaded in the run and no layout in jsdom, so what they prove is that the classes the heights come from are still spelled on the right elements. A build where the row grew a wrapper with padding of its own would pass. The 40px and the 24px are owed as a real measurement on hesperia — see the human list below |
| 6 | `text-file-frame.test.tsx` — *"survives a remount, because the frame outlives the file it shows"* and *"comes up folded when the cookie the last run left says so"*, both now arranging the FOLDED answer, because open is the default and a test that pressed to open would pass on a build that remembered nothing |
| 7 | the caveat describe is untouched and green; `fileFrameFolded().caveat` is still `true` |
| 8 | the two `beforeEach` disarms are gone (`text-file-frame.test.tsx`'s 52.3 describe, `text-file-viewer.test.tsx`'s 52.3 and 52.2 describes — the third was the same disarm, in the rename block); `text-file-frame.test.tsx`'s fold test is re-anchored with a docblock naming this story. **Corrected by the review (P3):** deleting the 52.2 block's arrangement left it with `resetFileFrameFoldForTest` alone, which resets the module preference but not the jar the hydrate reads — so the open form it depends on was a fact about what no earlier test in the file happens to persist, not an arrangement. It now clears `FILE_FRAME_FOLD_COOKIE` exactly as the 52.3 block below it does (`text-file-viewer.test.tsx:1313-1330`), and mutation F proves what that buys |
| 9 | measured below |
| P2 (review) | `text-file-frame.test.tsx` — *"hides its word rather than the file name when the row cannot afford both"*: the quick-capture 560px row affords the word, a four-panel strip's 360px row does not, the file name and the frame group survive both, the accessible name is the same string in both, and the threshold is `PROPERTIES_WORD_BUDGET_PX` either side of one pixel. Geometry DECLARED with `withActionWidths({ status: 96, frame: 56 })` and delivered with `withHandFiredResize`, the `note-editor.test.tsx` idiom — **without those, `src/test/setup.ts` answers one whole 1024px viewport for every zero-sized element and charges the budget 1024px twice, which is how the first draft of this test failed for the wrong reason.** What it does NOT prove: that 164px is what `Properties` beside `Save` really measures, that 96 and 56 are the real slot widths, or that the row stops overflowing on a screen |

### Row 9 — what defaulting open costs, measured

Read off the class chain a real `TextFileFrame` render produced (a throwaway
vitest harness that dumped `outerHTML`, then deleted), for a file whose block is
`title`, `id`, `created`, `tags` — the shape of the document he was looking at.
Tailwind v4 defaults: `--spacing` is not overridden, so `1` is 4px, and
`--text-xs` is not redefined in `index.css`, so it is 12px/16px. `--text-meta` IS
defined there: 11px × 1.4 = 15.4px.

Rendered chain:
`section.flex.flex-col.gap-1.border-b.px-3.py-2.text-xs` → the property grid
(`items-start gap-y-1`, four rows) → the add-a-property row
(`flex items-center gap-2 pt-1`).

    py-2 top                                        8
    row `title`   — Input h-7                      28
    row `id`      — read-only value, text-meta     15.4
    row `created` — Input h-7                      28
    row `tags`    — chip (16 + 2 + 2) vs Button
                    h-6 → max is 24                24
    gap-y-1 × 3                                    12
    gap-1 before the add row                        4
    add row: pt-1 (4) + Button sm h-8 (32)         36
    py-2 bottom                                     8
    border-b                                        1
    -------------------------------------------------
                                                  164.4px

The same arithmetic over a file with no frontmatter — one offered `tags` row —
gives 8 + 24 + 4 + 36 + 8 + 1 = **81px**, which is story 53.3's own figure to the
pixel. That is the check on the method, not a second claim.

**Against the 153px 53.3 claimed.** That 153 was 40 (the merged title row) + 81
(the properties band, folded, for a file with no frontmatter) + 32 (the caveat's
four lines down to two). Two of those three are untouched here. What this story
gives back is the properties band — and it gives it back to where it was BEFORE
53.3, which mounted that band unconditionally. So:

| file | 53.3's gain | after 54.2 |
|---|---|---|
| no frontmatter, unmanaged, 4-line caveat | 153px | **72px** (40 + 32) |
| no frontmatter, keeper-managed (no caveat band) | 121px | **40px** |
| his file — 4 properties, unmanaged | 153px | **72px**, and the band is 164.4px rather than 81px, which is 83.4px more than 53.3's floor case ever measured |

And the fold still reclaims every one of those pixels on request — the difference
being that it no longer charges for them in `---` lines through the middle of his
prose.

### What jsdom cannot see, and what a human must check on the Mac

jsdom performs no layout and this container has no Chromium, so nothing above is a
measured pixel: every number is summed from the class chain a real render emitted.
Five things need eyes on hesperia.

1. **The two width constants, measured.** `FILE_ACTIONS_NARROW_PX` (98) and
   `PROPERTIES_WORD_PX` (66) are arithmetic over Tailwind's scale plus an estimate
   of two strings in Inter at 12px [INFERENCE — text width is a font's business].
   What to measure: the actions group's `getBoundingClientRect().width` with the
   word hidden, and the width the word itself adds. If 98 + 66 is generous the word
   appears a few pixels of panel width later than it could, which is the harmless
   direction; if either is UNDER the real figure, the overflow this fix exists to
   stop comes back and the constants must go up.
2. **That the row stops overflowing, in the two cases the test names.** A 560px
   quick-capture window with the file open, and a strip of four panels: the file's
   name and the panel's close control must both be on screen and inside the row in
   both, and the word may only be present where the row can pay for it. This is the
   claim the jsdom test can only make about the DECISION, never about the pixels.
3. **The row's real height, and the control's**, against `h-10` = 40px including
   the hairline and `h-6` = 24px. The suite asserts the class strings and says so;
   a wrapper with padding of its own would pass them and change the band.
4. **The band's real height** for a file with several properties, against the
   164.4px above.
5. **That the grid and the Note tab agree** on his actual `about`-style file: the
   grid showing `id`, `created`, `tags`, and the first line of the Note tab being
   his heading rather than `---`.

No test here pretends to cover any of the five.
