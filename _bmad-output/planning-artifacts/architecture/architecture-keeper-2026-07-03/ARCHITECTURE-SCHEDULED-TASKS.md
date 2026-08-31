---
name: 'keeper'
type: architecture-spine-companion
purpose: build-substrate
altitude: initiative
paradigm: 'hexagonal Rust core + unidirectional view-model projection — unchanged; a task is a named, recorded, idempotent invocation of work the engine already knows how to do'
scope: 'keeper scheduled tasks — named housekeeping work with a schedule, a last run and a last result, runnable on the daemon and on the desktop app, drivable by an external scheduler, and visible in a Tasks view; extended by Epic 58 with a Tasks view the owner can drive, a per-task missed-window policy, and a read-only projection of the other work each host already paces'
status: final
created: '2026-08-22'
updated: '2026-08-31'
binds: [FR-346..FR-352, NFR-42, NFR-43, FR-353..FR-360, NFR-44, NFR-45]
sources:
  - _bmad-output/planning-artifacts/research/virtual-files-2026-08-22/
  - docs/sync.md §12, §14
  - 'read-only triage of the owner''s 2026-08-31 Tasks-view pass — four scouts over worktree feat/57-5-the-app-runs-them-too @ dbb7874, every verdict at file:line'
parent: ARCHITECTURE-SPINE.md
---

# Architecture Companion — Scheduled tasks

Extends the frozen spine with **AD-135..AD-142** — AD-135..AD-137 for Epic 57's record, schedule
and host honesty; AD-138..AD-142 for Epic 58's missed-window policy and for the work this host was
already pacing before tasks existed. Nothing here renegotiates the spine: `keeper-sync`
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

**What Epic 58 adds is a choice and a projection, not a second mechanism.** Its headline
requirement — a window missed while nobody was home runs *once* — is already true and already
deliberate (AD-138): `next_due_ms` is one `i64`, not a queue, so "overdue by one window" and
"overdue by two hundred" are the same state by construction (`keeper-sync/src/tasks.rs:735-739`).
What is absent is the ability to *choose* what happens to that window (AD-139), a record that it
was declined (AD-140), and any way to see the other periodic work each host paces (AD-141,
AD-142). Two of those are policy, one is a row, and one is read-only — none of them is a clock.

---

## Architecture decisions AD-135 … AD-142

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
    `keeper-syncd watch` (`keeper-syncd/packaging/keeper-syncd.service`). Tasks run whether or not
    anybody is logged into a desktop — **conditional on `loginctl enable-linger`**, and the
    condition is load-bearing rather than a footnote: `systemctl --user is-enabled` answers *wanted
    at login*, so an enabled unit without lingering is torn down with its user's last session and
    the schedule stops there. That makes lingering a *fact about the machine* the surface has to
    establish, not an install note it may assume: it is the difference between two true sentences,
    and assuming it produced an over-claim in Story 57.5 (see *Failure is announced once per onset*
    and the unhosted rule in the Decision below). The app reads it as logind does, by stat-ing
    `/var/lib/systemd/linger/$USER` — the file `enable-linger` creates and the only thing logind's
    own `Linger` property checks.
  - **macOS ships the daemon binary and no way to start it.** Two separate facts, and Epic 57's
    first draft of this AD collapsed them into a wrong one. The daemon **does** build and ship for
    Darwin: `release.yml`'s `syncd` job matrix carries `aarch64-apple-darwin` on a `macos-latest`
    runner (`.github/workflows/release.yml:238-243`, build at `:263`), and its own header explains
    why the job is separate from the signed app rather than platform-limited (`:207-228`). What
    `keeper-syncd` actually claims of itself is *"Linux-first, unix-only"*, because *"secret files
    are enforced by mode bits and `doctor` reads `/proc`"* (`keeper-syncd/src/main.rs:11-13`) —
    a statement about `/proc` and mode bits, not about a build failing. **The provable fact is
    narrower and stronger: no launchd plist for the daemon exists anywhere in this tree.** So its
    verbs run on a Mac by hand or from a `cron` line the owner writes, and nothing starts them in
    the background — which is why `daemon_presence` is `Absent` by construction off Linux and says
    so at the call site (`keeper/src/sync_ipc.rs:2023-2029`), why `DaemonPresence::Absent`'s own doc
    records it — *"always the case on macOS, where no launchd plist for the daemon exists anywhere
    in the repository"* (`keeper-core/src/tasks.rs:188-191`) — and why `docs/sync.md:2191-2199`
    states both halves plainly. On macOS the **only** background host is the desktop app — which is
    a real host: closing the window calls
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
  while keeper is running"* on macOS without a daemon, and, on a Linux box where the unit is
  enabled, *whichever* daemon sentence is true of that box: *"logged in or not"* where the user
  lingers, and *"while you are logged in — lingering is off, so its schedule stops when your
  session ends"* where they do not. A task that no present host can run is shown as **unhosted**,
  not as enabled-and-quiet.
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

