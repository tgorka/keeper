# Story 45.19 — Recording Remembers the Last Session

Bindings: FR-197, UX-DR80. Frontend + `keeper` + `keeper-core`.

## What already existed (the "check first" answer)

Nine times this epic family the asked-for thing was already present. Here it is
**partly** true, and the split matters:

| Asked for | Found | Verdict |
|---|---|---|
| A form with every session field | `RecordingMetaCard` (21.5/22.3/42.5) renders Title, Participants, Note, Tags, custom rows | Present, but **only for the NEXT session** and welded into one card. Extracted into `RecordingMetaFieldSet` so a second host can mount it. |
| Editing the last recording's **title** | `recording_retitle` (40.4) + the summary card's inline editor | **Present and left alone.** A title MOVES the session; that decision, its ordinal walk, its unwind and its refusals already exist and are tested. |
| Editing participants / note / tags / custom on a finished session | **Nothing.** `SessionMeta` holds all four; nothing but `recording_start` ever wrote them | The one real write path this story adds. |
| Reading a finished session's meta | `recording_session_summary` returns title only | New read: `recording_session_meta`. |
| The trim/split/drop rules for the form | Inline in `recording_start` (`ipc.rs`), ~30 lines | **Moved wholesale** into `SessionMeta::from_input`. An edit path had nowhere else to get them, and two copies is how a field starts round-tripping differently per surface. |
| A `recordings` default space | `keeper_core::notes::default_spaces` (44.3), key `recordings` | Consumed. Identity is `defaultKey`, never the name. |
| Session → note resolution | `recording_note_targets` (42.4) already fetched by the properties panel | Consumed. W3NoteFile confirmed over `hub` that 45.18 does **not** build session→note; their `vault_link` is profile-path ↔ vault-path only. |
| Recorder start | `recording_start`, one call site (the Start button) | Untouched. Nothing this story adds calls it. |

## What was built

### Rust — `keeper-core` (compiles and tests on Linux)

- `SessionMetaInput<'a>` — the form as it crosses the wire, raw text.
- `SessionMeta::from_input(session_id, &input)` — **the one place** the emptiness
  rule (`clean_field`), the tag tokenisation (`tags::split_list`) and the
  nameless-custom-row drop live. `recording_start` now calls it.
- `SessionMeta::tags_line()` — the inverse join, so open→save is a fixed point.
- `SessionMeta::to_form_vm()` — absent → `""`, once, in Rust.
- `SessionManifest::edit_details(&input)` — rewrites participants/note/tags/
  custom on a finished session. Carries `meta.title` and `meta.session_id`
  through untouched; mints a `meta` block only when the edit says something.
- `RecordingSessionMetaVm` / `RecordingSessionMetaFieldVm` in `vm.rs`
  (non-`ts_rs`, twin declared in `client.ts`, following `RecordingSummaryVm`).

### Rust — `keeper` shell (NOT compiled here; see below)

- `recording_session_meta(folder) -> Option<RecordingSessionMetaVm>` —
  `Ok(None)`, never an error, for a folder with no loadable manifest.
- `recording_meta_update(folder, participants, note, tags, custom)` — claims the
  folder in the live-reservation set (the same compare-and-set the retitle uses),
  loads, applies `edit_details`, atomically writes, and answers **from the file
  it wrote**, not from the request. Both registered in `lib.rs`; both have
  `#[cfg(not(desktop))]` twins.
- `recording_start`'s meta block replaced by one `SessionMeta::from_input` call;
  the folder-name title is now read back off that block instead of re-derived.

### Frontend

- `RecordingMetaFieldSet` (`recording/recording-meta-fields.tsx`) — the five
  fields, `idPrefix`-scoped so two hosts can be mounted at once, `disabled`-able.
  `TagVocabularyInput` gained a `disabled` prop for the same reason.
- `RecordingSummaryCard` — the inline editor is now a **details** editor
  (`SUMMARY_RETITLE_LABEL` is "Edit details"; "Save name" → "Save details").
  Opens synchronously on the title, reads the rest lazily, per-field re-seeds
  untouched fields when they land. Save sends the details first and the title
  second.
