# Real-client evidence — hesperia (macOS, keeper.app 0.8.x), captured 2026-09-08

Source: `~/Library/Logs/keeper/keeper.log` (895 MB, 4.35 M lines, 2026-08-09 → 2026-09-08), `~/Library/Application Support/dev.tgorka.keeper/sync.db`, and the synced repos themselves.

## Profiles (3)
| name | path | removable | lfsMode | threshold | lfsPruneLocal | settleMs | pollIntervalMs | notes |
|---|---|---|---|---|---|---|---|---|
| neuradrive | /Volumes/merope/neuradrive (USB APFS) | yes | materialize | 256 KiB | true | 5000 | 15000 | recordings → 70-comms/meetings |
| tgdrive | /Volumes/merope/tgdrive (USB APFS) | yes | materialize | 256 KiB | true | 5000 | 15000 | ~155 626 index entries; lfsNever list of 18 text extensions; recordings → 40-media/recordings |
| tgdrive-light | /Users/tgorka/tgdrive (internal SSD) | no | materialize | 256 KiB | true | 5000 | 15000 | second clone of the SAME remote on the same machine; virtualOverBytes=1 MiB, releaseTtlMs=24h |

Remote for all: Forgejo over HTTPS at `electra (tailnet)` (tailnet).

## Current session (since 2026-09-06 00:06, 4 450 log lines)
- Levels: 1869 ERROR, 1664 INFO, 610 WARN. Almost all ERRORs are matrix-sdk (beeper/electra HTTP), not sync.
- The remote `electra` has been UNREACHABLE since 2026-09-02 (tcp connect timeouts). Journal: `pull|pending` ×3 with 615/549/559 attempts; `lfsUpload|pending` ×2 (one is a 92-byte object `events.sync-conflict…` → 356 attempts; one is 2.0 GB screen recording, 15 attempts); `push|deferred` ×2 ("publishing is on hold until this folder's large files reach the remote (1 outstanding)").
- `sync retrying profile=… error=git object store failure: fetch from electra… tcp connect error: deadline has elapsed` — 53 lines across the 3 profiles in ~3 days.
- **Status walks on tgdrive: 1371 in this session, `scanned=155626` every time**, elapsed_ms min=170 p50=1079 p90=5287 max=60828. In busy hours the cadence is one walk every 1–5 s (e.g. 666 walks in hour 2026-09-08T21, while a screen recording session is writing into `40-media/recordings/...`). Sample line:
  `status walk finished folder="tgdrive" entries=26 scanned=155626 elapsed_ms=8921 added=0 modified=0 deleted=0`
  `entries` alternates 26 / 65 / 0 with added=modified=deleted=0.
- neuradrive: 217 walks, `scanned=77…`.
- `a status walk is already in flight; deferring this pass to the next tick profile=neuradrive` (1).
- `holding the push until this folder's large files reach the remote profile="tgdrive" objects=1` (3).
- `the recordings push did not land; the commits are local and the next pass publishes them profile="tgdrive" trigger=SessionEnd error=publishing is on hold …`
- `sync warning profile="tgdrive-light" 30-work/clients/luxia/import/solr_lexbase/solr/data/index/_a3pj.fdt.gz is empty but should hold 6372792995 bytes — not committing it; restore it from the remote before syncing this folder`
- Anomaly (75 lines, repeated each pass): `files git carries as plain blobs that today's threshold would send to LFS profile="tgdrive" measured=files=600 bytes=1958664573 threshold=262144 expected=none, on a folder whose threshold has never moved consequence=history keeps them as blobs for good`.
- Watchers: `folder watch armed root=… backend="native"` for all three (re-armed after volume re-attach on 09-07).
- `committed profile="tgdrive" commit=3e66470f… files=1 lfs=1` — one commit in 3 days.
- file_state table: 0 rows at sample time. materialized table: 89 289 rows. activity: 1500 rows (last: 2026-09-08 11:10 added `40-media/recordings/2026/2026-09-08 10.01 gsd-20260908/screen-0000…`).

