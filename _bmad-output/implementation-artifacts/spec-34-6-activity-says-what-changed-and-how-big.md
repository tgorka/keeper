---
title: 'Activity says what changed, and how big'
type: 'feature'
created: '2026-07-28'
status: 'review'
baseline_revision: '5c40a22'
---

<intent-contract>

## Intent

**Problem:** The ACTIVITY list in the Sync pane answers "what did sync touch" and nothing else. Two
things make it unreadable. (1) Every row renders a size-less path, because the `activity` table has
no size column — yet the size is measured on the way past: `StabilityGate::is_stable` samples the
file (`stability.rs:66-71`), then `forget`s the entry the moment it returns `Stable` (`:384-386`),
and `save_file_state` replaces the whole `file_state` table with the *still settling* entries only
(`db.rs:641-666`), so by the time `record_commit_activity` runs the number is gone. (2) The four
`ActivityKind`s were drawn as `FilePlus` / `FilePen` / `FileMinus` / `GitMerge` — three variations
on the same page outline at 14 px, which is one grey smudge repeated down the column.

**Approach:** Add a nullable `size_bytes` to the `activity` table through an additive
`ensure_*_column` migration (the convention `db.rs`'s own module doc already names), carry it up
through `ActivityRow` → `SyncActivityVm.sizeBytes`. Capture the number in `collect_stable_changes`,
where the path is still in hand: one `lstat` for each path that is actually about to be staged, and
for a deletion the length of the blob the index still records (an LFS-tracked path answers with the
size written inside its pointer, not the pointer's own ~130 bytes). The number rides to the activity
write on a new `StagedChange.sizes` map. In the UI, give the four kinds four different silhouettes
and render the size beside the existing relative timestamp with the pane's own `formatCopyBytes`.

## Boundaries & Constraints

**Always:** Nullable is the honest type — a row written before this change, and a deletion the index
no longer names, both have no size, and `null` renders as *nothing*. An existing `sync.db` keeps its
capped rows and reads them back with `size_bytes: None`. The migration follows `keeper-core`'s
`ensure_hue_index_column` shape (`PRAGMA table_info` then a conditional `ALTER TABLE`), which
`db.rs:5-6` already declares as this file's convention. Measurement happens *after* the gate mutex
is released — every other profile's scan queues behind that lock.

**Block If:** (none — the epic fixed every open decision this story needed)

**Never:** Do not write `0` for a size nobody measured, and do not render `0 B` or "unknown" for a
`null`. Do not edit the `CREATE TABLE activity` text in place — an existing install would never see
the change and every insert would then fail on a missing column. Do not touch `stability.rs`
(`StabilityVerdict` is compared by equality in a dozen tests in a module this story does not own),
`parse_req`, or any region another Epic 34 story owns.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Fresh install | no `sync.db` | `activity` created, then `ALTER TABLE` adds `size_bytes` — one path, always exercised | — |
| Upgraded install | `activity` with 500 rows and no `size_bytes` | column added; every row survives and reads back `size_bytes: None` | — |
| Migration re-run | column already present | `PRAGMA table_info` sees it; no `ALTER` issued | idempotent |
| Added / modified path | file on disk when the gate passes it | `lstat` size recorded and rendered (`12 bytes`, `2.5 MB`) | vanished in between → no size |
| Deleted path | index still holds the entry | blob length recorded — the size that was removed | entry already staged away → no size |
| Deleted LFS path | index blob is a pointer | the pointer's `size` field, not the pointer's ~130 bytes | unparseable → blob length |
| Conflict copy | copy written beside the canonical path | its on-disk size | `metadata` fails → no size |
| Size above `i64::MAX` | impossible for a file, possible for a corrupt row | stored/read as unknown, never wrapped | `try_from(..).ok()` |
| Row with a negative `size_bytes` | hand-edited/corrupt DB | read back as `None` | tolerated, not fatal |
| Empty file | 0-byte add | renders `0 bytes` — truthful, and distinct from unknown | — |
| Unknown `kind` string | a newer keeper's vocabulary | falls back to `FileIcon` + the raw kind word | already-existing tolerance |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/db.rs` -- `migrate` (the `activity` DDL and the new
  `ensure_activity_size_column`), `ActivityRow.size_bytes`, `record_activity`'s row tuple,
  `list_activity`'s SELECT, and the activity tests.
- `src-tauri/crates/keeper-sync/src/git/commit.rs` -- `StagedChange.sizes`, the map that carries the
  measurements from the scan to the activity write.
- `src-tauri/crates/keeper-sync/src/engine.rs` -- `collect_stable_changes` (measurement, after the
  gate lock is dropped), the new `removed_size` helper, `record_commit_activity`, and the conflict
  rows written by `do_pull`.
- `src-tauri/crates/keeper/src/sync_ipc.rs` -- `SyncActivityVm.size_bytes` and the `sync_activity`
  mapper.
- `src/lib/ipc/gen/SyncActivityVm.ts` -- the ts-rs binding, kept in step with the Rust type.
- `src/components/layout/sync-pane.tsx` -- `ACTIVITY_KINDS` (icons + tone) and `SyncActivityList`.
- `src/components/layout/sync-pane.test.tsx` -- the `SyncPane activity` describe block.

## Tasks & Acceptance

**Execution:**
- [x] `db.rs` -- Add `ensure_activity_size_column` (a `PRAGMA table_info(activity)` read then a
  conditional `ALTER TABLE activity ADD COLUMN size_bytes INTEGER`), called at the end of `migrate`;
  leave the `CREATE TABLE` text alone so a fresh and an upgraded install take the same path. -- An
  existing install keeps its rows.
- [x] `db.rs` -- `ActivityRow.size_bytes: Option<u64>`; `record_activity` takes
  `&[(ActivityKind, String, Option<u64>)]` and inserts the fifth column; `list_activity` selects it
  and maps `NULL`/negative to `None`. -- The size round-trips.
- [x] `git/commit.rs` -- `StagedChange.sizes: BTreeMap<PathBuf, u64>`, absent meaning unknown. --
  One structure carries paths and their sizes to the activity write.
- [x] `engine.rs` -- In `collect_stable_changes`, after `drop(gates)` and the `file_state` write,
  `FileSample::of` each added/modified path and `Self::removed_size` each deleted one. -- The number
  is captured while it is still knowable.
- [x] `engine.rs` -- `removed_size`: index entry → blob header size, and for a blob small enough to
  be a pointer, the parsed pointer's `size`. -- A deleted file reports the size that was removed,
  including an LFS one.
- [x] `engine.rs` -- `record_commit_activity` reads `staged.sizes`; `do_pull`'s conflict rows stat
  the copy they just named. -- All four kinds carry a size when one exists.
- [x] `sync_ipc.rs` + `SyncActivityVm.ts` -- `sizeBytes: number | null` on the VM and its binding.
- [x] `sync-pane.tsx` -- `CirclePlus` / `SquarePen` / `CircleMinus` / `TriangleAlert`, the last in
  `text-destructive`; render `formatCopyBytes(row.sizeBytes)` before the timestamp, and nothing when
  it is `null`. -- Four kinds tellable apart, each with its size.
- [x] Tests -- `db.rs`: a planted pre-34.6 table upgrades in place with its row intact and no size,
  then records sizes; the round-trip test asserts recorded sizes. `engine.rs`: the end-to-end commit
  test asserts 3 / 12 bytes on add, 10 on edit and 12 on the deletion. `sync-pane.test.tsx`: four
  distinct glyph classes, the three sizes rendered, and no size at all on the `null` row.

**Acceptance Criteria:**
- Given a `sync.db` whose `activity` table predates `size_bytes`, when the engine opens it, then
  every existing row is still listed and each reads back with no size.
- Given a fresh install, when a commit adds, edits and removes files, then each activity row carries
  the size that moved.
- Given a deleted file, when its row is written, then the size recorded is the one the index held —
  and `null`, never `0`, when the index no longer holds it.
- Given four rows of different kinds, when the list renders, then each shows a different glyph and
  the conflict row is the only coloured one.
- Given a row with `sizeBytes: null`, when it renders, then no size text appears at all.

## Design Notes

**Why a second `lstat` rather than plumbing the gate's sample out.** `is_stable` already holds a
`FileSample` carrying the size, but returning it means widening `StabilityVerdict::Stable`, which is
compared with `assert_eq!` in roughly a dozen `stability.rs` tests belonging to a module no story in
this batch owns. The measurement instead happens once per path that is genuinely about to be staged
— a file whose every byte is about to be read into the object database, so one extra `lstat` is
noise — and it is taken *after* the gate mutex is released rather than inside the scan loop, so a
large scan cannot make other profiles queue behind it.

**Why the index blob and not the index's stat block for a deletion.** `entry.stat.size` is the
obvious candidate and is already trusted elsewhere in the crate, but git stores it in 32 bits: a
5 GiB file deleted would be reported as 705 MB, which is precisely the kind of confident wrong
number this story exists to avoid. The blob's length is exact, and when the blob is an LFS pointer
the pointer's own `size` field is exact too — so both the ordinary and the large-file case answer
honestly, and everything else answers `None`.

**Why `formatCopyBytes` and not `formatSize`.** `formatSize` (`src/lib/recording-format.ts`) is a
digit-for-digit mirror of the Rust tray's whole-MB convention, and it would render every text file
in the list as "0 MB". `formatCopyBytes` already lives in `sync-pane.tsx`, counts decimal units the
way a file manager does, and mirrors `progress.rs`'s `format_bytes` family closely enough that the
pane never contradicts itself. Its name still says "copy" because renaming it would reach into the
copy card and the test file, which other Epic 34 stories are editing.

**Why only the conflict row is coloured.** Four different silhouettes (circle-plus, square-pen,
circle-minus, triangle) already separate the kinds in greyscale. Colouring all four would turn a
quiet log into a scoreboard; the pane's existing habit is that colour means "this one wants you",
which is true of a conflict copy and of nothing else in the list.

## Verification

**Deliberately not run by this agent:** the build, `cargo test` / `cargo nextest`, `cargo clippy`,
`cargo fmt` and `bun run check` were all left to the parent, per the batch constraint that six
agents are editing this worktree concurrently and the suite runs once at the end. Nothing below
claims to be a green test run.

**What was actually checked, by reading:**
- `keeper-core/src/registry.rs:432-452` and `keeper-core/src/archive/db.rs:249-276` — the
  `PRAGMA table_info` + conditional `ALTER TABLE` migration this change copies; `db.rs:5-6` names it
  as this file's own convention.
- Every caller of the changed signatures was re-greped after the edits: `record_activity` (engine
  conflict path, `record_commit_activity`, five db tests, one engine test), `ActivityRow` literals
  (two db tests, one engine serde test), `StagedChange` literals (four in `commit.rs`, one in
  `repo.rs`, two in `tests/lfs_roundtrip.rs` — all but one already used `..default()`, and that one
  was given it). No construction site remains unmigrated.
- `gix` 0.86 API surface confirmed against the vendored source:
  `Repository::find_header` (`gix-0.86.0/src/repository/object.rs:142`, not feature-gated) returning
  `gix_odb::find::Header` with `pub fn size(&self) -> u64` (`gix-odb-0.83.0/src/find.rs:33`), and
  `index_or_empty()` / `entry_by_path` / `find_object`, which `lfs/stage.rs:189-199` already uses in
  exactly this shape.
- `lucide-react` 1.26: `CirclePlus`, `CircleMinus`, `SquarePen` and `TriangleAlert` are all exported
  (`dist/lucide-react.d.ts`), and `createLucideIcon.mjs:19-23` gives every icon a
  `lucide-<kebab-name>` class — which is what makes the "four distinct glyphs" assertion meaningful
  rather than a tautology.
- The regenerated `SyncActivityVm.ts` was diffed by eye against `SyncPendingVm.ts` (the nearest
  `Option<i64>` + `#[ts(type = "number | null")]` neighbour) and matched byte for byte in shape,
  including the trailing space ts-rs emits before a doc-comment block — `bindings:check` compares
  the generated directory with `git status --porcelain`, so that whitespace is load-bearing.
- Formatter arithmetic was chosen to be exact rather than lucky: the test fixture uses 2 500 000
  (→ `2.5 MB`), 12 (→ `12 bytes`) and 4 000 (→ `4.0 kB`), each of which lands on a dyadic value so
  `formatCopyBytes`'s `Math.floor(scaled * 10)` cannot round the other way.

**Commands for the parent to run:**
- `bun run test:rust` -- expected: the two new/extended `db.rs` activity tests, the extended
  `engine.rs` commit-activity test and the camelCase serde test pass, and ts-rs rewrites
  `SyncActivityVm.ts` to exactly the committed bytes.
- `bun run check` -- expected: biome + tsc + vitest pass, including the extended `SyncPane activity`
  block.
- `bun run bindings:check` -- expected: clean, proving the hand-updated binding matches ts-rs.
