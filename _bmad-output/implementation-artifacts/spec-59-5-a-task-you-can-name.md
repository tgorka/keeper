---
title: 'Story 59.5: a task you can name'
type: 'feature'
created: '2026-08-31'
status: 'done'
baseline_revision: '925bdf4'
final_revision: '4008b40'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
---

<intent-contract>

## Intent

**Problem:** A task has ten typed columns and none of them is free text
(`db.rs:191-202`), so there is nowhere to say what a task is *for*. The epic's triage calls
this **absent**, and names why it is worse than merely absent: the Add form sends `id: ""` so
Rust mints a ULID (`sync_ipc.rs:2282`), and the edit form forbids changing an id ever, because
`task_runs.task_id` joins on it (`task-form.tsx:94-96`). Between those two rules a task's id is
either a ULID nobody chose or a word chosen once and frozen — so a person who names a task badly,
or does not name it at all, has no way back. The owner asked for *"description in the task"* and
that is what the ask is: not a notes field, a **name**.

**Approach:** One additive column, `description TEXT`, **nullable and with no `DEFAULT`** — which
is the opposite of 58.4's `on_missed` and deliberately so (see Boundaries). Carried through
`TASK_COLUMNS`, `StoredTask`, `read_task`, `decode_task`, `TaskRow` and `upsert_task`; onto
`TaskVm` and `TaskSaveReq`; reachable from **both** writers in this one story (AD-139) — a
`--description` / `--no-description` pair on `keeper-syncd tasks set`, and one control in
`src/components/sync/task-form.tsx`. Stored verbatim; blank rendered as absent by whatever draws
it.

## Boundaries & Constraints

**Always:**
- **Nullable, no `DEFAULT`**, on `ensure_journal_columns`' stated rule (`db.rs:424-428`): a row
  written before the column existed has **no** description, which is a different fact from having
  an empty one. A `DEFAULT ''` would make every pre-existing row on every install claim a blank
  description, and every surface would then have to un-tell that story.
- The `DEFAULT` that `on_missed` **required** is not required here, and the reason is exact:
  `on_missed` is `NOT NULL`, so an older binary's `INSERT` (which names only the columns it knows,
  `db.rs:3290-3293`) would fail against it without one. A nullable column needs nothing — SQLite
  fills an omitted nullable column with `NULL`. NFR-43's write half is bought for free.
- Stored **verbatim**, never trimmed or normalised. `schedule`'s rule (`db.rs:2884`) applied to a
  sharper case: this is the one column on the row that nothing but a person authored.
- `null` and `""` stay **different facts** in the store and across the wire. Only the *rendering*
  collapses them, and each renderer says so where it does it.
- The form does **not** trim what was typed — the form's own standing rule (`task-form.tsx:373-375`,
  `:385-392`). Unlike `id` and `schedule` there is no refusal behind this field to justify it, so
  the justification is the note beside the box, which promises exactly that.
- Reachable from the CLI **and** the app in this story. Hard criterion (AD-139).
- Clearing needs its **own flag**. `--description ''` cannot be told apart from a shell that
  expanded a variable to nothing, and those want opposite answers — `--no-schedule`'s exact
  precedent and reason (`commands.rs:659-665`).
- Bindings in `src/lib/ipc/gen/` are written only by `cargo test -p keeper-core` and committed from
  that run.

**Block If:** the column cannot be added without a `DEFAULT`; or a description would have to be
refused for its content, which would mean this field has a grammar and it does not.

