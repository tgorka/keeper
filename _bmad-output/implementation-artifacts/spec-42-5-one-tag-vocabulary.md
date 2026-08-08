---
title: 'Story 42.5: One Tag Vocabulary'
type: 'feature'
created: '2026-08-08'
status: 'done'
blocking_condition: ''
baseline_revision: '79c9f09'
final_revision: '77d989378425101a938d55967a1564384db512c5'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/epic-42-the-recordings-archive.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-42-2-searchable-sessions.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-42-4-the-note-stub.md'
---

<intent-contract>

## Intent

**Problem:** keeper has two tag vocabularies that look like one. Notes tags are parsed and normalised
by `notes/tags.rs` and counted into a tree; recording tags are a comma-split string, trimmed twice
and normalised **nowhere**. So `Client/Acme ` on a recording and `client/acme` on a note are two
different tags that a person will spend an afternoon failing to reconcile — and the sidebar count is
a half-truth, because it counts one producer while the surface implies it counts everything.

**Approach:** one normalisation, defined once, applied by both producers; one tree, fed by both; one
completion affordance, offered on both surfaces. This is a convergence story — it deletes a second
convention rather than adding a feature.

## Boundaries & Constraints

**Always:**
- Normalisation is defined in exactly one place — the tag module — and is documented and **one-way**.
  Case, whitespace and separator handling are stated as rules, not discovered by reading two callers.
- The tag tree gains a second producer through a real entry point. Today `tag_counts` is fed only by
  note upserts; recordings must feed it the same way notes do, not through a parallel path that can
  drift.
- A count shown to a person is the sum of every producer behind it. A tree node that says 7 means 7
  things, not 7 notes and some recordings it declined to mention.
- Existing note tags keep resolving to exactly the nodes they resolve to today. This story must not
  silently re-file anyone's notes.
- Recording tags already written to manifests are normalised on the way **in**, at the boundary — the
  manifest keeps saying what the user typed, and the index says what it means.

**Block If:**
- Normalising would change which node an existing NOTE tag resolves to. That would be a migration,
  not a convergence, and it needs its own decision.

**Never:**
- No change to the on-disk manifest format, and no rewriting of manifests to normalise their tags in
  place. The manifest is the user's text.
