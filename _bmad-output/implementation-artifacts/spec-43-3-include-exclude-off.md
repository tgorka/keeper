---
title: 'Story 43.3: Include, Exclude, Off'
type: 'feature'
created: '2026-08-09'
status: 'review'
blocking_condition: ''
baseline_revision: '9f7150d'
final_revision: ''
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/epic-43-a-note-can-show-you-the-file.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-42-5-one-tag-vocabulary.md'
---

<intent-contract>

## Intent

**Problem:** the chip bar can say "notes tagged `client/acme`" and cannot say "and not `draft`". The
grammar for saying it has existed since FR-105 — `keeper_core::notes::query` parses `-` as negation
in front of any term, including `tag:` — but it is reachable only by typing a query into a space
note, which is a power feature nobody discovers. Meanwhile the tag chip is a two-state control that
looks like a two-state control, so there is nowhere for a third state to go.

**Approach:** the chip becomes three-state — off, include (`+`), exclude (`−`) — in both surfaces
that render one (the filter bar and the tag tree), and the predicate behind it lands in
`keeper-core` beside `IndexEntry::matches_text`, which is where story 42.6 put the free-text axis for
the same reason: one definition, testable on Linux, with the Tauri shell as a thin call site
(AD-55/AD-56). No new grammar is added anywhere. `NoteQueryReq.tags` changes from `Vec<String>` to a
map from tag to term, which is what makes "include and exclude the same tag" unrepresentable rather
than resolved.

### What the DSL already parses for negation, and what was reused

Established by reading `notes/query.rs` before touching anything:

| Existing behaviour | Where | Reused as |
|---|---|---|
| `-term` → `Node::Not(term)`, in front of *any* term including a parenthesised group | `Parser::term`, L500–544 | the meaning of an `Exclude` chip: `-tag:x` |
| A bare `-` negates whatever term follows it | same | — (the chip has no free text) |
| `tag:x` matches `x` and everything beneath it, at the segment boundary | `Pred::Tag { strict: false }` + `tag_descends` | `Include` |
| `tag:x/*` matches descendants only | `Pred::Tag { strict: true }` | not exposed; a chip names a node, not a subtree-minus-itself |
| `tag:` is normalised through `notes::tags::normalise` (Story 42.5), and a term that is not a tag becomes the empty path that matches nothing | `tag_pred`, L665–684 | chips normalise identically, with the identical degradation |
| Juxtaposition is AND | `Parser::conjunction` | terms intersect; an exclusion intersects like an inclusion |

The one thing the DSL did **not** have in one place was the segment rule itself: `query.rs` had a
private `tag_descends`, `index.rs` had a private `is_tag_descendant`, and `keeper::notes_ipc` had a
third copy inside `tag_matches`. All three were the same four lines. `index.rs`'s is now `pub`, the
other two are gone, and `tag_covers` (equal-or-beneath) sits beside it as the one spelling of what
`tag:` means.

## Boundaries & Constraints

**Always:**
- The predicate is `keeper-core`'s. `keeper::notes_ipc::matches_filter` keeps only the axis that
  needs a shell fact (the commit head behind `origin:`).
- An exclusion is the negation of the inclusion spelled with the same tag, down to the segment rule.
  A chip's `+` and its `−` talk about the same set of notes or the sign changes the subject.
- Terms intersect. Every `include` present, every `exclude` absent.
- Chips are read through `notes::tags::normalise` — the one vocabulary (Story 42.5).
- The three states are distinguishable without hovering, in both surfaces, by a glyph AND a colour
  AND the accessible name.
- A space saved from the bar writes the DSL the DSL already parses: `tag:x` and `-tag:x`. A note
  keeper wrote stays a note Obsidian renders and a human can edit by hand.

**Block If:**
- Nothing. The change is additive to the wire in meaning and closed in shape.

**Never:**
- No new grammar in `query.rs`. The parser is untouched apart from importing the shared segment rule.
- No precedence rule anywhere. "Included and excluded" is either unrepresentable or unsatisfiable,
  never arbitrated.
- No second tag predicate in TypeScript (AD-20, AD-58). The store composes terms and never inspects
  a row.