**Never:** normalise, trim or truncate a stored description; give `description` a `DEFAULT`;
add a vocabulary or a refusal to a free-text field; hand-edit `src/lib/ipc/gen/**`; touch
`src/components/layout/tasks-pane.tsx` or its test (owned by `Main` for 59.1 — see the Boundary
Hand-off); touch `_bmad-output/planning-artifacts/**`; add the per-task missed-window delay
(that is 59.6) or the schedule preview (that is 59.7).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Default on upgrade | a row written before the column existed | reads back `description = None`, and explicitly **not** `Some("")` | No error expected |
| Older write | `INSERT` naming only the eleven columns it knows | succeeds; the row reads `None` | No error expected |
| Written after migration | a description with leading and trailing spaces | round-trips byte for byte | No error expected |
| Blank a person typed | `Some("")` written through the door | stored as `Some("")`, not collapsed to `None` | No error expected |
| Unreadable type in the column | a value SQLite hands back as non-text | `None`; the task still **runs** | Degraded to absence, never fatal (NFR-43) |
| CLI write | `tasks set nightly --description 'the photos'` | stored verbatim; `tasks list`, `tasks status` and `--json` all read it back | No refusal exists to make |
| CLI clear | `tasks set nightly --no-description` | back to `None` — the absent case, not the blank one | clap refuses it together with `--description` |
| CLI omit | `tasks set nightly --schedule '0 4 * * *'` | the description is untouched | No error expected |
| Human render, blank | a stored `""` or `"  "` | **no line at all**; `  name: ` with nothing after it reads as a failed read | No error expected |
| Human render, absent | `None` — every row on every install today | no line, on `[on missed: …]`'s rule: a line saying nothing about every row is noise | No error expected |
| `--json` | any of the three states | the key is **always present**, carrying the stored value verbatim including `""` | No error expected |
| Form round-trip | an edit form on a named task | the box arrives holding the stored name; what is typed is sent untrimmed | No error expected |
| Form, empty box | an add form nobody typed into | sends `null`, not `""` | No error expected |
| Form, refusal | the row moved under an open edit form | Rust's sentence rendered verbatim; the typed description survives for the retry | rendered in the form that asked |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/db.rs` -- `ensure_task_columns` gains the second column and the
  doc gains the nullable-vs-`DEFAULT` argument (`:448-491`); `TASK_COLUMNS` (`:2880`);
  `TaskRow.description` (`:2911-2922`); `StoredTask` (`:2997`); `read_task` and its NFR-43 note about
  this column's fallback being the answer rather than a route to the unknown path (`:3021-3048`);
  `decode_task` (`:3050`); `upsert_task`'s `INSERT`, `ON CONFLICT` and bound params (`:3289-3316`).
- `src-tauri/crates/keeper-core/src/tasks.rs` -- `TaskVm.description` (`:295-310`) and
  `TaskSaveReq.description` (`:392-400`). Both `#[ts(export)]`; ts-rs copies these doc comments into
  the bindings, so editing them **is** a binding change.
- `src-tauri/crates/keeper/src/sync_ipc.rs` -- `task_vm` (`:1827`) and `sync_task_save`'s `TaskRow`
  (`:2293-2297`). **Shell crate: both symbols listed for the macOS gate.**
- **Three files are shared by construction and are NOT in this story's commit.** 59.5, 59.6 and
  59.9 all edited each of them, and each holds at least one line that cannot compile until a
  *different* story's commit lands — so no per-story ordering exists in which every commit builds
  alone. The coordinator commits all three, once, with the repo's own multi-story subject convention
  (`fix(58.4,58.5): …`, `chore(58.3,58.6): …`). See the Spec Change Log. 59.5's content in each:
  - `src-tauri/crates/keeper-syncd/src/commands.rs` -- `TaskSetArgs.description` and
    `.no_description` (`:697-714`); `task_description_text`, new, holding the blank-is-absent rule
    (`:3322-3347`); the `name:` line in `task_lines` (`:3386-3397`); the `description` key in
    `task_json` and its fixed-key-set doc (`:3490-3541`); `cmd_task_set`'s three-way resolution
    (`:3993-4003`); and in the test module the `a_task` and `set_args` fixtures, the `sorted_keys`
    key-set assertion, and `a_task_description_is_writable_and_clearable_from_the_cli_and_read_back_by_both`.
  - `dev/mock-shell.ts` -- `description` on all seven `TaskVm` fixtures (`:1710-1842`), including one
    `""` and two named rows, and `sync_task_save`'s verbatim echo (`:2082`).
  - `src-tauri/crates/keeper-sync/src/engine.rs` -- one line: `description: None,` in the `fn task`
    test fixture, forced by the new `TaskRow` field.
- `src/components/sync/task-form.tsx` + `.test.tsx` -- `TASK_FORM_DESCRIPTION_LABEL` and
  `_NOTE` (`:99-122`), the `description` form value, both seedings, the untrimmed send, and the
  control sited directly under the id.
