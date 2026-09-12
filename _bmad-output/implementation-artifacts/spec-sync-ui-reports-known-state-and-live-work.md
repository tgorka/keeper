---
title: 'Sync UI reports known state and live work'
type: 'bugfix'
created: '2026-09-11'
status: 'done'
review_loop_iteration: 0
baseline_commit: 'f9a337d37a63b051c837aa7efe0746156d5574fd'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** On hesperia/v0.8.27, tgdrive said “up to date” while HEAD was one commit behind its already-fetched `origin/main`. Pending scans also left “Syncing” and completed counters visible after work ended. Empty queues prove neither remote equality nor ongoing activity.

**Approach:** Compare local history with the last-known upstream history using cheap read-only reference inspection. Give Pending's scan progress its own scoped lifetime; primary sync activity and stopped states take precedence. Rust authors the verdict; existing UI/IPC surfaces render it.

## Boundaries & Constraints

**Always:** Distinguish equal, ahead, behind, diverged and unknown history. Qualify claims as relative to the *last known remote*, not fresh server verification or LFS-content verification. Keep stopped-state/error precedence and real scan visibility. Retire poll progress on success, error, unwind and cancellation without clearing another operation.

**Ask First:** Changing sync policy, installing over the user's running app, merging or releasing.

**Never:** Change fetch/watcher/scan schedules, trigger network work from status reads, walk the worktree/index there, spawn git for polling, mutate repository/configuration as an observational side effect, infer equality from empty queues or `last_sync_ms`, add a DB migration or duplicate status policy in React.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Quiet history | Equal valid HEAD/tracking tip | Local history matches last-known remote; no absolute “up to date” claim | Not a proof about server freshness or file content |
| Outstanding history | Behind / ahead / diverged tips, empty queue | Explicit corresponding relation | Missing history needed for classification yields unknown |
| No reliable comparison | Missing refs/root, wrong branch/remote identity, unreadable repository | Remote comparison unknown | Never convert failed reads into equality |
| External git change | HEAD, tracking ref or profile identity changes | Next status read invalidates prior relation | Unchanged tips reuse cached ancestry |
| Pending scan | No primary activity or stopped state | Measured Scanning while the owned scan runs; no completed bar afterward | Scoped cleanup also on failure/cancellation |
| Overlap / stop | Scan overlaps transfer, pause, offline or terminal error | Primary activity or stopped state wins and survives scan completion | No compensating overwrite after the fact |
| Delayed stream | Old Scanning frame beside Fetching or Idle snapshot | No stale filename/count/rate/fraction from a different or inactive phase | Snapshot remains authoritative |

</frozen-after-approval>

## Code Map

- `src-tauri/crates/keeper-sync/src/engine.rs`: `status`/`statuses` (~2704) clone primary snapshots; `fill_queued` enriches reads. `publish` (~2774) currently folds poll frames into primary state. `pending` (~15077) drains scan frames with no Reservation. `WalkClaim` (~1615) scopes walking, while `Reservation::drop` (~15953) clears only primary progress. `settle_in`/`has_problem` preserve existing failure semantics. Per-profile maps are initialized in `open` and removed in `remove_profile`.
- `src-tauri/crates/keeper-sync/src/git/repo.rs`: reuse `open_read_only`, `head_commit_id`, `resolve_reference`; do not use repairing `open`, index status or commit counts. Status-only ancestry uses a fallible revision walk: existing `is_ancestor` treats merge-base `NotFound` as false even when a parent object is missing. Keep that sync-policy helper unchanged. Cache classification by typed object IDs and profile/repository identity, outside status/DB locks. No fetch API change is needed.
- `src-tauri/crates/keeper-sync/src/progress.rs`: `SyncStatus::idle`, new internal `RemoteRelation`, and `status_line` calm fallback. All constructors use `idle`; serde consumers inherit the optional relation.
- `src-tauri/crates/keeper/src/sync_ipc.rs`: `From<&SyncStatus>` already calls `status_line`. `SyncStatusVm`, generated TypeScript, tray and daemon callers retain their shape.
- `src/lib/stores/sync-detail.ts`: `syncLiveFraction`/`syncLiveRate`; `src/components/layout/sync-pane.tsx`: streamed details (~1004), Pending empty sentence (~202). Existing phase/activity gates are reused, not replaced by timers.
- Tests: engine's `a_pending_poll_publishes_the_progress_of_its_own_walk`, real-repo fixtures and `arrange_a_pass_in_flight`; `src/components/layout/sync-pane.test.tsx` captured progress sink exercises transitions.
- `src/components/settings/sync-section.tsx` and colocated tests: use the same non-idle live-progress boundary as the main pane, while retaining the existing activity predicate for polling.
- `src-tauri/crates/keeper-sync/src/profile/mod.rs`: correct only the Watching and hourly backstop documentation; no state-machine or interval change.

