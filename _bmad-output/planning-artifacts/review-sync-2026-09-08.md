# Review: keeper folder sync (git + LFS) — correctness, efficiency, scale, intent

- **Date:** 2026-09-08
- **Scope:** `src-tauri/crates/keeper-sync` (≈90 k lines), `keeper-syncd`, the shell's sync IPC, `docs/sync.md`, the BMAD artifacts for epics 23–35, 39, 41, 56, 57, and the real client on hesperia.
- **Method:** one evidence pass on the owner's Mac (log, `sync.db`, the three synced repositories), then nine read-only review lanes run in parallel — engine loop, status walk, LFS, virtual files, watcher/gate, git protocol, durable state, BMAD intent, dependencies — each anchored to `file:line`. The lane reports are in [`review-sync-2026-09-08/lanes/`](review-sync-2026-09-08/lanes/); the field evidence is [`review-sync-2026-09-08/evidence-hesperia.md`](review-sync-2026-09-08/evidence-hesperia.md).
- **Grading:** every claim below is either anchored (`file:line`, a log line, a number from the Mac) or marked `[INFERENCE]`. Severity: **P0** data loss / permanent livelock; **P1** wrong behaviour a user will hit; **P2** inefficiency or robustness gap; **P3** nit / doc drift.

---

## 0. The owner's questions, answered

| Question | Short answer |
|---|---|
| **Does it work correctly?** | Mostly, and the hard parts are unusually well done (§4). It has **one P0** — a modify/delete divergence leaves `MERGE_HEAD` behind and every later merge exits 128 forever (§3.1) — and a cluster of P1s that are all the same shape: a documented guarantee whose code path is dead or misclassified (conflict matrix, tier-4 verify, "N failures in a row", offline detection, prune-only-what-the-remote-holds). |
| **Is it efficient?** | Per pass, yes — one walk of 155 626 entries costs p50 1.1 s on a USB APFS volume. Per *event*, no: nothing is proportional to what changed. On hesperia the folder was walked **1 371 times in one session, up to 666 times per hour, every walk touching all 155 626 entries**, mostly because a 1 Hz recording banner poll runs an unclaimed, unthrottled full-tree walk (§3.2). The remote had been down six days and the engine never noticed (§3.3). |
| **Can it be made better?** | Yes, and cheaply. Four small changes remove ~90 % of the field cost: stop the 1 Hz durability walk; classify the fetch timeout as `Network`; feed the watcher's path list into the walk as include pathspecs (the "fsmonitor" keeper already has); set `index.skipHash=true`. Roadmap in §6. |
| **How does it work?** | §2, with the diagram. |
| **150 k files / 100 k LFS / 450 GB — will it choke?** | It does not fall over today (memory ~60–90 MB per walk, index 26 MB, walk ≈ 1 s warm). It **degrades**: ~100 % duty cycle while anything writes; profiles ticked serially so one folder stalls the others; a first materialize of 100 k objects is 100 k serial batch POSTs; `enqueue_unique` is O(n²) at that queue depth; every commit rewrites the whole index and every directory tree; nothing runs `gc`. Details in §5. The NFR it was built to (NFR-23: 100 000 files / **50 GB**) is below the real folder and was never re-authored (§3.5). |
| **`core.fsmonitor` / `core.untrackedCache` — used? useful?** | Not used. **Not useful for keeper's own walk**: gix 0.86 parses the `UNTR`/`FSMN` index extensions and never consults them (`gix-index/src/extension/*`, no consumer in `gix-status`/`gix-dir`). Worse, keeper's stat write-back re-serialises the index through `gix-index/src/write.rs:117-128`, which writes only the `tree`+`sparse` extensions — so enabling `core.untrackedCache` for a foreign `git status` is **deleted on keeper's next walk**. Same for `index.version=4` (gix writes v2/v3 only). The one honoured knob is `index.skipHash=true`. keeper's watcher *is* the fsmonitor; it just throws the path list away (§3.2). |
| **Are the intervals right for one personal user, rare edits, ≤3 clients?** | `settleMs` 5 s (10 s removable, 60 s ceiling), notes `commitIdleMs` 2 s: fine. `pollIntervalMs` 15 s is too aggressive as a *backstop* once a watcher is live, and — the real problem — every paced scan also enqueues a **fetch** and every successful fetch runs a **155 k-path prune plan**. Recommend: 300 s backstop when the watcher is live, a separate 5-min remote poll, prune only after an LFS unit completed. `pushIntervalMs` 30 s for notes is fine online and needs backoff offline (§3.4). |
| **Files not code — is git the right store? rustfs / PostgreSQL+SQLite instead?** | Git stays the right store for *this* owner (browsable folder, history and provenance, agents can clone, Forgejo already exists, offline-first, ≤3 clients). The cost problem is in keeper's walk policy, not in git — git solved exactly this class with fsmonitor and keeper already has the equivalent signal. What git is genuinely the wrong shape for is bounded and known: per-object LFS round trips, blobs already in history (1.96 GB permanent), no partial clone, two clones on one Mac with no local peer path. Moving the tree to PostgreSQL would mean owning a server and a protocol and losing everything above. Full argument in §7. |
| **Virtual files — do they work?** | **They work, with bugs.** The arrival path consults the policy (the earlier defect is fixed and tested); the release proof is per-object and fresh; `verify` excuses virtual and only virtual; the smudge filter never fetches. But on macOS every materialized row **counts down to a release that structurally cannot happen** (the platform refuses `OpenUnknown`, the UI shows "23 hr"); a pointerised `.keepervirtual` compiles into a policy that authorises nothing; the ledger has 89 289 rows, is never pruned and survives profile deletion (§3.6). |
| **Sync + communication implementation bugs?** | Register in §3; 117 findings across the nine lanes — 1 P0 (a second lane-rated P0 is downgraded to P1 with the reason stated), 25 P1, the rest P2/P3. |
| **Libraries used correctly?** | Yes, with three gaps: gix's own fetch transport has a 20 s connect timeout and **no read timeout** (a half-open socket parks a profile forever); the `git` CLI shim has no wall-clock bound; reqwest clients lack `tcp_keepalive`. The gitoxide fork (`keeper/gix-filter-0.33-reap`) is exactly two commits on the 0.86 release, its fix is merged upstream (GitoxideLabs/gitoxide#2946, 2026-08-27) but not yet released — retire the pin at gix-filter ≥ 0.34.1. No RUSTSEC advisories on any locked crate (§3.6, Dependencies; lanes/deps.md). |
| **Does the code match the BMAD intent?** | The AD-40…AD-53 spine is implemented. Five things drifted with no artifact noticing: the linked-worktree lane became a branch switch; `gc` and the three worktree verbs are dead code; research open question #6 ("when does `git gc` run?") was answered by a launchd cron the owner wrote; §18 claims "verified against real remotes" while story 31-4 is still in-progress; the store choice itself was never a decision (§3.5, §7). |