### AD-138 — Exactly-once catch-up is the existing design; a policy may govern the window, never enumerate it

- **Binds:** FR-356, FR-358, NFR-44; Epic 58
- **The mechanism, stated because Epic 58 must not claim to add it.** A task whose host was absent
  across two hourly windows yields **one** run, and it does so by construction rather than by a
  code path that could regress:
  1. `tasks::decide` (`keeper-sync/src/tasks.rs:725`) holds no count of elapsed windows. Its whole
     due test is a scalar compare on one stored instant — `None => Action::Arm`,
     `Some(at) if now_ms >= at => Action::Run`, `Some(_) => Action::None`
     (`tasks.rs:735-739`). `next_due_ms` is a single `i64`, **not a queue**, so "overdue by one
     window" and "overdue by two hundred" are the same state and are indistinguishable to the
     decider. There is no arithmetic anywhere that could produce N runs.
  2. The window is then **overwritten, never enumerated**. After the work, `claim_and_run` re-reads
     the clock and computes the next instant from the **finish** — not from the missed scheduled
     time — because *"a window computed from the instant the task became due would come due again
     the moment a run that overran it finished"* (`engine.rs:2231-2236`). That single line **is**
     the catch-up semantics.
  3. Per-window arbitration is one statement: a `Scheduled` trigger passes `due_at_most =
     Some(now_ms)` (`engine.rs:2214`) and `db::claim_task`'s conditional `UPDATE` carries
     `AND (?5 IS NULL OR (next_due_ms IS NOT NULL AND next_due_ms <= ?5))` (`db.rs:3303`), so one
     open window admits one claim and mints one `task_runs` row.
  4. Correctness across sleep comes from the clock contract, not from a wake notification:
     `SyncPlatform::now_ms` *"must be wall-clock, not monotonic: the scheduler has to reason about
     time that passed while the process was not running"* (`keeper-sync/src/platform.rs:312-315`).
     No sleep/wake handler exists and none is needed.
- **Rule.** **No `on_missed` setting may ever enumerate more than one missed window.** Not
  `run_now`, not `after a delay`, not a future fourth option. This is not tidiness: the `release`
  kind **deletes local content**, so N catch-up sweeps are N deletion passes at instants nobody
  chose — the tree names exactly that failure, in a regression test's own doc: re-enabling a
  `@daily` **release** task a month later *"fired a deletion sweep on the very next 1 Hz tick
  instead of at 03:00 — catch-up, which Epic 57 rules out by name, and which for the release kind
  means a deletion at an instant nobody chose"* (`db.rs:6289-6294`, asserted by
  `a_task_coming_back_into_service_arms_afresh_rather_than_catching_up`, `db.rs:6300-6305`).
- **What `MissedTickBehavior::Delay` is and is not.** The supervisor ticker sets it deliberately —
  *"Delay, not Burst: after a long stall we want one catch-up tick, not a backlog of them fired
  back to back at a git server"* (`engine.rs:2000-2003`; tokio's default is `Burst`). It is a
  thundering-herd guard on the git server. It is **not** the exactly-once guarantee, and conflating
  the two would misplace the whole design: even under `Burst` every tick re-reads `next_due_ms`
  from the row, and the first run pushes it into the future.
