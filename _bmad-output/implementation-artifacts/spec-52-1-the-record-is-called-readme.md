# Spec 52.1 — The record is called README

story: 52.1
status: review
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
- A hand-built session with an `about.md` and a `README.md` **somebody else
  wrote** (no `AGENTS.md`) trashes that README under its own key and then moves.
  Corrected at review: this shape read as Flat before the predicate narrowed, so
  its record is the `about.md` and that `README.md` was never a record — and with
  both verbs declining it and `check_deletable`/`RECORD_NAMES` refusing to clear
  the way, the operator's only exit was Finder.
- Prose pointers are rewritten ZONE-WIDE via `refs::rewrite_pointers`, in two
  spellings: the bare `about.md`/`[[about]]` beside the file that holds it, and
  the record's path **from the drive root**, which is the only spelling by which a
  pointer in another session resolves here at all (`refs::candidates`' third
  probe). Corrected at review: the bare form is what a zone-wide pass could
  already reach, so "a continuation link crosses sessions" justified a scope the
  pass did not use.
- The record stays undeletable: `check_deletable` (`files.rs:344-349`) goes from
  `AGENTS || ABOUT` to `AGENTS || README || ABOUT`. Corrected at review: dropping
  `ABOUT` put a delete button on the only carrier of an unmigrated session's
  identity, pins and lineage, months before anything could have moved it.
- `RECORD_NAMES` keeps `ABOUT` for as long as an unmigrated `about.md` can exist,
  and is the one list both the rename and the delete rule ask.

**Block if**
- A move would overwrite a `README.md` in a session that HAS its `AGENTS.md`.
  Refuse with the collision sentence, name both paths, never clobber — and say
  the true reason (keeper is already reading that README as the record) with a
  remedy keeper's own verbs do not forbid. One refusal skips one row of a
  zone-wide sweep; it never ends the sweep.
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
| `keeper-core/src/sessions/files.rs:56,344-349` | `check_deletable` → `RECORD_NAMES`, i.e. `AGENTS \|\| README \|\| ABOUT` (corrected at review: `ABOUT` must not leave the delete rule before it can leave `RECORD_NAMES`); one list asked by both rules |
| `keeper-core/src/sessions/migrate.rs` | a new `compile_record_rename` beside `compile_migrate`: optional `WriteFile AGENTS.md`, optional trash of the signpost, one `MoveFile about.md → README.md`, then the pointer rewrite |
| `keeper-core/src/sessions/template.rs:284-285,357-358,29-32` | seeds write `README.md`; the "why no README" paragraph is replaced by why there is one |
| `keeper-core/src/sessions/plan.rs:114-124,204-239` | `compile_create_from_shaped` keeps its `record_name` parameter — corrected at review: the name is the file the guarded lineage append is written to, and it is `about.md` on every unmigrated source |
| `keeper/src/sessions_root.rs:458-470,660-682` | one record name; keep the pin warning at `:458-464` intact |
| `keeper/src/sessions_ipc.rs:902-912,1143-1145,3209-3213,3344-3348` | the flat/folder record twins collapse; a new `sessions_record_migrate` command |
| `src/components/sessions/session-detail.tsx:104-111,349-366` | one label, one path |
| `src/components/sessions/sessions-pane.tsx:152-157`, `src/hooks/use-sessions-shortcut.ts:47-49` | the two composers read the one constant instead of retyping the same literal. **Not a fix**: the string is identical, so behaviour is unchanged — what makes these targets right for an older session is the migration having run |

## Tasks & Acceptance

| # | acceptance |
|---|---|
| 1 | `shape(["AGENTS.md", "README.md"])` is Flat; `shape(["about.md"])` is now Folder, and the test that asserted otherwise is inverted with a comment naming this story |
| 2 | migrating a keeper-created flat session moves `about.md` → `README.md` and nothing else; a byte-compare of the file before/after is equal |
| 3 | migrating a hand-built `about.md`-only session writes `AGENTS.md` first, and the session still reads Flat afterwards |
| 4 | migrating a half-migrated session trashes the signpost and the trash holds it |
| 5 | a folder-shaped session is untouched: the compiler emits an empty plan |
| 6 | a migration that would overwrite a `README.md` in a session that has its `AGENTS.md` refuses, naming both paths, giving the true reason and a remedy keeper does not itself forbid |
| 7 | a `[[about]]`-style pointer in ANOTHER session is rewritten by the zone-wide pass |
| 8 | `sessions-pane.tsx` and `use-sessions-shortcut.ts` compose the record's path from the one constant, asserted by reading the panel target each sets (`sessions-pane.test.tsx`, `use-sessions-shortcut.test.ts`). The story's original claim — that this "fixed" a flat session landing on the missing-file sentence — is **struck**: `SESSION_RECORD_NAME` is byte-identical to the literal it replaced, so nothing about what a press opens changed. A session written before this story opens its record once `sessions_record_migrate` has moved it |
| 9 | every test asserting the old names is inverted, not deleted, and each says why in one line |
| 10 | the migration is **reachable**: the session detail offers it where an `about.md` is still at the session's root, and pressing it calls `sessionsRecordMigrate(rootId)` with no session id, so one press sweeps the zone (`session-detail.test.tsx`) |
| 11 | continuing an unmigrated flat session appends `continued-by` to the record that EXISTS: `compile_create_from_shaped` writes to the name it was given, and `sessions_ipc` reads that name with `migrate::record_at` — which picks by the `id` in the frontmatter, so the half-migrated shape's signpost is never written to |
| 12 | a `{about.md, README.md}` session with no `AGENTS.md` migrates: the foreign README is trashed under `<session>-foreign-readme`, the record lands at `README.md` byte for byte, and the session reads Flat |
| 13 | a zone-wide sweep that meets a refusal keeps going: the answer carries `moved` and one `skipped` row per declined session, and the detail renders both |
| 14 | a pointer spelled from the drive root (`60-sessions/active/…/about.md`) is rewritten — in a link, a wikilink stem and a promote cell — including inside a session that still holds its own unmigrated `about.md` |

## Design Notes

_(filled at review)_

## Verification

_(filled at review)_
