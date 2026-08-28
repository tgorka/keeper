---
title: 'Story 44.2: The Stub Embeds Its Own Videos'
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
  - '{project-root}/_bmad-output/implementation-artifacts/spec-42-4-the-note-stub.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-43-5-one-attachment-vocabulary.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-43-7-the-attachment-panel.md'
---

<intent-contract>

## Intent

**Problem:** a recording stops, keeper composes a note, and the note is blank. `files:` lists every
segment in frontmatter — a machine's list, rendered by the properties panel as rows of text — and
the body says `# Weekly sync` and nothing else. Story 43.5 taught the body to turn `![[…]]` into a
player and Story 43.7 gave a panel that writes one at the caret, so the two halves have existed
since epic 43; the only thing joining them is the user, pressing a button per track, in the one
minute they were supposed to be writing about the meeting. The note that opens on the recording you
just made shows you nothing of it.

**Approach:** `compose` writes the session's videos into the body as ordinary Obsidian embeds, one
per line, directly under the heading, in the ledger's order. Nothing else about the stub changes.
The embed is the `files:` entry verbatim — the same string, in the same relative-to-the-destination-
root frame — so no path is composed, joined or re-derived anywhere.

## Boundaries & Constraints

**Always:**
- **The heading is the first line of the body.** `notes_vault::note_title` falls back to the body's
  first line, so anything above the heading becomes the note's displayed title. This is not
  hypothetical: 43.7's panel inserts at the caret, a caret at offset zero put an embed above
  `# Title`, and the owner's vault now contains a note called `![[recordings/…/screen-0000.mov]]`.
- Videos only, and decided by Story 43.5's `kind_for_file_name` — no second extension table.
  `manifest.json` and `events.log` are already reachable through `files:` and 43.7's panel, and an
  embed of either is a chip showing nothing where the recording should be.
- The embed string is the `files:` string, byte for byte. FR-145 holds in the body for the same
  reason it holds in the block: the composer is never handed an absolute path, and it joins nothing
  (the backend analogue of AD-65).
- Ledger order, never a sort. Sorting would lift `camera-0000.mov` above the screen segment it was
  recorded beside, and 44.1 reads the pair as one player.
- A session with no video gets the body it got before this story, byte for byte. No embed block
  means no separator line either.
- The stub stays a note Obsidian renders: `![[…]]` and nothing else. No marker, no attribute, no
  wrapper — byte-identical to what 43.7's panel writes and what `recording-embed.ts` recognises.