- **Prevents.** A story author reading the owner's request as *"implement exactly-once catch-up"*
  and rebuilding what exists, or a policy design that models missed windows as a backlog.

### AD-139 — `on_missed` is a three-way per-task policy, and its default is today's behaviour

- **Binds:** FR-356; Epic 58
- **Decision.** One additive column, `on_missed TEXT NOT NULL DEFAULT 'run_now'` on `tasks`, with
  three readable spellings and one rule each, decided in the pure layer:
  - **`run_now`** — the stored past window is honoured on the first tick that sees it. One run.
  - **`delay`** — the window is honoured, but not before `next_due_ms + delay`. Lateness is
    `now_ms - next_due_ms`, already on the row, so **no second column is needed**. The wait must be
    enforced in `decide`, **not** at the claim: `claim_task`'s `next_due_ms <= now` condition
    (`db.rs:3303`) passes throughout the delay and would let a `Requested` run through.
  - **`skip`** — the past window is abandoned and the next one armed. It **must re-arm**; returning
    `Action::None` leaves the past window standing, so the next tick decides again, forever.
- **`run_now` is the default because it reproduces today's restart behaviour**, so no existing
  install changes meaning on upgrade. It is also precisely systemd's `Persistent=true` semantics,
  in-process — and the shipped timer header already says so in the same words: *"A trigger missed
  while the machine was off or asleep fires once when it comes back, rather than waiting for
  tomorrow. Once, not once per missed day"*
  (`keeper-syncd/packaging/keeper-syncd-tasks@.timer:70-75`).
- **`skip` already exists, unnamed and unselectable.** `upsert_task` clears `next_due_ms` on three
  service edges — schedule text changed, disabled→enabled, mode became `scheduled` — precisely so a
  stale past window cannot fire (`db.rs:3050-3066`). So today the owner gets `run_now` or `skip`
  depending on which door the row last came through. The policy makes that choice explicit; it does
  not invent either behaviour.
- **Migration is real and additive; no JSON column absorbs it.** `tasks` has ten typed columns and
  no JSON blob (`db.rs:191-201`); the `json_set` precedent is on `profiles` (`db.rs:274-281`) and
  does not help. The DDL states its own rule: *"Any column added to either table later MUST be
  nullable or carry a DEFAULT, and MUST go through an additive `ensure_task_columns` rather than
  into this batch"* (`db.rs:184-189`) — and **`ensure_task_columns` does not exist yet**; only the
  comment demanding it does. It is written on the `ensure_journal_columns` shape (`db.rs:429-432`)
  and called beside its three siblings in `migrate` (`db.rs:234-236`). The `DEFAULT` is mandatory
  rather than tidy: `upsert_task`'s `INSERT` names its columns (`db.rs:3142-3146`), so an older
  binary writing against a newer schema fails without it.
- **`Action` gains a variant, and that is the point.** `Action` is `{None, Arm, Run}`
  (`tasks.rs:293-300`) and cannot express *skip*. `run_due_tasks`'s match is exhaustive
  (`engine.rs:2133-2148`), so a new variant **forces** every host to decide rather than inherit
  silence. `db::arm_task` cannot be reused for it: it is `WHERE id = ?1 AND next_due_ms IS NULL`
  because *"first sight can only happen once, so the statement says so"* (`db.rs:3256-3260`); a
  skip needs its own forward-only write.
- **An unreadable spelling is skipped and listed**, exactly as kind, mode, schedule and outcome
  already are — NFR-43's read half (`db.rs:3113-3128`), so a newer keeper's policy does not brick an
  older one's list.
- **Rule.** The policy ships with **both** its CLI flag and its form control in the same story. A
  knob writable by neither surface is born unreachable, which is the exact defect class Epic 58
  exists to close: no UI writes tasks today (`src/components/layout/tasks-pane.tsx:70-74`).

### AD-140 — A window the policy declines is a recorded fact, not a silence

