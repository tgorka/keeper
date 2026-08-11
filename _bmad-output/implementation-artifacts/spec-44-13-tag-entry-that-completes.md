---
title: 'Story 44.13: Tag Entry That Completes'
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
  - '{project-root}/_bmad-output/implementation-artifacts/spec-42-5-one-tag-vocabulary.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-43-3-include-exclude-off.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-43-4-spaces-you-can-edit.md'
---

<intent-contract>

## Intent

**Problem:** every place keeper lets someone choose a tag is half a control. The space editor
(43.4) is a `<select>` — browsable, untypeable, and unusable once a vault has two hundred tags. The
filter bar (43.3) has no chooser at all: the only way to raise a tag chip is to find the tag in the
sidebar tree, which is a fine way to browse and a poor way to reach a tag you can already name. The
recording card (42.5) is a text field with a `<datalist>` — typeable, and invisible until you have
typed. The owner asked for both halves at once, explicitly.

Underneath that there is a second problem the story names: nobody in this repo had answered "which
tags match what I typed". Both existing affordances delegated it — the editor popup to CodeMirror's
fuzzy matcher, the recording field to the browser's `<datalist>` — so there were two answers,
neither of them keeper's, and neither of them aware that a tag is a **path**. Typing `#ent` in the
editor offered `client`, on a substring hit in the middle of a segment.

**Approach:** one pure matcher (`components/tags/tag-match.ts`) that matches the way
`keeper-core`'s tag tree is shaped — at segment boundaries — and one control
(`components/notes/tag-combobox.tsx`) that renders a text field with the list **permanently**
beneath it. Both existing tag choosers in the notes surface adopt the control, and the editor's `#`
popup adopts the matcher, so the question is now answered once. Creating a tag the vocabulary does
not have is a permission the caller grants, and the refusal says why rather than showing an empty
box.

### What already existed, and what was reused rather than restated

| Existing behaviour | Where | Reused as |
|---|---|---|
| The vault's tag vocabulary as full paths, ancestors included, both producers summed | `keeper-core/notes/tags.rs::tag_tree`; `tagPaths` in `editor/tag-complete.ts`; `tagsVocabulary` (42.5) | the `vocabulary` prop; nothing new is read |
| What a tag IS — case, whitespace, slashes, emptiness | `keeper-core/notes/tags.rs::normalise` | untouched; the control hands text on verbatim |
| `tag:` in a space query is normalised on parse | `notes/query.rs::tag_pred`, L683 | why the space editor may accept a typed tag without owning normalisation |
| Chip state and the three-state cycle | `notes-filters.ts::withTagTerm` / `setTagTerm` | the control's `onChoose` target; no new store state |
| The `#` trigger and the heading-vs-tag rule | `editor/tag-complete.ts::OPEN_TAG` | unchanged; only the filtering moved |
| Escape walks the chip stack one chip per press | `note-filter-bar.tsx::onSearchKeyDown` | why the chooser stops Escape from bubbling |

## Boundaries & Constraints

**Always:**
- The field and the list are both present, always. There is no expanded/collapsed state and no
  popup — a list you have to open is a list nobody browses.
- Matching is segment-aligned, from any segment boundary, last segment by prefix. One definition,
  in `tag-match.ts`, asked by the control and by the editor popup.
- Case folding in `tag-match.ts` is a **search** rule. What a tag means stays
  `keeper-core/notes/tags.rs`'s, and every string the module returns came out of the vocabulary or
  out of the user's own keystrokes.
- Focus stays in the field. Arrowing moves `aria-activedescendant`; choosing — by key or by mouse —
  leaves the caret where it was, because tagging happens in runs.
- Creating is the caller's permission. Refused, the control names the refusal.
- No new dependency. No combobox library, no virtualisation.

**Block If:**
- Nothing. Additive to both call sites; the wire is untouched and no binding changed.

**Never:**
- No normalisation in TypeScript. No case rule, whitespace rule or slash rule that decides what a
  tag IS (AD-20, 42.5).