## Tasks & Acceptance

**Execution:**
- [x] `engine.rs` — derive and memoize last-known history comparison on read; isolate Pending progress under a scoped overlay. Preserve primary ownership and release locks before sink callbacks. Add consumer-level real-repository and lifetime/interleaving regressions.
- [x] `progress.rs` — optional `RemoteRelation::{Same,Ahead,Behind,Diverged}` (unknown is `None`), default unknown, qualified calm verdicts; retain active/stopped/pending precedence and test meaningful boundaries.
- [x] `sync-detail.ts`, `sync-pane.tsx`, colocated tests — select only active matching-phase stream details; narrow empty-list copy to “No pending files reported.” Prove stale stream cannot paint a later phase.
- [x] `docs/sync.md` — document Watching versus activity, history comparison limits and unchanged scheduling; record verification here.
- [x] `dev/mock-shell.ts` — repair the existing invalid Sync fixture shapes and type them against generated VMs so the actual pane can be rendered without a native shell.
- [x] Review patches — compare the Worktree lane's effective branch, hide retired Settings progress, skip optional stopped-state probes, retain memoized proofs across transient input failures, enable the existing bounded object-cache pattern for cold history walks, and re-run gates.

**Acceptance Criteria:**
- Given a clean local checkout behind a fetched upstream commit, when every existing status consumer reads it, then it reports behind without triggering fetch, scan or repository writes.
- Given a finished Pending request and no other work, when the next snapshot renders, then no Syncing badge, completed scan counter or stale rate remains.
- Given overlapping poll/pass activity or a stopped folder, when scan frames arrive and the poll exits, then they neither erase primary activity nor revive the stopped state.
- Given the real Sync pane at narrow and ordinary widths, when the history verdict changes and Pending completes, then the verdict remains readable and controls remain usable.

## Spec Change Log

- Browser preflight found `sync_problems: { profiles: [] }` crashing `SyncProblemsSection` on `.length`, and status fixtures missing required fields. Corrected only the development fixture, with `satisfies SyncProblemsVm` / `SyncStatusVm[]`; no production fallback or wire change.
- Regression testing exposed merge-base's ambiguous `NotFound`: a missing parent was incorrectly classified as divergence. The read-only status comparison now uses a fallible revision walk; existing sync/merge policy remains unchanged.

## Design Notes

The user requested planning **and implementation** together. This bounded spec proceeds under that authorization. No separate freshness clock: “last known remote” states the evidence boundary without conflating successful local/LFS work with branch contact. The poll overlay is not a second durable status and must not take the writer Reservation. The pre-existing blocking-walk cancellation/claim race is not expanded into a scheduling redesign.

## Verification

- Focused Rust real-repository and scan lifecycle tests; demonstrate regressions fail without the fix.
- `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core -p keeper-sync` and corresponding Clippy; `RUSTUP_TOOLCHAIN=stable` selects the installed toolchain. Nextest is unavailable locally.
- `bun run check`; real browser render of the app's modules/CSS and exercised status transitions. If shell Rust changes become necessary, run the macOS gate; never claim a Linux shell gate.
- Initial repro: extending the existing real Pending scan test with completed-state assertions fails on v0.8.27 (`Syncing` remains after `pending` returns).
- Isolated reproduction rerun after clearing commit-fixture progress: the same completed-state assertion fails (`Syncing == Syncing`), proving the Pending call rather than setup owns the stale state.
- BMAD specialist implementation workers were unavailable (provider HTTP 429 before edits); the same spec and disjoint file ownership were handed to available OpenAI workers. Review and verification remain required.

### Executed gates