- **Binds:** FR-357, NFR-44; Epic 58
- **The gap.** A missed window leaves **no row anywhere** today. `task_runs` rows are minted only by
  `db::claim_task` (`db.rs:3331`), which only executes when a host is present and reaches the task.
  So *"this window passed while nothing hosted it"* is invisible in history — and a `skip` or
  `delay` policy that declines a window without recording it would reintroduce the
  invisible-non-execution shape this whole feature exists to close. The engine already treats
  exactly that shape as the one place in the feature where a log level is load-bearing: *"the row
  stays unarmed, so the next tick decides `Arm` again and the task reports itself enabled and
  scheduled while nothing ever runs"* (`engine.rs:2160-2172`).
- **Decision.** A declined window is written as a **closed, zero-duration `task_runs` row** with its
  own outcome, so the Tasks view's *last run* moves and the run list says what happened. `detail`
  carries the declined instant and the policy that declined it.
- **No existing `TaskOutcome` can carry it**, and the doc comments settle it three times over
  (`keeper-sync/src/tasks.rs:184-205`): `Busy` is *"the work could not start because its target was
  already in use"*; `Deferred` is *"the work did not run because a condition it waits on was not
  met"*; `Abandoned` is *"the run was never closed by the host that started it"* and is written *"by
  the next host when it reclaims an expired lease"*. **All three require a host to have been present
  and to have reached the task.** None can be written when nothing was running.
- **`Deferred` in particular must not be reused.** `next_task_window` consumes it to retry within
  `TASK_RETRY_MS` — `min(scheduled, finished + 60 s)` (`engine.rs:2295-2301`) — so `Deferred` means
  *"try again very soon"*, the exact opposite of *"skip this window"*. Overloading it would silently
  turn `on_missed = skip` into `on_missed = retry in a minute`.
- **Prevents.** A policy whose two interesting settings are unobservable, and a Tasks view whose
  *last run* goes stale for a reason it cannot show.

### AD-141 — Other paced work is projected into the view, never migrated into the table

- **Binds:** FR-359, FR-360, NFR-45; Epic 58
- **The substrate argument still holds, and is verified live rather than assumed.** AD-135's reason
  for a second table is restated verbatim in the schema itself, and it is still true in today's
  code: *"`db::complete` is `DELETE FROM journal WHERE id = ?1`, so a finished unit leaves no trace
  at all, and `WorkKind` is a closed vocabulary of transfer primitives with no room for 'sync this
  folder nightly'; `activity` is by its own doc above 'a human-facing log, not a source of truth'
  and is capped per profile, so a schedule kept there would be forgotten by the thousandth file"*
  (`db.rs:167-175`; the `DELETE` is live at `db.rs:2128`, the `activity` doc at `db.rs:111`).
  Consequence: the owner's *"widziec joby synchronizacji git czy inne zaschedulowane"* cannot be
  served by pointing ⌘8 at an existing table. It is served by **projection from live state** —
  profiles, engine status, `activity` — as a distinct read-only class.
- **Rule: a task row over work whose own look-gate still stands is a schedule that does not
  schedule.** The tree states it in Story 57.1's own design notes, about the kind that hit it
  first: *"`release_expired` cannot be that kind: it carries its own hourly `release_is_due`
  look-gate, so a task's schedule would not control it — a nightly release task would have fired at
  03:00 and been declined by an interval that knows nothing about schedules."* That generalises
  unchanged to `scan_is_due` (`engine.rs:2917`), `sweep_is_due` (`engine.rs:3003`) and the notes
  cadence (`notes_vault.rs:2551-2566`).
- **`release_governance` is the only sanctioned way a task row may drive pre-existing paced work.**
  It folds every `Release` row for a folder into one least-permissive mode
  (`engine.rs:8165-8221`) and `release_permits` makes the row a **knob over** the existing
  success-edge sweep rather than a second driver: *"the schedule drives it **and** the success edge
  keeps working"* (`engine.rs:8226`). One body, two drivers, reasoned in place. Any future story
  that wants a real schedule over paced work builds that twin **and surrenders the existing gate in
  the same change**; adding a driver without surrendering the gate ships a folder that syncs twice.
