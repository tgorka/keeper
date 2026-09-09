---
title: 'Story 70.4: what must never become a pointer, and what must never be pruned'
type: 'feature'
created: '2026-09-09'
status: 'review'
baseline_revision: '3cb3fd7'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/epic-70-a-pass-that-costs-what-changed-and-a-folder-that-says-when-it-is-cut-off.md'
  - '{project-root}/_bmad-output/planning-artifacts/review-sync-2026-09-08/lanes/lfs.md'
---

binds: FR-521, FR-522, FR-523 → AD-230, AD-231. Findings F-LFS-1, F-LFS-2, F-LFS-3, F-LFS-7, F-LFS-9, F-scan-13, F-scan-14, F-VF-6.
depends on: nothing in wave A. Shares `engine.rs` by function with 70.1/70.2/70.3/70.5 (see the epic's ownership table).

<intent-contract>

## Intent

**Problem:** Four guarantees the crate already wrote down are held at one door each and open at the others. `GIT_CONTROL_FILES` (`stage.rs:110`) guards the *size* path only: `already_routed` (`:1443`), `mismatched_filtered_paths` (`:1493`) and `unconverted_after_repair` (`:1582`) route a `.gitattributes` the moment a stale anchored rule says `filter=lfs` — hesperia's `.gitattributes` still carries two such rules from the 2026-08-27 incident (1 314 669 gix warnings). `.lfsconfig`, `.keepervirtual` and `.keeper/*.toml` are protected from *virtualization* (`virtual_policy::is_control_file`) and not from *conversion*; `*.toml filter=lfs` — written by one oversized TOML anywhere — makes `.keeper/keeper.toml` `already_routed`. `lfsNever` is read in exactly one place (`LfsPolicy::from_profile`), so an existing rule keeps routing every `.md` however small and `ensure_lfs_rule` writes a rule for an extension the profile opted out of. `VirtualPolicy::compile` parses pointer text as three legal globs. Nothing detects a control file that is *already* pointer text — the machine announces it only as a million warnings. And `lfsPruneLocal` deletes the second local copy on every successful pull with conditions 1 and 2 of three (`prune.rs:34-39` leaves the remote's word "to the caller"; `engine.rs:9112` stops), while `audit.rs:20-26` records that premise failing in the field (16 objects, 8.0 GB missing on the server under two clean folders). `prune::plan` also re-opens the index once per tracked path (`indexed_pointer`, F-LFS-9).

**Approach:** ONE predicate, `lfs::stage::is_control_file`, is the union of both lists (`.gitattributes`, `.gitignore`, `.gitmodules`, `.lfsconfig`, `.keepervirtual`, any `.git` or `.keeper` component), and every routing door consults it: `LfsPolicy::excludes` (control file OR `lfsNever`) is asked by `applies`, by `already_routed` (now takes the policy), and — for control files — by the two repair functions. `ensure_attributes` takes the policy and retires a keeper-written managed-block rule that `LfsPolicy::retires` names: an exact-path rule whose path is a control file, or a `*.ext` rule where `.ext` is a control-file name or where `lfsNever` matches `a.ext`. User lines above the marker are never read for retirement; the rewrite goes through `repair_managed_block`'s existing keep-vector so a file with nothing to retire is byte-identical. `ensure_lfs_rule` builds the policy and refuses before writing. `VirtualPolicy::compile` refuses text `Pointer::parse` accepts with the unreadable arm's `SyncError::Config` shape, naming the file. The anomaly rides `report_blobs_over_threshold` (hourly, already holds `tracked` inside `spawn_blocking`; the commit leg would need one index read per pass to learn the same ≤ handful of paths, and 70.6 memoises the footprint number but not this check, which runs first). Prune takes `db::synced_oids` (rows with `synced_at_ms` and an `oid`) as a third input and `plan` refuses everything else; `mark_synced` calls it only when `lfs_moved` holds the profile (set by `note_unit_synced`, consumed by the prune) or when the hourly `next_prune_ms` look is due; `plan` hoists one index read and walks `entries()` directly.

**Why the sentence, not the deletion, moves for prune.** F-LFS-3 offered a batched `download` existence probe per prune. That is one round trip per 100 objects on every success edge, and it is the shape `remote_serves` already takes at the moment of deletion for *release*; prune is not release — it deletes a copy the worktree can rebuild — so a memo the remote already gave (the upload's own completion, or an audit's per-object `serves`) is the right proof, and `docs/sync.md` §8/§20 now say exactly that and no more.

## Boundaries & Constraints

