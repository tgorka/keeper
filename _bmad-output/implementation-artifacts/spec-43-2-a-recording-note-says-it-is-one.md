---
title: 'Story 43.2: A Recording Note Says It Is One'
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
  - '{project-root}/_bmad-output/implementation-artifacts/spec-42-4-the-note-stub.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-42-5-one-tag-vocabulary.md'
---

<intent-contract>

## Intent

**Problem:** `compose` writes `session:`, and Story 42.6 made that the predicate for the Recordings
lens. That predicate is the *machine's* — it is invisible to a person. A human browsing the vault's
tag tree sees each note's own tags and nothing at all saying what KIND of note it is, so the notes
keeper writes are the only notes in the vault that cannot be reached by the affordance the vault
already has for reaching notes. A quick capture with no tags of its own is reachable by nothing but
knowing it exists.

**Approach:** every composed stub carries one more tag, `recordings`, appended after the session's
own. It is resolved through Story 42.5's vocabulary rather than pushed as a string literal, so a
session already hand-tagged `Recordings`, `recordings ` or `#recordings` gets no second,
near-identical sibling in the tree. This is four lines of behaviour; the care is entirely in *where*
the tag goes and in *what question* decides whether to add it.

## Boundaries & Constraints

**Always:**
- The session's own tags stay byte-identical and in the order the user typed them. The note's
  frontmatter is one of the two places Story 42.5 promises the user's own text survives (the other
  is `manifest.json`); normalising it here would break that promise in the one place they can still
  read it.
- "Does this session already carry the tag" is asked of `tags::normalise`, never of the strings.
  The whole point of routing through the vocabulary is that `Recordings` and `recordings` are one
  tag, and this file must agree with the tree about that.
- `compose` stays total. There is no session for which it refuses to produce a stub — that is the
  promise Story 42.4 made, because the failure it exists to prevent is losing the one minute in
  which anything would have been written.
- The stub stays valid, unremarkable Obsidian. The tag list is the ordinary block sequence
  `serialise_new` already emits and `Frontmatter::parse` already reads back.

**Never:**
- No second tag vocabulary. No case rule, whitespace rule or `#`-stripping rule restated in
  `recording_note.rs` — if one appears there, Story 42.5's convergence is undone.
- No change to `tags.rs`. This story is a *consumer* of the vocabulary.
- No change to `SESSION_KEY` or to `is_recording_note`. The tag is for humans; the frontmatter key
  remains the one predicate for code, because a user is free to delete a tag and no note may stop
  being a recording note because somebody tidied their tag list.
- No retro-tagging of stubs already on disk. Notes keeper wrote before this story are the user's
  files now.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| Ordinary session | `tags: ["work", "quarterly"]` | `tags: [work, quarterly, recordings]` — own first, kind last | none |
