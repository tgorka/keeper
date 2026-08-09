---
title: 'Story 44.3: Default Spaces Replace the Fixed Rail'
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
  - '{project-root}/_bmad-output/implementation-artifacts/spec-43-4-spaces-you-can-edit.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-43-3-include-exclude-off.md'
---

<intent-contract>

## Intent

**Problem:** the rail lied about its own vocabulary. Inbox, Journal, Pinned and Recordings were four
hard-coded `<Button>`s built from a `SCOPE_ROWS` array in `notes-pane.tsx`, each resolving through a
`SCOPE_FLAG` table in the filter store to one `is:` string. They *were* saved filters — the only
difference from a space was that they could not be renamed, reordered, given an icon, edited or
deleted, so every space feature 43.4 shipped was a feature only power users ever saw. Above them sat
Today, which was a row that opened a note; it never filtered anything.

**Approach:** delete the rail. The four become real notes under `spaces/`, seeded once into a vault
that has never been offered them, and `NoteScope` collapses from seven variants to three (`all`,
`space`, `folder`). Their queries are the ones the rows ran, moved from a TypeScript table into
frontmatter the user can read and change. Today is removed rather than ported.

Everything that decides *whether keeper writes into somebody's vault* is a pure function in
`keeper-core::notes::default_spaces`, because the shell does not build on Linux (AD-55/AD-56) and
because this is the most dangerous write keeper makes: notes, into a real vault, on a pendrive,
through the sync engine.

### The queries are the rows', not new ones

| Default | Query | Where the flag comes from |
|---|---|---|
| Inbox | `is:untagged` | derived by `query.rs` from an entry with no tags |
| Journal | `is:journal` | `notes_vault::note_flags` — `rel.starts_with("journal/")` (read, not assumed) |
| Pinned | `is:pinned` | the index flag |
| Recordings | `is:recording` | the index flag `session:` frontmatter sets (Story 42.4) |

Every one is already a member of `query.rs`'s **closed** `IS_FLAGS` set, so nothing in the parser
changed and no flag was added. **Inbox is `is:untagged`, not `is:inbox`.** The story text suggested
`is:inbox`; adding it would have meant a second name for one predicate, inside a closed set the epic
explicitly says it grows by none — and it would have made every seeded Inbox a located parse error
in a fresh vault. `is:untagged` is what the deleted row sent, so the seeded space lists the same
notes the row listed.

### Where the seed ledger lives, and why it is a file in the vault

Seeding is recorded in **`.keeper-spaces.json` at the vault root**, and it syncs.

"keeper has already offered its defaults here" is a fact about **this vault**, not about this
laptop. If the record were machine-local, deleting Pinned on the desktop would be undone the next
time the laptop opened the same synced folder — which is precisely the forever-ownership AD-79
refuses. That rules out the two cheaper homes:

- **The profile row in `keeper.db`** (`NotesConfig`) is per-machine.
- **`.keeper/`** is per-machine *and* documented as a deletable cache ("keeper's own cache rather
  than vault content"). A fact that cannot be recomputed must not live somewhere a user is invited
  to clear to fix an index problem.

A dot-prefixed `.json` is safe in all three places that matter: Obsidian's explorer hides dotfiles;
the note walk only ever collects `.md`, so it never enters the index or a list; and
`keeper-sync`'s tier-0 corpus excludes the `.keeper` *directory*, not names beginning with it — its
own test pins `sub/.keeperrc` as the user's file. The file carries a `note` field explaining itself,
because a stranger's JSON in your vault root should say what it is and how to make it stop.

### Seeding does not wait for the index — it races nothing, it reads the disk

`notes_spaces` reads an `IndexSnapshot`. On a vault registered a millisecond ago that snapshot is
empty, so seeding off it would write four notes into a vault that already has four. Seeding
therefore asks the **`spaces/` directory** what is there (`notes_vault::siblings` + `read_note`),
which is true immediately and stays true when the drive is unplugged halfway through a write.

The hook is `notes_vault::refresh`, for every **newly registered** vault only, after the registry
lock is dropped. Not the frontend: a vault reached only from the tray or the capture window has to
be seeded too.

### Idempotence against a partially written state

`plan` is keyed on **what is on disk**, never on how far a previous attempt got:

- The drive is unplugged after two notes land. The ledger is written last, so it never landed. Next
  registration: the two on disk stand their keys down, the other two are written. Nothing doubles.
- The ledger *did* land and a note was then deleted. The ledger vetoes the automatic run for that
  key. The default stays deleted, forever, until the user presses Restore.
- Two defaults seeded in one pass cannot collide on a filename: `siblings` is read once and grown as
  each name is taken.
- A write fails mid-run. The run stops and **still records the keys that landed**, so a full disk is
  not retried as "write all four again" next launch.

### What happens to a user space that already has a default's name

**keeper stands its default down and never touches theirs.** A default is skipped when any existing
space either carries its key (`keeper.default`) or is already called what it would be called. The
name comparison is `naming::slug`'s fold — the same one that decides two notes cannot share a
filename — so `Inbox`, `inbox`, `  INBOX  ` and `Ínbóx` are one name, while `In box` (→ `in-box`)
and `Inboxes` are not.

The consequence, stated plainly: someone who built their own Inbox before keeper shipped one keeps
exactly their own, and never gets keeper's. Restore will not add a second one either — their space
*is* the Inbox as far as the rail is concerned. The cost is that they never see keeper's
`is:untagged` version unless they rename theirs; the alternative was two rows both saying "Inbox",
which is worse than a missing row nobody asked for.

### Identity is the marker, not the name

A seeded space carries `keeper.default: <key>` in its frontmatter. That is the only thing about it
the user cannot change, and everything else — name, icon, query, sort, position — is theirs from the
moment the note exists, which is the whole of AD-79. Two things depend on it:

- **Restore** knows a renamed Inbox is still the Inbox default and does not offer a second.
- **The empty state** keeps saying "No recording notes yet. keeper writes one each time a recording
  stops." after someone renames Recordings to "Sessions" — and does *not* lend that sentence to a
  space of the user's own that happens to be called Recordings.

`notes_space_save` splices the whole `keeper` map, so it now carries the marker through explicitly.
Without that, editing the seeded Inbox would silently demote it and Restore would offer a second.

## Boundaries & Constraints

**Always:**
- The decision is `keeper-core`'s. What to write, whether to write, what the note says and what the
  ledger means are all pure functions over values; the shell does directory reads and atomic writes.
- A seeded note is byte-for-byte the shape `notes_space_save` writes for a hand-made space, plus the
  marker. A second kind of space note would break in the editor.
- Nothing here deletes, rewrites or renames a space that exists. `plan` returns only things to
  write; there is no arm that returns something to remove.
- The ledger's unreadable case is timid. Automatic seeding writes nothing; only the user's own
  Restore proceeds.
- `Today` is removed from the rail, the store's type, the store's flag table and the tests. The FR-99
  *action* is untouched.

**Block If:**
- Nothing. `NoteSpaceVm.defaultKey` is an added field, the new command is additive, and the
  `is:` set was not touched.

**Never:**
- No new `is:` flag, and no second name for an existing one.
- No DSL in TypeScript. The rail sends a `spaceId`; Rust parses the space's own text.
- No seeding off the index.
- No second definition of "what a note is called" — the seeder calls
  `notes_vault::title_of_source`, which is `note_title` with the frontmatter already parsed.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| Fresh vault | no `spaces/`, no ledger | four notes written, ledger records four keys | none |
| Reopened | four notes, ledger of four | nothing written | none |
| One deleted, reopened | three notes, ledger of four | nothing written — it stays deleted | none |
| Half-written seed | two notes, no ledger | the other two written; the two present untouched | none |
| Restore, two missing | two notes, ledger of four | the two missing written | none |
| Restore, nothing missing | four notes | nothing written, `0` returned, "Nothing was missing." | none |
| Restore, no ledger readable | four keys, corrupt ledger | proceeds; the ledger is repaired on write | none |
| Automatic run, corrupt ledger | file present, not JSON | **nothing written** | logged, not surfaced |
| Automatic run, absent ledger | no file | four written | none |
| Existing vault with user spaces | `Active work`, `Archive triage` | four written beside them; neither touched | none |
| User space named `Inbox` | no marker, name folds to `inbox` | Inbox stood down; the other three written | none |
| User space named `In box` | folds to `in-box` | a different space; all four written | none |
| Renamed default | `Unfiled` carrying `default: inbox` | Inbox stood down by key, not by name | none |
| Unrecognised marker | `default: someday` | names no default; the real ones still seed | none |
| Editing a seeded default | rename + new icon through 43.4's editor | `keeper.default` survives the splice | none |
| Newer build's key in the ledger | `seeded: ["inbox","someday"]` | `someday` is kept, Inbox is not re-offered | none |
| Empty Recordings space | `is:recording`, no matches, no chips | "No recording notes yet…" | none |
| Renamed Recordings, empty | space named `Sessions`, marker `recordings` | same sentence | none |
| User space called Recordings, empty | no marker | the generic "No notes match these filters." | none |
| Vault with no spaces at all | `notes_spaces` → `[]` | the Spaces section still renders, with Restore | none |
| Restore with no vault open | `vaultId === null` | the control is disabled and sends nothing | none |
| Restore rejected by IPC | read-only volume | "keeper couldn't restore the default spaces."; list untouched | shown in place |
| A default's query in the rail | pressing Recordings | request carries `spaceId`, and **no flag** | none |
| Pinned space + pinned chip | both active | `flags: ["pinned"]` once; the space filters through `spaceId` | none |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/notes/default_spaces.rs` — new. `DEFAULT_SPACES`, `plan`,
  `SeedMode`, `ExistingSpace`, `render_note`, `default_key` / `default_key_of`, `parse_ledger` /
  `render_ledger`, `LEDGER_REL`, `SPACES_DIR`, and — after the field report — the `SeedVault` port,
  `SeedOutcome` and the whole `seed` run.
- `src-tauri/crates/keeper-core/src/notes/naming.rs` — `note_title`, moved out of the shell so the
  seeder and the index share one three-branch fallback.
- `src-tauri/crates/keeper-core/src/notes/mod.rs` — one module line.
- `src-tauri/crates/keeper-core/src/notes/vm.rs` — `NoteSpaceVm.default_key`.
- `src-tauri/crates/keeper/src/notes_ipc.rs` — `SpaceDef.default_key`; `space_def` reads it through
  core; `notes_spaces` emits it; `notes_space_save` carries it through the splice;
  `seed_default_spaces`, `notes_spaces_restore_defaults`, `apply_default_spaces`, `existing_spaces`,
  `read_seed_ledger`, `record_seed_ledger`.
- `src-tauri/crates/keeper/src/notes_vault.rs` — `refresh` collects newly registered vaults and seeds
  them after dropping the lock; `write_note` split so `write_vault_file` can write a non-note;
  `title_of_source`.
- `src-tauri/crates/keeper/src/lib.rs` — one command registration.
- `src/lib/ipc/gen/NoteSpaceVm.ts` — regenerated by ts-rs, not hand-written.
- `src/lib/ipc/client.ts` — `notesSpacesRestoreDefaults`.
- `src/lib/stores/notes-filters.ts` — `NoteScope` loses four variants and gains `space.defaultKey`;
  `SCOPE_FLAG` deleted; `scopeLabel` and `noteQueryFor` follow.
- `src/components/notes/notes-pane.tsx` — `SCOPE_ROWS`, the Today row and their imports deleted; the
  `no-recordings` branch keys on `defaultKey`.
- `src/components/notes/space-list.tsx` — always renders; the Restore control; `defaultKey` onto the
  scope.
- `src/components/notes/space-editor.tsx` — `pin` and `video` added to `SPACE_ICONS` (see below).

## Tasks & Acceptance

**Execution:**
- [x] `default_spaces`: the four, the plan, the ledger, the rendered note, the marker.
- [x] Seeding on first registration, recorded, idempotent against a partial write.
- [x] `notes_spaces_restore_defaults` and the rail control.
- [x] `NoteScope` collapsed; `SCOPE_FLAG` deleted; `Today` removed.
- [x] `defaultKey` on the wire, regenerated by ts-rs.
- [x] Tests, each proved by mutating the code it defends.

**Acceptance Criteria:**
- `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core --lib notes::` — 267 passed
  (29 of them this story's, 12 of those driving the real run against a real directory).
- `bun run test src/components/notes/notes-pane.test.tsx src/components/notes/space-list.test.tsx` —
  26 passed.
- `bun run test src/components/notes/ src/lib/stores/notes-filters.test.ts src/hooks` — 561 passed,
  so collapsing `NoteScope` broke nothing that reads it.

## Design Notes

**The plan is a pure function and the seed is not, on purpose.** The two failures worth fearing are
writing a default twice and writing one back after a deletion, and both are decisions rather than
effects. Putting them in `keeper-core` means they are provable on the host they were written on —
the `keeper` shell does not compile on Linux at all (below) — and it means the shell's contribution
is `read_dir`, `read_to_string` and `atomic_write`, which is the smallest unprovable surface the
feature can have.

**`plan` takes the ledger as `Option`, and the `None` arm is the interesting one.** An *absent*
ledger is a vault that has never been seeded and gets all four. An *unreadable* one is a vault keeper
knows nothing about, and the automatic path writes nothing. Collapsing those two into "not seeded"
would resurrect every default a user had deleted the day their JSON got truncated by a bad unmount.
The asymmetry is deliberate: not offering a space costs one menu item, and putting four back into
someone's vault behind their back is the outcome AD-79 exists to prevent. Restore ignores the ledger
entirely, because the ledger's whole job is to stop keeper acting *on its own*, and Restore is the
user acting.

**Presence is two questions, not one.** A default is present if a space carries its key, or if a
space is already called what it would be called. Neither subsumes the other, and each catches a case
the other cannot: the key catches a default the user renamed to `Unfiled` (a name check would happily
write a second Inbox beside it), and the name catches an Inbox the user built themselves before
keeper had one (a key check would happily write a second row saying "Inbox"). The mutation run below
confirms this — deleting the key filter leaves every other test green and fails only
`a_default_that_was_renamed_is_still_that_default`.

**The marker is the identity because everything else is editable.** AD-79's promise is that a default
is a space like any other, which means its name, icon, query, sort and order all belong to the user
the moment it exists. So none of them can be what "this is the Recordings space" means, and
`keeper.default` — written by keeper, never offered in the editor — is what is left. It is also what
lets the pane keep one sentence that would otherwise have to be deleted: `no-recordings` used to key
on `scope.kind === "recording"`, a kind that no longer exists, and the alternatives were matching on
the space's name (breaks on rename, and lends the sentence to a stranger's space with the same name)
or reading the space's DSL in TypeScript (AD-20, AD-58). A field Rust computed is neither.

**`notes_space_save` had to learn about a key it never writes.** `Frontmatter::set_in(source,
"keeper", …)` replaces the whole map, so any key the request does not carry is a key the save
deletes. Before this story that was harmless — the map was exactly `space`/`sort`/`limit`/`icon`.
Now the seeded Inbox would have been silently demoted to an ordinary space the first time anybody
renamed it, and Restore would then have offered a second Inbox. The fix is three lines in the update
branch and it is the kind of defect that only shows up two steps later, in a different feature.

**`pin` and `video` were added to the icon set, and that is not Story 44.4's job being done early.**
44.4 widens the set and makes the picker cover every default. What this story needed is narrower:
the two glyphs the deleted Pinned and Recordings rows already drew, so that the rail after the change
looks like the rail before it. Without them those two rows would draw the fallback `Layers` glyph —
correct behaviour for an unknown icon (43.4 pins exactly that), but a visible regression in the
surface this story replaces. Coordinated with the 44.13 author, who owns `space-editor.tsx` this
wave; the edit is the lucide import and two map entries, nothing below them.

**`write_note` was split rather than reused.** The ledger is not a note: announcing it with `touch`
would ask the reconciler to re-read a path the walk never collects, and `mark_dirty` would put a
commit cadence behind a file the user never touched. `write_vault_file` is `write_note` minus those
two announcements, and `write_note` now calls it, so there is still one atomic-write path.

**Today is deleted and its action is not.** AD-80 is about a row that never worked. `⌘⌥J`, the
palette's "Today's Journal", the tray item and `notes_journal_today` all still open or create today's
entry — that is FR-99 and it works. What is gone is the row in the notes rail that pretended to be a
filter and was not. The grep is in *Verification*.

**`isScopeOnly` and `emptyFilterReason` did not need to change.** Both enumerate the *chip* axes and
ask nothing about the scope's kind, so collapsing the union left them correct as written. That is
worth saying because it is the reason the empty-state logic survived a seven-variant type becoming a
three-variant one with one branch edited.

## Verification

Every test below was proved by mutating the code it defends, watching it fail, and restoring. The
tree was re-run clean after the last restore: 255 `keeper-core` `notes::` tests and 555 frontend
tests across `src/components/notes/`, `notes-filters.test.ts` and `src/hooks`.

### `keeper-core`

| Mutation | Tests that caught it |
|---|---|
| `plan` stops consulting the ledger on the automatic run | `a_default_the_ledger_already_offered_is_never_written_again`, `a_ledger_key_this_build_does_not_know_survives_a_read` |
| `plan` stops checking which defaults are on disk by key | `a_default_that_was_renamed_is_still_that_default` |
| `plan` stops checking names, so a user's own Inbox gets a second beside it | `a_user_space_that_already_has_a_defaults_name_stands_the_default_down` |
| Restore obeys the ledger, so it can never fill a hole | `restore_writes_the_missing_and_leaves_the_present_alone` |
| An unreadable ledger reads as "never seeded" | `an_unreadable_ledger_stops_the_automatic_seed_and_not_the_manual_one` |
| `render_note` stops writing `keeper.default` | `a_seeded_note_is_the_same_shape_a_saved_space_is`, `a_seeded_note_carries_its_key_and_reads_it_back` |
| Inbox is given a fresh `is:inbox` instead of the row's `is:untagged` | `every_default_query_parses_against_the_closed_flag_set`, `the_defaults_run_the_queries_the_deleted_rows_ran` |
| `default_key` stops matching the marker against the known set | `an_unrecognised_default_marker_names_no_default` |

`a_half_written_seed_converges_instead_of_doubling_up` and
`a_users_own_spaces_neither_block_the_defaults_nor_are_counted_as_them` are not in that table because
every mutation that breaks them breaks a listed test first. They are kept because they are the two
scenarios the story names by hand, and a future change to the filter order could separate them.

One mutation could not be run as written: replacing both `FirstRun` arms with a single `None` makes
`let ledger = match …` unable to infer its `Option<&BTreeSet<String>>`, so the crate does not
compile. The behaviour it would have tested is covered by the ledger-filter mutation above.

### `keeper-core`, the run against a real directory

Added after the field report. These drive `seed` through the `SeedVault` port against a temp
directory, so they cover the reads and the error classification the first version could not test.

| Mutation | Tests that caught it |
|---|---|
| `read_ledger` maps every read failure to "absent" — **the shipped bug** | `a_ledger_that_cannot_be_opened_is_not_read_as_an_absent_one` |
| `read_ledger` maps every read failure to a permanent block, `NotFound` included | 8 tests, every one that expects a fresh vault to be seeded |
| `read_ledger` treats an unparseable ledger as empty | `an_unreadable_ledger_blocks_the_run_with_a_sentence_naming_the_file` |
| The refusal sentence drops the errno | `a_ledger_that_cannot_be_opened_is_not_read_as_an_absent_one` |
| `read_existing` swallows the listing error and reads it as "no spaces" — **the second bug** | `a_spaces_directory_that_cannot_be_listed_blocks_the_run_rather_than_reading_as_empty` |
| `seed` reports an empty write instead of naming the already-satisfied case | `a_second_run_over_the_same_directory_is_already_satisfied_and_says_so`, `a_deleted_default_is_not_resurrected_by_the_next_run` |
| An unreadable ledger blocks Restore too, so the user cannot repair it | `an_unreadable_ledger_blocks_the_run_with_a_sentence_naming_the_file` |
| The collision counter forgets the filenames already in `spaces/` | `one_unreadable_space_does_not_take_the_run_down_and_is_not_written_over` |
| A partly-finished run records nothing, so a deleted default comes back | `a_run_that_stopped_still_recorded_what_landed_so_deleting_it_sticks` |

Two of those mutations survived the first pass and each exposed a genuinely missing test rather than
dead code: the ledger-read one, because the only unreadable-ledger test used *unparseable JSON* and
nothing covered a file that cannot be *opened* — which is the class that fits the owner's machine;
and the record-on-stop one, because the on-disk check alone makes the immediate retry correct and
only a *later deletion* distinguishes it. Both tests were added, and both mutations now fail.

### Frontend

| Mutation | Tests that caught it |
|---|---|
| A hard-coded Today + Inbox rail is put back above the groups | `renders the four defaults as spaces, and nothing when the vault has none`, `has no Today row, and no row that opens a note instead of filtering`, `selects the unfiled through the seeded Inbox…` |
| `noteQueryFor` stops sending `spaceId` | 4 pane tests + `asks for a seeded default's notes through its space id…`, `sends a space id rather than a flag for a space scope` |
| The `no-recordings` sentence keys on the space's *name* | `keeps the recordings sentence after the space is renamed, and does not lend it out` |
| The row stops carrying `defaultKey` onto the scope | `says the vault has no recordings…`, `keeps the recordings sentence after the space is renamed…`, `carries a seeded default's key onto the scope…` |
| `SpaceList` goes back to rendering `null` on an empty list | `shows the restore control on a vault with no spaces at all`, `cannot be pressed with no vault open` |
| Restore does not re-read the list | `re-reads the list after restoring, so the recreated spaces appear` |
| Restore claims success when nothing was missing | `says nothing was missing rather than claiming it restored something` |
| A failed restore is swallowed | `falls back to a plain sentence when the rejection carries no message` |
| A refusal shows the generic apology instead of the reason Rust gave | `shows the reason keeper gives, naming the file, rather than a generic apology` |

### The `Today` grep

`grep -n "Today"` across `src/`, `src-tauri/crates/keeper/src/` and
`src-tauri/crates/keeper-core/src/` returns nothing that is a rail row. What remains, and why:

- **FR-99's journal action**, deliberately kept: `openJournalToday`, `notesJournalToday`,
  `notes_journal_today`, `tray_journal_today`, the palette's `notes-journal-today` ("Today's
  Journal"), the tray label, and `⌘⌥J` in `use-notes-shortcut.ts`. A different surface, a different
  name, and it works.
- **`query.rs`'s `DateSpec::Today`** — the `date:created>=today` token the epic itself names as the
  thing an ordinary space would use instead.
- **Unrelated English** — `slash-menu`'s "Today's date" command, `format-time`'s doc comment, two
  test comments in recording code.
- **This story's own prose and its test**, which assert the row's absence.

### What could not be verified on this host

**The `keeper` shell crate does not build on Linux.** `pkg-config` is absent and there is no
`gobject-2.0.pc`; `cargo check -p keeper --lib` fails in the `gobject-sys` build script before a
single line of keeper's own code is compiled, and this container has no root to install either. So
every line in `notes_ipc.rs`, `notes_vault.rs` and `lib.rs` is **unexercised here**, and the macOS
gate is its first real check.

After the field report that surface is much smaller, and shrinking it *was* the fix. What is left:

- **The four `VaultSeedFiles` bodies** — `read_to_string`, `read_dir`, the note-versus-ledger write
  split, and the ULID. One call each. The classification they feed is `keeper-core`'s and is tested
  against a real directory, including `chmod 000` on both the ledger and `spaces/`.
- **The `refresh` loop** — that `engine_if_open()` is `Some` by the time `notes_vault::start` runs
  (read from `start_supervisor`, which builds the engine synchronously before it spawns), that a
  cold registry sends every vault down the `_` arm, and that seeding after `drop(guard)` cannot
  deadlock against the registry lock.
- **`notes_spaces_restore_defaults`** and its command registration.
- **`space_def`'s `default_key`**, `notes_spaces` emitting it, and `notes_space_save` carrying the
  marker through the splice.
- **`write_vault_file`**, and that `contained` accepts `.keeper-spaces.json` — its `is_internal`
  check is an exact `==` against `.keeper`, read rather than run.

Three specific claims are **asserted but not run**: that `.keeper-spaces.json` at a vault root is
not excluded by `keeper-sync`'s tier-0 corpus (read from `exclude.rs`'s own `sub/.keeperrc` test
rather than observed); that it never enters the note index (read from `walk`'s `rel.ends_with(".md")`
filter rather than observed); and that Obsidian hides it. The first two are one-line reads of code in
this repo; the third is a fact about another application and nothing here can prove it.

