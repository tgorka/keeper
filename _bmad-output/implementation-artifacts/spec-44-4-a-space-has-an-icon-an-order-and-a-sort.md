---
title: 'Story 44.4: A Space Has an Icon, an Order and a Sort'
type: 'feature'
created: '2026-08-09'
status: 'review'
blocking_condition: ''
baseline_revision: 'feat/epic-44-wave-1'
final_revision: ''
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/epic-44-the-vocabulary-is-the-space.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-43-4-spaces-you-can-edit.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-44-3-default-spaces-replace-the-fixed-rail.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-44-5-a-note-has-an-order-you-can-see.md'
---

<intent-contract>

## Intent

**Problem, and it is bigger than the story title suggests.** A space has carried a `sort` since
Story 37.4. `space_def` parses it, `NoteSpaceVm` ships it, Story 43.4's editor round-trips it — and
**nothing has ever read it**. `project_list` ordered every list, space or not, by one hard-coded
comparison: pinned first, then most-recently-modified. So a space saying `sort: created asc` was a
value that travelled a full round trip through four layers and changed nothing on screen. The icon
set was ten glyphs, four of which Story 44.3's defaults had just claimed, and there was no way to
position a space in the rail at all.

**Approach.** Three keys under `keeper:` — `sort`, `order`, `icon` — become things that work.

The ordering rule goes into a new `keeper-core::notes::sort`, not into the shell. `notes_ipc.rs` does
not build on Linux (AD-55/AD-56), and an ordering is exactly the kind of rule that is subtly wrong in
a way a reader cannot see. The shell's job shrinks to handing text over and putting the answer where
it goes. `created` and `modified` resolve through `query::resolve_date` — the DSL's own chain, now
public — so a space sorted by `created` and a space filtered by `date:created` cannot answer
differently about the same note. `order` defers whole to Story 44.5's `order::cmp_order`. `name`
folds through `search::fold_cmp`, the app's one alphabet.

### What was established before anything changed

| Fact | Where | Consequence for this story |
|---|---|---|
| `project_list` sorts by `list_order` unconditionally — pinned, then `updated_ms`, then path | `notes_ipc.rs:389` | the space's `sort` is dead; making it live is most of this story |
| `space_def` defaulted `sort` to the string `"modified desc"` when the key was absent | `notes_ipc.rs` | "absent" and "explicitly modified desc" were indistinguishable downstream, so they must resolve to one behaviour |
| `notes_spaces` sorted the rail by `name` alone | `notes_ipc.rs:965` | an `order` default of 0 with a name tie-break leaves today's rail byte-identical |
| The four seeded defaults are alphabetical (Inbox, Journal, Pinned, Recordings) and carry no `order` | `default_spaces.rs:84-115` | which is why the default of 0 was chosen rather than a seeded sequence |
| `keeper:` is spliced **whole** on save; a key the request omits is deleted | `notes_space_save` (44.3) | `order` had to join the request, not be carried through — only `default` is carry-through |
| A recording note's instant is *two* keys: `date` (local calendar day) and `start` (clock) | `recording_note::compose` | there is no single stored instant, so `recorded` composes them |
| `SESSION_KEY` non-blank is the one definition of "this note is about a recording" | `recording_note::is_recording_note` | `recorded_ms` makes exactly that test, over the index's projection of the same key |
| `IndexEntry` order is Story 44.5's `NoteOrder`, compared by `order::cmp_order` | `notes/order.rs` | `sort: order` calls it; there is no second comparator |

## Boundaries & Constraints

**Always:**
- The ordering rule lives in `keeper-core`. The shell hands over text and carries back an answer.
- One reading of "what is a stored timestamp": `query::stamp_ms`, which `date:` already used.
- One alphabet: `search::fold_cmp`.
- One definition of a note's own order: `order::cmp_order`, Story 44.5's.
- A value keeper cannot read produces a sentence, composed in Rust, that a surface shows.
- The tie-break is `path` ascending **whatever the direction is**, and it is never the only term —
  `notes_vault` holds entries in a `HashMap`, so there is no input order to fall back on and a
  comparator returning `Equal` for two distinct notes lets hash order through into the list.

