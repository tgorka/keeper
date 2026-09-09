# Lane: virtual files

**Verdict:** Virtual files: work with these bugs

## Findings

### F-VF-1 A materialized row on macOS counts down to a release that can never happen — P1, effort M

Evidence: platform.rs:70-75 probe_open_file_state returns OpenFileState::Unknown on every non-Linux target; engine.rs:11539-11542 refuses ContentRefusal::OpenUnknown; release_schedule (engine.rs:13631-13678) has no platform term and returns Due{at_ms}; files-pane.tsx:540-548 releaseIsCounting arms the countdown for any row with hold===null and FILES_RELEASE_REFUSED_HOLDS = ["Pinned","Kept"] (files-pane.tsx:443) is the only gate on the Release button; docs/sync.md:2962-2977 states 'Nothing releases content on a macOS or Windows machine'. Already recorded as a field defect in epic-69-the-file-you-can-fetch-and-the-task-a-bot-can-run.md:12.

Why: This is the owner's exact configuration: tgdrive-light is macOS, virtualOverBytes=1 MiB, releaseTtlMs=24h. Since 56.16 a bare floor arms the sweep, so every authorized materialized row shows '23 hr' plus the sentence 'keeper lets this content go on the first sync after the time runs out' (engine.rs:13545-13548) - a promise the platform structurally cannot keep. Same defect class 56.14 fixed one level down when it split Indefinite out of Kept.

Fix: Add a ReleaseSchedule variant (word "Held", sentence naming the platform) selected when platform.open_file_state cannot answer - probed once per release_schedules call against the profile root, not per row - and add the word to FILES_RELEASE_REFUSED_HOLDS. instant_or_words (engine.rs:13478-13495) makes this a one-place compile error.

### F-VF-2 On macOS the sweep hashes the whole file and calls the server before the check that always refuses — P2, effort S

Evidence: Guard order in release_resolved: whole-file SHA-256 at engine.rs:11509-11523, remote batch proof at :11526-11532, open-file state at :11534-11542. RELEASE_BUDGET_BYTES = 1 GiB (engine.rs:504), RELEASE_BUDGET_OBJECTS = 32 (:486).

Why: On macOS/Windows the last check is a constant Unknown, so every pass of an armed folder spends up to 1 GiB of SHA-256 and up to 32 LFS batch round trips inside the profile's reservation to reach a refusal knowable for free. With electra unreachable since 2026-09-02 those round trips are also tcp-connect timeouts.

Fix: Take a cheap self.platform.open_file_state(&absolute) immediately after release_path_gate (engine.rs:11417) and refuse OpenUnknown there, keeping the existing final check for the TOCTOU window - the pin is already read twice for exactly this reason.

### F-VF-3 A renamed or deleted path leaves an immortal sweep candidate — P2, effort S

Evidence: The sweep retracts exactly one refusal (engine.rs:9918-9950): the AlreadyPointer arm calls forget_materialized, the generic Err(SyncError::Refused(refusal)) arm only logs. release_resolved answers NotTracked when the index no longer carries the path (engine.rs:11404-11406). db.rs:1382-1386: 'nothing prunes a released row ... the bound is paths-ever-hydrated'. hesperia: 89 289 materialized rows vs 72 641 LFS-tracked index paths. No test covers a candidate whose path left the index (release_sweep.rs asserts NotTracked only via pin_entry, :900).

Why: Each such row is re-selected every pass, passes the policy pre-filter on its recorded size, consumes one of 32 attempt slots and pays a git::repo::open plus full index parse to be told the path is gone. A dated-export or recordings prefix sorts early, so the rotation cursor is all that keeps live candidates reachable.

Fix: Retract on NotTracked too, scoped to the arm where the repository opened and the index does not carry the path - not the .git-missing arm at engine.rs:11394-11396, which is an absent removable volume (AD-48). forget_materialized's AND COALESCE(pinned,0)=0 already protects a pin.

### F-VF-4 The sweep opens the repository and parses the whole index once per candidate — P2, effort M

