# Lane: scan / walk cost, cadence, and git accelerators

## Scope read

Keeper:
- `src-tauri/crates/keeper-sync/src/git/repo.rs` — 999–1203 (`status_paths`, `status_paths_reported`, unreadable memo, `ScannedEntries`), 1203–1423 (`StatusWatchdog`), 1469–1603 (`finish_walk`, `STATUS_THREAD_LIMIT`, `push_item`), 1599–1953 (`WalkPolicy`, `status_paths_excluding`, `persist_observed_stats`), 2099–2333 (`checkout_is_unfinished`, `restore_missing_checkout`), 660–830 (`enforce_local_config_with_filter`), 2948–2952 (`tracked_paths`)
- `src-tauri/crates/keeper-sync/src/engine.rs` — 154–158, 423–452 (`UNTRACKED_SWEEP_INTERVAL`, `POLL_WALK_MIN_INTERVAL`), 1109–1110, 1207–1239, 1379–1563 (`claim_walk`, `poll_walk_policy`, `commit_walk_policy`), 3529–3683 (`scan_is_due`, `sweep_scratch_if_due`, `report_blobs_over_threshold`), 3939–4033 (`fold_watch_events`), 4150–4168, 4269–4403 (`prime_worktree_changes`), 4899–5063 (`path_durability`), 7099–7203 (`collect_stable_changes`), 12240–12260 (`poll_may_walk`), 12329–12563 (`pending`)
- `src-tauri/crates/keeper-sync/src/footprint.rs` 139–263; `src/lfs/stage.rs` 59–200, 899–1003; `src/anomaly.rs` (whole); `src/exclude.rs` 1–123; `src/virtual_policy.rs` 615–631
- `src-tauri/crates/keeper/src/ipc.rs` 6989–7183, 7290–7293; `src/debug_log.rs` 125–133
- `src/hooks/use-recording-session.ts` 165–187
- `docs/sync.md` §18–§20 (2934–3060); `docs/performance.md` 1–145

gitoxide (`~/.cargo/git/checkouts/gitoxide-093af49ba645df05/0ae3023`, branch `keeper/gix-filter-0.33-reap`, HEAD `0ae30235`; `gix 0.86.0`, `gix-index 0.54.0`, `gix-status 0.33.0`, `gix-dir 0.28.0`, `gix-worktree 0.55.0`, `gix-filter 0.33.0`, `gix-attributes 0.34.0`):
- `gix-status/src/index_as_worktree/function.rs` 59–223, 259–333, 365–533, 601–636
- `gix-index/src/write.rs` 1–168; `src/access/mod.rs` 76–470; `src/decode/entries.rs` 10–130, `src/decode/header.rs` 38–41; `src/extension/{decode,mod,fs_monitor,untracked_cache}.rs`; `src/entry/flags.rs` 24–35; `src/file/init.rs` 60–101
- `gix/src/status/{mod.rs 98–115, platform.rs 95–140, index_worktree.rs 113–190, iter/mod.rs 29–143, iter/types.rs 75–118}`; `gix/src/repository/{index.rs 25–120, dirwalk.rs 23–61}`
- `gix-dir/src/walk/{mod.rs 149–238, classify.rs 27–200, 354–463, readdir.rs 39–43}`
- `crate-status.md` 57–58, 845–897; `etc/plan/performance.md` 48–66

Evidence: `../evidence-hesperia.md` (all sections, including the object-store addendum).

## How it works

`Engine` funnels every full-tree walk through `status_paths_reported` (`repo.rs:1062`) → `status_paths_excluding` (`repo.rs:1696`), which arms a watchdog + a progress ticker, builds `repo.status(ScannedEntries)`, sets `thread_limit = 4` (`repo.rs:1750`), optionally clears `dirwalk_options` (`repo.rs:1745`), converts the memoized unreadable set into `:(exclude,literal)` pathspecs (`repo.rs:1752-1758`), drains the iterator into four `Vec<PathBuf>` buckets via `push_item` (`repo.rs:1536`), and — only when the caller holds the walk claim — writes the observed stat data back (`repo.rs:1859`, `persist_observed_stats`).

`WalkPolicy` (`repo.rs:1619-1680`) has three shapes: `full` (dirwalk + write-back), `tracked_only` (no dirwalk, write-back), `read_only` (**dirwalk on**, no write-back). `status_paths` — the no-argument entry point — is `read_only` (`repo.rs:1048-1051`).

Three callers reach a walk. The commit leg takes `claim_walk` and `commit_walk_policy` (`engine.rs:7137`, `7168-7173`), paced by `scan_is_due`/`watch_wake_pending`/`settle_window_elapsed` (`engine.rs:3727-3728`). The Pending poll takes the same claim behind `poll_may_walk`, a 60 s floor (`engine.rs:431`, `12429-12444`). **`path_durability` takes neither** (`engine.rs:4982`) and is called at 1 Hz from the recording banner (`src/hooks/use-recording-session.ts:183-187` → `ipc.rs:7291` → `7026` → `7127`).

Under gix, one walk fans into three threads (`gix/src/status/iter/mod.rs:66,116`): index↔worktree, dirwalk, and — always, because `head_tree: Some(None)` is the default and the `Platform` has no off switch (`gix/src/status/mod.rs:109`, `platform.rs:115-119`) — a HEAD-tree↔index diff.

---

## Cost table — one walk at 155 626 entries (hesperia tgdrive), and at 150 k