---

## 1. Evidence from the real client (hesperia, 2026-09-08)

Full detail: [`evidence-hesperia.md`](review-sync-2026-09-08/evidence-hesperia.md). The facts the rest of this document leans on:

- Three profiles on one Forgejo remote over the tailnet: `tgdrive` (155 626 index entries, 26.4 MB index, USB APFS), `neuradrive` (775), `tgdrive-light` (a second clone of tgdrive on the internal SSD). `lfsMode=materialize`, threshold 256 KiB, `lfsPruneLocal=true`, `settleMs=5000`, `pollIntervalMs=15000`.
- The remote has been **unreachable since 2026-09-02**. Journal: 3 × `pull|pending` at 615/549/559 attempts, 2 × `lfsUpload|pending` (one 92-byte conflict copy at 356 attempts, one 2.0 GB screen recording at 15), 2 × `push|deferred` ("on hold until this folder's large files reach the remote"). The folder pane never said "offline".
- **1 371 status walks of `tgdrive` in the current session, `scanned=155626` every time**, elapsed p50 1 079 ms / p90 5 287 ms / max 60 828 ms; 666 walks in the hour a screen recording was writing into `40-media/recordings/`.
- Object store: 181 210 packed objects in 6 packs (1.02 GiB), 476 commits, 403 loose; `.git/lfs` 4.8 GB / 23 487 objects (worktree holds the content — prune works); 72 641 index paths route to `filter=lfs`; 179 keeper-written `*.ext filter=lfs` rules. Weekly `gc` is an **external launchd job**, not keeper.
- Standing anomaly every hour: 600 files / 1.96 GB carried as plain blobs above the threshold — "history keeps them as blobs for good".
- `.gitattributes` lines 114–115 still carry two anchored `/…/ecojsLIB/.gitattributes filter=lfs` rules — the residue of the 2026-08-27 incident in which a `.gitattributes` was itself converted to an LFS pointer and gix logged **1 314 669 warnings** parsing pointer text as attributes. Nothing retires those rules.
- **588 408 `gix_worktree_state::checkout::chunk … collided (AlreadyExists)` ERROR lines** (2026-08-30 → 09-01) from `tgdrive-light`'s first checkout into a non-empty directory. The log is 895 MB with a 1.33 GB rotated sibling; keeper has no log rotation and no per-target filter.
- Repository config on all three: `core.ignorecase=true core.precomposeunicode=true index.sparse=false filter.lfs.*=<keeper.app>`. **Not set:** `core.fsmonitor`, `core.untrackedCache`, `index.version`, `index.skipHash`, `feature.manyFiles`.

---

## 2. How it works (the digest)

`Engine::run` (`engine.rs:2122`) ticks at 1 Hz. Each `tick` reads every profile from SQLite, runs due tasks, then **iterates profiles serially** (`engine.rs:2192-2202`). `tick_profile` (`:3361`): volume gate → per-profile reservation → arm/fold the watcher → first-checkout gate → `remote_within_reach` (skips only when the state is exactly `Offline`) → `scan_due` (paced 15 s ∨ watcher wake ∨ settle window elapsed) → `drain_journal`.

`drain` (`:5119`) claims up to 16 journal rows and executes them **serially and inline on the async worker**; if nothing is claimed and a scan is due it runs `scan_and_enqueue`: `materialize_pending` → **enqueue `Pull` unconditionally** → `commit_local` (status walk, stability gate, LFS staging, commit) → enqueue `Push` if committed or ahead. `Pull` = `commit_local` again, gix fetch in `spawn_blocking`, ancestry check, `git merge --ff-only` or `converge_with_conflict_copies` (conflict copies via `std::fs::copy`, then `git merge -s ort -X theirs -X no-renames`). `Push` = `commit_local` again, refuse while any `lfsUpload` row exists (`LfsUploadPending` → `deferred`), `git push --porcelain`. Every successful pull/push → `mark_synced` → `prune_lfs_store` (155 k-path plan) and, hourly, the release sweep.

The journal (`db.rs`) has four states — `pending`, `running`, `deferred`, `parked`; done is a `DELETE`. Transient failures back off 2 s → 600 s with full jitter and **no attempt ceiling** (by design: a dead host retries forever, ≈ one attempt per 5 min per row at the cap). The stability gate (`stability.rs`) is tier 0 (name/shape globs) + `SF_DATALESS` + tier 2 (two equal `(size, mtime, ctime, inode)` samples ≥ W apart) with a 60 s ceiling that force-commits a file that never quiesces; tiers 3 and 4 exist as functions but are not on the commit path (§3.5).