Evidence: release_resolved opens per call (engine.rs:11398), again in the already-pointer repair (:11459) and again in the tail re-stat (:11637-11640); the sweep loops it up to RELEASE_BUDGET_OBJECTS times (:9905-9915). Its own doc admits 'up to RELEASE_BUDGET_OBJECTS repository opens' (:9521-9522). hesperia's tgdrive index is 26.4 MB / 155 626 entries. lfs_files already shows the right shape: one indexed_pointers read per listing (engine.rs:10432-10437).

Why: 32 x (open + 26 MB index parse) per pass inside the profile's reservation on a USB APFS volume, hourly, for work that mostly ends in refusals. It multiplies F-VF-2 and F-VF-3.

Fix: Hoist one git::repo::open + lfs::stage::indexed_pointers per sweep pass and pass the (pointer, blob) into release_resolved as an optional argument; the single-path request door keeps its own open. Same refactor materialize_landed already made for the 40-hour defect its doc records (:8226-8237).

### F-VF-5 A .keepervirtual that HEAD carries but the worktree does not is read as "no policy" — P2, effort S

Evidence: virtual_policy.rs:202 - Err(err) if err.kind() == ErrorKind::NotFound => String::new(). Nothing consults the index. The worktree-not-HEAD choice is documented at :186-190; the absence case is under-specified.

Why: hesperia logged 588 408 gix_worktree_state::checkout::chunk '<path>: collided (AlreadyExists)' errors on tgdrive-light's first checkout into a non-empty directory - a checkout that by construction does not write files already standing there - and story 56.15 exists because a clone can stop half-way. In both shapes a committed .keepervirtual may simply not be on disk, and keeper takes the permissive answer for arrivals: tier()==Unset, nothing virtual, the folder downloads everything. That is the failure the owner reported (sprint-status.yaml:893-903), reached by a route nobody closed.

Fix: In compile, when the read is NotFound, ask the index once whether .keepervirtual is tracked; if it is, raise the same SyncError::Config the unreadable arm raises, naming the file. Silence must mean 'no policy is committed', not 'the checkout did not finish'.

### F-VF-6 A .keepervirtual that is itself an LFS pointer compiles into a policy that authorizes nothing — P2, effort S

Evidence: compile parses whatever it reads with no pointer check between virtual_policy.rs:196 and :268. is_control_file (:623-633) stops keeper's own policy virtualizing it, but the LFS routing carve-out is shorter: lfs/stage.rs:110 const GIT_CONTROL_FILES: [&str; 3] = [".gitattributes", ".gitignore", ".gitmodules"] - .keepervirtual and .lfsconfig are absent. Pointer text parses to three legal globs, so tier() reports PatternFile, authorizes_anything() is true, and every resolve answers Materialize.

Why: Not hypothetical: hesperia logged 1 314 669 gix warnings from a .gitattributes that was itself an LFS pointer. The same shape on .keepervirtual silently deletes a repository's virtualization policy on every clone that pulls it while tier() reports a policy is in force, and arms the release sweep for a folder whose real policy is unknown. [INFERENCE] on the .keepervirtual consequence; the missing routing carve-out is read, not inferred.

Fix: Refuse in compile when the text is accepted by lfs::pointer::Pointer::parse, with the same SyncError::Config shape; and add .keepervirtual and .lfsconfig to GIT_CONTROL_FILES so keeper's own routing can never produce one.

### F-VF-7 The materialized ledger is unbounded and survives profile deletion — P2, effort S

Evidence: db.rs:1382-1384 - 'nothing prunes a released row: there is no DELETE FROM materialized anywhere in this crate'; a grep confirms none, including in the profile-removal path. Contrast activity, capped at 1500 rows and indexed (db.rs:130-131). materialized_rows reads and allocates every non-released row per sweep pass (db.rs:1266-1273, engine.rs:9797-9805); hesperia holds 89 289 rows.

