# Epic 57 — A task that runs when it should, and says so when it did

created: '2026-08-22'
source: the owner's second virtual-files pass — *"chce miec tez opcje w keeperze do uruchamiania croon taskow na sync i desktop - zeby byl opcje i widok w ui do taskow"*. Split out of Epic 56 because it has its own records, its own hosts and its own surface. Grounded by three read-only repository scouts (the daemon's execution model, the desktop shell's lifecycle, the requirement registries), 2026-08-22.
binds: FR-346…FR-352 (allocated here), NFR-42, NFR-43; AD-135…AD-137 (new, `architecture/architecture-keeper-2026-07-03/ARCHITECTURE-SCHEDULED-TASKS.md`); AD-62 (untouched, and the constraint this epic is shaped by); AD-98 (untouched); AD-131, AD-133 (Epic 56, consumed here)
see-also: Epic 56 (`epic-56-the-file-is-there-even-when-it-is-not.md`) — the release sweep this epic gives a schedule to

## What he said

> *Usuwanie nie musi byc automatyczne, moze to byc skrypt i puszczany w odpowiednim czasie (cron
> job like) - chce miec tez opcje w keeperze do uruchamiania croon taskow na sync i desktop - zeby
> byl opcje i widok w ui do taskow*

Two asks in one sentence, and they pull in opposite directions. *"Nie musi byc automatyczne"* wants
release to be something a person or a script triggers. *"Opcje w keeperze do uruchamiania cron
taskow"* wants keeper itself to hold the schedule. Both are buildable, and the second is the one
that needs an architecture, because keeper currently remembers nothing about work it has done.

## What exists, and what does not

| # | thing | verdict | evidence |
|---|---|---|---|
| 1 | periodic work inside a host | **exists, and its shape is settled** | `scan_is_due` (`engine.rs:1304`) paces scanning by the profile's own `poll_interval_ms`; `sweep_is_due` (`engine.rs:1390`) paces scratch cleanup by `SWEEP_EVERY_MS` (`engine.rs:352`). Both are due-gates on the supervisor tick (`TICK_MS = 1_000`, `engine.rs:338`), keyed on the **injected** platform clock, *"a schedule on the platform clock is a schedule a test can advance"* (`engine.rs:1321-1323`) |
| 2 | a second clock | **forbidden, by name** | AD-62: the notes cadence rides the ~1 Hz tick that already drives the tray, because *"two schedulers over one git repository is how you get concurrent index locks"* (`keeper/src/notes_vault.rs:2578-2582`). The desktop tick is `keeper/src/lib.rs:509-541` and already hosts `cadence_tick()` at `:539` |
| 3 | a one-shot entry point a cron can call | **exists, and is documented as exactly that** | `Command::Sync { once }` — *"Do one pass and exit — the cron entry point"* (`keeper-syncd/src/commands.rs:232`); `verify --remote` *"exits non-zero so a cron wrapper sees it"* (`docs/sync.md:325-326`); `sync_exit_code` gives a wrapper a meaningful taxonomy (`commands.rs:124`), and the unit file refuses to retry exit 2 and 3 |
| 4 | a record of work that ran | **absent** | `journal` (`db.rs:69-83`) is a per-profile queue with no name, no schedule and no result, and `db::complete` is `DELETE FROM journal WHERE id = ?1` — *"the only place work leaves the journal"* (`db.rs:826-832`). `activity` is *"a human-facing log, not a source of truth"* (`db.rs:99-101`). Grep for `last_run`, `next_run`, `cron`, `scheduler` across both crates finds only backoff prose |
| 5 | a `.timer` unit | **absent** | the only packaged unit is the systemd **user** service whose `ExecStart` is `keeper-syncd watch` (`keeper-syncd/packaging/keeper-syncd.service`). No `.timer` and no launchd plist for the daemon exists anywhere |
| 6 | a background host on macOS | **absent as a daemon; present as the app** | `keeper-syncd` is *"Linux-first, unix-only"* and *"deliberately does not pretend to build"* elsewhere (`keeper-syncd/src/platform.rs:11-19`). But the app is a real host: close hides rather than exits (`lib.rs:1106-1112`), the engine runs in-process (`keeper/src/sync.rs:406-426`), and launch-at-login exists (`lib.rs:210-211`) |
| 7 | any tasks vocabulary in the frontend | **absent** | the only `Task` type in the generated bindings is `SessionTaskVm` — a markdown checklist item in a work session (`keeper-core/src/sessions/vm.rs:388`), unrelated. `PrimaryView` (`src/lib/stores/primary-view.ts:38-49`) has no member for it, and **⌘8 is the first free number** (⌘1–⌘7 are allocated in `keeper-core/src/palette.rs`, sessions at `:646-655`) |