- `lib/recordings-space.ts` — `useRecordingsSpace()` and
  `openRecordingsSpace(space)`.
- `RecordingPane` header — the way across to Notes, present only when linked.
- `PropertiesPanel` — `RecordAnotherLikeThis`, below the recording paths.

## I/O and edge-case matrix

| Input | Output |
|---|---|
| Edit participants/note/tags/custom, Save | `recording_meta_update(folder, …)`; `recording_retitle` NOT called |
| Edit title only, Save | `recording_retitle` only; no manifest write |
| Edit both | details write **then** rename, in that order |
| Rename refused (live session) | details already saved; Rust's sentence in the fault slot; editor stays open on the typed text |
| `tags` = `" Client/Acme , acme ,, "` | manifest gets `["Client/Acme","acme"]` — verbatim tokens, empties dropped |
| custom row with blank **name** | dropped |
| custom row with blank **value** | kept (a row being filled in) |
| every detail emptied | keys omitted from the wire entirely; `session_id` survives |
| pre-40.3 manifest, edit fills a field | `meta` minted |
| pre-40.3 manifest, edit fills nothing | no `meta` object serialized |
| `edit_details` with a `title` in the input | title **ignored**; the manifest's own is carried |
| session folder has no manifest | read → `null`; editor shows `SUMMARY_DETAILS_UNAVAILABLE`, four fields frozen, no write attempted; the title still goes through the rename path (which refuses on its own terms) |
| session is live | `recording_meta_update` refuses with `recordingSessionLive` |
| session renamed, then edited | reads and writes the folder it moved to |
| recovered card | same editor, that card's own folder |
| vault has a Recordings space | button, labelled with the space's **own** name |
| vault has a space *named* "Recordings" with `defaultKey: null` | no button |
| no active vault | no button, and no `notesSpaces` call |
| space list read fails | no button |
| button pressed while already scoped to that space | view switches; scope **not** cleared (`setScope` is a toggle) |
| "Record another like this" on a resolvable session | all five fields into `recordingMetaStore`; view → `recording`; **`recording_start` never called** |
| …then Start pressed | `recording_start` receives exactly those five values |
| note with no `session:` | button absent; `recording_note_targets` never called |
| session not on this machine (targets `null`) | button absent |
| manifest unreadable | `RECORD_ANOTHER_UNREADABLE` beside the button; nothing filled; still in Notes |

## Deliberately NOT done

- **The archive row is not rewritten by a meta edit.** Same decision 40.4 took
  for the retitle, same reason, quoted in the command's doc: Story 42.1's row
  carries a codec and a frame rate that exist in no manifest, and rebuilding one
  from what this edit knows would write nulls over them. Honest consequence: the
  recordings browser keeps searching the tags the session was *started* with
  until `archive::recordings::rebuild_from_disk` runs. Adding a narrow
  `record_meta` port to `ArchiveHandle` is the follow-up if that bites.
- **The note's frontmatter and the manifest are not unified.** 42.4 composes the
  note once, at finalize. After this story a recording's tags exist in two places
  that do not track each other: `manifest.meta.tags` (what the recorder was told,
  editable here) and the note's `tags:` (44.14 / 45.17). Already true before this
  story; flagged to W3TagsDelete over `hub`, and the write path is exported if
  they want to push back.
- **No "last recording" surface at idle.** "The last recording" is the completion
  / recovery card, which is what the Recording pane shows. A card for the most
  recent session on a cold launch would be a new surface, not this story.
- **`recording_retitle` was not folded into the new command.** Each field has
  exactly one write path; neither is a second answer to the other.

## Tests

Rust — `cargo test -p keeper-core --lib recording::` **EXIT=0, 195 passed**:
`edit_details_reaches_every_field_of_the_manifest_on_disk` (asserted on the file,
re-read after `write`, two tags and two custom rows),
`edit_details_clears_a_field_the_user_emptied`,
`edit_details_mints_a_block_only_when_the_edit_says_something`,
`the_tag_line_round_trips_through_split_and_join`.

