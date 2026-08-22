---
name: 'keeper'
type: architecture-spine-companion
purpose: build-substrate
altitude: initiative
paradigm: 'hexagonal Rust core + unidirectional view-model projection — unchanged; a task is a named, recorded, idempotent invocation of work the engine already knows how to do'
scope: 'keeper scheduled tasks — named housekeeping work with a schedule, a last run and a last result, runnable on the daemon and on the desktop app, drivable by an external scheduler, and visible in a Tasks view'
status: final
created: '2026-08-22'
binds: [FR-346..FR-352, NFR-42, NFR-43]
sources:
  - _bmad-output/planning-artifacts/research/virtual-files-2026-08-22/
  - docs/sync.md §12
parent: ARCHITECTURE-SPINE.md
---

# Architecture Companion — Scheduled tasks

Extends the frozen spine with **AD-135..AD-137**. Nothing here renegotiates it: `keeper-sync`
still reaches the OS only through `SyncPlatform` (AD-40/AD-52), the engine is still the only thing
that decides *when* to use a capability, and **AD-62's rule holds absolutely** — one clock per host
process, because *"two schedulers over one git repository is how you get concurrent index locks"*
(`keeper/src/notes_vault.rs:2578-2582`).

**The one-sentence shape:** a *task* is a named row with a schedule, a last run and a last result;
the schedule is evaluated as a due-gate on the tick each host already runs, and the same task is
reachable as a one-shot CLI verb so a real `cron` or a systemd timer can drive it instead.

**What is genuinely new is a record.** Periodicity is not new — the engine already paces scanning
(`scan_is_due`, `engine.rs:1304`) and sweeping (`sweep_is_due`, `engine.rs:1390`,
`SWEEP_EVERY_MS`, `engine.rs:352`) as due-gates on its 1 Hz tick (`TICK_MS`, `engine.rs:338`), and
the desktop app runs its own 1 Hz tick that already hosts the notes cadence
(`keeper/src/lib.rs:509-541`). A one-shot entry point is not new either: `keeper-syncd sync --once`
is documented as *"the cron entry point"* (`keeper-syncd/src/commands.rs:232`). What does not exist
is anything that **remembers**: no task table, no name, no schedule, no last-run time, no result.

---

## Architecture decisions AD-135 … AD-137

### AD-135 — A task is a record, and the journal cannot be that record

- **Binds:** FR-346, NFR-43; Epic 57
- **Decision.** Two new tables in the existing `sync.db`, beside `activity`:
  - **`tasks`** — `(id TEXT, profile_id TEXT NULL, kind TEXT, schedule TEXT NULL, mode TEXT,
    next_due_ms INTEGER NULL, enabled INTEGER, updated_ms INTEGER)`, primary key `(id)`. A task
    with `profile_id IS NULL` is host-wide; a per-folder task names its profile.
  - **`task_runs`** — `(id INTEGER PRIMARY KEY AUTOINCREMENT, task_id TEXT, started_ms INTEGER,
    finished_ms INTEGER NULL, outcome TEXT NULL, detail TEXT NULL, host TEXT)`, bounded and
    append-only, with an index on `(task_id, id DESC)`.
- **Why not the journal.** `journal` (`db.rs:69-83`) is a per-profile work **queue**: it has no
  name, no schedule and no result, and `db::complete` is `DELETE FROM journal WHERE id = ?1` —
  *"the only place work leaves the journal"* (`db.rs:826-832`). A completed unit leaves no trace,
  so a Tasks view built on it could only ever show work that has not happened yet. `WorkKind`
  (`db.rs:609-623`) is also a closed six-variant vocabulary of transfer primitives; a task is a
  level above it and frequently enqueues several.
- **Why not `activity`.** Its own doc settles it: *"append-only and bounded: it is a human-facing
  log, not a source of truth"* (`db.rs:99-101`). `task_runs` is the same shape and the same
  discipline, but it **is** the source of truth for "when did this last run, and what happened" —
  which is exactly why it is a second table rather than three more `activity` columns.
- **Forward compatibility is not optional.** A row whose `kind` this build does not understand is
  **skipped, never fatal** — the rule `activity` already states: *"a row whose kind is not one this
  build understands is skipped rather than fatal … a newer keeper's activity must not brick an
  older one's list"* (`db.rs:1445-1447`). This is what lets one host write a task the other host's
  older binary has never heard of.
- **Migration.** Both tables are `CREATE TABLE IF NOT EXISTS` in `db::migrate` (`db.rs:59-158`);
  later columns use the additive `ensure_*_columns` idiom already there (`db.rs:156-158`). No
  content migration and therefore no `meta` marker (`db.rs:127-132,149-152`).

### AD-136 — The schedule is a due-gate on the host's existing tick, validated at save time, leased at run time

