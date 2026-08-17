# Spec 53.5 — A create that lands where the space says

story: 53.5
status: review
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
| `keeper-core/src/sessions/spaces.rs` (review) | `read_create_dir` warns for a value that is not one path; `narrow_target` requires the claimed default's own term to be in the query |
| `keeper/src/sessions_root.rs` (review) | `SessionPool` carries the walk's truncation instead of discarding it; `read_session_pool` splits out so a test can hand it a folder |
| `keeper-core/src/sessions/vm.rs`, `keeper/src/sessions_ipc.rs` (review) | `SessionSpaceFilesVm::pool_truncated` projects that flag onto every space in the payload |
| `src/components/sessions/session-spaces.tsx` (review) | the section says a short list is short, and does not also claim the session is empty |

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

**Inheritance on read, traced end to end.** `_spaces/tasks.md` carrying
`keeper.default: tasks` and no `keeper.create_dir` → `spaces::destination` →
`shape::kind_dir(Flat, Task, "tasks")` → `MkDir tasks` + `WriteFile
tasks/<stamp>-<slug>.md`, read back by the session's scan and listed by
`tag:task`. The repair verb (Story 53.4) remains the only writer into
`_spaces/`; nothing in this story rewrites a persisted space file (AD-121).

**A destination keeper cannot read is still a destination.** `read_one`'s
`create_dir` arm matches on the KEY ALONE and flattens the value, because
matching on `FieldValue::Str` would make an unreadable value `None` — and `None`
inherits, so the operator's next file would land in a folder they never named.
Since review the arm also WARNS for a list or a map, the way `order`, `folded`
and `rows` have always warned for a value keeper could not read: this is the one
key of a space file that decides where a file lands, so silence here was the one
silence that costs a file. A scalar is never warned about (`create_dir: 2026` is
a folder called `2026`), so the sentence cannot become furniture.

**A narrowing keeps a term the query already has.** `narrow_target` requires the
claimed default's own `tag:` term to be present among the query's terms, folded
through `tags::normalise` the way the engine folds them. Without that check a
repurposed seeded space — `keeper.default: about` over a query edited to
`tag:log tag:task`, a state `render_edit` deliberately keeps reachable so
`claimed()` still counts the file — would offer to replace BOTH of the operator's
terms with an unrelated query and empty the space of everything it listed. A
`tag:about/*` subtree term is not the bare tag and does not authorise the press.

**The pool's bound is part of the safety argument, and now part of the payload.**
The story's claim is that a flat session's pool descends subdirectories; true of
the traversal, but its bound is `MARKDOWN_WALK_BUDGET = 2_000` dirents visited,
spent before any filtering, root entries first and then the alphabetically-first
subtree exhaustively. Per-kind destinations therefore moved a fresh create from
the first directory enumerated into one reached only after every earlier-sorting
subtree, so on a wide session the file a create just wrote is genuinely absent
from the space that wrote it. `sessions_root::session_pool` used to discard the
truncation flag `read_ref_sources` had already computed; it now carries it
(`SessionPool::truncated`), `sessions_space_files` projects it
(`SessionSpaceFilesVm::pool_truncated`), and the section says *too many files to
read them all* instead of *nothing in this session yet* — a claim about the
session that keeper is not entitled to make where it stopped reading it. The
caveat is stated for the operator in `docs/sessions.md` beside the key.

## Verification

**Gates run at the tip of the epic-53 stack.**

| command | result |
|---|---|
| `cargo test -p keeper-core sessions::spaces` | 70 passed, 0 failed |
| `cargo clippy -p keeper-core --all-targets -D warnings` | clean |
| `npx tsc --noEmit` | clean |
| `npx vitest run src/components/sessions/session-spaces.test.tsx` | 79 passed |

**Tests added at review, each proved to fail without its fix.**

| test | pins | proved by |
|---|---|---|
| `spaces::tests::a_repair_never_substitutes_a_query_the_space_does_not_ask_for` | a space claiming `default: about` whose query is `tag:log tag:task` offers no repair; one whose query does contain the term still does, folded for case, and a `/*` subtree term does not | dropping the `any(same_tag)` clause → `Some("tag:about")` where `None` is asserted |
| `spaces::tests::a_destination_keeper_cannot_read_as_one_path_still_names_one_and_says_so` | a list `create_dir` still names a directory (never inherits) and puts one sentence on the space; a scalar warns about nothing | reverting the arm to `("create_dir", FieldValue::Str(raw))` → no warning and `create_dir: None` |
| `sessions_root::tests::a_pool_past_the_walk_budget_says_it_is_short` | the pool carries the walk's truncation, and the file a per-kind create wrote is past the bound | not runnable on Linux — see below |
| `session-spaces.test.tsx` › `says a short list is short rather than claiming the session is empty` | the flag reaches the payload and the section renders the notice on every space, and does not also claim the session is empty | forcing the notice off → the assertion fails |

**What could not be verified here.** `keeper` is the Tauri shell crate and does
not build on this Linux box (no pkg-config/glib), so `sessions_root.rs` and
`sessions_ipc.rs` are read-not-compiled: the pool test above and the
`pool_truncated` projection are unexecuted, and their compilation is the macOS
gate's (`bun run check:rust:macos`) to confirm. The TS binding
`src/lib/ipc/gen/SessionSpaceFilesVm.ts` was regenerated by `keeper-core`'s
ts-rs export, which does build here.