- No second matcher. `tag-complete.ts` stopped having an opinion; it asks.
- No `<datalist>` in the new control — the browser's match is the thing being replaced.
- No `aria-expanded="false"` while options are on screen.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| Browse, nothing typed | vocabulary of 4 | all 4 listed, in the vocabulary's own order | none |
| Browse, empty vault | vocabulary `[]`, query `""` | "This vault has no tags yet." | none |
| Type a leaf name | `acme` | `client/acme`, `client/acme/renewal` | none |
| Substring inside a segment | `ent` over `client`, `renewal/entry` | only `renewal/entry` | none |
| Hierarchy, exact earlier segment | `client/ac` | `client/acme`, `client/acme/renewal` | none |
| Hierarchy, partial earlier segment | `cl/ac` | nothing — an earlier segment must match whole | none |
| Half-typed separator | `client/` | `client` and its subtree | none |
| Query deeper than the tag | `client/acme/renewal` vs tag `client/acme` | no match | none |
| Ranking | `renewal` | `renewal/entry` (root-anchored) before `client/acme/renewal` | none |
| Ranking, same offset | `w` over `work`, `worry`, `work/clients` | shallower first, then alphabetical | none |
| Case | `CLIENT/Ac` | `client/acme`, spelled as the vault spells it | none |
| Create allowed, new tag | `client/newco`, `allowCreate` | "Create tag …" row, last; Enter emits the typed text verbatim | none |
| Create allowed, existing tag | `client/acme`, `allowCreate` | no create row — it already names a tag | none |
| Create allowed, already chosen | `Standup` with `standup` chosen | no rows; `"Standup" is already on this list.` | none |
| Create forbidden, new tag | `nonesuch` in the filter bar | `No tag matches "nonesuch". …`; Enter emits nothing | none |
| Chosen tags | `chosen: [client/acme]` | left out of the list | none |
| Arrow + Enter | ↓ ↓ Enter | third row chosen; `document.activeElement` is still the field | none |
| Arrow wrap | ↑ from the top | last row; ↓ returns to the first | none |
| Enter with no arrowing | after typing | the closest match, which is row 0 | none |
| Enter with nothing to choose | empty list | swallowed; the dialog's Save is not triggered | none |
| List shrinks under a still caret | arrow to row 3, then `chosen` grows | highlight clamps to the last row; Enter chooses it | none |
| Escape, query present | `acme` typed | query cleared; not dismissed | none |
| Escape, query empty | — | `onDismiss`; the event does not bubble to the bar's chip-walk | none |
| Mouse pick | mousedown + click on a row | row chosen; the press's default is prevented so the caret survives | none |
| Filter bar, chooser closed | bar mounted | `tags_vocabulary` is NOT called | none |
| Filter bar, vocabulary unreadable | IPC rejects | empty list, and the refusal sentence — a filter cannot invent a tag | none |
| Space editor, vocabulary unreadable | `notes_tag_tree` rejects | nothing to browse; typing still works, because creating is allowed there | none |
| Editor popup, `#w` then `ork` then `/c` | real `EditorView` | the list narrows at each keystroke and reaches the child past the slash | none |
| Editor popup, bare `#` | first column, not explicit | still closed — the heading rule is unchanged | none |

</intent-contract>

## Code Map

- `src/components/tags/tag-match.ts` — **new.** `tagMatchOffset`, `matchTags`, `namesTag`. The one
  answer to "which tags match what I typed".
- `src/components/notes/tag-combobox.tsx` — **new.** The control: field plus permanent listbox,
  `aria-activedescendant` keyboarding, `allowCreate`, and the three refusal sentences.
- `src/components/notes/editor/tag-complete.ts` — `filter: false`, `validFor` removed, options
  pre-narrowed by `matchTags`.
- `src/components/notes/note-filter-bar.tsx` — an "Add a tag filter" toggle in the chip row and the
  chooser beneath it, create forbidden; vocabulary read from `tagsVocabulary()` on open.
- `src/components/notes/space-editor.tsx` — the `<select>` replaced, create allowed; `addTagId`
  gone.
- `src/components/tags/tag-vocabulary-input.tsx` — comment only: the sentence claiming
  `tag-complete.ts` gets its matching from CodeMirror is no longer true.
- Tests: `tag-match.test.ts` (new), `tag-combobox.test.tsx` (new), `tag-complete.test.ts`,
  `note-filter-bar.test.tsx`, `space-editor.test.tsx`.

## Tasks & Acceptance

**Execution:**
- [x] One segment-aligned matcher, with ranking, in `tag-match.ts`.
- [x] The control: field AND list, always both.
- [x] Full keyboard operation asserted through `document.activeElement` and the resulting selection.
- [x] Create allowed in the space editor, refused with a reason on the filter bar.
- [x] The editor's `#` popup asks the same matcher.
- [x] Tests, each proved by mutating the code it defends.

**Acceptance Criteria:**
- `bun run vitest run src/components/notes/tag-combobox.test.tsx src/components/tags/tag-match.test.ts src/components/notes/note-filter-bar.test.tsx src/components/notes/space-editor.test.tsx src/components/notes/editor/tag-complete.test.ts` — 76 passed.
- With the neighbouring suites that mount the same surfaces or the same completion stack
  (`editor/indent-keymap.test.ts`, `notes-pane.test.tsx`, `tags/tag-vocabulary-input.test.tsx`,
  `tag-tree.test.tsx`) — 117 passed.