TS — the acceptance command **run by name**,
`bun run test src/components/layout/ src/components/recordings/` plus this
story's files (`src/components/recording/`,
`src/components/notes/properties-panel.test.tsx`,
`src/lib/recordings-space.test.ts`): **EXIT=0, 38 files, 789 passed**, twice
consecutively, zero unhandled errors, zero `export is defined`.

Rust — the acceptance command run by name,
`cargo test -p keeper-core --lib` (whole crate): **6 failures, none in
`recording::`** — `capture::` (×2), `notes::vm::`, `registry::` and
`vault_link::` (×2), every one of them a sibling's in-flight wave-3 work, and
every one confirmed by name rather than assumed. The `recording::` filter is
**195/195, EXIT=0**, re-run after the sweep restored the tree.

### Mutation table

Harness in `~/.W3Recording/`, sentinel `MUTR4519_NN`, unique in both directions.
Two mutations run, both against `SessionManifest::edit_details`, both the ones
Main ruled too serious to leave owed. Restore verified by **sha256 against the
pre-mutation copy** (identical) **and** a literal `grep -F "MUTR4519"` across
`src` and `src-tauri` (zero occurrences) — not by `git diff` alone, because a
diff over a shared worktree is full of siblings' lines and, per W3Export, blind
to new files entirely.

| # | Mutation | Blast radius if it shipped | Verdict |
|---|---|---|---|
| `MUTR4519_01` | `let title = self.meta…title.clone()` → `None` | **Silent data loss on the headline action**: any details save clears the session's title, and because the title also names the folder, the next retitle re-renders it to the untitled path | **CAUGHT** (suite EXIT non-zero; the assertion that pins it is `title: Some("Retro")` in `edit_details_reaches_every_field_of_the_manifest_on_disk`) |
| `MUTR4519_02` | `let session_id = self.meta…session_id.clone()` → `None` | **Green but does nothing, with three surfaces behind it**: the session detaches from its archive row (42.1), its note stub's `session:` line (42.4) and its recovery latch (20.3/40.3) — all of which look up by id and simply find nothing | **CAUGHT** by `edit_details_reaches_every_field_of_the_manifest_on_disk` **and** `edit_details_clears_a_field_the_user_emptied` (2 failed / 193 passed) |

Two further mutations were run later, from peer shapes rather than from my own
list, and each pins an invariant that was correct and unguarded:

| # | Mutation | Blast radius if it shipped | Verdict |
|---|---|---|---|
| `MUTR4519_03` | `loadDetails` raises `detailsUnavailable` for every session, not only the unreadable one | **Every details editor frozen.** The values would still be RIGHT, so every `toHaveValue` assertion in the suite passes — a dead editor and a working one are the same DOM to a value assertion | **CAUGHT** by `opens the editor on what the session's manifest actually holds`, after W3TagsDelete's shape made me witness the readable case's *absence* of the unavailable line and assert the field is enabled |
| `MUTR4519_04` | the details write given its own `try`, so a refused details write is followed by the rename anyway | **A refused save that MOVES the session.** The user reads "the disk is full" while the recording quietly relocates — and this is the invariant the whole one-fault-slot design rests on | **CAUGHT** by the new `does not attempt the rename when the details write failed`, and by that test only |

Two more from W3Chrome's *camouflage* variant — **when you write the same
construct twice and pair only one of them, the unpaired one is hidden by the
paired one**, because a satisfied glance over the second occurrence is really
checking the first. Both are accessible relationships that render
byte-identically when broken:

