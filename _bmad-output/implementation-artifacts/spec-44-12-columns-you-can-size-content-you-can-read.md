---
title: 'Story 44.12: Columns You Can Size, Content You Can Read'
type: 'feature'
created: '2026-08-09'
status: 'review'
blocking_condition: ''
baseline_revision: 'f782acc'
final_revision: ''
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/epic-44-the-vocabulary-is-the-space.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-43-8-the-files-tab.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-42-4-the-note-stub.md'
---

<intent-contract>

## Intent

**Problem:** every list in keeper is a fixed-width guess. The Properties panel is where the owner
met it: the key column is `w-32`, chosen once by somebody who could not see their vault, and a value
that does not fit is cut with nowhere to read the rest. The only thing keeper offered was
`title={value}` — a tooltip that does not exist for a keyboard, does not exist for a touch screen,
cannot be scrolled, and appears after a delay nobody chose. The same panel also previewed an
unparseable block with `frontmatter.slice(0, 400)`, which counts UTF-16 code units and therefore
emits half an emoji whenever the 400th unit lands inside one.

**Approach:** AD-83's three steps, in AD-83's order, each done by whoever can actually do it.

1. **Fit** is the layout engine's job and nobody else's. An unsized column is
   `minmax(72px, fit-content(50%))`: exactly as wide as its own glyphs in the real font, capped at
   half the pane. Measuring text in TypeScript to compute the same number would be a second, worse
   font metric, and the reason the column is a guess today is that somebody wrote a number.
2. **Resize** is a `role="separator"` in the tab order, a pointer handler and a custom property.
   Thirty lines. The width is written to `document.cookie` — the store `SidebarProvider` already
   keeps `sidebar_state` in — so it survives a reload without inventing an IPC command for a fact
   Rust has no use for, and without `localStorage`, which this codebase refuses out loud.
3. **Truncate, then offer the rest.** CSS truncates; the value that was truncated becomes the button
   that opens the complete value in a scrollable popover. The affordance renders **only** when
   `scrollWidth > clientWidth` says the text really was cut, because an affordance on every value is
   a tab stop on every value.

The one place keeper truncates in JavaScript rather than in CSS — the unparsed-block preview — now
counts graphemes.

## Boundaries & Constraints

**Always:**
- **No new dependency.** A resizable column is `clientX - startX` and a CSS variable. Nothing was
  added to `package.json`; the popover is the `radix-ui` one already vendored in
  `src/components/ui/popover.tsx`.
- **FR-145 / AD-65 untouched.** The only path this story renders is the profile-relative text the
  note already carried. `RecordingPath`'s visible text and its dropdown's arguments are byte-for-byte
  what 42.4 shipped; the popover shows the same relative string, never `absolutePath`.
- **The affordance is a control, not a hover.** A real `<button>`, in the tab order, opening a real
  `role="dialog"` whose scroll region is itself focusable. `title=` is gone from the surfaces this
  story owns.
- **Fitting is asked for, never computed.** `columnTemplate` emits `fit-content(50%)`; no code here
  measures a glyph.
- **A width is clamped on the way in and on the way out.** `[72, 640]`. A column dragged to zero has
  swallowed its own content *and* the handle that would bring it back.
- **Home is the door out.** It clears the width AND the cookie entry — otherwise the next launch
  restores the width somebody just escaped from.