```mermaid
sequenceDiagram
  participant T as tick (1 Hz, serial over profiles)
  participant P as tick_profile
  participant D as drain
  participant W as status walk (inline, blocking)
  participant J as journal (sqlite)
  participant R as remote
  T->>P: volume_ready? reserve? ensure_watcher, fold_watch_events
  P->>P: first_checkout_is_unfinished? remote_within_reach?
  P->>D: drain_journal(scan_due)
  D->>J: release_satisfied_waits, claim_ready(16)
  alt nothing claimed & scan due
    D->>W: materialize_pending, repair_recorded_pointers, walk, gate, commit
    W->>J: enqueue Pull (always), Push (if committed/ahead), LfsUpload rows
  else units claimed
    D->>W: Pull: commit_local (walk again)
    D->>R: fetch (spawn_blocking, 20 s connect timeout, no read timeout)
    D->>W: mark_synced: prune_lfs_store (155 k index pass)
    D->>W: Push: commit_local (walk again)
    D->>J: LfsUploadPending -> deferred; else git push; complete(row)
    D->>J: on Err: reschedule(attempts, backoff) -> record_failure
  end
```

Three callers reach a full-tree walk: the commit leg (takes the walk claim, `commit_walk_policy`), the UI Pending poll (claim + 60 s floor), and `Engine::path_durability` (`engine.rs:4982`) — **no claim, no floor, dirwalk on**, called at 1 Hz by the recording banner (`src/hooks/use-recording-session.ts:183-187`). That third caller is the field cadence.

Full walk-through with a 22-row table of every cadence constant: [`lanes/engine.md`](review-sync-2026-09-08/lanes/engine.md). Per-entry walk cost table (what one walk does at 155 626 entries): [`lanes/scan.md`](review-sync-2026-09-08/lanes/scan.md).

---

## 3. Findings register

IDs reference the lane reports (`F-<lane>-<n>`), where the full evidence, quotes and fix text live. Effort: S ≤ half a day, M ≤ a few days, L larger.

### 3.1 P0

| id | finding | evidence | fix |
|---|---|---|---|
| **PULLPUSH-1** | **A modify/delete divergence livelocks the profile permanently.** `-X theirs` cannot resolve modify/delete; the merge exits 1 leaving `MERGE_HEAD`; every later `merge` and `merge --ff-only` exits 128 "you have unmerged files". No `MERGE_HEAD`/`--abort` handling anywhere in the crate; `SyncError::GitCommand` is `Transient`, so it retries forever with no user-visible cause. `write_tree_from_index` skips unmerged entries (`commit.rs:530-532`), so a commit taken over that index silently drops the contested path. `cli.rs:823-827` records this exact ending in the field once already ("the profile stopped syncing entirely"). Measured by the lane against the exact `merge_theirs_args` vector, incl. file/directory and case-only collisions (`core.ignorecase=true` on hesperia). | `cli.rs:331-339`, `:832-857`; `engine.rs:6486`; `error.rs:263-265` | On a failed `merge_theirs`/`merge_ff_only` run `git merge --abort`; refuse `commit_local` while `MERGE_HEAD` exists; then resolve modify/delete per AD-43 through `conflict::resolve`, which exists and is tested but has **no production caller** (PULLPUSH-2). Effort M. |

### 3.2 P1 — walk cost and cadence (the field symptom)

| id | finding | fix |
|---|---|---|
| **scan-1/2/3** | `path_durability` runs `git::repo::status_paths` = `WalkPolicy::read_only()` — **dirwalk on, stat write-back off, no `claim_walk`, no floor** — to answer one boolean about one path, at 1 Hz from the recording banner with no in-flight guard. This is ~600 of the 666 walks/hour on hesperia; walks pile up on the blocking pool (each ~60 MB of index state + 4 worker threads). | Answer from one `entry_by_path` + one `lstat` + the HEAD tree entry; or an include pathspec `:(literal)<rela>` with `find_untracked:false`; take the claim; guard the client poll. Effort S. |
| **engine-5 / GATE-4** | A watcher wake sets `scan_due` on every tick while a file is being written; the only pacing on event-driven walks is the 1 Hz tick (the UI poll has a 60 s floor, the commit leg none). A continuously-written recording buys a 155 k-entry walk per second to learn the gate already holds the answer (`next_stable_ms`). | Do not set the wake for a path the gate already tracks with a future deadline; give event-driven walks a floor of `min(settle_ms, 5 s)`. Effort S. |
| **scan-5/6** | The watcher's path list is discarded into a per-profile boolean (`watch_wake: HashSet<String>` of profile ids, `engine.rs:1110`, `:4151`). `status_paths_excluding` already builds `:(exclude,literal)` pathspecs and gix narrows the index range by binary search and prunes the dirwalk for include pathspecs (`gix-status/.../function.rs:92-94`, `gix-dir/src/walk/mod.rs:235-237`). | Keep a bounded per-profile path set (cap ~4 096, overflow ⇒ full walk) and pass it as include pathspecs. **This is the fsmonitor optimisation, at the layer keeper controls.** Effort M. |
| **engine-3** | The supervisor's walk, repo opens, `prune_lfs_store` and the first clone run inline on the async worker, and profiles are ticked serially — one folder's 61 s walk, 20 s connect timeout or 2 GB upload stalls the other two. Module doc `engine.rs:17-21` claims the opposite. | Fence in `spawn_blocking`; drive `tick_profile` per profile concurrently (the per-profile `busy` reservation already makes it safe). Effort M. |
| **engine-4 / LFS-9 / db-13 / scan-11** | Per-pass work constant in tree size: every scan runs the pointer-repair window, `materialize_pending`, a `git merge-base` spawn and enqueues a fetch; every successful pull/push runs a 155 k-path prune plan that re-opens the index **per path** (`stage.rs:963-967`); the hourly footprint anomaly is a second 155 k `lstat` sweep to re-report a number it calls permanent. | Pull on its own 5-min cadence; prune only after an LFS unit completed; hoist one index snapshot; memoise the footprint on `(HEAD, threshold)`. Effort S–M. |

