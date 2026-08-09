---
title: 'Story 43.4: Spaces You Can Edit'
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
  - '{project-root}/_bmad-output/implementation-artifacts/spec-43-3-include-exclude-off.md'
---

<intent-contract>

## Intent

**Problem:** a space can be created and never changed. "Save as space" writes one; after that,
adjusting a saved filter means deleting the note and rebuilding it from memory, so people stop
building them. There is also no rename (the save command ignores the `name` it is handed on an
update, then returns it as though it had used it) and no icon anywhere in the persisted shape.

**Approach:** one editor over the space's three editable facts — name, icon, terms — behind one
write. Terms are Story 43.3's three-state chips, told what to do rather than reaching for the filter
store, because editing a space must not re-filter the list behind the dialog. Reading a stored query
*back* into chips is a new operation and it lives in `keeper-core::notes::query` beside the parser:
one grammar, one tokenizer, one definition of what a tag is. And because that read is lossy for
queries the DSL can express and a chip bar cannot, the decomposition is an enum — chips, or the
terms that stopped it — so "render three of a query's four terms and save" does not compile.

### How a space is persisted, established before anything was changed

| Fact | Where | Consequence for this story |
|---|---|---|
| A space **is** a note under `spaces/` — the `space` flag is literally `rel.starts_with("spaces/")` | `notes_vault.rs:1313` | rename keeps the note in its own directory, so a renamed space stays a space |
| Its definition is `keeper: { space: "<DSL text>", sort, limit }`, one level of nesting, with a nested `space: { query, … }` form also accepted | `notes_ipc::space_def` | `icon` is a fourth key, read in both forms |
| Its **name is not stored**: it is `entry.title` = frontmatter `title`, else the body's first non-blank line, else the filename stem | `notes_vault::note_title` | a rename must write something `note_title` will actually read |
| `notes_space_save` on an update splices only the `keeper` key and then returns `title: space.name` | `notes_ipc.rs:962` | that return was a lie; rename is implemented here |
| The query is DSL text: `tag: path: field: date: origin: is: text: link: backlink:`, `-` negation, `\|` for OR, parenthesised groups | `notes/query.rs` | the chip vocabulary is a strict subset, so the round trip is lossy |
| An empty query is a **parse error**, not "everything" | `query::parse` | the backstop under the editor's own refusal to save a space with no terms |

### The round trip, and what happens when it is lossy

`spaceQueryText` writes a flat conjunction of `tag:` / `-tag:` / `is:` / `origin:` / `text:"…"`.
`query::decompose` reads exactly that back and refuses everything else.

**Representable:** a flat conjunction whose every term is `tag:x` or `-tag:x` (at most one term per
tag, since a chip has one slot per tag), `is:<flag>`, `origin:<value>`, or one free-text term.

**Refused:** `|`, parenthesised groups, `path:`, `field:`, `date:`, `link:`, `backlink:`,
`tag:x/*`, a `tag:` value that is not a tag, a tag named twice, a second text term, a repeated
flag, and negation of anything that is not a `tag:`.

**What the editor does with a refused space:** it shows the query read-only, names the terms it will
not touch, and on save hands the stored query string back **byte for byte**. Name and icon stay
editable. The alternative — refusing the whole editor — would send someone back to hand-editing
frontmatter just to rename a space, which is the pain this story exists to kill; and rewriting the
terms would silently delete `date:modified>=-14d`, which the epic calls the worst available outcome.
The byte-identity promise is the safety property, so it is asserted from both ends (below).

## Boundaries & Constraints

**Always:**
- One write per save. A rename, an icon and a term set land together or not at all — three commands
  would leave a space renamed but still carrying the terms the user just deleted if the second
  failed.
- The chip is Story 43.3's, parameterised, never copied. Three redundant carriers of a chip's state
  (glyph, colour, accessible name) get one definition.
- The decomposition is `keeper-core`'s. No DSL parsing in TypeScript, ever (AD-20, AD-58).
- A tag is whatever `notes::tags::normalise` says it is. The "add a tag" control offers the vault's
  own tag tree; there is no free-text tag box, because that would be a second definition of a tag.
- Cancel writes nothing, anywhere.
- A note keeper wrote stays a note Obsidian renders: the query stays one line of DSL in frontmatter,
  and the rename follows the body heading so the file and its first line cannot disagree.