**Never:**
- No sort vocabulary in TypeScript. The form renders `sortEffective`; it never parses `sort`.
- No second `note_order`. The first draft of `sort.rs` had one; it was deleted when 44.5 landed.
- No `order: 0` stamped into a file. Zero *is* unpositioned.
- No refusal of a list because a presentation key is unreadable. The space still selects what it
  selects.

**Block If:** nothing. Every wire change is additive and every stored value keeps its old meaning.

## I/O & Edge-Case Matrix

### The sort, read (`sort::read`)

| Scenario | Input | Ordering | Warning |
|---|---|---|---|
| No key at all | `""`, `"  "` | `modified desc` | none — never naming a sort is not a mistake |
| Both words | `"created asc"` | `created asc` | none |
| Bare key | `"name"` | `name asc` | none |
| Bare key, the other half | `"created"` | `created desc` | none |
| Hand-spelled | `"  Modified   DESC  "`, `"NAME ascending"` | as written | none |
| Unknown word | `"bananas"` | `modified desc` | `keeper doesn't know the sort "bananas", so this space is sorted by modified, newest first.` |
| Known key, unknown direction | `"modified sideways"` | `modified desc` | quotes the **whole** value |
| Three words | `"modified desc extra"` | `modified desc` | quotes the whole value |
| Direction alone | `"desc"` | `modified desc` | quotes it |
| 4 KB of noise | `"xxxx…"` | `modified desc` | clipped to 48 chars + `…` |

### The sort, applied (`sort::compare`)

One fixture, four notes, five different answers. A fixture where two sorts agree proves neither.

| note | order | title | created | modified | recorded |
|---|---|---|---|---|---|
| `a.md` | 1 | Bravo | 03-01 | 01-01 | session 06-10 09:00 |
| `b.md` | 2 | Alpha | 01-01 | 03-01 | *none* |
| `c.md` | 3 | Delta | 05-01 | 04-01 | session 02-01 08:00 |
| `d.md` | 4 | Charlie | 04-01 | 05-01 | *none* |

| Sort | Result | Reverse |
|---|---|---|
| `order asc` | a b c d | — |
| `name asc` | b a d c | `name desc` → c d a b |
| `created desc` | c d a b | `created asc` → b a d c |
| `modified desc` | d c b a | `modified asc` → a b c d |
| `recorded desc` | a d c b | `recorded asc` → b c d a |

All five are distinct, and the test asserts that distinctness explicitly rather than leaving it to
the reader. `c.md` is the load-bearing row: a real February recording sorting **below** `d.md`, an
ordinary April note, is only correct under the stated stampless rule. Both "stampless to one end"
readings give a different answer.

| Scenario | Expected |
|---|---|
| Two notes the key cannot tell apart | `path` ascending, in both directions |
| A pinned note in a space sorted `modified desc` | stays at the bottom; pins do not float inside a space |
| A pinned note in the plain, space-less list | still first — `list_order` is unchanged |
| A space whose `sort` is unreadable | list runs, ordered by the default |

### `recorded`, and the note with no session (`sort::recorded_ms`)

| Scenario | Frontmatter | Result |
|---|---|---|
| A stub with both stamps | `session`, `date: 2026-02-01`, `start: 08:30` | that minute |
| A stub with no clock | `session`, `date` | midnight of that day |
| A clock nobody can read | `session`, `date`, `start: half eight` | the day — a session with an unreadable minute still happened |
| A day nobody can read | `session`, `date: soon` | `None` |
| A journal note with a `date` | `date`, no `session` | `None` — it was not recorded |
| A blank `session:` | `session: "   "` | `None` — the one state nothing can resolve, `is_recording_note`'s own test |
| Any note answering `None` | — | sorted by its `created` date, **interleaved** |