| # | Mutation | Blast radius if it shipped | Verdict |
|---|---|---|---|
| `MUTR4519_05` | drop `id={faultId}` from the fault paragraph, leaving `aria-describedby` dangling | The refusal is still on screen and still found by every `getByTestId` — **the only thing lost is the announcement to the person who cannot see the red text.** Nothing about the DOM looks wrong | **CAUGHT** by the live-session refusal test, which now resolves the attribute to an element and reads its text |
| `MUTR4519_06` | `detailsPrefix` a constant instead of `useId()` | Two recovery cards open at once render two inputs under one id; every `<label for>` resolves to whichever the browser found first, so typing into the second card's Participants edits the label of the first. The card's own comment claimed per-card ids and **nothing enforced it** | **CAUGHT** by the new two-card test, which is also the first test in this story to render two cards — the ordinary shape of a scan that salvaged two sessions |

One more, from W3CaptureWindow's instance of the generalised camouflage shape —
**does this thing name something, and does anything check the thing it names
exists?**

| # | Mutation | Blast radius if it shipped | Verdict |
|---|---|---|---|
| `MUTR4519_07` | rename `key: "recordings"` in `keeper_core::notes::default_spaces` | **Three files name this identity** — Rust writes it into every seeded space's `keeper.default`, `recordings-space.ts` reads it back, `notes-pane.tsx` reads it for the empty state — and nothing made them agree. Rename it and NOTHING fails: the space still seeds, the sidebar still lists it, and the button this story adds simply stops appearing, for everyone, permanently, with no error anywhere | **CAUGHT** by a new cross-language pin in `recordings-space.test.ts` |

That pin is a TypeScript test over a Rust file, following
`capture-capability.test.ts` and `no-user-agent-gating.test.ts` — this repo's
idiom for an invariant that is about a file rather than a function. It also runs
where it is needed: the `keeper` shell does not compile on Linux, so a Rust-side
assertion would be prose on the machine most of this epic was written on.

`MUTR4519_04` is W2Media's third category — *correct code with an unpinned
invariant*. Nothing was broken; the two sequenced writes share one fault slot
safely **only because a failure of the first skips the second**, and until this
test the answer to "what stops that becoming false" was "the author was careful".

Three named mutations were **not** run and stay owed, said plainly rather than
dropped: `save()` reordered so the rename goes first; `openRecordingsSpace`'s
already-selected guard removed; `detailsChanged` compared against the seed
instead of `stored`. The last of these is the one the pre-test defect below was
about, so it has a test aimed at it; the first two do not.

## Shape audit

Applied while building, before any sweep. Findings:

1. **What composes the input?** — the editor's fields are composed by
   `loadDetails`, not by the test's props. Probed: the test that opens the editor
   asserts `recordingSessionMeta` was called **with the folder**, and asserts both
   custom rows arrive. A read of the wrong session fills the form with somebody
   else's meeting and looks entirely normal.
2. **Did anything press the button?** — every write test presses Save and asserts
   the CALL (`recordingMetaUpdate` with all five arguments), not just that an
   editor rendered. The "record another" test presses Start afterwards and asserts
   `recording_start`'s seventh argument.
3. **A contract stated in a doc comment and enforced nowhere.** — `setScope`'s
   toggle semantics are documented in `notes-filters.ts` and were about to be a
   silent bug: a "take me there" button pressed while already on that space
   clears the scope. Partitioned on the field that DRIVES behaviour (`scope.id`),
   with a test.
4. **A fallback for a case that cannot happen.** — Rust collapses absent → `""`
   once (`to_form_vm`), so no `?? ""` exists on the TS side for these five fields.
5. **Opaque-fixture check / assert the fixture before asserting what reading it
   produces.** No binary fixtures, but the shape applied in its textual form and
   found one unpaired negative assertion. `edit_details_clears_a_field_the_user_emptied`
   matched `"participants"`, `"note"`, `"tags"` and `"custom"` as literal JSON
   and asserted their ABSENCE after the clear — while the struct equality beside
   it compares fields, not text. Rename any of those four in serde and the
   absence check passes for the wrong reason and the equality never notices: a
   test that has silently stopped testing anything. Closed by reading the four
   keys OUT of the fixture's own `manifest.json` first, so the day a key is
   renamed the test fails loudly on the presence check.

   Following W2Media's version of the shape, the fix is a **seam rather than a
   comment** — a note saying "these are literal keys, keep them in step" would be
   read by nobody in a hurry. Every other negative assertion in this story was
   already paired with a positive that uses the same constant or the same mock
   (the absent-button tests share `RECORD_ANOTHER_TESTID` and
   `RECORDINGS_SPACE_TESTID` with the tests that assert the button IS there, and
   `expect(notesSpaces).not.toHaveBeenCalled()` is paired with
   `toHaveBeenCalledWith("v1")`), so a broken constant fails loudly on the
   positive side. That is pairing rather than design, and it is said as such.