**Block If:**
- Nothing. The wire change is additive (`icon` is `Option`) and the new command is parse-only.

**Never:**
- No partial chip set. Either every term is a chip or none is.
- No second query writer. `spaceQueryText` is exported so the bar and the editor emit one grammar.
- No rewriting of a value keeper did not understand — not a query term, not an icon name.
- No fix to `notes_rename`'s doc-comment lie (see *Deliberately Not Done*).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| Chips round-trip | `tag:client/acme -tag:draft is:pinned origin:agent text:"quarterly review"` | include, exclude, one flag, one origin, one text — in written order | none |
| Editing changes what is selected | cycle `client/acme`, drop `draft` | saves `-tag:client/acme` | none |
| Adding a tag | choose `urgent` | saves `tag:client/acme -tag:draft tag:urgent` | none |
| Chip position | cycle the first chip | saves `-tag:client/acme -tag:draft`, order kept | none |
| Untouched axes | space with `is:pinned origin:agent text:"…"`, save with no edits | identical query | none |
| Lens removed | remove `is:pinned` | `tag:client/acme` | none |
| Icon chosen | pick `star` | `icon: "star"` in the request and in `keeper.icon` | none |
| Icon shown | space stored `flag` | `flag` is the pressed choice, `No icon` is not | none |
| Icon cleared | pick `No icon` | `icon: null`; the key is not written | none |
| Icon outside the set | stored `sparkles` | fallback glyph drawn; nothing in the picker pressed; a save that did not touch the picker sends `sparkles` back | none |
| Rename | name `"  Archive triage  "` | request carries `Archive triage`; frontmatter `title` spliced, file renamed, keeper's own `# ` heading followed | none |
| Rename, human-edited heading | body opens `# What I am actually tracking` | heading untouched | none |
| No name | name cleared to spaces | Save disabled, "A space needs a name." | none |
| No terms | every chip removed | Save disabled, "A space needs at least one term. Add one, or cancel." | `parse` also refuses an empty query at the edge |
| Terms count everything | only `is:pinned` left, then removed | enabled, then disabled | none |
| Grouped lossy query | `tag:a (tag:b \| tag:c) …` | refused **whole** — a group is not a term — shown read-only | none |
| Flat lossy query | `tag:client/acme path:journal/** date:modified>=-14d` | each refused term named; still no chips | none |
| Byte identity | rename a lossy space | stored query saved back unchanged, byte for byte | none |
| Sort and limit | lossy space, `created asc` / 42 | carried through untouched | none |
| Unparseable query | `nope:x` | "This space's query can't be read…", query kept verbatim, name and icon still editable | `notes_space_terms` rejects; the editor treats the rejection as a state |
| Cancel | name, icon and chips all changed | nothing saved, nothing re-read | none |
| Save fails | IPC rejects | "keeper couldn't save this space. Nothing was changed." | shown in place |
| List refresh | save | `notes_spaces` re-read, sidebar shows the new name | none |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/notes/query.rs` — `decompose`, `chip_tag`, `same_flag`; the
  tokenizer and parser are otherwise untouched.
- `src-tauri/crates/keeper-core/src/notes/naming.rs` — `retitle_heading`, beside `title_from_body`
  whose rule it mirrors.
- `src-tauri/crates/keeper-core/src/notes/vm.rs` — `NoteSpaceVm.icon`, `NoteSpaceReq.icon`,
  `NoteSpaceTagVm`, `NoteSpaceTermsVm`.
- `src-tauri/crates/keeper/src/notes_ipc.rs` — `space_icon`, `MAX_ICON_BYTES`, `SpaceDef.icon`;
  `notes_space_save` gains the icon key and the rename; `rename_in_place`; `notes_space_terms`.
- `src-tauri/crates/keeper/src/lib.rs` — one command registration.
- `src/lib/ipc/gen/NoteSpaceTagVm.ts`, `NoteSpaceTermsVm.ts`, and the two edited VMs — regenerated
  by ts-rs, not hand-written.
- `src/lib/ipc/client.ts` — `notesSpaceTerms` plus the type re-exports.
- `src/components/notes/space-editor.tsx` — new; the surface, `SPACE_ICONS`, `spaceIcon`.
- `src/components/notes/space-list.tsx` — the icon, the edit affordance, the re-read after save.
- `src/components/notes/note-filter-bar.tsx` — `TagFilterChip` exported and given `onCycle` /
  `onRemove`; the bar passes its own store calls, so its behaviour is unchanged.
- `src/lib/stores/notes-filters.ts` — `withTagTerm` exported. Nothing else.
- `src/hooks/use-notes-actions.ts` — `spaceQueryText` exported and taking an ordered chip list.

## Tasks & Acceptance

**Execution:**
- [x] `query::decompose` with an all-or-nothing result type; the DSL grammar untouched.
- [x] `icon` on the persisted shape, both frontmatter forms, bindings regenerated by ts-rs.
- [x] Rename inside `notes_space_save`: frontmatter `title`, the file, and keeper's own heading.
- [x] The editor: name, fixed icon set, 43.3's chips over a draft, the read-only arm.
- [x] Refusals: no name, no terms.
- [x] Tests, each proved by reverting the code it covers.

**Acceptance Criteria:**
- `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core --lib notes::` — 231 passed.
- `bun run test src/components/notes/space-list.test.tsx src/components/notes/space-editor.test.tsx`
  — 29 passed.
- `bun run test src/components/notes/note-filter-bar.test.tsx src/lib/stores/notes-filters.test.ts
  src/components/notes/tag-tree.test.tsx src/components/notes/notes-pane.test.tsx` — 38 passed, so
  parameterising the chip changed nothing for its first caller.
- `cargo clippy -p keeper-core --lib --all-targets` and `cargo fmt --check` — clean.

## Design Notes

**The decomposition is an enum, not a struct with a residue list.** The cheap shape was
`{ tags, flags, origin, text, unrepresentable }`, and it would have worked right up until someone
rendered the four chips it happily supplies for
`tag:a tag:b tag:c date:modified>=-14d` and saved them. That is not a bug a reviewer catches; it is
a bug a user discovers a month later when their space stops excluding old notes. Two variants make
the call site that would do it fail to compile, which is the same move Story 43.3 made with
`NoteQueryReq.tags` — delete the state rather than arbitrate it.

**A group is refused whole; a flat query names its terms.** `tag:a | tag:b` has no offending
*part*: extracting `tag:a` as a chip would be a lie about what the query selects, and naming `|` on
its own reads like a typo. So a query containing any `(`, `)` or `|` comes back as one entry — the
whole query — and only a flat conjunction is broken down term by term. Both shapes are pinned in
Rust and both are exercised through the editor.

**The chip vocabulary refuses more than it strictly has to, on purpose.** `tag:a tag:a` is
semantically one term and could safely collapse; `tag:Draft -tag:draft` is unsatisfiable and could
safely collapse to a contradiction. Both are refused, because collapsing either means keeper
rewriting text a person typed into text keeper prefers. The cost is that a handful of odd
hand-written spaces are name-and-icon-only; the benefit is that the rule a reader has to hold is one
sentence long — *if the chips cannot reproduce it exactly, they do not touch it.*

**Normalisation is the one rewrite that is allowed.** `tag:#Client/Acme` decomposes to the chip
`client/acme` and re-emits as `tag:client/acme`. That is a rewrite, and it is the right one: the
chip *is* the tag vocabulary (Story 42.5), the parser normalises the same value to the same tag, and
refusing here would make most hand-written spaces uneditable in exchange for preserving a spelling
that already means nothing to the query. It is a rewrite of spelling, never of meaning, which is the
line everything else in this story is drawn along.