**Nothing was driven in a browser.** The rail, the Restore control and its three sentences are
asserted through jsdom: accessible names, `disabled`, the exact `notesSpacesRestoreDefaults` call and
the rendered copy. That the `RotateCcw` glyph in the Spaces header reads as "restore" rather than
"refresh the list", and that a rail of four seeded rows looks like the rail it replaced, are
judgements only a person looking at the screen can make.

**The IPC boundary is mocked, so the seeded queries are pinned in both languages.** The pane test's
`SEEDED_SPACES` fixture carries the exact strings
`the_defaults_run_the_queries_the_deleted_rows_ran` pins in Rust, and its fake `notes_list` throws on
a query it does not know rather than matching everything — a fake that shrugged would turn a broken
lens into a green test. The remaining untested link is the last one: that `render_note`'s bytes reach
the file and come back as a `NoteSpaceVm` with the right `defaultKey`. That is shell code, and it is
on the macOS gate with the rest.

## Postmortem: it gated green and did nothing

The first version of this story passed on Linux and macOS with zero binding drift, was installed on
the owner's machine, and **wrote nothing into their vault and logged nothing about it**. 305k lines
of debug log contained neither the `info!` for a successful seed nor the `warn!` for a failed one.

### The defect

`apply_default_spaces` had a silent arm. It returned `Result<Vec<String>, String>`, and an empty
`Ok` meant two opposite things: *the vault already has every default* and *keeper could not read
something, so it declined*. `seed_default_spaces` matched that with `Ok(_) => {}`. A feature that
declines to act has to say so; this one could not, so a field report of "it did nothing" was
unanswerable from the log.

