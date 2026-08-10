# Spec 45.3 — The Files Surface Can Write

status: implemented
created: 2026-08-10
epic: 45 (open it, change it, put it back)
binds: FR-175, FR-176, AD-89, UX-DR66
retires: AD-75
depends on: 43.8 (the Files tab), 44.15 (`browse_root`), 44.16 (the CSV editor's write path),
44.17 (sync marks in Files)

## The decision this story reverses, written where the next reader will find it

**AD-75 — "the files surface never writes" — is retired by AD-89, deliberately and by the
owner.** The old rule was right while Files was a window onto the sync engine's world: keeper's
promise about a synced folder is that it never moves a file you did not ask it to move, and a
browser with a delete key in it is the shortest path to breaking that promise by accident. The
owner has now asked the surface to delete and create. The honest answer was to give it one write
path rather than to leave it read-only and watch a second one grow beside it in six months.

The reversal is recorded in four places, because those are the four places a reader arrives from:

| Where | What it says now |
| --- | --- |
| `keeper-sync/src/files_write.rs` module doc | The full argument: why AD-75 was retired and the three promises that replaced it |
| `keeper-sync/src/browse.rs` module doc, first paragraph | AD-75 retired by AD-89; this module is *still* read-only, and here is where the write decision lives instead |
| `src/components/layout/files-pane.tsx` module doc | The same, from the surface's side, plus what still does not exist |
| `files-pane.test.tsx`, at the guard | AD-75 retired by AD-89, owner's decision, epic 45 — and what the guard holds now |

What replaced one broad promise is three narrower ones:

1. **Only inside a notes vault.** A file outside one is listed, viewed and opened, and the surface
   *says why* it cannot be changed rather than offering an action that will fail.
2. **One writer.** Every byte goes through `notes_vault::write_vault_file` + `mark_dirty`; every
   removal goes through `notes_vault::trash_note`. Never a second writer, never a reach into the
   engine.
3. **Every destructive act is confirmed, and the confirmation names the file.**

## What was already there, and what was dead

The epic keeps finding the asked-for thing already present. Here is what the search found:

* **The removal path the reconciler understands already existed, and the story was right to make me
  look before inventing one.** `notes_vault::trash_note` renames a file into
  `<vault>/.keeper/trash/<ulid>/<rel>`, then `touch`es the path and `mark_dirty`s the vault. `.keeper`
  is already a tier-0 sync exclusion (`exclude.rs:159`), so git sees a deletion and commits it — the
  change is announced, not discovered on the next scan. NFR-30 forbids an `unlink` and this is why:
  the bytes stay recoverable locally *and* the commit that deletes the file is preceded by one that
  still holds it. **Nothing new was written for deletion.**
* **`write_vault_file` already existed with exactly the right seam** (44.3's default-space ledger,
  44.16's CSV editor). Its doc already drew the `touch`/no-`touch` split this story needed.
* **The containment rule already existed twice over** — `browse::resolve`'s lexical half and its
  canonicalising half. Rather than a third copy, `resolve` was split: `plain_segments` (the lexical
  rule) → `lexical_join` → `resolve`. `files_write` calls `plain_segments`. One rule, three callers.
* **`Engine::pending` already answered "does this file sync"**, and 44.17 already joined it to a
  listing. `browse::classify` was reachable only from the listing loop, so `browse::status_of` was
  added as a second *call site* of the same function — the delete confirmation and the row it was
  opened from cannot word one file's state differently.
* **Nothing was dead-but-present for the write path itself.** `FilesEntryVm` had no unused
  writability field, `sync_ipc.rs` had no half-wired write command, and `Cargo.lock`/`package.json`
  gained nothing: **this story adds no dependency.**

## The decisions this story was asked to make and justify

### Writability is data on the listing, not an error from an attempt

The story's rule is that a file outside a vault "can be listed and viewed but not written: the
surface says why rather than offering an action that will fail". That is only expressible if the
surface knows *before* it renders the control. So `FilesEntryVm.write: FilesWriteVm { writable,
reason }` rides with the entry and `FilesListingVm.write` rides with the directory.

They answer two different questions and it matters:

| Object | Question | Vault root | A folder | A file in the vault | A file outside |
| --- | --- | --- | --- | --- | --- |
| `FilesEntryVm.write` | may keeper change or remove THIS | no (`VaultRoot`) | no (`IsDirectory`) | yes | no (`OutsideVault`) |
| `FilesListingVm.write` | may keeper create a file IN HERE | yes | yes if inside the vault | n/a | no |

`reason` is `Some` exactly when `writable` is false, and it is a whole sentence composed by
`WriteRefusal`'s `Display` — in `keeper-sync`, so it is asserted on the machine the code is written
on, exactly as `BrowseRefusal`'s is.

**This is the LOCATION half only.** W1Registry's `ViewerEntry.writable` is the FORMAT half. An edit
needs both: a PDF in a writable folder is not editable, and a Markdown file outside a vault is not
either. A *delete* consults only the location half, because removing a PDF is not editing one.

### The scope comes from the LIVE vault, never from `profile.notes`

`WriteScope::new(profile_name, subfolder)` takes the subfolder of the vault the shell can actually
reach (`notes_vault::vault(id)`), not the one the profile config names. A profile configured with a
vault the registry has no slot for — unflagged, root moved, still starting — is exactly where the
flag the surface renders and the answer the command gives would diverge, and a pane told "writable"
by configuration and refused by the registry is a pane offering an action that will fail.

### Containment is compared over components, never over the raw string

A folder called `10-notes-archive` beside a vault at `10-notes` *starts with* the vault's name and is
not inside it. A `starts_with` on the string would have let a write out of the vault into its
neighbour. `WriteScope::vault_relative` walks the subfolder's components against the subpath's.
Mutation **M1** is exactly that mistake, and it is caught.

The configured subfolder is normalised once, at construction: `NotesConfig::validate` deliberately
*refuses rather than corrects*, so `Notes/`, `\Notes` and `a//b` all reach here verbatim while the
dirent path is always `/`-joined. Case is deliberately **not** folded — unlike the folder-role
marker, which decides a glyph, this decides where bytes land.

### The confirmation is composed in Rust, and it costs one extra round trip

`sync_delete_plan` is a separate command from `sync_delete_entries`. The plan is built by the same
code the delete runs — same scope, same resolution, same sync answer — so the dialog cannot promise
something the command will then refuse, and a file that vanished between the click and the
confirmation is *named* as a refusal rather than dropped in silence.

The sentences come from `FilesDeletePlanVm::compose`, a pure function in `keeper-core`, so every one
of them is asserted on Linux. A confirmation assembled in TypeScript from a count and a glyph would
be a second, unverified reading of the engine's answer, in the one place a wrong reading costs a
file.

`FilesSyncStatusVm::Unknown` **counts as syncing, and the sentence says so.** The two available
guesses are "this deletion stays on this machine" and "this deletion travels", and only one of them
is safe to be wrong about; silently picking the quiet one is the lie `Unknown` was introduced to
refuse.

### `touch` is included on write, where 44.16 excluded it

44.16's target is an embedded `.csv`, which the notes walk never collects, so telling the reconciler
about it would ask for an index entry that cannot exist. The Files surface can save a `.md` *inside
the vault*, which is a note — so the index must be told. `apply_batch` skips anything that is not
`.md` at no cost (`notes_vault.rs:918`), so including it is free and omitting it would leave a note
edited from Files invisible to search until an unrelated event moved the reconciler. The same
applies to delete, where `trash_note` already does it.

### A folder is refused, by name, rather than recursively deleted

One confirmation over a folder holding 100 000 files is not a confirmation. `WriteScope::file`
refuses `IsDirectory` with a sentence naming the folder, and `is_dir` comes from the dirent rather
than the name — a directory called `notes.md` exists (mutation **M3**).

### The collision check is case-insensitive, and an unreadable directory is a refusal

APFS and NTFS are case-insensitive by default: `README.md` created beside an existing `readme.md`
does not create a second file, it replaces the first with an empty one. An exact-match check passes
on this Linux box and destroys a file on the Mac it ships to (mutation **M4**). A directory that
cannot be read is a **refusal**, never a cleared check — that is the shape of the epic-44 defect
where `notes_create` could overwrite a note through an unreadable directory (mutation **M5**).

Residual race, named rather than hidden: the check and the write are two syscalls apart. Closing it
would need a `create_new` open, which would be a second writer — the constraint this story is built
on. The window is sub-millisecond and both racers are keeper itself.

### Saving is not creating

`sync_write_entry` refuses a path that is not on disk. A stale editor whose file was deleted
elsewhere must not put it back, and the collision rule lives on `sync_create_entry` where a person
is choosing a name.

### Multiselect is the model, and it is scoped to one profile

Node keys, not paths — a path is unique only within a profile and the tree shows several at once.
Plain click replaces, Cmd/Ctrl toggles, Shift takes the run over the flat visible order. Crossing
into another profile **replaces** whatever the modifier said, because every write command is scoped
to one profile id and half a selection no command can act on is worse than a selection that visibly
reset.

Delete lives in the header, not on the row: a per-row Delete button cannot answer "and the other
four", and the count belongs beside the control that acts on the count.

## I/O matrix — `keeper_sync::files_write`

| # | Scope | Call | Result |
| --- | --- | --- | --- |
| 1 | no vault | `file("clip.mov", false)` | `NoVault` — "Field holds no notes vault…" |
| 2 | `10-notes` | `file("10-notes/a.md", false)` | `Ok("a.md")` |
| 3 | `10-notes` | `file("10-notes-archive/a.md", false)` | `OutsideVault` — the neighbour is not the vault |
| 4 | `10-notes` | `directory("")`, `directory("recordings")`, `file("readme.md", false)` | `OutsideVault`, each naming `(10-notes)` |
| 5 | `a/b` | `directory("a/b")` | `Ok("")` — the vault root can be created in |
| 6 | `a/b` | `file("a/b/c/d.md", false)` | `Ok("c/d.md")` |
| 7 | `a/b` | `directory("a")` | `OutsideVault` — a parent is not inside |
| 8 | any | `file("..")`, `"../etc"`, `"10-notes/../../etc"`, `"/etc/passwd"`, `"."`, `"10-notes/./a.md"`, `"10-notes//a.md"`, `"10-notes/a.md/"` | `Escapes`, refused before the vault question |
| 9 | `10-notes` | `file("10-notes", true)` | `VaultRoot` — keeper will not delete its own vault |
| 10 | `10-notes` | `file("10-notes/notes.md", true)` | `IsDirectory` — from the dirent, not the name |
| 11 | `10-notes` | `file("10-notes/notes.md", false)` | `Ok("notes.md")` — same path, file dirent |
| 12 | `10-notes` | `create("10-notes", "Report.md")` | `{vault: "Report.md", profile: "10-notes/Report.md"}` |
| 13 | `10-notes` | `create("10-notes/daily", "2026-08-10.md")` | both frames, joined in Rust only |
| 14 | `10-notes` | `create("recordings", "x.md")` | `OutsideVault` — location before name |
| 15 | `10-notes` | `create(_, "")`, `create(_, "   ")` | `NameEmpty` |
| 16 | `10-notes` | names `a/b.md`, `..`, `.`, `/x.md`, `" lead.md"`, `"trail.md "`, `a\b.md` | `NameNotPlain` |
| 17 | `10-notes` | a 258-byte name | `NameTooLong { bytes: 258 }`; exactly 255 is allowed |
| 18 | `10-notes` | `"é" × 128` | `NameTooLong { bytes: 256 }` — bytes, not characters |
| 19 | `10-notes` | `.keeper`, `.obsidian`, `.git`, `.KEEPER` | `NameReserved`; `.gitignore` is fine |
| 20 | — | `collides(dir, "README.md")` beside `readme.md` | `Ok(true)` — case-insensitive |
| 21 | — | `collides(dir, "daily")` beside a `daily/` folder | `Ok(true)` |
| 22 | — | `collides(missing_dir, "x.md")` | `Err(Unreadable)` — never `Ok(false)` |
| 23 | — | `resolve_existing(root, "gone.md")` | `Missing` |
| 24 | — | `resolve_existing(root, "escape")`, a symlink out of the root | `Escapes`, after canonicalisation |
| 25 | `Notes/`, `/10-notes`, `\10-notes`, `a//b`, `/a/b/` | any | normalised; the sentence names the normalised form |

## I/O matrix — `FilesDeletePlanVm::compose`

| # | Files | `question` | `consequence` |
| --- | --- | --- | --- |
| 26 | one, `Synced` | `Delete 10-notes/Report.md?` | "This file syncs, so deleting it here removes it from every machine that syncs Vault." |
| 27 | three | `Delete 3 files?` | "These 3 files sync…" |
| 28 | one, `Excluded` **or** `NotInRepository` | names it | "This file does not sync, so this removes it from this machine only." |
| 29 | two, both local | counts | "None of these 2 files sync…" |
| 30 | 2 syncing + 1 excluded | `Delete 3 files?` | "2 of these 3 files sync… the other 1 do not and go from this machine only." |
| 31 | one, `Unknown` | names it | "keeper could not read this file's sync state, so it has assumed it syncs…" |
| 32 | `Synced` + `Unknown` | counts | "These 2 files sync…" + "could not read the sync state of 1 of them, and has counted it as syncing" |
| 33 | none deletable | "There is nothing here keeper can delete." | `""` — and `recovery` `""`; a leftover sentence would describe a deletion that is not going to happen |
| 34 | any | — | `recovery` names the vault's trash, singular/plural correct |

## I/O matrix — the pane

| # | Situation | Behaviour |
| --- | --- | --- |
| 35 | plain click | selects that row only; the previous selection is replaced |
| 36 | Cmd-click | adds one row; the gap between is NOT taken |
| 37 | Ctrl-click | same as Cmd — jsdom reports a non-Mac platform, so this is asserted on its own |
| 38 | Shift-click | takes the run over the flat visible order |
| 39 | click on a row's own button | changes neither selection nor panel |
| 40 | selection of 2 | header shows `2 items selected` and a Delete |
| 41 | Delete pressed | `syncDeletePlan(profileId, [subpaths])`; **nothing deleted**; dialog shows Rust's question, consequence, recovery, and every file named |
| 42 | confirm | `syncDeleteEntries(profileId, plan.files)`; the parent folders — and only those — are re-read; the selection empties; panels holding those files close |
| 43 | receipt with refusals | each reason rendered in `role="alert"`; the selection is not silently shrunk |
| 44 | a file whose `write.writable` is false | selectable, no Delete offered, `write.reason` is the row's explanation |
| 45 | a listing whose `write.writable` is false | no New file, no text box |
| 46 | New file | an inline named field at the top of that folder, autofocused |
| 47 | Create | `syncCreateEntry(profileId, subpath, name)` — the directory and the name cross separately; the folder is re-read and the new row appears |
| 48 | a colliding name | Rust's sentence in `role="alert"`, the field keeps what was typed, and no re-read happens |
| 49 | Delete key on a row | selects it and opens the confirmation; never deletes |
| 50 | Escape | clears the selection |
| 51 | Enter / Escape in the name field | create / cancel |
| 52 | any row | `aria-selected` on every node row, `aria-multiselectable` on the tree |

## Where a decline is announced

DW-162: a path that can decline to act says so at INFO or above; `tracing::debug!` never reaches the
packaged app's log.

| Decline | User-visible | Log |
| --- | --- | --- |
| Any `WriteRefusal` reaching a command | Rust's sentence, verbatim | `warn!` (`GatedMakeWriter` writes INFO only in debug mode; a refusal must already be on disk) |
| A per-path delete refusal inside a batch | the sentence, in the receipt | `warn!` |
| A vault write that failed on disk | the `NotesError` sentence | `warn!` |
| A delete that removed nothing at all | the refusals | `info!` "files: delete removed nothing" |
| A file trashed | the folder re-reads | `info!` with the grave path |
| A file written / created | — | `info!` |
| A location that is not writable | `write.reason` where the control would have been | none — this is not a decline, it is an absence, and it is visible before anything is attempted |

## Tests, and the mutations that prove they bite

Harness: `~/.w1fileswrite/sweep.py` — **private, never `/tmp`**. It runs the unmutated baseline at
exactly the verdict's scope **before and after** the sweep, aborts outright if the opening baseline
is red, writes a marker naming the applied mutant so a killed run can be repaired rather than
guessed at, restores in a `finally`, and byte-compares (SHA-256) every touched file after the sweep.
It reports `DID-NOT-COMPILE` and `DID-NOT-FINISH` as verdicts distinct from `SURVIVED`, and treats a
missing anchor as an **alarm** (`PATTERN-NOT-FOUND`) rather than a skip.

That gate earned its keep: the first run **aborted** because `cargo test -p keeper-core --lib vm::`
came back red with no mutation of mine applied. It was a sibling's live mutant in a shared worktree,
confirmed by them within minutes. Without the opening baseline the harness would have measured three
verdicts against someone else's broken code.

**15 mutations, 15 caught, 0 survived, 0 unproved.** Baseline green at both ends of every run.

### `keeper-sync` — `cargo test -p keeper-sync --lib files_write` (16 passed, both ends)

| Mutation | Caught by |
| --- | --- |
| M1 vault containment becomes a raw string prefix | `a_folder_whose_name_extends_the_vaults_is_not_inside_the_vault` |
| M2 the lexical traversal check is skipped | `traversal_is_refused_wherever_it_is_aimed` |
| M3 a folder is deletable like a file | `a_directory_is_refused_as_a_folder_whatever_it_is_named` |
| M4 the collision check becomes case-sensitive | `a_collision_is_found_whatever_case_it_was_written_in` |
| M5 an unreadable directory reports no collision | `a_directory_that_cannot_be_read_refuses_rather_than_reporting_no_collision` |
| M6 a missing path resolves instead of refusing | `a_path_that_is_gone_is_missing_rather_than_resolved` |
| M7 a name is not checked for shape | `a_name_must_be_one_plain_file_name` |

### `keeper-core` — `cargo test -p keeper-core --lib vm::` (273 passed, both ends)

| Mutation | Caught by |
| --- | --- |
| M8 an unreadable sync state is counted as not syncing | `an_unreadable_sync_state_is_counted_as_syncing_and_admitted` |
| M9 the confirmation always counts and never names | `a_delete_confirmation_names_one_file_and_counts_several`, `a_plan_that_can_delete_nothing_asks_no_question` |
| M10 a refusal carries no reason | `a_write_verdict_carries_a_reason_exactly_when_it_refuses` |

### The pane — `bun run vitest run src/components/layout/files-pane.test.tsx` (65 passed, both ends)

| Mutation | Caught by |
| --- | --- |
| M11 a plain click accumulates instead of replacing | `selects one row on a plain click and replaces the selection on the next` |
| M12 Delete is offered wherever something is selected | `offers no delete for a file the location refuses, and shows Rust's reason instead` |
| M13 New file is offered in any open folder | `offers no New file in a folder Rust says it cannot write to` (+ the delete guard) |
| M14 the delete skips the confirmation | `never deletes without a confirmation that named what goes` (+ two more) |
| M15 a refused create says nothing | `keeps the name on screen and shows Rust's sentence when the name collides` |

### The guard tests 43.8 wrote for AD-75, rewritten rather than deleted

`offers no control that could write, rename, move or delete` and `does not grow a write control
while offering to show a name` were red after this story, correctly: they enforced the decision the
owner reversed. Deleting them would have left the pane with **no** invariant at all — strictly worse
than before, because AD-75 at least had a test and AD-89 would have had none. They were rewritten to
hold what the new rule needs, keeping the old names' spirit so `git log -S` leads from one to the
other:

* `offers no control that could rename, move or duplicate — only the two writes 45.3 built`
* `does not grow an unbuilt write control while offering to show a name`
* `offers no delete for a file the location refuses, and shows Rust's reason instead` (new)
* `never deletes without a confirmation that named what goes` (new)

`FILES_WRITE_CONTROL_LABELS` **was renamed to `FILES_UNBUILT_CONTROL_LABELS`**, because its meaning
changed under the same name and that is a trap with a familiar face. It no longer lists `Delete` (it
exists) or `Save` (45.6 built it); it lists Rename, Move, New folder, Duplicate, Paste, Cut, Upload —
the controls with no command behind them, which is the same drift the AD-75 version caught.

### One test-file change that touched other agents' tests

An open, writable folder row now has a second button, so `within(row).getByRole("button")` — used at
**27** call sites across three agents' tests — became ambiguous. Rather than thread an accessible
name through all 27, the test file gained `expander(row)` (`getAllByRole("button")[0]`, the toggle),
and every site now says which button it means instead of relying on there being only one. Announced
to W1TypeSize and W1Panels.

### On `metaKey` (Main's warning)

The pane's own handler reads `event.metaKey || event.ctrlKey`, so a mouse test with `metaKey` does
exercise it — but three modifier clicks inside one `act` cannot tell you which modifier was honoured.
The Cmd/Shift test asserts the selection size **after each** click, and Ctrl was split into its own
test (`treats Ctrl-click as Cmd-click, because one of them is the wrong platform`). No CodeMirror
`Mod-` binding is involved in this story.

## Deliberately NOT done

* **Rename and move.** AD-89 names them as things the owner asked for; no wave-1 story builds them.
  They stay in `FILES_UNBUILT_CONTROL_LABELS`, asserted absent, because a control that arrives before
  its command fails on click.
* **Deleting a folder.** Refused by name with a sentence. One confirmation over a folder holding
  100 000 files is not a confirmation, and a recursive trash is a different story with a different
  undo story.
* **Writing outside a vault.** That half of AD-75 was not reversed (the epic says so explicitly:
  FR-145, AD-65) and is asserted, not merely absent.
* **A conflict copy on a stale write.** `sync_write_entry` has no `rev` argument. 44.16 refuses a
  stale CSV edit because it holds a table read at a revision; a text save is 45.6's contract and
  their `TextFileVm` carries what a staleness check would need. Adding half of one here would have
  been a second staleness rule.
* **Undo, or a trash browser.** The bytes are in `<vault>/.keeper/trash/<ulid>/` and in git history;
  a surface for them is not this story.
* **A drag target.** No `Upload`, no drop zone. 45.13 owns attachment insertion.
* **Closing the create-collision race with `create_new`.** That would be a second writer.

## What I could not verify here, and why

**`cargo check -p keeper` fails on this Linux box before it reaches any of my code**: `glib-sys` and
`gobject-sys` cannot find `pkg-config` (there is no `pkg-config` binary and no GTK `.pc` files on the
machine). So the following has **never been compiled anywhere**:

* `sync_write_entry`, `sync_delete_plan`, `sync_delete_entries`, `sync_create_entry`
* `vault_and_scope`, `writable_profile`, `write_refused`, `deletable`, `DeleteTarget`,
  `notes_write_error`
* the `scope` parameter threaded through `files_listing_vm`, and the two `FilesWriteVm` projections
  in it
* the four registrations in `lib.rs`'s `invoke_handler`

Their behaviour is asserted only *indirectly*: through `keeper_sync::files_write` and
`keeper_core::vm`, which they are a thin projection over and which are fully tested here, and through
the frontend suite, which exercises the VM shape against a mock rather than against the real command.
**The macOS gate is the arbiter for those functions**, and for `cargo clippy`, `cargo fmt --check`,
`bun run lint`, `bun run typecheck` and `bun run bindings:check`, none of which were run here (the
brief forbids them; Main gates once at the end).

Three consequences worth naming rather than hiding:

1. **Every decision that could be moved out of the shell was moved** (AD-55, AD-56). The containment
   rule, the vault-scope rule, the name rules, the collision rule and every user-facing sentence live
   in `keeper-sync` or `keeper-core` and are asserted over real temp directories on this machine. What
   is unbuilt is glue: look up a profile, look up a vault, call the decision, call the one writer,
   project the VM.
2. **What is NOT proved anywhere runnable** is that the real `sync_browse` produces a listing whose
   `write` field matches what the pane renders — that is `files_listing_vm`, and it is unbuilt code.
   The two halves are each proved (the scope decision in `keeper-sync`, the pane's reading of the
   field against a mock); the wire between them is not.
3. **The generated bindings** (`FilesWriteVm.ts`, `FilesDeletePlanVm.ts`, `FilesDeleteReceiptVm.ts`,
   `FilesDeleteRefusalVm.ts`) were emitted by ts-rs during `cargo test -p keeper-core` — never
   hand-written — and will be regenerated by the macOS gate.

Also not verified here: the real filesystem behaviour this story reasons about on a
**case-insensitive** volume. `collides` is tested case-insensitively on ext4, where the two names are
genuinely different files; the case that matters — APFS treating `README.md` and `readme.md` as one
file — cannot be reproduced on this box. The refusal is strictly conservative (it refuses on both
kinds of filesystem), so the failure mode of being wrong is "keeper asked for another name", not
"keeper destroyed a file".

## Code map

* `src-tauri/crates/keeper-sync/src/files_write.rs` (new) — `WriteScope`, `WriteRefusal` (+ `Display`),
  `CreateTarget`, `collides`, `resolve_existing`, `MAX_NAME_BYTES`, and 16 tests.
* `src-tauri/crates/keeper-sync/src/browse.rs` — `plain_segments` and `lexical_join` split out of
  `resolve`; `in_repository` extracted; `status_of` added; module doc records the AD-75 reversal.
* `src-tauri/crates/keeper-sync/src/lib.rs` — `pub mod files_write;`.
* `src-tauri/crates/keeper-core/src/vm.rs` — `FilesWriteVm`, `FilesDeleteRefusalVm`,
  `FilesDeletePlanVm` (+ `compose`), `FilesDeleteReceiptVm`; `write` on `FilesEntryVm` and
  `FilesListingVm`; `FilesEntryVm::new` takes it.
* `src-tauri/crates/keeper/src/sync_ipc.rs` — the four commands and their helpers; `files_listing_vm`
  takes a `WriteScope`.
* `src-tauri/crates/keeper/src/lib.rs` — the four registrations.
* `src/lib/ipc/client.ts` — `syncWriteEntry`, `syncDeletePlan`, `syncDeleteEntries`,
  `syncCreateEntry`, and three type re-exports.
* `src/components/layout/files-pane.tsx` — the selection model, the header toolbar, the confirmation,
  the inline create row, `aria-selected`/`aria-multiselectable`, the retired-and-replaced guard
  constant, and the AD-89 module doc.
* `src/components/layout/files-pane.test.tsx` — the rewritten guards, 12 new assertions, `expander`.
