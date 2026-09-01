---
title: 'Several tasks at once — the bulk consumer, then the selection model'
type: 'feature'
created: '2026-09-01'
status: 'done' # draft | ready-for-dev | in-progress | in-review | done | blocked
baseline_revision: '6606fcb'
review_loop_iteration: 0
followup_review_recommended: true
final_revision: '6bcb830' # the commit holding this spec at its final revision; the stamp itself follows it
context: []
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** Epic 59's 59.4 paragraph **refused** a multi-select in the Tasks pane, and the reason it
gave was checkable rather than aesthetic: *every task write in the whole stack is single-id*, so a
checkbox column would be "state whose only possible action is a loop of N writes, each with its own
conflict check and its own partial-failure story" — and it would be the second selection idiom, which
`spec-45-17…:200` forbids by name. The owner has now asked for several tasks at once.

**Approach:** Overturn the refusal **on the epic's own test**, in the order the epic itself
prescribed. `spec-43-8…:347-348` refused a Files selection for the same reason and was correctly
overturned at 45.3 *once a bulk consumer existed*. So this story builds the consumer first — a
batched `enable`/`disable`/`forget` over ids with a **per-id receipt** in `db.rs`, `engine.rs` and the
CLI — and only then a selection model **copied** from `files-pane.tsx`, not invented. Half one is
committed separately from and before half two.

## Boundaries & Constraints

**Always:**
- **The consumer lands before the state.** Commit 1 is Rust only (`db.rs`, `engine.rs`,
  `commands.rs`, `keeper-core/src/tasks.rs`) and contains no selection state. Commit 2 is the IPC
  verbs and the pane.
- **Per-id independence, not all-or-nothing.** Each id gets exactly the write door it gets today,
  with its own transaction. See Design Notes for the argument; this is the one design decision the
  epic said *is* the whole story.
- **N requested ids produce N receipt entries, in request order.** No dedup, no collapsing, no
  first-error-wins. `Err` is reserved for the store failing outright (the listing cannot be read) and
  is never one id's refusal — `FilesDeleteReceiptVm`'s rule (`vm.rs:4744-4750`): *"Partial success is
  a real outcome and is reported rather than thrown."*
- **Every side effect the single-id path has, per id.** `save_task` clears the process-local fault on
  `TaskSave::{Created, Rearmed}` only (`engine.rs:8366-8368`); `forget_task` clears it
  unconditionally (`engine.rs:8392`). The batch keys the clear on each id's own outcome and does
  **not** unify the two policies.
- **NFR-43 and AD-48, per id.** A row this build cannot read is refused, never overwritten
  (`db.rs:3273-3288`) and never deleted (`commands.rs:4132-4138`).
- **The selection model is copied.** Plain click replaces, Cmd/Ctrl toggles one, Shift takes the run
  over the flat visible order, `aria-selected` on **every** row (`"true"` and `"false"`, never
  omitted), `aria-multiselectable="true"` on the container, and the count sentence goes through
  `countLabel` with a `CountNoun` — never a hand-rolled plural.
- **Refusals render in Rust's words**, per id, from the receipt.

**Block If:**
- The macOS gate rejects a shell-crate symbol this story adds (`keeper/src/sync_ipc.rs`,
  `keeper/src/lib.rs`) — that crate does not compile on this Linux host.

**Never:**
- A checkbox column. Selection is on the row itself, as `files-pane.tsx` does it; 59.1's test that
  every row holds exactly one button stays green. Bulk actions live in the header beside the count —
  45.3's rule: *"a per-row Delete button cannot answer 'and the other four'."*
- A second selection slot beside 59.1's. The one slot widens from `string | null` to a set.
- Bulk actions on `TaskListing.unknown` rows or on 58.7's projected paced rows: neither is a
  `TaskVm`, and a control that can only fail is worse than no control.
- Selection issuing any IPC read. `spec-58-3`'s Never clause, enforced by
  `reads nothing at all when a task is chosen`.