### 3.3 P1 — offline / failure handling

| id | finding | fix |
|---|---|---|
| **engine-1 / db-1** | The fetch failure text `tcp connect error: deadline has elapsed` matches none of the ten substrings in `cli.rs:1109-1120`, so it becomes `SyncError::Git`, not `Network` → the profile never enters `Offline` → `remote_within_reach` (`engine.rs:3504`) never gates → six days of full walks and three profiles each paying a 20 s connect timeout, serially, while the folder pane said "Watching". | Classify by error *type* (reqwest `is_connect()/is_timeout()`, `io::ErrorKind`) at the gix boundary; keep substrings as fallback and add these. Add a test with the exact hesperia string. Effort S. |
| **engine-2 / db-2** | `transient_failures` is incremented in `record_failure` inside `drain`, which returns `Ok`, and `tick` (`engine.rs:2198-2200`) clears it on `Ok` — the same tick. `TRANSIENT_FAILURES_BEFORE_WARNING=3` is unreachable for journaled units; the only test calls `record_failure` directly. No escalation, no `NeedsAttention`, ever. | Reset on unit success, increment on unit failure; a test that drives `tick()` three times. Effort S. |
| **PULLPUSH-3 / Deps-2** | gix's fetch transport sets only `connect_timeout(20 s)` (`gix-transport/.../reqwest/remote.rs:63-65`); no read timeout, and `do_pull` wraps it in a bare `spawn_blocking`. A peer that accepts TCP and goes silent parks the profile forever. `docs/sync.md` §11's "the only place a client is built" is false for this leg. | `tokio::time::timeout` around the fetch that sets `interrupt`; set `gitoxide.http.connectTimeout`; fix §11. Effort S. |
| **PULLPUSH-4** | No `git` invocation has a wall-clock bound (`cli.rs:617` `command.output()`), no `http.lowSpeedLimit`; stdout/stderr accumulate unbounded before `STDERR_CAP`. | Deadline + kill; `-c http.lowSpeedLimit=1000 -c http.lowSpeedTime=60`. Effort S. |
| **db-3** | The offline transition is logged at `debug!` (`engine.rs:5318`) and both subscribers filter at `info` — the shipped Mac cannot say when a folder went offline. | Log the transition once at `info`. Effort S. |

### 3.4 P1 — data integrity and safety

| id | finding | fix |
|---|---|---|
| **LFS-3** | `lfsPruneLocal` (default **on** since 0.8.12) deletes the local object copy on two conditions — worktree holds content, journal owes no upload — and **never asks the remote**, although `prune.rs:38-42`, `docs/sync.md` §8 and §20 all say it does. `audit.rs:20-26` records the gate this relies on failing in the field ("16 objects, 8.0 GB, missing on the server while both folders reported a clean sync"). After prune the worktree file is the only copy. | Restrict prune to oids with a `synced_at_ms` memo (already written by `note_unit_synced`), or batch-probe the remote before deleting; correct the docs. Effort M. |
| **LFS-1 / LFS-2 / scan-13 / VF-6** | The git-control-file guard (`GIT_CONTROL_FILES = [.gitattributes, .gitignore, .gitmodules]`, `stage.rs:110`) is consulted only on the **size** routing path; `already_routed` (attribute path) has no guard, so a stale rule like hesperia's lines 114–115 re-converts a `.gitattributes` to a pointer the next time that path exists. `.lfsconfig`, `.keepervirtual`, `.keeper/*.toml` are not protected at all (one oversized `.toml` anywhere writes `*.toml filter=lfs`). A pointerised `.keepervirtual` parses as three legal globs and authorises nothing. `ensure_attributes` only appends; nothing retires a keeper-written rule. | One shared control-file predicate (`virtual_policy::is_control_file` is the complete one) applied at both routing gates and in the repair sweep; retire keeper-written rules that resolve to control files; refuse a `.keepervirtual` that parses as a pointer; an anomaly line for "a control file in this tree is pointer text". Effort S+M. |
| **GATE-1** | No volume re-check between the status walk (p90 5.3 s, max 61 s) and the commit; a detach mid-walk reads every tracked path as deleted, and the only mass-deletion guard (`commit.rs:191`) fires solely for an empty index. *Blast radius is bounded* because `.git` sits on the same volume, so the commit itself fails once the mount is gone — the risk window is a detach + re-attach within one walk, or the mount point being replaced. Rated P1 rather than the lane's P0 for that reason. | Re-assert the marker after the walk for a `removable` profile; refuse a commit whose deletions are a large fraction of the index on removable media. Effort S. |
| **GATE-2 / GATE-14** | Tiers 3 (`open_writer_veto`) and 4 (`verify_while_reading`) have **no caller on the commit path**; non-LFS staging is a bare `std::fs::read` (`commit.rs:241`). `docs/sync.md` §4 and `stability.rs:16-19` call tier 4 "the proof" and the reason the other tiers may be approximations. Exposure: files under the threshold and the `lfsNever` set, force-committed by the 60 s ceiling. | Route non-LFS staging through `verify_while_reading`; call `open_writer_veto` on Linux; or delete the dead tiers and correct the doc. Effort M. |
| **GATE-3 / GATE-8** | `declare_settled` backdates both clocks past the ceiling and `observe` never resets `pending_since_ms`, so a "finished"/primed path is `Stable` for its whole life, whatever is written after the assertion; `forget_all` (documented for detach/pause) has no production caller, so after hesperia's 09-07 re-attach every held entry was instantly past the ceiling. | Reset `pending_since_ms` on a changed sample. One line; fixes both. Effort S. |
| **PULLPUSH-5** | With no stored credential, `fetch` installs no callback and gix falls through to the machine's `credential.helper` chain — "authenticated as somebody else" per `repo.rs:548-556`; on removable media under `Trust::Full` a repo-scope `credential.helper=!cmd` is executed. `clone` and the CLI shim both clear the chain; fetch does not. | Open for fetch with an in-memory `credential.helper=` override and install `static_credential` unconditionally. Effort S. |
| **PULLPUSH-2** | `conflict::resolve` and the whole AD-43 matrix (deletion never beats modification) have zero production callers; behaviour is whatever `-X theirs` does. `docs/sync.md` §5 states the matrix as shipped behaviour. | Drive the matrix from the three `diff --name-only` sets `converge` already computes. Effort M (with PULLPUSH-1). |
| **PULLPUSH-6** | The phone's push uses the 60 s read-timeout client and buffers the whole pack in a `Vec`; a first push packs the entire history with no deltas. The identical failure was diagnosed and fixed for LFS (`basic.rs:700-705`). | Use `transfer_http` and a sized streaming body. Effort M. |
| **VF-1** | On macOS `probe_open_file_state` is constant `Unknown` and `release_resolved` refuses `OpenUnknown`, so nothing ever releases — while `release_schedule` has no platform term and the Files pane counts down "23 hr … keeper lets this content go on the first sync after the time runs out". This is `tgdrive-light`'s exact configuration. Already noted in `epic-69…md:12`. | A `Held` schedule word naming the platform, probed once per call; add it to the refused-holds set. Effort M. |
| **db-4** | `enqueue_unique` dedups on an **unindexed** TEXT `payload` (`db.rs:2009-2013`); the smudge loop calls it per object. A first materialize of 100 k objects ≈ 5 × 10⁹ row comparisons under the connection mutex on a tokio worker. | Index `(profile_id, payload)` or a `dedup_key` column. Effort S. |