- `compose` stays total (Story 42.4's promise). No input makes it refuse.

**Never:**
- No new dependency, no new module, no new IPC, no binding change. `NoteStub` keeps its shape.
- No second embed syntax and no markdown `![](…)` variant.
- No listing of the session folder. `facts.files` is what the note knows; a directory read here
  would be a file browser, and 43.8 is the file browser.
- No retro-embedding into stubs already on disk. Those are the user's files now.
- No change to `kind_for_file_name`, `links.rs`, `naming.rs` or any frontend surface.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| Two tracks | `screen-0000.mov`, `camera-0000.mov`, `manifest.json` | body is `# T\n\n![[…screen]]\n![[…camera]]\n\n` — both, in ledger order, under the heading | none |
| The manifest | `manifest.json` in `files:` | listed, never embedded | none |
| The events log | `events.log` in `files:` | listed, never embedded | none |
| Audio only | `mix-0000.m4a`, `manifest.json` | body is exactly `# T\n\n` — no embed, no extra blank line | none |
| No files at all | `files: []` | body is exactly `# T\n\n`; no `files:` key either | none |
| Image | `whiteboard.png` | listed, not embedded — 43.5 calls it `Image`, and only `Video` reaches the body | none |
| Uppercase extension | `camera-0000.MOV` | embedded; a file copied in from another machine is the same video | none |
| Double extension | `screen-0000.mov.bak` | not embedded; the last extension decides, so a backup gets no broken player | none |
| Blank entry | `["…mov", "   ", "…json"]` | the blank produces no line in `files:` and no embed | none |
| Name a wikilink cannot carry | `take [2].mov`, `a\|b.mov`, `take #3.mov`, a name with a newline | listed in `files:`, not embedded; the body gains no line | none |
| Title hijack | any of the above | `naming::title_from_body(body)` is the heading, never an embed | none |
| Untitled session | `title: None`, two tracks | heading is the date; still the title, still above the embeds | none |
| Caret hint | any | `body_offset` is the body's first byte — the heading; the writer's line is the end of that slice, below the embeds | none |
| Non-ASCII title | `Café résumé` | `body_offset` is a byte index that still slices on a character boundary | none |
| Round trip | any | `Frontmatter::parse` reads every key back and `links::extract` reads both embeds back | none |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/notes/recording_note.rs` — the story.
  - `compose`: the `files` pass now collects trimmed `&str` once and feeds both the `files:` list and
    the body, so the two can never name different strings.
  - `video_embeds`, new and private: the embed block, or an empty string.
  - `wikilink_can_name`, new and private: whether `![[file]]` would still mean this file.
  - `NoteStub::body_offset`'s doc, which now says what the offset is *not* and why moving it would
    be a mistake.
  - the module doc, which gained the section explaining why the body embeds and the block lists.
  - eight new tests.
- `src-tauri/crates/keeper/src/ipc.rs` — two assertions, no production code. Both pin the stub's
  whole body at the seam that actually writes a file to disk, and both sessions close `.mov`
  segments, so both now read the embeds. Expected values updated *and* their messages rewritten:
  prose saying the body is a heading and a blank line would be a stale doc comment sitting beside
  correct code.

Nothing else. `compose_stub` already passes `files`; no binding changes because `NoteStub` and
`RecordingNoteStubVm` are unchanged in shape.

## Tasks & Acceptance

**Execution:**
- [x] Embeds composed into the body, below the heading, in ledger order.
- [x] Videos selected by 43.5's `kind_for_file_name`, not by a table written here.
- [x] Empty embed block emits nothing at all — no orphan separator line.
- [x] Names Obsidian's wikilink grammar would eat are listed but not embedded.
- [x] `body_offset` decided, documented against the failure the alternative causes, and asserted.
- [x] Two `keeper` shell assertions updated to the new whole-body truth.
- [x] Tests: every matrix row, each proven by reverting the change it defends.

**Acceptance Criteria:**
- A two-track session's stub carries both embeds under the heading, in ledger order.
- The heading remains the note's title, asserted through the fallback the vault actually uses.
- A session with no video embeds nothing and gains no stray blank line.
- The body stays valid Obsidian markdown, read back by the vault's own link parser.

## Design Notes

**Where the body's first insertion point is, and why it is below the heading.** The body begins at
`front.len() + 1` — one past `Frontmatter::parse`'s offset, which lands on the blank line separating
the block from the prose. The first byte of the body is therefore the `#` of the heading, and the
first place anything may be *inserted* is after `# {title}\n\n`. That is where the embeds go.

The reason is `notes_vault::note_title`: it prefers frontmatter `title`, and failing that returns
`naming::title_from_body`, which is the body's first non-empty line with ATX markers stripped.
An embed written at offset zero *is* that first line, so the vault names the note
`![[recordings/…/screen-0000.mov]]` — which is exactly what 43.7's caret-at-zero insertion did to
the owner's vault. A composer that wrote embeds above the heading would do it to every recording
note ever written, silently, on a path nobody re-reads.
`the_heading_is_still_the_title_under_the_rule_the_vault_falls_back_to` asserts this through
`naming::title_from_body` itself rather than through a `starts_with('#')` of its own, because a rule
a test restates is a rule the test can be wrong about.

**`body_offset` stays on the heading, and the writer's caret is below the embeds anyway.** The story
asked where the caret hint should land now that the body is no longer just a heading. The answer is
that `body_offset` should not move, and the reason is what actually consumes it.

`recording-note-stub.tsx` slices the file at this offset: `contents.slice(0, bodyOffset)` is a head
it renders nowhere and never edits, `contents.slice(bodyOffset)` is the textarea's value, and the
caret goes to `value.length`. So the person's caret is the *end* of the editable half — which, now
that the embeds are in it, is the blank line under the last embed. The behaviour the story wants
falls out of putting the embeds in the body; nothing about the offset needs to change to get it.

Moving `body_offset` past the embeds looks like a better caret hint and is a defect. It would move
the embeds into the read-only head, making the one thing keeper just wrote into someone's note the
one thing they cannot delete. It would also disagree with `RecordingNoteStubVm::body_offset`, which
the shell recomputes from `Frontmatter::parse` against the file *on disk* — two same-named offsets
meaning two different positions, differing only for stubs with video, which is a split that goes
wrong quietly.

**Below the embeds, not above them, is the right place to type.** Both are real choices. Below wins
for the reason `recording-note-stub.tsx` already gives for its caret: keeper's prefill is context
keeper wrote and the user's sentence goes after it. Above would mean the first keystroke pushes the
recording down the page, and a note that stops opening as the recording after one word is not the
feature this story shipped. It also gives the finished note the order a reader expects — what this
is, then the recording, then what was said about it.

**The trailing blank line is the writer's line, not decoration.** With embeds the body ends
`…]]\n\n`; without them it ends `# T\n\n`, exactly as Story 42.4 left it. The blank is where the
caret lands. `video_embeds` appends its separator only when it emitted something, because a stub is
the one note nobody proofreads before saving and a stray blank line would end up in every recording
note a person owns.

**One pass over `facts.files`, not two.** The trimmed, non-empty list is collected once and used for
both `files:` and the embeds. Two independent passes would be two places to add the next filter to
and one place to forget it, and the failure mode is a note whose body shows a file its own
frontmatter does not list.

**`wikilink_can_name` is a refusal, not a sanitiser.** Obsidian's wikilink grammar consumes `]`,
`|`, `#` and `^`, and a newline ends the link outright. Each is legal in a macOS filename. An embed
built from such a name points at a shorter path that does not exist — a broken player where the
recording should be — and a newline would additionally put a line into the note that keeper did not
write. There is no escape to reach for: the honest answer is not to write the link. The file stays
in `files:`, so it stays one press away in 43.7's panel and one row away in the properties panel;
keeper only declines to write a link it already knows is wrong.

**The video question is asked of 43.5, and the test cross-checks the answer rather than restating
it.** `only_the_files_43_5_calls_video_are_embedded` pins the two extensions it expects *and* then
loops every file asserting `embedded == matches!(kind_for_file_name(file), Video)`. If a future
extension joins the video table, the attachment panel and the stub body move together or this test
fails; they cannot drift.

## Proof the tests fail without the change

`cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core --lib notes::recording_note` — 32
passed, 0 failed (25 before this story). Then seven mutations, each applied alone, the suite run,
the file restored byte-identical and re-run green. Every mutation was caught, and by the test
written to catch it:

| Reverted | Tests that failed |
|---|---|
| Embeds emitted **above** the heading | `the_heading_is_still_the_title_under_the_rule_the_vault_falls_back_to`, `a_two_track_session_embeds_both_videos_under_the_heading_in_ledger_order`, `the_body_offset_is_the_heading_and_the_writers_line_is_below_the_embeds`, `a_name_a_wikilink_cannot_express_is_listed_but_never_embedded` |
| No embeds at all — the 42.4 body | `a_two_track_session_embeds_both_videos_under_the_heading_in_ledger_order`, `the_embeds_read_back_as_embeds_naming_the_files_key_byte_for_byte`, `only_the_files_43_5_calls_video_are_embedded`, `the_body_offset_is_the_heading_and_the_writers_line_is_below_the_embeds`, `a_name_a_wikilink_cannot_express_is_listed_but_never_embedded` |
| Files sorted before embedding | `a_two_track_session_embeds_both_videos_under_the_heading_in_ledger_order`, `the_embeds_read_back_as_embeds_naming_the_files_key_byte_for_byte`, `only_the_files_43_5_calls_video_are_embedded`, `the_body_offset_is_the_heading_and_the_writers_line_is_below_the_embeds` |
| Trailing blank line appended unconditionally | `a_session_with_no_video_embeds_nothing_and_gains_no_blank_line`, `the_composed_stub_round_trips_every_key_through_the_notes_parser`, `an_untitled_session_is_titled_by_its_date_and_never_left_headingless`, `a_title_full_of_yaml_punctuation_still_round_trips`, `the_body_offset_is_a_byte_index_that_survives_a_non_ascii_title` |
| Video filter dropped — every file embedded | `only_the_files_43_5_calls_video_are_embedded`, `a_session_with_no_video_embeds_nothing_and_gains_no_blank_line`, `a_two_track_session_embeds_both_videos_under_the_heading_in_ledger_order`, `the_embeds_read_back_as_embeds_naming_the_files_key_byte_for_byte`, `the_body_offset_is_the_heading_and_the_writers_line_is_below_the_embeds` |
| `wikilink_can_name` filter dropped | `a_name_a_wikilink_cannot_express_is_listed_but_never_embedded` |
| `body_offset` moved past the embeds | `the_body_offset_is_the_heading_and_the_writers_line_is_below_the_embeds`, `the_heading_is_still_the_title_under_the_rule_the_vault_falls_back_to`, `the_composed_stub_round_trips_every_key_through_the_notes_parser`, `the_body_offset_is_a_byte_index_that_survives_a_non_ascii_title`, `a_two_track_session_embeds_both_videos_under_the_heading_in_ledger_order`, `a_session_with_no_video_embeds_nothing_and_gains_no_blank_line`, `a_name_a_wikilink_cannot_express_is_listed_but_never_embedded` |

The two that matter most are single-claim: `wikilink_can_name` is defended by exactly one test, and
the title hijack — the failure this story exists not to reproduce — is caught by
`the_heading_is_still_the_title_under_the_rule_the_vault_falls_back_to` through the vault's own
fallback function.

## What could not be proved here, stated plainly

This story's ledger lesson is that the risk lives in the impure shell, and here it partly does.

- **The two `keeper/src/ipc.rs` assertions are unverified on this machine.** The `keeper` shell crate
  does not build on Linux. Those two tests are the only end-to-end proof in the repo that a real
  finalize writes a real stub to a real disk, and they now assert the whole body including the
  embeds — but their expected strings were *derived* by reading `close_segment` (which writes
  `screen-{index:04}.mov`), `stub_files` and `relative_session_path`, not by running them. If the
  anchor resolves differently than read, those two assertions fail on the macOS gate and the fix is
  the expected string, not the composer. Both files were parse- and format-checked with `rustfmt
  --check` on a copy, which passes.
- **Obsidian was not run.** "This renders" rests on the syntax being byte-identical to what 43.7's
  panel already writes and 43.5's `recording-embed.ts` already recognises, and on `links::extract` —
  the vault's own reader, which skips code fences and inline spans — agreeing that both lines are
  embeds naming the `files:` entries. That is a real reader, not a `contains`, but it is keeper's
  reader and not Obsidian's.
- **The stop surface's caret was not driven.** `recording-note-stub.tsx` places it at
  `value.length`; that was read, not exercised. What is asserted here is the Rust-side half that
  makes the claim true: the embeds are inside the slice taken from `body_offset`, and that slice
  ends `…camera-0000.mov]]\n\n`. Driving the textarea belongs to a frontend test and this story owns
  no frontend file.
- **A real two-camera session was not recorded.** The ledger order asserted is the order
  `SessionFacts::files` arrives in, which `stub_files` takes from `manifest.segments`. That the
  manifest's segment order is the recording order is Story 41.5's property, not this one's.

## What this story deliberately does not do

- **It does not embed `manifest.json` or `events.log`.** Both are in `files:` and both have a working
  row in the attachment panel. An embed of either is a chip that shows nothing in the place a person
  is looking for the recording.
- **It does not re-classify anything.** `kind_for_file_name` is 43.5's and stays 43.5's; this module
  reads its answer and adds no extension of its own.
- **It does not touch existing notes.** No migration, no retro-embed. Stubs already on disk are the
  user's files.
- **It does not lay out the pair.** Two embeds on two lines; whether they render side by side under
  one transport is Story 44.1, in the frontend.
- **It does not sanitise a filename.** A name a wikilink cannot carry is skipped, never rewritten —
  renaming someone's file to make a link work is a much larger and much worse feature.
- **It does not move `body_offset`,** and the reason is in the Design Notes rather than in an
  omission: moving it would make the embeds undeletable and would desynchronise two same-named
  offsets across the crate boundary.
- **It does not add a caption, a heading or a "Recording" section above the embeds.** The heading is
  the note's title and the next thing is the recording; anything between them is text keeper wrote
  in the space the writer was about to use.
