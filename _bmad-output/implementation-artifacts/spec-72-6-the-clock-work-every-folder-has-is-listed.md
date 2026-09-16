---
title: 'Story 72.6: the clock work every folder has is listed'
type: feature
created: '2026-09-16'
status: done
baseline_revision: a8eb9c2
final_revision: ''
---

<intent-contract>

## Intent

**Problem:** Only Gc has a seeded task row. Sync and Verify exist as task kinds but are not offered. Per-folder disabled Sync proposals would shadow host-wide schedules, and the existing least-permissive Sync fold lets disabled siblings restore a competing paced driver.

**Approach:** Per Main's measured amendment, offer one host-wide Sync and one host-wide Verify row once per desktop machine, beside the existing per-folder Gc rows. Both are Scheduled and disabled. Sync remembers the 1-hour live-watcher backstop; Verify proposes weekly verification. Fix Sync's within-tier aggregation to prefer the most capable row; keep enabled-to-Off mapping and folder-tier precedence. Release retains its least-permissive deletion safety.

**Always:** Preserve user edits and deletion across opens; keep paced projection and filesystem triggers unchanged. Host-wide proposals must not shadow a user's host-wide schedule or alter folder-tier precedence.

**Block If:** Any proposal creates a second driver, or weakens Release's refusal to delete under conflicting instructions.

**Never:** Modify TaskRow, TaskKind, upsert_task, perform_task, UI, or keeper-core; add a scheduler or new gate; claim verification was already paced by the scratch sweep.

## I/O & Edge-Case Matrix

| Input / state | Expected behavior | Handling |
| --- | --- | --- |
| Desktop open | One disabled scheduled host-wide Sync and Verify | Marker per kind/machine |
| Second open or more folders | No duplicate or replacement | Persisted marker |
| Deleted seed | Stays deleted | Marker survives |
| Existing row at seed id | User row unchanged | Mark offered |
| Disabled host Sync proposal alone | Paced poll runs for each folder; governance Off | No folder-tier row invented |
| Enabled proposal | Paced scan stands down | Existing gate, Sync max fold |
| Proposal plus authored enabled host-wide Sync | Paced scan stands down | Same-tier maximum |
| Only authored Sync switched off | Paced poll resumes | enabled=false still maps to Off |
| Local Off plus host Scheduled | Paced poll runs for that folder | mine.or(host_wide) unchanged |
| Release Off plus Release Scheduled | Deletion veto remains | Release min fold unchanged |
| Disabled Verify, time advanced past a week | No run and no armed window | Existing task scheduler |
| Phone | No new seeds | Existing binary-engine gate |
| Paced projection | Scan, scratch sweep and optional notes remain | No projection edits |

</intent-contract>

## Code Map

Baseline anchors:
- `src-tauri/crates/keeper-sync/src/db.rs:4316-4409`: Gc cadence, id and marker-based seeding; host proposals added beside it.
- `src-tauri/crates/keeper-sync/src/engine.rs:1980-2021,2629`: open and profile-upsert seeding doors.
- `src-tauri/crates/keeper-sync/src/engine.rs:552,597,619`: remote eligibility floor 5 min, live-watcher scan backstop 1 h, scratch/footprint sweep 24 h.
- `src-tauri/crates/keeper-sync/src/engine.rs:4392-4459`: Verify calls the read-only verification verb, not the paced scratch sweep.
- `src-tauri/crates/keeper-sync/src/engine.rs:12169-12211,12350-12368`: shared governance fold and scan permission verdict.
- `src-tauri/crates/keeper-core/src/tasks.rs:966-983`: projected scan/sweep/notes; unchanged.
- `docs/sync.md:2120,2624,3307`: task chapter, paced projection, periodic inventory.

Governance aggregation, verbatim from baseline `engine.rs:12191-12211`:

```rust
// A row that is not live is a knob set to off, not a knob that is
// absent. See [`Self::release_governance`]'s doc.
let mode = if task.enabled {
    task.mode
} else {
    tasks::TaskMode::Off
};
let tier = match task.profile_id.as_deref() {
    Some(id) if id == profile_id => &mut mine,
    // Another folder's decision is about another folder. Folded in
    // here it would read as host-wide and switch this one off.
    Some(_) => continue,
    None => &mut host_wide,
};
*tier = Some(match *tier {
    Some(existing) if rank(existing) <= rank(mode) => existing,
    _ => mode,
});
}
// The narrower statement first.
Ok(mine.or(host_wide))
```

Rank immediately above: Off = 0, Manual = 1, Scheduled = 2. `sync_poll_permits` permits None/Off/Manual and declines Scheduled. Both `enabled` and `mode` therefore matter.

Release safety, verbatim `engine.rs:12094-12099`:
> Within one tier the **least permissive** mode wins — `Off` before
> `Manual` before `Scheduled` — because the safe reading of two rows
> disagreeing about a deletion is the one that deletes less. Two rows in
> one tier is a muddle a person made, and the answer to a muddle about
> deleting content is not to pick the row that deletes. The alternative,
> "whichever sorted first by id", would make the answer depend on a name.

## Tasks & Acceptance

- [x] Escalate and resolve both same-tier aggregation and local proposal shadowing.
- [x] Seed once per desktop host and preserve remembered decisions.
- [x] Prove seeding, cadence and governance through scoped Rust tests.
- [x] Document seeded versus paced work and honest cadences.

**Original epic acceptance (verbatim):** a db test proves the seeder writes one `Sync` and one `Verify` row per folder exactly once (marker respected across two opens) and that both are `enabled: false`; a test proves an enabled seeded `Sync` row governs the paced poll (stands it down) and a disabled one does not; a test proves a seeded row's schedule matches the cadence the engine paces itself at; the paced section still lists every gate with no settable schedule.