### 3.5 P1 — specification and status

| id | finding |
|---|---|
| **BMAD-1** | NFR-23 is 100 000 files / **50 GB** (`epics.md:3519`); the only measured row is 100 000 files / **393 MB** on Linux SSD (`sync.md:2988`); the real folder is 155 626 entries / ~450 GB on USB APFS. Never re-authored; `docs/performance.md` has no sync row at all (BMAD-7). |
| **BMAD-2** | No requirement bounds walk *cadence*; `sync.md:2996-2998` ("a folder that is not changing is cheap to keep watched") is the assumption the field contradicts. The 1-to-5-second walk is a specification gap, not a violated one. |
| **BMAD-3 / engine-13 / PULLPUSH-9** | Research §12 Q6 "when does `git gc` run?" was never answered; `GitCli::gc` and the three `worktree_*` verbs have no production caller; `cli.rs:302-304` claims gc "is the only thing keeping the object store bounded". AD-41's five shim verbs are three. |
| **BMAD-4** | `docs/sync.md` §18 asserts §§1–8/10/11/13 "verified against real git remotes, including a full LFS round trip against a local LFS server and the review-lane airlock"; `tests/lfs_roundtrip.rs:10` says "No network", no harness exists, no test touches `OpenPullRequest`, and story **31-4 Live Forgejo Integration is still `in-progress`** while 31-6 (phase acceptance) is `done`. |
| **BMAD-6 / PULLPUSH-9** | AD-50's "linked worktree" lane is a `git switch -c` in the user's own checkout (`engine.rs:5784-5796`); agent and human share one working tree; nothing prunes. No artifact records the change. |

### 3.6 P2 (grouped; full text in the lanes)

**Walk / index** — scan-4 every walk also runs a HEAD-tree↔index diff no caller asked for (`gix::status` default, no off switch); scan-7/8/9 `UNTR`/`FSMN` decoded-and-ignored, dropped on write-back, v4 downgraded, `index.skipHash` unset (one line); scan-10 macOS builds two 155 k icase tables + NFC per dirent per walk (why 0.76 s Linux → 1–9 s Mac); scan-12 the "996 s dirwalk" figure that justifies `WalkPolicy` is contradicted by the field (p50 1.08 s with the dirwalk on) — re-measure before trusting the 900 s untracked-sweep interval; Deps-3 a fresh `gix::Repository` per pass discards gix's index snapshot cache; GATE-6 the 26 MB index is parsed inside the process-wide `gates` mutex per busy tick; engine-10 blocking I/O and `platform.notify` under that same lock; engine-12 two independent 15-min backstops each force the dirwalk; engine-6 a permanently refused path (the 6.4 GB truncated `.gz`) re-enters the gate every walk; GATE-13 the walk log line omits `untracked`/`needs_update` so "entries=26" is unexplainable.

**LFS** — LFS-4 one batch POST + one connection **per object**, the 8-way window and 100-object batch never used (this is the 100 k-object answer); LFS-5 no `fsync` before an object is published and `contains` trusts name+size; LFS-6 `stage::clean` copies the full content into the store even when the object exists (the filter path already has the two-question shape); LFS-7 `lfsNever` cannot undo an existing rule and `ensure_lfs_rule` ignores it; LFS-8 a failed *clean* under `required=false` stores raw bytes as a blob, permanently; LFS-10 nothing ever collects an unreferenced object from `.git/lfs`; LFS-11 a repair batch suppresses the user's own commits for that pass (~147 passes on the measured backlog); LFS-12/13 per-request watchdog thread; 64 KiB memset per upload frame.