6. **A branch reachable only from a second host.** — the details editor has two
   hosts: the completion card and the **recovered** card. A test drives the
   recovered one specifically, because every completion-card test would pass over
   a recovered-only break.
7. **Assert what you handed on, not only what came back** (Main/W2Media). —
   `RecordingMetaFieldSet` is handed `fields` + `onChange` + `disabled` by two
   hosts. Tests read values back out of the rendered inputs and drive `onChange`
   through real edits, and the unavailable case asserts the `disabled` prop
   actually reached the inputs (`toBeDisabled` on participants *and* tags).
8. **Two-item collections everywhere.** — every custom-row and tag fixture has
   two entries, in Rust and in TS. A mutant keeping only the first element fails
   `edit_details_reaches_every_field_of_the_manifest_on_disk`, the editor-opens
   test, and the record-another test.
9. **`await` is not a success check when the callee catches its own failure**
   (W3NoteFile). Checked, clean: `recordingMetaUpdate`, `recordingRetitle` and
   `recordingSessionMeta` are bare `invoke` wrappers that reject — none of them
   swallows into a store, so every `await` and `.catch` here is a real check.
10. **When you replace a mechanism, the contracts the old one kept are not in
    the diff** (W3Capture). This story *replaced* the title-only rename editor
    with a details editor, so the check applies. What the old one promised, and
    where each promise now lives: the editor stays OUTSIDE the card's
    `role="status"` live region (a text field inside an aria-atomic region
    re-announces the whole card on every keystroke) — carried, and **pinned** by
    the pre-existing `keeps the rename editor out of the live region and carries
    focus with it`; focus follows the affordance↔field swap in both directions —
    carried, same test; a refusal retracts as the user edits toward a correction
    — carried, and extended to the four new fields; every control freezes while a
    write is in flight — carried; Save is inert when nothing would be sent —
    carried and generalised from "the title is unchanged" to "neither half
    changed"; and the commit affordance is never bare "Save", which would read as
    the recording being saved, which happened already — carried as "Save details".
11. **Which references have a query-shaped witness, and which do not**
    (W3Chrome's closing refinement: *when the checking form and the convenient
    form are the same form, nobody has to remember* — with their own caveat that
    the standard is right and not always reachable). This story's seven
    references, classified rather than assumed:

    - **Reachable, witnessed for free.** The five `htmlFor`/`id` pairs in
      `RecordingMetaFieldSet` and the title field's `aria-label`: `getByLabelText`
      cannot match unless the reference resolves, and it is how every test in this
      story reads a field anyway. Nobody has to remember.
    - **Reachable, and upgraded because of this shape.** The title field's
      `aria-describedby`. The first fix read the attribute and looked the id up by
      hand — the forgettable form. `toHaveAccessibleDescription` computes the
      description THROUGH the reference, cannot pass while it dangles, and is
      shorter than the version it replaced. Re-probed as `MUTR4519_08`: **CAUGHT**.
    - **Forgettable, and no better form exists.** `detailsPrefix`'s per-card
      uniqueness (no query can see an id collision — only rendering two cards can)
      and the cross-language `"recordings"` key (a contract spanning a Rust source
      file and a TypeScript module has no witness but reading both). For these the
      extra assertion is not laziness, it is the state of the art — and the
      cross-language one runs on Linux, which is the half the shell crate cannot
      give this box.

