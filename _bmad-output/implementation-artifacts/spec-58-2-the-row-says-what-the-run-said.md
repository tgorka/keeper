---
title: 'Story 58.2: the row says what the run said'
type: 'feature'
created: '2026-08-31'
status: 'in-review'
baseline_revision: 'f8fbb90'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** Every completed task run already records what it did. `perform_sync_task` composes `"{synced} synced, {busy} already syncing, {deferred} waiting, {failed} failed"` (`engine.rs:2418-2420`) and `"{detail}: {reason}"` on failure (`:2421-2423`); `finish_task_run` persists it (`db.rs:3477-3487`); `TaskRunVm.detail` carries it to the frontend on **both** `sync_tasks.lastRun` and `sync_task_history` (`keeper-core/src/tasks.rs:262-263`); the CLI prints it (`commands.rs:3305-3317`). `grep detail src/components/layout/tasks-pane.tsx` finds **nothing**. The row says a run ended; it never says what the run said. This is unreachable data, not missing data.

**Approach:** Render `task.lastRun.detail` on the row as a fifth `Field` in the same `<dl>` as the outcome, spanning the grid so an engine sentence of no fixed length has the width to be read. Absent when there is nothing to report. **Frontend only: no Rust file changes and no `src/lib/ipc/gen/**` changes.**

## Boundaries & Constraints

**Always:**
- Render `detail` verbatim. It is composed in Rust from counts the engine measured; nothing here re-words, re-tallies, truncates or parses it.
- A `detail` of `null` renders as **absence** — the cell is not drawn at all — never as an empty cell, a dash, or a sentence this file invented. Three states reach it and all three are real: `lastRun === null` (never ran), an in-flight run (`claim_task` inserts the row with `detail` NULL, `db.rs:3328-3330`), and a reclaimed lease (`outcome = 'abandoned'` written with **no** detail, `db.rs:3322-3326`, `:3388-3392`).
- The label joins the existing `Last …` family and reads as a column header, not as a sentence.
- The four existing cells and the host block keep their exact wording and behaviour; the never-ran row must still say `never run` **exactly twice** (`tasks-pane.test.tsx:301` asserts the count, and a third copy of one fact is what that assertion protects).
- The CLI's settled column vocabulary is `outcome`, relative time, host, detail (`commands.rs:3299-3320`). This story adds the fourth of those to the row and invents no fifth word.

**Block If:** the change appears to require editing any file under `src-tauri/` or `src/lib/ipc/gen/**`.