Why: The row count grows with paths-ever-hydrated over the machine's whole life, not with the folder, and a deleted profile's rows are immortal. The query plan is fine - PRIMARY KEY (profile_id, path) at db.rs:160-165 gives an implicit index serving both the profile_id range and the ORDER BY path without a sort - what is unbounded is the row count and the per-pass materialization into Rust Strings.

Fix: DELETE FROM materialized WHERE profile_id = ?1 in the profile-removal path, and age out rows whose released_at_ms is older than a fixed horizon (90 days) on the success edge the sweep already rides. Both readers already filter released_at_ms IS NULL.

### F-VF-8 verify's folder gate asks a different question from every other gate — P3, effort S

Evidence: engine.rs:12016-12017 uses policy.tier() != VirtualPolicyTier::Unset where release_mode_gate (engine.rs:13221) and the other gates use policy.authorizes_anything(). A .keepervirtual of nothing but ! lines reports PatternFile and authorizes nothing (pinned by virtual_policy.rs:969-987).

Why: No correctness consequence - the per-path resolve at :12112-12115 answers Materialize for every path - but excusable==true buys a git::repo::open_read_only plus indexed_pointers over 155 626 index entries on a verb whose own comment says it wants to stay cheap for the ordinary policy-less folder (:12008-12013).

Fix: Use authorizes_anything(). One word, and all four gates ask one question.

### F-VF-9 sparse.rs is live but buys nothing on this machine, and index.sparse=false is load-bearing — P3, effort S (doc only)

Evidence: SparseCone has three production consumers: reconcile_sparse_cone (engine.rs:5847, :5934, :5968-6008), the LFS transfer filter in materialize_pending (:8500), and the release cone check (:11407) - it is not dead. index.sparse=false is enforced twice per open because gix::status hard-fails with TreeIndexDiff(IsSparse) on a true sparse index (git/repo.rs:14-23), including clearing a config.worktree shadow (repo.rs:884-901). All three hesperia profiles carry index.sparse=false and an empty subpaths[], so no sparse-checkout is applied at all.

Why: The two facts read as contradictory from outside. Cone sparse-checkout costs nothing here because it is switched off; index.sparse=false costs nothing because a full cone would not benefit from a sparse index. The operative statement is docs/sync.md §20.3: sparse checkout does not reduce LFS traffic, which is why the per-path policy exists.

Fix: Nothing in code. docs/sync.md §9 could state in one line that subpaths[] and .keepervirtual are two different levers and neither implies the other.

### F-VF-10 Stories 56.16 and 56.17 shipped without a sprint-status.yaml row — P3, effort S

Evidence: spec-56-16-a-floor-on-its-own-means-something.md and spec-56-17-materialize-for-a-while.md both carry status: 'done' with final_revision stamps (f519dbe, 830deba) and their code is present (virtual_policy.rs:260 floor_selects / VirtualPolicyTier::SizeFloor; db.rs:1135-1149 set_release_at, ReleaseSchedule::DueByRequest). sprint-status.yaml:882-952 lists 56-1 through 56-15 and stops. Two files also claim to be 56.14 (engine-side, done; surface-side, in-review) against one ledger row.

Why: The ledger is what a reviewer or retrospective reads. It understates epic 56 by the two stories that most recently changed release semantics - 56.16 armed the sweep for floor-only folders (which is what makes F-VF-1 and F-VF-2 reachable on hesperia at all) and 56.17 added a per-path deadline.

Fix: Add 56-16-a-floor-on-its-own-means-something and 56-17-materialize-for-a-while rows, and give the two 56.14 halves distinct keys.

## What is correct / well done

