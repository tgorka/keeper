---
title: 'Story 70.8: the record and the gates'
type: 'docs'
created: '2026-09-09'
status: 'done'
baseline_revision: 'd42a9e6'
final_revision: '766a11f'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
---

binds: AD-236; NFR-23 (re-authored), NFR-61, NFR-62 (allocated); D-18
depends on: 70.1–70.7 (everything this story records)

<intent-contract>

## Intent

**Problem:** The review of 2026-09-08 found the sync documentation asserting things the code does not do (a linked worktree, a verified Forgejo round trip, one HTTP client, a `gc` that runs) and requirements below the machine they run on (NFR-23 at 100 000 files / 50 GB against a 155 626-entry, ~450 GB folder), with no requirement at all on how often the tree is walked and no record of the decision to store the tree in git.

**Approach:** Say what runs. Re-author NFR-23; add NFR-61 (cadence) and NFR-62 (silence); record D-18 (the store is git, and what that costs — with the assumptions and the revisit triggers); correct `docs/sync.md` §3, §7, §11, §18, §19, §20; give `docs/performance.md` a row per sync NFR; annotate the four ledger rows the field reproduced and open five for what the epic leaves out; add the sprint rows 56-16/56-17 that shipped without one; run every gate on Linux and on hesperia; install; measure.

**Always:** a doc sentence about behaviour names the code that does it or the test that proves it; a number in §19 is one that was read off a machine, with the machine named.

**Never:** claim a verification that has no artefact (the reason §18 was wrong); leave a finding unrecorded because it is out of scope.

</intent-contract>

## Code Map

| Path | Change |
|---|---|
| `docs/sync.md` §3 | the order a pass actually runs (commit first, fetch on its own clock, a merge that cannot finish is undone), profiles ticked concurrently |
| `docs/sync.md` §7 | a lane is `git switch -c` in the profile's own tree, not a linked worktree |
| `docs/sync.md` §11 | which legs `keeper_sync::http` covers; the fetch and `git` deadlines; the `Offline` transition |
| `docs/sync.md` §18 | only what is proven in the repository; story 31-4 named as open; `Held`; the `gc` kind |
| `docs/sync.md` §19 | the reference folder, the walk's cost model, the cadence before/after, NFR-61; fsmonitor/untrackedCache/v4 are inert and undone |
| `docs/sync.md` §20 | item 6 rewritten (gc runs; history is not rewritten); items 8–11 added |
| `docs/performance.md` | rows for NFR-23, 24, 25, 26, 61, 62 with their real enforcement points |
| `docs/decisions.md` | D-18 |
| `_bmad-output/planning-artifacts/epics.md` | NFR-23 re-authored; NFR-61, NFR-62 |
| `_bmad-output/implementation-artifacts/deferred-work.md` | DW-134, DW-137, DW-146 annotated; DW-247…DW-251 opened |
| `_bmad-output/implementation-artifacts/sprint-status.yaml` | 56-16, 56-17 rows; epic 70 rows to `review` |

## Verification

### Gates on Linux (this box, `epic70/work` at `d42a9e6` + the narrowing fix)

- `cargo fmt --all --check` clean.
- `cargo clippy -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings` clean.
- `cargo test -p keeper-sync -p keeper-syncd` → **1584 passed, 0 failed** (1218 lib + 220 integration incl. the new `conflict_matrix` (15) and `lfs_control_files` (5), 157 syncd).
- `bun run lint` (4 pre-existing warnings, none in touched files), `bun run typecheck` clean, `bun run test` → **340 files, 5783 passed**.
- `bun run check:core-tauri-free`, `check:core-sync-free`, `check:syncd-lean` pass.

### Gate on hesperia (`bun run check:rust:macos`)

- First run: keeper-sync lib **1208 passed, 2 failed** — both narrowing tests, `scanned=10000`. Root cause read from gitoxide's source: with `core.ignoreCase` gix adds the `icase` magic to every pathspec (`gix/src/status/index_worktree.rs:191-195`) and `gix-pathspec`'s common prefix for an `icase` pattern is the empty prefix directory (`gix-pathspec/src/search/init.rs:50-64`), so the index range is not narrowed on APFS; entries are still rejected by a string match before any `lstat` (`gix-status/src/index_as_worktree/function.rs:288-303`, counted at `:200` either way). Tests now assert what each platform can promise (`git::repo::walks_case_insensitively`); `WalkPolicy::include` and §19 record it.
- Second run: fmt, clippy `--workspace -D warnings`, keeper-sync lib green; the `keeper` shell crate reported **456 passed, 1 failed** — `voice_ipc::tests::the_button_the_tray_and_the_hotkey_each_stop_the_voice_once`, a file this epic does not touch (`git diff 4f4c2f2..HEAD -- src-tauri/crates/keeper/src/voice_ipc.rs` is empty). Re-run in isolation 3/3 green and the whole `cargo test -p keeper` re-run **457 passed, 0 failed** — a pre-existing parallel-run flake in the voice tests, not this epic's.
- Generated bindings: no drift (`src/lib/ipc/gen` identical after the remote test run).

### Install (`bun run install:macos`) and the field, hesperia, 2026-09-09

- `installed and running: /Applications/keeper.app`; `sync supervisor started` at 09:58:53Z.
- **Walks.** The hour before the install: **525** `status walk finished` lines on `tgdrive`, `scanned=155626` each, no recording running. The first ten minutes after: **10** walks across all three profiles (tgdrive 7, neuradrive 3), i.e. ≈ 60/hour against NFR-61's ≤ 60 — and those are the paced backstop plus the watcher's wakes for the four `events.log` files the meetings folder rewrites (`included=4`, `included=65`). `scanned=155626` on every tgdrive walk is the APFS case explained above: visited, matched, not stat-ed. The measurement under a live recording (the 666/hour case) is owed to the next session that records; the mechanism it depended on (`path_durability`'s walk) is gone.
- **Offline.** The remote came back before the install (the journal drained at 00:56 local: the 2 GB upload, the three pulls and both deferred pushes are gone; `journal` is empty), so the `Offline` transition was exercised only by the tests (`a_remote_that_accepts_and_never_answers_is_offline_within_the_deadline`, `three_failed_units_through_the_tick_reach_needs_attention`), not in the field.
- **Store maintenance.** `tasks` holds `gc-<profile>` × 3, `every 7d`, `scheduled`; `sqlite_master` lists `journal_dedup` and `journal_kind`; `index.skipHash=true` is in `/Volumes/merope/tgdrive/.git/config`.
- **Log.** Rotated on first write: `keeper.log` 24 KB, `keeper.log.1` 897 MB. The gitoxide floods are filtered.
- **Control-file rules.** `.gitattributes` still carries the two anchored `…/.gitattributes filter=lfs` lines — retirement lands with the next commit `prepare` stages on that folder (nothing has changed in `tgdrive` since the install); the test `a_stale_anchored_rule_for_a_control_file_is_retired_and_the_file_is_committed_as_text` proves the path.

### Owed

- The recording-window walk count on hesperia (NFR-61's hardest case) — read it from `status walk finished` with `caller=` the next time a session records.
- The rule retirement on `tgdrive` — confirm with `grep -c 'gitattributes filter=lfs' /Volumes/merope/tgdrive/.gitattributes` after its next commit.
- DW-247…DW-251.