- A batched `run`. Runs take a lease and are `async`; nothing asked for it.
- Inventing a baseline for `forget`. `delete_task` makes no such promise today.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| all succeed | 3 stored ids, `enabled: false` | 3 entries, each `Saved(Updated)`; window kept | none |
| a disabled id comes alive | stored `enabled = 0`, `enabled: true` | `Saved(Rearmed)`; `next_due_ms` cleared; **fault cleared** | none |
| an idle re-save | identical row | `Saved(Updated)`; **fault NOT cleared** | none |
| **one baseline moved, others did not** | 3 ids, one `baseline_updated_ms` ≠ stored | 2 × `Saved`, 1 × `Refused("… changed elsewhere …")`; the refused row is **unchanged** | per id |
| baselined id forgotten elsewhere | `Some(baseline)`, row gone | `Refused("… no longer exists …")`; never re-inserted | per id |
| unreadable stored row | id in `TaskListing.unknown` | `Refused("… stored, but this keeper cannot read it …")`; not written, not deleted | per id |
| malformed id | `" "` or `"x "` | `Refused(validate_id`'s sentence`)` before any SQL | per id |
| no such id | well-formed, unstored | `Missing` — distinct from `Refused` | per id |
| forget a stored id | id in `listing.tasks` | `Forgotten`; `task_runs` cascade; fault cleared | none |
| forget an unstored id | absent | `Missing`; fault cleared (matches `forget_task`) | none |
| empty batch | `[]` | `Ok(vec![])`; CLI says so rather than printing nothing | none |
| the same id twice | `["a", "a"]` | 2 entries; the second refuses if it carried a baseline | per id |
| listing unreadable | store failure | `Err(SyncError::Journal)` — the one whole-batch failure | whole call |
| CLI, some refused | `tasks disable a ghost` | one line per id; `{ "results": [...] }`; exit `EXIT_CONFIG` | via `Ok(u8)` |
| pane, 3 of 5 refused | selection of 5 | the three reasons rendered per id, in Rust's words; the selection is **not** silently shrunk | per id |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/db.rs` — `upsert_task:3226` (validation → NFR-43 refusal → baseline
  CAS → write, in one `unchecked_transaction:3248`), `TaskSave:3367`, `delete_task:3883`,
  `list_tasks:3384`, `TaskListing:2995`, `TaskRow:2905`. Batch precedents: `record_activity:2764`
  (empty-input early return), `release_host_leases:3699` (runtime-sized binding). Tests: `conn():3896`,
  `task():5892`, `raw_task():5911`, `…baseline_has_moved…:7221`, `what_a_save_reports…:7420`.
- `src-tauri/crates/keeper-sync/src/engine.rs` — `save_task:8364` (3 statements; the fault clear at
  `:8366-8368`), `forget_task:8390`, `tasks():8374`, `with_db:1507`, `lock:1501`,
  `task_faults: Mutex<HashSet<String>>:957`, `note_task_outcome:2030`. Test fixture `engine():13377`,
  `task():13476`, `notifications_posted:19100`, and the test the batch must not regress:
  `a_task_coming_back_into_service_clears_its_fault…:15046`.
- `src-tauri/crates/keeper-syncd/src/commands.rs` — `TaskCommand:501` (`Enable:606`, `Disable:615`,
  `Forget:624`), dispatch `:1114-1124`, `cmd_task_set_enabled:4105`, `cmd_task_forget:4139`,
  `select_task:3244`, `unreadable_task:3203`, `Printer:964`, `task_lines:3393`, `report_task:3897`,
  `task_forgotten_document:3708`, `sorted_keys:5866`, `every_subcommand_parses:4197`,
  the required-selector test `:4281`. `Vec<String>` idiom: `AddArgs.subpath:765`. Per-item loop
  precedent: `cmd_verify:1747` — *"let one folder's failure be one folder's."*
- `src-tauri/crates/keeper-core/src/tasks.rs` — house rules `:23-38`; `TaskVm:274` (`updated_ms:339`),
  `TaskRunVm:236`, `TaskSchedulePreviewVm:1179` (the flat-struct-with-`Option` decision, `:1163-1178`).
  `keeper-core` may not depend on `keeper-sync` (AD-40, `package.json:26`).
- `src-tauri/crates/keeper-core/src/vm.rs` — `FilesDeleteReceiptVm:4744`, `FilesDeleteRefusalVm:4526`,
  `IpcError:1827`. The receipt precedent and its argument.
- `src-tauri/crates/keeper/src/sync_ipc.rs` — `sync_task_save:2322` (the only `Some(baseline)` caller,
  `:2386-2388`), `sync_task_forget:2430`, `sync_ipc_error:903`, the 45.3 receipt loop `:4560-4582`.
  **Shell crate: does not compile on this host.**
- `src-tauri/crates/keeper/src/lib.rs:980-984` — `generate_handler!`. **Shell crate.**
- `src/lib/ipc/client.ts` — the barrel (`:267-273` exports, `:420-424` imports) and the wrappers
  (`syncTaskSave/syncTaskForget:6388-6399`, each doc ending `Rejects with:`).
- `src/lib/ipc/gen/**` — written only by the ts-rs derive; regenerate and commit
  (`package.json:24 bindings:check`).