**Binding coordinator amendments (2026-09-16):** Replace per-folder Sync/Verify with one host-wide row each (`profile_id: None`), once per machine. Sync uses LIVE_WATCH_BACKSTOP_MS (1 h); Verify defaults to `every 7d` as an explicitly labelled proposal, not a pre-existing verification cadence. Sync's within-tier fold uses max rank; Release keeps min. Preserve `enabled=false` → Off and `mine.or(host_wide)`. Tests must leave proposals present and cover disabled default, authored enabled host sibling, a sole authored row switched off, and local Off overriding host Scheduled.

## Design Notes

The first escalation found disabled proposals block scheduled siblings through the existing min fold. The initial amendment skipped seeding where a Sync row already existed, but that left the live case of a newly authored sibling broken. The second measurement identified that per-folder proposals also shadow all subsequently authored host-wide schedules through correct folder-tier precedence. Main explicitly chose host-wide seeds plus a Sync-only max fold. Release's deletion-safety quote above is the reason not to apply max to both kinds.

Measured before changing the fold: throwaway `clock_task_probe_hand_authored_disabled_sync_blocks_enabled_sync` wrote two hand-authored Scheduled local rows, one disabled and one enabled, over a profile inserted without seeding. It passed its assertion that governance was Off and the paced poll permitted (1 passed, 1289 filtered): the competing driver pre-existed. The throwaway proof is converted into a regression asserting Scheduled and poll refused, plus a check that the sole disabled authored row permits pacing. Host proposals remain in every fixture.

No paced row is removed. No scheduler or gate is introduced. Verification is read-only and distinct from scratch/footprint checks; its weekly proposal is labelled honestly. The 24-hour scratch cadence remains separate from the hourly release/helper look. Core's stale scratch-sweep comment at `keeper-core/src/tasks.rs:1022` remains untouched and is reported for wave 2.

The requested triage file was read in full via decoded `.text`; it summarizes the inventory but does not contain its claimed 12-row report (it refers to a prior report section). Actual constants and docs §21 were inspected directly.

## Verification

- `cargo nextest run --manifest-path src-tauri/Cargo.toml -p keeper-sync -E 'test(task)'`: failed before compilation; default rustup update failed EXDEV.
- Same command with `RUSTUP_TOOLCHAIN=stable-x86_64-unknown-linux-gnu`: failed because cargo-nextest is absent.
- `cargo install cargo-nextest --locked` with that override: started, then cancelled on Main's instruction to use cargo test instead.
- `RUSTUP_TOOLCHAIN=stable-x86_64-unknown-linux-gnu cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync task`: initial intermediate implementation 82 passed (77 lib + 5 integration); not final verification because the contract changed afterward.
- `RUSTUP_TOOLCHAIN=stable-x86_64-unknown-linux-gnu cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync clock_task_probe_hand_authored_disabled_sync_blocks_enabled_sync`: first attempt failed compilation because the host-wide seeder signature changed while cargo waited for the shared build lock.
- `RUSTUP_TOOLCHAIN=stable-x86_64-unknown-linux-gnu cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync --lib clock_task_probe_hand_authored_disabled_sync_blocks_enabled_sync`: **1 passed**, 1289 filtered; measured the pre-existing defect above.
- `RUSTUP_TOOLCHAIN=stable-x86_64-unknown-linux-gnu cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync --lib clock_task_`: **before fix, 4 passed / 2 failed**. Both `clock_task_disabled_authored_sync_cannot_veto_an_enabled_sibling` and `clock_task_proposal_does_not_veto_a_host_schedule_or_change_folder_precedence` failed with `left: Some(Off), right: Some(Scheduled)`. **After fix, 6 passed**, 1284 filtered.
- `RUSTUP_TOOLCHAIN=stable-x86_64-unknown-linux-gnu cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync task`: **final 84 passed, 0 failed** (79 library + 5 release-sweep integration; 1415 filtered across targets).
- `RUSTUP_TOOLCHAIN=stable-x86_64-unknown-linux-gnu cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync --lib governance`: **2 passed**, 1288 filtered — Sync max and Release min.
- `RUSTUP_TOOLCHAIN=stable-x86_64-unknown-linux-gnu cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync --lib governed`: **2 passed**, 1288 filtered — watcher/settle triggers and Pending status walks survive governance.
- `RUSTUP_TOOLCHAIN=stable-x86_64-unknown-linux-gnu cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync --lib sync_poll_permits`: **1 passed**, 1289 filtered — all task modes.

Reachability is exercised through real `Engine::open` and its public task listing; scheduler tests advance the injected clock past eight days and observe no armed window or history for disabled proposals. The cadence test parses the stored Sync schedule and compares its next window to LIVE_WATCH_BACKSTOP_MS. Db tests close and reopen the actual SQLite file, preserving deletion and edits. No projection code changed; scan, scratch sweep and optional notes still come from `paced_work`.

No shell crate or bindings were changed. No git commands, repo-wide gates, or formatters were run. The coordinator owns final formatting and the nextest rerun where installed. Existing Gc assertions were scoped to Gc rather than assuming it was the only kind; release integration now asserts the actual contract (no Release row invented) rather than an incidental exact list of all kinds. No wording-only test was added or re-pinned. No throwaway script remains.

## Shipped in

PR #363 of stack #364 (epic 72), branch `epic72/tasks`. The macOS gate (`bun run check:rust:macos`) passed on hesperia over the stack tip, which is where the `keeper` shell crate compiles at all.
