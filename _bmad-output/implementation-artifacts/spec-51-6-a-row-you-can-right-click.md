# Spec 51.6 — A row you can right-click, and a title that renames its file

story: 51.6
status: in-progress
branch: `feat/51-6-a-row-you-can-right-click` (on 51.5)
binds: FR-295, FR-296, FR-297; AD-120, the recorded rename refusal in `docs/sessions.md`
sentinel: `MUT51-6`

<intent-contract>

**The ask.** *"nie widze opcje w pracym plawiszu w spaces (jak open in new tab i inne z notes)"* and
*"jak zmieniam title property - nie zmiania sie nazwa pliku"*.

**Problem, part one.** No `ContextMenu` is mounted anywhere in `src/components/sessions/`; a space row
is a bare `<Button>` with one gesture. **And this app has no tabs** — the multi-document model is
panels, so *open in a new tab* has no referent; the existing verb is *Open in a new panel*. The right
donor is the **Files pane's** row menu (`files-pane.tsx:1962-1985`), which is already keyed by
`(profileId, path)`; the notes menu is the wrong donor because five of its seven items are vault-only
facts.

**Problem, part two.** Nothing anywhere renames a file when `title` changes — not for a session file
(the properties panel's only write is a frontmatter byte-splice) and **not for a vault note either**,
because `notesRename` is shipped with **zero call sites in `src/`**. So FR-97 has been unreachable, and
the owner's report is the first time anyone noticed.

**The recorded refusal, weighed.** `docs/sessions.md` refuses to rename session files: *"a rename is a
link-rewriting problem, not a file-system one, and half of it would be worse than none"*, with the
stated reason that a file identified by its path loses its pins. **The pins half has no teeth** — a
session pool entry carries no pin (`pool.rs:146-149` says so in terms), and pinned/unread live in the
session's record, per session. **The link-rewriting half has teeth**, and this story does that half
rather than skipping it.

**Approach.** One row menu, reused. And a rename that moves the file and rewrites what points at it.

**Always.**
- The rename is `MoveFile` (story 50.2's primitive) plus a rewrite pass over the pointers that name the
  old path, in one journaled plan. Either both land or neither does.
- Every pointer class is enumerated in the spec's table below, and each is either rewritten or
  explicitly out of scope with a reason. No pointer class is left undecided.
- The title write and the rename are one verb from the person's point of view: they changed the title.
- A rename that would collide refuses, and says which file it would have overwritten.
- `notesRename` becomes reachable from the note properties panel, using the command that already
  exists and already rewrites links.

**Block if.**
- The new title slugs to nothing → refused with the existing sentence; the title change is not applied
  either, because a half-applied rename is the "half of it would be worse than none" the doc warns of.
- The file is the session's record (`about.md` / `README.md`) → the title changes, the file does not.
  `shape()` keys on those names.
- The file is in `workspace/` → refused; the fence owns it.

**Never.**
- Never rename without rewriting the pointers this table says are rewritten.
- Never rewrite a pointer inside `workspace/` or `artifacts/`: one is fenced, the other is output.
- Never invent a "tab": the menu says *panel*, because that is what the app has.

**Pointer inventory — what a session-file rename must do.**

| pointer | rewritten? | why |
|---|---|---|
| `refs.rs` reference targets in the session's markdown | **yes** | this is the link-rewriting half with teeth |
| wikilinks `[[…]]` in the session's markdown | **yes** | same |
| the session record's own frontmatter (`title`) | n/a | that is the edit that triggered this |
| session-level pins / unread / `head_rev` | no — unaffected | they live on the session, not the file |
| the promote table | **yes if it names the file** | it is a list of paths in the record |
| `keeper.session.continues` lineage | no | it names sessions, not files |
| the recordings lens | no | it keys on a `session:` frontmatter value |
| `.keeper/` cache | no | it rebuilds from disk |

**I/O and edge-case matrix.**

| # | input | expected |
|---|---|---|
| 1 | title `untitled` → `Kick Off` on `2026-08-16-1812-untitled.md` | the file becomes `2026-08-16-1812-kick-off.md`, the stamp preserved |
| 2 | a wikilink `[[2026-08-16-1812-untitled]]` in a sibling | rewritten in the same plan |
| 3 | a `refs.rs` reference naming the old path | rewritten |
| 4 | a title that slugs to nothing | refused; the title is not written either |
| 5 | a rename onto an existing name | refused, naming the collision |
| 6 | the session record | the title changes, the filename does not |
| 7 | a `workspace/` file | refused with the fence's sentence |
| 8 | a rename mid-flight interrupted (journal replay) | either state is consistent; no half-rename |
| 9 | right-click a space row | a menu: Open, Open in a new panel, Reveal, Copy path, Rename, Delete |
| 10 | the menu's Rename | the same verb as the properties title, one implementation |
| 11 | keyboard: the menu opens from the row and closes on Escape | reachable without a pointer |
| 12 | a vault note's title changed in the properties panel | `notesRename` runs and the note's file follows |

</intent-contract>

## Code Map

| file | change |
|---|---|
| `keeper-core/src/sessions/files.rs` | `compile_rename(session, rel, new_title, pointers) -> Plan`: `MoveFile` plus `GuardedWrite`s for the rewritten pointers, one verb, one journal row |
| `keeper-core/src/sessions/refs.rs` | the pointer scan gains a rewrite that returns the new bytes for a file whose links named the old path |
| `keeper/src/sessions_ipc.rs` | `sessions_file_rename(root_id, session_id, rel, new_title)`; the properties write on a session file routes through it when `title` changed |
| `src/components/notes/properties-panel.tsx` | a title change asks the surface's rename verb: `sessions_file_rename` for a session file, `notesRename` for a note (FR-97, finally reachable) |
| `src/components/sessions/session-spaces.tsx` | the Files-pane row menu, reused; wording says *panel* |
| `docs/sessions.md` | the rename, what it rewrites, and what it deliberately does not |

## Tasks & Acceptance

- [ ] `compile_rename` + the pointer rewrite, matrix rows 1–8
- [ ] the row menu, rows 9–11
- [ ] `notesRename` wired, row 12
- [ ] `docs/sessions.md` records the relaxation and the pointer inventory

**Acceptance.** The owner renames a file by changing its title, the links that named it follow, and a
right-click on a space row offers the verbs a file row offers in Files — in the app's own vocabulary.

## Design Notes

_(filled at review)_

## Verification

_(filled at review)_
