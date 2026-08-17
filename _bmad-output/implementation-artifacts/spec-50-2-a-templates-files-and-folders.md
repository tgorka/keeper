# Spec 50.2 — A template's files and folders

story: 50.2
status: in-progress
branch: `feat/50-2-a-templates-files-and-folders` (on top of 50.1)
binds: FR-284; AD-65 (Rust composes every path), the journal/plan contract
sentinel: `MUT50-2`

<intent-contract>

**The ask, verbatim.** *"w session templates nie widze mozliwosci dodawania/usuwania/zmiany nazwy
plikow/folderow"*

**Problem.** Story 49.1 made the Templates room a place you can stand: it lists templates, lists the
files inside one, opens them for editing, creates a named template and renames one. Every write verb
it has acts on the template **directory**; none acts on its **contents**. There is no recorded refusal
to relax — only a gap.

The session file verbs cannot be pointed at a template: `sessions_file_new`, `sessions_file_new_kind`
and `sessions_file_delete` all resolve their directory through
`sessions_root::row_of(root_id, session_id)`, a lookup over the scan's rows, and `_template/` is never
scanned as a session (FR-225). The refusal is `session_error` — "no such session" — an id-lookup wall.

The plan vocabulary already covers almost all of it: `MkDir`, `WriteFile`, `MoveDir`, `TrashDir`,
`TrashFile`, `EmptyDirKeep` (`plan.rs:29-61`). **The one missing primitive is `MoveFile`** — nothing in
this crate renames a file.

**Approach.** Four template-scoped commands, addressed by `(root_id, template_name, rel)` rather than
by a session id, each compiling to a journaled plan the existing executor runs.

**Always.**
- Every path is composed in Rust (AD-65) and validated against the template root: no `..`, no
  absolute, no escape, and never `workspace/`'s scratch semantics — a template's `workspace/` is a
  skeleton directory and may be created and trashed like any other.
- Every write goes through `sessions_exec::run` with a journal row, so a template edit appears in the
  zone's history exactly as a create does.
- A delete goes to `.keeper/trash/<id>/` and keeps its basename, per `TrashFile`/`TrashDir`'s own
  contract — nothing here unlinks bytes.
- A rename refuses a collision rather than overwriting. `docs/sessions.md`'s refusal to rename session
  *files* is about link identity — a path IS the id, so a rename breaks the pins pointing at it — and
  it has **no teeth inside a template**: nothing points at a template's files, and a create copies
  them rather than referencing them. That judgement is recorded here because it is a relaxation.
- The room lists a template as a **tree**, because a template's shape is folders.

**Block if.**
- The name slugs to nothing → the refusal `template_mint` already words. A file keeps its extension:
  slugging `Kick Off.md` must not produce `kick-off-md`.
- The destination exists → refused, naming it. Neither a file nor a folder is overwritten.
- The target is outside the template root after normalisation → refused before anything opens.
- A directory delete that would take the template root itself → refused; use the template rename/trash
  verbs story 49.1 owns.

**Never.**
- Never a recursive delete without the trash. `TrashDir` is recoverable; `remove_dir_all` is not, and
  a template is a thing somebody wrote.
- Never show or touch `.DS_Store` and friends: `pattern_files` skips every dotfile except `.gitkeep`
  (`sessions_ipc.rs:512-514`), and because that same walk is what a create copies, the stray never
  travels into a session. A delete verb given eyes for it would be a delete verb for a file the room
  otherwise pretends does not exist.
- Never a second directory walk. `sessions_template_entries` (story 49.1) is the room's reader; the
  tree is built from what it returns.

**I/O and edge-case matrix.** Every row is a test.