- `src/lib/ipc/gen/TaskVm.ts`, `TaskSaveReq.ts` -- regenerated, never hand-edited.
- `src-tauri/crates/keeper-sync/tests/release_sweep.rs` -- the same one-line `TaskRow` fixture
  ripple as `engine.rs`, but this file is unshared, so it **is** in this commit. 58.4 hit both the
  same way.
- **Not touched, and not needed:** `src/lib/stores/sync.ts`. 58.4 changed it only to mirror a closed
  vocabulary (`TASK_MISSED_POLICIES`); free text has none.

## Tasks & Acceptance

**Execution:**
- [x] `keeper-sync/src/db.rs` -- add `description TEXT` to `ensure_task_columns` with the argument
  for its nullability written down; carry it through `TASK_COLUMNS`, `StoredTask`, `read_task`,
  `decode_task`, `TaskRow` and `upsert_task`.
- [x] `keeper-core/src/tasks.rs` -- `description: Option<String>` on `TaskVm` and `TaskSaveReq`,
  doc-commented for what blank means and why this is the only editable name.
- [x] regenerate `src/lib/ipc/gen/TaskVm.ts` and `TaskSaveReq.ts` via `cargo test -p keeper-core`
  and commit them from that run.
- [x] `keeper/src/sync_ipc.rs` -- carry it in both directions, with no refusal: there is no
  vocabulary here to fail to read.
- [x] `keeper-syncd/src/commands.rs` -- `--description` / `--no-description` on the
  `--schedule`/`--no-schedule` pair idiom; `task_description_text`; the `name:` line in both human
  renderings; the key in `--json`; the omitted-keeps-stored rule.
- [x] `src/components/sync/task-form.tsx` -- one `Input` under the id, seeded from the row, sent
  untrimmed, with a note that states the id rule the field exists because of.
- [x] `dev/mock-shell.ts` -- the key on all seven fixtures with a blank and two named rows, and the
  save echo.
- [x] tests: the migration/`None`-not-`Some("")` claim in `db.rs`; the flag pair and both renderings
  in `commands.rs`; the round-trip, the `null`-not-`""` send, the verbatim refusal and the note in
  `task-form.test.tsx`.

**Acceptance Criteria:**
- Given a store written by a keeper with no `description` column, when `migrate` runs, then every
  row reads `None` and not `Some("")`.
- Given an older binary's `INSERT` against the migrated schema, when it names only the columns it
  knows, then the write succeeds — with no `DEFAULT` to make it so.
- Given a description written from either writer, when it is read back, then it is byte for byte
  what was given, including leading and trailing spaces.
- Given `--no-description`, when `tasks set` runs, then the row returns to the **absent** state and
  not to the blank one.
- Given a stored blank, when the human CLI renders the row, then no name line is drawn, while
  `--json` still carries the key with `""`.

## Spec Change Log

### 2026-08-31 — three shared files moved out of this story's commit, by the coordinator

**What changed:** no code. The CLI half of this story, the mock-shell fixtures and one `engine.rs`
fixture line are unchanged as *code* and are no longer in this story's *commit*.
`keeper-syncd/src/commands.rs`, `dev/mock-shell.ts` and `keeper-sync/src/engine.rs` land as one
coordinator commit carrying 59.5's, 59.6's, 59.8's and 59.9's hunks together.

**Why, and it is a fact about git rather than a preference.** `git commit --only -- <paths>` commits
the **worktree content** of the paths it is given. It isolates a committer from files they did not
name; it does **not** isolate them from a sibling inside a file they both named. So whoever named one
of those three files first would have committed the other stories' hunks under their own subject.

Sequencing could not fix it, and that is the actual finding. Each of the three files held at least
one line that cannot compile until a *different* story's commit lands — `commands.rs` needs 59.9's
`TaskKind::Verify` for its `TaskKindArg::Verify` arm and 59.6's `missed_delay_ms` column for its
flag; `engine.rs` needs the same column for its one reader. There is therefore **no order** in which
those files can be committed per-story and have every commit build alone, which is a property this
repo enforces. The coordinator committing all three once is not a workaround for a scheduling
failure; it is the only arrangement that exists.

The multi-story subject has precedent in this repo already — `fix(58.4,58.5): …`,
`chore(58.3,58.6): …` — so nothing was invented.

