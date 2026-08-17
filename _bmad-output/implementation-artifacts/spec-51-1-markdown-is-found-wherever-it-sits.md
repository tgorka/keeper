# Spec 51.1 — Markdown is found wherever it sits

story: 51.1
status: in-progress
branch: `feat/51-1-markdown-found-wherever-it-sits` (bottom of the epic-51 stack)
binds: FR-285, FR-286; AD-120 (kind is a tag, never a folder), AD-119, AD-113
sentinel: `MUT51-1`

<intent-contract>

**The ask, three items deep.** *"dodaj katalog spaces … i wyswietlaj wszystkie stworzone pliki md w tym
katalogu zamiast w root session"*, *"log files w session umiesc w katalogu log"*, and *"references nie
wyswietla references ze spaces"*. All three are one missing capability: **markdown in a subdirectory
is invisible.**

**Problem.** `read_ref_sources`' flat branch reads ONE level, files only, and returns before the folder
branch runs (`sessions_root.rs:1055-1072`). So a file in `spaces/` or `log/` is in no pool, no space,
no board and not even *Unfiled*. And the folder branch takes `README.md` **by name** and then walks
only `refs/` and `prompts/` (`:1073-1099`) — so the owner's root `references.md`, which pre-50.1
*Add reference* wrote there, is invisible to the References panel AND to every space.

**Approach.** Teach the pool to find markdown where it actually is, and change nothing about what a
file *is*. `kind_dir(Flat, _) → Ok(None)` stays: keeper keeps writing at the root, so there is no
migration and no changed board semantics for existing sessions. The operator may make `spaces/` or
`log/` (story 51.2's verb) and move files in; the tag still decides the kind, which is AD-120 working
as designed rather than being relaxed.

**Always.**
- One walk, budgeted by the existing `REF_SCAN_BUDGET` (`sessions_root.rs:1014`) — a session is not a
  vault index and must not become one.
- `artifacts/`, `workspace/` and dotted directories are skipped, under both shapes. Those exclusions
  are already stated at `sessions_root.rs:870-874` and hold here for the same reasons: one is
  versioned output, one is fenced scratch (AD-113).
- A pool entry's `rel` is its path **relative to the session**, so two files with the same basename in
  two directories are two entries and a space can list both.
- The folder shape gains root markdown; it keeps `README.md` as its record, and the record is not
  duplicated into the pool as an ordinary entry.

**Block if.**
- The walk would exceed the budget → it stops and says so, exactly as the existing scan does. A
  truncated pool must be reported, never silently short.
- A symlinked directory would take the walk outside the session → refused; the existing containment
  check owns that decision.

**Never.**
- Never infer kind from a directory. `pool::read_one` reads tags and nothing else (`pool.rs:251-252`);
  this story does not touch it, and there is a test that a file in `log/` with no `log` tag is unfiled.
- Never let `kind_dir` return a subdirectory for the flat shape. That is the expensive version: it
  costs the board-row log probe (`last_log_flat`, `sessions_root.rs:392`), the migration's output
  (`migrate.rs`), and the flat contract's own premise, and buys only that keeper does the filing.
- Never read `artifacts/` or `workspace/` markdown into the pool. `workspace/` dies with the session.

**I/O and edge-case matrix.** Every row is a test.

| # | input | expected |
|---|---|---|
| 1 | flat session, `spaces/plan.md` tagged `task` | in the pool, `rel = "spaces/plan.md"`, listed by the Tasks space |
| 2 | flat session, `log/2026-08-16-note.md` tagged `log` | in the pool and in the log view |
| 3 | flat session, `spaces/plan.md` with NO tags | in the pool and reported as *Unfiled* — not silently absent |
| 4 | flat session, `workspace/scratch.md` | NOT in the pool, under any tag |
| 5 | flat session, `artifacts/report.md` | NOT in the pool |
| 6 | flat session, `.hidden/x.md` | NOT in the pool |
| 7 | folder session, root `references.md` tagged `ref` | in the pool and listed by References — the owner's orphan becomes visible |
| 8 | folder session, root `references.md` with no tags | in the pool, reported as *Unfiled* |
| 9 | folder session, `README.md` | still the record; not a second pool entry |
| 10 | folder session, `refs/inputs.md` and root `inputs.md` | two entries, distinct `rel`s, both listable |
| 11 | a tree deeper than the budget | the pool is truncated and says so; no panic, no silent short read |
| 12 | 200 files across 12 directories | one walk, within budget, and the perf assertion the scan already makes still holds |

</intent-contract>

## Code Map

| file | change |
|---|---|
| `keeper/src/sessions_root.rs:1019-1099` | `read_ref_sources`: the flat branch recurses (budgeted, skipping the three exclusions); the folder branch additionally takes root `*.md` besides `README.md`. One helper walks for both so the two shapes cannot disagree about exclusions |
| `keeper/src/sessions_root.rs:370-392` | `entry_names` / `last_log_flat` take the same source list, so the board row's newest-log line follows the pool instead of drifting from it |
| `keeper-core/src/sessions/pool.rs` | untouched except tests: the AD-120 guard (row 3, row 8) and that `rel` carries the subdirectory |
| `docs/sessions.md` | the on-disk section: markdown is found in subdirectories; `artifacts/`, `workspace/` and dotted directories are not read; the tag still decides the kind |

## Tasks & Acceptance

- [ ] one budgeted walk, shared by both shapes, with the three exclusions
- [ ] the folder shape reads root markdown without duplicating its record
- [ ] the board-row log probe follows the same sources
- [ ] matrix rows 1–12, including the AD-120 guards and the truncation report
- [ ] `docs/sessions.md`

**Acceptance.** The owner's root `references.md` appears in References; a file he moves into `spaces/`
or `log/` is listed by the space whose tag it carries; and `workspace/` is still invisible.

## Design Notes

_(filled at review)_

## Verification

_(filled at review)_