- `biome check` over `src/components/notes` and `src/components/tags` — clean but for two
  pre-existing warnings in files this story did not touch.

## Design Notes

**Segments, not substrings, and the offset is the ranking.** A tag is a path, and the vocabulary
arrives as full paths because that is how `keeper-core`'s tree is flattened. So a query is cut the
same way and matched against a consecutive run of the tag's segments beginning at a boundary:
earlier segments whole, the last one by prefix. That single rule gives `acme` → `client/acme`
without special-casing "search deep tags", gives `client/ac` → the `acme` children and not the
`anvil` sibling, and refuses `ent` → `client` — which is the concrete thing CodeMirror was getting
wrong. The offset where the match began is kept rather than discarded because it is the only
ranking signal worth having: a root-anchored hit is a closer answer than one three levels down.
Order is (offset, depth, path) — three total comparisons, so two runs over one vocabulary can never
disagree about which row the arrow keys are on.

**Case is folded, and that is not a normalisation rule.** 42.5's line is that a rule about case,
whitespace or slashes stated outside `tags.rs` is a second vocabulary. Folding case to decide what
to OFFER is not such a rule: it decides nothing about identity, it is exactly the courtesy a
`<datalist>` already extends, and every string the module returns is either the vault's own
spelling or the user's own keystrokes. `namesTag` compares segment-wise for the same reason —
`/Client/Acme/` and `client/acme` are one path written twice, and offering to create the second
while the first is on screen would be the control lying about its vocabulary. It deliberately stops
short of `normalise`: `My Tag` against an existing `my-tag` is allowed through as a creation, and
Rust folds it onto the existing tag at the boundary, which is the right outcome by a different road.

**Creating is allowed in the space editor and refused on the filter bar.** 43.4 made the space
editor a `<select>` and said why: a free-text box would be a second definition of what a tag is. It
would not, and the reason is checkable — `notes/query.rs::tag_pred` runs every `tag:` term through
`tags::normalise` on the way in (L683), which is the same road a hand-written space travels. What
separates the two surfaces is not normalisation but consequence. A space is a document being
authored and re-evaluated forever; naming `client/newco` before the first note carries it is
ordinary, and refusing means you cannot build the space until after you have built the notes. A live
filter chip for a tag no note carries is an empty list with nothing to explain it, so the bar says
`No tag matches "nonesuch".` — the story's "rather than silently offering nothing". One consequence
is worth stating: until the dialog is reopened, a created chip shows the text as typed while the
saved query means the normalised form. Reopening re-reads the terms through `notesSpaceTerms` and
the chip shows the canonical spelling.

**`filter: false` and `validFor` leave together.** CodeMirror's `validFor` says "further typing that
still matches this regex may reuse these options without re-querying". That is safe only while
CodeMirror is also re-filtering them. Keeping it beside `filter: false` would have frozen the popup
on whatever the first character matched — a defect that every mocked-`CompletionContext` assertion
in the file would have sailed past, because a mocked context is asked exactly once. It is the reason
this story added a real `EditorView` test.

**No `useMemo` around the row list.** Every caller builds `chosen` by mapping its own chip array, so
the dependency changes identity on every render and the memo would miss every time while looking
like it did not. One pass over the vocabulary per render is what this costs, said out loud in the
code rather than hidden behind a hook that does nothing.

**The highlight clamps; the query resets.** Two different things. Typing resets the highlight to row
0, which is what makes Enter mean "the closest match". The clamp is for the other case — the list
shrinking while the caret sits still, which happens when a chip is raised elsewhere in the bar with
the chooser open. Without it `aria-activedescendant` points at an id that is not in the document and
Enter fires on nothing while a row still looks chosen.

**Escape is stopped, not just handled.** The bar's own Escape contract walks the chip stack down one
chip per press. A chooser that let its dismissal bubble would throw away the chip behind it on the
way out.

**The listbox has no accessible name.** The field owns the name and points at the list with
`aria-controls`. Naming both gave "Add a tag" two targets — which surfaced immediately as a
duplicate-match failure in the space editor's existing `getByLabelText("Add a tag")`, and would have
surfaced for a screen-reader user as two identically named things in the same group.

**`div`s with roles rather than `ul`/`li`.** `role="listbox"` on a list element overrides the
element's own semantics, which this repo's lint refuses and a screen reader has to reconcile. The
option rows carry `tabIndex={-1}`: not in the tab order, deliberately, because the
`aria-activedescendant` pattern moves a highlight and not the caret.

## Verification

Every test below was proved by mutating the code it defends, running the five suites, watching it
fail, and restoring. Baseline was green before and after each mutation.