**What it costs this spec:** nothing in the contract. AD-139's criterion — *reachable from the CLI
and the app, or not at all* — is still met by the story, now across two commits rather than one. The
Verification section's CLI result stands unchanged: it was run, and mutation-proved, against exactly
the `commands.rs` content that goes into the shared commit.

**What a reviewer should therefore do:** review 59.5 as the union of this commit and the
coordinator's shared one. Reading this commit alone will show a `TaskVm.description` and a
`TaskSaveReq.description` with no CLI writer, which would look like a half-built story and is not.

## Design Notes

**Why the nullability argument is the whole story.** 58.4's column needed a `DEFAULT` and this one
must not have one, and the two facts have the same cause read from opposite ends: `on_missed` is
`NOT NULL`, so an omitted `INSERT` column has nothing to fall back on; `description` is nullable,
so it has. The tidy-looking `DEFAULT ''` is one word away, reads as harmless, and would quietly
convert *"nobody named this"* into *"somebody named this nothing"* on every row of every existing
install. That is why the test asserts `None` **and explicitly `assert_ne!` against `Some("")`**: an
`is_none()` check would pass under either reading of what the column means.

**Where the blank-is-absent rule lives, and why it is in two places.** The store keeps `null` and
`""` apart; both renderers collapse them. That is not an inconsistency, it is the division of
labour — `taskReportText` (`tasks-pane.tsx:655`) arrived at exactly this rule for exactly this class
of writer, and `task_description_text` is its Rust counterpart. They are two functions because they
are in two languages, not because they are two decisions, and each one's doc says so and points at
the other. The `--json` document deliberately does **not** go through either: a machine consumer is
owed the row, not a rendering of it.

**Why a line rather than a column, and why only when there is one.** `task_lines`' first line is
positional and a description has no bound, so appending it there would put unbounded prose where a
reader is counting columns. It is therefore a second line — and drawn only when there is something
to draw, on `[on missed: …]`'s rule (`commands.rs:3370-3375`) rather than `last:`'s: every row on
every install predates this column, so `name: none stored` on all of them would be a line that says
nothing about anything.

**Why an `Input` and not a `Textarea`.** The ask is a name, the row that will draw it has one line
for it, and a box that invites paragraphs would promise a surface that does not exist. If Epic 60
ever wants real notes on a task, that is a different field with a different bound.

**What the note has to say, and why it is about the id.** The field's own behaviour is guessable;
the reason it exists is not. An add form sends `""` to have Rust mint a ULID, and an edit form
cannot change an id at all. A reader who does not know both of those keeps hunting for an editable
name. So the note states them, and its test asserts them against `TASK_FORM_ID_ADD_NOTE` and
`TASK_FORM_ID_EDIT_NOTE` rather than against a copy of the sentence, so the three cannot come to
disagree about which one is frozen.

**One place the two states genuinely merge, stated because it is a real loss.** An edit form seeds
`task.description ?? ""`, so a stored `""` and a stored `null` both arrive as an empty box — a box
cannot show the difference. Sending it back then writes `null`. So an edit that touches nothing else
normalises a blank somebody once typed into the absence it already looked like. That is acceptable
and is the only behaviour a text input can honestly have; it is written down so a later reader does
not take it for a bug in the store, which does keep them apart.

## Verification