### The rail position (`sort::read_order`, `sort::rail_order`)

| Scenario | Input | Position | Warning |
|---|---|---|---|
| No key | absent, `""`, `"   "` | 0 | none — a cleared field is absent, not broken |
| A number | `order: 2` | 2 | none |
| A quoted number | `order: "3"` | 3 | none |
| A fraction | `order: 1.5` | 1.5 — slots between 0 and 2 | none |
| A negative | `order: -1` | −1 — floats above the unpositioned block | none |
| A word | `order: first` | 0 | `keeper doesn't know the position "first", so this space sits where an unpositioned one does.` |
| Not finite | `inf`, `NaN` | 0 | as above |
| A list | `order: [a, b]` | 0 | as above, flattened to `"a b"` — a newline in a sidebar sentence is two half-sentences |
| Four unpositioned defaults | — | Inbox, Journal, Pinned, Recordings | none |

### The editor and the rail

| Scenario | Expected |
|---|---|
| Opening a space | Sort by / Direction show `sortEffective`, split; Rail position shows `order`, blank at 0 |
| Opening a space stored `bananas` | Sort by shows **Modified**, Direction **Newest first**, and Rust's sentence is listed |
| Saving that space untouched | writes `modified desc` — a repair the user watched, not a rewrite |
| Changing the sort | writes `<key> <dir>` |
| Changing the key | direction is **not** reset; its labels follow the key, so nothing is hidden |
| Direction labels | `name` → A to Z / Z to A; the dates → Oldest / Newest first; `order` → Lowest / Highest first |
| Choosing `recorded` or `order` | one line under the control states the rule; the other three keys have none |
| Rail position emptied | saves 0, never a refusal and never `NaN` |
| Rail position `1.5`, `-1` | saved as given |
| Icon picker | 24 glyphs + No icon, including all four the seeded defaults ask for |
| A row whose space has warnings | subtitle `Some of this space's settings can't be read`, in the accessible name, whole sentence on `title` |
| A row that is both broken and misread | the parse failure wins the one line — it means the space selects *nothing* |
| A clean row | no subtitle, no `title` |
| The rail | renders in the order Rust handed it, never re-sorted client-side |

</intent-contract>

## Code Map

- `keeper-core/src/notes/sort.rs` — **new.** `SortKey`, `SortDir`, `SpaceSort`, `DEFAULT_SORT`,
  `StoredSort`, `read`, `compare`, `recorded_ms`, `StoredOrder`, `read_order`, `rail_order`,
  `DEFAULT_SPACE_ORDER`.
- `keeper-core/src/notes/mod.rs` — one module line.
- `keeper-core/src/notes/query.rs` — `DateField` and `resolve_date` made public; `stamp_ms` extracted
  from `field_ms`, which now calls it. No grammar change, no behaviour change.
- `keeper-core/src/notes/vm.rs` — `NoteSpaceVm` gains `sort_effective`, `warnings`, `order`;
  `NoteSpaceReq` gains `order`.
- `keeper/src/notes_ipc.rs` — `SpaceDef` gains `order` and `warnings`; `space_def` reads
  `keeper.order` and collects both warnings; `space_query` becomes `space_lens`, returning the query
  *and* the ordering from one read; `project_list` applies it; `notes_spaces` projects the three new
  fields and sorts the rail by `rail_order`; `notes_space_save`'s `with_icon` becomes
  `with_presentation` and writes `order` when it is not zero.
- `src/lib/ipc/gen/NoteSpaceVm.ts`, `NoteSpaceReq.ts` — regenerated by ts-rs.
- `src/components/notes/space-editor.tsx` — `SPACE_ICONS` widened to 24; `SPACE_SORT_KEYS`,
  `SORT_DIR_LABELS`, `SPACE_SORT_NOTES`, `SPACE_SORT_RECORDED_NOTE`; the two `<select>`s, the
  position input, the warning list, and `sort`/`order` in the request.