| Mutation | Tests that caught it |
|---|---|
| `matchTags` matches the last segment by `includes` instead of `startsWith` (a substring matcher) | `refuses a substring that starts inside a segment`; `tagCompleteSource matches at a segment boundary, not by substring` |
| `validFor` restored in place of `filter: false` (CodeMirror filters again, popup freezes) | `re-queries as the tag is typed rather than pinning the first list`; **`the popup in a real editor narrows as each character is typed, and reaches the child past the slash`** |
| The active-row clamp removed (`const at = active`) | `keeps Enter on a real row when the list shrinks underneath it` |
| Escape's `stopPropagation` removed | `keeps Escape to itself, so dismissing does not also drop the chip behind it` |
| `allowCreate` ignored — creating allowed everywhere | `says there is no such tag where creating is not allowed`; `refuses to invent a tag, because a filter can only narrow to what exists`; plus three list-shape assertions |
| The create row `unshift`ed above the matches | `puts the offer to create below the matches, never above them`; `adds a tag by typing it, from a list that is still there to be browsed` |
| The option's `onMouseDown` preventDefault removed | `refuses the focus a mouse press would take, so the caret survives a click` |
| The list rendered only once something is typed (field, no browse) | 14 tests across all three suites, including `renders every tag with an empty query, in the vault's own order` and `browses the vault's tags before a key is pressed` |
| The filter bar stops returning focus to its toggle on dismiss | `takes the caret on opening and gives it back on Escape` |
| Chosen tags no longer filtered out of the list | `leaves out the tags already on the bar`; `leaves a chosen tag out of the list it offers`; `will not create a second copy of a tag already chosen`; `adds a tag by typing it, from a list that is still there to be browsed` |

**What could not be proved here.** Three things, stated plainly:

1. **Nothing was driven in a real browser.** The keyboard claims are jsdom claims about
   `document.activeElement`, `aria-activedescendant` and the resulting `onChoose` call. The two
   places jsdom is definitively not the real thing are `scrollIntoView` (stubbed in
   `src/test/setup.ts`, so "the highlighted row stays visible while arrowing through a list taller
   than its box" is unasserted) and focus-on-mousedown (jsdom does not move focus on a press, so
   that test asserts the prevented default — the mechanism — rather than the outcome). Whether the
   list's 12rem cap and the accent highlight read correctly on screen is a judgement only a person
   looking at it can make.
2. **The editor popup was exercised through a real `EditorView`, but not through a real browser.**
   `#w → #work → #work/c` narrowing is asserted against actual CodeMirror with the real
   `autocompletion` extension, and `indent-keymap.test.ts`'s existing Tab-accepts-`#work` test still
   passes. What a real web view does with the popup's positioning is untested here as it was before.
3. **`tagsVocabulary()` is called without a vault id from the filter bar**, so Rust resolves the
   active vault — the same call shape the recording card has used since 42.5. That the active vault
   and the notes pane's vault are the same vault is true by construction in the shell and is not
   asserted anywhere on this host; the space editor, which has a vault id, passes it.

## Deliberately Not Done

- **The recording metadata card still uses its `<datalist>`.** It is a comma-separated multi-tag
  text field, not a chooser, and turning it into one is a change to a surface Story 44.14 owns. Its
  stale comment — which claimed `tag-complete.ts` gets its matching from CodeMirror — was corrected
  to point at `tag-match.ts` and to say why it still delegates, because a comment that contradicts
  the code is worse than no comment.
- **No virtualisation of the option list.** Story 44.10 owns that, epic-wide, and hand-rolling a
  window here would be the second implementation it then has to delete. The list is capped at 12rem
  and scrolls; a vault with ten thousand tags will render ten thousand rows into a scroll box, and
  that is 44.10's problem by design rather than by omission.
- **No match highlighting in the offered rows.** CodeMirror lost its `match` ranges when `filter`
  went to `false`, and the new control never had them. `tagMatchOffset` returns enough to compute
  them; nothing asked for them, and inventing an emphasis rule is a second thing to keep consistent
  between two surfaces.
- **No multi-select.** `onChoose` emits one tag and the caller decides what that means — an
  `include` chip on the bar, an `include` term in the space. A control that accumulated its own
  selection would be a second place chip state lives.
- **`Home`/`End`/`PageUp`/`PageDown` are not bound.** Type, arrow, Enter and Escape are the AC and
  are what the owner will use; adding keys nobody asked for is a contract to keep.
- **The three-state chip is unchanged.** The chooser raises an `include` chip; cycling it to
  `exclude` is still the chip's own press (43.3). A chooser that could raise an exclusion directly
  would be a second place the cycle is spelled.