- **What the duplication actually costs, precisely.** Not a corrupt git index: `sync_once` opens
  with `self.reserve(&profile.id)` (`engine.rs:7955-7957`) and `tick_profile` takes the same
  reservation (`engine.rs:2762-2766`), and `claim_task`'s lease serializes hosts. It costs
  **duplicated work and a lying record** — the task's run row reports *"1 synced"* for a folder the
  supervisor would have synced anyway, and a collision is recorded as `Busy`, which does not even
  consume the window (`engine.rs:2295-2301`).
- **The projected class carries no schedule editor and no Run now.** Those two controls are the
  claim *"keeper decides when this happens, and you can change it"*, which is false for
  event-triggered pacing: `scan_due` is `paced || watch_wake_pending || settle_window_elapsed`
  (`engine.rs:3099-3102`), so two of its three triggers are filesystem events a schedule cannot own.

### AD-142 — AD-62 is about clocks, not visibility

- **Binds:** FR-359, NFR-45; Epic 58
- **What AD-62 says, verbatim, and what it therefore constrains.** *"Called from the ~1 Hz tick that
  already drives the tray, which is the whole of AD-62: a 1 Hz tick has exactly the resolution a 2 s
  idle-commit needs, and two schedulers over one git repository is how you get concurrent index
  locks"* (`keeper/src/notes_vault.rs:2577-2579`). The subject is **schedulers over a repository**.
  A read-only projection registers no scheduler, so AD-62 permits it outright.
- **Decision.** A read-only projection into ⌘8 is permitted and adds nothing to the clock budget.
  **No second due-gate may be registered for work an existing gate already paces** — that, and not
  visibility, is the line.
- **The guard is mechanical, and the precedent for reading past it is already set.**
  `src/test/task-host-tick.test.ts` scans `keeper/src` and asserts exactly one
  `tokio::time::interval` exists (`keeper/src/lib.rs:510`); any story that adds a clock in the shell
  fails it. The Tasks pane's own 30 s display clock is already argued past AD-62 in the same terms:
  *"This is a display clock in the frontend and not a second scheduler — AD-62's rule is about
  `tokio::time::interval` in the `keeper` crate, and nothing here polls the engine"*
  (`src/components/layout/tasks-pane.tsx:424-429`).
- **Prevents.** A story refusing the owner's visibility ask on AD-62 grounds, and — the opposite
  error — a story satisfying it by registering a pacer of its own.

---

## Deferred

- **Arbitrary user commands as tasks** — a task kind is one of keeper's own verbs, never a shell
  string. Running user-supplied commands from a sync daemon is a different security posture
  (egress, credentials, `NoNewPrivileges=yes` in the unit) and needs its own decision. Revisit only
  with a stated threat model.
- **A launchd agent for `keeper-syncd` on macOS** — the honest fix for AD-137's asymmetry, and a
  separate deliverable: it needs a plist, an installer path and a `loginctl`-equivalent story for
  logout. It does **not** need the daemon crate to start building on macOS — that job already
  ships (`.github/workflows/release.yml:238-243`); the gap is purely that nothing launches it.
  Until then the app is the macOS host and AD-137 makes that visible.
- **Distributed task ownership across machines** — two clones of one folder both running a nightly
  release sweep is safe (each releases only what its own ledger proves), but *coordinating* them is
  not needed and not designed. Revisit if a task appears whose effect is not per-clone.
- **A task kind over the notes cadence** — the one genuine migration candidate left after AD-141:
  `notes_vault::cadence_tick` has a real per-vault identity and a real cadence
  (`keeper/src/notes_vault.rs:2551-2566`), unlike `scan_is_due`, whose triggers are two-thirds
  filesystem events (`engine.rs:3099-3102`). It stays deferred because it is the code AD-62's
  sentence is attached to, and because a task row over it is only honest if the existing gate is
  surrendered in the same change. Epic 58 projects it read-only (AD-141) instead. Revisit if the
  owner asks to *change* the notes cadence rather than to see it.