Underneath the silence, one read was classified wrongly and one was not classified at all:

- **`read_seed_ledger` mapped every `io::Error` except `NotFound` to "keeper cannot tell", and
  "cannot tell" to a permanent, silent no-op.** On a vault sitting on removable media that is the
  likely class: `EACCES` from macOS TCC on `/Volumes`, `EIO` from a drive that spun down. Being
  timid about *writing* is right — that is the AD-79 argument and it stands. Being timid about
  *saying* is what shipped the invisibility.
- **`existing_spaces` read `spaces/` through `notes_vault::siblings`, which `unwrap_or_default()`s
  the listing error.** That is the same conflation in the opposite direction and nobody had noticed
  it: a sleeping USB volume would have read as "this vault has no spaces" and written a second Inbox
  beside the first. Found while building the harness below, not by the field report.

### What was ruled out, and how

| Candidate | Verdict | Evidence |
|---|---|---|
| The presence rule matched the owner's four `recordings · first-recording` spaces | **Disproved on disk** | `the_owners_vault_gets_its_four_defaults_beside_the_four_spaces_it_already_had` builds their exact `spaces/` in a temp directory with no ledger and asserts all four defaults are written. `naming::slug` folds their name to `recordings-first-recording`, which equals no default. |
| `refresh` never reached the hook | **Disproved by reading, then removed as a possibility** | `sync::start_supervisor` calls `engine(platform)` **synchronously** and fills the slot before spawning, so `engine_if_open()` is `Some` on the next line where `notes_vault::start` runs; a cold registry then sends every vault down the `_` arm. The freshness dependency is deleted anyway — see below. |
| The ledger read declined | **The only path left, and it cannot be narrowed further from here** | Three sub-causes fit every measurement: a non-`NotFound` errno, a ledger present and unparseable, and a ledger present and *correct* because an earlier run seeded and the notes were later removed. The third is invisible in sync activity because `record_seed_ledger` writes through `write_vault_file`, which deliberately skips `touch`/`mark_dirty` — which is exactly why the "no `spaces/` writes" and "`file_state` is 0" greps came back clean. |