- `src/components/notes/space-list.tsx` — `SPACE_SETTINGS_SUBTITLE`, the subtitle precedence, the
  `title`, the accessible name.
- `src/hooks/use-notes-actions.ts` — `saveFilterAsSpace` sends `order: 0`.

## Tasks & Acceptance

**Execution:**
- [x] `keeper-core::notes::sort`: the vocabulary, the fallback, the comparator, the rail order.
- [x] `query::resolve_date` / `DateField` / `stamp_ms` made public rather than re-derived.
- [x] `sort: order` delegated whole to Story 44.5's `order::cmp_order` / `cmp_order_desc`.
- [x] The shell: read both keys, apply the sort, order the rail, write `order`.
- [x] The icon set widened to twenty-four, covering all four seeded defaults.
- [x] The editor: sort key, direction, rail position, the warning list, the two explanations.
- [x] The rail row: the visible fallback.
- [x] Bindings regenerated by ts-rs, never hand-written.
- [x] Every test proved by mutating the code it defends, watching it fail, and restoring.

**Acceptance Criteria:**
- `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core --lib notes::` — **299 passed**
  (20 of them `notes::sort`).
- `bun run test src/components/notes/space-editor.test.tsx src/components/notes/space-list.test.tsx`
  — **52 passed**.
- `bun run test src/components/notes/notes-pane.test.tsx` — **11 passed**, so the added VM fields
  broke no existing consumer.

## Design Notes

**The space's sort is the whole ordering, and pins no longer float inside one.** This is a
deliberate behaviour change and it is the one a reviewer should look hardest at. Before this story
every list — space or not — put pinned notes first. AD-81 says "an ordering the reader cannot
account for reads as randomness", and a sort with a hidden first term is exactly that: a user who
asked for `name asc` and got one note out of alphabetical order has no way to tell whether the sort
is broken or the note is pinned. So inside a space the sort is all of it. The plain, space-less list
is untouched and still puts pins first, because that rule was never a space's and removing it from
the default lens is a different story's decision. The user who wants their pins first still has
`is:pinned` and the Pinned default space.

There is a second reason, and it is about the tests: a hidden pinned term would make every one of the
five sorts agree about the pinned note, which is precisely the "a fixture where two sorts agree
proves neither" trap the AC names.

**`recorded` for a note with no session is its `created` date, interleaved.** The story asked for a
rule rather than an arbitrary end, and three things picked this one.

It is the chain this codebase already uses. Ten lines from `resolve_date`'s `Created` arm sits its
`Touched` arm, whose doc says a missing per-device fact "degrades to `modified` rather than failing —
a missing local fact must never break a shared space." Same shape of problem, same answer; two ideas
where one will do is how a vocabulary rots.

The substitute is on the same scale as the fact it replaces. `recorded` and `created` both answer
*when did this thing happen*. A sentinel — `i64::MIN`, or a partition — answers a different question
and then pretends it did not.

And interleaving is the only option that does not make a note's position depend on something the
reader cannot see. Under "stampless last", a note moves across the entire list the moment somebody
adds a `session:` key to it, and nothing in the row explains why. Interleaved, every note's position
is accountable from a date on its face. That is the *whole* content of AD-81's complaint.

The cost, stated: a `recorded` list mixes two kinds of date, and a note whose `created` is wrong sits
in the wrong place. That is a note with a wrong date, which is a fixable thing a reader can see.

**The fallback is whole, never partial.** `modified sideways` could have become "modified, in some
direction". Story 43.4 met the same fork with the chip vocabulary and answered it the same way — *if
keeper cannot reproduce the value exactly, it does not honour half of it* — and the reason is that the
rule a reader has to hold stays one sentence long. A partial honouring needs a second sentence
explaining which half survived.

