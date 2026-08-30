---
title: 'The app runs them too, and ⌘8 says which host will'
type: 'feature'
created: '2026-08-29'
status: 'in-progress'
baseline_revision: 'fb4d4a7'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/planning-artifacts/epic-57-a-task-that-runs-when-it-should.md'
  - '{project-root}/_bmad-output/planning-artifacts/architecture/architecture-keeper-2026-07-03/ARCHITECTURE-SCHEDULED-TASKS.md'
warnings: ['multiple-goals', 'oversized']
---

<intent-contract>

## Intent

**Problem:** Waves 1–2 gave keeper a task record, a dialect, a lease, a due-gate on the engine's
own supervisor tick and a CLI. The desktop app links that engine and already starts that
supervisor at boot (`lib.rs:600-604`), so it is *already* a host — but nothing in the app or the
frontend can see, name, drive or even mention a task, the app's quit path hands its lease back
only by racing process exit, a task that fails every hour would notify nobody, and the owner's
complaint is literally *"nie widzę w menu croon like job schedules"* (FR-351, FR-352, AD-137).

**Approach:** Make the desktop host honest and visible: hand the lease back on quit rather than
hope, notify a task failure once per onset, expose the five engine-door verbs as IPC commands over
Linux-regenerable `keeper-core` wire types, and add a Tasks view at **⌘8** whose every row states
the host that will actually run it — including the honest negatives AD-137 names.

## Boundaries & Constraints

**Always:**
- **No second clock.** The app hosts due tasks on `Engine::run`'s existing supervisor tick, started
  once at boot. Nothing is added to the shell's 1 Hz tray tick and no interval, timer or thread is
  created anywhere — AD-62, and a test counts the shell's intervals rather than reading a log.
- **Desktop-gated the way `sessions`/`notes` are.** `mod sync_ipc` is already `#[cfg(desktop)]`, so
  the commands register only in the desktop splice; the view rides the existing `sync` capability
  and mints no twelfth `CapabilitiesVm` flag. iOS has no task surface at all.
- **Every host claim on screen is true.** The host is computed by one pure function over facts the
  app can actually establish, never by a platform sniff in TypeScript. On macOS there is no daemon,
  so the app is the only host and a task runs only while keeper is running. On Linux the daemon
  runs it only when its unit is enabled **and** it reads the same `sync.db` — which by default it
  does not (`~/.local/share/keeper-sync` vs `~/.local/share/dev.tgorka.keeper`).
- **Failure notifies once per onset**, per `Engine::warn`'s rule and its 3 600-an-hour reason.
- **Unknown-kind rows are shown as unknown** (NFR-43), the tolerance the CLI already has.
- **The wire mirrors the CLI's `taskDoc`/`runDoc`**: camelCase, `null` where null is a real value.
- **Wire types live in `keeper-core`** so their ts-rs bindings regenerate on Linux; the shell only
  maps `keeper_sync` rows onto them.