**The honest statement: I cannot separate those three from this host.** What I can do is make the
next launch say which one it was in one line, and make two of the three impossible to be permanent.

### The fix

- **`SeedOutcome` has four arms and no silent one**: `Wrote(paths)`, `AlreadySatisfied`,
  `Blocked(sentence)`, `Stopped { written, reason }`. The shell logs each — `info`, `debug`, `warn`,
  `warn` — and a refusal names the file *and* the errno.
- **A refusal records nothing, so the next refresh retries.** It is a pause, not a verdict.
- **`spaces/` that cannot be listed now blocks** instead of reading as "no spaces".
- **Restore is never blocked by an unreadable ledger** and repairs it: the user is looking at the
  rail, and the ledger's job is to stop keeper acting *on its own*.
- **The refusal reaches the user.** `SpaceList` shows Rust's sentence rather than the generic
  apology, because ".keeper-spaces.json could not be read (permission denied)" sends someone to the
  file and "keeper couldn't restore the default spaces" sends them to a bug report.
- **Seeding runs for every registered vault on every refresh, not only a newly registered one.** The
  run plans against what is on disk and refuses to write when it cannot read, so repeating it is
  free. The freshness filter bought one `read_dir` per refresh and cost the second chance — the
  difference between a vault that heals itself and one that needs a menu item the user has no reason
  to press.