**Git protocol** — PULLPUSH-7 `Trust::Full` honours every repo-scope `filter.*` driver; only `lfs` is sanitised; PULLPUSH-8 every commit rewrites the whole index and every directory tree (gix has no exists-check on write); PULLPUSH-10 staging applies exactly one filter (LFS), so any peer-committed `text=auto`/`ident` rule makes a path permanently dirty; PULLPUSH-11 conflict copies are untracked until a later pass, overwritable within a second, lossy for non-UTF-8 names; PULLPUSH-12 never shallow, never partial, first clone takes every branch (1.02 GiB packs + 4.8 GB LFS for a tip); PULLPUSH-13 file/dir and case-only collisions hit PULLPUSH-1.

**Virtual files** — VF-2 macOS sweep hashes up to 1 GiB and calls the server before the check that always refuses; VF-3 a renamed/deleted path is an immortal sweep candidate; VF-4 repo open + 26 MB index parse per candidate, ×32 per pass; VF-5 a `.keepervirtual` HEAD carries but the worktree lacks is read as "no policy" (the tgdrive-light half-checkout shape); VF-7/db-7 `materialized` unbounded (89 289 rows) and `delete_profile` leaves it behind, non-transactional.

**Durable state / logging** — db-5 the whole ledger is read on the tokio runtime per Files browse; db-6 ledger writes are one auto-commit + one SQL compile per path, `prepare_cached` used nowhere; db-8 no log rotation, library WARN/ERROR written even with debug off — `gix_attributes=error,gix_worktree_state=error` is one line; db-9 migration is a read-then-`ALTER` race between app and daemon; db-10 `[INFERENCE]` a `file://` remote can put a deferred push into a 1 Hz loop; engine-8 the notes push re-runs `sync_once` every 30 s offline and `sync_once` walks up to four times per pass; engine-9 `wake_now` resets the hourly footprint sweep; engine-11 shutdown cannot interrupt a tick.

**Watcher** — GATE-5 a watcher that dies or goes deaf is never detected (the `Err` batch is one `warn!`); GATE-7 `prime_moved_paths` never sets `untracked_appeared`; GATE-9 notify reports canonical paths, keeper strips the configured root — a symlinked root breaks anchored excludes and forces `full()` every batch; GATE-10 a nested repository is skipped forever with a log line the user never sees; GATE-11 `EventKind::Other` never reaches `classify` (the debouncer drops it first) so the "macOS vague kinds" test is unreachable; GATE-12 FSEvents' persistent journal (`sinceWhen`) is the largest available win for start-up and `notify` cannot expose it.

**Dependencies** — Deps-1 retire the fork at gix-filter ≥ 0.34.1; Deps-5 no `tcp_keepalive`; Deps-6 rusqlite's default `NO_MUTEX` is load-bearing and undocumented; rusqlite 0.37 → 0.40 (SQLite 3.50.2 → 3.53.2) is routine.

**BMAD** — BMAD-8 125 engine tests silently skip without `git` (DW-146 open); BMAD-9 the NFR-26 "log-scan test" credited by the phase acceptance does not exist; BMAD-10 no sync decision in either decision register; BMAD-11 FR-80 has no path for content already in history; BMAD-12 21 open ledger entries, several of them the field's symptoms (DW-130, DW-134, DW-137); BMAD-12b zero story specs for the 61 shipped stories of epics 23–31; BMAD-13 fsmonitor/untrackedCache never considered, so never rejected with a reason; VF-10 stories 56.16/56.17 shipped without a sprint-status row.

### 3.7 P3

Doc drift: §3 order (fetch first vs commit first), §7 linked worktree, §11 "only place a client is built", §18 understates the shipped §12 surfaces, `sparse-checkout init|set|reapply` vs `set|disable`, "tier 3 … optionally open file descriptors", three docstrings claiming indexes that do not exist (db-11), `PRAGMA foreign_keys` a no-op (db-12), `footprint::measure` "sub-second" on a 573 GiB tree (db-14), VF-8 `verify`'s folder gate asks `tier()!=Unset` where every other gate asks `authorizes_anything()`, VF-9 `subpaths[]` and `.keepervirtual` are two levers and the doc should say so, BMAD-15 the removable-media trust answer is a checkbox nobody is told is load-bearing, PULLPUSH-15 two clones on one Mac share nothing and there is no local peer path (measured: 7 days apart).

---

## 4. What is correct and well done

Recorded so the register above is read in proportion — most of the hard parts are right, and several are better than the prior art the research surveyed.