12. **The camouflage variant** (W3Chrome). Two unwitnessed accessible
    relationships found and closed — see `MUTR4519_05` and `_06`. The tell in
    both cases was that the construct existed and the *target* of the construct
    did not have to: an attribute pointing at nothing, and an id that need not be
    unique. Neither is visible in a render assertion.
13. **Count the doors.** — this story has four: the completion card, the recovered
   card, the Recording pane header, and the note's properties panel. Test counts:
   7 / 1 / 4 / 4. The recovered card has one, deliberately targeted at the thing
   that differs (the folder it writes to); everything else about that editor is
   the same component.

### A real defect the audit found, before any test existed

The first shape of the editor landed the stored baseline in state (`setStored`)
and re-seeded the fields from a `useEffect`. That left **one render** in which the
baseline said "the manifest has participants" while the fields still said `""` —
and in that render Save was **enabled over an empty form**. Pressing it would have
written the empty seed over the stored details: the exact edit nobody asked for,
at the story's headline feature. Found because
`re-seeds the editor when the title lands` failed intermittently on
`toBeDisabled`, and the honest reading was not "flaky test" but "there is a window
where the two disagree". Fixed by setting the fields and the baseline in the same
update inside `loadDetails`; the effect is gone. The comment on `loadDetails`
records it.

## What I could NOT verify here, and why

- **The `keeper` shell crate does not build on Linux.** `recording_session_meta`,
  `recording_meta_update`, their mobile twins, the `lib.rs` registrations and the
  `recording_start` refactor have **never been compiled**. Every core function
  they call is compiled and tested. First checks on the macOS gate:
  (a) `cargo check -p keeper` — the `recording_start` edit changed a binding's
  provenance (`title` now comes off `session_meta`) and `meta_custom` from moved
  to borrowed; (b) record something, press **Edit details**, change participants
  and a tag, Save, and read `manifest.json` — that is the story's headline
  promise and no test on this box has touched a real manifest through IPC;
  (c) rename **and** edit details in one Save on a live session and confirm the
  details landed while the rename was refused by name.
- **Only two of five named mutations were run**, the two Main ruled too serious
  to leave owed; both were caught. The three TS-side mutations listed above stay
  owed. No sweep ran against the frontend at all, so the TS reversion evidence is
  the inline kind: the empty-form-over-stored-details defect below was found and
  closed by a test that fails without the fix.
- **`useRecordingsSpace`'s hydration leg** is exercised only through a
  pre-hydrated store in tests; `ensureNotesVaultsHydrated`'s own path is 37.x's
  and tested there.
- **The command registration has no witness, and this is the gap I would close
  first.** Found from W2Media's protocol-handler defect, minutes before the wave
  was called; **identified, not closed.** `recording_session_meta` and
  `recording_meta_update` are named in three places — `client.ts` invokes the
  string, `ipc.rs` defines the command, `lib.rs` lists it in
  `generate_handler!`. Drop the third and **nothing here fails**: the functions
  still exist so `cargo check` is clean, the frontend composes the same invoke so
  every mocked test passes, and at runtime every "Edit details" and every Save
  rejects with "command not found". The whole story, invisible, with a green
  tree. It is the same shape as W2Media's unregistered protocol handler and
  W3CaptureWindow's `capture.html` named twice: **before this story there was one
  namer and nothing to disagree; adding the second and third created the
  hazard.**

  The remedy is theirs, and it is a dozen lines: a TypeScript test that reads
  `lib.rs` and pins `ipc::recording_session_meta,` and `ipc::recording_meta_update,`
  against the literals `client.ts` invokes — the `capture-capability.test.ts` /
  `file-scheme-registration.test.ts` idiom, which exists precisely because the
  `keeper` shell does not compile on Linux and a Rust-side assertion would be
  prose on the machine that needs it. **Owed, and named as the first thing to add
  in this area.** Until it exists, `cargo check -p keeper` followed by pressing
  Edit details once on the gate is the only thing standing between this story and
  a silent no-op.

- **No pixel drawn.** jsdom renders no layout; the header button's placement and
  the editor's density are unverified visually.