- `dev/mock-shell.ts` — the browser-dev shell must answer the new verbs.
- `src/components/layout/files-pane.tsx` — **the one selection idiom.** `selected`/`anchorKey:1156-1171`,
  `select:1656-1706`, `selection` memo `:1708`, `deletable:1726`, `handleRowClick:2146`,
  Space/Delete/Escape `:2061-2094`, `aria-selected:2528-2532`, `aria-multiselectable:3096-3102`,
  `filesSelectionSentence:233-247`, the count badge gate `:2989-3010`, the receipt render `:1875-1891`.
- `src/lib/count-label.ts` — `countLabel:93`, `ITEMS:61`, `RUNS:73`.
- `src/components/layout/tasks-pane.tsx` — 59.1's master/detail. `selectedId:1601`, `selectTask:2050`,
  `refusals:1571`, `messageOf:755`, `orphanRefusals`, `TASKS`/`TASKS_RAIL_LIST_LABEL`.
- `src/components/layout/files-pane.test.tsx` — the semantics to assert against: `:2609` plain-replaces,
  `:2630` Cmd-adds-one/Shift-fills, `:2657` Ctrl-is-Cmd, `:2790` names-what-it-could-not-delete,
  `:2912` Escape clears.
- `docs/sync.md:2026-2032` (verb table), `:2163-2171` (the `--json` envelopes this story changes).

## Tasks & Acceptance

**Execution — commit 1, the bulk consumer (Rust only, no selection state):**
- [x] `src-tauri/crates/keeper-sync/src/db.rs` — add `TaskBatchId { id, baseline_updated_ms }`,
  `TaskBatchOutcome { Saved(TaskSave), Forgotten, Missing, Refused(String) }`,
  `TaskBatchEntry { id, outcome }`, and `set_tasks_enabled(conn, &[TaskBatchId], enabled, now_ms)` /
  `forget_tasks(conn, &[String])`. Each reads `list_tasks` **once**, then per id: `validate_id` →
  `Refused`; in `listing.unknown` → `Refused` with `unreadable_task`'s sentence; absent → `Missing`;
  otherwise `TaskRow { enabled, updated_ms: now_ms, ..stored.clone() }` through the **existing**
  `upsert_task` (`Saved(effect)` / `Refused(err.to_string())`), or `delete_task` (`Forgotten`). Doc
  the transaction-scope decision on the functions. Rationale: the batched statements are the consumer
  the epic's refusal said was missing, and reusing the single-id door is what keeps its promises.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` — `set_tasks_enabled(&self, &[db::TaskBatchId], enabled)`
  and `forget_tasks(&self, &[String])`, one `platform.now_ms()` stamp for the whole batch, one
  `with_db` hold, then the fault bookkeeping **per entry**: clear on `Saved(Created | Rearmed)`,
  clear on `Forgotten | Missing`, never on `Saved(Updated)` or `Refused`. Rationale: `Updated` must
  not re-arm the alarm (`engine.rs:15101-15107`) and `Refused` must not lose a live fault.
- [x] `src-tauri/crates/keeper-core/src/tasks.rs` — `TaskBatchIdReq { id, baseline_updated_ms }`,
  `TaskBatchOutcomeKind { Saved, Forgotten, Missing, Refused }`,
  `TaskBatchEntryVm { id, outcome, effect: Option<String>, reason: Option<String> }`,
  `TaskBatchReceiptVm { entries }` — all with the file's derive triple and `#[ts(type = "number | null")]`
  on the baseline. Rationale: the receipt must cross the wire for half two.
- [x] `src-tauri/crates/keeper-syncd/src/commands.rs` — `Enable`/`Disable`/`Forget` take
  `#[arg(required = true, num_args = 1.., value_name = "TASK")] task: Vec<String>`; `cmd_task_set_enabled`
  and `cmd_task_forget` call the batched engine verbs, render **one line per id**, emit one
  `tasks_batch_document` naming `results`, and return `EXIT_CONFIG` through the `Ok(u8)` channel when
  any entry is not a success. Update the enum doc's "seven things" sentence.
