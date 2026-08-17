# Spec 53.5 — A create that lands where the space says

story: 53.5
status: in-progress
branch: `work/epic-53-create-lands-in-place` (on top of `work/epic-53-narrow-a-space`)
baseline_revision: 8c8a3eb
final_revision: ''
binds: FR-320, FR-321, FR-322; AD-120, AD-121
sentinel: `MUT53-5`

<intent-contract>

**The ask, verbatim.** *"the space notes in sessions are created in main sessions
folder instead of the spaces subfolder, template also not creating the space
subfolder for it"*

**Why 52.5 did nothing for him.** It shipped the mechanism and no default:
`SessionSpace.create_dir` defaults empty, and `kind_dir` specifies empty as "the
contract's answer" — the session root under Flat (`shape.rs:338-344`). No default
space ships a destination, the seeder never writes the key, and he never typed
one. He was handed a switch nobody set.

**Two measurements make a default safe.** A flat session's pool scan DOES descend
subdirectories (`read_ref_sources` → `markdown_rels(dir, true)`,
`sessions_root.rs:1272`), so a file written to `tasks/` is read back and still
matched by `tag:task` — the destination cannot hide his files. And `shape()` keys
on `AGENTS.md` alone (`shape.rs:98-101`), so new subdirectories cannot flip a flat
session to folder-shaped.

**The distinction that reaches an existing zone.** His `_spaces/*.md` carry no
`keeper.create_dir` key at all. So **absent must mean "ask the default", and only
an explicit empty value means "the session root"**. Without that, a default is
invisible to every zone that already exists — the same shadowing that makes 53.4 a
repair rather than a const edit.

**Always**
- Each default space names where its creates land, per kind, under Flat only: the
  contract still owns `refs/` and `prompts/` under Folder, and About and the
  residue space name nothing.
- An absent `keeper.create_dir` inherits the claimed default's destination. An
  explicit empty value is a deliberate "the session root" and is honoured as such.
- Reading still derives kind from the tag. A file in `tasks/` tagged `[ref]` is a
  ref. AD-120 is untouched.
- The directory is created if absent, through the same journaled `MkDir` the folder
  verb uses — one plan, one journal row.
- A template's `_spaces/*.md` carries its `keeper.create_dir` through the copy
  verbatim, and a template's own directories are created on a session create.
- Seeding stays hole-fill: a zone that already holds a space keeps its file. What
  changes for that zone is the absent-key inheritance, not a rewrite.

**Block if**
- The destination escapes the session, names `workspace/`, or is a dotted
  directory: the three refusals 52.5 already wrote, each with its own sentence.
- The kind has no home under the session's shape: a destination cannot make a
  homeless kind creatable (`shape.rs:321-323`).

**Never**
- Never infer a kind from a directory on read.
- Never rewrite a persisted space file to install a default (AD-121). Inheritance
  on read is the mechanism; a write needs a press.
- Never honour a destination under Folder for a kind the contract already files.

</intent-contract>

## Code Map

| where | change |
|---|---|
| `keeper-core/src/sessions/spaces.rs` | `DefaultSessionSpace` gains a destination; `read_one` distinguishes absent from empty and inherits from the claimed default |
| `keeper-core/src/sessions/shape.rs:306-345` | unchanged contract; the caller now passes an inherited value where there was none |
| `keeper-core/src/sessions/template.rs`, `pattern.rs` | a seeded/copied space keeps its destination; the template's directories are created |
| `keeper/src/sessions_ipc.rs` | the create verb passes the inherited destination; `sessions_space_save` still validates an explicit one |
| `src/components/sessions/session-space-editor.tsx` | the field shows the inherited default as its placeholder, so "empty" reads as what it now means |
| `docs/sessions.md` | the absent-vs-empty rule, in the operator's words |

## Tasks & Acceptance

| # | acceptance |
|---|---|
| 1 | a flat session's task create from the Task space lands in `tasks/<stamp>-<slug>.md`, and the directory is made if absent |
| 2 | the created file carries its kind tag and the Task space lists it — proving the scan reads it back |
| 3 | a space file with NO `keeper.create_dir` inherits its default's destination |
| 4 | a space file with an EXPLICIT empty value writes to the session root, and says so in the editor |
| 5 | a folder-shaped session still files refs and prompts by contract, and a destination does not override that |
| 6 | About and the residue space name no destination, and a create is still refused for them |
| 7 | reading still derives kind from the tag: a file in `tasks/` tagged `[ref]` is a ref |
| 8 | a template's space definition carries its destination through a copy, and the template's directories exist after a create |
| 9 | no persisted `_spaces/*.md` is rewritten by any of this |

## Design Notes

_(filled at review)_

## Verification

_(filled at review)_