| No tags at all | `tags: []` | `tags: [recordings]`; the `tags:` key is now always emitted | none |
| All-blank tag list | `tags: ["", "   "]` | blanks dropped, `tags: [recordings]` — never a nameless entry | none |
| Already tagged, exact | `tags: ["recordings"]` | unchanged; nothing appended | none |
| Already tagged, cased | `tags: ["Recordings"]`, `["RECORDINGS"]` | the user's spelling kept verbatim; nothing appended | none |
| Already tagged, padded | `tags: ["recordings "]`, `["  Recordings"]` | trimmed to the user's word; nothing appended | none |
| Already tagged, inline form | `tags: ["#recordings"]` | kept verbatim (quoted by the serialiser, since bare `#` opens a YAML comment); nothing appended | none |
| Own tags with their own casing | `tags: ["Zeta", "client/Acme "]` | emitted as typed, in order, then `recordings` | none |
| Round-trip | any of the above | `Frontmatter::parse` reads the list back and the body offset stays exact | none |
| Constant drifts from vocabulary | a future rule makes `normalise("recordings") != "recordings"` | a test fails here, not a twin node in someone's sidebar | none |
| Vocabulary rejects the constant | hypothetical `normalise` returning `None` | no tag appended; `compose` still returns a stub | none |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/notes/recording_note.rs` — the whole story.
  - `RECORDINGS_TAG`, a new public constant beside `SESSION_KEY`.
  - `kind_tag`, a private helper that answers "append, or already there?" through `tags::normalise`.
  - the tag block of `compose`, which now always emits `tags:`.
  - the module doc, which said tags are carried as stored full stop and now names the one exception.
- `src-tauri/crates/keeper-core/src/notes/tags.rs` — read, not changed. `normalise` and
  `normalise_all` are the vocabulary this story consumes.

Nothing else. `keeper/src/ipc.rs` builds the `SessionFacts` and needed no edit; no shell test asserts
a stub's tag list; no binding changes, because `NoteStub` is unchanged in shape.

## Tasks & Acceptance

**Execution:**
- [x] `RECORDINGS_TAG` defined once, documented against the failure it prevents.
- [x] Appended after the session's own tags, with the ordering justified in the code comment.
- [x] Presence decided by `tags::normalise`, so no spelling produces a twin.
- [x] `tags:` emitted even when the session carries none.
- [x] Three existing tests corrected (two of them asserted `tags:` was absent, which is now false).
- [x] Tests: every matrix row, each proven by reverting the change it defends.

**Acceptance Criteria:**
- A stub carries `recordings` in addition to the session's own tags.
- It appears exactly once when the session already carries it in any casing or with surrounding
  whitespace.
- The session's own tags are unchanged and in their original order.
- A session with no tags at all still gets `tags:` with this one.

## Design Notes

**The tag goes last, after the session's own, and the reason is already written in this file.**
`compose` states its one ordering rule out loud for `session:`, `recording:` and `files:` — "these
are keeper's own bookkeeping rather than anything the writer typed", so they trail the facts about
the session. `recordings` is keeper's own too. Putting it first would give the file's single
ordering rule an exception on its second application, which is how a convention stops being one.

Two smaller reasons point the same way. A truncated property row — Obsidian's inline preview, and
keeper's own chip rows — shows the head of the list, and the head should be the tag the user chose,
not the one keeper added. And prepending would make keeper's classification look like the writer's
first thought about their own recording, which is a small lie the note tells every time it is
opened.

**"Already carries it" is a question for the vocabulary, not for `==`.** `kind_tag` normalises both
sides and compares canonical forms. That is the difference between this story and the naive one: a
literal comparison would let `Recordings` on a session produce `[Recordings, recordings]`, which the
tree files under one node and the note's own chip row shows as two — precisely the near-identical
pair Story 42.5 was written to eliminate. What the user typed is left byte-identical; keeper simply
declines to say it a second time.

**The constant is a literal *and* goes through `normalise` anyway.** That looks redundant for
exactly as long as `normalise("recordings")` returns `"recordings"`, which is why
`the_kind_tag_is_already_what_the_vocabulary_makes_of_it` pins the two together. If a future rule
ever changed the canonical form of a bare word, this file would start emitting a tag the tree files
under a different name, and it would surface as a failing test here rather than as a duplicated
chip in somebody's sidebar. The vocabulary is the authority on what the tag *is*; the constant only
says which tag is meant.

**`kind_tag` returns `Option` rather than panicking on an unnormalisable constant.** `compose` is
total by contract (Story 42.4) — the one thing it may never do is refuse — so the unreachable branch
degrades to "no kind tag" rather than to a panic in the minute the note would have been written. The
branch is unreachable, and the test above is what keeps it that way.

**`tags:` is now unconditional, and two existing tests said the opposite.**
`a_session_with_no_participants_and_no_tags_omits_those_lines_entirely` and the tail of
`empty_tags_are_dropped_and_an_all_blank_tag_list_omits_the_line` both asserted the key was absent.
They were renamed rather than deleted: `participants:` is still an omit-do-not-label field and that
half still needs defending, and the all-blank list still must not produce a nameless entry — the
expected result is now `[recordings]` instead of nothing. Renaming was not cosmetic; a test whose
name says "omits the line" while asserting the line is present is worse than no test.

**`#recordings` is quoted on the way out, and that was already handled.** A session hand-tagged
`#recordings` keeps its `#`, and `needs_quotes` already refuses to emit a bare leading `#` because
`- #recordings` is a YAML comment, not a list entry. The stub stays valid in Obsidian without this
story adding a rule about it.