**Rename lives in `notes_space_save`, and it writes frontmatter `title`.** Three facts forced this.
`note_title` prefers frontmatter `title` over the body's first line over the filename stem, so the
key is the only place a name is *guaranteed* to stick. `notes_rename` renames the file and nothing
else, so calling it alone would have renamed `active-work-2026-08-09.md` and left the sidebar
showing the old name. And a space's three edits have to be one write. The file is renamed too,
through the same `naming::note_filename` path `notes_rename` uses, because a file called
`active-work` defining a space called "Archive triage" is a vault that disagrees with the app, and
the vault is what Obsidian and the sync see.

**The heading rule, and why it is exactly an equality test.** keeper writes `# <name>` as a new
space's whole body, so leaving it behind after a rename produces a file named "Archive triage" whose
first rendered line says "Active work". keeper may maintain that heading because keeper wrote it.
A heading someone changed is a thing they said, and following a rename into it would be keeper
editing prose it does not own — so the match is exact, against the line `title_from_body` would have
read, and prose, a deeper heading, a later heading and an empty body all decline. The function is
`keeper-core::notes::naming`'s rather than the shell's for one reason: the shell does not build on
Linux, and a rule this easy to get subtly wrong must be provable on the host it is written on.

**An unknown icon name is not an error and is not rewritten.** The set is the editor's — it is a set
of drawings and Rust has none — so the shell trims the stored name, caps its length and passes it
through. If the set later shrinks, the row draws the fallback glyph rather than a hole, the picker
shows nothing selected, and a save that did not touch the picker sends the old name straight back.
Same principle as the query: keeper does not silently rewrite a value it did not understand. The
only judgement Rust makes is the length cap, because frontmatter is agent-writable and a 4 MB "icon
name" is a bug rather than an icon.