| # | input | expected |
|---|---|---|
| 1 | `MoveFile { from, to }` executed | the file is at `to`, absent at `from`, journal cleared |
| 2 | `MoveFile` whose `to` exists and is a different file | refused, nothing moved (the `MoveDir` precedent, incl. the same-file carve-out) |
| 3 | `sessions_template_file_new(root, Some("test1"), "notes.md")` | an empty markdown file at `_template/test1/notes.md`, journaled, room re-reads |
| 4 | the same with `"refs/inputs.md"` | parent created in the same plan, then the file |
| 5 | the same with `"../escape.md"` | refused before anything opens |
| 6 | `sessions_template_dir_new(root, None, "artifacts")` on a template that has it | idempotent — `MkDir` succeeds if it exists (`plan.rs:30-31`), no error, no duplicate journal noise |
| 7 | `sessions_template_rename_entry(root, Some("test1"), "about.md", "Record.md")` | `_template/test1/record.md` — slugged stem, extension preserved |
| 8 | rename onto an existing name | refused naming the collision |
| 9 | rename a directory (`refs` → `references`) | `MoveDir`, contents intact |
| 10 | `sessions_template_delete_entry(root, None, "README.md")` | in `.keeper/trash/<id>/README.md`, gone from the template |
| 11 | delete a directory with contents | `TrashDir`, recoverable whole |
| 12 | delete `""` or `"."` (the template root) | refused, naming the verb that does that instead |
| 13 | any of the four on an unknown root or unknown template | the existing `root_error` / "there is no template at …" sentences |
| 14 | the room after each verb | re-read through the story-49.1 nonce; the tree shows the change |
| 15 | `.DS_Store` present in `_template/` | invisible in the room before and after; no verb can name it |
| 16 | a nested file's row | its label is the template-relative path (`prompts/hand-off.md`), and the tree groups it under `prompts/` |

</intent-contract>

## Code Map

### Rust

| file | change |
|---|---|
| `keeper-core/src/sessions/plan.rs:29-61` | `PlanStep::MoveFile { from, to }`, documented as `MoveDir`'s twin and why a rename is a move rather than a copy-delete |
| `keeper/src/sessions_exec.rs` | execute `MoveFile`, reusing `same_directory`'s identity logic (story 49.1's fix) so a case-only rename on APFS is not a self-collision. Tests: rows 1–2 |
| `keeper-core/src/sessions/template.rs` | `compile_file_new`, `compile_dir_new`, `compile_entry_rename`, `compile_entry_delete` — pure compilers beside `compile_install`/`compile_rename`, each returning a `Plan` with its own verb string. Tests: rows 3–12 at the plan level |
| `keeper/src/sessions_ipc.rs` | four commands beside the story-49.1 three: `sessions_template_file_new`, `sessions_template_dir_new`, `sessions_template_rename_entry`, `sessions_template_delete_entry`, each `(root_id, name: Option<String>, …)` and each reusing `template_at` for the template segment and the same normalise-and-refuse guard for `rel`. Non-desktop stubs. Register all four in `lib.rs` |
| `keeper-core/src/sessions/naming` or `files.rs` | the "slug the stem, keep the extension" helper if none exists — check `files::new_stamped` (`files.rs:245-254`) first and reuse |

### TypeScript

| file | change |
|---|---|
| `src/lib/ipc/client.ts` | four wrappers beside `sessionsTemplateEntries`/`sessionsTemplateRename`, doc comments naming each refusal |
| `src/components/sessions/session-templates.tsx` | the flat entry list becomes a tree grouped by the template-relative path `sessions_template_entries` already returns; per-entry rename and delete; per-template *New file* and *New folder*. Reuse the repo's tree idiom — `session-tree.tsx` is the sessions one; say in the review which was reused and why |
| `src/components/sessions/session-templates.test.tsx` | rows 3–16 on the TS side: the calls, the refusal sentences in the live region, the tree grouping, and that `.DS_Store` never appears |
| `docs/sessions.md` | the four verbs, the trash promise, and the recorded relaxation about renaming inside a template |

## Tasks & Acceptance

- [ ] `PlanStep::MoveFile` + executor + rows 1–2
- [ ] four pure compilers in `template.rs` with plan-level tests (rows 3–12)
- [ ] four commands, guarded, journaled, registered, with non-desktop stubs
- [ ] client wrappers
- [ ] the room becomes a tree with create/rename/delete per entry (rows 14–16)
- [ ] `docs/sessions.md`, including the rename relaxation and its reason

**Acceptance.** In the Templates room a person can add a file or a folder to a template, rename either,
and delete either into a recoverable trash — with the zone's history recording each one, and with the
stray `.DS_Store` still invisible.

## Design Notes

_(filled at review)_

## Verification

_(filled at review)_