- **Binds:** FR-347, FR-348, FR-349, NFR-42; Epic 57
- **Decision, in three parts.**
  1. **Evaluation.** Each host evaluates due tasks on the tick it already runs — the engine's
     supervisor tick for `keeper-syncd`, the app's 1 Hz `tokio::time::interval` for the desktop
     (`keeper/src/lib.rs:509-541`). A task is due when `next_due_ms <= now`, computed from the
     injected platform clock, exactly as `scan_is_due`/`sweep_is_due` already are
     (`engine.rs:1304,1390`). **There is no task thread, no second interval and no `.timer` inside
     the process.** The decision function is pure — `decide(state, schedule, now_ms) -> Action` —
     following the notes cadence's own split (`notes_vault.rs:2551-2566`), so the state machine is
     tested against a fixed clock and never against a real one.
  2. **Validation at save time, refusal not coercion.** A schedule keeper cannot parse is refused
     with the expression quoted, when it is written — never accepted and silently never run. The
     precedent is verbatim in this tree: `validate_quiet_time` (`profile/mod.rs:708-712`) refuses a
     malformed window because *"a window nobody can parse is a window that would silently never
     open"*. Accepted dialect: a 5-field cron expression **or** the plain forms `@hourly`,
     `@daily`, `@weekly`, and `every <n><unit>`; the parser is a keeper-owned function over a small
     grammar, not a new dependency, and the floor is one minute for the same reason
     `MIN_POLL_INTERVAL_MS` exists (`keeper-syncd/src/config.rs:36-40`: below that *"the scheduler
     spends more time waking than working"*).
  3. **Exactly one runner, by lease.** A task row carries a `running_host` + `lease_until_ms`
     claimed in the same `UPDATE` that starts it — the pattern `db::claim_ready` already uses to
     make two supervisors unable to take one row (`db.rs:773-783,816-819`). This matters here more
     than anywhere: on a Linux box the daemon **and** the app can both be running against the same
     `sync.db` (`ipc.rs:4926-4930` already warns that *"the running engine and keeper-syncd also
     write"* it). A lease that expires is reclaimable, so a killed host does not wedge a task
     forever.
- **The external scheduler is a first-class driver, not a workaround.** `keeper-syncd tasks run
  <id>` runs exactly one task, prints its outcome and exits with the existing exit-code taxonomy
  (`sync_exit_code`, `keeper-syncd/src/commands.rs:124`; `RestartPreventExitStatus=2 3` in the unit
  file). This is what makes the owner's *"nie musi byc automatyczne, moze to byc skrypt"* true
  rather than approximated: cron, a systemd timer, or a human at a prompt all reach the same code
  path as the in-process due-gate, and the run is recorded identically in `task_runs`.
- **What the anti-timer stance actually forbids.** `docs/sync.md:889-893` refuses unattended
  replacement of the daemon **binary** on a timer, and restates it at `:1066-1067` about `update`.
  It does not forbid scheduled work — the same document expects a cron wrapper around
  `verify --remote` (`:325-326`). Nothing in AD-135..AD-137 lets a schedule install, replace or
  restart a binary; `update` remains manual, and a task **may not** be of that kind.
- **Rejected — a systemd timer as the only mechanism.** It would work on Linux and leave macOS
  with nothing (AD-137), and it would put the schedule somewhere keeper cannot show, validate or
  report on. The unit file that ships is a *thin caller* of `tasks run`, not the source of truth.

### AD-137 — Which host runs a task is a platform fact, and the UI must not pretend otherwise

- **Binds:** FR-350, FR-351, FR-352; Epic 57
- **The facts, because they are asymmetric and the asymmetry is load-bearing.**
  - **Linux** has a packaged background host: the systemd **user** unit whose `ExecStart` is
    `keeper-syncd watch` (`keeper-syncd/packaging/keeper-syncd.service`), with `loginctl
    enable-linger` for post-logout. Tasks run whether or not anybody is logged into a desktop.
  - **macOS has no daemon at all.** There is no launchd plist for `keeper-syncd` anywhere in the
    repository, and `keeper-syncd/src/platform.rs:11-19` states the daemon is *"Linux-first,
    unix-only"* and *"deliberately does not pretend to build"* elsewhere. On macOS the **only**
    background host is the desktop app — which is a real host: closing the window calls
    `api.prevent_close()` + `window.hide()` and keeps the process, its `SyncService` and its
    notification pipeline alive (`keeper/src/lib.rs:1106-1112`), the app embeds the engine
    in-process (`keeper/src/sync.rs:406-426`), and Launch-at-login exists
    (`tauri_plugin_autostart`, `keeper/src/lib.rs:210-211`, `ipc.rs:10187-10222`). But **quit means
    quit**: `RunEvent::ExitRequested` (`lib.rs:1173-1190`) ends the host, and nothing runs until the
    app is next launched.
  - **iOS is not a host.** The OS owns the runtime and `pause_all()` runs on backgrounding
    (`keeper/src/lifecycle.rs:12-14,106-138`), so tasks are desktop-gated, the way `sessions` and
    `notes` already are in `CapabilitiesVm` (`keeper-core/src/vm.rs:129-137`).
- **Decision.** A task declares which hosts may run it, and the Tasks view states, per task, **the
  host that will actually run it and when it last did** — including the honest negative: *"only
  while keeper is running"* on macOS without a daemon, and *"the daemon runs this"* on a Linux box
  where the unit is enabled. A task that no present host can run is shown as **unhosted**, not as
  enabled-and-quiet.
- **Why this is an architecture decision and not UI copy.** The alternative is the failure this
  tree has already paid for twice: a feature that looks enabled and does nothing
  (`sprint-status.yaml`'s recurring `incorrect` lesson; DW-140/DW-206). A schedule is exactly the
  kind of thing whose non-execution is invisible — nobody notices the absence of housekeeping —
  so the surface must assert the host, and a story must test the unhosted case.
- **Failure is announced once per onset, never per attempt.** The engine's `warn` already
  implements the rule and the reason: notify on `None → Some`, never on a wording change, because
  a text-keyed rule produced *3 600 notifications an hour* (`engine.rs:1094-1156`). A task that
  fails every hour is one notification and a sticky state, not twenty-four.
- **The record is shared; the schedule is per host by default.** Both hosts read and write the same
  `tasks`/`task_runs` tables in `sync.db` when they share a data dir — but they do **not** share
  one by default (`ipc.rs:651-656` vs `keeper-syncd/src/platform.rs:77-81`). A schedule the owner
  wants both surfaces to honour therefore lives where both read it: the folder TOML tier
  (AD-132, `keeper-core/src/config/mod.rs:13-20`). The `tasks` table is the *runtime* record for
  the host that owns that data dir; the TOML tier is the *intent*. When they disagree, the TOML
  layer wins on every read, per AD-98.

---

## Deferred

- **Arbitrary user commands as tasks** — a task kind is one of keeper's own verbs, never a shell
  string. Running user-supplied commands from a sync daemon is a different security posture
  (egress, credentials, `NoNewPrivileges=yes` in the unit) and needs its own decision. Revisit only
  with a stated threat model.
- **A launchd agent for `keeper-syncd` on macOS** — the honest fix for AD-137's asymmetry, and a
  separate deliverable: it needs a plist, an installer path, a `loginctl`-equivalent story for
  logout, and the daemon crate to build on macOS at all (`platform.rs:11-19`). Until then the app
  is the macOS host and AD-137 makes that visible.
- **Distributed task ownership across machines** — two clones of one folder both running a nightly
  release sweep is safe (each releases only what its own ledger proves), but *coordinating* them is
  not needed and not designed. Revisit if a task appears whose effect is not per-clone.
- **Catch-up semantics for missed windows** — a task whose host was off for a week runs once when
  the host returns, not seven times. A `catch_up` policy is a knob nobody has asked for.

## Feasibility

FR-346–FR-352 are implementable within AD-135..AD-137 plus the frozen spine, **with no new
crates**: the tables are `CREATE TABLE IF NOT EXISTS` in the existing migrator (`db.rs:59-158`),
the due-gate is the shape `scan_is_due`/`sweep_is_due` already have (`engine.rs:1304,1390`), the
lease is `claim_ready`'s single-statement claim (`db.rs:773-783`), the CLI verb is one arm in an
existing `clap` enum (`keeper-syncd/src/commands.rs:212`, dispatch `:507-550`) beside a `--once`
mode that already exists, the IPC commands are two entries in the desktop splice
(`keeper/src/lib.rs:908-1003`) pinned by `command-registration.test.ts`, and the Tasks view is one
`PrimaryView` member (`src/lib/stores/primary-view.ts:38-49`), one sidebar const
(`sidebar-pane.tsx:244-257`), one `app-shell.tsx` arm (`:299-331`), one palette row with **⌘8** —
the first free number (`keeper-core/src/palette.rs`, sessions holds ⌘7 at `:646-655`) — and a list
modelled on the Sync pane's pending/parked lists (`sync-pane.tsx:1316-1447`).

The riskiest seams, in order: **the lease** (AD-136 — two hosts on one `sync.db` is the normal case
on Linux, and a task that runs twice concurrently over one git repository is the concurrent-index-
lock failure AD-62 exists to prevent), **the schedule parser's refusal path** (AD-136 — a schedule
that parses to "never" and reports itself enabled is the invisible-failure shape), and **the
unhosted case** (AD-137 — the macOS user with the app quit must be told, not silently unserved).
Each is testable against a fixed clock and a real SQLite file; none of them needs a real timer, and
a story that proves a schedule by sleeping is asserting the wrong thing.
