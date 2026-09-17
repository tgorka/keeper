---
status: review
baseline_revision: 6e5695e
final_revision: ''
---

# Story 74.2 — The copy date window is gone

<intent-contract>

## Problem

Story 72.9 shipped `modified_after_ms`/`modified_before_ms` — a half-open source-mtime window the person types into two date fields — as AD-245. The owner's verdict on his own feature: *"nie wiem czy to ma sens"*. What he wanted from a copy task was never a window; it was a mark of how far the last pass got, so the next one copies what changed and nothing else.

A typed date cannot do that job. It is a guess about the past that the person has to keep updating, it silently skips a file whose mtime the walk could not read (`copy.rs:1021-1024`), and it is the only producer of `CopyOutcome::Skipped`, a wire variant whose existence is now unexplained.

## Approach

One cutover: the fields, the filter, the flags, the inputs, the bindings, the docs rows and the orphaned variant. No shim, no alias, no deprecated path.

## Always

Delete the tests that asserted the window rather than rewriting them to assert nothing.

## Block If

Nothing.

## Never

Never leave a compatibility alias on the wire, and never write a column-drop migration: this crate's schema rule is additive-only, so the two stored columns stay inert and a comment at `ensure_task_columns` says so out loud.

</intent-contract>

## Code Map

The removal map is `agent://ScoutCopyDates` (file:line, ~45 sites, eight sections). The layers:

- `keeper-core/src/tasks.rs` — `TaskVm`, `TaskSaveReq`, their validation and doc lines.
- `keeper-sync/src/copy.rs` — `CopyOptions`' two fields and the plan-time filter in `classify` (`:1015-1038`).
- `keeper-sync/src/db.rs`, `engine.rs` — the row mapping in `perform_copy_task`.
- `crates/keeper/src/sync_ipc.rs` + `src/lib/ipc/gen/{TaskVm,TaskSaveReq}.ts` — the wire, and `CopyOutcome::Skipped` with it.
- `keeper-syncd/src/commands.rs` — the CLI flags, and the `docs/sync.md` word-list guard that pins them.
- `src/components/sync/task-form.tsx`, `dev/mock-shell.ts`, `docs/sync.md` §14.

## Tasks & Acceptance

Acceptance, verbatim from the epic: *no occurrence of either field name anywhere outside a changelog or a ledger entry; the CLI-vs-`docs/sync.md` word-list guard passes; `keeper-sync`'s copy suite is green with the date tests removed rather than rewritten to assert nothing; `bun run check` passes with the form fields gone.*

## Design Notes

`CopyOutcome::Skipped` goes with the window because the window was its only producer. Keeping a variant no code can emit is how a wire type becomes a riddle two epics later.

## Verification

- No occurrence of `modified_after_ms`, `modified_before_ms`, `modifiedAfterMs` or `modifiedBeforeMs` anywhere in `src-tauri/`, `src/`, `dev/` or `docs/` except three deliberate mentions that explain the removal: the inert-column comment in `db.rs`, `CopyOptions::modified_since_ms`'s doc, and `docs/sync.md` §14's paragraph saying the window is gone and why.
- `CopyOutcome::Skipped` went with it, as the spec required: the window was its only producer, and the copy log's summary line lost the `skipped` column with it. A file behind the mark is now absent from the report rather than counted as skipped.
- The stored `tasks` columns stay inert per the crate's additive-only rule, with the comment at `ensure_task_columns` saying so — no drop migration.
- Tests that asserted the window were **deleted**, not re-pointed: `copy_date_window_counts_skips_and_keeps_half_open_boundaries`, the store's "sends the modified-window bounds" test, the pane's "sends the copy card's optional UTC date window" and "keeps bounded-out files named in the settled report", and the CLI's flag rows.
- `cargo clippy -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings` clean; `cargo test` over the three crates green; `bunx tsc --noEmit` and the full vitest suite (5912 tests) green.

## What the macOS job caught that no local gate could

The shell crate does not build on the Linux dev host, so `crates/keeper` is
verified only by CI — and it found four real defects in this epic's shell-side
edits, each on its own round trip:

1. `entry_vm` still matched `CopyOutcome::Skipped`, whose only producer was the
   date window. (The local sweep was a grep, and mine filtered comment lines in
   a way that hid this call site.)
2. Four lines of `///` on a function parameter, which `rustc` rejects outright.
   Nothing local parses that file, so nothing local could say so.
3. `SyncProfile` gained `tasks` and the shell's own guard demanded it be named
   in `EXPRESSED` or `PRESERVED` — the guard doing exactly its job. `PRESERVED`,
   truthfully: no form shows the flag, so no request may express it.
4. The same guard's second half: for every preserved field the fixture's `prior`
   must differ from a fresh profile, or the preservation assertion is vacuous.
   `tasks` was `None` in both, and the guard said so by name.

Recorded because it is the argument for those guards, and because the pattern
generalises: on this host a shell-crate change is verified by inspection, and
inspection misses a match arm, a doc comment and two halves of a guard.