**Never:** treat `detail` as an error string (it is written on every completed run, success included); truncate or clamp it (a failure's detail carries the actionable half); parse the counts out of it to render a badge; add a second capture point or ask for stdout — a task run is an in-process `sync_once`/`release_expired` call (`engine.rs:2318-2334`) with no stream to capture; touch `src/components/sync/task-form.tsx`, `dev/mock-shell.ts` or `src-tauri/**` (owned by `Story584`); touch `_bmad-output/planning-artifacts/**` (owned by `Epic58Plan`).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| A completed sync run | `lastRun.detail = "3 synced, 0 already syncing, 0 waiting, 0 failed"`, `outcome = "ok"` | the row shows that exact string beside `Succeeded` | No error expected |
| A failed run | `detail = "0 synced, 0 already syncing, 0 waiting, 1 failed: could not resolve host git.tgorka.dev"` | the whole string, the reason included, uncorrected and unclipped | No error expected |
| In flight | `finishedMs = null`, `outcome = null`, `detail = null` | no report cell; the outcome cell reads `running now` | No error expected |
| Lease reclaimed | `outcome = "abandoned"`, `detail = null` | no report cell; the outcome cell reads `Abandoned by the host that started it` | No error expected |
| Never ran | `lastRun = null` | no report cell; `never run` appears exactly twice on the row | No error expected |
| A newer keeper's run | `outcome = null`, `unknownOutcome = "sublimated"`, `detail = "recorded by keeper 0.9.0"` | the stored spelling **and** its detail, both verbatim | No error expected |

</intent-contract>

## Code Map

- `src/components/layout/tasks-pane.tsx` — the row's `<dl>`, the `Field` helper, and `taskOutcomeText`, which already separates never-ran / in-flight / unknown-spelling / known-outcome and is not changed.
- `src/components/layout/tasks-pane.test.tsx` — the `run()` fixture (its `detail` is already `"no folders to sync"`), the per-column row-contract assertion, and the refusal test that owns the *never run appears twice* count.
- `src-tauri/crates/keeper-core/src/tasks.rs:238-266` — `TaskRunVm`; `detail` at `:262-263`. Read only.
- `src-tauri/crates/keeper-sync/src/engine.rs` — `perform_sync_task` and `perform_release_task` compose the sentence; `finish_task_run` (`db.rs`) persists it with the outcome in one statement. Read only, and cited by symbol: `src-tauri/` is being rewritten by the same wave.
- `src-tauri/crates/keeper-syncd/src/commands.rs` — `task_run_lines`, the settled column set. Read only.

## Tasks & Acceptance

**Execution:**
- [x] `src/components/layout/tasks-pane.tsx` — add `TASK_LAST_REPORT_LABEL`, give `Field` an optional `wide` prop that spans the grid and wraps engine prose, and render the report cell only when the stored detail has content — one label, one derived value, no new state.
- [x] `src/components/layout/tasks-pane.tsx` — extend the module doc comment with what 58.2 added and why an absent or blank detail is silence, so the file states its own rule.
- [x] `src/components/layout/tasks-pane.test.tsx` — assert the I/O matrix: the real sentence on screen, the failure sentence whole, the absence states, the per-row scoping, and the layout claim the `wide` prop makes.

**Acceptance Criteria:**
- Given a task whose newest run recorded a summary, when the pane renders, then that summary's own words are on the row — asserted against the string itself, not against the presence of an element.
- Given a run with no report — absent, empty or whitespace-only — when the pane renders, then the row carries no report cell and no empty one, and the outcome cell alone explains the silence.
- Given two rows where only one has a report, when the pane renders, then the report is on its own row and on no other.
- Given a Run now the engine refuses, when the refusal renders, then the report the row already had is still there.
- Given a mutation that removes the blank guard, when the suite runs, then a test fails.
- No file under `src-tauri/` or `src/lib/ipc/gen/` is modified.

## Design Notes

**Why silence and not a sentence.** The states that arrive with no report already have a cell that names them: `running now`, `Abandoned by the host that started it`, `never run`. A fourth sentence — "nothing recorded" — would be this file inventing a fact next to Rust's, and for the never-ran row it would print `never run` a third time. `SyncActivityList` states the rule for exactly this shape: *"A size nobody measured shows nothing at all: `0 B` would claim the file was empty, and `unknown` is noise on a line already busy answering when"* (`sync-pane.tsx:1334-1336`).

**Blank counts as absent.** `detail` is `TEXT NULL` with no non-empty constraint and `finish_task_run` binds whatever it is handed, so a writer this build never met — the NFR-43 case the pane exists to tolerate — can store `""` or `"   "`. A `!== null` guard renders a LAST REPORT heading over nothing, which is the one shape a reader genuinely would read as a failed read. The guard is on the trimmed value; the untrimmed one is what is drawn.

**Why it spans the grid, and why width alone is not enough.** The other four cells are short in every shape this build writes — a cron expression, a coarse relative time, an outcome label; `detail` has no bound at all, so a git error in a quarter-width column wraps to five lines and pushes the host claim off the fold. Width without wrapping is still broken: `min-w-0` shrinks the grid track but nothing breaks a token like `fatal: unable to access 'https://…/long/path.git/'`, so the cell carries `[overflow-wrap:anywhere]` (the class `sync-pane.tsx:1444` already uses for a git failure) and `whitespace-pre-wrap` (`sync-git-row.tsx:170`) — without the latter, HTML collapses a multi-line git reason and "rendered verbatim" would be false.

## Verification

**Commands:**
- `bun run vitest run src/components/layout/tasks-pane.test.tsx` — expected: all pass, including the 31 pre-existing.
- `bun run typecheck` — expected: clean.
- `bun run lint` — expected: 4 warnings + 1 info (the pre-existing baseline).
- `bun run test` — expected: at or above 301 files / 5002 tests, plus this story's additions.
- `git diff --stat -- src-tauri src/lib/ipc/gen` — expected: empty.

**Manual checks (if no CLI):**
- Mutate the null guard away (render the cell unconditionally) and confirm a null-detail test fails; restore, and verify the restore by reading `git diff` rather than from memory.

## Spec Change Log

### 2026-08-31 — Execution line tightened to match the Always clause
- **Triggering finding:** the Execution task said "render the report cell only when `task.lastRun?.detail` is a string", which literally licenses the empty-string cell the Always clause in `<intent-contract>` already forbade ("never as an empty cell"). The implementation faithfully followed the looser of the two and shipped a `!== null` guard.
- **Amended:** the Execution line now says "only when the stored detail has content"; a Design Note ("Blank counts as absent") records why, and an acceptance criterion names the empty and whitespace-only spellings. No content inside `<intent-contract>` was touched.
- **Known-bad state avoided:** a `LAST REPORT` heading over nothing — the one rendering that genuinely reads as a failed read, which is the opposite of the story's claim.
- **KEEP:** the absence-not-a-sentence decision and its `SyncActivityList` grounding; the per-row scoping; the decision to render the untrimmed string.

## Review Triage Log

### 2026-08-31 — Review pass
- intent_gap: 0
- bad_spec: 1: (high 0, medium 0, low 1)
- patch: 12: (high 0, medium 5, low 7)
- defer: 0
- reject: 3: (high 0, medium 1, low 2)
- addressed_findings:
  - `[medium]` `[patch]` an empty or whitespace-only stored `detail` rendered the labelled empty cell the spec forbids — the guard now tests the trimmed value and draws the untrimmed one, with both spellings asserted.
  - `[low]` `[bad_spec]` the Execution line licensed that empty cell; tightened, and recorded in the Spec Change Log above rather than looped back, because the Always clause was already right and the code fix is one expression.
  - `[medium]` `[patch]` the cell had no overflow-wrap, so the unbreakable git URL the `wide` prop exists for still broke the row — `[overflow-wrap:anywhere]` added, copied from `sync-pane.tsx:1444`, which renders a git failure for the same reason.
  - `[medium]` `[patch]` "rendered verbatim" was false for a multi-line git reason, which HTML collapses — `whitespace-pre-wrap` added (`sync-git-row.tsx:170`), and asserted.
  - `[medium]` `[patch]` the pane's canonical row-contract test enumerated five labels for a row that now says six, so deleting the cell left it green — the label and a real-string assertion added there, and the test renamed.
  - `[medium]` `[patch]` nothing asserted the span or the wrapping, both of which are silent omissions rather than type errors — one class-reading test added, with the reason it is the only one in the file.
  - `[medium]` `[patch]` no test rendered two rows, so a report drawn on the wrong row would have passed everything — a loud-and-silent pair added.
  - `[low]` `[patch]` the row's "a refusal changes nothing else" invariant was never asserted for the fifth cell, because the refusal test pins `lastRun: null` — a refused Run now over a row that has a report added.
  - `[low]` `[patch]` the dt-count test asserted magic totals and carried a comment forbidding a later reader from fixing them, days before Story 58.3 adds to the same row — it now counts cells whose heading is the report label, which states the property and survives.
  - `[low]` `[patch]` the never-ran test duplicated an existing count assertion verbatim; it now asserts only its own half and names the test that owns the count.
  - `[low]` `[patch]` `engine.rs:2421-2423` pointed at a comment about unplugged drives rather than at the failure compose, and `tasks-pane.tsx:482` / `tasks-pane.test.tsx:301` were self-invalidated or off by one — every cross-file citation in the diff is now by symbol or by test name.
  - `[low]` `[patch]` the doc claimed the sync counts were the only report shape; `perform_release_task` composes a different and roughly twice-as-long sentence for a first-class task kind — both arms are now named.
  - `[low]` `[patch]` the `wide` justification asserted the other four cells are fixed vocabularies, which NFR-43 makes false for the outcome and schedule cells — narrowed to what is true and sufficient.
- Rejected: a `max-h-64` scroll region for an extreme-length report (the row already renders `host.sentence` and the refusal unclamped, and a nested scroll inside the pane's `ScrollArea` is worse than a tall row); a guard keying the cell on `finishedMs` (unreachable — `finish_task_run` writes `finished_ms`, `outcome` and `detail` in one statement and nothing else writes `detail`, so the guard could only suppress a report Rust recorded); "Run now removes the cell until a manual Refresh" (the pane deliberately does not poll, and `run_task_now` runs the task synchronously, so its own re-read sees the finished run).

## Auto Run Result

Status: done

**What was implemented.** The Tasks row now says what the run said. `TaskRunVm.detail` had been written on every completed run, typed, served over IPC, wrapped and mocked for a whole wave, and `tasks-pane.tsx` had zero occurrences of it; the row said a run had ended and never what it reported. It is now a fifth `Field` in the row's `<dl>`, spanning the grid and wrapping the way this app wraps engine prose, drawn only when the stored detail has content. **No Rust file and no file under `src/lib/ipc/gen/` was touched.**

**Files changed**
- `src/components/layout/tasks-pane.tsx` — `TASK_LAST_REPORT_LABEL`; a `wide` prop on `Field` that spans the grid and adds `whitespace-pre-wrap [overflow-wrap:anywhere]`; a `report` value that treats blank as absent; the module doc's 58.2 paragraph.
- `src/components/layout/tasks-pane.test.tsx` — the row-contract test extended to six labels and renamed, plus an eleven-test block for the story's one property.

**Review findings:** 12 patches applied, 1 spec amendment recorded without a loopback, 0 deferred, 3 rejected, 0 intent gaps.

**Verification performed**
- `bun run vitest run src/components/layout/tasks-pane.test.tsx` — **42 passed** (31 before this story; +11).
- `bun run typecheck` — no diagnostic in any file this story touched. Seventeen errors exist in the worktree, all in `dev/mock-shell.ts` and `src/components/sync/task-form.tsx`, which a sibling agent is mid-edit on while its `TaskVm`/`TaskSaveReq` regeneration is still pending.
- `bunx biome check` on all four frontend files this session touched — clean.
- `git diff --stat -- src/lib/ipc/gen` — empty. Every `src-tauri/` change in the worktree belongs to the sibling story.
- Mutation proof: dropping `.trim() !== ""` from the report guard failed *"stays silent for a stored report that is blank rather than absent"* (1 failed / 41 passed); restored from a copy taken before the mutation, and the restore verified by reading `git diff` for the guard line rather than from memory. 42 pass again.
- No visual verification: the `keeper` shell crate does not link on this Linux host. `dev/mock-shell.ts` already carries four run fixtures covering a clean summary, a release tally, a failure with its reason, an in-flight run with no detail and a newer keeper's spelling, so the dev shell exercises every branch of this cell with no fixture work.

**Residual risks**
- The report's length is unbounded by construction and this story deliberately does not clamp it, so a pathological engine message makes one row tall. Judged the lesser evil against a nested scroll region or a truncated reason.
- The layout claim is asserted by reading two class names, because jsdom performs no layout. A Tailwind class rename would pass the assertion and break the rendering.
