# Spec 45.17 — Tags You Can Edit, Spaces and Notes You Can Delete

status: implemented (Linux host; `keeper` shell crate never compiled)
story: Epic 45, Wave 3, Story 45.17
bindings: FR-194, FR-195, UX-DR78, NFR-30, AD-79
agent: W3TagsDelete

---

## What I found already there, before writing anything

The instruction was to check whether the asked-for thing already exists. Four of six pieces did.

| Asked for | Already there | Verdict |
|---|---|---|
| Delete a note | `notes_delete` (IPC, since epic 37), `notesDelete` (client), `deleteNote` (action), `NotesActions.remove` (hook binding) | **Complete end to end and reachable from nothing.** `remove` was bound to no surface, no keystroke, no menu. A destructive verb sat wired-and-unmountable in main for a whole epic — W3NoteFile's "declared and never mounted" shape with a delete behind it. |
| `trash_note`, never an unlink | `notes_vault::trash_note`, and `notes_delete` already used it | Nothing to add. 45.3 found the same function for the Files pane. |
| The "offered once" ledger | `default_spaces::{plan, seed, LEDGER_REL, parse_ledger, render_ledger}` | **The ledger genuinely can express "deleted on purpose".** No tombstone invented — see below. |
| A tag chooser | `TagCombobox` (44.13), `TagsProperty` (44.14) | Existed, and served three of five shapes a `tags:` key comes in. |
| Delete a space | nothing | Built. |
| A confirmation | `FilesDeletePlanVm` (45.3) as the precedent, nothing for notes | Built to that precedent. |

---

## The ledger question, answered

**The ledger already is the tombstone, and this story adds none.** `plan(FirstRun, …)` skips every key in `offered`, and `seed` records a key when it writes the note. So "keeper already offered this" and "the user threw this away" are the same fact from the seeder's side, and a deletion's whole job is to make sure the key is in that set.

`default_spaces::record_deleted(vault, source)` does exactly that and nothing else. It is **not** a no-op, and the case that makes it load-bearing is one the ledger could not already answer:

> `keys_recorded` is best effort — `let _ = vault.write(...)`. A vault whose ledger write failed has its seeded spaces on disk and **no ledger at all**. Deleting one of them then has to record it, because nothing recorded it when it was written, and the next `refresh` would otherwise write it straight back.

Reached in the test the way it happens in the field — the seed runs with the ledger path refused (a full disk, a read-only `.keeper-spaces.json`) — rather than by hand-removing a file.

### One residual hole, reported rather than fixed

`seed` records only the keys it **wrote**. A default it stood down because a space of the *user's own* already carried that name never reaches the ledger. So: a person with their own "Inbox", who later deletes it, gets keeper's Inbox on the next refresh.

I left 44.3 alone deliberately. The ledger's word is *offered*, and keeper never offered that one — it declined. The sentence is coherent and the deleted space was not a default. But the person cannot tell those two Inboxes apart, so it will read as a resurrection. **The smallest change that closes it is one line in `seed`: record the keys `plan` stood down for presence, not only the keys it wrote.** It changes 44.3's stated semantics, so it is the owner's call, not mine.

---

## What was built

### Rust (`keeper-core`, provable on this host)

- `default_spaces::DeleteRecord` — `Recorded` / `AlreadyRecorded` / `NotADefault` / `Blocked(reason)`. No silent arm, `SeedOutcome`'s rule. `report()` returns a level at or above `REPORT_FLOOR`, asserted.
- `default_spaces::record_deleted(&mut dyn SeedVault, source)` — reads `keeper.default` off the note's own bytes, reads the ledger through the same port `seed` uses, inserts, writes. An unreadable ledger **blocks and is not overwritten**, for `seed`'s reason: it may be a newer build's.
- `notes::vm::NoteDeletePlanVm` + `for_note` / `for_space` — the confirmation's words, composed where the removal is (45.3's rule). One struct for both, because a space **is** a note.

### Rust (`keeper` shell, never compiled here)

