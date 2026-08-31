---
title: 'Story 58.7: everything else this host paces'
type: 'feature'
created: '2026-08-31'
status: 'done'
baseline_revision: '3d8735a'
final_revision: 'c0549ad'
review_loop_iteration: 1
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** The owner asked to see *"joby synchronizacji git czy inne zaschedulowane w keeperze rzeczy w tym widoku"*. ⌘8 shows `tasks` rows and nothing else, so the work keeper actually paces on this machine — the per-profile scan (`engine.rs:3106-3119`, interval `profile.effective_poll_interval_ms()`, `profile/mod.rs:1083-1085`), the hourly LFS scratch sweep (`engine.rs:3141-3152`, `SWEEP_EVERY_MS` `:443`) and the notes cadence (`notes_vault.rs:2550-2566`, `profile/mod.rs:202-206`) — is invisible. It cannot be shown by pointing the view at a table: `db::complete` is `DELETE FROM journal WHERE id = ?1` so a finished unit leaves no trace, and `activity` is by its own doc *"a human-facing log, not a source of truth"* (AD-135, AD-141).

**Approach:** A read-only class **projected at read time from the profile list**, in the same view, as a distinct non-task section. Every row is derived from state that already exists; no timer, no interval and no due-gate is registered (AD-142). The pure projection and its wording live in `keeper_core::tasks` beside `task_host`; one new command reads it; the pane renders it with its existing fold, `label-caps` heading and loading-versus-empty idiom. Nothing writes a `tasks` row for projected work.

## Boundaries & Constraints

**Always:**
- **Project, never schedule.** No `tokio::time::interval`, no due-gate, no second pacer anywhere. `src/test/task-host-tick.test.ts` must still find exactly one interval in `keeper/src`. The frontend reads the projection **inside the existing `refresh()`**, so it costs no read the pane did not already make, and relative wording rides the pane's one display clock (`now`, `TASKS_CLOCK_TICK_MS`).
- **Under-claim.** `next_scan_ms` / `next_sweep_ms` are an in-memory `Mutex<HashMap>` discarded on restart, and `scan_due` is `paced || watch_wake_pending || settle_window_elapsed` (`engine.rs:3288-3291`) — two of three triggers are filesystem events. So the view model **has no `nextRunMs`, no `lastRun` and no run history at all**: a claim the projection cannot honour must be impossible to express, not merely unrendered.
- **Cadence is `None` unless the clock really paces it.** `tick()` skips both the sweep and `tick_profile` for `!profile.enabled` (`engine.rs:2092-2097`), so a paused folder advertises no cadence. This invariant is enforced in Rust — `cadence` is `Some` only for standing `paced` — so no TypeScript branch can put a cadence beside "paused".
- **The scan row honours 58.8.** `Story588`'s `Engine::sync_governance_mode(profile_id) -> Option<TaskMode>` returns `Some(Scheduled)` once a scheduled Sync task has taken that folder's paced backstop; the scan row then reads *governed* and advertises no cadence. On a tasks-table read error the method returns `None` and the row prints its poll interval — the poll genuinely still runs then, so that row is true. **An error is not a governance state.**
- **These rows are visibly not tasks.** A *Paced* badge (the `TASKS_UNKNOWN_BADGE` idiom), a section heading and a subtitle that says in words that nothing here has a schedule you can set and nothing here can be run on demand — so nobody hunts for a control that cannot exist.
- The sentence on each row is composed **in Rust** and rendered verbatim, `HOST_SENTENCE_*`'s rule: these sentences carry architectural truth (the two event triggers, the stood-down backstop, that only the app paces notes) and must not be re-derived in the browser.
- A refused projection read shows the refusal and keeps the last good rows; it must not blank the task list beside it. *A failed read is a fault to report, not a fact to invent.*
- `null` is unread and `[]` is "keeper paces nothing here"; they never render the same words.