**Commands (this story's own, narrow by instruction — the coordinator owns the project-wide gates):**
- `cargo test -p keeper-sync --lib the_description_column` -- expected: 1 passed.
- `cargo test -p keeper-syncd --bins a_task_description` -- expected: 1 passed.
- `bun run vitest run src/components/sync/task-form.test.tsx` -- expected: 24 passed (21 before).
- `cargo test -p keeper-core` -- regenerates the two bindings, committed from that run.
- `bunx biome check` over the three frontend files this story owns -- expected: clean.
- `cargo fmt` -- verified against my four Rust files; one `assert_eq!` canonicalised
  (`fn_call_width`) and applied.

**Why the Rust runs happened in a throwaway worktree.** `keeper-sync` did not compile in the shared
worktree for most of this story, through no fault of this change: a sibling agent's in-flight
`engine.rs` references a `missed_delay_ms` column that is 59.6's to add, and another sibling's
`TaskKindArg::Verify` (59.9) references a `TaskKind` variant that does not exist yet. Both are
expected mid-wave states. So every Rust command above was run in `git worktree add /tmp/v595 925bdf4`
carrying **only this story's diff**, which is also a cleaner claim than the shared tree could have
made. `bun run typecheck` was run in the shared tree and its only task-related diagnostic is the
hand-off below.

**Mutation proof.** Four guards mutated one at a time, each owning test confirmed to fail on the
exact fact it claims, and **every restore verified by `diff` against the pre-mutation file** rather
than by remembering what was changed:

| mutation | owning test that failed | the failure it produced |
|---|---|---|
| `ADD COLUMN description TEXT NOT NULL DEFAULT ''` | `the_description_column_is_additive_and_a_row_without_one_has_none` | `01NAMED predates the column, so it has no description` — `left: Some("") right: None`, which is precisely the fact the story turns on |
| `--no-description` made inert (`if false`) | `a_task_description_is_writable_and_clearable_from_the_cli_and_read_back_by_both` | `` `--no-description` restores the absent case, not the blank one `` — `left: Some("") right: None` |
| `task_description_text`'s trim filter removed | same test | `blank draws nothing: a heading over an empty string reads as a failed read` — the `name:` line was drawn over a stored `""` |
| the form's send changed to `.trim()` | `round-trips the stored description and sends what was typed, untrimmed` | `- "description": "  the photos, nightly  "` / `+ "description": "the photos, nightly"` |

**Manual checks (if no CLI):**
- The `keeper` shell crate cannot link on this host (glib/pkg-config). Symbols touched there, listed
  for the macOS gate: `sync_ipc.rs::task_vm`, `sync_ipc.rs::sync_task_save`. Both are single-field
  additions to struct literals whose other fields are unchanged.

## Boundary Hand-off — what `Main` must wire in `tasks-pane.tsx`

Nothing was exported from that file and nothing in it was edited. Two items:

**1. One line of fixture, required for `bun run typecheck` to pass.**
`src/components/layout/tasks-pane.test.tsx:155`'s `task()` builder is a full `TaskVm` literal, so
the new required field breaks it. It is the only remaining typecheck error in the tree:

```
src/components/layout/tasks-pane.test.tsx(156,3): error TS2322: Type '{ … description?: string | null | undefined; … }' is not assignable to type 'TaskVm'.
```

Add `description: null,` beside `onMissed` (`:168`), which already carries the comment explaining
why the fixture holds fields this pane does not render.

**2. The row render, to whatever level 59.1 sites it at.**

- **Label:** `Description` is the form's word. The row may prefer `Name` — the epic's own design
  note says *"the name he means already exists"* — but the two surfaces should not use three words
  for one column, so pick one and use it in both, or state why the row differs.
- **Placement:** directly **under the id**, which is where the form puts the control and why: the id
  is a frozen or minted key, and this is the name a person reads. On a level-1 list line it belongs
  beside the id rather than under it; on the level-2 detail it is a `Field`.
- **The blank rule, and it is not optional:** absent **and** blank both render as **nothing** — no
  label, no empty cell. This is `taskReportText`'s existing rule and the reason is its reason: a
  heading over an empty string is the one shape a reader takes for a failed read. `TaskVm.description`
  can be `null` or `""` or `"  "`, all three reachable, so the test is
  `description === null || description.trim() === ""`. If a `taskDescriptionText(task)` helper is
  wanted, it is `taskReportText`'s shape exactly and the Rust counterpart is
  `commands.rs::task_description_text`.
- **`TASK_LAST_REPORT_LABEL`'s treatment is the precedent to copy** (`:272`, rendered at
  `:1001-1005`): a `Field` drawn only inside `{report !== null && (…)}`, with the null-deciding rule
  in a named function rather than inline. A description is shorter than a run report, so it does not
  need `wide`.
- **The mock shell already exercises all three states** for a visual check: a named row
  (`01JNIGHTLYSYNC…`), a `""` row (`01JRELEASESWEEP…`, which must draw nothing), and `null` rows.
  The orphaned-task fixture (`01JORPHANEDTASK…`) is named on purpose — a ULID id and a folder that
  is gone is the row where a name earns its keep.

## Review Triage Log
