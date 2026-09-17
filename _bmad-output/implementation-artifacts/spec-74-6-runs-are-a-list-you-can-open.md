---
status: in-progress
baseline_revision: 6e5695e
final_revision: ''
---

# Story 74.6 — Runs are a list you can open, and configuration is visible even when it is a constant

<intent-contract>

## Problem

The owner asked for three lists: tasks, then that task's runs, then a run's detail — "the same as in notes". Two thirds exist: the runs disclosure shipped with story 58.3 (`tasks-pane.tsx:1178-1316`), but its rows are not clickable (`:1255-1260`) and no run detail exists anywhere.

He also asked to see a kind's configuration **even when it is hardcoded**, read-only in that case. The paced section already renders with zero controls and no mirrored constants — `syncPacedWork` injects `SWEEP_EVERY_MS` from the engine (`sync_ipc.rs:2234-2282`) — but it shows a cadence sentence, not a configuration.

The layout constrains the answer: Tasks already spends four columns, the floors sum to 928 px and ~1 470 px is in use at the owner's 1568 px. A fifth fixed column does not fit, and this epic moves no floor.

## Approach

Make a run a panel target — the drill-down keeper already has for `note | file | recording | task` — and give a projected row a configuration block instead of a control.

## Always

A run row opens a panel, with `Panel.replaced` semantics its siblings have (`panels.ts:270-295`). One view model for every kind's runs. A bot run's detail lists its `bot_audit` rows and a sync run's its `activity` rows — surfaced, never duplicated. A paced row shows every cadence value it is governed by, with the values injected by the shell.

## Block If

No ledger folder is configured: the list still renders from `task_runs` and says where a file ledger would live.

## Never

Never a control on a row nobody can drive — AD-141's gate ("a projection never migrates; no schedule editor, no Run now on the projected class") and AD-142 ("a read-only projection registers no scheduler") hold exactly; this story adds facts to read. Never a fifth fixed column, and never mirror a cadence constant into TypeScript.

</intent-contract>

## Code Map

- `src/components/layout/tasks-pane.tsx:1178-1316` — `TaskRunList`; `:834-941,1809-1932` — the paced section.
- `src/lib/stores/panels.ts:270-295` — `Panel`, `setActiveTarget`, `openPanel`, `sameTarget`'s kind list, which gains `run`.
- `src/components/notes/notes-pane.tsx:460-490,530-660` — the `openRow`/`openRowBeside` gesture pair and the composition to copy.
- `crates/keeper/src/sync_ipc.rs:2367-2412` — `sync_task_history`, `sync_task_run_now`; `:2234-2282` — `syncPacedWork` (shell crate: macOS CI).
- `keeper-core/src/bots/audit.rs`, `keeper-sync/src/db.rs:3998-4345` — the audit rows and the `activity` table, the existing halves of "save the logs". `journal` is **not** one: it is a work queue that DELETEs on completion.

## Tasks & Acceptance

Acceptance, verbatim from the epic: *a run row opens a panel whose target survives a pane switch and a reload; the paced section still renders **zero** controls (a test asserts no button, no switch, no schedule field on a projected row) while showing every cadence value; a bot run's detail lists its audit rows and a sync run's lists its activity rows, both read-only; with no ledger folder configured the runs list still renders from `task_runs` and says where a file ledger would live.*
