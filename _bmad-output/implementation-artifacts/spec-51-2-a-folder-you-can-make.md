# Spec 51.2 — A folder you can make, and the two every session gets

story: 51.2
status: in-progress
branch: `feat/51-2-a-folder-you-can-make` (on 51.1)
binds: FR-287, FR-288; AD-113 (`workspace/` is fenced), AD-119
sentinel: `MUT51-2`

<intent-contract>

**The ask.** *"nie moge stworzyc folderu w sessions"* and *"dodaj domyslny katalog artifacts i workspace
do kazdego template/session"*.

**Problem.** Item 3 is absent AND unreachable: `files::compile_new` already emits `MkDir` for a nested
parent (`files.rs:334-339`), but nothing can send a nested path — the create dialog's Folder field is a
`<select>` over folders already in the tree, and the tree's row verbs are open / open-with / reveal /
delete. Item 9: `zone_skeleton` writes **two files and zero directories** (`template.rs:300-307`,
asserted at `:1039-1042`), and a create forces `artifacts/`+`workspace/` only when the pattern is a
SESSION (`pattern.rs:298-302`). The owner's own `_template/` has them by luck.

**Approach.** A session-scoped folder verb that reuses the session module's own guard, and a skeleton
that matches the contract the docs already state.

**Always.**
- The verb uses `files::check_dir` (`files.rs:167`), which already refuses `workspace/`, traversal,
  absolute paths and dotted folders — and it *also* asks `WriteScope`, because two predicates that
  must agree should both run.
- `MkDir` is idempotent by contract (`plan.rs:31`), so asking for a folder that exists changes nothing.
- Journaled like every other zone write, and the tree re-reads.
- A session folder name folds the way a template's does — `Interview Kit` → `interview-kit` — and that
  choice is **stated**, because templates fold and sessions had no precedent.

**Block if.**
- The target is `workspace/` or inside it → refused. Scratch is fenced (AD-113) and a folder there
  would invite writes the engine refuses.
- Traversal, absolute, or a dotted segment → refused before anything is opened.

**Never.**
- Never point `template::compile_dir_new` at a session. Its guards have no `workspace/` refusal —
  `template.rs:419-428` records the inverse rule, and aimed at a live session it would compile
  `MkDir active/s/workspace/whatever` straight through the fence.
- Never add `.gitkeep` to a skeleton directory: `MkDir` steps do not need it (`is_placeholder` exists
  for FILE-list copies).

**Contract friction, stated rather than smuggled.** The `AGENTS.md` keeper writes into every session
says *"Do not create other directories. A new kind of thing is a new tag, not a new folder — that is
the whole point of this layout."* (`template.rs:125-126`, pinned by a test). A New-folder button
contradicts that sentence on a flat session. This story amends the sentence to permit non-markdown
containers and directories the operator makes deliberately, because `check_dir` plus the write fence
already keep the dangerous case out, and 51.1 makes markdown in a subdirectory legible.

**I/O and edge-case matrix.**

| # | input | expected |
|---|---|---|
| 1 | `sessions_dir_new(root, session, "log")` | `MkDir <session>/log`, journaled, tree re-reads |
| 2 | the same twice | idempotent, no error, no duplicate journal noise |
| 3 | `"Interview Kit"` | `interview-kit` — folded, and the folding is asserted |
| 4 | `"workspace"` or `"workspace/x"` | refused with the fence's own sentence |
| 5 | `"../escape"`, `"/abs"`, `".hidden"` | refused before anything opens |
| 6 | `"a/b/c"` | parents created in one plan |
| 7 | unknown root or session | the existing `root_error` / `session_error` sentences |
| 8 | `zone_skeleton` | now names `artifacts/` and `workspace/` as well as its two files |
| 9 | *Write keeper's template into this zone* | the installed template has both directories |
| 10 | a session created from a hand-made template lacking them | still gets both |
| 11 | a session created from a session | unchanged — it already got both |
| 12 | the tree after a create | the new folder is a row, and a file can be created into it |

</intent-contract>

## Code Map

| file | change |
|---|---|
| `keeper-core/src/sessions/files.rs` | `compile_dir_new(session, rel) -> Result<Plan, FileVerbError>`: `check_dir` then one `MkDir`, verb `"dir-new"`; the name folds through the same slug rule templates use, stated in the doc |
| `keeper/src/sessions_ipc.rs` | `sessions_dir_new(state, root_id, session_id, rel)`, asking `WriteScope` as `resolve_session_file` does; non-desktop stub; registered in `lib.rs` |
| `keeper-core/src/sessions/template.rs:300-307,356-364` | `zone_skeleton` names the two directories; `compile_install` emits their `MkDir`s; the `:1039-1042` assertion updated |
| `keeper-core/src/sessions/pattern.rs:298-302` | drop the `PatternKind::Session` guard so a template create forces them too — a template create is no longer purely verbatim, and the doc says so |
| `src/components/sessions/session-file-actions.tsx` | a fourth verb, *New folder*, beside the three; always-visible like its siblings |
| `keeper-core/src/sessions/template.rs:120-130` (`AGENTS_MD`) | the amended sentence |
| `docs/sessions.md` | the verb, the fence, and the amended contract line |

## Tasks & Acceptance

- [x] `files::compile_dir_new` with the session module's guard, matrix rows 1–6
- [x] `sessions_dir_new` + registration + non-desktop stub
- [x] the skeleton and the create both guarantee `artifacts/` and `workspace/` (rows 8–11)
- [x] *New folder* in the session file actions (row 12)
- [x] `AGENTS.md`'s sentence amended, and its test updated to the new wording
- [x] `docs/sessions.md`

**Acceptance.** The owner can make a `log/` or `spaces/` folder inside a session and create files into
it; every template keeper writes has `artifacts/` and `workspace/`; and `workspace/` still refuses.

## Design Notes

_(filled at review)_

## Verification

_(filled at review)_