- Relative times render client-side from instants (`formatSyncWaited`'s precedent).

**Block If:** nothing. Every decision is derivable from AD-137, waves 1–2 and this tree.

**Never:**
- No new interval, thread, timer or `tokio::time::interval` in the `keeper` crate.
- No `TaskKind::Update`, no new task kind, no change to `decide`, the dialect or the lease.
- No systemd unit and no `docs/sync.md` §13 (57.7 owns both).
- No hand-written file under `src/lib/ipc/gen/`.
- No second `invoke_handler` call site.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| a due task on the app's tick | supervisor running, window open | the run is recorded; **zero** new intervals in the shell | none |
| the window is closed | ⌘W / red button | `prevent_close` + `hide`; the supervisor is never signalled | none |
| the app quits | ⌘Q → `ExitRequested` | this host's task leases are handed back before exit | logged, never fatal |
| a task fails twice running | `Failed`, then `Failed` | exactly one notification | — |
| it recovers, then fails again | `Failed`, `Ok`, `Failed` | two notifications | — |
| busy / deferred between failures | `Failed`, `Deferred`, `Failed` | still one notification (no recovery happened) | — |
| macOS, scheduled task | no daemon anywhere | host `app`: "keeper runs this — only while keeper is running" | — |
| Linux, unit enabled, same data dir | shared `sync.db` | host `daemon`: "the daemon runs this, logged in or not" | — |
| Linux, unit enabled, other data dir | the default | host `app`, not `daemon` | — |
| a task naming a folder that is gone | `profileId` set, `profile` null | **unhosted**, with the reason | — |
| scheduled but no schedule stored | `mode=scheduled`, `schedule=null` | **unhosted**, with the reason | — |
| a task that is off or disabled | `mode=off` or `enabled=0` | host `off` — never *unhosted*, and never enabled-and-quiet | — |
| a mode `manual` task | enabled | host `onRequest`: nothing schedules it | — |
| an unreadable row | `kind='teleport'` | listed under `unknown` with the reason; the view renders it | never fatal |
| Run now on a busy task | live lease elsewhere | the refusal is shown on the row | `IpcError`, row keeps state |
| Run now on an off task | `mode=off` | refusal quoted on the row | `IpcError` |
| a run a newer keeper recorded | unreadable outcome spelling | rendered as its stored spelling, not "unknown" | — |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/tasks.rs` — **NEW.** The wire types (`TaskVm`, `TaskRunVm`,
  `UnknownTaskVm`, `TaskListingVm`, `TaskSaveReq`, `TaskHostVm`, `TaskHostKind`, `DaemonPresence`)
  and the two pure functions AD-137 turns on: `daemon_presence` and `task_host`. In `keeper-core`
  so `cargo test -p keeper-core` regenerates every binding **on Linux**.
- `src-tauri/crates/keeper-core/src/lib.rs` — `pub mod tasks;`.
- `src-tauri/crates/keeper-core/src/palette.rs` — `TASKS_CATEGORY`, the `tasks-view` action with
  its `⌘8` chip, the category in `CATEGORY_ORDER` and in `registry_sections`' gate.
- `src-tauri/crates/keeper-sync/src/engine.rs` — `task_faults` sticky state; `note_task_outcome`
  (the once-per-onset edge) called from `claim_and_run`; `release_task_leases` made `pub`.
- `src-tauri/crates/keeper/src/sync_ipc.rs` — `sync_tasks`, `sync_task_history`,
  `sync_task_run_now`, `sync_task_save`, `sync_task_forget`; `task_vm`/`task_run_vm` mapping; the
  Linux `systemctl --user is-enabled` + daemon-data-dir probe feeding `daemon_presence`.
- `src-tauri/crates/keeper/src/lib.rs` — the five commands in the desktop splice;
  `sync::finalize_for_quit()` on `ExitRequested`; the no-second-clock source test.
- `src-tauri/crates/keeper/src/sync.rs` — `finalize_for_quit()`.
- `src/lib/ipc/client.ts` — five wrappers and the type re-exports.
- `src/lib/stores/primary-view.ts` — `"tasks"`.
- `src/hooks/use-tasks-shortcut.ts` — **NEW.** ⌘8, with the typing and IME guards.
- `src/components/layout/tasks-pane.tsx` — **NEW.** The rows, the formatters, the copy.
- `src/components/layout/app-shell.tsx`, `src/components/layout/sidebar-pane.tsx`,
  `src/components/command-palette/actions.ts` — the arm, the entry before Settings, the dispatch.
- `dev/mock-shell.ts` — the five fixtures covering every state the view can render.

## Tasks & Acceptance

**Execution:**
- [x] `keeper-core/src/tasks.rs` — the wire types and the two pure functions; every 64-bit field
      annotated `#[ts(type = "number")]` / `"number | null"` — one place holds AD-137's decision,
      and it is the one place that compiles here.
- [x] `keeper-core/src/tasks.rs` — unit-test the host matrix above exhaustively, the unhosted
      reasons, and `daemon_presence` over both facts.
- [x] `keeper-core/src/palette.rs` — the gated `Tasks` category and the `tasks-view` ⌘8 action, plus
      a test that it is present iff the gate is on — the palette IS the menu bar (`menu.rs:114`).
- [x] `keeper-sync/src/engine.rs` — `task_faults`, `note_task_outcome`, `pub release_task_leases`.
- [x] `keeper-sync/src/engine.rs` — test the onset edge over a recording platform: many failures,
      one notification; recovery re-arms; `Busy`/`Deferred` neither notify nor clear. Test that
      `release_task_leases` frees a held lease so another host claims it.
- [ ] `keeper/src/sync_ipc.rs` — the five commands and the row→VM mapping.
- [ ] `keeper/src/sync.rs` + `keeper/src/lib.rs` — `finalize_for_quit` on the quit path only, the
      splice, and a source test asserting one interval in the shell and that `CloseRequested`
      reaches neither the stop nor the release.
- [ ] `src/lib/ipc/client.ts` — five wrappers with literal command names.
- [ ] `src/hooks/use-tasks-shortcut.ts` (+ test) — ⌘8/Ctrl+8, IME, typing targets, capability gate.
- [ ] `src/components/layout/tasks-pane.tsx` (+ test) — the rows, Run now, the unhosted case, the
      unknown row, the macOS sentence; exported copy constants and testids.
- [ ] `src/lib/stores/primary-view.ts`, `app-shell.tsx`, `sidebar-pane.tsx`, `actions.ts` — wiring.
- [ ] `dev/mock-shell.ts` — scheduled-with-next-due, mid-run-holding-a-lease, failed-last-run,
      unknown-kind and unhosted fixtures, so the whole view is exercisable in a browser on Linux.

**Acceptance Criteria:**
- Given the desktop build, when the shell's sources are scanned, then exactly one
  `tokio::time::interval` exists in `keeper/src` and it is the pre-existing tray tick.
- Given ⌘8 on a machine with the sync capability, when it is pressed outside a text field, then the
  Tasks view opens and the event is `defaultPrevented`; with the capability off, nothing happens.
- Given the palette registry with the tasks gate on, when the menu bar is built from
  `registry_sections`, then a `Tasks` submenu carrying `tasks-view` and its `⌘8` chip is in it.
- Given a listing containing one row of each state, when the pane renders, then every row states
  its kind, schedule, host sentence, next due, last run and last outcome, and offers Run now.
- Given a Run now that the engine refuses, when the command rejects, then the row shows the refusal
  and no row claims the task ran.

## Spec Change Log

## Review Triage Log

## Design Notes

**The app was already a host; this story makes it an honest one.** `Engine::tick` calls
`run_due_tasks` (engine.rs:1904) and `lib.rs:600-604` starts that supervisor under `#[cfg(desktop)]`.
Adding a poll to the shell's 1 Hz tray tick would be the *second* scheduler over one git repository
that AD-62 forbids by name. So "the app runs due tasks on the tick it already owns" is satisfied by
the tick it already owns — asserted, not re-implemented.

**The quit path was a race, and the fix is not only a join.** `stop_supervisor` signals and returns;
the supervisor's `JoinHandle` was dropped at spawn (sync.rs:445-451), and `Engine::run`'s post-loop
`finalize()` → `release_task_leases` therefore raced process exit and usually lost. Worse,
`self.tick().await` runs inside the select *branch*, so a supervisor mid-tick cannot even observe
the signal. `finalize_for_quit()` signals **and** releases this host's leases directly through the
`Arc` the quit thread already holds: one bounded `UPDATE`, idempotent, exactly what `finalize` does.

**A task's onset is not a profile's.** `Engine::warn` keys on `SyncStatus.warning`, which is
per-profile — and a task may be host-wide, so it has no profile to be sticky on. `task_faults:
Mutex<HashSet<String>>` keyed by task id is the same rule in the same shape: insert-and-notify on
the `absent → present` edge, remove on `Ok`, and leave `Busy`/`Deferred` alone because a run that
did not happen is neither a failure nor a recovery.

```rust
// The pure host decision. No clock, no database, no `cfg!`.
pub fn task_host(t: TaskHostFacts<'_>, daemon: DaemonPresence) -> TaskHostVm {
    if !t.enabled || t.mode == "off"        { return off(); }
    if t.profile_id.is_some() && t.profile.is_none() { return unhosted(FOLDER_GONE); }
    if t.mode == "manual"                    { return on_request(); }
    if t.schedule.is_none()                  { return unhosted(NO_SCHEDULE); }
    match daemon { DaemonPresence::Runs => daemon_host(), _ => app_host() }
}
```

**Why `unknownOutcome` is `string | null` here while the CLI makes it absent.** The CLI document
needs absence because `outcome: null` alone cannot separate "in flight" from "a newer keeper wrote
a spelling we cannot read". A view model has both keys always, so `unknownOutcome: null` says the
first and a string says the second — unambiguous without a conditional key, and `#[ts(optional)]`
on a ts-rs field would make the frontend handle `undefined` for no gain.

## Verification

**Commands:**
- `GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=keeper GIT_AUTHOR_EMAIL=dev@keeper.local GIT_COMMITTER_NAME=keeper GIT_COMMITTER_EMAIL=dev@keeper.local cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync -p keeper-core -p keeper-syncd` — expected: 0 failed, at or above the 3704 baseline.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings` — expected: clean.
- `cargo fmt --manifest-path src-tauri/Cargo.toml -p keeper-sync -p keeper-syncd -p keeper-core` then `--check` — expected: no diff. `cargo fmt --check` also *parses* the shell crate, which is the only local syntax gate it has.
- `bun run lint && bun run typecheck && bun run test` — expected: lint at baseline (4 warnings + 1 info), typecheck clean, 297+ files green including `src/test/command-registration.test.ts`.

**Manual checks (if no CLI):**
- The `keeper` shell crate cannot be compiled on this host (`gobject-sys`). Every symbol it gains is
  read back against its call site and reported for `bun run check:rust:macos`.
- Each guard is mutated away in turn and the owning test must fail; the restore is verified by
  reading `git diff`, never from memory.