- The deletion proof is per object and fresh: remote_serves (engine.rs:10255) is called at the moment of deletion (:11526) and collapses every failure to false (:10340-10352); synced_at_ms only selects which clock applies and authorizes nothing (db.rs:902-906). So a stale synced_at_ms cannot cause a deletion - with electra down 6 days every candidate refuses UnprovenOnRemote. sync.db is per machine and rows are keyed by profile_id, so no cross-clone contamination.
- A rename cannot make the sweep release the wrong path: the ledger row is only a selector; release_resolved re-derives the committed pointer for that path from the index (:11397-11406) and hashes the actual bytes (:11509-11524), so it can only delete content that IS that path's committed content and that the server affirms now. A reused path is corrected by remember_materialized + note_arrival (engine.rs:8592-8612) or note_local_authorship.
- materialize_pending really does consult VirtualPolicy now, ahead of both the store check and the PointerOnly branch (engine.rs:8552-8556) - the memory note's defect is fixed. Tests: tests/virtual_arrival.rs:444, :496, :773 (the queued-before-the-policy case in materialize_landed).
- Engine::verify excuses virtual and only virtual, on four independently-earned facts - committed, authorized, absent-not-truncated, and held by a visible filesystem remote (engine.rs:12088-12146). Tests: virtual_state_is_not_a_fault.rs:400, :451, :480, :515, :543.
- The smudge filter never fetches: smudge serves only what the store already holds and passes the pointer through otherwise (filter.rs:192-215); delay is deliberately not advertised (:362-375, test the_handshake_agrees_on_version_two_and_refuses_delay); clean re-emits pointer input verbatim (:225-245). A foreign git checkout/add/stash in a folder with virtual paths leaves pointers alone and cannot commit a pointer-naming-a-pointer.
- The state vocabulary is total: EntrySyncStatus's eight variants map one-for-one onto FilesSyncStatusVm's eight (sync_ipc.rs:3528-3554); Waiting{reason}'s five reasons collapse into one wire variant plus a Rust-composed sentence. FilesReleaseVm is dropped unless the row is a materialized file (vm.rs:4433) and FilesDeletePlanVm::compose names all three new states in travels explicitly (vm.rs:4726-4733, tests :9423, :9519). No engine state the VM cannot spell.
- release_due_at is a pure function (engine.rs:13346) with the pin above the provenance branch and row.synced_at_ms? - no unwrap_or on the line that would let a locally-authored never-confirmed file become eligible; release_at_ms is read below it so --for 1m cannot get around FR-341 (release_sweep.rs:735, :2081).
- The gate ordering around the sweep is unusually careful: release_permits sits above release_is_due and removes an armed window (engine.rs:9498-9500, :9509-9527); a paused folder is checked first because sync_once does not check enabled (:9707-9710); a faulted folder config fails closed (:9718-9727).
- virtual_policy.rs's test suite is the strongest in this lane - BOM, backslash-bang escape, bare-punctuation lines, three anchoring spellings, negated-directory subtree expansion, protections-only lists, control files under a floor, and resolve_answers_identically_after_the_entire_worktree_is_deleted (:1189) fencing the no-I/O claim.

## Story map (epic 56)