| Step | Anchor | Count per walk @155 626 | @150 k | Notes |
|---|---|---|---|---|
| open `.git/index`, mmap `map_copy_read_only` | `gix-index/src/file/init.rs:65` | 1 open + 26.4 MB COW map | 25 MB | fresh per walk: keeper opens a new `gix::Repository` per walk (`engine.rs:12459`, `4945`) |
| SHA-1 verify of the index | `file/init.rs:67-76` | 26.4 MB hashed | 25 MB | skipped only if `index.skipHash=true` — **unset on hesperia** |
| decode into `State` | `gix-index/src/decode/entries.rs:101-127` | 155 626 `Entry` (88 B) + ~15 MB path backing ≈ **29 MB heap** | ~28 MB | v4 decodes but expands paths, so heap is version-independent |
| `prepare_icase_backing` ×2 | `gix/src/status/index_worktree.rs:129`, `gix/src/repository/dirwalk.rs:60` | 2 × 155 626 hash inserts, ≈ 2 × 4 MB | idem | **macOS only** (`core.ignorecase=true`); zero on a case-sensitive Linux FS |
| pathspec common-prefix range | `gix-status/.../function.rs:92-94` | binary search; exclude-only patterns ⇒ prefix empty ⇒ **full range** | idem | this is the lever that is currently unused |
| entry-flag skip | `function.rs:278-285` | 155 626 checks, **0 skipped** | idem | `UPTODATE`/`FSMONITOR_VALID` are never set by gitoxide (see F-scan-7) |
| pathspec match per entry | `function.rs:288-300` | 155 626 string matches, **no syscall** | idem | matches happen *before* any `lstat` |
| `verified_path` (symlink stack) | `function.rs:387` | ~1 `lstat` per directory transition | idem | amortised, not per file |
| `lstat` per entry | `function.rs:409` | **155 626** | **150 000** | the floor of the index↔worktree leg |
| stat compare + racy check | `function.rs:472-484` | 155 626 compares, early return when clean | idem | a clean entry costs *no* read, *no* attribute lookup |
| attribute lookup + filter + ODB blob read | `function.rs:601-636` | only for the `k` dirty/racy entries | `k` | this is where the 450 GB can enter: an LFS-routed dirty path is streamed through `filter.lfs.clean` |
| dirwalk `read_dir` + NFC precompose | `gix-dir/src/walk/readdir.rs:40` | 1 `opendir` per directory + per-dirent NFC normalisation | idem | `core.precomposeunicode=true` ⇒ macOS pays an allocation + normalisation per dirent |
| dirwalk kind resolution | `classify.rs:200` | `d_type` from readdir usually suffices; falls back to `lstat` | idem | APFS returns `d_type`, so this is mostly free |
| HEAD-tree ↔ index diff | `gix/src/status/iter/mod.rs:66-108` | full HEAD tree walk: ~1 ODB read per tree object (≈10–15 k on this repo) + 155 626 comparisons | idem | **always on; cannot be disabled through gix 0.86's `Platform`** |
| index write-back (`persist_stats` and ≥1 stat changed) | `repo.rs:1859`, `gix/src/status/iter/types.rs:87-113` | 1 **deep clone of the 29 MB `State`** + one 26.4 MB write + SHA-1 | 25 MB | `has_changes()` is `false` on a fully quiet folder ⇒ no write. One changed entry ⇒ the whole 26.4 MB |

**Asymptotics.** CPU and syscalls are `Θ(n)` in *index entries*, not in bytes: 150 k entries ⇒ ≥150 k `lstat` + ~150 k stat compares + one 25 MB SHA-1 + one 28 MB decode, per walk. **100 k LFS pointers cost nothing extra** — a pointer blob is ≤ ~200 B and its index entry is the same size as any other (hesperia: 72 641 of 155 626 index paths resolve to `filter: lfs` and walks still finish at p50 1.08 s). **450 GB costs nothing** while stats match; it costs a full read + LFS clean per *dirty* path, which is why `STATUS_THREAD_LIMIT = 4` exists (`repo.rs:1504`). Memory is ~60 MB per concurrent walk without write-back (26 MB map + 29 MB state + 8 MB icase) and ~90 MB with it — per *concurrent* walk, see F-scan-2.

Measured against this model, hesperia's p50 of 1.079 s for 155 626 entries over 4 threads is ≈28 µs of wall-clock per entry per thread — consistent with a warm-cache `lstat` on APFS plus the icase and NFC work. The p90 of 5.3 s and max of 60.8 s are the same walk under I/O contention from the recording.

---

## Caller trace — why a walk runs every 1–5 s on hesperia

Arithmetic first. In the hour `2026-09-08T21` there were **666 walks**, all `scanned=155626`, all with `added=modified=deleted=0` and `entries` alternating 26 / 65 / 0.