**The fallback is visible, and the editor is where it is repaired.** Three surfaces carry it. The
rail row gets a short constant subtitle and the whole sentence on `title`; the editor lists every
sentence in full; and the form is seeded from `sortEffective`, so the control shows the ordering the
list is actually running. That last one is the load-bearing piece: a dropdown that had to work out
for itself what `""` or `bananas` resolves to would be a second copy of the fallback rule, written in
the language that cannot run its tests, and the two copies would disagree the first time the rule
changed.

**Saving rewrites an unreadable sort, and that is allowed here.** Everywhere else in this pair of
stories keeper refuses to rewrite a value it did not understand — the query byte-for-byte, the icon
name untouched. The sort is the exception and the difference is consent: the form showed the fallback
*and* the sentence naming what it could not read, so pressing Save is the user agreeing to the
repair. The icon has no such moment, because the icon set is a set of drawings Rust cannot reason
about and a shrunk set is keeper's fault rather than the file's; the sort vocabulary is ten strings
and closed.

**`sortEffective` is a second field rather than a parsed enum on the wire.** `sort` stays as the
file's own text because byte-honesty about frontmatter is a promise this codebase makes and because
the warning sentence has to quote it. `sortEffective` is what the list is doing. Two fields, two
clear jobs, and no TypeScript parsing either of them.

**Zero is unpositioned, and it is not written.** The four seeded defaults carry no `keeper.order` and
this story does not add one — `default_spaces.rs` was owned by another agent mid-fix, but that
constraint and the right answer coincided. Their names are alphabetical, `notes_spaces` used to sort
by name alone, and a default of 0 with a name tie-break reproduces that rail exactly. Nobody's
sidebar moves until they move it. Negative positions are how a space floats above the unpositioned
block without renumbering everything under it, which is CSS `order`'s design for the same reason.

**The position is `f64` for Story 44.5's reason.** `1.5` is how a person slots a row between 1 and 2,
and an integer field would read `1.5` and `1.2` as the same position — a tie invented by the type
rather than by the vault. The rail comparison uses `total_cmp`, so it is total whatever arrives.

**Changing the sort key does not reset the direction.** Choosing "Order" while the direction says
"desc" leaves it at "desc", which the labels immediately render as "Highest first". Resetting it to
each key's natural direction would mean reproducing `SortKey::natural`'s table in TypeScript, and
that table is a rule with a test. Nothing is hidden by not resetting: the direction control's own
labels follow the key, so the user always reads what they will get.

**Two of the five keys carry a line of explanation and three do not.** `name`, `created` and
`modified` do what they are called. `recorded` is meaningful only for a note that came from a session,
and `order` promises a manual ordering while most notes have never been given one — a reader who is
not told that concludes the sort is broken rather than that the vault is unordered. The `order`
wording is Story 44.5's own sentence, verbatim, because it describes `cmp_order`'s rule and that
story owns it. A sentence under every control is a sentence nobody reads.

## Verification

Every test below was proved by mutating the code it defends, watching it fail, and restoring the
file. The tree was re-run clean after the last restore: 299 `keeper-core` `notes::` tests and 63
frontend tests across the three touched suites.

### `keeper-core::notes::sort`

