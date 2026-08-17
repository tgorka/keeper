# Spec 50.1 — A space you can write into, wherever the session keeps it

story: 50.1
status: in-progress
branch: `feat/50-1-write-where-the-session-keeps-it` (on top of the epic-49 stack)
binds: FR-277, FR-278, FR-279, FR-280, FR-281; AD-120 (kind is a tag, never a folder), AD-65
sentinel: `MUT50-1`

<intent-contract>

**The ask, verbatim.** *"w session spaces nie widze przycisku nowych notes (tylko ilosc) - jestes pewien
ze kazdy space przyjmuje wiecej niz 2 notes?"* and *"nie widze tez otwierania w okienku notes"*.

**Problem.** Three separate defects produce one experience.

1. Story 49.2 gated the create control on `shape === "flat"` (`session-spaces.tsx:599`). The owner's
   sessions are **folder-shaped**, so every space suppresses it. The gate's reason is true —
   `sessions_file_new_kind` writes into the session root (`sessions_ipc.rs:2478-2480`) and a
   folder-shaped pool reads `README.md` + `refs/` + `prompts/` only
   (`sessions_root.rs:1068-1089`) — but it treats a symptom. The fix is to write where the pool
   looks.
2. The control is hover-only. It is the house pattern for a row's edit/delete, and wrong for the one
   verb a section exists to offer; the session create verbs the owner already knows are
   always-visible labelled buttons (`session-file-actions.tsx`).
3. `Add reference` writes `references.md` into the session ROOT on a folder-shaped session, ungated
   (`sessions_ipc.rs:2866-2871`). That file is on the owner's disk and no space and no *Unfiled*
   notice can see it — a write into a blind spot.

And a fourth, which is why the second report exists: **story 49.2's note arm can never execute.**
`notePathForFile` resolves only when the vault contains the zone, and `SessionsConfig::validate`
refuses that containment in either direction (`profile/mod.rs:648-654`). The arm, three matrix rows
and a `stillWanted` race guard exist for a state the product forbids.

**Approach.** Make the create write where the shape keeps that kind, tag it so the space lists it,
show the control, fix the reference destination, and delete the arm that cannot fire.

**Always.**
- The kind→directory mapping is **public, in the domain, beside the inverse it already has**
  (`migrate::carried_kind`, `migrate.rs:277-284`). One source of truth for both directions.
- A create writes the directory **and** the tag. AD-120: `pool::read_one` derives kind from tags only
  (`pool.rs:253`), so a file in `refs/` without `tags: [ref]` is still unfiled.
- `files::check_dir` already accepts a subdirectory parent and refuses `workspace/` and traversal
  (`files.rs:172-190`) — reuse it; do not write a second guard.
- The create control renders like a session create verb: visible, labelled, not hover-revealed.
- Every refusal is a sentence where the person is looking.

**Block if.**
- The kind has no home in this session's shape → **no control**, and the space says so in one line
  rather than offering a button that would write into a blind spot. In the folder contract that is
  `task`; `log` is not a file at all (a folder-shaped session's log is a `## Log` heading in the
  README — `pool::log_view`, `pool.rs:397-410`), so the space defers to the existing
  `sessions_log_today` verb rather than growing a second writer.
- The space's query is not one creatable `tag:` term → unchanged from 49.2, no control.
- `workspace/` is never a create destination, at any shape.

**Never.**
- Never infer a kind from a directory when reading. The directory is where a create *puts* a file;
  the tag is what makes it that kind. Two readers of one fact is how `refs/` and `tag:ref` start to
  disagree.
- Never keep the note arm "in case". A branch no configuration reaches is a claim the code cannot
  keep; it goes, along with the spec sentence that promised it.
- Never widen `SessionsConfig::validate`. Its reason — two indexers claiming one tree — has teeth.

**I/O and edge-case matrix.** Every row is a test.