## Older history in the same log (last 2 M lines, 2026-08-27 → 09-06)
- **1 314 669 WARN `gix_attributes::search::attributes: Attribute has non-ascii characters or starts with '-': sha256:c95beec7…` / `…: https://git-lfs.github.com/spec/v1`** on 2026-08-27 (1.04 M) and 08-28 (270 k). gix parsed an LFS *pointer* as a `.gitattributes` file: some `.gitattributes` in the tree (or the root one) was itself stored as an LFS pointer. Two lines per attribute lookup, every path, every walk.
- **588 408 ERROR `gix_worktree_state::checkout::chunk: <path>: collided (AlreadyExists)`** between 2026-08-30T22 and 09-01T06 (453 456 distinct "collided (AlreadyExists)" lines) — the first checkout of `tgdrive-light` into a directory that already held files; gix checkout emitted one ERROR per pre-existing file, and the checkout was apparently re-run many times (the same path appears up to 12×).
- Earlier sessions (memory, 2026-08-27): `Engine::do_push` aborted every pass with `LfsUploadPending` while a repair sweep converted blobs to pointers one per pass → 6-day livelock; 60 280 of 155 662 index entries had no stat data so every walk re-hashed 25.4 GB (fixed since by `WalkPolicy::persist_stats`); 147 orphaned `git status` children carrying keeper's `filter.lfs.process`.
- Log rotation: the current file is 895 MB; a `.1786234-oversized` sibling is 1.33 GB.

## The synced repositories themselves (git config --local, 2026-09-08)
- tgdrive: 155 626 index entries, `.git/index` = 26.4 MB, HEAD 3e66470 (2026-09-08 18:06 "sync(tgdrive): 1 added"). `.gitattributes` 180 lines, keeper-managed `*.ext filter=lfs …` rules.
- tgdrive-light: 155 625 entries, HEAD 756612e (2026-09-01 "sync(tgdrive-light@hesperia): 4 modified") — 7 days behind tgdrive on the same machine because the remote is down (no local peer path).
- neuradrive: 775 entries.
- Config on all three: `core.ignorecase=true core.precomposeunicode=true core.symlinks=true index.sparse=false filter.lfs.{clean,smudge,process}=<keeper.app binary> filter.lfs.required=false lfs.<url>.access=basic`.
- **NOT set anywhere: `core.fsmonitor`, `core.untrackedCache`, `index.version`, `index.threads`, `feature.manyFiles`, `core.splitIndex`.** So keeper's own gix walk and any foreign `git status` both pay the full lstat walk.
- tgdrive object store (2026-09-08): `git count-objects -vH` → loose 403 (1.6 MiB), in-pack 181 210 in 6 packs (1.02 GiB), 476 commits on HEAD, `.git/logs` 224 K, `.git/lfs` 4.8 G holding 23 487 objects (worktree holds the content; `lfsPruneLocal=true` is working). Index v2 (`DIRC` 2), 155 626 entries, 26 MB. `.gitattributes`: 179 `filter=lfs` extension rules; 72 641 index paths resolve to `filter: lfs`. Only ONE `.gitattributes` is tracked and it is not a pointer today. `git` on the Mac is 2.52.0. `gc.auto` unset (default 6700 loose). `GitCli::gc` exists in `git/cli.rs:305` but has NO production caller (only a test at cli.rs:1783); the weekly gc on hesperia is an external launchd job writing `~/Library/Logs/keeper/tgdrive-gc.log` (last: 2026-09-06 "no repository … volume unmounted?").
- Volume: `/Volumes/merope` 920 GiB APFS on USB, 573 GiB used.
- tgdrive `.gitattributes` lines 114–115 still carry two anchored rules `/00-inbox/2026-07-30-gdrive-root/resources/work/ecomundo/…/ecojsLIB/.gitattributes filter=lfs diff=lfs merge=lfs -text` and `…/ecojsLIB2/.gitattributes filter=lfs …` — the residue of the 2026-08-27 incident (a `.gitattributes` converted to an LFS pointer). Nothing in keeper retires them. None of the 18 `lfsNever` extensions overlaps a committed `*.ext filter=lfs` rule today.