- No `aria-pressed` on a three-state control.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| Exclusion removes what inclusion shows | note tagged `draft`; chip `draft` | `+` admits it, `−` removes it | none |
| The two compose | `+client/acme`, `−draft` | a `client/acme` note without `draft` is admitted; the same note tagged `draft` is not | none |
| Exclusion does not widen | `+client/acme`, `−draft` | a `client/other` note is still excluded | none |
| Excluded ancestor | `−client` | `client`, `client/acme` and `client/acme/renewal` all removed | none |
| Segment rule, exclusion side | `−client` | `clients` survives | none |
| Segment rule, inclusion side | `+client/acme` | `client/acme/renewal` admitted, `client/acmecorp` not | none |
| Chip spelling | chip `#Client/Acme ` | resolves to `client/acme` through the one vocabulary | none |
| Chip that is not a tag | chip `---` | `+` admits nothing, `−` removes nothing | none |
| Two spellings, opposite terms | `Draft` include + `draft` exclude | unsatisfiable — empty list, both chips still on screen | none |
| No chips | empty map | every note admitted | none |
| Untagged note | `−draft` / `+draft` | admitted / not admitted | none |
| Chip cycle | press, press, press | include → exclude → off, and off is the chip leaving the bar | none |
| Same tag twice | `setTagTerm(draft, include)` then `exclude` | one entry, in its original bar position | none |
| Tree, plain press | press a node three times | include → exclude → off, other chips cleared each time | none |
| Tree, shift press | shift-press a second node | added to the intersection | none |
| Empty result | `+client/acme`, `−draft`, no rows | "No notes match these filters." plus "Narrowed by client/acme and not draft." | none |
| Empty vault | no chips, no rows | the fixed sentence, no term line | none |
| Save as space | `+client/acme`, `−draft` | `tag:client/acme -tag:draft` | none |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/notes/index.rs` — `NoteTagTerm`, `TagTerms`,
  `IndexEntry::matches_tags`; `is_tag_descendant` made public and `tag_covers` added beside it.
- `src-tauri/crates/keeper-core/src/notes/query.rs` — the private `tag_descends` deleted; `Pred::Tag`
  evaluates through the shared rule.
- `src-tauri/crates/keeper-core/src/notes/vm.rs` — `NoteQueryReq.tags: BTreeMap<String, NoteTagTerm>`.
- `src-tauri/crates/keeper/src/notes_ipc.rs` — `matches_filter` delegates; local `tag_matches` gone;
  the terms are folded once per query in `project_list` rather than once per entry.
- `src/lib/ipc/gen/NoteTagTerm.ts`, `NoteQueryReq.ts` — regenerated by ts-rs, not hand-written.
- `src/lib/ipc/client.ts` — one re-export.
- `src/lib/stores/notes-filters.ts` — `tags: string[]` → `tagTerms: readonly TagChip[]`;
  `toggleTag` → `cycleTag` + `setTagTerm`; `nextTagChipState`, `tagChipState`, `emptyFilterReason`.
- `src/components/notes/note-filter-bar.tsx` — `TagFilterChip`.
- `src/components/notes/tag-tree.tsx` — node state, cycle, shift-cycle.
- `src/components/notes/notes-empty-state.tsx` — optional `detail` line.
- `src/components/notes/notes-pane.tsx`, `src/hooks/use-notes-changes.ts` — the rename.
- `src/hooks/use-notes-actions.ts` — `spaceQueryText` emits `-tag:` for an exclusion.

## Tasks & Acceptance

**Execution:**
- [x] `NoteTagTerm`, `TagTerms`, `IndexEntry::matches_tags` in `keeper-core`.
- [x] One definition of the segment rule; two copies deleted.
- [x] `NoteQueryReq.tags` as a map; bindings regenerated by ts-rs.
- [x] Three-state chip in the filter bar and in the tag tree.
- [x] The empty result names the terms.
- [x] `-tag:` in the space the bar saves.
- [x] Tests, each proved by reverting the code it covers.

**Acceptance Criteria:**
- `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core --lib notes::` — 212 passed.
- `bun run test src/lib/stores/notes-filters.test.ts src/components/notes/note-filter-bar.test.tsx`
  — 24 passed.
- `bun run test src/components/notes/tag-tree.test.tsx src/components/notes/notes-pane.test.tsx
  src/components/notes/note-list.test.tsx` — 19 passed.

## Design Notes

**An excluded ancestor excludes its subtree.** The decision the story asked for, and the reason is
that the alternative is incoherent rather than merely different. `tag:client` selects
`client/acme/renewal`; if `-tag:client` left it alone, the same chip would mean two different sets
depending on which sign it carried, and a user hiding a whole client would be surprised by its
renewal notes surviving. It is also what the DSL already answers, because `-tag:x` is literally
`Not(Tag(x))` — choosing otherwise would have meant inventing a second negation for chips only.
Asserted both ways: `−client` removes `client`, `client/acme` and `client/acme/renewal`, and leaves
`clients` alone.

**A map on the wire, not two lists.** The cheap change was `tags` plus `excludedTags`. It would have
worked and it would have left `draft` in both lists representable, at which point something has to
pick a winner and the AC's "impossible to express rather than resolved by precedence" becomes a lie
told at the UI layer only. Keying the terms by tag deletes the state instead of arbitrating it, and
it costs one field rather than two. The rename churn (`tags` → `tagTerms` in the store, reaching
`notes-pane.tsx` and two hooks) is the price, and it is paid by the compiler rather than by a reader.

**Two spellings of one tag are the residual collision, and the DSL answers it.** Normalisation is
many-to-one, so `Draft: include` and `draft: exclude` are two map keys and one tag. `TagTerms` keeps
a `Vec` rather than folding into a map after normalising, which means both terms survive to
evaluation and the conjunction is unsatisfiable — the list is empty, both chips are still on screen,
and nothing silently dropped one of them. That is exactly what `tag:draft -tag:draft` does in a
space. Collapsing into a map here would have been the precedence rule by the back door.

**Normalisation is hoisted out of the entry walk.** `normalise` allocates and `matches_tags` runs
against every entry on every keystroke; `TagTerms::new` folds the chips once per query, in
`project_list`. Ten thousand entries times three chips is thirty thousand allocations that NFR-28's
100 ms budget does not have to pay for.

**Off is the absence of a term.** `NoteTagTerm` has two variants and the UI's `TagChipState` has
three. A wire enum with an `Off` variant would be a term that admits everything, which every reader
of a query then has to prove harmless; an absent key proves it by construction. The asymmetry is
stated on both types.

**A chip is rewritten in place, never appended beside itself.** `withTagTerm` maps over the list
rather than filtering and pushing, so a chip keeps the position the user first put it in. The bug
avoided is small and infuriating: a chip that jumps to the end of the bar on every press is a target
that moves under the cursor mid-cycle.

**The tree reads the next state before it clears the bar.** A plain press in the tag tree means "show
me this and nothing else", so it calls `clearAll()` — and `clearAll` empties `tagTerms`, so cycling
*after* it would restart at include on every press and leave exclude reachable only with the shift
key. `nextTagChipState` is exported for exactly this: the order of the cycle is the control's
contract, and the tree and the bar must not each own a copy of it. Asserted in
`tag-tree.test.tsx`, and the wrong ordering is one of the reverts below.

**No `aria-pressed`.** It has two values. A chip reporting `pressed=false` while actively removing
notes from the list is worse than saying nothing, so the state and the consequence of pressing are
spelled in the accessible name instead — "Tag draft: excluded. Stop filtering by it." In the tree,
`aria-selected` stays `false` for an excluded node, which is true: it is emphatically not selected.

**The empty result names every active term, not the one to blame.** Attributing an empty list to a
single chip would need one query per term, and it would still be a guess — two terms can each be
innocent alone and empty the list together. So `emptyFilterReason` lists what is narrowing, in bar
order, in words: "Narrowed by client/acme and not draft." The exclusion is said as *not draft*
rather than as `−draft` because the `−` glyph does not survive being read aloud, and an exclusion is
precisely the term whose effect a person cannot see — an over-eager inclusion leaves a list you can
tell is wrong, while an exclusion leaves the same empty pane whether it removed one note or nine
hundred. The sentence is a second line under the fixed copy rather than folded into it, because the
fixed copy is a fact about the state and the term list is the part that changes.

## Verification

Every test below was proved by mutating the code it defends, watching it fail, and restoring.

| Revert | Tests that caught it |
|---|---|
| `NoteTagTerm::Exclude => true` (an exclusion is accepted and ignored) | `an_exclusion_removes_what_an_inclusion_would_have_shown`, `an_inclusion_and_an_exclusion_compose`, `an_excluded_ancestor_excludes_its_descendants`, `a_chip_is_read_through_the_tag_vocabulary`, `a_tag_included_and_excluded_under_two_spellings_is_unsatisfiable` |
| Exclusion tests only the exact tag, not the subtree | `an_excluded_ancestor_excludes_its_descendants` |
| `is_tag_descendant` becomes a plain `starts_with` | `an_included_ancestor_admits_its_descendants_and_not_its_neighbours`, `an_excluded_ancestor_excludes_its_descendants`, `a_chip_that_is_not_a_tag_matches_nothing_and_excludes_nothing`, and 37.3's own `a_parent_tag_counts_distinct_notes_in_its_subtree_not_tag_occurrences` |
| `TagTerms::new` stops normalising | `a_chip_is_read_through_the_tag_vocabulary` |
| `TagTerms` holds a `BTreeMap`, so a normalisation collision picks a winner | `a_tag_included_and_excluded_under_two_spellings_is_unsatisfiable` |
| Cycle drops the exclude state | `cycles off, include, exclude, off`, `cycles include, exclude, off as the chip is pressed` |
| `withTagTerm` appends instead of rewriting in place | `cannot hold one tag as both included and excluded`, `keeps a chip where it is in the bar when its state changes`, `cycles off, include, exclude, off` |
| The sentence loses the word "not" | `names the excluded term that emptied the list…`, `names a lone term without inventing a list`, `names the term that emptied it, including the exclusion` |
| `noteQueryFor` drops exclude terms | `sends an excluded chip as an exclude term rather than dropping it`, `cannot hold one tag as both included and excluded` |
| The bar chip's name stops saying its state | `shows which state it is in without being hovered` |
| The bar chip renders excluded exactly like included | `shows which state it is in without being hovered` |
| The tree node's name stops saying its state | `shows an excluded node as excluded without being hovered`, `cycles a node through include, exclude and off on plain presses` |
| The tree node renders excluded exactly like included | `shows an excluded node as excluded without being hovered` |
| The tree cycles *after* `clearAll` instead of before | `cycles a node through include, exclude and off on plain presses` |

**What could not be verified on this host.** The `keeper` shell crate does not build on Linux, so
`notes_ipc.rs` was changed without a compile: the delegation in `matches_filter`, the once-per-query
`TagTerms::new` in `project_list`, and the ported test
`the_shell_hands_the_chip_terms_to_the_core_predicate` are unexercised here and the macOS gate is
their first real check. Nothing visual was driven in a browser either — the chip assertions are
jsdom assertions about the accessible name, the `data-tag-term` attribute, the rendered glyph element
and the fact that the two states' class strings differ, which is as close to "visible without
hovering" as this host can get; that the destructive colour actually reads as a negation on screen is
a judgement only a person looking at it can make.

## Deliberately Not Done

- **No third state in the space DSL.** The chip is a face for `-tag:`, and nothing was added to the
  grammar. A space still round-trips through Obsidian as text a human wrote.
- **No `tag:x/*` chip.** The strict descendants-only form exists in the DSL and has no control,
  because a chip names a node and "everything under here except here" is not a thing anyone asks a
  chip for.
- **No exclusion on the other chips.** Scope, "Changed by agent" and "Pinned only" stay two-state.
  Widening them is a bigger question about what a negated lens means, and this story is about tags.
- **No per-term attribution in the empty state.** Named terms, not a culprit; the reasoning is under
  Design Notes.
- **No space editor.** Story 43.4 reuses `setTagTerm` and `tagChipState` for that, which is why both
  are exported rather than private to this file.