### The test that would have caught it

The repo's recurring lesson landed exactly on this story: **every assertion was below `plan`, and
all the risk was above it.** `plan` is a pure function over hand-placed values; the vault is a
directory on a pendrive.

So the reads are now a port, `default_spaces::SeedVault` — `read`, `list`, `write`, plus the id and
the two clocks — and the whole run is `default_spaces::seed`, in a crate that builds on every host.
The shell's adapter is four bodies of one call each. Twelve tests drive the real run against a real
temp directory with real permission bits, including:

- the owner's vault, reproduced from the field report;
- a ledger `chmod 000` — the class that fits removable media, and the case that had **no test at
  all** before;
- a `spaces/` `chmod 000`;
- one unreadable space among readable ones;
- an unplugged drive mid-write, and the same again after the user deletes what landed.

`contained`'s refusal is mapped to `io::Error::other` in the adapter rather than to `NotFound`, so a
containment refusal can never again arrive at the seeder disguised as an absence.

### What is still unprovable here

The four adapter bodies and the `refresh` loop. That is a much smaller surface than the previous
version's — the ledger read, the directory read, the error classification, the plan, the filename
allocation, the write order and the ledger record all moved into tested code — but it is not zero,
and the macOS gate is still their first real check.

**And the diagnosis itself is not closed.** If the next launch on the owner's machine logs
`notes: default spaces already settled for this vault`, the cause was a ledger that was there and
correct, and the recovery is Restore. If it logs `notes: did not seed the default spaces`, the
sentence names the file and the errno. Either way it is one grep, which is what this story owed and
did not deliver the first time.