- **Roving tab order is respected where the surface has one.** The Files tree's overflow trigger
  takes the row's `actionTabIndex`, so a folder of two hundred long names is not two hundred Tab
  presses (43.8's rule, applied to the new control).
- **The Files pane stays read-only (AD-75).** Asserted by name against
  `FILES_WRITE_CONTROL_LABELS` with the new control present.

**Never:**
- No second truncation rule, no second persistence store, no second popover primitive.
- No `localStorage`, and no new IPC command or settings row for a view preference.
- No measurement of text in TypeScript to decide a column width.
- No affordance rendered on a value that fits.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| First open, no cookie | any note with properties | `--keeper-columns: minmax(72px, fit-content(50%)) 0px minmax(0, 1fr)` | none |
| Drag right | seam at 160, pointer 160 → 260 | `260px 0px minmax(0, 1fr)`; cookie carries `properties-key:260` | none |
| Drag, then remount | the above, unmounted and rendered again | still `260px` — the cookie is read in a lazy initialiser, never cached | none |
| Drag left past the floor | pointer to −600 | `72px` — clamped, not zero | none |
| Drag right past the ceiling | pointer beyond 640 | `640px` | none |
| Pointer that never grabbed | `pointermove` with no prior `pointerdown` | nothing moves | none |
| Second pointer mid-drag | a different `pointerId` moves | ignored; the grabbing pointer still owns the seam | none |
| Non-primary button | `pointerdown` with `button: 2` | no drag starts | none |
| `ArrowRight` / `ArrowLeft` | seam focused | ±8 px | none |
| Shift + arrow | seam focused | ±32 px | none |
| `Home` / double-click | a sized column | back to fitted; the cookie entry is removed | none |
| Screen reader on the seam | fitted | `aria-valuetext="Fitted to content"`, no `aria-valuenow` | none |
| Screen reader on the seam | sized | `aria-valuenow` = the px width, `aria-valuemin/max` = 72/640 | none |
| Two surfaces, one cookie | Properties dragged after Files | both widths survive; neither overwrites the other | none |
| Cookie from an older build | `properties-key:180\|garbage\|:12\|Bad Id:9\|x:notanumber` | the one good entry is read, the rest dropped | none |
| Cookie out of range | `properties-key:9999` | clamped to 640 on read | none |
| No cookie at all / another origin's jar | `sidebar_state=true; theme=dark` | `{}`, column fits | none |
| Value fits | ULID in a 1000 px column | plain `span`, no button, no tab stop | none |
| Value does not fit | ULID in a 120 px column | the value itself becomes the trigger | none |
| Overflow affordance opened | click, or Enter on the focused button | `role="dialog"` holding the COMPLETE value | none |
| A value taller than the panel | any | region is `max-h-64 overflow-y-auto` and `tabindex="0"` | none |
| Escape in the panel | open | closes, focus returns to the trigger | none |
| Recording path | `recording:` / `files:` in a session note | shows the relative path; the affordance shows the same relative path | none |
| Recording path, no `title` | any | no element in the panel carries a `title` attribute | none |
| File tree, long name | 67 chars in a 200 px row | one trigger, on that row only | none |
| File tree, short name | `ok.md` | no trigger | none |
| File tree, unfocused row | any | the trigger is `tabindex="-1"` | none |
| File tree, focused row | row focused | the trigger is `tabindex="0"` | none |
| File tree, folder row | a folder whose name overflows | the trigger is a SIBLING of the toggle button, never inside it | none |
| Notes list, ≤3 tags | `["work","q3"]` | no `+n` at all | none |
| Notes list, >3 tags | 5 tags | `+2`, named `More tags on this note: invoices, urgent` | none |
| `+n` opened | click | the hidden tags, each still a filter chip | none |
| `+n` chip clicked | a tag in the panel | `onToggleTag(tag)` fires; the note does NOT open | none |
| Unparsed block, short | under 400 graphemes | shown whole, no ellipsis, no affordance | none |
| Unparsed block, long | 500 emoji after an odd-length prefix | 400 graphemes + `…`, no lone surrogate, no `` | none |
| Unparsed block, the rest | the same | one control opens the complete block | none |
| Grapheme cases | ZWJ family, regional-indicator flag, base + combining mark | never split; `slice` splits all three | none |
| Empty frontmatter | a note with no properties | no grid and no seam — there is no boundary to drag | none |

</intent-contract>

## Code Map

- `src/lib/column-widths.ts` (new) — `COLUMN_WIDTH_COOKIE`, `MIN/MAX_COLUMN_WIDTH`,
  `COLUMN_KEY_STEP(_COARSE)`, `clampColumnWidth`, `readColumnWidths`, `columnWidthCookie`,
  `columnTemplate`. Pure; takes the cookie string as an argument.
- `src/lib/truncate.ts` (new) — `ELLIPSIS`, `truncateGraphemes`.
- `src/components/ui/resizable-columns.tsx` (new) — `useResizableColumn`, `ColumnResizer`,
  `COLUMN_GRID_CLASS`, `COLUMN_TEMPLATE_VAR`, `COLUMN_ROWS_VAR`, `COLUMN_RESIZER_LABEL`,
  `COLUMN_FITTED_VALUE_TEXT`.
- `src/components/ui/overflow-value.tsx` (new) — `useOverflowing`, `OverflowValue`,
  `FullValueButton`, `OVERFLOW_TRIGGER_LABEL`, `OVERFLOW_PANEL_LABEL`.
- `src/components/notes/properties-panel.tsx` — the two-column grid and its seam; the `id` value,
  the recording paths and the unparsed preview get the affordance; `title=` removed;
  `slice(0, 400)` → `truncateGraphemes`.
- `src/components/layout/files-pane.tsx` — `RowName` (render prop) + `FILES_NAME_LABEL`.
- `src/components/notes/note-row.tsx` — `+n` becomes a control that names and opens the hidden tags;
  `NOTE_MORE_TAGS_LABEL`.
- `src/test/layout.ts` (new, not collected as a suite) — `withTextLayout`, `withRect`, `TEST_CHAR_PX`.
- Tests: `src/lib/column-widths.test.ts`, `src/lib/truncate.test.ts`, and new blocks in
  `properties-panel.test.tsx`, `files-pane.test.tsx`, `note-list.test.tsx`.

**Placement decisions.**

- **`src/components/ui/`, not a story-local file.** `biome.json` already carves that directory out
  for `noDocumentCookie` and for `a11y`, which is exactly what a cookie-backed
  `role="separator"` needs — and the sidebar's own cookie lives there. Putting keeper's second
  cookie-backed view preference anywhere else would mean two answers to one question.
- **The template travels as a CSS custom property**, not as `style.gridTemplateColumns`. Partly so a
  surface can override one declaration; mostly because jsdom's CSS parser silently DROPS
  `minmax(72px, fit-content(50%))`, and a value the test environment throws away is a value no test
  can defend. The first draft of this story asserted `style.gridTemplateColumns` and read `""`.
- **A render prop in `files-pane.tsx`**, not a row component. The truncating span has to live inside
  a folder's toggle button — clicking a folder's name is how you open it — while the trigger has to
  live outside it, because a button inside a button is not HTML and its click would toggle the
  folder on the way past. Extracting the whole row would have restructured a file W2 stories are
  about to edit, for no gain.

## Tasks & Acceptance

**Execution:**
- [x] Pure width codec, clamp and grid template.
- [x] Grapheme-safe truncation.
- [x] `useResizableColumn` + `ColumnResizer`: pointer capture, keyboard, `Home`, ARIA value text.
- [x] `useOverflowing` + `OverflowValue` / `FullValueButton`, with a scrollable focusable panel.
- [x] Properties panel: grid, seam, `id`, recording paths, unparsed preview.
- [x] Files pane: row-name overflow inside the roving tab order.
- [x] Notes list: `+n` names and opens the tags it hides.
- [x] A jsdom layout model, so the overflow shell is exercised rather than assumed.
- [x] Tests, and a revert proof for each central claim.

**Acceptance Criteria:**
- `bun run test src/components/notes/properties-panel.test.tsx` — **22 passed**.
- `bun run test src/components/layout/files-pane.test.tsx` — **26 passed**.
- `bun run test src/components/notes/note-list.test.tsx` — **8 passed**.
- `bun run test src/lib/column-widths.test.ts` — **7 passed**.
- `bun run test src/lib/truncate.test.ts` — **5 passed**.
- `bun run lint`, `bun run typecheck`, `cargo` anything, and any full suite — **not run here**, by
  instruction; the parent gates Linux and macOS.

## Verification

Every claim was proved by removing the implementation and watching the named test fail, then
restoring the file byte-for-byte and re-running it green.

| Reverted | Test that caught it | Observed failure |
|---|---|---|
| `fit-content(50%)` → a fixed `8rem` (the `w-32` this story deletes) | `fits the key column to its content until somebody says otherwise`, `asks the layout engine to fit until a width is chosen`, `forgets a width rather than recording a null…`, + 2 more | the fitted template became a number again |
| The `document.cookie` write in `onWidth` | `moves the boundary to where the pointer took it, and remembers` | the remount came back fitted — the drag was theatre |
| `startWidth + (clientX - startX)` → `startWidth` | `moves the boundary to where the pointer took it, and remembers`, `refuses to drag a column down to nothing` | the seam ignored the pointer entirely |
| The `pointerId` / no-drag guard on `pointermove` | `ignores a pointer that never grabbed the seam` | any pointer crossing the seam snapped the column to it |
| `clampColumnWidth`'s min/max | `refuses to drag a column down to nothing`, `never lets a column reach zero…`, `clamps a stored width that is out of range on the way back in` | the column went to `-600px`; a stored `9999` was adopted |
| `scrollWidth > clientWidth` → `false` (what an untreated jsdom always answers) | `offers the whole of a value the column cut`, `shows a recording path in full…`, both `FilesPane — a name too long` tests | the affordance never rendered — the exact defect class epic 43 shipped twice |
| `scrollWidth > clientWidth` → `true` | `says nothing extra about a value that fits`, `offers the whole name, and only for the name that did not fit`, + 11 pre-existing FilesPane tests | a tab stop on every value, and the tree's accessible structure changed underneath its own suite |
| The panel's `{value}` → `{value.slice(0, 12)}` | `offers the whole of a value the column cut`, `shows a recording path in full…`, `previews an unreadable block…`, `offers the whole name…` | the "full value" was a truncation of a truncation |
| `max-h-64 overflow-y-auto` on the scroll region | `shows a recording path in full, from a control and not a tooltip` | a value longer than the panel had no way down |
| `tabIndex={0}` on the scroll region | `shows a recording path in full, from a control and not a tooltip` | the region took focus but arrow keys could not scroll it |
| `tabIndex` passthrough on `FullValueButton` | `keeps the affordance out of the tab order until its row is the focused one` | every overflowing row became a Tab stop, undoing 43.8's roving order |
| `OverflowValue` → `<span title={relativePath}>` (the code this story replaces) | `shows a recording path in full, from a control and not a tooltip` | the tooltip came back and the control vanished |
| `truncateGraphemes`'s cluster join → `text.slice(0, limit)` | `never emits half a character`, `is what a naive slice is not`, `previews an unreadable block without cutting a character in half` | a lone high surrogate in the output |
| `truncateGraphemes(frontmatter, 400)` → `frontmatter.slice(0, 400)` at the call site | `previews an unreadable block without cutting a character in half` | the panel rendered `` where a thumb had been |
| The `+n` popover → an inert `<span>+{n}</span>` | `names the tags it is hiding rather than only counting them`, `opens the hidden tags, and each one still filters` | the row counted what it would not name |
| `stopPropagation` on the `+n` trigger | `opens the hidden tags, and each one still filters` | opening the panel also opened the note — the trigger sits inside the row's own button |
| `stopPropagation` on the panel and its chips | `opens the hidden tags, and each one still filters` | a click in the portal still reached the row through the React tree and opened the note |

**How fitting and overflow were tested despite jsdom reporting zero for every measurement.**

jsdom performs no layout: `scrollWidth`, `clientWidth` and every bounding rect are zero. Two
different techniques, and neither of them stubs keeper.

- **Overflow.** `src/test/layout.ts`'s `withTextLayout(px)` answers the two DOM properties the real
  hook reads — `scrollWidth` from the element's own `textContent`, `clientWidth` from the pane width
  the test chose — and it does so **only for elements carrying Tailwind's `truncate`**, which is
  precisely the set of elements that can produce an ellipsis. Nothing else on screen has its
  geometry redefined underneath it. The hook, its layout effect, the real `scrollWidth >
  clientWidth` comparison, the real conditional render, the real Radix popover and the real focus
  restoration all run. What is modelled is the browser, not the component. The pair of mutations
  above (force `false`, force `true`) is the proof that the measurement is actually load-bearing in
  the test and not decoration: forcing it either way breaks a different set of tests.
- **The drag.** `withRect(seam, 160)` pins one element's box, instance-level, leaving `setup.ts`'s
  suite-wide viewport shim intact for everything else. The drag is then driven with real
  `pointerdown`/`pointermove`/`pointerup` through the real handlers, and the assertion is the real
  custom property on the real grid container plus the real `document.cookie` — read back through
  `readColumnWidths`, and again after a real unmount and remount.

**What could NOT be proved here, stated plainly.**

- **That the CSS truncates at all.** The test environment loads no stylesheet, so
  `text-overflow: ellipsis` is never applied and the visible `…` is never painted. Every truncation
  assertion in a component test is an assertion about the contract that produces the ellipsis
  (`truncate` on the element, `overflow-hidden` on the track), not about the ellipsis. The one
  truncation that IS proved end to end is the JavaScript one — the unparsed preview — because that
  string exists in the DOM and can be inspected for a lone surrogate.
- **That `fit-content(50%)` measures the right glyphs.** The fitted width is asserted as "keeper
  asked the layout engine to fit", which is the whole of keeper's contribution. Whether the engine
  then measures correctly is the engine's contract. jsdom's CSS parser does not even retain the
  declaration, which is why it travels as a custom property.
- **That a real value overflows a real pane in the real font.** Every overflow test chooses its own
  pane width and its own 8-px glyph. The thresholds are the test's, not the browser's.
- **That the drag feels right.** Pointer capture, the cursor, the hit-strip width and whether the
  seam is findable with a mouse are all unobservable in jsdom. `setPointerCapture` is a `vi.fn()`
  from `setup.ts`.
- **Enter on the focused trigger.** jsdom does not synthesise the `click` a browser dispatches for
  Enter/Space on a focused `<button>`. The test asserts both halves the browser joins — the trigger
  is focusable and receives focus, and the `click` event opens the panel — plus the parts that ARE
  real keyboard behaviour: focus lands in the panel, Escape closes it, and focus returns to the
  trigger. The Enter → click step itself is a platform guarantee, unproven here.
- **The macOS gates.** `bun run lint`, `bun run typecheck`, `cargo clippy`, `cargo fmt --check` and
  the full suite were not run, by instruction.

## Deliberately NOT done

- **No draggable seam in the Files pane or the notes list.** Both are one flexible text column
  beside a content-sized trailing group; there is no boundary between two columns to move, and the
  action buttons have intrinsic minimum widths so shrinking a track around them would only overflow
  them. Manufacturing a seam there would be a new fixed-width guess of a different shape, which is
  the thing this epic is deleting. Both surfaces get AD-83's steps 1 and 3. When 44.17 adds a
  sync-status column to the Files tree there will be a real boundary, and `useResizableColumn` is
  already general — it takes an id and a label and nothing else.
- **No virtualisation anywhere.** AD-84 is 44.10's, and this story adds no unbounded render: the
  Files tree mounts what 43.8 mounted, and the notes list's `+n` popover exists only for rows the
  virtualiser already rendered.
- **`src/components/notes/attachments-panel.tsx` still uses `title=`** (line 241, the same idiom
  this story removed from the Properties panel). It is 43.7's surface and is not in this story's
  named blast radius; the fix is one import and one element, and `OverflowValue` is ready for it.
  Recording it here rather than doing it silently in someone else's file.
- **Editable scalar values keep their `<input>` and get no affordance.** A caret scrolls; the value
  is reachable and the panel does not need a second way to read what the user can already select.
  Only the values keeper renders read-only — `id`, the recording paths, the unparsed block — cannot
  be scrolled, and those are the ones that got it.
- **No column headers.** The Properties panel is a compact inline panel; a header row to hang the
  seam on would cost a line of every note's editor to label two columns that are self-evident. The
  seam spans the grid's explicit rows instead.
- **Nothing in Rust, and no binding.** The story is frontend-only and a column width is a lens the
  viewer chose, not a fact the engine has any use for.