**Always:**
- `is_control_file` matches on the file name at any depth and on any `.git`/`.keeper` path component; a file that merely *contains* the name (`.gitattributes.bak`) is content.
- `already_routed` answers `false` for a path `LfsPolicy::excludes`; `mismatched_filtered_paths` and `unconverted_after_repair` skip control files before the attribute question.
- Retirement touches only lines below `MANAGED_HEADER` that carry keeper's exact `ATTRIBUTE_SUFFIX`; the user's lines are copied through byte for byte, including their line endings.
- `ensure_lfs_rule` refuses an extension `ext` where `.{ext}` is a control-file name (git's `*` matches the empty string, so `*.gitattributes` would route `.gitattributes`) or where `lfsNever` matches `a.{ext}`; both are `Ok(false)` with a `warn`, like every other refusal there.
- `compile` refuses pointer text before the BOM strip and before parsing; an empty file is not pointer text (`is_pointer_candidate` first, then `Pointer::parse`).
- The anomaly carries the four fields; `measured` names the path and the subtree it governs (`.gitattributes`/`.gitignore` → their directory; the rest → the whole folder).
- `plan` releases only an oid in `synced`; a row with `synced_at_ms` and `oid = NULL` confirms nothing.
- `prune_lfs_store` runs on a `mark_synced` only if an LFS upload unit completed for that profile since the last prune, or the hourly look is due; a pull that moved no unit runs no plan (`EngineCounters::lfs_prune_plans` unchanged).

**Block If:** nothing — the epic fixed every shape; `repair_managed_block`'s keep-vector and `note_unit_synced`'s stale-unit guard answered the two design questions (how to retire without reformatting, and why `(oid, synced_at_ms)` is a consistent pair).

**Never:** touch a line above the marker; touch `WalkPolicy`, `stability.rs`, `commit.rs` (70.1/70.5); make `keeper-sync` depend on `tauri` or `keeper-core`; run prune on a profile whose ledger has no confirmation; run `cargo fmt` on the crate.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Predicate | `.gitattributes`, `a/b/.gitignore`, `.gitmodules`, `.lfsconfig`, `.keepervirtual`, `.keeper/keeper.toml`, `.git/config`, `x/.keeper/host.toml` | `true`; `docs/.gitattributes.bak`, `notes.md`, `keeper.toml` at root → `false` | none |
| Attribute door | stale anchored `/a/b/.gitattributes filter=lfs` below the marker; commit of `a/b/.gitattributes` (1 KiB) through `stage_and_commit` | blob is the text; managed rule gone; user lines above marker byte-identical | none |
| Extension door | `*.toml filter=lfs` present; `.keeper/keeper.toml` 2 MiB over threshold; `.lfsconfig` and `.keepervirtual` likewise | all three committed as blobs of their own bytes; `*.toml` rule kept | none |
| Repair sweep | index carries raw `a/.gitattributes` under a `filter=lfs` rule | not in `mismatched_filtered_paths`, not in `unconverted_after_repair` | none |
| `lfsNever` on the attribute door | `*.md filter=lfs` rule; `lfsNever=["*.md"]`; a 100-byte `.md` | not routed; the `*.md` managed rule retired on the next write | none |
| `ensure_lfs_rule` | ext `gitattributes`; ext `md` under `lfsNever=["*.md"]` | `Ok(false)`, no write, one `warn` each; `attribute_writes` unchanged | refusal is the handling |
| `compile` | `.keepervirtual` bytes = a rendered pointer | `Err(SyncError::Config(..))` naming the file | the refusal IS the handling |
| `compile` | empty `.keepervirtual` | `Ok`, `Unset` (unchanged) | none |
| Anomaly | tracked `a/.gitattributes` whose worktree bytes are a pointer | `pointerised_control_files` → `[a/.gitattributes]` with subtree `a`; the anomaly line names both | none |
| Prune, no memo | committed LFS path, object stored, no `synced_at_ms` | plan empty | none |
| Prune, memo | same, `materialized` row with `oid` + `synced_at_ms` | plan holds the oid | none |
| Prune, memo without oid | `synced_at_ms` set, `oid NULL` | plan empty | none |
| Prune cadence | `mark_synced` with nothing moved | `lfs_prune_plans` unchanged; after `note_unit_synced` → +1; a second `mark_synced` → unchanged; after `next_prune_ms` elapses → +1 | none |

</intent-contract>

## Code Map

| Path | Change |
|---|---|
| `src-tauri/crates/keeper-sync/src/lfs/stage.rs` | `CONTROL_FILES` (5 names) + `pub fn is_control_file` replaces `GIT_CONTROL_FILES`/`is_git_control_file` (:110-125); `LfsPolicy::{excludes, retires}` (:146-202); `repair_managed_block(text, retire)` and `ensure_attributes(root, patterns, policy)` (:621-746); `already_routed(repo, rela, policy)` (:1443); control-file skip in `mismatched_filtered_paths` (:1515) and `unconverted_after_repair` (:1590); `prepare` passes the policy (:1665, :1685); `pub fn pointerised_control_files(root, tracked)`; `pub(crate) fn pointer_blob` for `prune::plan`; tests |
| `src-tauri/crates/keeper-sync/src/lfs/virtual_policy.rs` | `is_control_file` delegates (:623); `compile` pointer refusal + `POINTER_TEXT_POLICY_PREFIX` (:197-213); tests |
| `src-tauri/crates/keeper-sync/src/lfs/prune.rs` | module doc condition 3 rewritten; `plan(repo, root, store, tracked, owed, synced)` over one index read |
| `src-tauri/crates/keeper-sync/src/db.rs` | `pub fn synced_oids(conn, profile_id) -> HashSet<String>` beside `referenced_oids` (:2352); test |
| `src-tauri/crates/keeper-sync/src/engine.rs` | `EngineCounters::lfs_prune_plans` (:163); fields `lfs_moved`, `next_prune_ms` after `lfs_ssh_credentials` (:1126) + init (:1427) + cleanup with `next_sweep_ms`; `report_blobs_over_threshold` raises the control-file anomaly (:3586); `ensure_lfs_rule` refuses via `LfsPolicy` and passes it (:4477-4556); `mark_synced` gates the prune (:5720); `note_unit_synced` sets `lfs_moved` (:8099); `prune_lfs_store` takes `synced_oids`, bumps the counter (:9110); `prune_is_due`; tests |
| `src-tauri/crates/keeper-sync/tests/lfs_prune.rs` | `plan_for` passes `synced`; new no-memo case |
| `src-tauri/crates/keeper-sync/tests/lfs_roundtrip.rs` | `ensure_attributes` callers pass a policy |
| `src-tauri/crates/keeper-sync/tests/lfs_control_files.rs` | new: the attribute/extension doors through `stage_and_commit`, the rule retirement, the repair sweep, the anomaly probe |
| `docs/sync.md` | §8 condition 3 + cadence paragraph; §20 item 6 |
| `src-tauri/crates/keeper-sync/src/footprint.rs` | `measure(root, synced)` — `reclaimable` is what prune may release, and prune releases only confirmed oids; tests pass an empty set |
| `src-tauri/crates/keeper/src/sync_ipc.rs` | `sync_footprint` reads `engine.synced_oids(&id)` and passes it to `measure` — **not compiled on this host** (GTK) |

## Tasks & Acceptance

**Execution:**
- [x] predicate + `LfsPolicy::{excludes, retires}` + every door (`applies`, `already_routed`, repair pair, `prepare`)
- [x] `ensure_attributes` retirement through `repair_managed_block`; `ensure_lfs_rule` refusals
- [x] `compile` pointer refusal
- [x] `pointerised_control_files` + anomaly at the hourly site
- [x] `db::synced_oids`; `prune::plan` third input + hoisted index; `lfs_moved`/`next_prune_ms` gate; counter
- [x] docs §8, §20

**Acceptance Criteria (the epic's 70.4 list):**
- stale anchored `/a/b/.gitattributes filter=lfs` in the managed block → text committed, rule gone, user lines byte-identical. **Met** — `lfs_control_files::a_stale_anchored_rule_for_a_control_file_is_retired_and_the_file_is_committed_as_text` (tracked file, modified, committed through `prepare` + `stage_and_commit`, read back with `git show`); mutations (1), (1b).
- `.lfsconfig`, `.keepervirtual`, `.keeper/keeper.toml` stay blobs though `*.toml filter=lfs` exists. **Met** — `…::keepers_own_control_files_stay_blobs_though_a_rule_and_the_threshold_both_say_pointer` (two over the threshold, the folder config under it so only the attribute door could take it; `settings.toml` is the control group and IS routed); mutation (1).
- pointer-text `.keepervirtual` → `compile` returns `Config` naming the file. **Met** — `…::a_policy_file_that_is_pointer_text_is_refused_by_name`; mutation (3).
- pointer-text `.gitattributes` fixture → anomaly with path and subtree. **Met** — `…::a_tracked_control_file_that_is_pointer_text_is_found_with_the_subtree_it_governs` proves the probe and the subtree; the `Anomaly` line is raised from `Engine::report_pointerised_control_files` at the hourly site (see *Not verified*).
- no `synced_at_ms` → never in the plan; with it → in the plan. **Met** — `lfs_prune::an_object_the_remote_was_never_seen_holding_is_never_released` and the memo-carrying `plan_for` under every other case; `db::tests::synced_oids_are_the_confirmed_objects_and_nothing_less`; mutation (5a).
- a successful pull that moved no LFS unit runs no prune (counter). **Met** — `engine::tests::a_success_edge_that_moved_no_lfs_unit_runs_no_prune_plan` drives `mark_synced` and counts `lfs_prune_plans`; mutation (5b).

## Design Notes

**Retirement rides the keep-vector.** `repair_managed_block` already decides per managed line whether it survives (duplicates) and what it is rewritten to (the 46.1 repair). A retired rule is a third reason for `keep[i] = false`, judged on the *decoded* pattern after repair so a broken spelling of a control-file rule is retired rather than repaired into a working one. Because the function returns `None` when nothing changed, a file with nothing to retire is still never rewritten — the byte-identity property the existing tests pin.

**Why `*.ext` retirement asks about `.ext` and `a.ext`.** wildmatch's `*` matches the empty string, so `*.gitattributes` covers `.gitattributes` itself; and `lfsNever`'s basename globs are compiled as `**/pattern`, which matches a bare `a.ext`. A scoped opt-out (`notes/*.md`) does not match `a.md` and does not retire the global rule — correct, since the rule still governs the rest of the tree.

**Prune's third input is a memo, and it says so.** `synced_at_ms` is written by `note_unit_synced` (which refuses a stale unit by checking the index still names the uploaded oid) and by `audit_remote_objects`'s per-object `serves`. `note_local_authorship` clears it on every new commit of the path. So `(oid, synced_at_ms)` on a row is a consistent pair, and `synced_oids` returns exactly the oids the remote was observed holding.

**The flag is set for uploads only.** A download's arrival writes no confirmation (`note_arrival` deliberately leaves `synced_at_ms` NULL), so a prune after a download could find nothing new; the hourly look covers the audit's memos.

## Verification

- `cargo check -p keeper-sync -p keeper-syncd --all-targets` → clean for this story (the one warning, `git::fetch::classify` unused, is 70.2's in-flight work).
- `cargo test -p keeper-sync --test lfs_control_files --test lfs_prune --test lfs_roundtrip` → 5 + 8 + 17 passed.
- `cargo test -p keeper-sync --lib -- lfs::stage lfs::virtual_policy lfs::prune footprint db::tests::synced_oids an_lfs_rule_for_a_control_file a_success_edge_that_moved_no_lfs_unit an_lfs_rule_that_cannot the_session_lfs_rule` → 117 passed.

| Mutation | Tests that failed | Observed |
|---|---|---|
| (1) `already_routed`: `if false && policy.excludes(rela)` | `a_stale_anchored_rule…committed_as_text` (pointer committed), `keepers_own_control_files_stay_blobs…` (`.keeper/keeper.toml` a pointer), `stage::tests::an_opted_out_path_under_an_existing_rule…` | 3 failed |
| (1b) `repair_managed_block`: `if false && policy.retires(&pattern) \|\| …` | `a_stale_anchored_rule…` (rule still in the block), `stage::tests::ensure_attributes_retires…`, `…an_opted_out_path_under…` | 3 failed |
| (3) `compile`: `if false && is_pointer_candidate(…)` | `a_policy_file_that_is_pointer_text_is_refused_by_name` | 1 failed |
| (5a) `plan`: both `synced` checks `if false && …` | `lfs_prune::an_object_the_remote_was_never_seen_holding_is_never_released` | 1 failed; the other 7 (memo-carrying) still pass, as they should |
| (5b) `mark_synced`: `(true \|\| take_lfs_moved \|\| prune_is_due)` | `engine::tests::a_success_edge_that_moved_no_lfs_unit_runs_no_prune_plan` | 1 failed |

Every restore proven by the editor's snapshot hash returning to the pre-mutation value (`#1D12`, `#B5F1`, `#AAC8`) or by re-reading the line; the full set above green again afterwards.

The first draft of the two door tests committed *new* files and passed under mutation (1): a brand-new path is never `already_routed` (no index entry). The tests were rewritten to track each control file as text first (`commit_raw`, filter bypassed, the way history came to hold them) and then modify it — which is the shape that opened the door on hesperia.

**Not verified here, and why.** `keeper/src/sync_ipc.rs`'s two-line change (`engine.synced_oids` → `measure(&root, &synced)`) was not compiled — the shell crate does not build on Linux; the risk is a compile error, not a behaviour error. The anomaly *line* is raised from `report_blobs_over_threshold`, which runs behind the hourly `sweep_is_due` clock inside `spawn_blocking`; the probe it calls is tested directly and the reporting function is a `for` over its result with a fixed `Anomaly`, so a log-capture test was not added. The hesperia measurement in the epic's acceptance (`.gitattributes` lines 114–115 retired after install) is 70.8's install step.