1. **`entries` > 0 with all three change buckets at 0 ⇒ the emitted items were untracked directory entries.** `push_item` routes `TreeIndex` changes into `added`/`deleted`/`modified` (`repo.rs:1541-1563`) and `DirectoryContents` into `untracked` (`repo.rs:1580-1584`), which the log line does not print. `NeedsUpdate` items never reach keeper at all — gix intercepts them (`gix/src/status/iter/mod.rs:302-305`). So `find_untracked` was **true** on those 666 walks.
2. **Commit leg: cannot produce 666/h.** `scan_is_due` paces at `pollIntervalMs = 15 000` (`engine.rs:3538`) ⇒ ≤240/h even if every tick walked, and it only picks `full()` when `untracked_appeared` is set or the 900 s sweep is due (`engine.rs:1499-1530`). The log shows **one commit in three days**.
3. **Pending poll: cannot produce 666/h.** `poll_may_walk` enforces a 60 s floor after each walk *finishes* (`engine.rs:431`, `12248-12250`) ⇒ ≤60/h, and it uses `poll_walk_policy`, which is `tracked_only` (no dirwalk) except on the 900 s sweep.
4. **`path_durability` is the only unbounded caller.** `ipc.rs:7291` → `recording_snapshot_off_runtime` → `off_async_runtime(with_disk_figures)` (`ipc.rs:6982`) → `RecordingDurabilityReader::read` (`ipc.rs:7127`) → `Engine::path_durability` → `git::repo::status_paths(&repo)` (`engine.rs:4982`) = `WalkPolicy::read_only()` = **dirwalk on, write-back off, no `claim_walk`, no interval floor**. The frontend fires it every 1000 ms with no in-flight guard (`src/hooks/use-recording-session.ts:183-187`).
5. It only reaches the walk once the *session folder* is in HEAD (`engine.rs:4965-4980` returns early otherwise) — which is exactly what happens after the first segment of a recording is committed. That is the observed onset: 666 walks in the hour a screen recording was writing into `40-media/recordings/…`.

`60 (poll) + ~few (commit) + ~600 (durability) ≈ 666`. The residual 1–5 s cadence is the durability walk restarting as fast as it finishes.

**Could each caller answer without a full walk?**

| Caller | Question it actually asks | Cheaper answer available today |
|---|---|---|
| `path_durability` | "is *this one path* still identical to what HEAD holds?" | Yes — one path. `status_paths_excluding` already threads pathspecs; an *include* pathspec `:(literal)<rela>` narrows via `prefixed_entries_range` (`function.rs:92-94`) to an `O(log n)` range and prunes the dirwalk (`gix-dir/src/walk/mod.rs:235-237`). Simpler still: the answer is `entry.id == HEAD tree entry id && stat matches` = 1 binary search + 1 `lstat`. |
| Pending poll | "what is waiting?" | Mostly yes: settling rows come from the gate and repair rows from the index (`engine.rs:12345-12420`); only the untracked sweep genuinely needs a dirwalk, and it is already on a 900 s cadence. |
| Commit leg | "what changed since the last commit?" | Yes for the tracked half — the watcher already names every path (`engine.rs:3989-4010`); only "did something appear that git has never seen" needs the dirwalk, and `untracked_appeared` already isolates that case (`engine.rs:1526`). |

**The plumbing to invert already exists and is one signature away.** `status_paths_excluding(repo, skip: &[PathBuf], …)` builds `:(exclude,literal)` patterns at `repo.rs:1752-1758` and hands them to `platform.into_iter(patterns)` at `repo.rs:1761`. gix routes include and exclude pathspecs through the same `gix_pathspec::Search`. What is missing is the *path list*: `fold_watch_events` computes the relative path of every event and even index-checks it (`engine.rs:3989-4004`) — then throws it away into a per-profile boolean, `watch_wake: Mutex<HashSet<String>>` (`engine.rs:1110`, `4151-4152`).

---

## gix 0.86 verdict — `core.fsmonitor` and `core.untrackedCache`

**Both extensions are decoded and then ignored. Neither would help keeper's own walk. Enabling `core.untrackedCache` for foreign `git status` is actively destroyed by keeper.**

| Claim | Source anchor | Verdict |
|---|---|---|
| `UNTR` is parsed | `gix-index/src/extension/untracked_cache.rs:115-155`, `extension/decode.rs:46-47` | **parsed** |
| `FSMN` is parsed (V1 + V2) | `gix-index/src/extension/fs_monitor.rs:18-21`, `extension/decode.rs:49-50` | **parsed** |
| Anything *consumes* them | only `gitoxide-core/src/index/information.rs:96-101` (prints their names) | **no consumer in `gix`, `gix-status` or `gix-dir`** |
| `crate-status.md` self-assessment | `:57-58` ("big-repo accelerators … fsmonitor, untracked-cache" unchecked), `:846` ("accelerated walk with `untracked`-cache" unchecked), `:877-879` (write: `UNTR` ☐, `FSMN` ☐) | confirms |
| Roadmap | `etc/plan/performance.md:55-65` — "untracked-fsmonitor-cache: **Use** untracked-cache and fsmonitor data aggressively where valid" is a *plan*, not shipped | confirms |
| `gix-status` has an `FSMONITOR_VALID` fast path | `gix-status/.../function.rs:278-285` | exists but is **dead**: nothing in gitoxide ever *sets* `Flags::FSMONITOR_VALID` or `Flags::UPTODATE` (grep across the whole checkout finds only reads, plus `gix-dir/tests/walk_utils/mod.rs:313` which sets it to *pretend* the index was refreshed) |
| `gix-dir` has an `UPTODATE` fast path | `classify.rs:376-392`, `gix/src/repository/dirwalk.rs:23-25` ("items will only count as tracked if they have the `UPTODATE` flag set") | same — never populated, so the dirwalk always resolves kinds from `d_type`/`lstat` |
| gix **writes** the index without them | `gix-index/src/write.rs:117-128` — the writer's extension list is exactly `tree` + `sparse`; `Extensions::All` cannot add more | **UNTR, FSMN, REUC and `link` are silently dropped on every write** |
| gix writes v2/v3 only | `gix-index/src/write.rs:144-151` (`detect_required_version` returns `V3` or `V2`, never `V4`) | `index.version=4` is downgraded by keeper's write-back, although v4 *reads* fine (`decode/header.rs:39`, `decode/entries.rs:81-104`) |
| `index.skipHash` is honoured | `gix/src/config/tree/sections/index.rs:11-12`, `gix/src/repository/index.rs:33-41`, `gix/src/status/iter/mod.rs:57-60`, `iter/types.rs:107-111` | **read and write both honour it** |

