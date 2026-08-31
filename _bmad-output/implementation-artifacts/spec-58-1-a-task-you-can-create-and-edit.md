---
title: 'Story 58.1: a task you can create and edit from the app'
type: 'feature'
created: '2026-08-31'
status: 'in-review'
baseline_revision: 'dbb7874'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** `sync_task_save` and `sync_task_forget` are implemented, registered, typed, wrapped in `client.ts` and mocked in `dev/mock-shell.ts`, and **no control in the app has ever called either** — the ⌘8 Tasks pane has exactly two buttons (Refresh, Run now) and an empty state that tells the owner to open a terminal. Create, edit and delete are *unreachable*, not absent.

**Approach:** One `TaskForm` component in the `AddFolderForm` mould — two modes on `editing = task !== undefined`, seeded once, native `<form onSubmit>`, Rust's refusal rendered verbatim — revealed inline in the Tasks pane (header disclosure for add, per-row disclosure for edit), plus an `AlertDialog` confirm for Forget. **Frontend only: no Rust file changes and no `src/lib/ipc/gen/**` changes.**

## Boundaries & Constraints

**Always:**
- Send what was typed. The id goes **untrimmed** (`tasks::validate_id` refuses a padded id on purpose, `tasks.rs:704-711`); the schedule goes as written (`TaskSchedule::parse`, floor `60_000` ms, ceiling 366 d).
- Render the rejection's own sentence with `syncErrorMessage`, corrected in no way, in the form that asked.
- Creating sends `id: ""` so `sync_ipc.rs:2203` mints the ULID; editing sends the stored id verbatim so `upsert_task` updates rather than duplicates.
- `mode` and `enabled` stay two controls (AD-135: `decide` reads both, `tasks.rs:726`).
- Profile is **picked from `syncProfiles()`**, never typed: this is the one refusal the backend does not make, and a `profileId` naming nothing comes back `unhosted` and fails at run time with `"no such folder"`.
- A stored `profileId` the picker's list does not contain gets its own option — a `<select>` whose value matches no option renders the FIRST one, which here is "the whole machine", a scope change the next Save would make true (`template-select.tsx:9-12`).
- Any save or delete is followed by the pane's existing `refresh()`.

**Block If:** a fix appears to require editing a Rust file or `src/lib/ipc/gen/**`.

**Never:** re-implement or pre-validate any Rust rule (id emptiness/padding, schedule grammar/floor/ceiling, scheduled-with-no-schedule, kind/mode readability, the NFR-43 stored-row guard); send `nextDueMs`/`runningHost`/`leaseUntilMs` (no keys exist); tidy input to make a save succeed; offer edit or delete on an `unknown` row; predict the host sentence; use a `Dialog` for create/edit (AD-C7 idiom is inline disclosure); collapse mode and enabled into one control; touch `_bmad-output/planning-artifacts/**` (owned by `Epic58Plan`).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Create | add form, id box left blank, kind `sync`, mode `scheduled`, schedule `@daily`, profile "the whole machine" | `syncTaskSave({ id: "", kind: "sync", mode: "scheduled", enabled: true, profileId: null, schedule: "@daily" })`; form closes; pane re-reads | No error expected |
| Edit | edit form on stored task `01SCHED` | fields arrive filled; save sends `id: "01SCHED"` — one row, not two | No error expected |
| Padded id | typed id `" nightly"` | sent untrimmed | `invalid sync configuration: task id must not begin or end with whitespace, got " nightly"` shown inline, every typed value kept |
| Sub-minute schedule | schedule `every 30s` | sent as typed | `invalid sync configuration: task schedule must not fire more often than once a minute (60000 ms), got "every 30s"` shown inline |
| Scheduled, no schedule | mode `scheduled`, schedule box empty | `schedule: null` sent | Rust's `is scheduled with no schedule` refusal shown inline |
| Stored profile gone | editing a task whose `profileId` names no listed profile | that id is its own option and stays selected | — |
| Delete | Forget pressed | confirm first, saying it deletes a record and never content; only then `syncTaskForget(id)`, then `refresh()` | rejection shown on the row |
| Profile read fails | `syncProfiles()` rejects | picker still offers "the whole machine" and the stored id; the read's own sentence is shown | Non-fatal — the form stays usable |

</intent-contract>

## Code Map