- [x] `src-tauri/crates/keeper-sync/src/db.rs` (tests) — the receipt tests below.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` (tests) — the fault-bookkeeping test below.
- [x] `src-tauri/crates/keeper-syncd/src/commands.rs` (tests) — the CLI tests below; add the batched
  spelling to `every_subcommand_parses` and keep `:4281`'s required-selector assertion green.
- [x] `docs/sync.md` — the verb table and the `--json` envelope rows this story makes false.

**Execution — commit 2, the selection model (copied):**
- [x] `src-tauri/crates/keeper/src/sync_ipc.rs` — `sync_tasks_set_enabled(ids: Vec<TaskBatchIdReq>, enabled)`
  and `sync_tasks_forget(ids: Vec<String>)`, each `-> Result<TaskBatchReceiptVm, IpcError>`, mapping
  `db::TaskBatchOutcome` to the wire kind in a **total match**. **Shell crate — macOS gate.**
- [x] `src-tauri/crates/keeper/src/lib.rs` — register both in `generate_handler!`. **Shell crate.**
- [x] `src/lib/ipc/client.ts` — barrel exports/imports for the four new bindings and the two wrappers,
  each with its `Rejects with:` line.
- [x] `src/lib/ipc/gen/**` — regenerate via the Rust suite and commit what it writes.
- [x] `dev/mock-shell.ts` — answer both verbs with a receipt.
- [x] `src/components/layout/tasks-pane.tsx` — widen 59.1's **one** selection slot to
  `ReadonlySet<string>` + `anchorKey`; port `select(node, "replace" | "toggle" | "extend")` and
  `handleRowClick`'s `metaKey || ctrlKey` before `shiftKey` decoding from `files-pane.tsx`;
  `aria-selected` on every row, `aria-multiselectable="true"` on the container; a `tasksSelectionSentence`
  through `countLabel(count, TASKS)`; header bulk Enable/Disable/Forget rendered **only** when the
  selection is non-empty; the detail region shows a task exactly when the selection resolves to one,
  so single-select is unchanged; the receipt's refusals land in the existing per-id `refusals` map.
- [x] `src/components/layout/tasks-pane.test.tsx` — the frontend tests below; rewrite 59.1's
  `marks exactly one name as current…` into its `aria-selected` twin, still asserting the selection's
  **contents** rather than its size.

**Acceptance Criteria:**
- Given a selection of several tasks in ⌘8, when Enable, Disable or Forget is pressed, then one call
  acts on all of them and the person sees, per id, what happened.
- Given the bulk consumer, when `git log` is read, then `db.rs`/`engine.rs`/the CLI carry it in a
  commit that precedes the commit introducing any selection state.
- Given `spec-43-8…:347-348`'s refusal and the epic's own test, when this spec is read, then it
  records that the refusal is overturned because a bulk consumer now exists — the same way 45.3
  overturned it for Files — and that the selection model was copied, not invented.
- Given the gates, when they run, then Rust ≥ 3831 tests, frontend ≥ 5132, clippy `-D warnings` clean
  on the three crates, `cargo fmt` applied, typecheck clean, lint at 4 warnings + 1 info and 0 errors.

**Test list (each is a mandatory proof):**
1. `db.rs` — **the receipt is the reason this story exists**: a batch of five where two succeed, one's
   baseline has moved, one is unreadable and one is absent returns five entries in request order, and
   the caller can tell which is which; the moved-baseline row is byte-identical afterwards.
2. `db.rs` — N ids produce N entries: no collapsing, no first-error-wins, ordered.
3. `db.rs` — `forget_tasks` refuses an unreadable row rather than deleting it (AD-48), and answers
   `Missing` for an absent id.
4. `engine.rs` — a batch containing one `Rearmed` id and one `Updated` id clears the fault for the
   first and **not** the second, asserted through `notifications_posted` exactly as `:15046` does.
5. `commands.rs` — each id gets its own rendered line (assert the `Vec<String>` the renderer returns),
   and `--json` carries the per-id shape (assert the document's key set with `sorted_keys`, and each
   entry's keys).
6. `commands.rs` — a batch with one refusal exits `EXIT_CONFIG` and still emits exactly one document.
7. `tasks-pane.test.tsx` — plain click replaces, Cmd adds exactly one without filling the gap, Ctrl
   behaves as Cmd, Shift fills the run — asserted through `aria-selected` `"true"`/`"false"` and the
   count sentence, against the **same** semantics `files-pane.test.tsx:2609/2630/2657` assert.
8. `tasks-pane.test.tsx` — a receipt where three of five refused renders three reasons, in Rust's
   words, per id, and does not shrink the selection.
9. `tasks-pane.test.tsx` — `unknown` rows and projected paced rows are not selectable and offer no
   bulk action.
10. **Mutation** — collapse the receipt to a single boolean ⇒ test 1 fails; delete the baseline check
    inside the batch ⇒ the moved-baseline half of test 1 fails. Restore both and verify each restore
    by reading `git diff`.

## Spec Change Log

## Review Triage Log

### 2026-09-01 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 12: (high 2, medium 5, low 5)
- defer: 2: (high 0, medium 2, low 0)
- reject: 1: (high 0, medium 0, low 1)
- addressed_findings:
  - `[high]` `[patch]` `forget_tasks` propagated a mid-batch `delete_task` failure with `?`, so ids
    already deleted lost their receipt and the pane reported a whole-batch failure while rows had
    really gone — the exact partial-success-thrown shape the function's own doc and
    `FilesDeleteReceiptVm:4744-4750` forbid, and asymmetric with `set_tasks_enabled`'s own
    `Err ⇒ Refused` arm. Now that id's `Refused`, with a store-level test (an `ABORT` trigger, no
    mock and no production code added to be failable). Mutation confirmed it bites.
  - `[high]` `[patch]` With an empty selection the detail-region fallback (`tasks[0]`) was leaking
    into `aria-selected`, so the first row announced itself selected inside an
    `aria-multiselectable` listbox while no selection existed and no bulk verb was offered — and
    **its test asserted that wrong answer as correct**, which is why the first mutation sweep stayed
    green over it. `aria-selected` now follows the set alone; the test asserts nothing is selected on
    mount, that the detail region still draws the first task, and that a click then moves the mark by
    contents. Every other selection assertion in the file was audited for the same lean.
  - `[medium]` `[patch]` `report_task_batch` re-introduced throw-after-success: `?` on
    `engine.tasks()` / `list_profiles()` / `task_history()` **after** the writes committed meant a
    failed read-back suppressed the receipt entirely and put a second document on stdout. The
    read-back is now best-effort with a `warn!`, so the exit code stays the one the receipt earned.
  - `[medium]` `[patch]` `docs/sync.md` said a `missing` id is benign and then made it exit 2. The
    behaviour is right (`cmd_verify`'s rule: work that could not run is not work that passed); the
    prose now says `missing` differs from a refusal in *how it is reported* and still counts.
  - `[medium]` `[patch]` The whole-batch error alert and the `TASKS_BULK_NO_REASON_TEXT` fallback —
    the two guards against a bulk failure rendering as nothing — shipped untested. Both now tested,
    including that a rejected bulk Forget keeps the selection its success path empties.
  - `[medium]` `[patch]` The bulk Enable/Disable controls had no in-flight guard, so a double-click
    sent a second call carrying the caller's own pre-bump baselines and every id came back
    `changed elsewhere`. `bulkWriting` now disables all three, cleared after the re-read.
  - `[medium]` `[patch]` `cursorId` could resolve to `null` — two-plus rows selected makes
    `resolvedId` null, and a forgotten anchor row left the listbox with no `tabIndex 0` at all. It now
    falls back through `selection[0]` to `tasks[0]`, with a test that drops the anchor on a refresh.
  - `[low]` `[patch]` The batch's fault-clear keeps its unreachable `TaskSave::Created` arm — kept so
    the policy is `save_task:8366`'s verbatim rather than a second subtly different one — and now says
    it is unreachable and why.
  - `[low]` `[patch]` `dev/mock-shell.ts` had the window semantics **inverted** against
    `db.rs:3316-3329`: it nulled `nextDueMs` on disable and kept it on a re-enable, the opposite of
    what ships, so the dev surface contradicted production. Also `!== null` on the baseline refused a
    well-formed id whose baseline arrived `undefined`.
  - `[low]` `[patch]` `docs/sync.md` listed `created` among `effect`'s values for three verbs that
    can never emit it; it now says only `tasks set` creates a row.
  - `[low]` `[patch]` `set_tasks_enabled`'s doc now records the single-snapshot assumption: every id
    resolves against one pre-batch `list_tasks`, so a duplicated id ignores what the earlier
    occurrence wrote — benign because the payload is the batch's, not the id's.
  - `[low]` `[patch]` `row` on a `saved` entry can be `null` (the re-read could not find it, or could
    not run at all after P2). Both the CLI doc and the wire contract now say so instead of promising a
    row.

Not this story's problem: two entries appended to `deferred-work.md` — `files-pane.test.tsx:3278`'s
load-sensitive `findByRole` timeout (surfaced by this story's gate, in a file it does not touch), and
the `columnFoldStore` module-state leak that every pane suite exercising the fold still carries.
Rejected as noise: the `selected` Set is not pruned of ids the listing has dropped — that is
`files-pane.tsx`'s own copied shape (the `selection` memo absorbs it), and a refusal about a row that
has gone is deliberately still shown, which is what 59.1 widened `orphanRefusals` for.

## Design Notes

### Transaction scope: per-id independence, argued rather than assumed

The epic named this as *the whole design, not an edge case*. Three facts decide it.

1. **`upsert_task` cannot be nested.** It opens its own `conn.unchecked_transaction()` (`db.rs:3248`),
   which rusqlite 0.37 implements as a raw `BEGIN DEFERRED` (`transaction.rs:120-125`); an outer
   `BEGIN` fails at the first inner one. `db.rs:2561-2564` states the assumption in as many words.
   An all-or-nothing batch therefore requires hoisting `upsert_task`'s body behind a
   `&Transaction`-taking inner form — for which this file has **no public precedent**.
2. **All-or-nothing would *revoke* a promise the single-id door already makes.** `upsert_task`'s
   documented guarantee is that a refused write changes nothing — meaning *that row*. Under
   all-or-nothing a refused write would also un-change four unrelated rows, which is a different and
   weaker promise wearing the same words. The epic's warning was not to *silently* choose differently;
   the honest choice is not to choose differently at all.
3. **The house answer already exists and is written down.** `FilesDeleteReceiptVm:4744-4750`:
   *"Partial success is a real outcome and is reported rather than thrown … Each path answers for
   itself."* `cmd_verify:1740-1746` says the same for a loop: *"let one folder's failure be one
   folder's"*, after a `?` that "used to discard every report already computed".

So: **one `with_db` hold for the whole batch** (no other host-local caller interleaves, because the
engine's single connection sits behind that mutex), **N inner transactions**, atomic per id. The
moved-baseline case is then exactly what the receipt is for: four ids are enabled and the fifth says,
in Rust's own sentence, that it was changed elsewhere and should be re-read.

### Why the batch carries a baseline at all, and why `forget` does not

`TaskSaveReq::baseline_updated_ms`'s doc says the baseline is a *request* field "because only the
caller knows whether it holds a reading worth checking". The app's edit form is the one caller that
passes `Some` (`sync_ipc.rs:2386-2388`); the CLI passes `None` because it reads and writes inside one
call. A bulk action from a rendered list is the *first* case, not the second — the person decided
against five rows they were looking at. Taking ids only would make the bulk path silently weaker than
the single-id path it replaces, which is the downgrade the epic warned about. So `TaskBatchId` carries
an `Option<i64>`, the CLI passes `None` per id (unchanged behaviour), and the pane passes each
`TaskVm.updatedMs`.

`forget_tasks` takes plain ids: `delete_task` makes no baseline promise today and inventing one here
would be a new promise, not a preserved one.

### `Missing` is not `Refused`

`delete_task` returns `Ok(())` for an id that never existed and cannot tell "gone now" from "was never
here" (`db.rs:3883-3889`); the CLI compensates by selecting first. The batch keeps that compensation
and makes the answer a **fourth outcome** rather than folding it into a refusal, because the two need
different words on screen: a refusal is something to act on, and `Missing` after another host forgot
the row is usually benign. `validate_id`'s doc draws the same line — *"a spelling this keeper could
never have stored"* versus *"well formed, but no such task"*.

### The `--json` envelope changes, deliberately

`docs/sync.md:2163-2171` documents `tasks enable`/`disable` emitting `{ "task": … }` and
`tasks forget` emitting `{ "forgot": id }`. Both become `{ "results": [ … ] }`, one entry per id, with
the read-back row moved **inside** each successful entry so nothing is lost. The rejected alternative
was to keep the old envelope for one id and the new one for many — a contract whose shape depends on
argument count, which is the receipt-thrown-away failure the assignment names. The human success
lines are unchanged (`Enabled task nightly`, `Forgot task nightly and its run history`); refusals use
the tasks section's own `{id}: {reason}` shape (`task_lines:3509`).

### The selection model, and the two seams with 59.1

Copied from `files-pane.tsx` and named: the three-mode `select`, the precedence gate (`replace` ∥
cross-scope ∥ empty ⇒ one row), Cmd/Ctrl's one-pass `if (!next.delete(k)) next.add(k)`, Shift's
inclusive `slice(low, high + 1)` over the flat visible order with the anchor left **unmoved**, the
`-1` fallback to a single row, `aria-selected` on every row, `aria-multiselectable="true"` on the
container, and the count through `countLabel`. The Files pane's cross-profile replace maps onto Tasks
as **selection is `TaskVm` rows only** — `unknown` rows and projected paced rows are outside it, for
the same reason: half a selection no command can act on is worse than one that visibly reset.

Two things genuinely cannot be reused, and both are named because 59.1 owns them:

- **`aria-current` → `aria-selected`.** 59.1 used `aria-current` on the deliberate ground that
  `aria-selected` announces a set to a reader when only one thing can be chosen (`chat-row.tsx:266` is
  the app's single-selection idiom). That ground is exactly what this story removes. The attribute and
  the refusal flip **together**: while the refusal stood, `aria-current` was right; now that the
  consumer exists, `aria-selected` is. 59.1's test asserting one `aria-current` row is rewritten
  rather than worked around, and keeps asserting the selection's *contents* — "exactly one" and "the
  right one" are different claims.
- **`selectTask`'s single-at-a-time side effects.** It closes the previous task's edit form and bumps
  `historyToken`. Under a set that becomes a question, answered thus: those side effects fire when the
  selection **resolves to one task** — which is every plain click, every ↑/↓, and the moment a
  multi-selection collapses back to one. An additive Cmd-click that grows the set past one does not
  close the form; it hides the detail region, which is where the form lives, so nothing is left seeded
  from a task that is no longer the one on screen. Single-select is therefore byte-for-byte what 59.1
  shipped.

### Read the contract, do not only mutate

The mutation sweep in test 10 proves the tests are load-bearing; it says nothing about whether the
load is right. Each behaviour above was read from the doc comment or chapter paragraph that defines
it before a test was written against it: `upsert_task:3144-3225` for the baseline and the rearm edges,
`save_task:8341-8363` for the fault clear, `cmd_task_forget:4128-4136` for AD-48,
`validate_id:1119-1140` for the two refusal classes, `count-label.ts:29-31` for zero being a number,
and `files-pane.tsx:1656-1668` for what each modifier is *supposed* to mean.

## Verification

**Commands:**
- `GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=keeper GIT_AUTHOR_EMAIL=dev@keeper.local GIT_COMMITTER_NAME=keeper GIT_COMMITTER_EMAIL=dev@keeper.local cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync -p keeper-core -p keeper-syncd` — expected: ≥ 3831 passing, 0 failing. **Measured: 3845.**
- `cargo clippy --manifest-path src-tauri/Cargo.toml -p keeper-sync -p keeper-core -p keeper-syncd --all-targets -- -D warnings` — expected: clean. **Measured: clean.**
- `cargo fmt --manifest-path src-tauri/Cargo.toml -p keeper-sync -p keeper-core -p keeper-syncd -p keeper` — expected: applied, no diff after. **The package list is load-bearing: the bare form fails with "Failed to find targets" because the workspace root has no lib or bin, and `-p keeper` is how the uncompilable shell crate still gets its one local gate.**
- `bun run typecheck` — expected: clean. **Measured: clean.**
- `bun run test` — expected: ≥ 5132 passing, 0 failing. **Measured: 302 files / 5147.**
- `bun run lint` — expected: 4 warnings + 1 info, 0 errors (baseline). **Measured: exactly that, with no new disable comments.**
- `git status --porcelain -- src/lib/ipc/gen` — expected: empty after the regenerated bindings are committed. **Measured: empty.**

**Manual checks:**
- The shell crate (`keeper/src/sync_ipc.rs`, `keeper/src/lib.rs`) cannot compile here. Report
  `sync_tasks_set_enabled`, `sync_tasks_forget` and their `generate_handler!` registration to the
  macOS gate, alongside the `sync_task_schedule_preview` / `TASK_SCHEDULE_PREVIEW_COUNT` debt 59.7
  already owes it.

## Auto Run Result

Status: **done**. Three commits on `feat/59-a-task-you-can-find`, none pushed:
`4864051` → `9bc610f` → `816860f`, from baseline `6606fcb`.

### What was implemented

Story 59.4 **overturns epic 59's own refusal of a Tasks multi-select, on the epic's own test.** The
refusal was conditional and said so: a checkbox column was refused *because every task write in the
stack was single-id*, so a selection would have been state whose only action was a loop of N writes
with N partial-failure stories. `spec-43-8…:347-348` refused a Files selection on the identical
ground and was correctly overturned at story 45.3 **once a bulk consumer existed** — the epic names
that as the test to apply. So the consumer was built first, in its own commit, and only then the
selection model, **copied** from `files-pane.tsx` because `spec-45-17…:200` forbids *inventing* a
second idiom. The ordering is checkable in `git log`, which is one of the acceptance criteria.

**Half one, `4864051` — the bulk consumer.** `db::set_tasks_enabled` / `db::forget_tasks` over ids
with an optional per-id baseline, answering `Saved(TaskSave)` | `Forgotten` | `Missing` |
`Refused(String)` per id, N in and N out in request order. `Err` is reserved for the store failing
outright and is never one id's refusal. Per-id independence rather than all-or-nothing, argued from
three facts rather than assumed: `upsert_task` opens its own `BEGIN DEFERRED` and cannot be nested;
an all-or-nothing batch would silently *revoke* the single-id door's promise that a refused write
changes nothing *that row*; and `FilesDeleteReceiptVm:4744-4750` already records the house answer.
The engine verbs keep every side effect the single-id paths have, per id — the fault clear on
`Created | Rearmed` only, and `forget_task`'s unconditional clear preserved by clearing on
`Forgotten | Missing` too. The CLI's three verbs became variadic, render one line per id, and emit
one `{ "results": [...] }` document with a fixed five-key shape per entry.

**Half two, `9bc610f` — the selection model.** 59.1's one selection slot widened to a set plus an
anchor, never a second slot beside it. `select(id, mode)` is ported branch-for-branch: plain
replaces, Cmd/Ctrl toggles exactly one with the one-pass `if (!next.delete(id)) next.add(id)`, Shift
takes the inclusive run over the flat visible order and leaves the anchor where it was. `aria-current`
became `aria-selected` on every row with `aria-multiselectable` on the container — the attribute and
the refusal flip **together**, which is the invariant 59.1's author asked for and agreed to over
`hub`. Single-select is unchanged because the detail region draws a task exactly when the selection
resolves to one. Refusals land in the existing per-id `refusals` map and surface through the
`orphanRefusals` slot 59.1 had already widened for exactly this.

**`816860f` — the twelve review patches.** Two were high severity and both were the same class: a
partial success reported as a total failure (`forget_tasks` throwing away the receipt for ids it had
already deleted) and a test that asserted a wrong answer as correct (an empty selection announcing
the detail-region fallback row as selected). See the Review Triage Log.

### Files changed

- `src-tauri/crates/keeper-sync/src/db.rs` — the batched statements, the three types, and the
  transaction-scope argument on the functions.
- `src-tauri/crates/keeper-sync/src/engine.rs` — the two engine verbs and the per-id fault bookkeeping.
- `src-tauri/crates/keeper-core/src/tasks.rs` — the four wire types, with the `effect`/`reason`
  exclusivity invariant documented and the shell's total match named as its keeper.
- `src-tauri/crates/keeper-syncd/src/commands.rs` — variadic `Enable`/`Disable`/`Forget`, the per-id
  renderer, the receipt document, and the exit code through `Ok(u8)` so only one document reaches stdout.
- `src-tauri/crates/keeper/src/{sync_ipc.rs,lib.rs}` — the two commands and their registration.
  **Shell crate: never compiled here.**
- `src/lib/ipc/client.ts`, `src/lib/ipc/gen/**` — the wrappers and the four generated bindings.
- `dev/mock-shell.ts` — both verbs answered per id, with the real window semantics.
- `src/components/layout/tasks-pane.tsx` (+ test) — the selection model, the bulk surface, the receipt.
- `docs/sync.md` — the verb table and the `--json` envelope this story deliberately changed.

### Review findings

12 patches applied (2 high, 5 medium, 5 low), 2 deferred, 1 rejected. No intent gaps and no spec
repair loopback: every finding was a localized deviation from an invariant the spec had already
stated, so re-deriving 2 600 verified lines would have been strictly worse than patching forward.

### Verification performed

Rust **3845** passing (baseline 3831, +14), clippy `-D warnings` clean on all three crates,
`cargo fmt` applied, typecheck clean, frontend **302 files / 5147** passing (baseline 5132, +15),
lint exactly at baseline with no new disable comments, `src/lib/ipc/gen` clean.

Four mutations were run and every one bit: collapsing the per-id receipt to a single boolean; deleting
the baseline check inside the batch; making Cmd-click fill the gap instead of adding one; rendering
only the first refusal. Two more were run against the high-severity patches: restoring `forget_tasks`'
`?`, and restoring the `aria-selected` fallback. Every restore was verified by **reading `git diff`**.

The pane was also driven for real, not only in jsdom — the pane's own comment names that blind spot
(*"jsdom performs no layout, so no component test in this file could ever catch a control that had
left the screen"*). Chrome was stood up on this host and pointed at `bun run dev` + `dev/mock-shell.ts`,
and a plain click, a Cmd-click (middle row visibly not taken), a Shift-click (the run filled), a bulk
Disable and a bulk Forget were performed on real pixels. The observed facts: the count badge and the
three controls appear only with a selection, `header.scrollWidth === header.clientWidth` so nothing is
clipped, `sync_tasks_set_enabled` went out with three **distinct** `baselineUpdatedMs` values — one
per row's own `updatedMs` — and an all-`missing` receipt rendered as three separate per-id lines.

### Residual risks

- **The macOS gate owes this story.** `keeper` does not link on this Linux host, so
  `sync_ipc::task_batch_receipt_vm`, `sync_ipc::sync_tasks_set_enabled`, `sync_ipc::sync_tasks_forget`,
  their `generate_handler!` registrations and the widened `keeper_core::tasks` import have had only
  `cargo fmt -p keeper` run against them. They sit alongside 59.7's `sync_task_schedule_preview` /
  `TASK_SCHEDULE_PREVIEW_COUNT` debt.
- **A machine contract changed on purpose.** `tasks enable`/`disable`/`forget` now emit
  `{ "results": [...] }` rather than `{ "task": … }` / `{ "forgot": id }`. Uniform per-id, because an
  envelope whose shape depends on argument count is the receipt-thrown-away failure this story exists
  to close. `docs/sync.md` is updated; any external script reading the old envelope will need the new one.
- **`missing` counts against the CLI exit code.** Deliberate and now documented, but it means
  `tasks disable a b c` returns 2 when `b` was already forgotten by another host.