Consequences:

- **keeper's own walk gains nothing from either setting.** There is no code path by which a valid `UNTR` or `FSMN` block shortens a gix walk.
- **Setting `core.untrackedCache=true` would help only foreign `git status` (Finder plug-ins, the user's shell, agents) — and only until keeper's next walk with `persist_stats`.** `write_changes` (`gix/src/status/iter/types.rs:87-113`) re-serialises the whole index through `write.rs:117-128`, dropping the `UNTR` block; git then rebuilds it. Same for `core.fsmonitor`'s `FSMN`.
- **keeper's watcher already *is* the fsmonitor.** `folder watch armed … backend="native"` for all three profiles delivers the same information git's fsmonitor hook delivers. It is not wired to the walk because the path list is discarded (`engine.rs:4151`). Feeding those paths in as *include pathspecs* is the fsmonitor optimisation, implemented at the layer keeper controls, with no dependency on gitoxide shipping `UNTR`/`FSMN` support.

---

## What is correct / well done

- **`persist_stats` is the right fix for the right defect and is correctly gated on the walk claim** (`repo.rs:1628-1636`, `1859-1860`). The 60 280 stat-less entries / 25.4 GB re-hash per pass is gone; hesperia's p50 of 1.08 s over 155 626 entries proves it.
- **`WalkPolicy::tracked_only` is a genuine, correct optimisation**, and its correctness argument (`engine.rs:1507-1517`) is sound: watcher + `untracked_appeared` is a valid substitute for the dirwalk on the commit leg, and the flag is *consumed* rather than merely read (`engine.rs:1526`).
- **`claim_walk` is the right shape** — non-blocking, single door, counter at the door (`engine.rs:1461-1470`) — and the argument for having no waiting variant is correct.
- **`STATUS_THREAD_LIMIT = 4` with a `const` assertion** (`repo.rs:1504-1510`) is a load-bearing bound enforced where it cannot be argued away.
- **`ScannedEntries` / the three-signal watchdog** (`repo.rs:1189-1200`, `1300-1345`) is unusually good: emission is correctly rejected as a liveness proxy, and the scratch-bytes signal is the work itself rather than a proxy for it.
- **The pathspec exclusions are `literal`** (`repo.rs:1685-1688`) — the one detail everyone gets wrong on a folder containing `[2026]` and `*`.
- **`restore_missing_checkout` pre-clears with `SKIP_WORKTREE` after an `lstat` sweep** (`repo.rs:2216-2262`) rather than letting the checkout collide. This *is* the fix for the 588 408 `collided (AlreadyExists)` errors and the 900 s filter wedge behind them; the safety flags are kept anyway so the residual race is lost safely. The reasoning at `repo.rs:2166-2172` — that `overwrite_existing = false` alone does **not** protect bytes, only `destination_is_initially_empty = true` does — is correct against the gix source.
- **`checkout_is_unfinished` is checked *before* the walk on the commit leg** (`engine.rs:7122-7133`), which is what stops a 155 625-deletion walk on a repairing repository.
- **`is_git_control_file`** (`lfs/stage.rs:96-125`) is the right guard in the right place — ahead of the size rule (`stage.rs:194`).
- **`Anomaly`'s four-field contract** (`anomaly.rs:29-58`) is the correct shape for field diagnosis and the reason the hesperia log is readable at all.

---

## Findings

### F-scan-1 The recording durability probe runs an unclaimed, unthrottled full-tree walk at 1 Hz — P1, effort S

**Evidence.** `engine.rs:4982`:
```rust
let status = match git::repo::status_paths(&repo) {
```
`status_paths` is `WalkPolicy::read_only()` — `find_untracked: true`, `persist_stats: false` (`repo.rs:1048-1051`, `1674-1678`). `path_durability` takes no `claim_walk` and consults no interval; `claim_walk`'s only two callers are `engine.rs:7137` and `12430`. Field: 1371 walks in the session, `scanned=155626` every time, 666 in one hour while a recording ran.

**Why.** This is the entire 1–5 s cadence. Each pass costs 155 626 `lstat`s, a 26.4 MB SHA-1, a 29 MB index decode, two 155 626-entry icase tables, a full HEAD-tree diff and a full directory walk — to answer one boolean about one path. Because it is `read_only`, every stat it observes is discarded, so it cannot make itself cheaper. Because it holds no claim, it runs *beside* the commit leg's walk, halving the throughput of the pass that actually publishes data — the exact failure `claim_walk` was written to prevent (`engine.rs:1443-1452`). On a USB volume it also competes with the recording it is reporting on: the session's max walk is 60.8 s.

**Fix.** Answer the question directly: binary-search the index for `rela` (`entry_by_path`, `O(log n)`), `lstat` the one file, compare against the entry's stat and the HEAD tree entry id. If a walk is genuinely wanted, call `status_paths_excluding` with an *include* pathspec `:(literal)<rela>` and `find_untracked: false`, and take `claim_walk`.

### F-scan-2 The 1 Hz poll has no in-flight guard, so full-tree walks pile up on the blocking pool — P1, effort S

**Evidence.** `src/hooks/use-recording-session.ts:183-187`:
```ts
const interval = setInterval(() => {
  void recordingStatus().then(adopt).catch(() => {});
}, 1000);
```
`recording_status` dispatches to `spawn_blocking` (`ipc.rs:6982`, `1348`), and the walk it reaches takes no lock (F-scan-1).

**Why.** The poll fires every 1000 ms regardless of whether the previous one returned; hesperia's walks have p90 = 5.3 s and max = 60.8 s. Nothing bounds the number of concurrent full-tree walks except tokio's blocking-pool ceiling (512). Each concurrent walk carries ~60 MB of index state, 4 status worker threads, a tree-index producer thread and a detached watchdog thread. A single 60 s walk admits up to ~60 more behind it. That the observed rate is 666/h rather than 3600/h says the machine is already the limiter.

**Fix.** Guard the poll on the client (skip a tick while one is in flight) *and* make the engine idempotent under concurrency by giving `path_durability` the walk claim — a refused claim should return the last known answer, which `RecordingDurabilityReader` already floors correctly (`ipc.rs:7137-7156`).

### F-scan-3 `WalkPolicy::read_only()` turns the directory walk **on** — P2, effort S

**Evidence.** `repo.rs:1674-1678`:
```rust
pub const fn read_only() -> Self {
    Self { persist_stats: false, find_untracked: true }
}
```
Callers: `path_durability` (`engine.rs:4982`), `prime_worktree_changes` (`engine.rs:4330`), `git/history.rs:279`, `repo.rs:2531`.

**Why.** `find_untracked` is documented one screen earlier as "the expensive half of a walk … it `lstat`s the whole tree every time" (`repo.rs:1637-1641`), yet the policy named for the cheapest callers enables it. `path_durability` asks about a path that is in HEAD — by construction *not* untracked — so the dirwalk can never change its answer. `repo.rs:2531` (pre-write collision check) likewise only consults `modified`.

**Fix.** Split into `read_only_tracked()` (`find_untracked: false`) and keep the untracked variant for the callers that actually read `status.untracked`. Two-line change; removes the dirwalk from the 1 Hz path.

### F-scan-4 Every walk also runs a HEAD-tree↔index diff that no caller asked for and gix cannot switch off — P2, effort M

**Evidence.** `gix/src/status/mod.rs:109` sets `head_tree: Some(None)` by default; `gix/src/status/iter/mod.rs:66-108` spawns `gix::status::tree_index::producer` and calls `repo.tree_index_status(&tree_id, …)` whenever it is set. `gix/src/status/platform.rs:115-119` offers only `head_tree(id)` — there is **no** method that sets it back to `None`. keeper never touches it (`repo.rs:1725-1751`).

**Why.** On a folder keeper maintains, the index and HEAD are identical except in the milliseconds inside `stage_and_commit`; the diff's entire contribution is `status.added`, and it is `0` on every one of hesperia's 1371 walks. Its cost is a full recursive walk of HEAD's tree — order 10⁴ ODB reads from 6 packs totalling 1.02 GiB, plus 155 626 comparisons, on a third thread, on every walk.

**Fix.** Call the lower-level `repo.index_worktree_status(...)` (`gix/src/status/iter/mod.rs:117-127` shows the exact call) instead of the `status()` platform, and synthesise `added` only where a caller needs it (the commit leg, once). Alternatively upstream a `Platform::no_head_tree()`.

### F-scan-5 The watcher's path list is discarded into a per-profile boolean — P2, effort M

**Evidence.** `engine.rs:1110`:
```rust
watch_wake: Mutex<HashSet<String>>,
```
holding profile ids, set by `note_watch_wake(&profile.id)` (`engine.rs:4151-4152`). `fold_watch_events` has each event's absolute path, strips it to `rela`, and binary-searches the index with it (`engine.rs:3989-4004`) — then drops it (only a debug `watch_tap` keeps a copy, `engine.rs:4008-4019`).

**Why.** This is the single input that would make steady-state cost proportional to the change set. keeper's watcher is the fsmonitor equivalent (F-scan-7), and the walk already accepts pathspecs (`repo.rs:1752-1761`). Without the path list, every wake — one saved file — costs a walk of the whole index.

**Fix.** Keep a bounded `HashMap<profile_id, HashSet<PathBuf>>` beside `watch_wake` (cap ~4 096 paths; overflow sets a `full_walk_owed` flag, exactly as an fsmonitor cookie overflow does) and hand the set to `status_paths_reported` as include pathspecs.

### F-scan-6 The pathspec plumbing only ever excludes; inverting it is the ranked-#1 design change — P2, effort M

**Evidence.** `repo.rs:1752-1761`:
```rust
let mut pattern = gix::bstr::BString::from(":(exclude,literal)");
pattern.extend_from_slice(&gix::path::into_bstr(path.as_path()));
```
gix supports the include direction fully: `index.prefixed_entries_range(pathspec.common_prefix())` narrows the index scan by binary search (`gix-status/.../function.rs:92-94`), `pattern_matching_relative_path` rejects non-matching entries **before any syscall** (`function.rs:288-300`), and the dirwalk prunes recursion — "we only traverse into directories if it matches" (`gix-dir/src/walk/mod.rs:235-237`).

**Why.** With include pathspecs, a pass with 3 changed paths costs 3 `lstat`s + `O(log n)` instead of 155 626 `lstat`s. Even when the paths are scattered (no useful common prefix), every non-matching entry costs a string comparison rather than a syscall — on hesperia the difference between ~1.1 s and single-digit milliseconds.

**Fix.** Give `status_paths_excluding` an `include: &[PathBuf]` parameter alongside `skip`, emit `:(literal)` patterns for it, and feed it from F-scan-5's set. Keep the full walk for the untracked sweep, the first pass of a run, and any pass where the watcher overflowed.

### F-scan-7 gix decodes `UNTR`/`FSMN` and never uses them; keeper's index write-back **deletes** them — P2, effort S (config/doc) / L (upstream)

**Evidence.** `gix-index/src/write.rs:117-128` — the writer's extension table is exactly two closures, `tree` and `sparse`. `crate-status.md:877-879` lists write support for `REUC`, `UNTR`, `FSMN` as unchecked. Nothing sets `Flags::UPTODATE` or `Flags::FSMONITOR_VALID` anywhere in the checkout outside a test fixture (`gix-dir/tests/walk_utils/mod.rs:313`), so the fast paths at `gix-status/.../function.rs:278-285` and `gix-dir/src/walk/classify.rs:376-392` are dead.

**Why.** Two consequences. (a) Enabling `core.fsmonitor` or `core.untrackedCache` **does nothing for keeper's own walk** — there is no consumer. (b) Enabling `core.untrackedCache` for the *user's* benefit (Finder, shell, agents on a 155 k-file folder) is defeated: every keeper walk with `persist_stats` calls `write_changes`, which re-serialises the index through `write.rs` and drops the `UNTR` block, so git rebuilds the cache from scratch on its next `status`. Same for `FSMN`. This is a silent interference with the user's own repository configuration that nothing in keeper documents.

**Fix.** Do not recommend `core.untrackedCache`/`core.fsmonitor` to keeper users, and say so in `docs/sync.md` §20 with the reason. If foreign-`git status` speed becomes a goal, the fix is upstream `UNTR`/`FSMN` write-through in `gix-index`, or keeper must stop rewriting the index in place.

### F-scan-8 `index.skipHash` is not set, so every walk SHA-1s 26.4 MB, twice when it writes back — P2, effort S

**Evidence.** Evidence file: "**NOT set anywhere**: `core.fsmonitor`, `core.untrackedCache`, `index.version`, `index.threads`, `feature.manyFiles`, `core.splitIndex`". gix honours the key on read (`gix/src/repository/index.rs:33-41`, `gix-index/src/file/init.rs:67-76`) and on write (`gix/src/status/iter/mod.rs:57-60` → `iter/types.rs:107-111`). `enforce_local_config_with_filter` (`repo.rs:688-783`) sets `index.sparse`, `user.*` and `filter.lfs.*` and nothing else.

**Why.** Per walk: one SHA-1 over 26.4 MB on open, plus one on write-back when stats changed. At the current cadence that is real, repeated, avoidable CPU on the machine the user is recording on. It is the cheapest win available and git 2.52 (hesperia) understands the key.

**Fix.** `config.set_raw_value("index.skipHash", "true")` beside `index.sparse` at `repo.rs:696`. State the trade in the doc comment: the index loses its integrity trailer, which for a file keeper rewrites from a verified `HEAD` is an acceptable exchange.

### F-scan-9 `index.version=4` would be silently downgraded to v2 by keeper's write-back — P2, effort S (doc) / L (upstream)

**Evidence.** `gix-index/src/write.rs:144-151`:
```rust
fn detect_required_version(&self) -> Version {
    self.entries.iter().find_map(|e| e.flags.contains(entry::Flags::EXTENDED).then_some(Version::V3))
        .unwrap_or(Version::V2)
}
```
Reading v4 works (`decode/header.rs:39`, `decode/entries.rs:81-104`), and v4 paths are expanded into the same in-memory backing, so v4 saves **disk and page cache**, not heap.

**Why.** `index.version=4` is the obvious "pure config" recommendation for a 155 k-entry index (path deltas typically cut the file 30–50 %, so 26.4 MB → ~15 MB, shortening both the SHA-1 and the write). But keeper's first `persist_stats` walk rewrites it as v2 and the user's setting evaporates with no message. Recommending it without this caveat would be wrong.

**Fix.** Either document it as a non-option (`docs/sync.md` §20) or upstream v4 writing to `gix-index`. Do not ship a recommendation keeper itself undoes.

### F-scan-10 On macOS every walk builds two 155 626-entry case-folding tables and NFC-normalises every dirent — P2, effort M

**Evidence.** `gix/src/status/index_worktree.rs:129`:
```rust
let accelerate_lookup = fs_caps.ignore_case.then(|| index.prepare_icase_backing());
```
and independently `gix/src/repository/dirwalk.rs:60` — the same call, on the same index, for the dirwalk. `prepare_icase_backing` is `O(n)` hashing over every entry path plus every directory prefix (`gix-index/src/access/mod.rs:145-186`) and carries the upstream note "TODO: needs multi-threaded insertion". `gix-dir/src/walk/readdir.rs:40` passes `opts.precompose_unicode` into every `read_dir`. hesperia: `core.ignorecase=true core.precomposeunicode=true`.

**Why.** This is the concrete answer to "what dominates on macOS versus the 0.76 s Linux figure in `docs/sync.md` §19". Linux on a case-sensitive FS builds **zero** icase tables and does **zero** NFC work; macOS builds two per walk and normalises every dirent. Add USB APFS `lstat` latency and 0.76 s becomes 1.1–9 s. The attribute stack is *not* a factor on a clean tree — `matching_attributes` is only reached for entries that need a content comparison (`gix-status/.../function.rs:601-636`).

**Fix.** Not keeper's to fix directly, but another argument for F-scan-6: an include pathspec makes both tables unnecessary because the dirwalk is pruned and the index range narrowed. Short of that, upstream a shared `AccelerateLookup` between the status and dirwalk legs (both are built from the same `index`).

### F-scan-11 The hourly footprint anomaly is a second full-tree `lstat` sweep plus 155 626 index snapshots — P2, effort S

**Evidence.** `engine.rs:3590-3597` → `footprint::blobs_over_threshold` (`footprint.rs:193-213`):
```rust
for rela in tracked {
    if lfs::stage::indexed_pointer(repo, rela).is_some() { continue; }
    let Ok(meta) = std::fs::symlink_metadata(root.join(rela)) else { continue };
```
`indexed_pointer` re-enters `repo.index_or_empty()` **per path** (`lfs/stage.rs:964`), which re-stats `.git/index` through `recent_snapshot` (`gix/src/repository/index.rs:117-119`) on every call. `tracked_paths` (`repo.rs:2949`) first materialises a 155 626-element `Vec<PathBuf>` (~20 MB).

**Why.** Hourly per profile (`SWEEP_EVERY_MS`, `engine.rs:471`) — 75 anomaly lines over three days matches. Each run is 155 626 `symlink_metadata` + 155 626 `stat(.git/index)` + 155 626 binary searches + up to 155 626 pack header reads, on a USB volume, on the blocking pool, for a number that has not changed in weeks. It is the same walk the status pass just did, done again with worse constants.

**Fix.** Hoist the index snapshot out of the loop (pass `&gix::index::File` into `blobs_over_threshold` instead of `&Repository`), and derive the count from the walk that already ran — or memoise it against the HEAD commit id, since the answer can only change when a commit lands.

### F-scan-12 The 996 s dirwalk figure that justifies the whole `WalkPolicy` design is contradicted by the field log — P2, effort S

**Evidence.** `repo.rs:1639-1642`: "Measured on tgdrive, `find . -type f` over 157 490 files takes 996 s on that USB volume, and that is the floor for a pass that asks the question at all." Repeated at `engine.rs:1478`, `7163-7166`, `12440-12443`. Against it: 666 walks in one hour on that same folder, **with the dirwalk enabled** (see the caller trace), p50 1.079 s, min 170 ms.

**Why.** The 996 s number is a cold-cache-and-contended measurement quoted as a steady-state floor, and three design decisions cite it (the `tracked_only` policy, the 900 s `UNTRACKED_SWEEP_INTERVAL`, the poll's untracked skip). If the warm figure is ~1 s, the untracked sweep is being deferred 900 s to save a cost three orders of magnitude smaller than stated — meaning new files can sit invisible for 15 minutes for no measured benefit. The decisions may still be right; the number they rest on is not.

**Fix.** Re-measure the dirwalk in isolation on hesperia (`full()` vs `tracked_only()` A/B on the same folder) and either correct the comments or shorten `UNTRACKED_SWEEP_INTERVAL`.

### F-scan-13 `.lfsconfig` and `.keepervirtual` are protected from virtualization but not from LFS conversion — P2, effort S

**Evidence.** `virtual_policy.rs:623-631` protects four classes:
```rust
if crate::lfs::stage::is_git_control_file(rela) { return true; }
if name.is_some_and(|name| name == VIRTUAL_PATTERN_FILE || name == ".lfsconfig") { return true; }
```
but `is_git_control_file` itself covers only three names (`lfs/stage.rs:110`): `[".gitattributes", ".gitignore", ".gitmodules"]`, and it is the *only* guard consulted by `LfsPolicy::applies` (`stage.rs:194`).

**Why.** A `.lfsconfig` or `.keepervirtual` over the threshold is converted to a pointer by staging. Both are read unfiltered by their consumers — `engine.rs:7919` reads `.lfsconfig` with `std::fs::read_to_string`, `virtual_policy` compiles `.keepervirtual` from the worktree (`virtual_policy.rs:187-189`) — so a pointerised copy silently becomes "no LFS endpoint override" and "an empty virtualization policy". `virtual_policy.rs:1498-1500` states this failure mode in prose and its own test asserts it (`:1830`), yet the routing side is one name-list away from allowing it. The asymmetry is the bug: two lists that must agree, and only one is complete.

**Fix.** Move `.lfsconfig` and `.keepervirtual` into `GIT_CONTROL_FILES`, or make `virtual_policy::is_control_file` the single shared predicate, so routing and virtualization cannot diverge.

### F-scan-14 A `.gitattributes` that is already pointer text is neither detected nor repaired; the failure is silent apart from a log flood — P2, effort M

**Evidence.** The guard at `lfs/stage.rs:96-125` prevents keeper from *creating* one; nothing consumes `is_git_control_file` in a detection or repair path (its only callers are `stage.rs:194` and `virtual_policy.rs:624`). Attributes for the status walk are sourced **worktree-first**: `gix/src/status/index_worktree.rs:114` passes `attributes::Source::WorktreeThenIdMapping`, so the on-disk bytes win. Field: 1 314 669 `gix_attributes::search::attributes` WARN lines on 2026-08-27/28, two per parse of `version https://git-lfs.github.com/spec/v1` + `oid sha256:…`.

**Why.** While that state holds, `filter=lfs` routing is void for the whole subtree below the affected file — which is the shape of hesperia's standing anomaly: 600 files / 1.96 GB carried as plain blobs on a folder whose threshold never moved. **[INFERENCE]** — the causal link between the 08-27 pointer window and those 600 blobs is consistent with both records but not directly proven here; the mechanism, however, is stated in keeper's own source (`stage.rs:99-105`) and the consequence is permanent ("history keeps them as blobs for good"). There is no anomaly line for "a control file in this tree is pointer text", so a machine in this state announces itself only as a million warnings.

**Fix.** On each commit pass, check the tracked `.gitattributes`/`.gitignore`/`.gitmodules`/`.lfsconfig`/`.keepervirtual` paths against `worktree_pointer` (already available, `stage.rs:1044`) and raise an `Anomaly` naming the path and the subtree it governs; optionally hydrate it from the LFS store when the object is present.

### F-scan-15 The global `EnvFilter::new("info")` lets third-party crates write hundreds of megabytes into the shipped log — P2, effort S

**Evidence.** `keeper/src/debug_log.rs:129-132`:
```rust
.with_env_filter(
    tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
)
```
with no per-target directives, and `notes_ipc.rs:1627-1629` confirms no `RUST_LOG` is set in the macOS app. Field: 1 314 669 WARN from `gix_attributes::search::attributes` and 588 408 ERROR from `gix_worktree_state::checkout::chunk`, in an 895 MB log with a 1.33 GB rotated sibling.

**Why.** Both floods are gitoxide emitting one line per *item* — per attribute pattern, per collided path. keeper cannot fix the emission upstream, but it owns the filter. Two million lines from two library targets is a log nobody can read at the moment it matters, and it is why the field diagnosis was expensive. The checkout half is now unreachable in normal operation (`restore_missing_checkout` pre-clears with `SKIP_WORKTREE`), but the filter is what makes the *class* survivable.

**Fix.** Add target directives to the default filter: `info,gix_attributes=error,gix_worktree_state=warn,gix_dir=warn`. One line.

---

## Ranked proposals

Config-only first (no keeper code, but note which are traps):

| # | Change | Kind | Effect | Effort |
|---|---|---|---|---|
| C1 | `index.skipHash=true` in `enforce_local_config_with_filter` (`repo.rs:696`) | config, written by keeper | removes one SHA-1 over 26.4 MB per index open and per write-back | S |
| C2 | `index.threads` — leave alone | config | gix reads it (`gix/src/repository/index.rs:26-32`) but it only parallelises *decode*; keeper already caps status threads at 4 for filter-deadlock reasons (`repo.rs:1504`) | — |
| C3 | **Do not** set `index.version=4` | anti-recommendation | gix writes v2/v3 only (`gix-index/src/write.rs:144-151`); keeper's write-back silently reverts it | S (doc) |
| C4 | **Do not** set `core.untrackedCache` / `core.fsmonitor` | anti-recommendation | no consumer in gix; and keeper's write-back *deletes* the `UNTR`/`FSMN` blocks (`write.rs:117-128`), leaving foreign `git status` worse off than if it had rebuilt its own | S (doc) |

Code, ranked by benefit-per-effort:

1. **Stop the 1 Hz full-tree walk** (F-scan-1, F-scan-2, F-scan-3) — S. Answer `path_durability` from one index lookup + one `lstat`; add the walk claim; add a client-side in-flight guard. This alone removes ~600 full-tree walks per hour on hesperia — the dominant scan cost on the machine — and needs no new mechanism.
2. **`index.skipHash=true`** (F-scan-8) — S. One line, applies to every walk on every profile.
3. **Split `read_only()`** so callers that never read `status.untracked` do not pay the dirwalk (F-scan-3) — S.
4. **Log-target filter** (F-scan-15) — S.
5. **Retain the watcher's paths and feed them as include pathspecs** (F-scan-5 + F-scan-6) — M. The change that makes steady state proportional to the change set: `prefixed_entries_range` for the index leg, pathspec-pruned recursion for the dirwalk, bounded overflow fallback to the full walk. It is the fsmonitor optimisation, implemented where keeper already owns the data, with no dependency on gitoxide.
6. **Drop the unasked-for HEAD-tree diff** by calling `index_worktree_status` directly (F-scan-4) — M. Removes a producer thread and ~10⁴ ODB reads per walk.
7. **Fold the hourly footprint sweep into the walk that already ran, and hoist the per-path index snapshot** (F-scan-11) — S/M.
8. **Unify the control-file predicate; add a pointerised-control-file anomaly** (F-scan-13, F-scan-14) — S then M.
9. **Re-measure the dirwalk and correct or act on the 996 s figure** (F-scan-12) — S to measure, M if `UNTRACKED_SWEEP_INTERVAL` should shrink.
10. **Upstream**: `UNTR`/`FSMN` write-through and v4 writing in `gix-index`; a shared `AccelerateLookup` between the status and dirwalk legs; `Platform::no_head_tree()` — L, and only worth it if keeper wants foreign `git status` on these folders to be fast too.

## Open questions I could not settle

- **The exact repetition mechanism behind 1 314 669 attribute warnings.** Two lines per parse means ~657 k parses of the pointer text. The worktree-first attribute source (`index_worktree.rs:114`) and a per-thread `gix_worktree::Stack` explain re-parsing on directory changes, but I could not derive 657 k from the walk cadence of those two days without running it. The mechanism (a pointerised `.gitattributes` is parsed as rules) is certain; the multiplier is not.
- **Whether the 600 blobs-over-threshold on tgdrive were created during the 08-27/28 pointerised-`.gitattributes` window.** Consistent, and the mechanism is documented in keeper's own source, but proving it needs `git log` archaeology on the Mac. Marked `[INFERENCE]` in F-scan-14.
- **The true warm cost of the dirwalk on that USB volume** (F-scan-12). The field log shows full walks at p50 1.08 s, irreconcilable with 996 s as a steady-state floor, but I cannot run the A/B from here.
- **Whether tokio's blocking pool actually accumulated concurrent walks on hesperia.** The code permits it (F-scan-2); 666/h rather than 3600/h suggests something throttled it, and I could not tell from the log whether that was the frontend, the IPC layer, or I/O contention. A `status_walks` counter delta against wall-clock, or a log line naming the walk's caller, would settle it — and is worth adding on its own merits.