| # | input | expected |
|---|---|---|
| 1 | `kind_dir(Flat, Ref)` | `None` — a flat session keeps everything at the root |
| 2 | `kind_dir(Folder, Ref)` | `Some("refs")` |
| 3 | `kind_dir(Folder, Prompt)` | `Some("prompts")` |
| 4 | `kind_dir(Folder, Task)` | **refused** — the folder contract has no task home; the caller must not invent one |
| 5 | `kind_dir(Folder, Log)` | **refused** — a folder-shaped log is a README heading, not a file |
| 6 | `kind_dir(_, About)` | refused, as `sessions_file_new_kind` already refuses `about` |
| 7 | `sessions_file_new_kind(root, folder-shaped session, "ref", "Inputs")` | writes `refs/<stamped>.md` with `tags: [ref]`, returns that subpath, and the References space lists it on the next read |
| 8 | the same on a flat session | unchanged from today: a root-level stamped file with the tag |
| 9 | `sessions_file_new_kind(root, folder-shaped, "task", …)` | refused with a sentence naming the shape, nothing written |
| 10 | a folder-shaped session whose `refs/` does not exist | the directory is created first, in the same journaled plan |
| 11 | `sessions_ref_add` on a folder-shaped session | writes where the folder pool reads, not the root |
| 12 | Tasks on a folder-shaped session | no create control, and one line saying this shape keeps no tasks file |
| 13 | References on a folder-shaped session | a create control, visible without hovering |
| 14 | any space, flat session | the control is visible without hovering |
| 15 | a row in any space | opens the file target; **no** `notePathForFile`/`openNoteForFile` call remains in this surface |
| 16 | 49.2's rows 9, 11, 12 | deleted, with the reason recorded in this spec, not silently dropped |

</intent-contract>

## Code Map

### Rust

| file | change |
|---|---|
| `keeper-core/src/sessions/shape.rs` or `files.rs` | `pub fn kind_dir(shape: Shape, kind: KindTag) -> Result<Option<&'static str>, KindHasNoHome>` (name it in the module's own voice) — the public mapping, with `migrate::carried_kind` (`migrate.rs:277-284`) made its inverse or documented as such so the two cannot drift. Tests: matrix rows 1–6 |
| `keeper/src/sessions_ipc.rs:2437-2502` | `sessions_file_new_kind` becomes shape-aware: read the row's shape, ask `kind_dir`, build the destination through `files::check_dir` (`files.rs:172-190`), `MkDir` the parent in the same plan, keep `render_new`'s tag stamping (`files.rs:272-300`) unchanged. Refuse a homeless kind with a sentence naming the shape |
| `keeper/src/sessions_ipc.rs:2830-2877` | `sessions_ref_add`'s default destination asks the same mapping instead of always writing `references.md` at the root |
| `keeper-core/src/sessions/pool.rs` | **read side unchanged.** Confirm by test that a file written into `refs/` WITHOUT the tag is still unfiled — the AD-120 guard |

### TypeScript

| file | change |
|---|---|
| `src/components/sessions/session-spaces.tsx:599` | the `shape === "flat"` half of `creatable` goes; the control's condition becomes "Rust said this space can create" — which now means shape-aware. Add the one-line reason where a kind has no home in this shape |
| `src/components/sessions/session-spaces.tsx` (the control) | always visible: drop `opacity-0 group-hover:opacity-100` for the create only. Edit and Delete keep theirs |
| `src/components/sessions/session-spaces.tsx:339-425` | delete `openSpaceFile`'s note arm, `notePathForFile`/`openNoteForFile` imports, `SESSION_SPACE_VAULTS_UNKNOWN`, the `vaults` prop and its hydration, and the `stillWanted` plumbing that existed only for it. The opener becomes the file target it always was in practice |
| `src/components/sessions/session-detail.tsx` | drop the `vaults` wiring |
| `src/components/sessions/session-spaces.test.tsx` | delete rows 9, 11, 12; keep row 10 as the only opener case; add rows 12–15 |
| `_bmad-output/implementation-artifacts/spec-49-2-a-space-you-can-write-into.md` | a `## Superseded` note: the note arm and its acceptance sentence described a configuration the validator refuses; 50.1 removed them |
| `docs/sessions.md` | what a space's create writes, per shape; that a row opens the file; that a session file is not a vault note and why |

## Tasks & Acceptance

- [ ] the public kind→directory mapping, with the inverse documented, matrix rows 1–6
- [ ] `sessions_file_new_kind` shape-aware, `MkDir` in-plan, homeless kinds refused (rows 7–10)
- [ ] `sessions_ref_add` writes where the shape reads (row 11)
- [ ] a test proving an untagged file in `refs/` is still unfiled (AD-120)
- [ ] the flat gate removed, the control always visible, the homeless-kind line (rows 12–14)
- [ ] the note arm and its three rows deleted; 49.2's spec corrected (rows 15–16)
- [ ] `docs/sessions.md`

**Acceptance.** On the owner's folder-shaped session, References and Prompts each show a *New note*
button without hovering; pressing it writes a tagged file into `refs/`/`prompts/` and that file appears
in that space; Tasks says plainly that this shape keeps no tasks file; and no code path claims a note
that the product refuses to let exist.

## Design Notes

_(filled at review)_

## Verification

_(filled at review)_