- `src/components/sync/task-form.tsx` — NEW. The one component, two modes. Pattern: `add-folder-form.tsx:1046-1063` (seed once), `:1219-1224` (`setSaving`/`setError(null)`), `:1445-1449` (`setError(syncErrorMessage(raw))`, `finally`), `:1455-1461` (native submit), `:2079-2093` (mode-dependent submit label + Cancel).
- `src/components/sync/task-form.test.tsx` — NEW. Twins of `add-folder-form.test.tsx:219` / `:376` / `:512` / `:1001`.
- `src/components/layout/tasks-pane.tsx` — the three mount points (header add, per-row edit, per-row forget), the `AlertDialog` confirm, the three rewritten `TASKS_PANE_EMPTY_*` constants and the pane doc comment that claims creation is impossible (`:57-106`, `:71-74`).
- `src/components/layout/tasks-pane.test.tsx` — `:365-399` assert the copy says "cannot create one yet" and ban `/\badd\b/`; both must change with the copy.
- `src/lib/stores/sync.ts:39-45` — where `TASK_KINDS`/`TASK_MODES` go, beside `SYNC_DIRECTIONS`/`SYNC_LFS_MODES`: one registry of vocabularies Rust enumerates and no IPC ships.
- `src/lib/ipc/client.ts:6382,6391` — `syncTaskSave`, `syncTaskForget`; `:3056` `syncProfiles`.
- `src/components/layout/files-pane.tsx:3146-3180` — the `AlertDialog` confirm idiom (driven in tests via `findByRole("alertdialog")`).
- `src/components/sessions/session-file-actions.tsx:51-56` — why a native `<select>` and not Radix here: Radix `Select` throws on an empty-string value, and "the whole machine" **is** the empty-string sentinel for `profileId: null`.

## Tasks & Acceptance

**Execution:**
- [x] `src/lib/stores/sync.ts` -- add `TASK_KINDS = ["sync", "release"]` and `TASK_MODES = ["off", "manual", "scheduled"]` with derived types, each doc'd as a mirror of `TaskKind::from_stored` / `TaskMode::from_stored` -- no IPC enumerates them and a second registry in a component would drift.
- [x] `src/components/sync/task-form.tsx` -- the component: exported copy constants, six controls (id, kind, mode, enabled switch, profile, schedule), `saving`/`error`, verbatim refusal, `onSaved`/`onCancel` -- one component so the add and edit surfaces cannot word or validate a task differently (AD-C7).
- [x] `src/components/layout/tasks-pane.tsx` -- reveal it: header "Add a task" disclosure, per-row Edit disclosure and Forget with an `AlertDialog`; rewrite the three empty-state constants and the doc comment; refresh after every save and delete; no controls on `unknown` rows.
- [x] `src/components/sync/task-form.test.tsx` -- the I/O matrix: create sends `id: ""`, edit sends its own id, a rejection is shown verbatim and keeps every typed value, the padded-id and sub-minute refusals arrive uncorrected, the stored-profile-gone option survives.
- [x] `src/components/layout/tasks-pane.test.tsx` -- rewrite the empty-state assertions; keep the mechanical `keeper-syncd <group> <verb>` drift guard, narrow the blanket `/\badd\b/` ban to a CLI-verb ban; add the reveal/edit/forget-confirm cases.

**Acceptance Criteria:**
- Given a keeper with no tasks, when the owner opens ⌘8, then the empty state offers a control that creates one and never instructs them to open a terminal instead.
- Given a stored task, when the owner presses Edit, then the form arrives filled from the row already on screen with no extra IPC read, and Save updates that row.
- Given an `unknown` row, when the owner looks at it, then it carries no Edit and no Forget.
- Given a save or a delete that succeeds, when it settles, then the listing is re-read so `nextDueMs` and the host verdict move.
- Given a Forget, when the owner is asked to confirm, then the question says it deletes a record and never content.
- No Rust file and no file under `src/lib/ipc/gen/` is modified.

## Spec Change Log

## Review Triage Log