- `notes_delete_plan(vault_id, note_id) -> NoteDeletePlanVm`. A read failure **fails the call**: a confirmation keeper cannot compose honestly is one it must not show.
- `notes_delete` extended: reads the source **before** the bytes move, trashes, then records. A read failure does **not** fail the delete — the person asked, and it happened — but it becomes `Blocked` at WARN naming the file, never `NotADefault`, because a wrong answer wearing an absent one's clothes is how a space comes back and nobody knows why.
- `lib.rs`: `notes_delete_plan` registered.

**One removal path for a note and for a space.** A second command for spaces would have been the one that forgot the ledger.

### Frontend

- `note-delete-dialog.tsx` — one confirmation, three hosts. Every word is Rust's. `preventDefault` on the action keeps it mounted through the command so a refusal has somewhere to be said.
- `note-actions.tsx` — the per-note menu in the editor header, with a `children` slot above the destructive item. Consumed by 45.21 (`ExportNoteItem`) and 45.15 (`CaptureNoteItem`).
- `space-list.tsx` — a delete control per row, after the pencil; clears the scope only when the deleted space was the active one.
- `note-list.tsx` / `notes-pane.tsx` — `Delete` and `Backspace` on the cursor row **ask** and never delete (45.3's keystroke rule).
- `properties-panel.tsx` — the tag row generalised; `tagsOf` and `serialiseTags` are the two value decisions.
- `use-notes-actions.ts` — `deleteNote` now takes a note id, and **`NotesActions.remove` is deleted**: an unconfirmed bound verb one keystroke from any surface is a second path around the confirmation.

---

## I/O and edge-case matrix

### `serialiseTags(tags, entry)` and `tagsOf(entry)`

| `tags:` on disk | read as | add `work` | remove the only tag |
|---|---|---|---|
| `tags:\n  - a\n  - b` (block) | `[a, b]` | block, appended | block, one line gone |
| `tags: [a, b]` (flow) | `[a, b]` | flow | `tags: [a]` |
| `tags: standup` (scalar) | `[standup]` | `tags: [standup, work]` — flow, same line | `tags: ""` |
| `tags:` (empty) | `[]` | `tags: [work]` | n/a |
| `tags:\n  work: true` (nested map) | not the tag row | new `tags:` key beside it, map untouched | n/a |
| no `tags:` key | `[]` | `tags:\n  - work` appended before the closing fence | n/a |
| no frontmatter at all | `[]` | `---\ntags:\n  - work\n---\n` prepended | n/a |
| `tags:` twice | first wins | first only | first only |

A scalar never has to hold exactly one tag after an edit — it goes 1→0 or 1→2 — so there is no "still one" arm. The sweep proved it (below).

### `NoteDeletePlanVm`

| input | question | consequence | recovery |
|---|---|---|---|
| note | names the title | names the path, links stop resolving | trash + history |
| space, `default_key: None` | names the space | notes it lists stay where they are | trash + history |
| space, `default_key: Some(_)` | names the space | …**plus** "will not add it back on its own; Restore default spaces brings it back" | trash + history |

Driven by `default_key`, never by the name: a renamed Recordings space is still keeper's, and a user's own space called Recordings is not.

### `record_deleted`

| ledger state | note | outcome | ledger after |
|---|---|---|---|
| has the key | a seeded default | `AlreadyRecorded` | byte-identical |
| absent / lacks the key | a seeded default | `Recorded` | key added |
| any | a space the user wrote | `NotADefault` | byte-identical |
| there and unparseable | any | `Blocked`, WARN, names the file | byte-identical |

### The confirmation, as a surface

| case | behaviour |
|---|---|
| plan in flight | "Reading what this would remove…", no Delete button |
| plan failed | Rust's sentence, no Delete button, Cancel only |
| declined | dialog closes; **no delete command called** |
| confirmed | `notes_delete`, then the panel showing that note closes |
| delete refused | dialog stays, `role="alert"` with Rust's sentence, row still listed |

---

## Verification

**Acceptance commands, run by name.**

- `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core --lib notes::` — **EXIT=0, 511 passed, 0 failed.**
- `bun run test src/components/notes/` — see the note below; my five files are **EXIT=0, 117/117, three consecutive repeats, zero unhandled errors, zero `export is defined`**.

**On the second command:** the literal scope is `src/components/notes/`, which at the time of writing contains four suites belonging to sweeps still in flight (`note-file-links.test.tsx`, `live-preview-marks.test.ts`, and two under W3Chrome's and W3NoteFile's windows). Running it produces reds I did not cause and cannot attribute stably, because the tree moves between repeats — W2Media's rule: *four runs over three different trees is four measurements of nothing.* So the number I stand behind is the five files this story owns or edits, three repeats, and I say so rather than quoting a bigger number that decays.

**Files whose suites this story changed:** `note-actions.test.tsx` (new, 6), `note-list.test.tsx` (+2, 17), `space-list.test.tsx` (+5, 24), `properties-panel.test.tsx` (+9, 42), `notes-pane.test.tsx` (+3, 28 incl. siblings').

---

## Mutation table

Harness `~/.W3TagsDelete/`, never `/tmp`. Sentinel `MUTTD<NN>`, unique in both directions; each restore verified by anchor count in both directions before and after. **26 mutations, 26 caught, 2 survivors found and closed.**

| # | Where | Mutation | Result |
|---|---|---|---|
| TD01 | `record_deleted` | `if !keys.insert` → `if keys.insert` | caught |
| TD02 | `record_deleted` | unreadable ledger → empty set instead of `Blocked` | caught |
| TD03 | `record_deleted` | write failure → `Recorded` | caught |
| TD04 | `for_space` | `default_key.is_some()` → `false` | caught |
| TD05 | `for_space` | drop "stays where it is" | caught |
| TD06 | `DeleteRecord::report` | `Recorded` stops naming the key | caught |
| TD07 | dialog | plan asked with `""` instead of the note id | caught |
| TD08 | dialog | delete called with the wrong id | caught |
| TD09 | `deleteNote` | drop `closeTarget` | caught |
| TD10 | dialog | drop `preventDefault` on the action | caught |
| TD11 | `tagsOf` | scalar reads as `[]` | caught |
| TD12 | `serialiseTags` | `tags.length > 1` → `> 0` | **SURVIVED** → closed by deleting the branch |
| TD13 | tag-row lookup | drop the `!entry.nested` guard | **SURVIVED** → closed by a new assertion |
| TD14 | `TagCombobox` | `chosen={items}` → `chosen={[]}` | caught |
| TD15 | `tagsOf` | `entry.items` → `.slice(0, 1)` | caught |
| TD16 | note list | `Delete \|\| Backspace` → `Delete` only | caught |
| TD17 | notes pane | delete the first row, not the cursor row | caught |
| TD18 | space list | always clear the scope | caught |
| TD19 | space list | always delete the first space | caught |
| TD20 | tag row | a new `tags:` key written flow instead of block | caught |
| TD21 | `serialiseTags` | list style forced to block | caught |
| TD22 | `serialiseTags` | drop the empty-scalar arm | caught |
| TD23 | `serialiseTags` | flow → block for a promoted scalar | caught |
| TD24 | `PropertyControl` | change the `aria-label={entry.key}` convention the tag row's absence assertion rests on | caught, **after** the witness below was added |
| TD25 | dialog | give the description its own `id`, so the dialog's `aria-describedby` dangles | caught, **after** the relationship assertion below was added |

### The two survivors

**TD12 was not a missing test — it was a branch no input can reach.** `serialiseTags` distinguished "one tag" from "two or more" on a scalar entry, and a scalar can only go 1→0 or 1→2: nothing rewrites a one-tag scalar into a different one-tag scalar. The boundary was unobservable, and so was the `tags[0] ?? ""` beside it. **Both deleted rather than tested** — Main's shape 4, and the fix is to remove the case, not to invent an input for it. TD22 and TD23 then probe the simpler form and both die.

**TD13 is the more serious one, and it is a fixture that could not distinguish the right answer from the mutant.** Dropping the `!entry.nested` guard makes a note whose `tags:` is an indented map render its map as the tag row. My test asserted one chooser and no chips — and **the mutant produces exactly that**, because a map's value is not a list so it renders no chips either. Only the *write* tells them apart, and the difference is a user's nested map surviving or being spliced over by a flat list. Closed with an assertion on the written frontmatter.

---

## Shape audit

Run after the sweep was green. Seven shapes, five from peers.

1. **What composes the input?** — `NoteDeletePlanVm` is built in Rust and asserted there directly (`for_note`, `for_space`, the marker-vs-name partition), not only through a hand-built VM in a component test. `record_deleted` is driven through the real `SeedVault` port against a real directory, not through hand-placed inputs — 44.3's own lesson, in its own file.
2. **Did anything press the button?** — every delete test presses Delete in the dialog and asserts the command; the decline tests assert `notesDelete` was **not** called, not merely that the dialog closed. A dialog that closed while a delete was in flight looks identical and is the opposite outcome.
3. **A contract stated in a doc comment and enforced nowhere** — `for_space`'s clause is partitioned on `default_key`, the field that DRIVES behaviour, and the test uses one path for both arms so the two sentences differ by exactly that clause. My first version used two paths and the comparison was vacuous; it failed and I fixed the test, not the assertion.
4. **A fallback for a case that cannot happen** — found two, both in `serialiseTags`, both deleted (TD12). Re-probed after removal: TD22/TD23.
5. **Assert a fixture is what it claims** — `deleting_a_default_the_ledger_never_recorded_still_tombstones_it` asserts the *premise* (`recorded(&vault).is_empty()`) before asserting what deletion does about it. Without that line the test would pass on a vault that had a perfectly good ledger.
6. **A branch reachable only from a second host** — deletion has **three** doors and all three are tested where they live: the editor menu (2 tests, through a real `NoteEditor`), a space row (5), and the list's `Delete` key (3 in the pane + 2 in the list). Door counts: 2 / 5 / 5. No zero.
7. **Assert what you handed on, not only what came back** — `chosen={items}` is asserted through the real `TagCombobox` by typing a tag the note already has and reading its "already on this list" line; `chosen={[]}` renders identically and dies on it (TD14). Every collection fixture has **two** items: two spaces in the space-list tests, two rows with the cursor on the *second* in both list-delete tests, two tags in the flow-list test. `slice(0, 1)` dies (TD15), so does "delete the first row" (TD17, TD19).
8. **An absence with no witness in the same representation** (W3Recording, sharpened by W3Chrome). **FOUND ONE, and it was the story's headline claim.** `reads an inline tag as a tag` asserted `queryByRole("textbox", { name: "tags" })` is absent — "a chip, not a text box", which is the whole point of generalising the scalar. Nothing anywhere asserted the positive form of that convention, so changing `PropertyControl`'s `aria-label={entry.key}` would have made the absence pass while testing nothing. Paired with a witness in the **same fixture and the same representation** — `title` is an ordinary scalar in `SCALAR` and DOES get a text box named after its key — and then probed: TD24 changes the convention and now dies. It survived every earlier probe because a mutation of a line I had thought about cannot point at an assertion that was never load-bearing.
9. **Does this thing NAME something, and does anything check the thing it names exists?** (W3Recording, from W3Chrome's dangling `aria-controls`.) **FOUND ONE, in the safety-critical part of the surface.** The confirmation's consequence and recovery reach a screen reader **only** through the dialog's `aria-describedby`. Every assertion I had found that text by `data-testid`, which passes exactly as well when the relationship is broken and the two sentences are announced to nobody — and those two sentences are the entire reason the button is safe to press. **A reference and a dangling reference are the same bytes**, so no render assertion can tell them apart. The test now resolves the attribute to an element and reads *its* text; TD25 gives the description its own `id` and dies on it.
10. **A new feature can break an old contract without touching the line that states it** (W3CaptureWindow). `note-list.tsx` states the rule *"binding a familiar key to an unfamiliar verb is worse than leaving it silent"*, and I bound bare `Delete`/`Backspace` to a destructive verb without editing that sentence. Checked what the familiar verb actually is instead of assuming it was mine: `conversation-pane.tsx:1441` binds bare `⌫`/`Delete` on the selected message to a redaction **dialog**, and `files-pane.tsx:1153` binds them to a delete **confirmation**. Three surfaces, one verb, none of them removing anything on the keystroke — so the contract is kept for the reason the paragraph gives. The enumeration and the evidence are now in that doc comment. **Note that TD16 (`Delete || Backspace` → `Delete` only) was CAUGHT and says nothing about this**: a mutation proves a line is load-bearing; it cannot tell you the line should not be there.
11. **What did the deleted thing promise?** (W3Capture.) I removed `NotesActions.remove`, and `use-notes-actions.ts`'s header still claimed the row verbs are all reachable through that binding. Narrowed, with delete's deliberate absence and its reason stated where the claim was. No test could find this: it is prose about a thing that no longer exists.
12. **A set is a global fact; grep the whole tree for every value you added** (W3Chrome). I widened no set and changed no shared value — every constant this story adds is new. The one wording that coincides is `NOTE_DELETE_CONFIRM = "Delete"` against `FILES_DELETE_LABEL = "Delete"`, and that is the one-affordance-one-wording rule being obeyed rather than a collision.

**Two more, from peers, run and clean:**

- *A doc comment that names another module's behaviour is an assertion nobody runs* (W3Chrome). Mine claimed the recovery sentence matches `FilesDeletePlanVm`'s. **It does not** — files says "recorded in this folder's history", notes has no folder. I read the actual string, worded mine deliberately, and the comment now says the divergence is deliberate rather than claiming a match.
- *Who writes the field on the boring path?* (W3NoteFile). `entry.title` drives both plan questions. It comes from the index for every note, not from a rare event.

---

## Deliberately NOT done

- **No count of the notes a space lists.** The confirmation states the invariant — every note stays — which is stronger than a number and needs no query run inside a dialog.
- **No backlink count in a note's confirmation.** Links resolve by ULID, so the rule is unconditional; a count that is right only while nothing else is writing is worse.
- **No undo.** `trash_note` puts the bytes in `.keeper/trash/<ulid>/` and the commit cadence carries the removal; recovery is the trash and the history, as it is for the Files pane.
- **No multiselect delete for notes.** The Files pane has a selection model; the notes list has a roving cursor and no selection. Inventing a second selection model is a different story.
- **No fix to 44.3's name-collision ledger hole.** Reported above with the one-line change that closes it.
- **Tags are not pushed back to a recording's `manifest.json`.** W3Recording flagged the divergence (45.19 edits the manifest, this edits the note) and is documenting it; unifying them is neither story's.

---

## What I could not verify here, and why

- **`keeper/src/notes_ipc.rs` and `keeper/src/lib.rs` have never been compiled.** The shell crate does not build on Linux (AD-55/AD-56). That covers `notes_delete_plan`, the extension to `notes_delete`, and the handler registration. Everything they *decide* is in `keeper-core` and is green; what is unproven is that they compile and that the two of them are wired.
- **No note has ever been trashed.** The acceptance asks for "deleting a note trashes it rather than unlinking, asserted on the trash path". `trash_note` lives in the shell crate and there is no Linux-testable seam to it. What is asserted here is that the frontend calls `notes_delete` with the right ids, that `notes_delete` is the only removal path in the frontend, and that the recovery sentence names the trash. **The trash path itself is unasserted on this host.**
- **"Declining changes nothing on disk, asserted byte-for-byte"** is met in Rust for the *ledger* (`deleting_a_space_keeper_did_not_seed_leaves_the_ledger_byte_identical`, and the unreadable-ledger test compares the planted bytes) and in TS by asserting no delete command is called. **No test on this box has watched a file not change on disk**, because nothing here can write one through IPC.
- **The seed→delete→reseed cycle is proved against a real directory** (`deleting_a_seeded_default_leaves_it_deleted_across_a_reseed`, `deleting_a_default_the_ledger_never_recorded_still_tombstones_it`) — but through `record_deleted` called the way the shell calls it, not through the shell.

### First checks on the macOS gate

1. `cargo check -p keeper` — the two commands and the registration have never seen a compiler.
2. Delete a note. Confirm the file is in `<vault>/.keeper/trash/<ulid>/` and **not** merely gone. That is NFR-30 and no test here has seen it.
3. Delete the seeded **Recordings** space, quit, relaunch. It must not come back. Then press **Restore default spaces** and it must.
4. Open a note with `tags: standup` written inline. The row must show a chip, not a text box.