- **Journal-as-plan.** `enqueue_unique` with running-cover for content-keyed units; `recover_running` at open and finalize; `deferred` (condition) vs `pending` (clock) vs `parked` (human) kept distinct; upload debt journaled **before** the commit; a push refuses behind any upload row including parked ones. Backoff is pure, jittered downward so the cap is a true maximum, exponent clamped at 32, all tested.
- **Task leases** are one conditional `UPDATE` with the claim, the abandonment and the run insert in one transaction; two hosts cannot both claim.
- **Durability (NFR-24)** is satisfied with evidence: a deterministic per-boundary grid plus a real-SIGKILL sweep that found two production repairs.
- **The pointer encoding** is spec-conformant and canonicality is *derived* by re-rendering, so a non-canonical pointer another client wrote is passed through byte-for-byte rather than re-encoded (the "modified forever" trap). pkt-line framing and the long-running filter conversation — including the empty second status list on success — match git's documented exchange; `delay` is deliberately not advertised; smudge never fetches.
- **Upload body** is an exact-`Content-Length` streaming `http_body::Body` with a progress watchdog that bounds *silence*, not duration (the reason a 2 GB upload on a slow tailnet can still finish); download resume digests every byte including the ones not fetched this time; verify-after-upload is unconditional; the Forgejo quirk list is pinned by tests.
- **The gitoxide fork** is two commits, tests-first, with the manifest block documenting the retire procedure; the reaping proof reads the real process table.
- **`WalkPolicy::persist_stats`** is the right fix for the right defect (60 280 stat-less entries / 25.4 GB re-hashed per pass → gone); `tracked_only` is a genuine optimisation with a sound correctness argument; `claim_walk` is the right shape; `STATUS_THREAD_LIMIT=4` is enforced by a `const` assertion; `restore_missing_checkout` pre-clears with `SKIP_WORKTREE`, which *is* the fix for the 588 k collided errors.
- **The watcher** checks `need_rescan()` before the kind match and before `paths` (the bug almost every integration ships); `NoCache` is named rather than inherited (avoids a `WalkDir` of 154 765 entries at arm time); `retire` never blocks; tier 0 filters the wake, not only the walk, against the same compiled set.
- **Removable media**: marker existence not parse, corrupt marker is an error not an overwrite, newer schema preserved verbatim, atomic write with `sync_all`; volume swap at the same path is `Foreign` → `NeedsAttention`.
- **Virtual files**: the release proof is a fresh per-object batch call at the moment of deletion — `synced_at_ms` only selects which clock applies and authorises nothing, so with the remote down every candidate refuses `UnprovenOnRemote`; a rename cannot release the wrong path; `verify` excuses virtual on four independently-earned facts; the eight `EntrySyncStatus` variants map one-for-one onto the VM.
- **Git hygiene**: every invocation is an argv, `safe_ref`/`absolute_arg` refuse option-shaped arguments, `core.hooksPath=/dev/null/…`, `GIT_TERMINAL_PROMPT=0`, secrets via environment not argv, `scrub_userinfo` on every line; `-X no-renames` with the 138 311-unmerged-paths measurement that motivated it; ancestry instead of a `fast_forward` bit; one bounded reconcile-and-retry so ≤3 clients cannot ping-pong; `push_http` enforces no-force client-side.
- **Anomaly discipline** (measurement, expectation, consequence, at WARN) is why the field log was readable at all.

---

## 5. Will it choke at 150 k files / 100 k LFS objects / 450 GB?

**Memory** — a walk is ≈ 26 MB mmap + ≈ 29 MB decoded index + 2 × 4 MB icase tables (macOS) ≈ 60 MB, ≈ 90 MB with write-back; per *concurrent* walk (scan-2). Fine, unless the 1 Hz poll piles them up.

**CPU / syscalls per walk** — Θ(entries), not bytes: 150 k `lstat` + 150 k stat compares + one SHA-1 over 25 MB (twice on write-back) + a HEAD-tree diff (~10⁴ ODB reads) + on macOS two icase tables and NFC per dirent. Pointers cost nothing extra; 450 GB costs nothing while stats match, and one full read through the LFS clean filter per *dirty* path. Warm ≈ 1 s on hesperia; the max of 61 s is contention with the recording.

**What does not scale today**

| term | today | at 100 k LFS / 450 GB |
|---|---|---|
| walks per event | full tree per wake, per 1 Hz durability poll | same — proportional-to-change is the fix (§6 step 3) |
| profiles | serial; one stalls all | worse with more profiles |
| first materialize | 1 batch POST + 1 GET **per object** (LFS-4) | 100 k round trips; RTT-bound, not bandwidth-bound |
| first upload | 1 object at a time, non-resumable (protocol), 90 s stall watchdog | 450 GB serial; ~100 k × RTT overhead; a failed 2 GB object restarts from zero — inherent to `basic` |
| queueing | `enqueue_unique` unindexed payload (db-4) | O(n²) at 100 k rows |
| commit | whole index (26 MB) + every directory tree rewritten (PULLPUSH-8) | ∝ directories, not change |
| `materialized` ledger | 89 289 rows, never pruned, whole-ledger read per browse | grows with paths-ever-hydrated |
| history | 1.02 GiB packs, 1.96 GB of blobs permanently over threshold; no gc caller | grows; cron only |
| first clone | full history + all heads + all LFS (no shallow, no partial) | 1 GiB packs + 450 GB |
| log | no rotation; library floods (1.9 M lines) written with debug off | unbounded |

**Verdict:** no hard wall at 150 k / 450 GB; the engine already runs there. The bar it was built to (NFR-23: 100 k / 50 GB) is simply the wrong bar, and every steady-state statement in the docs is about a smaller machine. With §6 steps 1–4 the steady state becomes seconds per *change* instead of seconds per *second*.

---

## 6. Roadmap (ranked by benefit ÷ effort)

