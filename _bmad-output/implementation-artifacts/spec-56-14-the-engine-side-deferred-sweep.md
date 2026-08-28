---
title: 'The engine-side deferred sweep'
type: 'bugfix'
created: '2026-08-28'
status: 'done'
review_loop_iteration: 0
baseline_revision: '1016bef'
final_revision: '0b0b25f'
followup_review_recommended: true
context: []
warnings: ['multiple-goals']
---

<intent-contract>

## Intent

**Problem:** Epic 56's nine stories plus four follow-ups left 73 entries in `_bmad-output/implementation-artifacts/deferred-work.md`, 38 of them with their fix inside `keeper-sync`. Several lose data or lie: a release DELETED the `materialized` row, discarding the recency history the release clocks were built on; `materialize` decided "keeper does not overwrite a local modification" and then never re-checked before `rename(2)`; an unreadable pointer file was reported as a local modification the user must undo; a `!`-only `virtualPatterns` list silently switched a whole folder's virtualization off; a FIFO standing at a repair path reached `File::open` and blocked forever; a hard-linked release reported bytes it did not reclaim; a filesystem remote carrying a `.lfsconfig` could never release anything.

**Approach:** Work the entries in order of consequence — data or truth first, then cost-under-load, then test-only — and record an argued outcome for every one. Three outcomes only: **fix** with a test proven to fail without it, **keep** with the condition that would make the fix worth doing, or **stale** with quoted proof the claim is no longer true.

## Boundaries & Constraints

**Always:** Every epic-56 entry whose fix lives in `src-tauri/crates/keeper-sync/**` gets one of the three outcomes recorded in the ledger. Every fix names its test. Every `keep` says what would have to change.

**Block If:** A fix requires editing `src-tauri/crates/keeper/**` (the Tauri shell crate, which cannot be compiled on this host) — then the entry stays open naming the exact symbol.

