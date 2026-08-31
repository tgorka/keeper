# Epic 57 Context: A task that runs when it should, and says so when it did

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Give keeper named, scheduled, **recorded** housekeeping work — a task with a schedule, a last run and
a last result — runnable on the daemon, on the desktop app, and from an external scheduler.
Periodicity is not new: both hosts already run a 1 Hz tick with due-gates on it, and a one-shot CLI
pass is already the documented cron entry point. What is absent is anything that *remembers*: no name,
no schedule, no next-due time, no last outcome, so no surface can honestly say whether housekeeping
ran. One constraint shapes every story — **one clock per host process**, because two schedulers over
one git repository produce concurrent index locks. A task is therefore a due-gate on a tick that
already exists: never a thread, never a second interval, never an in-process timer. Two things stay
forbidden: a task may never install, replace or restart a binary (`update` is not and will not be a
task kind), and a task kind is one of keeper's own verbs, never a shell string.

## Stories

- Story 57.1: A task is something keeper remembers
- Story 57.2: A schedule that refuses what it cannot parse
- Story 57.3: The verb a cron can call
- Story 57.4: Release becomes a task
- Story 57.5: The app runs them too
- Story 57.6: A view of what runs, and when it last did
- Story 57.7: Packaging and the chapter

## Requirements & Constraints

- **FR-346** named tasks, each with a kind, an optional profile, a mode, a schedule, a next-due time, a
  last run and a last result.
- **FR-347** an unparseable schedule is refused when saved, with the expression quoted.
- **FR-348** a due task runs on its host's existing tick and never twice concurrently — including when
  a daemon and the app share one database.
- **FR-349** CLI verbs list, report and run tasks, with an exit code a cron wrapper can act on.
- **FR-350** release becomes a task with modes off / manual / scheduled, the existing success-edge
  behaviour still the default.
- **FR-351** the app shows schedule, host, next due, last run and last result, offers run-now, and
  notifies a failure once per onset.
- **FR-352** a task no present host can run reads *unhosted*; Linux packaging ships a timer unit
  calling the same one-shot verb.
- **NFR-42** a task never holds a git index lock concurrently with its host's sync pass, and every task
  is idempotent and safely abandonable mid-run — SIGTERM is a bounded finalize, never a corruption.
- **NFR-43** task history is bounded like the activity log, and a row of a kind this build does not
  know is skipped rather than fatal.

**How this epic must be tested.** The repeated failure here is a story asserting its claim through a
pure function while the risk lives in the impure shell. Keep the decision pure —
`decide(state, schedule, now_ms) -> Action` over the injected clock, asserted by tick count and never
by elapsed wall time. The risk sits in three impure places: **the lease** (two real connections to one
`sync.db` racing one due task — exactly one runs, and a dead holder's lease is reclaimable; calling the
runner twice in sequence in one process proves nothing), **the refusal path** (a schedule that parses
to "never" while reporting itself enabled is the invisible-failure shape), and **the unhosted case**
(macOS with no daemon must read *unhosted*, not *enabled-and-quiet*).

## Technical Decisions

**A task is a record, and neither existing table can be it.** Two new tables in the existing `sync.db`:
`tasks` — `(id TEXT PK, profile_id TEXT NULL, kind TEXT, schedule TEXT NULL, mode TEXT, next_due_ms
INTEGER NULL, enabled INTEGER, updated_ms INTEGER)`, null profile meaning host-wide — and `task_runs` —
`(id INTEGER PK AUTOINCREMENT, task_id TEXT, started_ms, finished_ms NULL, outcome TEXT NULL, detail
TEXT NULL, host TEXT)`, append-only, bounded, indexed on `(task_id, id DESC)`. The journal is a work
*queue* whose completion is a `DELETE`, over a closed vocabulary of transfer primitives a task sits
above; the activity log is human-facing and explicitly not a source of truth, whereas `task_runs` **is**
the truth for "when did this last run, and what happened". Both tables are `CREATE TABLE IF NOT EXISTS`
in the existing migrator, later columns use the additive `ensure_*_columns` idiom, and there is no
content migration and therefore no `meta` marker. A row of an unknown `kind` is **skipped, never
fatal** — what lets one host write a task the other host's older binary has never heard of.