1. **Stop the 1 Hz full-tree walk** (scan-1/2/3) — S. Removes ~600 walks/hour on hesperia. Answer `path_durability` from one index lookup + one `lstat`; take the claim; guard the client poll.
2. **Classify the fetch timeout as `Network`** (engine-1/db-1) and **fix the failure counter** (engine-2/db-2) — S. Offline becomes visible and the offline gate finally engages.
3. **Feed the watcher's paths into the walk as include pathspecs** (scan-5/6) with a bounded set and overflow ⇒ full walk — M. Steady state ∝ change set. This is the fsmonitor keeper already has.
4. **`index.skipHash=true`** beside `index.sparse` (scan-8); **log-target filter** `gix_attributes=error,gix_worktree_state=error` plus rotation (db-8); **`merge --abort` + `MERGE_HEAD` precondition** (PULLPUSH-1) — S each.
5. **Fence blocking work and tick profiles concurrently** (engine-3) — M.
6. **Cadence**: pull on its own 5-min clock, `pollIntervalMs` 300 s when the watcher is live, prune only after an LFS unit completed, event-driven walk floor (engine-4/5) — S.
7. **Control-file predicate unification + rule retirement + pointerised-control-file anomaly** (LFS-1/2, scan-13/14, VF-6) — S+M.
8. **Prune only what the remote is known to hold** (LFS-3) — M; **batch LFS units into one `do_lfs`** (LFS-4) — M.
9. **Timeouts on gix fetch and `git` CLI; credential-helper override on fetch** (PULLPUSH-3/4/5) — S.
10. **Virtual files on macOS**: `Held` schedule word, cheap `OpenUnknown` pre-check, retract `NotTracked`, one index per sweep (VF-1..4) — S/M.
11. **Maintenance**: a `Gc` task kind on the existing scheduler; `materialized` ageing; `delete_profile` cleanup (BMAD-3, VF-7, db-7) — S.
12. **Docs and ledger**: re-author NFR-23 (200 k / 500 GB / 100 k LFS on removable APFS), add a cadence NFR, downgrade §18 to what is proven, record the store decision (§7) as `docs/decisions.md` D-18, fix the drift list in `lanes/bmad.md` — S.
13. **Upstream / longer**: `UNTR`/`FSMN` write-through and v4 in `gix-index`; `Platform::no_head_tree()`; a read timeout or client injection in `gix-transport`; FSEvents `sinceWhen` in `notify` — L, only if foreign `git status` speed or restart cost become goals.

---

## 7. Is git the right store for "files, not code"?

**What was decided.** Nothing. `epics.md:3471-3474` enters git as an owner premise ("synchronize a local folder with a server over the git protocol, built on gitoxide"); the research (`research-sync-2026-07-25.md`) scopes itself to *how*, surveys Syncthing/Nextcloud/rclone/gut-sync/Dropbox for **file-stability mechanics**, and rejects Syncthing's BEP *given* git. git-annex, DVC and restic appear a month later as pointer-design prior art, not candidate stores. The one open question that bears on it — §12 Q6, history growth and `gc` — was never answered. There is no D-entry for the store.

**What the field says about git's cost model here.** The folder is 99.99 % static and 0.01 % append-heavy. git's per-operation cost is a full index ↔ worktree comparison per pass and a full tree rewrite per commit. That is the wrong shape *if* every event pays it — which is what keeper does today — and the right shape once the pass is scoped to the change set, which git itself achieved with fsmonitor and the untracked cache, and which keeper can achieve with the watcher path list (§6 step 3). In other words: **the field cost is keeper's walk policy, not git.**

**Where git is genuinely the wrong shape, and what it costs**

| shape | cost | mitigation inside git |
|---|---|---|
| per-object LFS transfer round trips | 100 k × RTT on first materialize/upload | batch units (LFS-4); protocol allows 100 per batch |
| content already in history as blobs | 1.96 GB permanent, ~2× the pack store | threshold discipline + an opt-in `migrate --above` rewrite (destructive, documented) |
| no partial/shallow clone in keeper | 1 GiB packs + 4.8 GB LFS to reach a tip | expose the plumbed `shallow`; evaluate gix `blob:none` |
| no local peer path | two clones on one Mac 7 days apart with the remote down | optional second remote; a `file://` fetch already works |
| history growth | 476 commits / 181 k objects / 1.02 GiB; gc by cron | a `Gc` task kind |

**Alternatives, honestly**

- **Object store (rustfs / S3-like) as the tree.** Solves nothing keeper finds hard — the completeness gate, the journal, conflicts, offline, provenance — and removes what makes the folder useful to a person and to agents: a browsable checkout with history that any `git` can read. You would still need LFS-shaped content addressing and a metadata layer; you would have to write the server.
- **PostgreSQL (server) + SQLite (client) metadata, content-addressed blobs.** This is the Dropbox/Syncthing shape: rows diff, cost ∝ change by construction, conflicts by version vectors. It is the right architecture for a *product* with many users; for one owner with ≤3 clients it means owning a server, a protocol, a migration and a conflict model, and losing the DAG, the trailers, the `git log --follow`, the agent airlock and Forgejo. The research already rejected version vectors for a stated reason (`research-sync…:216-221`) — the reason still holds.
- **Syncthing-like P2P.** Would have converged the two Mac clones while the remote was down. Costs history and the single global model. Worth stealing exactly one idea: a second, local remote.

**Verdict.** Keep git + LFS as the store. Keep SQLite (`sync.db`) as the local "what changed / what is owed" truth — it already is. Fix the cost model in keeper (walk ∝ change, batch LFS, concurrent profiles, gc). Record this as a decision with the falsifiable assumptions above and three revisit triggers: (a) a folder with > 10⁶ entries, (b) > 3 concurrent writers on one remote, (c) a first-materialize that must finish in minutes rather than hours.

---

## 8. Verification notes and open questions

- The conflict matrix in `lanes/pullpush.md` was **measured** against keeper's exact `merge_theirs_args` vector in throwaway repositories; the auto-gc claim likewise.
- The 1 Hz caller attribution (scan-1) was verified by reading `engine.rs:4982` and `use-recording-session.ts:183-187`; the arithmetic (60 poll + few commit + ~600 durability ≈ 666/h) is consistent with the log but the log does not name the caller — adding a `caller` field to `status walk finished` closes that permanently (GATE-13).
- Not settled from here: what the alternating `entries=26/65/0` are (most plausibly `NeedsUpdate` on LFS-tracked racily-clean paths); whether tokio's blocking pool actually accumulated concurrent walks; the warm cost of the dirwalk on the USB volume (the "996 s" figure vs p50 1.08 s with the dirwalk on); whether `/Volumes/merope/.fseventsd` is functional; whether any hesperia folder has ever reached the release sweep; git's exact `status=abort` semantics for the clean direction (LFS-8).
- No code was changed by this review. No cargo command was run (the shell crate does not compile on this Linux box).