**Never:** No `cargo` or `bun` gate run from this agent — the coordinator runs every gate once for both siblings. No new public API in `keeper-sync` whose only possible caller is the shell crate: that is a stub, and `3630`/`3645` are `keep` for exactly that reason. No `MissingObject::size` rename — `docs/sync.md` §13 calls the `--json` field names the contract, so an alias or rename needs a deprecation decision.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Release, ordinary file | `nlink == 1`, remote proves the object | Pointer published; `Release.size_bytes == pointer.size`; ledger row retained with `released_at_ms` stamped and every clock intact | No error |
| Release, hard-linked file | `nlink > 1` | Same release; `Release.size_bytes == 0`; `info!` names the link count | No error — the release is real |
| Release, filesystem remote + unusable `.lfsconfig` | `lfs_access` errs, `remote_store` is `Some` | The filesystem store answers the per-object proof | Store lacks the object at that size → `UnprovenOnRemote` (fail-closed) |
| Materialize, target edited in the publish window | Target no longer holds the committed pointer | Refused `Integrity`; the user's bytes survive; no `.keeper.*` sibling left | `SyncError::Integrity` naming the path |
| Materialize, already held | Worktree holds non-pointer bytes of the pointer's length | `AlreadyMaterialized`; ledger row written by `observe_materialized`; `at_ms` unmoved, `last_used_ms` now | Ledger failure is `warn!` only |
| Download, object already in the store | Pending `LfsDownload` row, `store.contains` true | No endpoint resolution, no batch round trip; straight to `materialize_landed` | Publish errors propagate as before |
| Repair, FIFO at the path | Non-regular file where a missing object's content should be | Refused `Integrity` naming "a named pipe" | Refusal, never a block |
| Verify, `lfsMode = pointerOnly`, no policy | Committed pointer, object absent, remote holds it | Counted in `virtual_paths` | The other three facts still earned per path |
| Verify, `pointerOnly` + truncated object | Object present at the wrong length | Still `report.bad` | Damage is not a virtual path |
| Policy, `virtualPatterns = ["!a/keep.mp4"]` | Committed `.keepervirtual` names the zone | File's permissive list stays in force; the protection unions in; tier `PatternFile` | No error |
| Validate, `virtualPatterns = ["scans/["]` | Saved from the form or loaded from TOML | `SyncError::Config` quoting `scans/[` | Refused at the box |
| Folder layer, one bad key | `[folder]` sets a good key and a refused one | The good key is in force and named in `owned`; the bad one is not; the fault names it | Fault recorded, so the release sweep still declines the folder |
| Subpath with a backslash, non-Windows | `40-media\clip.mp4` | Keyed as itself, so the verb answers about the file named | Not found → `NotTracked` |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/db.rs` -- the `materialized` ledger: the new `released_at_ms` column, `forget_materialized`'s retention, `observe_materialized`, and the present-tense filter on both readers.
- `src-tauri/crates/keeper-sync/src/engine.rs` -- `do_lfs`'s store short-circuit, `materialize_held`'s already-held ledger write, `remote_serves`' filesystem fallback, `release_resolved`'s reclaimed-bytes figure and `spawn_blocking` hash, `verify`'s pointer-only excuse and read-only open, `ReleaseSchedule`'s word split.
- `src-tauri/crates/keeper-sync/src/lfs/stage.rs` -- `clean`'s non-regular refusal, `index_key`'s Windows gate, the shared `staging_path`, `materialize`'s pre-rename re-check and staging cleanup, `read_worktree_pointer`.
- `src-tauri/crates/keeper-sync/src/lfs/hydrate.rs` -- `plan`'s error type, and `Release::size_bytes`' contract.
- `src-tauri/crates/keeper-sync/src/lfs/virtual_policy.rs` -- the override decision, the tier fallback, `check_patterns`.
- `src-tauri/crates/keeper-sync/src/profile/mod.rs` -- `validate` compiles the virtualization list.
- `src-tauri/crates/keeper-sync/src/profile/folder.rs` -- `overlay` takes a parsed table; `salvage_keys` retries a failed layer key by key.
- `src-tauri/crates/keeper-sync/src/browse.rs` -- `classify`'s `replacing` rung.
- `src-tauri/crates/keeper-sync/src/copy.rs` -- `describe_kind` widened to the crate.
- `src-tauri/crates/keeper-sync/src/git/repo.rs` -- `open_read_only`.
- `src-tauri/crates/keeper-sync/tests/**` -- `hooks_never_run.rs` hermeticity; the new behavioural tests.

## Tasks & Acceptance

**Execution:**
- [x] `src/db.rs` -- grow `released_at_ms`; make `forget_materialized` an UPDATE; add `observe_materialized`; filter both readers on `released_at_ms IS NULL` -- a release discarded `last_used_ms`, `synced_at_ms` and `local_origin` at the instant those columns were designed to still answer.
- [x] `src/engine.rs` -- write a ledger row from the already-held arm -- FR-334's last-use fact was invisible for a path a human explicitly named.
- [x] `src/engine.rs` -- short-circuit a download the local store already satisfies -- a stale journal row re-fetched bytes already on disk.
- [x] `src/engine.rs` -- report the bytes actually reclaimed -- `rename(2)` replaces one directory entry, so a hard-linked release reclaims nothing.
- [x] `src/engine.rs` -- fall back to the filesystem remote's store when no LFS server is addressable -- such a profile could never release anything.
- [x] `src/engine.rs` -- excuse a `pointerOnly` folder in `verify`, through the same `virtual_paths` count -- the checks called the designed state a fault for the one mode whose purpose is leaving pointers.
- [x] `src/engine.rs` + `src/git/repo.rs` -- open the index without housekeeping in `verify` -- a check that repairs what it is checking is not a check.
- [x] `src/engine.rs` -- hash on the blocking pool -- the first interactive caller pinned a runtime worker for a multi-gigabyte read.
- [x] `src/engine.rs` -- give `Indefinite` its own word -- one word over a releasable row and a guaranteed refusal left the Files pane nothing to gate on but prose.
- [x] `src/lfs/stage.rs` -- refuse a non-regular file in `clean` -- `File::open` on a FIFO never returns.
- [x] `src/lfs/stage.rs` -- gate `index_key`'s translation to Windows -- `\` is an ordinary filename character elsewhere, so a verb answered about a different file than the caller named.
- [x] `src/lfs/stage.rs` -- share and bound `staging_path` -- 12 bytes of decoration made a long-named path unreachable in both directions for the life of the file.
- [x] `src/lfs/stage.rs` -- re-state the decision before `rename(2)`, and clean up the staging file on failure -- an editor save landing in the publish window was destroyed.
- [x] `src/lfs/stage.rs` + `src/lfs/hydrate.rs` -- add `read_worktree_pointer` and make `plan` answer `SyncError::io` for an unreachable file -- an unreadable pointer was reported as a local modification to undo.
- [x] `src/lfs/virtual_policy.rs` -- decide the override on the permissive half alone; add the tier fallback; add `check_patterns` -- a `!`-only list replaced a committed zone with nothing.
- [x] `src/profile/mod.rs` -- compile the virtualization list in `validate` -- the form accepted a glob the engine refuses at run time.
- [x] `src/profile/folder.rs` -- retry a failed layer key by key -- one bad key silently discarded every other key in the file.
- [x] `src/browse.rs` -- treat `replacing: true` as arriving content -- a replacement download said the content was still on the remote.
- [x] `tests/**` and in-file `mod tests` -- one test per fix, each proven to fail without it; make `hooks_never_run.rs` hermetic; kill the `git::resolve` ETXTBSY race.
- [x] `deferred-work.md` -- record fix / keep / stale for every entry in this column.

**Acceptance Criteria:**
- Given a released path, when the sweep or the request door releases it, then the row survives with `released_at_ms` stamped and every clock intact, and no present-tense reader returns it.
- Given a path whose content is already present and which keeper never published, when a human asks for it, then the ledger records the sighting without moving the landing clock.
- Given a hard-linked release, when it succeeds, then the reported figure is `0` and the sibling still holds the content.
- Given a target edited between the decision and the rename, when `materialize` publishes, then it refuses and the user's bytes survive.
- Given a `virtualPatterns` list of only protections, when the policy compiles, then the committed permissive list is still in force and the protection unions in.
- Given a `.keeper/keeper.toml` with one refused key beside good ones, when the layer is applied, then the good keys are in force and the fault names the bad one.
- Given a FIFO where a missing object's content should be, when the repair pass reaches it, then it refuses by name rather than blocking.
- Given `lfsMode = pointerOnly` and no policy, when `verify` runs, then the absent objects are counted as virtual and a truncated one is still a fault.

## Outcome Ledger

56 epic-56 entries carry a status and an argued block. Across both columns of story 56.14:
**28 fixed, 5 stale with quoted proof, 23 kept with the condition** — 20 fixed, 2 stale and
17 kept in this column. Every test's failure without its fix is stated in its own doc comment.

| Ledger entry | Outcome | Test |
|---|---|---|
| A release deletes the `materialized` row | fixed | `db::tests::a_released_row_keeps_its_clocks_and_leaves_every_present_tense_reader`, plus `forget_materialized_removes_exactly_one_row` extended |
| `alreadyMaterialized` writes no ledger row | fixed | `tests/materialize_entry.rs > the_already_held_answer_records_the_use_without_restarting_the_arrival_clock`; `db::tests::observing_content_records_the_use_and_never_moves_the_landing_clock` |
| No re-check before `rename(2)` | fixed | `lfs::stage::tests::a_target_that_no_longer_holds_the_committed_pointer_is_never_published_over`; `a_target_deleted_since_the_decision_is_not_recreated_by_the_publish` |
| `!`-only list triggers the wholesale override | fixed | `lfs::virtual_policy::tests::a_profile_list_of_only_protections_does_not_replace_the_committed_zone`; `a_protections_only_profile_list_over_no_file_is_still_the_profiles_policy` |
| A FIFO reaches `lfs::stage::clean` | fixed | `lfs::stage::tests::cleaning_refuses_a_named_pipe_rather_than_blocking_on_it_forever`; `cleaning_refuses_a_directory_standing_where_content_should_be` |
| A hard-linked release over-reports bytes | fixed | `tests/dehydrate_entry.rs > a_released_hard_linked_file_reports_no_bytes_reclaimed` |
| A backslash subpath is looked up under another name | fixed | `lfs::stage::tests::a_backslash_in_a_name_is_an_ordinary_character_and_keys_as_itself` |
| A folder layer that fails validation is discarded whole | fixed | `profile::folder::tests::a_layer_that_trips_one_rule_keeps_the_keys_it_got_right`; `a_layer_with_no_problems_applies_whole_and_reports_nothing` |
| `worktree_pointer` folds a read failure into `None` | fixed | `lfs::hydrate::tests::a_pointer_file_that_cannot_be_read_is_an_error_and_not_a_modification` |
| An inline publish does not retire a pending `LfsDownload` | fixed, at the transfer | `tests/materialize_entry.rs > a_queued_download_the_store_already_holds_costs_no_transfer` |
| Filesystem remote + `.lfsconfig` can never release | fixed | `tests/dehydrate_entry.rs > a_committed_lfsconfig_naming_no_server_still_releases_from_a_path_remote` |
| `Engine::verify` is no longer read-only | fixed | `git::repo::tests::open_read_only_leaves_a_stale_index_lock_that_open_would_remove` |
| `.keeper.{name}.tmp` and `NAME_MAX` | fixed | `lfs::stage::tests::a_name_within_twelve_bytes_of_name_max_can_still_be_materialized` |
| `LfsMode::PointerOnly` reported as a fault by `verify` | fixed | `tests/virtual_state_is_not_a_fault.rs > a_pointer_only_folder_excuses_its_pointers_with_no_policy_at_all`; `a_truncated_object_is_damage_even_in_a_pointer_only_folder` |
| A queued download over held bytes reads `Waiting` | fixed | `browse::tests::a_download_replacing_content_this_machine_holds_is_materializing_not_waiting` |
| `dehydrate_entry` hashes on the runtime worker | fixed | no new test — see the note below |
| `ReleaseSchedule` draws one word for three reasons | fixed | `engine::tests::release_schedule_pairs_an_instant_with_words_exactly_one_way`, extended |
| `SyncProfile::validate` does not compile `virtual_patterns` | fixed | `profile::tests::a_malformed_virtual_pattern_is_refused_where_the_person_typed_it`; `lfs::virtual_policy::tests::check_patterns_accepts_every_legitimate_list_and_refuses_a_malformed_glob` |
| `tests/hooks_never_run.rs` fails on a host with `core.hooksPath` | fixed | the test itself, made hermetic; the defect and the repair were both reproduced by hand with `git` on this host |
| `git::resolve` ETXTBSY flake | fixed | the test itself; the script is renamed into place so the exec path is a fresh inode |

`spawn_blocking` around `content_oid` carries no new test on purpose: the observable contract
is unchanged — the same digest, the same refusal, the same success — and what changed is which
thread pays for it. A test that asserted "no runtime worker was pinned" would be asserting on
tokio's internals, and one that asserted a timing property would be the load-sensitive flake
this same ledger records twice. The existing `dehydrate_entry.rs` suite already proves the
digest guard still refuses a same-length edit and still permits a real release, which is the
whole of what a caller can observe.

### Pre-existing tests whose premise the fixes changed

Repaired, never deleted, each with its own doc recording why:

- `tests/virtual_state_is_not_a_fault.rs > the_unauthorized_path_is_the_only_row_reported` —
  the file's fixture is `LfsMode::PointerOnly`, so under the mode excuse there is no such
  thing as an unauthorized path in its folder. Retargeted at a `LfsMode::Materialize` folder,
  which is the default and the mode story 56.10 made the policy load-bearing for. This was the
  ONLY test in the whole three-crate suite that the verify change broke, and it is exactly the
  one asserting the fact the mode now supplies — which is the evidence that the change is
  precisely scoped.
- `profile::folder::tests` — four tests asserted the whole-layer discard:
  `a_refused_layer_leaves_the_stored_profile_exactly_as_it_was` (renamed
  `a_refused_local_path_falls_alone_and_cannot_move_the_folder`),
  `a_non_main_folder_may_not_carry_settings`,
  `a_broken_file_makes_this_folders_config_read_as_faulted` — whose fail-closed claim now
  rests on the recorded fault rather than on the default taking over — and
  `owned_fields_never_names_a_key_the_overlay_refused`.
- `lfs::hydrate::tests` — the `plan` refusal tests now unwrap through a `refusal(..)` helper
  that panics on any non-`Refused` error, so each of those cases is still asserted to be a
  DECIDED fact about the file rather than merely "an error".

### Gate result

Run once on this branch after every edit landed, with the sibling's cargo lock released:

- `cargo test -p keeper-sync -p keeper-syncd -p keeper-core` — **3578 passed / 0 failed**
  (baseline on this branch was 3553; the delta is this story's new tests).
- `cargo clippy -p keeper-sync -p keeper-syncd -p keeper-core --all-targets -- -D warnings` —
  clean. The `proc-macro-error2` future-incompatibility note is a pre-existing dependency
  warning, not a lint failure.
- `cargo fmt --check -p keeper-sync` — clean.
- The frontend and the Tauri shell crate were not built here: neither is in this column, and
  `keeper` cannot link on this host.

## Kept, with the condition

- **The release sweep's guard order (`OpenUnknown` after the hash and the round trip).** Story 56.11 removed the premise on Linux: `probe_open_file_state` answers for real there, so a Linux pass now spends its hash on candidates that go on to be released, which is the cost the order was designed to pay. It becomes worth doing when a second platform ships a real answer, or when macOS still cannot answer and a per-pass capability probe is added — at which point the probe is one call and the saving is the whole byte budget. Fixing it now would reorder guards to optimize a case that no longer exists on the only platform that can answer.
- **`Engine::materialized_paths` unfiltered by the cone, and `sync_browse` scanning it twice.** Both callers are `keeper/src/sync_ipc.rs` (`:2197`, `:3674`, and `sync_browse`), the shell crate, which cannot be compiled on this host. A cone-filtered `db::materialized_paths_under` landing today would be a public API with no caller in any crate that builds here. One shell-crate story should add the filtered read and collapse the double scan together; splitting it leaves an unproven primitive.
- **`Engine::reserve` is an in-process map.** Cross-process exclusion over one `sync.db` and one index is a design change, not a repair: `with_db` is a mutex where it would have to be a transaction, `enqueue_unique`'s SELECT-then-INSERT would need to be atomic, and `recover_running` would have to stop running unconditionally at every one-shot `Engine::open`. It becomes worth doing when a supported configuration runs the daemon and the CLI over one folder at once; today that is a documented misuse.
- **A verified copy refuses `Busy` per path while its folder syncs.** The direction is safe — a refusal, never a stub — and the fix is the same cross-process/whole-copy reservation question as `reserve`. It becomes worth doing when a copy can take the folder's reservation once for its whole run, which needs the reservation to outlive a single path.
- **`ls-files` is keyed by the index, so a released-and-uncommitted path produces no row.** The listing's key is "which of this checkout's LFS paths do I hold", which is the right key for the verb. Answering "what is this clone still holding" needs a second, ledger-keyed listing with its own state vocabulary — a released row is neither `virtual` nor `materialized` nor `absent` — and that is a new document, not a filter change.
- **A present `remote` key does not prove the server was asked.** The short-circuits are correct: there is no batch API on a filesystem remote and none on a disabled folder. The honest repair is the sentence, not the shape, and `docs/sync.md` §13 now carries it — "`remote.missing == []` means *nothing keeper could ask reported this object missing*, not *the server confirmed every object*". A wire discriminator would be a published-contract addition for a claim prose already makes.
- **`MissingObject::size` versus `LfsFile::size_bytes`.** §13 calls the `--json` field names the contract, so a rename is a breaking change and an alias is a second spelling of the same field forever. It becomes worth doing at the next deliberate `--json` version bump, where both can be renamed at once.
- **The gitignore dialect has no root-anchored single-segment spelling outside the virtualization lists.** `virtual_policy::anchor_line` handles a leading `/`; `exclude::anchor` deliberately does not, and its current meaning is depended upon by every stored `excludes` and `lfsNever` list. Teaching `add_pattern` the leading slash would change what an existing profile means, which needs a migration decision, not an edit.
- **A path keeper just materialized reads ` M` until the next pass.** Self-corrects, at the cost of one hash, and the committer repairs the stat when it stages the path. It becomes worth doing when gix offers a way to decline `set_entry_stat_size_zero`, or when `persist_observed_stats` can be told to skip an entry the same pass just wrote.
- **macOS cannot answer "is this file open".** Three options were weighed and rejected with reasons in the original entry; none has changed. It becomes worth doing when `libproc` drops its unconditional `bindgen` build dependency, or when keeper is willing to carry a hand-written `libproc` FFI shim with its own soundness argument.
- **The Linux answer's other-uid narrowing.** Deliberate and load-bearing: pid 1 alone guarantees an unreadable descriptor table on every Linux box, so the strict rule answers `Unknown` — refusing every release — on every host, which is the failure the story existed to end.
- **The unbounded `/proc` walk on a hung NFS or FUSE mount.** Real and new, and unfixable with `std`: `std::fs::metadata` on a hard-mounted NFS path is uninterruptible and offers no timeout. It becomes worth doing when the walk can be moved behind a bounded worker whose abandonment is safe — which needs a thread that may be leaked, and a decision about leaking it.
- **`SyncProfileVm.folder_owned` re-derives the overlay.** Threading `FolderOutcome` out of `profile::in_force` and `db::list_profiles` has nowhere to land: `SyncProfileVm` and `SyncProfileReq` are both defined in the shell crate. Recorded on the sibling's side; the keeper-sync signature is deliberately unchanged.
- **A profile with an unborn HEAD commits nothing, forever.** Not diagnosed. What this pass added is a narrowing, recorded in the entry: four hypotheses are ruled out with quoted code — `stage_and_commit` creates a root commit correctly, `head_commit_id` answers `Ok(None)` by design and is tested, gix's status does report untracked files on an unborn HEAD with an empty index (keeper's own `status_reports_untracked_files_and_nothing_else` proves it), and the stability gate reads no git object at all — leaving three candidate expressions named by file and line. Finishing it needs a reproduction with the shipped `keeper-syncd` binary, which is a `cargo build` this agent is forbidden to run.

## Stale, with proof

- **"The 56.5 guard-order entry is now moot on Linux."** Discharged rather than outstanding: its content is an annotation on the 56.5 entry, and that entry's `keep` block now carries the argument in full. Nothing is left for a reader to act on here.
- **"No production path constructs a `VirtualPolicy`, so FR-329's startup refusal is unreachable."** False since story 56.2 and further false since 56.10 and this story. `VirtualPolicy::compile` is now called from five production sites in `engine.rs` — the arrival leg (`:6066`), the second arrival path (`:6205`), `verify` (`:8641`), `release_schedules` (`:9243`) and `release_mode_gate` (`:9756`) — each of which propagates its `SyncError::Config`. As of this story `SyncProfile::validate` also calls `lfs::virtual_policy::check_patterns`, so the refusal now fires on every profile write and every profile load, which is strictly earlier than "at startup".

## Left for the shell crate

Nothing new. The two entries whose fix would have to land in `src-tauri/crates/keeper/**` are named above with their exact symbols: `sync_browse` (the double scan) and `SyncProfileVm::from` (the re-derived overlay). `sync_release_entry`'s stale doc comment is the sibling's record.

## Spec Change Log

## Review Triage Log

### 2026-08-28 — Review pass

- intent_gap: 0
- bad_spec: 0
- patch: 11: (high 4, medium 4, low 3)
- defer: 1: (low 1)
- reject: 0
- addressed_findings:
  - `[high]` `[patch]` `note_local_authorship` was the one row-creating writer not taught to clear `released_at_ms`, and it is the ONLY writer that can reach a released row — content the owner puts back at a released path is not pointer text, so `pending_smudges` skips it and neither `remember_materialized` nor `observe_materialized` runs. The row stayed retired forever: bytes just authored were never a release candidate at any age, carried no schedule in a listing, and read to `materialized_paths` as content this machine does not have. Both reviewers found it independently, with a reproduction. `released_at_ms = NULL` added to its `DO UPDATE SET`, with the reasoning in its doc.
  - `[high]` `[patch]` `observe_materialized` wrote no `oid` or `size_bytes` although the already-held arm is holding the committed pointer. `release_path_gate` reads `row.size_bytes.unwrap_or(u64::MAX)` against a `size < over_bytes` floor, so a NULL-size row cleared **any** `virtualOverBytes` floor — releasing exactly the small file a person had just named — contributed nothing to `RELEASE_BUDGET_BYTES`, and made `release_schedules` compute a countdown against a size nobody measured. The identity is now written on both arms.
  - `[high]` `[patch]` `lfs::stage::materialize` raised on a raced target, which escaped `materialize_pending`'s loop, was captured by `sync_once` as `arrival_fault`, **failed the whole pass** and skipped `mark_synced` — so one file being saved during a sync abandoned every remaining publish, reported a failed sync, and left the release window unarmed. Replaced with `Published::{Content, TargetMoved}`: the two sweeps `continue` past it, as their own two sibling `continue`s already do for the same discovery, and the request door answers `ContentRefusal::LocallyModified`, which is what `hydrate::plan` would have said an instant later. Found by self-review before the reviewers reported; both confirmed the mechanism.
  - `[high]` `[patch]` `clean`'s new pre-open `symlink_metadata` refused a symlink to a REGULAR file, which `File::open` has always followed — so `republish_missing_objects` reported an object unrecoverable, "those bytes exist on some other machine or nowhere", for a file sitting behind a link on another volume. The stat now asks twice: what is at the path, and, when that is a link, what it leads to. A link to a FIFO is still refused, by the second answer.
  - `[medium]` `[patch]` `materialize`'s new staging cleanup did not cover the `fs::copy` failure — the one that leaves the LARGEST partial file — because an early `?` returned before the cleanup block. An `ENOSPC` part-way through a multi-gigabyte object left a junk `.keeper.*.tmp` in the owner's folder on the very failure where they can least afford the space, and `exclude.rs` keeps it out of every commit so nothing ever collected it. The copy is now inside the same fallible block.
  - `[medium]` `[patch]` `browse::classify`'s new disjunction let `replacing: true` short-circuit past the worktree probe, so a `None` probe — a directory, or a path with no file at all — reached `Materializing`. The ledger row outliving the file is the ordinary case, so a deleted-but-ledgered path with a queued download would have read "this content is arriving" forever, over a file `materialize` now declines to publish on every pass. The rung requires the probe to have answered.
  - `[medium]` `[patch]` `remote_serves`' filesystem fallback fired on ANY `lfs_access` error, including `Auth` and `Network` — so a release could proceed on a removable drive's word about a named server that was merely down, and AD-48 treats an absent drive as never a failure, leaving the content on neither. Narrowed to `SyncError::Config`, the class `endpoint::derive` raises when no endpoint can exist for that remote at all.
  - `[medium]` `[patch]` The reclaimed-bytes figure read `nlink` from an `lstat` taken before a whole-file hash, a network round trip and the `/proc` walk. Re-stated from a fresh stat beside the second pin read, falling back to the earlier answer rather than refusing a release every guard has already permitted.
  - `[low]` `[patch]` `clean`'s doc claimed the post-open `fstat` "closes the window" in which a regular file is replaced by a FIFO. It cannot: the `fstat` is on the already-opened handle, so it cannot run until the blocking `open` returns. Corrected to say the window is narrowed from "always" to "a few microseconds", and to record that closing it needs `O_NONBLOCK`, which needs `libc`, which this crate does not carry and its `deny(unsafe_code)` posture argues against.
  - `[low]` `[patch]` `clean`'s doc named `republish_missing_objects` as its sole caller; `stage::prepare` also calls it. Both are named now, with which one the refusal is load-bearing for — that omission is what let the symlink case go unconsidered.
  - `[low]` `[patch]` `forget_materialized`'s doc claimed the retained rows are "bounded by the folder, not by time". False: nothing prunes a released row, so a path deleted or renamed upstream leaves its row forever and the bound is paths-ever-hydrated. The doc now states the real bound and says a prune is a decision rather than an edit.
  - `[low]` `[defer]` A prune for released `materialized` rows. Recorded in `deferred-work.md` against this spec.

Two findings were considered and NOT changed, with the argument recorded rather than
the code:

- `Release::size_bytes` now carrying "bytes reclaimed" means a hard-linked release emits
  `{"outcome": "released", "sizeBytes": 0}`, and `keeper-syncd`'s `already_pointer_json`
  doc reserves a zero size for "nothing was released". The two documents remain
  distinguishable by `outcome`, which is the field a consumer must branch on either way,
  and a consumer totalling RECLAIMED bytes wants the zero. The field's own doc has always
  said "the bytes this machine no longer holds"; what changed is that it is now true. The
  surface half of story 56.14 documents the `0 B` line in `docs/sync.md` §13 explicitly.
- The residual blocking-in-async in `release_resolved` — the index open and parse, the
  `lstat`, and the `/proc` walk — is unchanged and is recorded as such in the ledger. Only
  the whole-file hash was moved, because only it is bounded by the size of the user's file.
