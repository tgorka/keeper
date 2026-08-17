# Spec 52.1 — The record is called README

story: 52.1
status: in-progress
branch: `work/epic-52-readme-record`
baseline_revision: c873fa6
final_revision: ''
binds: FR-300, FR-301; AD-120, AD-111 (one journaled plan), AD-65
sentinel: `MUT52-1`

<intent-contract>

**The ask, verbatim.** *"w sessions wciaz jest about.md zamiast README.md"* — asked
for the second time, after story 51.8 refused it.

**Why it was refused, and why that refusal is now spent.** `shape.rs:85-91` is
`if has(AGENTS) || has(ABOUT) { Flat } else { Folder }`. Adding `README` to that
`||` flips every folder-shaped session on disk to Flat on the next rescan — which
is why 51.8 was blocked rather than done. The safe direction is the opposite one:
**narrow** the predicate to `has(AGENTS)` alone, and move the file.

**Always**
- A flat session's record is `README.md`. One name, no branch: the
  `if flat { ABOUT } else { README }` pairs at `sessions_root.rs:469`, `:679`,
  `sessions_ipc.rs:907-911`, `plan.rs:235-239` and `session-detail.tsx:349`
  collapse to a constant.
- A migration is ONE plan through the existing journaled executor
  (`sessions_exec::run`), replayable and crash-safe (AD-111).
- Bytes travel verbatim. A user who edited `about.md` by hand keeps every byte,
  every frontmatter key, the `id`, the `pinned` flag and the `keeper:` lineage
  map, because the plan MOVES the file and never recomposes it.
- A hand-built flat session that has `about.md` and no `AGENTS.md` gets
  `AGENTS.md` WRITTEN before the move. Without that step it silently reads as
  folder-shaped afterwards.
- A half-migrated session (`AGENTS.md` + `about.md` + a README signpost, the
  shape `migrate.rs:222-226` leaves) trashes the signpost first, then moves.
  Recoverable in `.keeper/trash/`, never unlinked.
- Prose pointers are rewritten ZONE-WIDE via `refs::rewrite_pointers`, because a
  continuation link crosses sessions.
- The record stays undeletable: `check_deletable` (`files.rs:344-349`) switches
  from `AGENTS || ABOUT` to `AGENTS || README`.
- `RECORD_NAMES` keeps `ABOUT` for as long as an unmigrated `about.md` can exist,
  so a rename of it still refuses rather than half-working.

**Block if**
- A move would overwrite an existing `README.md`. Refuse with the collision
  sentence and name both paths; never clobber.
- The zone is not writable, or a session is mid-write.

**Never**
- Never add `README` to `shape()`'s disjunction.
- Never infer shape from anything stored. `docs/sessions.md:112` — the files are
  the truth, and `sessions_root.rs:468` recomputes it per scan.
- Never rename by copy-then-delete. One `MoveFile`, so a crash cannot lose it.

</intent-contract>

## Code Map

| where | change |
|---|---|
| `keeper-core/src/sessions/shape.rs:31,85-91,74-75` | drop `ABOUT` from the predicate; rewrite the doc that promised the fallback |
| `keeper-core/src/sessions/files.rs:56,344-349` | `check_deletable` → `AGENTS \|\| README`; `RECORD_NAMES` unchanged for now, with the reason in a comment |
| `keeper-core/src/sessions/migrate.rs` | a new `compile_record_rename` beside `compile_migrate`: optional `WriteFile AGENTS.md`, optional trash of the signpost, one `MoveFile about.md → README.md`, then the pointer rewrite |
| `keeper-core/src/sessions/template.rs:284-285,357-358,29-32` | seeds write `README.md`; the "why no README" paragraph is replaced by why there is one |
| `keeper-core/src/sessions/plan.rs:114-124,204-239` | `record_name` parameter collapses to the constant |
| `keeper/src/sessions_root.rs:458-470,660-682` | one record name; keep the pin warning at `:458-464` intact |
| `keeper/src/sessions_ipc.rs:902-912,1143-1145,3209-3213,3344-3348` | the flat/folder record twins collapse; a new `sessions_record_migrate` command |
| `src/components/sessions/session-detail.tsx:104-111,349-366` | one label, one path |
| `src/components/sessions/sessions-pane.tsx:152-157`, `src/hooks/use-sessions-shortcut.ts:47-49` | the two latent no-shape-branch composers — fixed here, on their own merits |

## Tasks & Acceptance

| # | acceptance |
|---|---|
| 1 | `shape(["AGENTS.md", "README.md"])` is Flat; `shape(["about.md"])` is now Folder, and the test that asserted otherwise is inverted with a comment naming this story |
| 2 | migrating a keeper-created flat session moves `about.md` → `README.md` and nothing else; a byte-compare of the file before/after is equal |
| 3 | migrating a hand-built `about.md`-only session writes `AGENTS.md` first, and the session still reads Flat afterwards |
| 4 | migrating a half-migrated session trashes the signpost and the trash holds it |
| 5 | a folder-shaped session is untouched: the compiler emits an empty plan |
| 6 | a migration that would overwrite an existing `README.md` refuses, naming both paths |
| 7 | a `[[about]]`-style pointer in ANOTHER session is rewritten by the zone-wide pass |
| 8 | `sessions-pane.tsx` and `use-sessions-shortcut.ts` open the record that exists for BOTH shapes (a flat session no longer lands on the missing-file sentence) |
| 9 | every test asserting the old names is inverted, not deleted, and each says why in one line |

## Design Notes

_(filled at review)_

## Verification

_(filled at review)_