## The correction this epic is built on

The first draft of Epic 56 refused a scheduler on the grounds that this tree refuses timer-driven
work, citing `docs/sync.md:889-893`. **That reading was wrong.** The passage refuses exactly one
thing — replacing the running daemon's *binary* unattended, because it *"can be mid-push at any
moment"* — and restates it at `:1066-1067` as *"`update` never runs on a timer, and never restarts
the daemon"*. The daemon's whole mode of operation is timer-driven; the engine is a 1 Hz clock with
due-gates hanging off it.

So the real constraint is not "no timers". It is **AD-62: one clock per host process**, because two
schedulers over one git repository produce concurrent index locks. Everything in this epic is
shaped by that: a task is a due-gate on a tick that already exists, never a thread of its own.

And one thing stays forbidden: `update` may not be a task. A schedule that replaces a binary is
precisely what `docs/sync.md:889-893` refuses, and AD-136 says so in writing.

## What the binds mean

FR-346…FR-352 and NFR-42/NFR-43 are allocated here; FR-345 and NFR-41 were the previous ceilings.

| id | statement | story | AD |
|---|---|---|---|
| FR-346 | keeper holds named tasks, each with a kind, an optional profile, a mode, a schedule, a next-due time, a last run and a last result | 57.1 | AD-135 |
| FR-347 | A schedule keeper cannot parse is refused when it is saved, with the expression quoted; it is never accepted and silently never run | 57.2 | AD-136 |
| FR-348 | A due task runs on the host's existing tick, and a task never runs twice concurrently — including when a daemon and the app share one database | 57.2 | AD-136 |
| FR-349 | `keeper-syncd tasks list` / `status` / `run <id>` report and drive tasks from the command line, with an exit code a cron wrapper can act on | 57.3 | AD-136 |
| FR-350 | Release is a task with three modes — off, manual, scheduled — and automatic release on the success edge is the default rather than the only path | 57.4 | AD-136, AD-131 |
| FR-351 | The app shows every task with its schedule, its host, its next due time, its last run and last result, and offers run-now; a failure notifies once per onset | 57.5, 57.6 | AD-137 |
| FR-352 | A task no present host can run is shown as unhosted, and Linux packaging ships a timer unit that calls the same one-shot verb | 57.6, 57.7 | AD-137 |
| NFR-42 | A task may never hold a git index lock concurrently with its host's sync pass, and every task is idempotent and safely abandonable mid-run — SIGTERM is a bounded finalize, not a corruption | 57.1, 57.2 | AD-136 |
| NFR-43 | The task record is bounded like `activity`, and a task row whose kind this build does not understand is skipped rather than fatal | 57.1 | AD-135 |

## Why the suite cannot see the risk in this epic

The recurring lesson in `sprint-status.yaml` — *a story that asserts its central claim through a
pure function while the risk lives in the impure shell comes back `incorrect`* — has a specific
shape here, and it is **not** the one a story author will reach for first.

The pure part is easy and must stay pure: `decide(state, schedule, now_ms) -> Action` over an
injected clock, exactly as the notes cadence separates its decision from its effect
(`notes_vault.rs:2551-2566`). A test that proves a schedule by **sleeping** is asserting the wrong
thing and will be flaky besides.