**"No terms" is refused at the form, and again at the edge.** `query::parse` already rejects an
empty query, so a space that selects everything cannot be written even by a caller that tries. The
form refuses first anyway, because a disabled button with a sentence under it is an explanation and
a rejected IPC call is an error message.

**`spaceQueryText` takes an ordered list, not `NoteQueryReq`'s map.** JavaScript puts integer-like
object keys first, so a vault with a `2026` tag would have had its saved query silently reordered.
The map is right on the wire (it is what makes include-and-exclude unrepresentable) and wrong as an
ordered input, so the writer takes the chip array the store already keeps in order.

**The chip was parameterised rather than copied.** `TagFilterChip` reached into
`notesFiltersStore` directly, which is correct for the filter bar and impossible for an editor whose
draft must not touch the live list. Two callbacks make it usable by both. A second chip component
would have been a second copy of the glyph rule, the colour rule and the accessible-name rule, and
the accessible name is the one nobody would notice rotting.

## Verification

Every test below was proved by mutating the code it defends, watching it fail, and restoring.
The tree was re-run clean after the last restore: 231 `keeper-core` `notes::` tests and 67
frontend tests across the six touched suites.

### `keeper-core`

| Revert | Tests that caught it |
|---|---|
| `decompose` ignores `(`, `)` and `\|`, decomposing a grouped query as if it were flat | `a_grouped_or_disjoint_query_is_refused_whole_because_a_group_is_not_a_term`, `the_editors_worked_lossy_example_is_refused_whole` |
| A partial chip set escapes instead of `Unrepresentable` | 9 tests, including `a_term_outside_the_chip_vocabulary_is_named_verbatim`, `a_query_of_nothing_but_refusals_names_every_one_of_them`, `the_editors_worked_flat_lossy_example_names_its_two_refused_terms` |
| `chip_tag` flattens `tag:x/*` to `tag:x` | `the_descendants_only_tag_form_is_refused_rather_than_flattened`, `a_query_of_nothing_but_refusals_names_every_one_of_them` |
| `chip_tag` stops normalising | `a_tag_is_read_back_through_the_one_vocabulary`, `a_tag_term_that_names_no_tag_is_refused`, `a_tag_named_twice_is_refused…` |
| One tag may fill two chip slots | `a_tag_named_twice_is_refused_because_the_bar_has_one_slot_per_tag` |
| `decompose` skips the `parse` gate | `a_query_that_does_not_parse_decomposes_into_an_error_not_an_empty_chip_set` |
| `retitle_heading` rewrites whatever the first line is | `a_retitle_leaves_a_body_keeper_did_not_write_untouched` |
| `retitle_heading` drops `\r` from the ending it preserves | `a_retitle_follows_the_heading_keeper_wrote` |
| `retitle_heading` leaves trailing whitespace dangling after the new heading | `a_retitle_follows_the_heading_keeper_wrote` |

### Frontend