- No new tag syntax, no aliases, no renames, no bulk retagging UI.
- No change to how notes are indexed beyond admitting a second producer.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| Case and trailing space | recording `Client/Acme `, note `client/acme` | the same tag node, asserted over the tree | none |
| Hierarchy | `client/acme/renewal` | files under `client` → `acme` → `renewal`, counting each ancestor once | none |
| Separator handling | a tag with a leading, trailing or doubled `/` | one documented rule, applied identically to both producers | none |
| Empty after normalising | `  ` or `///` | dropped, not stored as an empty tag | none |
| Counts | 2 notes and 3 recordings under `client/acme` | the sidebar shows 5 | none |
| Last recording removed | the only recording carrying a leaf | the leaf goes; a sibling keeps the parent alive | none |
| Completion, cross-producer | a tag that exists only on notes | offered in the recording metadata card | none |
| Completion, cross-producer | a tag that exists only on recordings | offered on the notes surface | none |
| Selecting a tag | a node in the sidebar | can scope either surface | none |
| Existing note tags | any tag already indexed | resolves to the same node it does today | none |
| A manifest's own text | any recording | unchanged on disk; only the index normalises | none |
| Duplicate after normalising | `Acme` and `acme` on one recording | one tag, counted once | none |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/notes/tags.rs` — the one normalisation, documented; and the tree's
  second-producer entry point. **This file is the whole point of the story:** if a rule ends up
  restated anywhere else, the story failed.
- `src-tauri/crates/keeper-core/src/notes/index.rs` — `tag_counts` / `IndexSnapshot::tag_tree` learn a
  producer. Today they are fed only by note upserts.
- `src-tauri/crates/keeper-core/src/archive/recordings.rs` / `recordings_fts.rs` — recording tags are
  normalised at the boundary, on the way into the row and the index. The manifest is untouched.
- `src-tauri/crates/keeper/src/ipc.rs` — wherever recording tags are parsed from the metadata card
  today (the comma-split), replaced by the one parser.
- Frontend: the recording metadata card's tag input gains completion. **The existing affordance is a
  CodeMirror `CompletionSource` and is unusable from a plain `<Input>`** — this story supplies a
  completion the card can actually use, and does not fork the vocabulary to do it.
- Sidebar tag counts — rendered from the tree, so they follow once the tree is right.

## Tasks & Acceptance

**Execution:**
- [x] One documented, one-way normalisation in the tag module, applied by both producers.
- [x] Recording tags normalised at the boundary; manifests untouched.
- [x] A real second-producer entry point into the tag tree and its counts.
- [x] Completion in the recording metadata card, over the shared vocabulary.
- [x] Sidebar counts summing both producers.
- [x] Tests: every matrix row.

**Acceptance Criteria:**
- A recording tagged `Client/Acme ` and a note tagged `client/acme` resolve to the same tag node,
  asserted in a Rust test over the tree.
- Completion in the recording metadata card offers tags that exist only on notes.
- Removing the last recording carrying a leaf tag removes the leaf while a sibling keeps the parent.
- The counts shown in the sidebar equal the sum of both producers.

## Design Notes

**Nothing about how a note tag resolves changed, and that is the load-bearing claim.** `normalise`'s
body was not touched — the diff of `tags.rs` is additions and doc comments only. The rules were
*promoted* from implementation detail into prose, not altered. A frozen regression table,
`promoting_the_rule_moved_no_existing_note_tag`, pins ten real tag shapes to the node each resolves to
today plus six rejected shapes, so a future tidy-up that would re-file someone's notes fails there
instead of shipping as a silent migration.

**The rules, now stated once:** edge whitespace is not part of the tag; a leading run of `#` is
punctuation; case does not distinguish tags; interior whitespace folds to `-`, except beside a `/`
where the slash is already a boundary; `/` never yields an empty segment, so leading, trailing and
doubled slashes collapse; a tag with no alphanumeric character is rejected and dropped rather than
stored empty. One-way by construction — the canonical form is what the index and tree hold, and what
the user typed stays in the note's frontmatter and the recording's `manifest.json`.

**Three restatements were deleted, which is the actual work of a convergence story.**
`notes/query.rs::tag_pred` carried a partial second copy (`trim` + `trim_start_matches('#')` +
`to_lowercase`); `notes_ipc.rs::tag_matches` stripped its own leading `#`; and the frontend
comma-split with its per-tag trim and empty filter is gone. All three now route through the one
parser, and in each case a non-tag term still matches nothing — observably identical, one definition
fewer.

**One duplication was kept deliberately, and documented.** The segment-boundary descend test exists as
SQL in `recordings_fts::TAG_PREDICATE_SQL` and as Rust in `query::tag_descends`. Two languages, no
single definition available to share, and a fixed two-arm prefix test that cannot drift. Saying so in
the doc comments is better than pretending it is an oversight.

**Recording tags are canonicalised at the boundary, never in the manifest.** `RecordingRow::from_
manifest` writes `tags_json` through `normalise_all`, and both write paths — the live sink and
`rebuild_from_disk` — share that one derivation. So a rebuild canonicalises rows written by an older
build without rewriting a single `manifest.json`. The filter side normalises too, so both sides of the
tag predicate are canonical.

**The wire carries the raw line, and Rust tokenises it.** `metaTags` went from `string[]` to `string`:
the user still types comma-separated text, but what counts as a tag is decided in one place. The card
no longer has a private notion of what a tag is.

**A real defect fell out of the count change.** The tag tree's accessible name was
`Tag ${path}, ${count} notes, filter` — a sentence that became a lie the moment the count included
recordings. It now says `items`. The counts are summed in the tree itself, so the sidebar and the
existing CodeMirror completion both followed without a second call.

**Completion is `<Input list>` + `<datalist>`.** It is the affordance the repo already uses for a
free-text field wanting live suggestions, and the browser supplies filtering, keyboard handling and
screen-reader semantics for free. A combobox would only be justified if picking a tag did more than
insert text. Picking a suggestion completes the tag after the last comma rather than replacing the
field — a caret-boundary rule for the affordance, not a normalisation rule, and the direct analogue of
the CodeMirror source's `OPEN_TAG` regex.