**Block If:** the projection cannot be built without registering a clock or a due-gate; or `keeper-core` would have to depend on `keeper-sync` (`bun run check:core-sync-free`).

**Never:** write a `tasks` row for projected work; migrate `journal` or `activity` into the tasks tables; offer Run now, Edit, Forget, a schedule editor or a history disclosure on a projected row; poll; invent a second list idiom; project anything the inventory rules invisible (see Design Notes); edit `src-tauri/crates/keeper-sync/src/engine.rs` or `docs/sync.md` (owned by `Story588`).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Enabled folder | `enabled`, `pollIntervalMs = 15000` | scan row, standing `paced`, cadence *every 15 seconds*, sentence naming the two filesystem triggers | No error expected |
| Floored interval | `pollIntervalMs = 0` (DW-116 row) | cadence *every 2 seconds* — `effective_poll_interval_ms`, never the stored zero | No error expected |
| Paused folder | `enabled = false` | every row for it reads standing `paused`, cadence **absent** | No error expected |
| Governed scan | `sync_governance_mode` → `Some(Scheduled)` | scan row reads standing `governed`, cadence **absent**; sweep and notes rows unchanged | No error expected |
| Governance read failure | `sync_governance_mode` → `None` on a table error | scan row reads `paced` with its poll interval | The poll still runs; not an error state |
| Scratch sweep | any enabled folder | sweep row, cadence *every hour*, from `SWEEP_EVERY_MS` | No error expected |
| Notes vault | `notes = Some`, cadence 2 s / 30 s | notes row: committed after 2 s of quiet, pushed within 30 s, and **only while the app is running** | No error expected |
| No vault | `notes = None` | no notes row for that folder | No error expected |
| Removable folder | `removable = true` | the scan row's sentence adds that nothing is paced while the drive is away | No error expected |
| No folders | profile list empty | the *keeper paces nothing yet* sentence, never the loading line | No error expected |
| Unread | before the first read lands | the loading line, never the empty sentence | No error expected |
| Refused read | `sync_paced_work` rejects | the refusal verbatim in the section; the task rows above are untouched | The refusal is the render |
| Clock ticks | 3 × `TASKS_CLOCK_TICK_MS` elapse | still exactly one `syncPacedWork` call | No error expected |
| Projected row controls | any projected row | **no** Run now, Edit, Forget, Runs disclosure — no buttons at all | No error expected |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/tasks.rs` -- home of `TaskVm`/`task_host`; gains `PacedWorkVm`, `PacedWorkKind`, `PacedWorkStanding`, the `PACED_*` wording and the pure `paced_work` projection. keeper-core because it is the only crate whose ts-rs bindings regenerate on this Linux host, and it is already core-free of both Tauri and keeper-sync.
- `src-tauri/crates/keeper/src/sync_ipc.rs` -- `sync_tasks`'s neighbours; gains `sync_paced_work`, the adapter that turns each `SyncProfile` plus `Engine::sync_governance_mode` into the borrowed facts the projection takes.
- `src-tauri/crates/keeper/src/lib.rs:978-982` -- the `invoke_handler` list; one line.
- `src-tauri/crates/keeper-sync/src/engine.rs` -- **read only, owned by `Story588`**: `SWEEP_EVERY_MS:443` (widened to `pub` by them), `sync_governance_mode` (added by them), `tick():2092-2097` (the `enabled` filter), `scan_is_due:3106`, `scan_due:3288`.
- `src-tauri/crates/keeper-sync/src/profile/mod.rs` -- `effective_poll_interval_ms:1083`, `NotesCadence:234-243`, `MIN_POLL_INTERVAL_MS:150`.
- `src/lib/ipc/client.ts` -- `syncTasks:6338`; gains `syncPacedWork` and the `PacedWorkVm` re-export.
- `src/lib/ipc/gen/**` -- written **only** by the ts-rs export step (`bun run test:rust`), never by hand.
- `dev/mock-shell.ts:1835` -- `sync_tasks`'s fixture; gains a `sync_paced_work` answer.
- `src/components/layout/tasks-pane.tsx` -- the view. New `PacedWorkList` component and section, its own constants; `refresh()` reads both commands in one settled pass.
- `src/components/layout/tasks-pane.test.tsx` -- 63 green at baseline; the new assertions land here.
- `src/components/layout/list-fold.tsx` -- `useFold`/`FoldToggle`, reused rather than re-implemented.
- `src/test/task-host-tick.test.ts` -- the mechanical AD-62 guard; must still pass unchanged.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/tasks.rs` -- add `PacedWorkKind` (`scan` | `scratchSweep` | `notesCadence`), `PacedWorkStanding` (`paced` | `paused` | `governed`), `PacedWorkVm { id, kind, profileId, profile, standing, cadence: Option<String>, sentence }`, the borrowed `PacedFolderFacts<'_>`, and `pub fn paced_work(&[PacedFolderFacts]) -> Vec<PacedWorkVm>` -- the whole projection is pure and testable without a shell, and the VM structurally cannot claim a next run.
- [x] `src-tauri/crates/keeper-core/src/tasks.rs` -- unit-test the matrix rows: floored interval, paused, governed, sweep cadence, vault present/absent, removable, and a serde test asserting the emitted keys contain no `nextDueMs`/`lastRun`.
- [x] `src-tauri/crates/keeper/src/sync_ipc.rs` -- add `sync_paced_work`: `list_profiles()` (propagating, `sync_tasks`'s rule), per profile fill the facts from `effective_poll_interval_ms()`, `keeper_sync::engine::SWEEP_EVERY_MS`, `notes.cadence` and `engine.sync_governance_mode(&id)`, then call the projection.
- [x] `src-tauri/crates/keeper/src/lib.rs` -- register the command.
- [x] `src/lib/ipc/client.ts` -- `syncPacedWork(): Promise<PacedWorkVm[]>` and the generated-type re-export, documenting that the class is read-only and why it carries no next run.
- [x] `dev/mock-shell.ts` -- a `sync_paced_work` fixture covering a paced folder, a paused one and a vault.
- [x] `src/components/layout/tasks-pane.tsx` -- `PacedWorkList` plus its constants (heading, subtitle, badge, kind labels, loading/empty/no-cadence text); read it inside `refresh()` with `Promise.allSettled` so a refused projection cannot blank the tasks.
- [x] `src/components/layout/tasks-pane.test.tsx` -- the new tests, including the absence assertions in both shapes (`queryAllByRole("button")` empty **and** by name).

**Acceptance Criteria:**
- Given a machine with folders keeper syncs, when ⌘8 is opened, then every paced thing on it is listed with its real cadence, under a heading that says these are paced and not scheduled.
- Given any projected row, when it is inspected, then it offers no Run now, no Edit, no Forget, no Runs disclosure and no schedule editor — and the section says so in words before anyone hunts for one.
- Given the pane is left open, when the display clock ticks repeatedly, then the projection is not re-read.
- Given a folder whose paced backstop 58.8 surrendered to a scheduled sync task, when its scan row renders, then it states it is governed and advertises no cadence.
- Given `bun run test:rust`, when it completes, then `src/lib/ipc/gen` holds the regenerated bindings and `bun run bindings:check` is clean.
- Given `src/test/task-host-tick.test.ts`, when it runs, then it still finds exactly one `tokio::time::interval` in `keeper/src`.

## Spec Change Log

- 2026-08-31 -- the projection gained a fourth standing, `unregistered`, and
  `PacedNotesFacts` gained `registered`. The spec's matrix said a folder with
  `notes = Some` yields a paced notes row; review showed that a *configured*
  vault and a *paced* vault are two different facts, so the matrix row now needs
  the registry to agree. See the triage log below.

## Review Triage Log

**Pass 1 — 2026-08-31, salvage.** The story's own review never ran: the session
that implemented it ended before it, leaving `review_loop_iteration: 0` and this
section empty while the code was complete and green. Two read-only lenses were
run against commit `99769f4` (a blind lens and an edge-case lens); the blind
lens' provider was out of credits and its half was done by hand. Nine findings,
all triaged, none deferred.

**Fixed — two claims that were false on somebody's machine.** These are the
class that matters: a sentence the view states as fact.

1. *A paused folder's notes vault was still committed and pushed.* The notes row
   printed *"this folder is paused, so nothing here is paced"* while the vault
   cadence had no `enabled` gate anywhere: `Engine::tick` skips a disabled
   profile before the scan and the sweep, but the cadence's push arm calls
   `sync_once` directly, and so does the quit flush. Fixed in the pacer rather
   than in the wording, because pausing a folder is exactly a request not to
   touch it — and the gate sits in the two automatic callers, not in `sync_once`,
   which is also the Sync now button a person may press on a paused folder.
   Declining leaves the phase untouched, so resuming serves the work that was
   owed; `Cadence::stand_down` exists because `finish(false, …)` would have
   discarded an owed push. The behaviour is documented in `docs/sync.md` §14.
2. *A vault the registry does not hold advertised a cadence.* `register_one`
   returns nothing when the vault root cannot be canonicalized — a drive that is
   away, a folder that moved — and the row was built from the profile's vault
   *configuration*, so it recited "committed after 2 seconds of quiet, pushed
   within 30 seconds" while nothing at all was pacing it. The projection now
   takes `registered` and answers with the fourth standing. Its sentence names
   the remedy honestly: the registry is rebuilt at launch and on a vault flag,
   **not** when a drive returns, so "it resumes on its own" would have been the
   comfortable lie.

**Fixed — four surfaces that under- or over-claimed.**

3. *The subtitle said nothing here can be run on demand.* False for two of the
   three kinds: the scan's work is what the Sync pane's **Sync now** runs, and a
   vault is flushed whenever the window hides. A sentence written to stop
   somebody hunting for a control was hiding the control they wanted; it now says
   *none of it can be started from this section* and names where the scan can be.
4. *The empty sentence claimed the machine has no folders.* `list_profiles`
   skips a profile row this build cannot deserialize, so on a downgrade an empty
   projection and a machine full of folders are indistinguishable from here. The
   sentence now speaks only about what keeper paces.
5. *The fold dropped rows.* Every other folding list is capped by the query
   behind it; this projection has no query, so the expanded view silently lost
   every row past the global unfolded size — in the one section whose claim is
   completeness. `useFold` grew `unfoldToAll`, and the control's label now
   promises the hook's own limit rather than the setting's.
6. *The sweep row over-claimed on removable media.* `sweep_scratch_if_due` does
   not check `volume_ready`, so with the drive away it deletes nothing while the
   row said it deletes scratch on this cadence. The removable clause now rides
   the sweep too — and still not the notes row, whose unregistered sentence
   already names the missing folder.

**Fixed — three that were not user-visible, and would have become so.**

7. *The invariant was stated, not enforced.* `standing` and `cadence` are
   independent public fields, so a fourth kind could have put *"paused, about
   every 15 seconds"* on screen — the exact phrase this class exists to make
   impossible. `paced_work` now `debug_assert!`s the pairing over every row it
   returns, and the sweep test asserts that every standing really occurred in it.
8. *A test comment claimed coverage a mock cannot provide.* The governed-row test
   was labelled the Story 58.8 contract test; it feeds a hardcoded governed row
   through the IPC mock, so reverting 58.8's stand-down leaves it green. The
   comment now says what the test asserts and points at the guards that would
   actually fail.
9. *`duration_words` could panic.* `ms + 500` overflows within 500 of
   `u64::MAX` — a debug panic, a release wrap to an absurdly small cadence — on a
   number a config file can hold. Now `saturating_add`.

**Judged and not changed.** The reviewer filed a React-key collision risk and a
`duration_words` boundary sweep and then withdrew both with the reasoning shown:
the three id prefixes are distinct literals so `scan:` + A can never equal
`sweep:` + B, and every reachable interval is floored by
`MIN_POLL_INTERVAL_MS` or by `NotesConfig::validate`. Both readings were checked
against the same code and agreed with.

**Mutation proof.** Each new guard was inverted and the failure observed:
removing the registration gate fails the unregistered-vault test *and* the
invariant sweep; capping the fold at the unfolded size fails the completeness
test; making `stand_down` call `finish(false, …)` fails the owed-work test on
macOS with `left: Idle, right: Ahead` — the exact state loss it exists to
prevent. All three restored and re-verified green.

## Design Notes

**What stays invisible, and why.** The inventory found 22 periodic or quasi-periodic items behind two clocks. Only three have both an identity and a cadence, and those three are what this story projects. The rest are left out on purpose, because a row nobody can act on and whose numbers mean nothing is worse than its absence:

- **Queue reads and event drains** — `drain_finished_assertions`, `retain_watchers`, the journal drain, per-file settle windows. No cadence at all; they run every tick because they are cheap reads.
- **The first-checkout retry** — paced by the journal's own backoff, and `tick_profile` says in place why `scan_is_due` is the wrong authority for it.
- **The hourly release *look* gate** (`release_is_due`) — its own doc calls it *"a look interval and not a schedule"*, and the sweep it gates is **already a task row** governed by `release_governance`. Projecting the gate would put a second, fake schedule beside a real one.
- **Watcher re-arm, LFS credential TTL, fetch throttles, notify's internal rescan** — failure recovery, lazy cache expiry and library-internal timers. Surfacing them would imply keeper schedules them.
- **Startup and shutdown passes** (`recover_running`, orphaned-recording recovery, lease release) and the **session-scoped recording disk guard** — one-shot or scoped to a live session, so a row would claim a cadence that does not exist.
- **`lfs_prune_local`** — success-edge work with no clock of its own.

**Why the projection is not in `keeper-sync`.** It needs a ts-rs-exported view model, and `keeper-sync` has no ts-rs; `check:core-sync-free` keeps `keeper-core` off `keeper-sync`, so the two crates meet only in the shell. The projection is pure over borrowed primitives — `task_host`'s exact shape — so it belongs beside it, where the bindings regenerate on this host.

**Why the sentence is composed in Rust.** Each one carries a fact a browser cannot re-derive safely: that a saved file brings the scan forward, that a governed folder's backstop has stood down, that only the running app paces a notes vault. Composed once, rendered verbatim, and the *cadence is `Some` only when standing is `paced`* invariant is enforced there too — so "paused, every 15 seconds" is unrepresentable rather than merely untested.

**The 58.8 dependency, stated.** The scan row's truth depends on a sibling story's behaviour. The governed-row test is therefore a **contract test**: it fails if someone later reverts 58.8's stand-down and leaves the paced poll running under a schedule, which is exactly when this view would start lying.

## Verification

**Commands:**
- `bun run vitest run src/components/layout/tasks-pane.test.tsx` -- expected: 63 baseline + the new tests, all green.
- `bun run vitest run src/test/task-host-tick.test.ts` -- expected: green, one interval found.
- `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core tasks::` (with the git identity prefix) -- expected: the new projection tests pass.
- `bun run test:rust` then `bun run bindings:check` -- expected: `src/lib/ipc/gen` regenerated and committed clean.
- `bun run lint`, `bun run typecheck`, `bun run test`, `cargo fmt`, `cargo clippy --all-targets -- -D warnings` -- expected: at or above the measured baseline.
- Mutation proof: make a projected row render a Run now control (and separately, let a paused row keep its cadence), confirm a test fails each time, restore, and verify the restore by reading `git diff`.