**The schedule is validated at save time, refused rather than coerced.** Dialect: a 5-field cron
expression, or `@hourly` / `@daily` / `@weekly` / `every <n><unit>`. The parser is a keeper-owned
function over that small grammar — **no new dependency**. Anything else is refused when written, with
the expression quoted, as a typed config error, in the manner of the existing quiet-time validator. The
floor is one minute, and a shorter schedule is **refused, not clamped**, for the reason the daemon's
poll-interval floor exists: below it the scheduler spends more time waking than working.

**Evaluation is a due-gate on the tick each host already runs.** Due when `next_due_ms <= now` from the
injected platform clock, in the shape the existing scan and sweep gates have — the engine's supervisor
tick for the daemon, the app's own 1 Hz interval for the desktop. No new interval anywhere, and the
decision is separated from its effect the way the notes cadence already splits them.

**Exactly one runner, by lease.** A task row carries `running_host` + `lease_until_ms` claimed in the
*same* `UPDATE` that starts the run — the single-statement claim the journal already uses so two
supervisors cannot take one row. On Linux the daemon and the app can both be running against one
`sync.db`, which the tree already warns about; an expired lease is reclaimable, so a killed host does
not wedge a task forever.

**An external scheduler is a first-class driver, not a workaround.** The one-shot verb runs exactly one
task, prints its outcome, exits with the existing exit-code taxonomy, and records the run identically
to an in-process one, so cron, a systemd timer and a human all reach the same code path. A systemd timer
as the *only* mechanism was rejected — it leaves macOS with nothing and puts the schedule where keeper
cannot validate, show or report on it; the shipped unit is a thin caller of the verb.