| Mutation | Tests that caught it |
|---|---|
| A stampless note sorts to `i64::MIN` instead of by its `created` date | `a_note_with_no_session_sorts_by_when_it_was_created`, `every_sort_orders_the_same_four_notes_differently`, `a_direction_reverses_the_fact_and_never_the_tie_break` |
| A bare key always means `desc` | `a_bare_key_takes_the_direction_its_own_word_means` |
| A known key with an unknown direction is half-honoured | `half_a_sort_is_refused_whole_rather_than_half_honoured` |
| The `path` tie-break reverses with the direction | `a_direction_reverses_the_fact_and_never_the_tie_break` |
| An unknown sort falls back silently | `a_sort_keeper_cannot_read_falls_back_out_loud`, `half_a_sort_is_refused_whole_rather_than_half_honoured`, `an_agent_written_sort_cannot_put_a_megabyte_in_the_sidebar` |
| `order desc` reverses Story 44.5's whole comparator instead of only its value | `the_order_sort_is_story_44_5s_comparator_and_not_a_second_one` |
| `recorded_ms` stops gating on `session:` and reads any note's `date` | `only_a_note_with_a_session_has_a_recorded_time` |
| `recorded_ms` ignores `start` and uses only the day | `a_recording_notes_stamp_is_its_own_day_and_clock_joined` |
| An unreadable `start` throws the whole stamp away | `a_recording_notes_stamp_is_its_own_day_and_clock_joined` |
| The rail ignores each space's position | `a_position_lifts_a_space_out_of_the_alphabet_and_a_negative_one_above_it` |
| An unreadable position falls back silently | `a_position_keeper_cannot_read_falls_back_out_loud_too`, `a_quoted_value_never_arrives_in_the_sidebar_as_two_half_sentences` |
| The echoed value keeps its newlines | `a_quoted_value_never_arrives_in_the_sidebar_as_two_half_sentences` |
| The echoed value is not length-capped | `an_agent_written_sort_cannot_put_a_megabyte_in_the_sidebar` |
| `name` compares raw bytes instead of `search::fold_cmp` | `alphabetical_means_what_search_means_by_it` |
| `created`/`modified` read `created_ms`/`updated_ms` directly instead of the DSL's chain | `created_and_modified_are_the_dsls_own_answers` |
| `canonical()` drops the direction word, so the editor writes a value that reads back differently | `the_canonical_spelling_reads_back_as_itself` |
| An absent `sort` is reported as a mistake | `a_space_that_names_no_sort_gets_the_default_and_no_complaint` |

All seventeen were applied one at a time to a clean tree, run against
`cargo test -p keeper-core --lib notes::sort`, and reverted. None survived.

### Frontend

| Mutation | Tests that caught it |
|---|---|
| The form is seeded from `sort` instead of `sortEffective` | `shows the fallback for a sort keeper could not read, and says why`, `repairs an unreadable sort on save, having shown what it was going to write` |
| Saving carries `space.sort` back instead of what the form showed | `shows the ordering the list is actually running, and saves it back`, `repairs an unreadable sort on save…` |
| The rail position is never sent | `shows and saves a position, including a negative and a fraction`, `treats an emptied box as unpositioned rather than as a reason to refuse` |
| An emptied position box becomes `NaN` | `shows an empty box for a space nobody positioned, and saves it as unpositioned`, `treats an emptied box as unpositioned…` |
| The direction labels are one pair reused for every key | `words the direction in the terms of whatever is being sorted` |
| The per-sort explanation is never rendered | `explains the two sorts whose names do not give their behaviour away` |
| The editor never lists what keeper could not read | `shows the fallback for a sort keeper could not read, and says why` |
| The icon set loses `calendar-days` | `offers every glyph a seeded default asks for` |
| The row says nothing about a setting keeper could not read | `says so when it could not read a space's sort, rather than quietly not obeying it` |
| The whole sentence never reaches the row's `title` | same test |
| The rail re-sorts by name behind Rust's back | `renders the rail in the order it was handed, not in its own` |

### What could not be verified on this host

**The `keeper` shell crate does not build on Linux.** `cargo check -p keeper --lib` fails in
`glib-sys`'s build script: *"The pkg-config command could not be found."* So **every line of
`notes_ipc.rs` in this story is unexercised here**, including the ones that matter most:

- `project_list` applying `sort::compare` for a space and `list_order` for the plain list — the
  behaviour change at the centre of this story. The comparator is proved exhaustively in
  `keeper-core`; that the shell *calls* it, and calls it only for a space, is not.