## Deliberately NOT Done

- **No `order` or `sort` on a space.** Story 44.4 owns both. The seeded defaults carry
  `sort: modified desc`, which is what `space_def` already falls back to, and they land in the rail
  in the order `notes_spaces` sorts by — name — which happens to be Inbox, Journal, Pinned,
  Recordings. That is the rail's old order by luck, not by design, and 44.4 makes it deliberate.
- **The icon set was not widened.** Only `pin` and `video` were added, for the reason above. Covering
  the defaults properly, and everything else the picker should offer, is 44.4's AC.
- **No default template, no New Note from a space.** 44.6 and 44.7.
- **No migration of an existing user's `SCOPE_FLAG` habits.** There is nothing to migrate: the four
  rows held no state, so a vault that already has spaces simply gains four more the first time this
  build opens it.
- **Restore is not offered per-default.** One action that fills every hole, not four. A per-row
  "bring this one back" control would need a list of the absent, which is a surface for a case that
  is one press away from being handled wholesale.
- **`notes_rename`'s doc-comment lie is still there** (`notes_ipc.rs`, reported by 43.4). Untouched
  again, for the same reason: it affects every note rather than only spaces.
- **The ledger is not repaired when nothing is missing.** A corrupt `.keeper-spaces.json` on a vault
  that already has all four is left alone, because `plan` returns an empty list and the run stops
  before the write. The effect is that automatic seeding stays blocked on that vault — which is the
  behaviour a deleted default wants anyway, so repairing it would be work in service of no outcome.