The impure part is where this epic can lose: **two processes, one SQLite file**. On a Linux box the
daemon and the app both write `sync.db` — the tree already warns about it (`ipc.rs:4926-4930`) — so
the lease must be proven with two real connections racing one due task, in the manner
`db::claim_ready` is already proven (`db.rs:773-783`, *"marking them `running` in the same statement
so two supervisors can never take the same row"*). A single-process test that calls the runner twice
in sequence proves nothing about the failure that matters.

The second impure risk is **silence**: a task that is enabled, unhosted and quiet is
indistinguishable from a working one. 57.6 must assert the negative — the macOS-without-a-daemon
case reads *unhosted*, not *enabled*.

## Stack order

    57.1  a task is something keeper remembers      (tasks + task_runs tables, kinds, modes, outcomes, the unknown-kind skip, bounded history)
    57.2  a schedule that refuses what it cannot parse (the dialect, save-time validation, the due-gate on each host's existing tick, the lease)
    57.3  the verb a cron can call                  (keeper-syncd tasks list/status/run, exit codes, --json)
    57.4  release becomes a task                    (Epic 56's sweep as the first built-in kind: off / manual / scheduled, with the success edge still the default)
    57.5  the app runs them too                     (the desktop tick hosts due tasks, desktop-gated, failure notified once per onset, quit means quit)
    57.6  a view of what runs, and when it last did (⌘8, the Tasks pane, schedule + host + next due + last result, run-now, the unhosted state)
    57.7  packaging and the chapter                 (the systemd timer + oneshot pair that calls `tasks run`, docs/sync.md §13, and the macOS truth stated once)

57.1 → 57.2 is a strict chain (the gate needs the record). 57.3 and 57.4 both depend on 57.2 and are
disjoint from each other. 57.5 depends on 57.2 and is where the desktop host appears; 57.6 depends
on 57.5 for the host vocabulary it must display honestly. 57.7 last, because a documented timer that
calls a verb which does not exist is worse than no documentation.

**57.4 is the story that closes the owner's ask**, and it can ship before 57.5/57.6 — a release task
driven by `cron` calling `keeper-syncd tasks run release` satisfies *"moze to byc skrypt"* with no UI
at all.

## Acceptance, per story

**57.1** — `tasks` and `task_runs` exist in `sync.db`, created by the existing migrator with
`CREATE TABLE IF NOT EXISTS` and no `meta` marker (no content migration). A run records started,
finished, outcome and detail; history is bounded the way `activity` is. A row whose `kind` this
build does not know is **skipped and listed as unknown**, never fatal — the rule stated at
`db.rs:1445-1447`, asserted with a hand-written row of a fictional kind.

**57.2** — a 5-field cron expression, `@hourly`/`@daily`/`@weekly`, or `every <n><unit>` parses;
anything else is refused **at save time** with the expression quoted, in the manner of
`validate_quiet_time` (`profile/mod.rs:708-712`), and the refusal is a typed `SyncError::Config`. A
schedule under one minute is refused rather than clamped. Due evaluation happens on the host's
existing tick with no new interval anywhere — a test asserts the tick count, not elapsed wall time.
The lease is proven with **two connections** to one database racing the same due task: exactly one
runs, and a lease whose holder died is reclaimable.

**57.3** — `keeper-syncd tasks list` prints every task with its schedule, mode, next due and last
outcome; `tasks status <id>` prints its history; `tasks run <id>` runs exactly one task, records the
run identically to a scheduled one, and exits with a code from the existing taxonomy — success 0, a
real failure non-zero so a cron wrapper sees it (`docs/sync.md:325-326` is the precedent, and
`sync_exit_code` the mechanism). `--json` output is a stable contract, the split Epic 56's listing
already makes.

**57.4** — the Epic 56 release sweep is registered as the built-in `release` task kind, with modes
**off**, **manual** and **scheduled**. `off` means the success-edge sweep does not run either;
`manual` means only `tasks run` releases; `scheduled` adds the schedule. The default is the
success-edge behaviour Epic 56 ships, unchanged, so upgrading changes nothing. Every AD-125 refusal
and the AD-131 clock apply identically regardless of who triggered the run — a task is not a
privileged caller, and a test asserts that a locally-authored, unconfirmed path is refused by
`tasks run release` exactly as it is by the sweep.

**57.5** — the desktop app runs due tasks on the 1 Hz tick it already owns (`lib.rs:509-541`),
desktop-gated the way `sessions`/`notes` are in `CapabilitiesVm`, with no second interval added. A
failing task raises a notification **once per onset** and holds a sticky state, following `warn`
(`engine.rs:1094-1156`) — a task failing hourly is one notification, not twenty-four. Closing the
window does not stop tasks (`lib.rs:1106-1112`); quitting does, and the UI says so rather than
implying otherwise.

**57.6** — a Tasks view at **⌘8**: one `PrimaryView` member, one sidebar entry before Settings
(`sidebar-pane.tsx:244-257`), one `app-shell.tsx` arm, one palette row with the chip, and a
shortcut hook in the shape of `use-sessions-shortcut.ts` including its typing and IME guards. Each
row shows kind, schedule, **which host will run it**, next due, last run and last outcome, modelled
on the Sync pane's pending/parked lists (`sync-pane.tsx:1316-1447`) and rendering relative times
client-side from timestamps the way `formatSyncWaited` already does (`sync-pane.tsx:613-623`). A
task no present host can run reads **unhosted**, with the reason. Run-now is one command; both new
commands appear in `generate_handler!` on the desktop call site or
`command-registration.test.ts` fails.

**57.7** — `keeper-syncd-tasks.timer` + a oneshot service that calls `keeper-syncd tasks run`, in
`packaging/`, with the same user-unit posture as the existing service and `RestartPreventExitStatus`
honouring the exit taxonomy. `docs/sync.md` grows a §13 that states the task model, the one-shot
verb, that `update` is not and will not be a task, and — once, plainly — that macOS has no daemon,
so on macOS a task runs only while keeper is running.