### 2026-08-31 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 15: (high 0, medium 6, low 9)
- defer: 1: (high 0, medium 1, low 0)
- reject: 3: (high 0, medium 0, low 3)
- addressed_findings:
  - `[medium]` `[patch]` A Forget confirmed while that row's edit form had a save in flight deleted a row the settling save re-inserted (`upsert_task` inserts when the id is absent), silently undoing a confirmed deletion — `TaskForm` now reports `saving` through `onSavingChange`, the pane holds `formSaving`, and the row's Forget is disabled while a write is on its way.
  - `[medium]` `[patch]` Pressing a disclosure toggle mid-save unmounted the form, so Rust's refusal had nowhere to land and a collapsed disclosure with no message read as a save that happened — the header's Add and each row's Edit are disabled by the same `formSaving` flag.
  - `[medium]` `[patch]` A second confirmation re-issued a delete for a task already going — `deleting: Record<string, true>` beside `running`, on `runNow`'s finding-7 shape.
  - `[medium]` `[patch]` A refused Forget whose row the re-read no longer listed was written to `refusals[id]`, which only `TaskRow` draws, so a failed delete was indistinguishable from a successful one — orphaned refusals are promoted to a pane-level `TASKS_ORPHAN_REFUSAL_TESTID` alert, named by their task.
  - `[medium]` `[patch]` The unlisted-folder option was keyed off the *current* picker value, so selecting "the whole machine" to compare removed it and the gone folder's id became unrecoverable — keyed off `task.profileId` now.
  - `[medium]` `[patch]` Keyboard focus fell to `<body>` whenever a form closed, because both closing controls live inside it — the header and each row hold a trigger ref and focus it on the open→closed transition, `recording-summary-card.tsx`'s shape.
  - `[low]` `[patch]` `taskForgetConfirmTitle(forgetting ?? "")` rendered "Forget task ?" through `AlertDialogContent`'s 100 ms exit animation, on the one dialog whose job is naming the record — the subject now outlives the ask in its own state slot.
  - `[low]` `[patch]` A schedule of nothing but spaces was coerced to `null` instead of being sent for `TaskSchedule::parse` to refuse — `=== ""` rather than `.trim() === ""`, matching the untrimmed id two lines above.
  - `[low]` `[patch]` `profiles === null` was collapsed to `[]` before any consumer saw it, so the picker held one option during its own read and was indistinguishable from a machine with no folders — `TASK_FORM_PROFILE_READING_NOTE`, the pane's own unknown-not-empty rule.
  - `[low]` `[patch]` A stale `editingId` auto-expanded a re-created id, since the Add form takes a typed id — `refresh` drops an `editingId` the read no longer contains. `forgetSubject` deliberately is not pruned: that read fires on every Run now settle, and closing a question under the person answering it is worse.
  - `[low]` `[patch]` `TASK_FORM_ID_ADD_NOTE` did not say that an id a task already has *replaces* that task — `upsert_task` has no create-only mode, so the note now admits it where the id is chosen.
  - `[low]` `[patch]` The unlisted-option comment claimed "the next Save would make that claim true"; React's controlled-select fallback mutates the DOM and fires no `change`, so the stored id is still what is sent. Reworded to the real defect: the control reports a scope the task does not have and no control can express the real one.
  - `[low]` `[patch]` `TASK_FORM_EDIT_TITLE`'s doc said several rows can have a form open at once, contradicting `editingId`'s single slot — restated as distinguishing the Add and Edit forms on one screen.
  - `[low]` `[patch]` The successful-Forget test answered the same listing to both reads, so it passed over an implementation that deleted the wrong id — it now answers an empty listing second and asserts the row leaves the pane.
  - `[low]` `[patch]` The refused-Forget test mocked `"no such task"`, which this path cannot emit (`db::delete_task` is two unconditional DELETEs), and asserted the refusal globally against a one-row fixture — an `internal` store error on a two-row fixture, asserted `within` the right row.
  - Deferred: a task edit form open across a listing change writes its seed-time values back over another host's change, because `upsert_task` has no compare-and-set. Recorded in `deferred-work.md`; the honest fix is an `updated_ms` precondition in Rust, which a frontend-only story could not make.
  - Rejected: seeding `kind`/`mode` through a `find(…) ?? default` narrowing (unreachable — `db::decode_task` partitions on both, and defaulting a `release` task to `sync` would be a worse lie than the state it guards); an add-form existence check against the loaded listing (that is a rule Rust owns, and the note says it instead); and holding the confirm dialog open over a refusal (it would hide the row the refusal is written on).

## Design Notes

**Why the empty-state copy still names the CLI.** The rewrite must stop *instructing* the owner to open a terminal while a button sits above the sentence, but `keeper-syncd tasks set` is still how a headless Linux daemon host gets a task, and the mechanical CLI-drift guard in `tasks-pane.test.tsx:365` is real value. So the button becomes the primary path and the command is named as the other one — which keeps the guard alive. The blanket `expect(COPY).not.toMatch(/\badd\b/)` goes, because the UI now legitimately says "Add"; the phrase loop already checks every `keeper-syncd <group> <verb>` against the real clap tree, which is what that ban was for.

**No kind/mode fallback option, deliberately.** `db::decode_task` (`db.rs:2998-3003`) partitions on kind AND mode: a spelling this build cannot read never reaches `listing.tasks`, so every editable `TaskVm` has a kind in `TASK_KINDS` and a mode in `TASK_MODES`. The `profileId` case is different and does need its own option, because `profile: null` with `profileId: "…"` is a live state `task_host` acts on.

**Deviation from `bmad-dev-auto` step-01.** No `epic-58-context.md` was compiled: epic 58's planning doc is being authored concurrently by `Epic58Plan`, which owns `_bmad-output/planning-artifacts/**`, and compiling context from a document that does not exist yet is not possible. Context is the four read-only triage passes (`agent://CreateEditUi` above all), `epic-57-context.md` and `spec-57-5-the-app-runs-them-too.md`. Confirmed with `Epic58Plan` over `hub` before starting.

## Verification

**Commands:**
- `bun run typecheck` -- expected: clean.
- `bunx vitest run src/components/sync/task-form.test.tsx src/components/layout/tasks-pane.test.tsx` -- expected: all green.
- `bun run lint` -- expected: at baseline (4 warnings + 1 info).
- `bun run test` -- expected: green at or above 300 files / 4978 tests.
- `git diff --name-only` -- expected: no path under `src-tauri/` and none under `src/lib/ipc/gen/`.

**Manual checks (if no CLI):**
- Mutate one guard away (send the id trimmed) and confirm the padded-id test fails; restore and verify by reading `git diff`, not from memory.