- Pre-review baseline gates: `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core -p keeper-sync` passed 4,464 tests with one pre-existing ignored core test; the sync unit suite ran all 1,283 tests. CLI build and Clippy passed. Frontend lint, typecheck and 5,790 tests in 341 files passed; its final dependency-tree leg needed `RUSTUP_TOOLCHAIN=stable` to avoid the local Rustup cross-device update failure. Existing Biome warnings and the `proc-macro-error2` future-compatibility notice are outside this change.
- Final post-review gates passed: workspace `cargo fmt --all --check`; the complete `keeper-core` / `keeper-sync` suite with the unchanged recording-search performance case run separately as described below; `cargo build -p keeper-syncd`; `cargo clippy -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings`; and all of `bun run check`, including 5,791 tests in 341 files and all three dependency-tree boundaries. Cargo used the same `src-tauri/Cargo.toml` manifest and `RUSTUP_TOOLCHAIN=stable`.
- Resource isolation: the first post-review combined run hit the untouched recording-search NFR at 67.93 ms versus its calibrated 63.91 ms budget. A concurrent, independently owned `kb-sweep` held about 4 GB in the 8 GiB container. The identical performance executable passed pinned to CPU 0 at 62.44 ms against 63.88 ms; no threshold or test was changed. The final combined run excluded only that separately executed case, used two CPUs and one Cargo build job, then ran the frontend on three CPUs. An earlier overlapping compiler/typecheck run had hit the container memory limit.
- Mutation proof: restored the old unconditional “up to date” fallback and separately removed the stream phase-match guard. The exact focused tests passed before mutation, failed on the old claim / stale 100% bar, and passed after restoration. Both files were restored byte-for-byte. The original Pending reproduction also passes in the complete sync suite.
- Actual CLI: built `keeper-syncd` against a temporary real local/bare repository pair. JSON and human `status` reported behind, same, ahead, diverged and unknown; behind remained usable with the remote directory unavailable. Every status command left HEAD, refs, index and config byte-identical.
- Post-review CLI: the rebuilt binary compared `keeper/tgdrive/p1` against its own tracking ref, reporting same despite a different base `origin/main`; deleting only the lane tracking ref reported unknown, not the base relationship. Both human and JSON reads again left HEAD, refs, index and config byte-identical. Eight CLI scenarios completed overall.
- Actual renderer: the app's Sync pane and compiled CSS ran through Vite in the macOS Paseo browser, with the development IPC fixture—not the installed Tauri app. At 780 px the pane was 732 px and verdict column 303.26 px; all five verdicts fit, with divergence wrapping to two 16 px lines. At 1100 px the pane was 944 px and the verdict column 515.26 px; the long verdict fit one line. No document horizontal overflow. Opened and cancelled Edit folder at 780 px.
- Rendered transition: Scanning displayed 100% and its current filename; Fetching rejected the delayed scan frame and removed its bar/count/filename/rate; a matching LFS frame displayed 25%, filename and rate; Watching/Idle removed all live details. Screenshots confirmed the 780 px divergence and 1100 px behind/idle states.
- Browser harness corrections were not product defects: resizing the resident browser reloaded its document, and importing an unversioned Vite store created a second instance beside the live HMR-versioned import. The successful stream exercise imported the exact URL served to `SyncPane`.
- Settings progress was verified in the real `SyncSection` module with compiled application CSS at a measured 768 px width: Idle plus three queued files and retired 100/100 counters showed no meter; Pushing 1/4 showed exactly 25%; returning to Idle removed it. Screenshot confirmed the queued sentence and absent meter. This was an independent production-component mount, not a claim that the whole Settings development fixture works: unrelated null hotkey/capture/list-settings replies break that fixture and are recorded in deferred work.
- No Tauri shell, IPC VM or generated binding contract changed. No installed app, real sync folder, remote branch or release was modified.
- Cleanup: stopped the owned Vite server and reverse SSH tunnel, closed the verification tab, and removed disposable repositories and capture files. The `code` command is unavailable here, so the spec was not auto-opened in VS Code.

### Matrix traceability

All listed covering tests ran and passed; none of these tests was skipped.

| Matrix row | Executed covering tests |
|---|---|
| Quiet history | `quiet_history_reports_only_the_known_remote_relation`; `pending_and_settling_work_outrank_remote_equality`; `status_history_compares_the_worktree_lane_not_its_base` |
| Outstanding history | `status_history_tracks_external_refs_and_reuses_proven_ancestry`; `status_history_recovers_missing_ancestry_without_refs_moving` |
| No reliable comparison | `status_history_is_unknown_for_missing_refs_and_wrong_identity`; `status_history_recovers_missing_ancestry_without_refs_moving`; `status_history_compares_the_worktree_lane_not_its_base` |
| External git change | `status_history_tracks_external_refs_and_reuses_proven_ancestry`; `status_history_is_unknown_for_missing_refs_and_wrong_identity` |
| Pending scan | `a_pending_poll_publishes_the_progress_of_its_own_walk`; `cancelling_pending_retires_its_live_scan_without_settling_primary`; `pending_worker_error_retires_reported_scan_progress`; `pending_sink_unwind_retires_reported_scan_progress` |
| Overlap / stop | `pending_scan_yields_to_primary_and_stopped_states_between_sinks`; `pending_scan_never_revives_an_already_stopped_profile`; `pending_scan_cannot_cover_an_error_on_an_idle_primary`; `an_old_poll_owner_cannot_erase_a_readded_profiles_new_scan`; `stopped_states_outrank_activity_and_remote_relation` |
| Delayed stream | `retires scan details across a fetching snapshot, its live frame, and idle`; Settings regression `does not turn queued work or retired counters into live progress` |

