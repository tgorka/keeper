# Epic 58 — A task you can drive, and a window that passed while nobody was home

created: '2026-08-31'
source: the owner's Tasks-view pass, written after installing the app and opening ⌘8 — *"dodaj opcje w desktop app na dodanie tasku … Chce tez widziec joby synchronizacji git czy inne zaschedulowane w keeperze rzeczy w tym widoku i tym mechanizmie"*. Grounded by four read-only repository scouts over `feat/57-5-the-app-runs-them-too` @ `dbb7874` (the create/edit surface, run history and output, missed-window semantics, and what else this host paces), 2026-08-31. Every verdict below carries the `file:line` it was read from.
binds: FR-353…FR-360 (allocated here), NFR-44, NFR-45; AD-138…AD-142 (new, `architecture/architecture-keeper-2026-07-03/ARCHITECTURE-SCHEDULED-TASKS.md`); AD-135, AD-136, AD-137 (Epic 57, consumed here — AD-137's macOS paragraph corrected); AD-62 (untouched, and re-read rather than assumed); AD-98, AD-132 (untouched); AD-C7 (the one-component-two-modes form rule, consumed by 58.1)
see-also: Epic 57 (`epic-57-a-task-that-runs-when-it-should.md`) — the mechanism this epic makes drivable. Epic 58 stacks on it; it is not merged.

## What he said

> dodaj opcje w desktop app na dodanie tasku (jestem ok jezeli bedzie wywolywany tylko jak keeper
> bedzie aktywny i komputer aktywny) - jak keeper nie bedzie aktywny to scheduler uruchomi zalegle
> zadania jak bedzie aktywny (jak jest zaschedulowany np co godzine a keeper sie uruchomi za 2h to
> zalegly run bedzie uruchomiony tylko raz) - moze byc opcja czy uruchomic or razu, z opiznieniem
> czy wogole i czekac na nastepny schedule w takiej sytuacji
>
> W widoku schedule bede rowniez widzial kiedy sie ostatnio uruchomil, bede mogl manualnie kliknac
> uruchom, bede mogl zobaczyc czy sie zakonczyl ostatnio pozytywnie czy negatywnie (czy wogole
> jeszcze sie nie uruchomil). bede mogl widziec output, oraz widziec widok z listy uruchomien i te
> informacje z uruchomien
>
> Bede mogl edytowac w schedule view.
>
> Chce tez widziec joby synchronizacji git czy inne zaschedulowane w keeperze rzeczy w tym widoku i
> tym mechanizmie
>
> Uzyj bmad jezeli nie uruchomiony czy zaschedulowany jeszcze do uruchomienia

Nine asks, in reading order: **create** a task from the app; **run missed work once** when the host
returns; **choose** what happens to a missed window — at once, delayed, or not at all; see **when it
last ran**; **run it by hand**; see **whether it ended well or badly, or has never run**; see its
**output**; see a **list of runs**; **edit** in the same view. Then one more, which is the largest:
show **git sync and everything else keeper schedules** in this view and in this mechanism.

One sentence in that block is a **concession, not a requirement**, and it is worth naming as one:
*"jestem ok jezeli bedzie wywolywany tylko jak keeper bedzie aktywny i komputer aktywny."* It costs
this epic nothing. There is no launchd plist for the daemon anywhere in the tree, so on macOS the
desktop app already **is** the only host and AD-137 already says so on screen
(`docs/sync.md:2191-2199`). No story here has to fight sleep: correctness across a suspend comes
from the clock contract — *"must be wall-clock, not monotonic: the scheduler has to reason about
time that passed while the process was not running"* (`keeper-sync/src/platform.rs:312-315`) — and
no wake handler exists or is needed.

And one parenthetical is **the exactly-once rule stated in his own words**:

> (jak jest zaschedulowany np co godzine a keeper sie uruchomi za 2h to zalegly run bedzie
> uruchomiony tylko raz)

That is already true, already deliberate, and already regression-tested. See *What exists* row 4 and
*Where this epic departs* item 1 — his sentence and the code agree, so no story promises to build it.

## What exists, and what does not

| # | ask | verdict | evidence |
|---|---|---|---|
| 1 | create a task from the app | **unreachable, not absent** | `sync_task_save` is implemented (`keeper/src/sync_ipc.rs:2186`), an UPSERT whose doc states the edit path — *"A blank `id` mints one … a non-blank one is used verbatim, so a task can be edited rather than duplicated"* (`sync_ipc.rs:2176-2178`) over `db::upsert_task`'s `INSERT … ON CONFLICT(id) DO UPDATE` (`keeper-sync/src/db.rs:3082`) — registered in `generate_handler!` (`keeper/src/lib.rs:977-982`), typed as `TaskSaveReq` (`keeper-core/src/tasks.rs:345-359`), wrapped as `syncTaskSave` (`src/lib/ipc/client.ts:6382`), and answered by a stateful dev mock (`dev/mock-shell.ts:1868-1897`). **Production callers: zero.** The only references under `src/` are the wrapper and a `vi.fn()` at `tasks-pane.test.tsx:18`. The pane instead prints a CLI command (`tasks-pane.tsx:558-566`) |
| 2 | edit in the schedule view | **unreachable, not absent** | the same verb. `TaskVm` already carries every field `TaskSaveReq` needs, so an edit form seeds from the row the pane already holds **with no extra IPC read**. Delete is equally built and equally uncalled: `syncTaskForget` (`client.ts:6391`), `sync_task_forget` (`sync_ipc.rs:2270`), whose own framing is already written — *"Deletes a record, never content"* (`sync_ipc.rs:2270-2272`) |
| 3 | when it last ran / run by hand / good or bad or never | **already on screen** | last run `tasks-pane.tsx:359-361`; Run now button `:344-353` → `runNow` `:489-513` → `syncTaskRunNow` → `engine.rs:7916-7932`, the *same* `claim_and_run` a scheduled run takes; outcome `:362` with `OUTCOME_LABELS` `:199-206`; never-ran is genuinely representable and distinct from long-ago (`TASK_NEVER_RAN_TEXT` `:159`, `TaskVm.last_run: Option<TaskRunVm>` `keeper-core/src/tasks.rs:303-304`). The *"looks never-ran but was actually a failed read"* trap was found and closed inside Story 57.5's own review (`spec-57-5:297-300`) |
| 4 | a missed window runs exactly once | **already true, and deliberate** | `tasks::decide` holds no count of elapsed windows: `None => Arm`, `Some(at) if now_ms >= at => Run`, `Some(_) => None` (`keeper-sync/src/tasks.rs:735-739`). `next_due_ms` is one `i64`, **not a queue**, so overdue-by-one and overdue-by-two-hundred are the same state. The window is then overwritten, never enumerated — the next instant is computed from the **finish**, because *"a window computed from the instant the task became due would come due again the moment a run that overran it finished"* (`engine.rs:2231-2236`). Regression-tested: `a_task_coming_back_into_service_arms_afresh_rather_than_catching_up` (`db.rs:6300-6305`) |
| 5 | choose at-once / delayed / skip | **genuinely absent** | `tasks` has ten typed columns and no JSON blob (`db.rs:191-201`), and `ensure_task_columns` **does not exist** — only the DDL comment demanding it: *"Any column added to either table later MUST be nullable or carry a DEFAULT, and MUST go through an additive `ensure_task_columns` rather than into this batch"* (`db.rs:184-189`). `Action` is `{None, Arm, Run}` (`tasks.rs:293-300`) and **cannot express skip**: returning `None` leaves the past window standing, so the next tick decides again, forever. Two of his three options exist unnamed — `run_now` is what an ordinary restart does, `skip` is what `upsert_task`'s three service edges do (`db.rs:3050-3066`) — so today he gets one or the other depending on which door the row last came through |
| 6 | see the output | **unreachable, and narrower than the word suggests** | `task_runs.detail` (`db.rs:213-221`) is written on **every** completed run: `perform_sync_task` composes `format!("{synced} synced, {busy} already syncing, {deferred} waiting, {failed} failed")` (`engine.rs:2417-2418`), `format!("{detail}: {reason}")` on failure (`:2420`), persisted at `:2249-2262`. It reaches the frontend on both `TaskVm.lastRun` and `sync_task_history` (`keeper-core/src/tasks.rs:262-263`) and the CLI already prints it (`keeper-syncd/src/commands.rs:3299-3320`). `grep detail src/components/layout/tasks-pane.tsx` → **no matches**. There is no stdout: a task run is an in-process `sync_once`/`release_expired` call (`engine.rs:2318-2334`), not a child process, so `detail` is the only capture point and it is already correct |
| 7 | a list of runs | **unreachable, not absent** | the whole path is built: DDL + index `task_runs_recent` (`db.rs:225-226`), cap trim (`db.rs:3340-3348`), `db::task_runs` (`:3513-3517`), `Engine::task_history` (`engine.rs:7882-7884`), `sync_task_history` with a clamped limit (`sync_ipc.rs:2123-2137`), `syncTaskHistory` (`client.ts:6348`), mock (`dev/mock-shell.ts:1818-1828`). The only reference under `src/` is the `vi.mock` stub at `tasks-pane.test.tsx:17`. The pane imports two of five verbs (`tasks-pane.tsx:47`) |
| 8 | git sync and everything else, in this view and mechanism | **projection, not migration** | the per-profile pacing has no identity, no persistence and no result: `scan_is_due` is paced by `profile.effective_poll_interval_ms` (`engine.rs:2917-2932`), its window lives in a `Mutex<HashMap>` that dies with the process (`engine.rs:954`), and it is not even purely periodic — `scan_due` is `paced ‖ watch_wake_pending ‖ settle_window_elapsed` (`engine.rs:3099-3102`). The tasks-vs-journal argument still has teeth and is verified live: `db::complete` is still `DELETE FROM journal WHERE id = ?1` (`db.rs:2128`) and `activity` is still *"a human-facing log, not a source of truth"* (`db.rs:111`), restated in the schema at `db.rs:167-175`. A naive task row over already-paced work is **a schedule that does not schedule** — Story 57.1's own note: *"a nightly release task would have fired at 03:00 and been declined by an interval that knows nothing about schedules"* |
| 9 | AD-62 forbids any of this | **no — AD-62 is about clocks, not visibility** | its verbatim subject is *"two schedulers over one git repository is how you get concurrent index locks"* (`keeper/src/notes_vault.rs:2577-2579`). A read-only projection registers no scheduler. The mechanical guard is a source scan for `tokio::time::interval` in `keeper/src` (`src/test/task-host-tick.test.ts`), and the pane's own 30 s display clock is already argued past it in the same terms — *"a display clock in the frontend and not a second scheduler"* (`tasks-pane.tsx:424-429`) |

## The finding this epic is built on

**Five of the nine asks are unreachable, not absent.** `sync_task_save`, `sync_task_forget` and
`sync_task_history` are implemented, registered, typed, wrapped in TypeScript, mocked in the dev
shell, tested in Rust — and called by nothing. `task_runs.detail` is written on every completed run
and read by no component. The pane's own copy states the gap out loud: *"No tasks yet. This view
lists, inspects and runs tasks; it cannot create one yet"* with the reason above it —
*"`sync_task_save` is a registered command and the wire type exists, but no control here calls it"*
(`tasks-pane.tsx:70-74`, `:107`).

So **Wave 1 is a pure frontend wave: no Rust, no schema, no new IPC, no new AD.** That is the whole
reason this epic is small, and it is also the reason it exists at all — this is the second time in
two epics that a complete backend shipped with no door. Epic 56 shipped virtual files the owner
could not see because `VirtualPolicy` had one production consumer
(`sprint-status.yaml:893-907`); Epic 57 shipped a task record the owner cannot write. The lesson is
now a rule with teeth in AD-139: **a knob ships with every surface that must write it, in the same
story.**

Nothing in Epic 57 refused a create form. It was never allocated: FR-351/FR-352 mention no creation
surface (`epic-57:58-59`) and 57.6's scope was display-plus-run (`epic-57:92`, `:142-150`). The
empty-state sentence is a truthful *description* of the gap, not a guard over an invariant — unlike
AD-137's host claim, which does have teeth. So 58.1 reverses no decision; it obsoletes three
constants, and must rewrite them in the same change or the app will keep pointing at a terminal
while a button sits above the sentence (`TASKS_PANE_EMPTY_SENTENCE` `:107`,
`TASKS_PANE_EMPTY_COMMAND` `:113`, `TASKS_PANE_EMPTY_AFTER` `:123`, all test-anchored in
`tasks-pane.test.tsx`).

## Where this epic departs from what he asked for

Written here rather than buried in a story, because each one is a place where the honest answer is
not the literal one.

1. **"zalegly run bedzie uruchomiony tylko raz" is already true — no story builds it.** It is a
   property of the data model, not a code path: one scalar window, computed from the finish instant,
   arbitrated by one conditional `UPDATE` (`tasks.rs:735-739`, `engine.rs:2231-2236`, `db.rs:3303`).
   It was recorded as *deferred* by Epic 57, and half of that deferral was wrong: the sentence *"a
   task whose host was off for a week runs once when the host returns, not seven times"* described
   shipped behaviour, and only *"a `catch_up` policy is a knob nobody has asked for"* was scope.
   AD-138 promotes the first half to a rule and AD-139 answers the second. **What this epic does add
   is the defence of it** — 58.6 closes the one real hole, on Linux only.
2. **"widziec joby synchronizacji git … w tym widoku i tym mechanizmie" resolves to projection plus
   one governance story, not migration.** Two different sentences: *in this view* is 58.7 and is
   cheap; *in this mechanism* is 58.8 and is the expensive half, because a `Sync` task today is a
   **second body** — `perform_sync_task` → `sync_once` (`engine.rs:2353`, `:7946`) — beside the
   ordinary `tick_profile` → `scan_due` → `drain_journal` path (`engine.rs:2750`, `:3099`), and
   nothing suppresses `scan_is_due` when a Sync task exists. The cost is not a corrupt git index
   (both routes take `Engine::reserve`, `engine.rs:7955-7957` vs `:2762-2766`, and `claim_task`'s
   lease serializes hosts): it is duplicated work and **a lying record** — the run row reports "1
   synced" for a folder the supervisor would have synced anyway. Epic 57 solved this shape once, for
   Release, with `release_governance` (`engine.rs:8165-8221`) making the task row a knob over the
   pre-existing sweep — *"the schedule drives it **and** the success edge keeps working"*
   (`engine.rs:8226`). 58.8 is that twin for Sync. There is no `sync_governance` today.
3. **Most of "everything else keeper schedules" must stay projection forever.** An inventory of every
   periodic thing in the product found two clocks and twenty-two periodic or quasi-periodic items hanging off them. The
   ones with a real identity and a real cadence are the per-profile scan
   (`engine.rs:2917`), the hourly LFS scratch sweep (`engine.rs:2952`, `:3003`,
   `SWEEP_EVERY_MS` `:443`) and the notes cadence (`notes_vault.rs:2551-2566`). The rest are queue
   reads, event drains, watcher re-arms, session-scoped watchdogs, startup recovery passes and
   library-internal timers — a task row over any of them would **claim a cadence that does not
   exist**. 58.7 projects the three that have one, read-only; the notes cadence stays deferred in
   the companion, because it is the code AD-62's sentence is attached to.
4. **"bede mogl widziec output" cannot mean stdout, because there is none.** A task run is an
   in-process call, not a child process (`engine.rs:2318-2334`), so nothing captures a stream and no
   column exists to hold one. What exists is a one-line structured summary written on every run
   (`engine.rs:2417-2418`), and 58.2 puts it on screen. Per-file granularity is the `activity` table
   reached by `sync_activity` (`sync_ipc.rs:1442`) — a join to display, not a new capture point, and
   deliberately out of scope here.
5. **His concession is recorded as a concession, not built as a requirement.** *"jestem ok jezeli
   bedzie wywolywany tylko jak keeper bedzie aktywny"* matches the shipped design exactly, so it
   buys the epic a story it does not have to write. It does **not** mean the missed-window question
   goes away — it is precisely because the host is often absent that the policy matters.
6. **"Uzyj bmad" is satisfied by this artifact set**, not by a story: the architecture companion's
   AD-138…AD-142, this epic, and the nine story keys in `sprint-status.yaml`.

## What the binds mean

FR-353…FR-360 and NFR-44/NFR-45 are allocated here. **FR-352 and NFR-43 were the previous ceilings**
(`epic-57-a-task-that-runs-when-it-should.md:5`, and `:49` recording FR-345/NFR-41 as the ceilings
before it); a repo-wide grep over `*.md`, `*.rs`, `*.ts`, `*.tsx` and `*.yaml` finds nothing above
either. AD-137 was the AD ceiling
(`architecture/architecture-keeper-2026-07-03/ARCHITECTURE-SCHEDULED-TASKS.md:51`).

| id | statement | story | AD |
|---|---|---|---|
| FR-353 | A task can be created, edited and deleted from the app: one form in two modes, the backend's refusal shown in its own words, and a confirm before a record is forgotten | 58.1 | AD-135, AD-137, AD-C7 |
| FR-354 | A task row states what the run itself reported, not only that it ended | 58.2 | AD-135 |
| FR-355 | A task's runs are openable as a bounded, newest-first list carrying outcome, time, host and detail per run | 58.3 | AD-135 |
| FR-356 | Each task carries a missed-window policy — run now, run after a delay, or skip and wait for the next window — writable from the CLI **and** from the app, defaulting to today's behaviour | 58.4 | AD-138, AD-139 |
| FR-357 | A window a policy declines is recorded as a closed run with its own outcome, so *skip* and *delay* move the last-run line instead of going quiet | 58.5 | AD-140 |
| FR-358 | One missed window yields one run even when a background daemon and an external timer both drive the same task on one host | 58.6 | AD-138 |
| FR-359 | The Tasks view also shows the other work this host paces, as a read-only class with its real cadence, no schedule editor and no Run now | 58.7 | AD-141, AD-142 |
| FR-360 | A sync task governs the profile's existing pacing rather than adding a second driver to it | 58.8 | AD-141 |
| NFR-44 | No missed-window policy may enumerate more than one missed window, whatever its setting, and no policy may decline a window without leaving a record | 58.4, 58.5, 58.6 | AD-138, AD-140 |
| NFR-45 | Nothing added by this epic registers a clock, a due-gate or a second pacer: the projected class is read-only, and a governance story surrenders the existing gate rather than racing it | 58.7, 58.8 | AD-141, AD-142 |

58.9 allocates no FR of its own; it documents FR-353…FR-360 in `docs/sync.md` §14.

## Why the suite cannot see the risk in this epic

The recurring lesson in `sprint-status.yaml` — *a story that asserts its central claim through a
pure function while the risk lives in the impure shell comes back `incorrect`* — has three shapes
here, and only the first is obvious.

**Shape one, the pure trap that is not the risk.** `decide` is where the policy lives, and it is
pure over an injected clock. A three-way policy is four table-driven cases and they will all pass on
the first try. That is not where 58.4 can lose. It loses in the **claim**: the delay must be
enforced in `decide`, because `claim_task`'s `next_due_ms <= now` condition (`db.rs:3303`) passes
throughout the delay window and a `Requested` trigger bypasses it entirely
(`engine.rs:2214-2215`). A test that only exercises `decide` proves the delay and misses that a
`tasks run` during the delay runs anyway.

**Shape two, two drivers rather than two connections.** Epic 57's impure risk was two processes on
one `sync.db`, and 57.2 proved it with two racing connections. 58.6's risk is different and a
straight copy of that test will not find it: **one host, two triggers.** The systemd timer's
`tasks run` arrives as `TaskTrigger::Requested`, which sets `due_at_most = None`
(`engine.rs:2215`) and so skips the window condition, while the daemon's next tick claims the same
past window as `Scheduled`. Both claims succeed and one missed window yields two runs. It is already
recorded as reachable in normal operation rather than in theory — *"a `Busy`/`Deferred` run sets
`next_due_ms` to `min(scheduled, now + TASK_RETRY_MS)`, which can already be past — so a requested
run landing on an overdue window is reachable in normal operation, not only in theory"*
(`deferred-work.md:5036-5042`), and the shipped timer header warns about it. The test must drive
**both trigger kinds against one overdue window**, not two connections against one lease.

**Shape three, the negative nobody asserts.** A declined window is a *non-event*, and this feature's
one load-bearing `warn` exists because non-events are invisible: *"the row stays unarmed, so the
next tick decides `Arm` again and the task reports itself enabled and scheduled while nothing ever
runs"* (`engine.rs:2160-2172`). 58.5 must assert that `skip` and `delay` **produce a row** and that
the row is not one of the three outcomes that would lie about it. And 58.7 must assert its own
negative: the projected class has **no** Run now and **no** schedule field, because the moment it
grows either it has claimed a cadence it does not own.

One more, cheaper but real: **58.1 must not re-implement a single validation rule.** Every one is in
Rust already — `validate_id`, refused rather than trimmed because *"silently accepting three
spellings of one intended task is worse than saying so"* (`tasks.rs:700-714`); the schedule floor
and ceiling (`tasks.rs:31`, `:42`, `:386-391`); the scheduled-with-no-schedule refusal
(`db.rs:3087-3092`); the NFR-43 stored-row guard (`db.rs:3113-3128`). The form's own precedent
states the rule: *"Nothing here re-implements those rules or tidies input up to make a save
succeed"* (`add-folder-form.tsx:30-31`). A client-side cron regex is the failure mode.

## Stack order

    Wave 1 — reachability, frontend only, no Rust
    58.1  a task you can create and edit              (sync_task_save in two modes + sync_task_forget; the three empty-state constants rewritten)
    58.2  the row says what the run said              (TaskRunVm.detail on the row)
    58.3  a list of runs you can open                 (sync_task_history, expandable per row)

    Wave 2 — the missed-window policy, Rust + schema
    58.4  a window that passed while nobody was home  (the three-way on_missed policy, ensure_task_columns, the new Action variant, CLI flag AND form control)
    58.5  a window nobody ran is still a fact         (the recorded outcome that makes skip and delay observable)
    58.6  two hosts, one missed window, one run       (the daemon-plus-timer double-run)

    Wave 3 — projection
    58.7  everything else this host paces             (a read-only class in ⌘8: no schedule editor, no Run now)
    58.8  a sync task that governs instead of duplicating (the sync_governance twin of release_governance)
    58.9  the chapter grows a policy                  (docs/sync.md §14)

Wave 1's three stories are **disjoint and parallelisable**: 58.1 owns the form and the empty state,
58.2 owns one `Field` on the row, 58.3 owns one expandable section. They collide only in
`tasks-pane.tsx`, so one of them owns the file and the other two coordinate.

58.4 → 58.5 is a strict chain: the outcome exists to make the policy observable, and a policy shipped
without it is the invisible-non-execution shape. 58.6 depends on 58.4 only for vocabulary — the
double-run hole exists today and could be fixed alone — but it belongs after, because the fix is a
statement about which trigger may consume a window and the policy is what makes that statement
readable.

58.7 depends on nothing in Wave 2. **58.8 depends on 58.7**, not the reverse: the projected class is
where a governed sync task has to appear once it stops being a second driver, and building the
governance fold with nowhere honest to render it repeats the mistake this epic is fixing. 58.9 last,
because a documented policy whose form control does not exist is worse than no documentation.

**58.1 is the story that closes the owner's first and loudest ask**, and it ships with no Rust at
all.

## Acceptance, per story

**58.1** — one component in two modes in the `AddFolderForm` mould (`add-folder-form.tsx:1046`,
`editing = <thing> !== undefined` at `:1059`, seeded **once** from the prop at `:1061-1063`), mounted
**inline** in the Tasks pane header and empty state rather than in a dialog — the AD-C7 rule, *"one
component in two places so the two surfaces cannot word or validate the same profile differently"*
(`sync-pane.tsx:20-24`). Create sends `id: ""` so Rust mints the ULID (`sync_ipc.rs:2205-2209`); edit
sends the row's `id` verbatim and seeds from the `TaskVm` the pane already holds, with **no extra IPC
read**. Every refusal is rendered in Rust's own words with nothing re-worded, nothing corrected and
**no trimming** — a typed `" nightly"` is refused, not tidied (`tasks.rs:704-707`). The form offers
the two vocabularies no IPC enumerates (`kind`: `sync`/`release`; `mode`: `off`/`manual`/`scheduled`)
as frontend constants in the `SYNC_DIRECTIONS` pattern, keeps `enabled` a **separate** control from
`mode` (AD-135 makes them two questions), and sources the profile picker from `syncProfiles()` plus
an explicit whole-machine option — **the one refusal the backend does not make**: `sync_task_save`
accepts a `profileId` naming nothing and the row comes back `unhosted`
(`keeper-core/src/tasks.rs:151-152`). Forget uses the `AlertDialog` confirm idiom
(`files-pane.tsx:3146`) and says what it does — *"Deletes a record, never content"*. Edit is **not
offered on an `unknown` row** (`tasks-pane.tsx:586-611`): `upsert_task` would refuse it
(`db.rs:3113-3128`), so the control would be a button that can only fail. The listing is re-read
after a save via the pane's existing `refresh()` (`:450`) so `nextDueMs` and the host verdict move.
`TASKS_PANE_EMPTY_SENTENCE`, `TASKS_PANE_EMPTY_COMMAND` and `TASKS_PANE_EMPTY_AFTER` (`:107`, `:113`,
`:123`) are rewritten with their assertions. Twins of `add-folder-form.test.tsx:219`, `:376`, `:512`
and `:1001` exist. No Rust file is touched.

**58.2** — `task.lastRun.detail` is rendered on the row as a fifth `Field` beside the outcome
(`tasks-pane.tsx:362`), null-safe, with the never-ran and in-flight states unchanged. A run whose
`detail` is absent — the lease-reclaim path writes `outcome = 'abandoned'` with **no** detail
(`db.rs:3322-3326`, `:3388-3392`) — renders as absence, not as an empty string. No Rust, no schema.

**58.3** — an expandable per-row section calling `syncTaskHistory(task.id)` (`client.ts:6348`),
modelled on `SyncActivityList` (`sync-pane.tsx:1391-1443`) rather than a third list idiom — its
`label-caps` heading, its **null-versus-empty** distinction, its `useFold`/`FoldToggle` truncation,
its unknown-kind fallback. Columns mirror the CLI's already-settled set: outcome word, relative
time, host, detail (`commands.rs:3299-3320`), newest first, with that command's empty state
(*"no runs recorded"*) as the precedent. Relative times render client-side from timestamps against
the pane's existing display clock, never a second one. A failed history read shows the refusal and
keeps whatever the pane last had — the rule Story 57.5's review established: *a failed read is a
fault to report, not a fact to invent* (`sync_ipc.rs:2072-2077`). No Rust, no schema, no new IPC.

**58.4** — `on_missed TEXT NOT NULL DEFAULT 'run_now'` on `tasks`, added by a **newly written**
`ensure_task_columns` on the `ensure_journal_columns` shape (one column at a time; the
`PRAGMA table_info` statement dropped before any `execute` on the same connection,
`db.rs:429-432`), called in `migrate` beside its three siblings (`db.rs:234-236`). The `DEFAULT` is
asserted, not assumed: `upsert_task`'s `INSERT` names its columns (`db.rs:3142-3146`), so a test
proves an older-shaped write still succeeds. The policy is read into `TaskState` (`tasks.rs:277-280`) and
decided in `decide`; `Action` gains the variant that lets *skip* **re-arm** rather than return
`None`, and `run_due_tasks`'s exhaustive match (`engine.rs:2133-2148`) is extended rather than
defaulted. `db::arm_task` is **not** reused for the skip write — it is `WHERE next_due_ms IS NULL`
because *"first sight can only happen once, so the statement says so"* (`db.rs:3256-3260`). `delay`
adds **no column**: lateness is `now_ms - next_due_ms`, and the wait is enforced in `decide`, proven
by a test that a `tasks run` **during** the delay is not silently converted into the delayed run.
An unreadable policy spelling is skipped and listed, not fatal (`db.rs:3113-3128`). Under **every**
setting, a two-hundred-window absence yields **one** run — NFR-44, asserted directly.
**The CLI flag and the form control ship in this same story, and that is a hard acceptance
criterion, not a preference.** A policy writable from neither surface is born unreachable, which is
the exact defect this epic exists to fix: no UI writes tasks today (`tasks-pane.tsx:70-74`) and the
only writer is `keeper-syncd tasks set`. Splitting them produces a column nobody can set from the
app and a story that reports itself done.

**58.5** — a fourth thing a `task_runs` row can say, written as a **closed, zero-duration** row at
the instant a policy declines a window, with `detail` naming the declined instant and the policy
that declined it. No existing `TaskOutcome` is reused, and the story states why against the doc
comments: `Busy` is *"the work could not start because its target was already in use"*, `Deferred`
is *"the work did not run because a condition it waits on was not met"*, `Abandoned` is *"the run was
never closed by the host that started it"* — **all three require a host to have been present**
(`tasks.rs:184-205`). `Deferred` in particular is not reused, because `next_task_window` consumes it
to retry within `TASK_RETRY_MS` (`engine.rs:2295-2301`) — *"try again very soon"*, the opposite of
skip. The new spelling is forward-compatible on read (NFR-43) and appears in `OUTCOME_LABELS`
(`tasks-pane.tsx:199-206`) and in the CLI's outcome column. A test asserts that after a `skip` the
pane's **last run moves**, which is the whole point: before this story a declined window left no row
anywhere, because `task_runs` rows are minted only by `claim_task` (`db.rs:3331`).

**58.6** — one missed window yields one run on a host running **both** `keeper-syncd watch` and the
`Persistent=true` timer (`keeper-syncd-tasks@.timer:70-75`). The mechanism is named in the story:
`TaskTrigger::Requested` sets `due_at_most = None` (`engine.rs:2215`) and bypasses `claim_task`'s
window condition (`db.rs:3303`) while the daemon's tick claims the same past window as `Scheduled`.
The test drives **both trigger kinds against one overdue window** — not two connections against one
lease, which is 57.2's test and proves something else. A deliberate manual `tasks run` on a task
that is **not** overdue keeps working unchanged; the fix may not turn *run it now* into *run it if
due*. `deferred-work.md:5036-5042` is closed with the reasoning. macOS is unaffected and the story
says so rather than testing a host that does not exist there.

**58.7** — a read-only class in ⌘8, visually distinct from tasks, projecting the work this host paces
with its real cadence: the per-profile scan (`engine.rs:2917-2932`, interval
`profile.effective_poll_interval_ms`, `profile/mod.rs:144-150`), the hourly LFS scratch sweep
(`engine.rs:2952`, `SWEEP_EVERY_MS` `:443`) and the notes cadence
(`notes_vault.rs:2551-2566`, `DEFAULT_PUSH_INTERVAL_MS` `profile/mod.rs:202-206`). Each row states
that it is paced rather than scheduled, and the scan row states that **two of its three triggers are
filesystem events** (`engine.rs:3099-3102`) rather than implying a clock owns it. **No schedule
editor and no Run now on any projected row** — asserted as a negative, because those two controls
are the claim *"you can change when this happens"*, which is false. No `tokio::time::interval` is
added anywhere: `src/test/task-host-tick.test.ts` still passes, and the projection rides the pane's
existing display clock. Nothing writes a `tasks` row for projected work — a task row over a still
standing look-gate is a schedule that does not schedule.

**58.8** — `sync_governance`, the twin of `release_governance` (`engine.rs:8165-8221`): every `Sync`
task row for a profile folds into one least-permissive mode over the same explicit rank
(`TaskMode` deliberately derives no `Ord`, and the story keeps the rank spelled where the claim is
made), and the fold **modulates `scan_is_due` for that profile** rather than adding a driver beside
it. The acceptance is the sentence `release_permits` already sets: *"the schedule drives it **and**
the success edge keeps working"* (`engine.rs:8226`) — a `scheduled` sync task must not leave the
15 s pacing running underneath it, and an `off` row must not silently stop a folder the owner never
asked to stop. A test asserts the negative directly: with a scheduled Sync task present, the
ordinary pass does **not** also sync the same folder in the same window, and the run record does not
report a folder as synced by the task that the supervisor would have synced anyway. NFR-45's second
half is the gate: the existing gate is **surrendered**, not raced.

**58.9** — `docs/sync.md` §14 grows the policy and the projection: the three `on_missed` settings in
the owner's own three words, that the default reproduces pre-58 behaviour so an upgrade changes
nothing, that `run_now` is `Persistent=true` semantics in-process, that a declined window is
recorded and what it reads as, the daemon-plus-timer guidance now that 58.6 makes one cadence in two
places safe, and — plainly, once — that the projected class is *paced, not scheduled*, so nobody
looks for a Run now button that is deliberately absent. It states the macOS truth as the code now
does rather than as AD-137's first draft did: the daemon **does** build and ship for Darwin
(`.github/workflows/release.yml:238-243`), and the provable gap is that **no launchd plist exists
anywhere in the tree**, so nothing starts it in the background.