**Which host runs a task is a platform fact, and the asymmetry is load-bearing.** Linux has a packaged
background host (a systemd *user* unit running the daemon's watch mode, with lingering for post-logout),
so tasks run with nobody logged in. **macOS has no daemon at all** — that crate is Linux-first,
unix-only, and deliberately does not build elsewhere; the only macOS background host is the app, which
is a real one (close prevents-close and hides, keeping process, engine and notifications alive;
launch-at-login exists) but **quit means quit**. iOS is not a host, so tasks are desktop-gated the way
sessions and notes already are in the capabilities view-model.

**Record shared, intent per host.** Both hosts read and write the same tables when they share a data
dir, and by default they do not share one. A schedule both surfaces must honour lives in the folder TOML
tier where both read it; the `tasks` table is the *runtime* record for whichever host owns that data
dir, and when they disagree the TOML layer wins on every read.

**Deferred:** arbitrary user commands as task kinds (a different security posture), a launchd agent for
the daemon on macOS, distributed task ownership, and catch-up — a task whose host was off for a week
runs **once** when it returns, not seven times.

## UX & Interaction Patterns

- **The surface asserts the host; it never implies it.** A row states kind, schedule, *which host will
  actually run it*, next due, last run and last outcome; a task no present host can run reads
  **unhosted** with the reason. This is architecture, not copy: non-execution of housekeeping is
  invisible by nature, and this tree has twice shipped a feature that looked enabled and did nothing.
- **Failure is announced once per onset, never per attempt** — notify on the `None → Some` edge and hold
  a sticky state, per the engine's existing warn rule, which exists because a text-keyed rule once
  produced thousands of notifications an hour.
- **The honest macOS sentence, once:** a task runs only while keeper is running; closing the window does
  not stop tasks, quitting does, and the UI says so. Relative times render client-side from timestamps.
- **⌘8 is the first free number** (sessions holds ⌘7): one primary-view member, one sidebar entry before
  Settings, one app-shell arm, one palette row, and a shortcut hook in the existing shape with its
  typing and IME guards. Both new IPC commands must appear in the desktop registration splice or the
  command-registration test fails.

## Cross-Story Dependencies

- **57.1 → 57.2 is a strict chain** (the due-gate needs the record); **57.3 and 57.4** both depend on
  57.2 and are disjoint from each other.
- **57.5 depends on 57.2** and is where the desktop host appears; **57.6 depends on 57.5** for the host
  vocabulary it must display honestly; **57.7 last**, because a documented timer calling a verb that
  does not exist is worse than no docs.
- **57.4 closes the owner's ask** and can ship before 57.5/57.6: a release task driven by cron calling
  the one-shot verb needs no UI at all.
- **Epic 56 is consumed here**: its release sweep becomes the first built-in task kind, its success-edge
  behaviour stays the default so upgrading changes nothing, and every refusal in its release path
  applies identically no matter who triggered the run — **a task is not a privileged caller**.

## Repo Anchors

Verified against this worktree by grep; planning-doc line numbers are stale. `tasks`, `task_runs`,
`running_host`, `lease_until_ms` and `next_due_ms` appear **nowhere** in `db.rs` — the record does not
exist yet.

### `src-tauri/crates/keeper-sync/src/db.rs`

| what | where |
|---|---|
| `migrate` — where the two new `CREATE TABLE IF NOT EXISTS` go | `:58` |
| `activity` table (the shape and discipline `task_runs` copies) | `:107` |
| `materialized`, `meta` tables | `:148`, `:155` |
| additive-column idiom: `ensure_activity_columns` / `ensure_journal_columns` | `:238` / `:359` |
| `enqueue_unique` | `:1792` |
| `claim_ready` — the single-statement claim the lease copies | `:1857` (`claim_ready_of_kind` `:1879`) |
| `complete` — "the only place work leaves the journal" (`DELETE`) | `:2008` |
| unknown-kind-is-skipped rule in prose | `:2634-2636` (profile twin `:1462`) |
| existing skip tests to model 57.1's fictional-kind test on | `:2744`, `:4050` |

### `src-tauri/crates/keeper-sync/src/engine.rs`

| what | where |
|---|---|
| `TICK_MS: u64 = 1_000` | `:400` |
| `SWEEP_EVERY_MS: i64 = 3_600_000` | `:442` |
| `Engine::run` — supervisor loop and its `tokio::time::interval` | `:1690-1691` |
| `warn` — the once-per-onset notification rule | `:1631` |
| `tick_profile` | `:1759` |
| `scan_is_due` / `sweep_is_due` — the due-gate shape to copy | `:1926` / `:2012` |
| a due-gate test advancing the injected clock (pattern to follow) | `:11376-11379` |

### Schedule validation and floors

| what | where |
|---|---|
| `validate_quiet_time` — refuse-with-the-value-quoted precedent | `keeper-sync/src/profile/mod.rs:736` |
| `MIN_POLL_INTERVAL_MS` (engine tier, floored) | `keeper-sync/src/profile/mod.rs:150`; applied `:1084` |
| `MIN_POLL_INTERVAL_MS` (daemon tier, **refused** with the value quoted) | `keeper-syncd/src/config.rs:40`; refusal `:147-152` |
| the refuse-vs-floor divergence, asserted on purpose | `keeper-sync/src/profile/mod.rs:1444-1488` |

### Hosts and CLI

| what | where |
|---|---|
| desktop 1 Hz tick (`tokio::time::interval`, `MissedTickBehavior::Skip`) | `keeper/src/lib.rs:510` |
| AD-62 stated in prose at the desktop tick | `keeper/src/lib.rs:533-537` |
| `notes_vault::cadence_tick` — the decision/effect split to follow | `keeper/src/notes_vault.rs:2580` (rule `:2578-2579`) |
| close hides, process survives (`prevent_close` + `hide`) | `keeper/src/lib.rs:1120-1124` |
| "the running engine and `keeper-syncd` also write" `sync.db` | `keeper/src/ipc.rs:4926-4928` |
| `sync_exit_code` — the exit taxonomy `tasks run` reuses | `keeper-syncd/src/commands.rs:130` |
| `Command` enum (where a `Tasks` arm goes) | `keeper-syncd/src/commands.rs:230`; `Sync` `:247`; dispatch `:691` |

### Frontend

| what | where |
|---|---|
| `PrimaryView` union (one new member) | `src/lib/stores/primary-view.ts:38` |
| `CapabilitiesVm.sessions` — the desktop gate to mirror | `keeper-core/src/vm.rs:137` (struct `:93`) |
| sessions holds `⌘7`; **`⌘8` appears nowhere in the tree** | `keeper-core/src/palette.rs:653` |
