# Spec 54.2 — Properties you can see, and never in your prose

story: 54.2
status: in-progress
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

**Two clauses of spec 53.3 are overridden here, deliberately.**
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
height as the Save button beside it. The width it takes comes out of group 1,
which is the only member of the row allowed to give ground.

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
| 5 | `text-file-frame.test.tsx` — *"names itself on the bar, in a row whose height it does not change"*: the visible word, no `aria-label`, two glyphs, `h-10` on the row and `h-6` on the control |
| 6 | `text-file-frame.test.tsx` — *"survives a remount, because the frame outlives the file it shows"* and *"comes up folded when the cookie the last run left says so"*, both now arranging the FOLDED answer, because open is the default and a test that pressed to open would pass on a build that remembered nothing |
| 7 | the caveat describe is untouched and green; `fileFrameFolded().caveat` is still `true` |
| 8 | the two `beforeEach` disarms are gone (`text-file-frame.test.tsx`'s 52.3 describe, `text-file-viewer.test.tsx`'s 52.3 and 52.2 describes — the third was the same disarm, in the rename block); `text-file-frame.test.tsx`'s fold test is re-anchored with a docblock naming this story |
| 9 | measured below |

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

jsdom performs no layout, so nothing above is a measured pixel: every number is
summed from the class chain a real render emitted. Three things need eyes on
hesperia:

1. **The control's width.** The button went from a 32px square to glyph + word +
   chevron. `px-2` + 12 + `gap-1` + the word at 12px + `gap-1` + 12 puts it near
   106px [INFERENCE — text width is a font's business and this box has no
   Chromium]. In a narrow panel that width comes out of the file name, which
   truncates. Worth looking at in a 560px quick-capture window and in a strip of
   four panels.
2. **The band's real height** for a file with several properties, against the
   164.4px above.
3. **That the grid and the Note tab agree** on his actual `about`-style file: the
   grid showing `id`, `created`, `tags`, and the first line of the Note tab being
   his heading rather than `---`.

No test here pretends to cover any of the three.