| story | code | test |
|---|---|---|
| 56.1 policy that says which files may stay away | lfs/virtual_policy.rs (whole); profile/mod.rs virtual_patterns, virtual_over_bytes; folder TOML tier | virtual_policy.rs:666-1903 (36 unit tests); tests/virtual_policy.rs:74 (real git, tree stays clean) |
| 56.2 a listing that knows what it does not hold | browse.rs EntrySyncStatus::Virtual, lfs_oid, mtime_ms; lfs/listing.rs; db.rs:386 ensure_materialized_columns, :1266 materialized_rows; engine.rs:10420 lfs_files | tests/lfs_listing.rs:167-195; db.rs:4811 migration-in-place |
| 56.3 a file you can ask for | engine.rs:10519 materialize_entry, :10577 materialize_request, db::Urgency | tests/materialize_entry.rs:222, :328, :407 |
| 56.4 a release that refuses five times | engine.rs:11263 dehydrate_entry, :11382 release_resolved, :10255 remote_serves; lfs/hydrate.rs ContentRefusal | tests/dehydrate_entry.rs:292, :364, :425, :495, :560, :628, :684 (OpenUnknown) |
| 56.5 it lets go a day after it landed | engine.rs:9701 release_expired, :13346 release_due_at, :3656 release_is_due; db.rs note_arrival/note_local_authorship/note_synced/note_use/set_pinned | tests/release_sweep.rs:603, :735, :783, :830, :995, :1052, :1114, :1169, :1481 |
| 56.6 the checks stop calling the normal state a fault | engine.rs:11969 verify | tests/virtual_state_is_not_a_fault.rs:400, :451, :480, :515, :543 |
| 56.7 the row says what it is, and what a delete will do | browse.rs Materializing/Materialized/MaterializedView; vm.rs:3883 FilesSyncStatusVm, :4722 travels; sync_ipc.rs:3528-3554 | tests/lfs_listing.rs:299, :443; vm.rs:9423, :9519; browse.rs:2460, :2602, :2687 |
| 56.8 a virtual-files chapter | docs/sync.md:649-1304 (§9) | none (doc); §18 status text is the only cross-check |
| 56.9 the button and the time you have left | engine.rs:13396 ReleaseSchedule, :12662 release_schedules, :10550 release_instruction; vm.rs:4331 release; files-pane.tsx:443, :540 | engine.rs:17600-17760 release_schedule unit tests; tests/release_sweep.rs:1527 |
| 56.10 the policy decides what arrives | engine.rs:8552 materialize_pending, :8340-8360 materialize_landed, :13209 release_mode_gate, :13267 release_path_gate | tests/virtual_arrival.rs:444, :496, :543, :594, :650, :683, :773, :816, :878 |
| 56.11 is this file open, answered for real | platform.rs:59 probe_open_file_state, :165 open_file_state_under_proc; keeper-syncd/platform.rs:468 | platform.rs:989, :1020, :1056, :1092-1294; dehydrate_entry.rs:2133, :2201 (Linux-gated) |
| 56.12 drive settings for virtual files | shell crate + TS form controls | not found in this lane - keeper crate does not build on Linux |
| 56.13 what the end-to-end run found | spread across engine.rs/stage.rs fixes | e2e script /tmp/vf-e2e.sh per sprint-status.yaml:917-925 - not in the repo |
| 56.14 the deferred sweep (engine side) | ReleaseSchedule::Indefinite; db.rs:1417 retaining forget_materialized; engine.rs:11509 spawn_blocking hash; :11570-11600 hard-link reclaim; :10322-10360 path-remote fallback | release_sweep.rs:1384, :1814; dehydrate_entry.rs:425, :495 |
| 56.14 the deferred sweep (surface side) | files-pane.tsx:443, :2549 release gating | frontend suite (not read this lane); spec still status: in-review |
| 56.15 a clone that stopped says so | WorkKind::Checkout, empty-index refusal in stage_and_commit | out of lane |
| 56.16 a floor on its own means something | virtual_policy.rs:260 floor_selects, :399 authorizes_anything, VirtualPolicyTier::SizeFloor | virtual_policy.rs:1627, :1676, :1706, :1741, :1781, :1819, :1849, :1882 |
| 56.17 materialize for a while | db.rs:1135 set_release_at, MaterializedRow::release_at_ms; engine.rs:10550 release_instruction, ReleaseSchedule::DueByRequest | release_sweep.rs:1895, :1949, :2002, :2081, :2155 |

## Answers to the nine questions

**1_policy_parse_precedence_and_missing_file** — Parse and precedence are correct and heavily tested. Read from the worktree at virtual_policy.rs:196, never HEAD. Precedence: profile permissive list replaces the file's wholesale (only the permissive half - a !-only list does not mute the committed zone, :241-256); protections union across all sources; floor is a floor under everything and the selector when no permissive line exists (:260). Missing file = silence (F-VF-5: wrong when the checkout simply did not deliver it). Unreadable = hard SyncError::Config. A pointer-text .keepervirtual is NOT detected (F-VF-6).

**2_materialize_pending_consults_policy** — Yes, fixed in #284 (56.10). engine.rs:8495 compiles once per pass; :8552-8556 consults it ahead of both store.contains and the PointerOnly branch. Test: tests/virtual_arrival.rs:444 a_virtual_path_keeps_its_pointer_though_the_object_is_already_here, plus :496 (no fetch) and :773 (a unit queued before the policy does not publish when it lands).