### Review triage

- Three independent BMAD passes completed: blind/adversarial review, branching and edge-case review, and verification-gap review. No frozen-intent gap or bad-spec contradiction was identified.
- **Patch / high:** Worktree comparison must resolve `working_branch(profile)`, not its base branch. The new real-repository test failed with `None != Some(Same)` before the product correction; deleting lane tracking must remain unknown even when base tracking exists.
- **Patch / medium:** Settings → Sync must not show a 100% meter for an Idle snapshot with queued work and retired counters. Its new consumer regression failed against the old renderer before adding the non-idle guard.
- **Patch / performance and documentation:** enable the existing 4 MiB cold object-cache pattern, avoid optional repository probes for Paused/MediaAbsent, retain a previously keyed proof across a transient unreadable input without returning it for that failed read, and document cold graph cost and the stored-tracking-ref boundary. Correct Watching and one-hour backstop comments. The strengthened original Pending regression now fails loudly if its engine fixture cannot open.
- **Defer / pre-existing:** recorded separate ledger entries for ownerless remote-object audit progress, remote-contact scheduling starvation, the blocking-worker/WalkClaim cancellation race, cross-operation same-phase frames, missing Gix post-push tracking updates, and Gix Worktree push-branch routing. Browser verification also recorded the unrelated invalid Settings development fixtures. None is hidden by a renderer workaround or an unrequested sync-policy change.
- Deliberately retained boundaries: exact configured repository identity, snapshot authority, conservative unknown on incomplete history, and no new progress-generation wire contract. A cold divergence classification may walk both histories; memoized unchanged reads do not repeat it.

## Suggested Review Order

**Truthful status reads**

- Status reads compose primary work, scoped polling and qualified history without changing sync policy.
  [`engine.rs:2760`](../../src-tauri/crates/keeper-sync/src/engine.rs#L2760)

- Inspect memoization, fallible ancestry and effective-branch identity together.
  [`engine.rs:2826`](../../src-tauri/crates/keeper-sync/src/engine.rs#L2826)

- Calm sentences state the known history relationship instead of promising fresh remote equality.
  [`progress.rs:664`](../../src-tauri/crates/keeper-sync/src/progress.rs#L664)

**Progress ownership**

- A token-scoped owner retires only its own presentation overlay.
  [`engine.rs:1638`](../../src-tauri/crates/keeper-sync/src/engine.rs#L1638)

- Primary and stopped states win; callbacks run outside locks.
  [`engine.rs:2976`](../../src-tauri/crates/keeper-sync/src/engine.rs#L2976)

- Follow the real Pending scan through cancellation, failure and normal completion.
  [`engine.rs:15348`](../../src-tauri/crates/keeper-sync/src/engine.rs#L15348)

**Rendering authoritative snapshots**

- Only matching active phases may borrow streamed details.
  [`sync-detail.ts:179`](../../src/lib/stores/sync-detail.ts#L179)

- The main pane consumes the shared selector instead of retaining retired scan details.
  [`sync-pane.tsx:1002`](../../src/components/layout/sync-pane.tsx#L1002)

- Queued Idle work does not become a live Settings meter.
  [`sync-section.tsx:622`](../../src/components/settings/sync-section.tsx#L622)

**Regression evidence**

- Real repository history must compare the Worktree lane, never its base.
  [`engine.rs:17428`](../../src-tauri/crates/keeper-sync/src/engine.rs#L17428)

- The original real-walk regression now requires completed progress to disappear.
  [`engine.rs:25151`](../../src-tauri/crates/keeper-sync/src/engine.rs#L25151)

- Exercise scanning, fetching, a matching transfer and final retirement through the consumer.
  [`sync-pane.test.tsx:822`](../../src/components/layout/sync-pane.test.tsx#L822)

- Queued work and stale completed counters cannot resurrect the Settings meter.
  [`sync-section.test.tsx:546`](../../src/components/settings/sync-section.test.tsx#L546)

**Supporting contracts and limits**

- Development Sync fixtures satisfy the existing generated view-model shapes.
  [`mock-shell.ts:1375`](../../dev/mock-shell.ts#L1375)

- Watching describes work observation, not proven replication.
  [`profile/mod.rs:102`](../../src-tauri/crates/keeper-sync/src/profile/mod.rs#L102)

- Document history confidence, cold-query cost and unchanged scheduling.
  [`sync.md:1501`](../../docs/sync.md#L1501)

- Keep discovered pre-existing producer, transport and fixture defects visible without expanding this fix.
  [`deferred-work.md:6073`](deferred-work.md#L6073)