- `space_def` reading `keeper.order` and collecting both warnings.
- `space_lens` returning the ordering beside the query.
- `notes_spaces` projecting `sort_effective` / `warnings` / `order` and ordering the rail.
- `notes_space_save` writing `order` only when non-zero.

Two shell tests were written for the macOS gate and **have never been compiled**:
`a_space_definition_reads_its_position_and_its_sort` and
`a_sort_and_a_position_keeper_cannot_read_are_both_reported`. The file was confirmed to *parse*
(`rustfmt --emit=stdout`, exit 0) and every changed expression was read against the types by hand,
which is not the same as a compiler and should not be read as one.

This is the epic's own lesson and it applies here: Story 44.3 shipped green on two hosts and did
nothing on the owner's vault because the claim was proved through a pure planner while the risk lived
in the shell. The mitigation taken was the same one 44.3's rewrite took — move the decision into
`keeper-core` until what is left in the shell is wiring. What remains unproved is genuinely wiring,
but it is wiring nobody has run.

**Nothing was driven in a browser.** The two `<select>`s, the number input, the warning list and the
24-glyph picker are asserted through jsdom: values, accessible names, option text, `title`. That
twenty-four icons are distinguishable at sidebar size, that a 24-cell grid does not overflow the
dialog at its narrowest, and that the truncated row subtitle is legible are judgements only a person
looking at the screen can make.

**The frontend is transpiled, not type-checked, by `bun run test`.** Vitest strips types without
checking them, so a missing field on `NoteSpaceVm` in an untouched consumer would not surface in the
runs above. Every construction site of `NoteSpaceVm` (three test files) and of `NoteSpaceReq`
(`space-editor.tsx`, `use-notes-actions.ts`) was found by grep and updated, but `tsc` was not run —
it is outside this task's permitted commands.

**No real vault was touched.** No space note was written, read back, or opened in Obsidian. That
`keeper.order` survives the `keeper:` splice, that a `1.5` renders as `1.5` rather than `1.5000001`,
and that Obsidian shows the key are all unobserved.

## Deliberately Not Done

- **`keeper.limit` is still dead.** It is read, shipped, and written back, and `project_list` sizes
  its window from `req.limit` regardless — the identical defect this story fixed for `sort`, one key
  over. Left alone because applying it is a change to how many rows every space returns, which
  interacts with 44.10's virtualisation and 44.11's counts; whoever takes it has to decide first
  whether `limit` caps what a space *selects* or what it *renders*, because the count means something
  different under each. Filed as **DW-163**.
- **`recorded` reads the stub's user-editable `date` and `start`.** There is no single stored
  instant, and those two keys are the user's own text that FR-121 forbids keeper from overwriting. So
  a hand-retyped `date` moves a note in a `recorded` list even though `manifest.json` still knows the
  real time. The fix is a schema decision — a machine-owned `keeper.recorded` written by
  `compose` — which changes every recording note and belongs beside Story 44.2. Filed as **DW-164**.
- **The four seeded defaults were not given explicit `order` values.** `default_spaces.rs` was being
  edited by another agent for a 44.3 field defect and was off limits, and the right answer coincided
  with the constraint: their names are already alphabetical and the unpositioned default reproduces
  the existing rail exactly. Giving them 1–4 would be equivalent today and would grow four keys in
  every vault's frontmatter for nothing.
- **The rail is not drag-reorderable.** The position is a number in a form. A drag would want an
  insertion rule that writes fractional positions — which is why the type is `f64` and why the drag
  is cheap to add later — but this story's AC is that the rail honours the order, not that there is a
  second way to set it.
- **`notes_rename`'s doc-comment lie is still there.** Story 43.4 reported it (`notes_ipc.rs`, "also
  updates the note's own first heading" — it does not) and it is still true, still out of scope, and
  still deserves its own story.
- **A space's sort does not affect the plain list, the folder lens, or backlinks.** All three still
  use `list_order`. Only a space carries a sort; giving one to the default lens is a question about
  what the app's default ordering should be, which nobody asked.