**3_release_proof_before_delete_and_two_clocks** — The proof is a fresh per-object LFS batch download call taken at the moment of deletion (engine.rs:11526 -> remote_serves :10255), not synced_at_ms and not a HEAD request; a filesystem remote is proved by a size-verified store.contains. synced_at_ms is only a candidate selector, so it cannot be stale-true into a deletion - with the remote down 6 days everything refuses UnprovenOnRemote. materialized is keyed by (profile_id, path); a rename leaves an orphan row but it cannot release the wrong path (release_resolved re-derives the committed pointer and hashes the bytes) - it does burn budget forever (F-VF-3).

**4_macos_refusal_is_total_and_unsurfaced** — The refusal is total: probe_open_file_state returns Unknown on every non-Linux target (platform.rs:70-75) and release_resolved refuses OpenUnknown (:11539-11542), so no path releases on the owner's Mac by any door. Neither the UI nor the CLI says so - release_schedule has no platform term and the Files row counts down (F-VF-1). Only docs/sync.md:2962-2977 records it.

**5_materialized_ledger** — Written by remember_materialized on every publish-over-a-pointer, plus observe_materialized/note_local_authorship/set_pinned/set_release_at. It is one row per path-ever-hydrated per profile, never deleted (F-VF-7), so 89 289 rows on hesperia against 72 641 LFS-tracked index paths. Indexed for the sweep's query by the (profile_id, path) primary key - range scan in path order, no sort - but the sweep reads and allocates every non-released row of the profile each pass, then filters in Rust.

**6_sparse_checkout** — Live, not dead: three production consumers (reconcile_sparse_cone, the LFS transfer filter, the release cone check). Unused by all three hesperia profiles because subpaths[] is empty. index.sparse=false is mandatory because gix::status hard-fails on a sparse index (F-VF-9).

**7_entry_sync_status_vs_vm** — All eight EntrySyncStatus variants map one-for-one onto FilesSyncStatusVm's eight (sync_ipc.rs:3528-3554). No state the engine can produce that the VM cannot spell; Waiting{reason}'s five reasons collapse into one variant plus a Rust-composed sentence, by design.

**8_lfs_filter_interaction** — smudge never fetches - it serves only what the store already holds and otherwise passes the pointer through (filter.rs:192-215), and delay is deliberately not advertised. So a foreign git checkout in a folder with virtual paths leaves the pointer. clean re-emits pointer input verbatim (:225-245), so git add/commit -a/stash cannot commit a pointer-naming-a-pointer.

**9_verify_excuses_virtual_only** — Yes. Four independently-earned facts - the index commits this exact pointer, the policy authorizes this path (or lfsMode is PointerOnly), the object is absent rather than truncated, and a visible filesystem remote holds it (engine.rs:12088-12146). LfsMode::Disabled earns nothing. Only cost quibble is the folder-level gate asking tier()!=Unset instead of authorizes_anything() (F-VF-8).

## Open questions

- Whether any hesperia folder has ever reached the sweep at all: release_expired runs on mark_synced's success edge (engine.rs:5734), and with electra unreachable since 2026-09-02 and pull rows at 615 attempts it is likely no pass has succeeded in 6 days - so F-VF-1/F-VF-2 are latent-but-armed rather than currently burning. Settling it needs a log grep for 'released content whose retention window had expired' / 'release sweep declined a candidate'.
- Whether tgdrive-light actually commits a .keepervirtual: the evidence gives virtualOverBytes=1 MiB from the profile row but not whether a pattern file exists, which decides whether the tier is SizeFloor (F-VF-1 fires) or PatternFile.
- The 56.12 drive-settings surface and the sync-status-mark.tsx Record maps are in crates/files I could not compile or fully read; the eight-glyph totality claim rests on the spec's execution checklist rather than my own reading.
- Whether a .keepervirtual pointer has ever actually occurred - the sibling case (.gitattributes as a pointer, 1.3 M warnings) is proven in the evidence; the .keepervirtual consequence is [INFERENCE] from compile's code path.