- ~~**Catch-up semantics for missed windows**~~ — **decided, AD-138..AD-140** (Epic 58). The first
  half of the deferred sentence, *"a task whose host was off for a week runs once when the host
  returns, not seven times"*, was never a deferral: it is the shipped behaviour and is now a rule
  with teeth. The second half, *"a `catch_up` policy is a knob nobody has asked for"*, was scope
  and is now false — the owner asked, in these words: *"moze byc opcja czy uruchomic or razu, z
  opiznieniem czy wogole i czekac na nastepny schedule w takiej sytuacji"*.

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

The riskiest seams for Epic 57, in order: **the lease** (AD-136 — two hosts on one `sync.db` is the normal case
on Linux, and a task that runs twice concurrently over one git repository is the concurrent-index-
lock failure AD-62 exists to prevent), **the schedule parser's refusal path** (AD-136 — a schedule
that parses to "never" and reports itself enabled is the invisible-failure shape), and **the
unhosted case** (AD-137 — the macOS user with the app quit must be told, not silently unserved).
Each is testable against a fixed clock and a real SQLite file; none of them needs a real timer, and
a story that proves a schedule by sleeping is asserting the wrong thing.

**FR-353–FR-360 are implementable within AD-138..AD-142, with no new crates and — for the whole
first wave — no Rust, no schema and no new IPC.** FR-353/FR-354/FR-355 are pure reachability: `sync_task_save`,
`sync_task_forget` and `sync_task_history` are implemented, registered in `generate_handler!`
(`keeper/src/lib.rs:977-982`), typed in `keeper-core`, wrapped in `src/lib/ipc/client.ts:6348,
6382,6391`, mocked in `dev/mock-shell.ts:1818-1897`, and have **zero production callers**; and
`task_runs.detail` is written on every completed run (`engine.rs:2417-2418` composes *"{synced}
synced, {busy} already syncing, {deferred} waiting, {failed} failed"*), carried on `TaskRunVm.detail`
(`keeper-core/src/tasks.rs:262-263`), printed by the CLI already
(`keeper-syncd/src/commands.rs:3299-3320`) and never read by `tasks-pane.tsx`. The form shape is
settled by `AddFolderForm` — one component, `editing = <thing> !== undefined`, seeded once, the
backend's refusal rendered verbatim, mounted inline rather than in a dialog
(`src/components/sync/add-folder-form.tsx:1046,1059,1061-1063`, AD-C7 at `sync-pane.tsx:20-24`);
`AlertDialog` is the confirm-a-destructive-action idiom (`files-pane.tsx:3146`). The run list
extends `SyncActivityList` (`sync-pane.tsx:1391-1443`) over the column set `task_run_lines`
(`commands.rs:3306-3318`) already settled. FR-356/FR-357 are the only schema work: one additive
`ensure_task_columns`, one `Action` variant, one `TaskOutcome` variant, one forward-only window
write. FR-359's projection reads state that already exists in memory
(`engine.rs:954-978`, `profile/mod.rs:144-206`).

The riskiest seams for Epic 58, in order: **the double-run hole** (AD-138 — a Linux box running both
`keeper-syncd watch` and Story 57.7's `Persistent=true` timer gets two runs for one missed window,
because a `Requested` trigger sets `due_at_most = None` (`engine.rs:2215`) and bypasses
`claim_task`'s window condition (`db.rs:3303`) while the daemon's next tick claims the same past
window independently — recorded at `deferred-work.md:5036-5042`, not reachable on macOS where no
daemon host exists); **the invisible decline** (AD-140 — `skip` and `delay` are unobservable until a
row exists, and the outcome vocabulary has no slot that does not lie); and **the projection's
honesty** (AD-141 — a read-only class that grows a Run now button becomes the duplication trap). All
three are testable against a fixed clock and a real SQLite file; the double-run needs **two
connections and two triggers**, not one process calling the runner twice.