## Proof the tests fail without the change

`cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core --lib notes::recording_note` — 25
passed, 0 failed. Then six mutations, each applied alone, the suite run, the file restored. Every
one was caught, and by the test that was written to catch it:

| Reverted | Tests that failed |
|---|---|
| The kind tag is never added | `the_composed_stub_round_trips_every_key_through_the_notes_parser`, `the_kind_tag_follows_the_sessions_own_tags_and_changes_none_of_them`, `a_session_with_no_participants_omits_that_line_and_still_carries_its_kind_tag`, `empty_tags_are_dropped_and_an_all_blank_tag_list_leaves_only_the_kind_tag` |
| Presence decided by `tag == &kind` instead of `tags::normalise` | `a_session_that_already_says_recordings_in_any_spelling_gets_exactly_one` |
| `insert(0, …)` instead of `push` — the tag pinned first | `the_kind_tag_follows_the_sessions_own_tags_and_changes_none_of_them`, `the_composed_stub_round_trips_every_key_through_the_notes_parser`, `empty_tags_are_dropped_and_an_all_blank_tag_list_leaves_only_the_kind_tag` |
| The session's own tags emitted through `normalise` too | `the_kind_tag_follows_the_sessions_own_tags_and_changes_none_of_them`, `a_session_that_already_says_recordings_in_any_spelling_gets_exactly_one` |
| The append guarded by `if !tags.is_empty()` — an untagged session gets no `tags:` | `a_session_with_no_participants_omits_that_line_and_still_carries_its_kind_tag`, `empty_tags_are_dropped_and_an_all_blank_tag_list_leaves_only_the_kind_tag` |
| `RECORDINGS_TAG` changed to `"Recordings"` | all five above, plus `the_kind_tag_is_already_what_the_vocabulary_makes_of_it` |

**One honest gap, stated rather than papered over.**
`a_session_that_already_says_recordings_in_any_spelling_gets_exactly_one` does *not* fail when the
kind tag is removed entirely — a session that already carries the tag looks the same either way, so
that test defends the deduplication and nothing else. Presence is defended by the four other tests
in row one. Each test catches its own claim, which is the property worth having; a test that fails
for every mutation is not evidence about any of them.

**What could not be exercised here.** Nothing. This story is a pure function in `keeper-core`, its
risk is entirely in that function, and Linux runs it. The one thing downstream of it — the tag
reaching the vault's tag tree — happens through `tags::note_tags` on the ordinary note-indexing path
that every other note already takes, and this story adds no code to that path. The proof that the
emitted list resolves to one tree node is the `normalise_all` assertion inside
`a_session_that_already_says_recordings_in_any_spelling_gets_exactly_one`, which calls the same
vocabulary the index calls.

## What this story deliberately does not do

- **It does not make the tag the predicate.** `is_recording_note` still keys on `session:` alone. A
  tag is a thing a user may delete, and no note may stop being a recording note because somebody
  tidied their tag list.
- **It does not normalise the session's own tags.** That would silently rewrite the user's text and
  is explicitly out of bounds in Story 42.5's "the manifest keeps saying what the user typed".
- **It does not retro-tag existing stubs.** Notes already written are the user's files.
- **It does not add a `recordings/…` hierarchy.** One flat kind tag; a sub-hierarchy would be a
  second convention invented here, and nothing has asked for one.
- **It does not touch the Recordings lens, the sidebar or any surface.** The tag lands in the tree
  through the ordinary note-indexing path, because the stub is an ordinary note.
