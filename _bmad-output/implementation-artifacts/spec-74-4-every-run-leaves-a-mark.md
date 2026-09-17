---
status: review
baseline_revision: 6e5695e
final_revision: ''
---

# Story 74.4 — Every run leaves a mark whose name is enough

<intent-contract>

## Problem

A copy task walks everything, every time. `task_runs` keeps a one-line count (`db.rs:223-231`); the per-file record — path, bytes, sha256, outcome — already exists and is already the right content (`CopyReport`, rendered by `render_copy_log`, `copy.rs:1247`), written into the destination as `keeper-copy-<stamp>.log` and **read by nothing, ever**. So the one fact the next run needs — how far the last one got — is the one fact nobody wrote down.

## Approach

Put that fact in a file **name**, in the ledger folder, so the next run answers "what changed since?" by listing a directory.

## Always

`<ledger>/<task-id>/<year>/run-<mark>-<trigger>-<verdict>-<fp8>.md`, where `mark` is `YYYYMMDDTHHMMSSZ` of the **high-water source mtime the run covered**, and `fp8` is eight hex of a SHA-256 over the task's own configuration. The next copy reads names only: the greatest `ok` mark whose `fp8` matches. Writes are atomic (sibling temp + rename, `SessionManifest::write`'s discipline). The body is `render_copy_log`'s text, moved.

## Block If

A `failed` or `partial` run never advances the mark. A fingerprint mismatch means the mark describes a different job, so none applies and the pass is full.

## Never

Never mark with the wall clock. A file written while the pass was walking would be dated before a clock-based mark and skipped **forever**; the newest mtime actually copied is the only safe mark. Never open a file to find the mark — if the name is not enough, the grammar is wrong. Never let a mark file's name carry a segment `plain_segments` refuses.

</intent-contract>

## Code Map

- `keeper-core/src/task_ledger.rs` (new, pure: no clock, no `std::fs`) — `RunFileName::{render,parse}`, `RunTrigger`, `RunVerdict`, `stamp`/`parse_stamp`, `year_folder`, `config_fingerprint`, `latest_mark`.
- `keeper-sync/src/ledger.rs` (new) — the writer, the year folder, `task.toml`, and the reader that lists the two newest year directories' names.
- `keeper-sync/src/copy.rs` — `CopyOptions::modified_since_ms` (exclusive: the mark is an instant already covered) and the high-water mark on the report; `Identical` counts toward it (the file was covered), `Failed` does not.
- `keeper-core/src/recording.rs` — `SessionManifest::write`, the atomic-write precedent; `archive/recordings.rs`'s `rebuild_from_disk`, the precedent for disk-as-truth with the database as a cache.

## Tasks & Acceptance

Acceptance, verbatim from the epic: *a ledger fixture with three runs across two year folders yields the greatest `ok` mark from names only, with no file opened; a `failed` or `partial` run does not advance it; a changed configuration changes `fp8` and the next pass is full; the mark is the newest mtime **copied**, proved by a fixture whose source is touched mid-pass and picked up by the following run; a torn writer leaves no `.tmp` residue and no half-parsed name; the grammar refuses characters `plain_segments` refuses and round-trips through it.*

## Verification

- `keeper-sync/src/ledger.rs`: the grammar (`RunFileName::{render,parse}`, `RunTrigger`, `RunVerdict`, `stamp`/`parse_stamp`, `year_folder`, `config_fingerprint`, `latest_mark`) and the filesystem half (`TaskLedger::{resolve,latest_mark,write_run,write_config}`, `render_run_header`, `is_run_file`). `stamp`/`parse_stamp` use the crate's own `platform::civil_from_unix_ms` and its new inverse rather than a `chrono` dependency the crate deliberately does not have; `parse_stamp` validates by round-trip, which is what rejects 31 February and a year the arithmetic cannot represent.
- End-to-end in `engine.rs`: `a_second_copy_carries_only_what_changed_since_the_mark` runs one task three times against a real profile with `[folder.tasks]` — first pass full, mark `3000` read back out of the file **name** in the `1970/` year folder (with its trigger and verdict), second pass copying one file (`6 bytes; 1 files`) and leaving the file behind the mark alone, then a destination change making the mark inapplicable and the next pass full again. `a_copy_without_a_ledger_folder_still_runs_and_writes_nothing_extra` pins the no-ledger path.
- The ledger's own suite: the mark read across the two newest year folders (December's success beats January's failure), a missing ledger answering `None`, the atomic write leaving no `.writing` residue and a stray temp not parsing as a mark, the configuration file rewritten only when it changed, and a person's own files in the folder not reading as runs.
- **Mutation proof** (`/tmp/mut74.py`, four mutations, each run alone and reverted, sources restored and compared):
  - exclusive bound → inclusive: `a_mark_bounds_the_work_and_the_report_names_the_next_one` FAILED;
  - `Identical` stops counting as covered: `an_identical_file_advances_the_mark_and_nothing_else_does` FAILED;
  - a failed run allowed to advance the mark: `the_mark_comes_from_names_across_the_two_newest_year_folders` FAILED;
  - the fingerprint no longer gating: `a_mark_from_a_different_configuration_does_not_apply` FAILED.
- The plan's `files_total` excludes behind-the-mark files, so the progress bar's denominator is work it will actually do — found by the mark test, which asserted `(1, 1)` and got `(1, 3)`.