| Revert | Tests that caught it |
|---|---|
| The read-only arm re-emits the query from chips | `saves the stored query back byte for byte when only the name changed`, `saves a flat lossy query back byte for byte too`, `still lets the icon change…`, `says so and keeps the unreadable query…` |
| The read-only arm renders chips anyway | all 7 tests in *a space whose query the chips cannot hold* and *…does not parse* |
| An empty draft is allowed to save | `refuses to save rather than becoming everything`, `counts a lens, an origin and a search as terms too` |
| The icon is never sent | `persists the icon that was chosen`, `shows no selection for an icon outside the set…`, `still lets the icon change…`, `draws the fallback glyph…and keeps the name` |
| An icon outside the set is rewritten to none on open | `shows no selection for an icon outside the set and leaves the stored name alone`, `draws the fallback glyph…and keeps the name` |
| An unknown icon name draws nothing | `draws the fallback glyph for an icon name that is not in the set any more, and keeps the name` |
| `withTagTerm` appends instead of rewriting in place | `keeps a cycled chip where it was, so the query keeps its order` |
| `spaceQueryText` drops the exclusion sign | `writes the DSL the chips now say…`, `adds a tag from the vault's own vocabulary…`, `keeps a cycled chip where it was…` |
| The name is sent untrimmed | `sends the new name, trimmed` |
| A broken query becomes an empty chip set | `says so and keeps the unreadable query rather than offering an empty chip set` |
| Cancel saves before closing | `writes nothing at all, whatever was typed`, `leaves the list alone when the editor is cancelled` |
| The list always edits its first space | `opens the editor for the row that was pressed` |
| The list does not re-read after a save | `re-reads the list after a save, so the sidebar shows the new name` |
| The editor invents `sort` and `limit` instead of carrying them | `carries the sort and limit it never showed` |

### What could not be verified on this host

**The `keeper` shell crate does not build on Linux** (`pkg-config`/`gobject-2.0` are absent), so
every line in `notes_ipc.rs` is unexercised here and the macOS gate is its first real check. That
covers: `space_def` reading `icon`, `space_icon`, the `icon` key in the written definition,
`notes_space_save`'s rename branch (the `title` splice, the heading call, `rename_in_place`), and
`notes_space_terms`. Three of those five were deliberately pulled *out* of the shell for exactly
this reason — the decomposition and the heading rule are `keeper-core`'s and are fully tested above —
and the two shell-only tests that were written (`a_space_definition_reads_its_icon_in_both_frontmatter_forms`,
`an_unrecognised_icon_name_survives_and_only_noise_is_refused`) have never been compiled.

**Nothing was driven in a browser.** The dialog, the icon picker and the chips are asserted through
jsdom: accessible names, `aria-pressed`, the `data-space-icon` attribute, and the exact request
handed to `notesSpaceSave`. That the eight icons are distinguishable at sidebar size, and that the
pencil is discoverable on hover, are judgements only a person looking at the screen can make.

**The IPC boundary is mocked, so the two lossy fixtures are pinned in both languages.** The editor
tests mock `notesSpaceTerms`, which means a mock inventing a reply shape Rust never produces would
be a green test over a broken seam. Both `LOSSY_QUERY` and `FLAT_LOSSY_QUERY` are therefore the
exact strings `the_editors_worked_lossy_example_is_refused_whole` and
`the_editors_worked_flat_lossy_example_names_its_two_refused_terms` feed the real decomposer, and
the mocked replies are what it actually returns. The remaining untested link is the last one: that
the string in `NoteSpaceReq.query` reaches the file unchanged. That is one `FieldValue::Str` in
`notes_space_save` and it is shell code, so it is on the macOS gate with the rest.

## Deliberately Not Done

- **`notes_rename`'s comment is a lie and it was left alone.** `notes_ipc.rs:1437` says
  "Retitling also updates the note's own first heading when it carried one" and the function does no
  such thing — it renames the file and returns. That is a real defect, it touches every note rather
  than only spaces, and it deserves someone deciding whether the doc or the code is wrong. Reported
  to the epic owner for its own story. `notes_space_save`'s own lie — returning a `title` it ignored
  — *was* fixed, because rename could not be implemented around it.
- **No `tag:x/*` control, no negated lens.** Both exist in the DSL and neither has a chip. Story
  43.3 left the second open on purpose ("what does a negated lens mean" is a bigger question) and
  the first is not a thing anyone asks a chip for. Both are simply refused by `decompose`.
- **No editing of `sort` or `limit`.** They are presentation, deliberately outside the query
  grammar, and this story is about what a space *selects*. They are carried through untouched.
- **No free-text query field in the editor.** A space's query is editable as text already — in the
  note, in Obsidian, by an agent. Adding a second text editor for it here would mean shipping the
  live validator, the token underline and a second save path, for a surface whose whole point is
  that you do not have to read DSL.
- **No "duplicate this space" and no delete.** Both are list verbs, not editor verbs, and neither is
  in the AC.
- **No migration.** `icon` is absent from every space that exists today and absent is a valid value;
  nothing is rewritten to add it.
