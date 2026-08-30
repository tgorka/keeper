---
title: 'The app runs them too, and ⌘8 says which host will'
type: 'feature'
created: '2026-08-29'
status: 'done'
baseline_revision: 'fb4d4a7'
final_revision: '6b3a9ad'
review_loop_iteration: 0
followup_review_recommended: true
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
| Linux, unit enabled, same data dir, **lingering on** | shared `sync.db`, `/var/lib/systemd/linger/$USER` present | host `daemon`: "the keeper-syncd unit on this machine runs this, logged in or not" | — |
| Linux, unit enabled, same data dir, **lingering off** | shared `sync.db`, no linger marker | host `daemon`, second sentence: "…runs this while you are logged in — lingering is off, so its schedule stops when your session ends" | — |
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
- [x] `keeper/src/sync_ipc.rs` — the five commands and the row→VM mapping.
- [x] `keeper/src/sync.rs` + `keeper/src/lib.rs` — `finalize_for_quit` on the quit path only, the
      splice, and a source test asserting one interval in the shell and that `CloseRequested`
      reaches neither the stop nor the release. The source test landed in
      [`src/test/task-host-tick.test.ts`](../../src/test/task-host-tick.test.ts) rather than in
      `keeper/src/lib.rs` — see Design Notes, *Where the source test lives*.
- [x] `src/lib/ipc/client.ts` — five wrappers with literal command names.
- [x] `src/hooks/use-tasks-shortcut.ts` (+ test) — ⌘8/Ctrl+8, IME, typing targets, capability gate.
- [x] `src/components/layout/tasks-pane.tsx` (+ test) — the rows, Run now, the unhosted case, the
      unknown row, the macOS sentence; exported copy constants and testids.
- [x] `src/lib/stores/primary-view.ts`, `app-shell.tsx`, `sidebar-pane.tsx`, `actions.ts` — wiring,
      plus a registry↔handler cross-check in `actions.test.ts`: `keeper-core` proves the registry
      carries `tasks-view` and its ⌘8 chip, and that test proves the id has a handler here, which
      is the half no Rust test can see.
- [x] `dev/mock-shell.ts` — scheduled-with-next-due, mid-run-holding-a-lease, failed-last-run,
      unknown-kind and unhosted fixtures, so the whole view is exercisable in a browser on Linux.
      Two more were added — a `manual` row and a switched-off one — because the pane branches on
      five host kinds and the named five left `onRequest` and `off` unexercised.

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

### 2026-08-30 — Review pass 3 (the deferred product decision, decided)
- intent_gap: 0
- bad_spec: 0
- patch: 0
- defer: 0
- reject: 0
- addressed_findings:
  - 2 `[bad_spec, medium]` — **decided and fixed: the probe, not the hedge.** Pass 2 left this one finding open on purpose, because choosing between its two repairs was a product decision coupled to story 57.7's packaging, which did not exist yet. It exists now, so the choice was made against the unit that actually ships (`keeper-syncd-tasks@.timer`: `OnCalendar=daily`, `Persistent=true`, no `[Install]` — a **user** timer, which is exactly the thing lingering governs).

    **The decision.** `HOST_SENTENCE_DAEMON` keeps *"logged in or not"* verbatim, and becomes **true**, because `daemon_presence` now refuses to reach it until a caller has established that the user lingers. A fourth `DaemonPresence` state, `RunsUntilLogout`, carries the box where the unit is enabled and shares the database but lingering was never enabled, with its own sentence: *"the keeper-syncd unit on this machine runs this while you are logged in — lingering is off, so its schedule stops when your session ends"*. Same `TaskHostKind::Daemon`, two sentences — the shape `HOST_SENTENCE_APP_OTHER_DATA_DIR` already established under `TaskHostKind::App`, and the reason no new host kind, no new pane branch and no new `HOST_KIND_LABELS` entry were needed.

    **Why the probe rather than the hedge, argued rather than asserted.** The hedge — one sentence covering both cases — would have bought its honesty by deleting information the person setting a nightly sweep actually needs, and it would have weakened AD-137's Always rule (*every host claim on screen is true*) into *every host claim on screen is vague*. It is only the right answer if the fact cannot be established cheaply and reliably, and the fact can: **`Path::exists("/var/lib/systemd/linger/$USER")` is not a proxy for `loginctl show-user --property=Linger` — it is the same predicate.** logind's `Linger` D-Bus property getter (`src/login/logind-user-dbus.c:172-190`) is one call to `user_check_linger_file`, whose entire body is `access("/var/lib/systemd/linger/<cescape(name)>", F_OK)` (`src/login/logind-user.c:717-737`); `loginctl enable-linger` creates that file with `touch` (mode 0644) inside a directory logind creates mode 0755 (`src/login/logind-dbus.c:1655-1671`), and `disable-linger` unlinks it. So the file is the state of record, not a cache — logind also enumerates the directory at startup (`src/login/logind.c:288-330`) — it is world-readable, so an unprivileged process asking about its own user needs no privilege, and the literal path is hardcoded in all three of those places, unchanged from systemd **v219 (2015)** to `main`. Cost: one `stat`. No subprocess, no new dependency, and the D-Bus round trip the review proposed would have answered from the same byte.

    **Where the change falls.** `keeper-core` (regenerable here): the fourth variant, the new sentence, `LINGER_DIR`, the pure `linger_marker_path`, and `daemon_presence`'s new `DaemonHostFacts` parameter — a named-field struct, because `unit_enabled` and `lingering` are adjacent `bool`s and swapping them turns *"no unit here"* into *"logged in or not"*. `src/lib/ipc/gen/DaemonPresence.ts` was regenerated by the export test on this host, not hand-edited. `keeper` (macOS gate): `daemon_lingering()` — the one `exists()` — and `daemon_presence_here` threading it in. The pure/impure boundary is the one finding 3 already set with `profile_unreadable`: `tasks.rs` does no I/O and only spells the path, the shell stats it.

    **The two names refused.** `Path::join("")` and `join(".")` both yield `/var/lib/systemd/linger` itself, which exists on every systemd box — so an empty or dotted `$USER` would report *lingering* for a user who does not linger. `linger_marker_path` answers `None` for `""`, `.`, `..`, anything containing `/`, and anything containing a NUL, and the refusal is tested. A name holding bytes `cescape` would escape is stored escaped and this lookup misses it, answering `false` — the under-claiming direction, deliberately.

    **On a machine with no systemd at all** the answer is `false` and not an error: `Path::exists` folds every `ENOENT` on the way down into `false`. Measured on the host this was written on — pid 1 is `docker-init`, `/run/systemd/system` and `/var/lib/systemd/linger` are both absent, no `loginctl` or `systemctl` on `PATH` — and covered by `probing_a_user_who_does_not_linger_answers_false_rather_than_failing`. It is also unreachable in production on such a box: `systemctl --user is-enabled` fails there, so `DaemonPresence::Absent` is returned before lingering is consulted.

    **Tests, one per branch of the resulting sentence, asserting the real strings** so a reword cannot silently restore the over-claim: `a_linux_box_whose_enabled_unit_shares_the_data_dir_reads_daemon` (lingering on), `a_linux_box_whose_enabled_unit_does_not_linger_says_the_schedule_stops_at_logout` (lingering off), `a_host_with_no_unit_reads_app_whatever_the_lingering_fact_says` (the non-Linux case, fed the most credit-worthy input this function can take), `the_two_daemon_sentences_differ_and_only_the_lingering_one_claims_post_logout`, plus `an_enabled_unit_that_does_not_linger_runs_only_until_logout`, the `linger_marker_path` pair, and `an_enabled_unit_reading_the_stock_daemon_dir_cannot_see_this_database` re-run over **both** lingering answers (a unit that cannot see the row stays irrelevant however long it lives). `src/test/task-host-tick.test.ts` gains the gate for the half that cannot be compiled here: `daemon_presence_here` must contain `lingering: daemon_lingering()` and must not hardcode either constant, and `daemon_lingering` must reach `linger_marker_path(...).exists()` with no `Command::new` and no `loginctl`.

    **Mutation-proved, both directions.** Making the probe always report lingering (`if facts.lingering` → `if true`) fails `an_enabled_unit_that_does_not_linger_runs_only_until_logout`; restoring the over-claim on the sentence (the `RunsUntilLogout` arm rendering `HOST_SENTENCE_DAEMON`) fails `a_linux_box_whose_enabled_unit_does_not_linger_says_the_schedule_stops_at_logout` and `the_two_daemon_sentences_differ_and_only_the_lingering_one_claims_post_logout`. Both mutations were reverted and the restore verified against `git diff`.

    **Everything that repeated the claim moved with it**, so no two surfaces disagree: this spec's I/O matrix (the one daemon row is now two), `ARCHITECTURE-SCHEDULED-TASKS.md` AD-137 (lingering is stated as the condition it is, and the Decision bullet names both sentences), `docs/decisions.md`'s AD-137 digest, `docs/sync.md` §14 — whose *"the verdict does **not** check lingering"* paragraph was **false** the moment this landed and now states both sentences and how the fact is read — and both unit headers: `keeper-syncd-tasks@.service`'s *"the app's Tasks view does NOT check lingering"* is corrected to what the view now does (and narrowed to the blind spot that remains, which is the timer itself, not lingering), and `keeper-syncd.service`'s weak *"To keep syncing after logout"* aside is promoted to the required step with its verification, matching the two task units. The ledger entry is closed with the decision and its reasoning.

### 2026-08-30 — Review pass 2 (loopback fixes)
- intent_gap: 0
- bad_spec: 0
- patch: 0
- defer: 0
- reject: 0
- addressed_findings:
  - 1 `[bad_spec, high]` — `finalize_for_quit` freed a lease while its run was executing. **Fixed**, and the ordering is the fix. `Engine::finalize_task_leases_for_quit(budget)` (`keeper-sync/src/engine.rs`) now owns the whole quit sequence and `sync::finalize_for_quit` calls it: (1) **settle**, bounded by `TASK_QUIT_SETTLE` (2 s) — a run that ends inside it closes its own row and releases its own lease with the true outcome, which is what `finalize`'s post-loop ordering used to guarantee for free; (2) **release, conditionally** — `db::release_host_leases` gained a `hold` set and no longer NULLs `running_host` for a task this process is still running. That run **is** recorded `TaskOutcome::Abandoned` (quitting mid-run is abandoning it) and its lease expires the ordinary `TASK_LEASE_MS` way, so the daemon on a shared `sync.db` cannot satisfy `claim_task`'s `running_host IS NULL` and start a second concurrent run over a tree an orphaned git child is still writing. In-flight runs are tracked by a `watch::Sender<HashSet<String>>` on the engine, registered after the claim and released by a `Drop` guard (`TaskRunInFlight`), so a `?`, an error or a panic cannot leak an entry. Both properties the review verified as clean stay clean: `release_host_leases` is scoped to `host` in **both** statements, and `upsert_task` still binds the lease columns to NULL on insert and never touches them on conflict.
  - 2 `[bad_spec, medium]` — **re-triaged, not fixed here.** `HOST_SENTENCE_DAEMON`'s "logged in or not" over-claims on a box where the unit is enabled but `loginctl enable-linger` was never run. The finding is correct and the two repairs it names are both coherent, but choosing between them changes a sentence the intent contract's matrix quotes verbatim **and** must agree with what story 57.7 installs — and 57.7's packaging landed in this same worktree while this pass ran. Fixing it inside this pass would have meant one agent deciding a product question across two stories' artifacts with no owner reachable to arbitrate. Recorded in `deferred-work.md` as a live over-claim with both repairs and the 57.7 coupling spelled out, so the decision is made once, by the owner, against the unit that actually ships. Nothing else in this pass depends on it: `daemon_presence` and every other sentence are untouched.
  - 3 `[patch, medium]` — a failed or skipped profile read rendered every folder-scoped task as folder-gone. **Fixed at both routes.** The swallows are gone: `sync_tasks` and `sync_task_save` now propagate `list_profiles` and `task_history` with `?` like their twelve sibling call sites. And the non-transient route — `db::list_profiles` silently skipping a row it cannot deserialize — is now answerable: `db::unreadable_profile_ids` names the ids it dropped, `TaskHostFacts` carries `profile_unreadable`, and `task_host` answers the new `UNHOSTED_FOLDER_UNREADABLE` instead of `UNHOSTED_FOLDER_GONE`. Still *unhosted*, truthfully — `run_due_tasks` reads the same skipped list — but the sentence now names a fault to fix rather than a folder to forget.
  - 4 `[patch, medium]` — `task_faults` was cleared only by `TaskOutcome::Ok`, so a task coming back into service kept a stale fault and its next failure was silent for the process's life. **Fixed at the root rather than at the symptom.** `db::upsert_task` now decides its three "back into service" edges **in Rust**, binds that one answer into the statement, and returns it as `db::TaskSave { Created, Updated, Rearmed }`; `Engine::save_task` clears the fault on `Created | Rearmed` and `Engine::forget_task` clears it too. One rule, one place, two consumers — the previous SQL `CASE` was correct but private to one statement, which is exactly why the fault state could not learn about the same edges. The pre-read and the write now share a transaction.
  - 5 `[patch, medium]` — the ⌘8 empty state named `keeper-syncd task add`, which does not exist. **Fixed, and the finding's own platform premise was corrected in the process.** The copy is now three constants: what the view is ("it cannot create one yet" — true; no control here calls `sync_task_save`), the real command `keeper-syncd tasks set nightly --kind sync --schedule "0 3 * * *"`, and what happens next. The review's "the binary does not ship on macOS at all" is **inaccurate**: `release.yml:229-282` builds and publishes `keeper-syncd-aarch64-apple-darwin` with a checksum. The true, narrower fact — no launchd plist exists anywhere in the tree, so nothing *starts* it in the background on a Mac — is what the copy and `daemon_presence_here`'s doc now say. So no platform branch is needed and none was added: the command is true to read on either platform, the promise is only "keeper runs a due task while keeper is running", and each row still states its real host. `sync_ipc.rs`'s `daemon_presence_here` doc previously drew the right conclusion from a half-stated premise and now states both halves.
  - 6 `[patch, low]` — the pane's clock froze at the last read. **Fixed:** a 30 s `setInterval` updating only `now`, cleared on unmount, plus the constant `TASKS_CLOCK_TICK_MS`. The test comment that asserted the property in prose is corrected and the property is now driven with fake timers, including an assertion that `syncTasks` was **not** called again — a display clock, not a second poller. Untouched by AD-62, which is scoped to the `keeper` crate.
  - 7 `[patch, low]` — the Run now in-flight guard was one shared slot. **Fixed:** `Record<string, true>` keyed by id, deleting only the id that settled. `Record` rather than `Set` to match `refusals` beside it and the project's `ts-set-map` rule.
  - 8 `[patch, low]` — listing reads were unsequenced. **Fixed:** an incrementing token in a `useRef`, applied only when it is still the latest, on **both** the success and the failure branch.
  - 9 `[patch, low]` — a Run now refusal was never cleared by a re-read. **Fixed, with one correction to the finding.** "Clear on every successful `setListing`" as written would erase the refusal in the tick it appeared, because `runNow`'s own settle issues a read — destroying the pane's whole answer to a refused Run now and an acceptance criterion of this story (the pre-existing test caught it). The rule implemented is the one the finding meant: a read *later* than the attempt clears; the attempt's own contemporaneous re-read does not (`refresh(keepRefusals)`).
  - 10 `[patch, low]` — unknown rows were keyed on an id `db::list_tasks` does not guarantee non-empty. **Fixed:** keyed on `${index}:${row.id}`, and an empty id renders `TASKS_UNKNOWN_NO_ID_TEXT` rather than a blank span. Tested with two rows whose id is `""`, asserting both distinct reasons render and that React logged no duplicate-key error.
  - 11 `[patch, low]` — the daemon probe blocked a tokio worker with an unbounded subprocess. **Fixed:** `daemon_presence_probe` runs the whole thing under `tauri::async_runtime::spawn_blocking` with a 5 s outer budget, and `daemon_unit_enabled` replaces `.output()` with a spawn plus a polled 2 s deadline that kills and reaps the child. Every failure direction reads `Absent`, which the function already documents as the safe one.
  - 12 `[patch, low]` — three `@daily 03:00` fixtures the parser refuses. **Fixed** to `0 3 * * *`, `30 4 * * *`, `0 2 * * *`. The dialect was **not** extended: `Never` in this contract forbids changing it, and a time-of-day alias is a real feature for a wave that owns the dialect. Guarded by a keeper-sync test that extracts every `schedule: "…"` literal from `dev/mock-shell.ts` and feeds it through `TaskSchedule::parse` — the engine's own parser judging the harness's own text, so no second list can go stale. It asserts the extraction found at least five literals first, so a renamed field cannot make it vacuous.
  - 13 `[patch, low]` — the Design Notes' `task_host` pseudocode contradicted the code, and gate 6's doc mis-stated NFR-43. **Both fixed.** The block now gates on `mode == "scheduled"` and ends `unhosted(UNKNOWN_MODE)`, matching the implementation the prose gate list already agreed with. `UNHOSTED_UNKNOWN_MODE`'s doc now states plainly that the state is **unreachable in production** — `db::decode_task` files an unreadable mode under `TaskListing.unknown` before `task_host` is reached — and that the branch is kept as a total function against a future in-crate caller, which is what its unit test constructs.
  - 14 `[defer → patch, medium]` — **re-triaged and fixed.** `db::upsert_task`'s forward-compatibility guard read `kind` only, so a save over a row whose `mode` a newer keeper wrote silently rewrote it. Deferred by the review as pre-existing; fixed here because finding 4's repair makes `upsert_task` read the stored `mode` anyway, so the guard became five lines beside its own twin. Leaving a known silent-rewrite defect in a ledger when its fix is free next to the code that already reads the column is the worse trade. The ledger entry is closed with the resolution.

**Run, and clean.** Every finding is either fixed in the diff or re-triaged above with its reason; one (finding 2) is deliberately left as a product decision coupled to story 57.7 and is recorded in the ledger rather than guessed at. `followup_review_recommended` is set to **`true`**, and not for the absence of a review: finding 2 is a live over-claim in shipped copy awaiting an owner decision, findings 1, 3, 5 and 11 changed behaviour in `keeper/src/{sync,lib,sync_ipc}.rs` — the crate that **cannot be compiled on this host** — and the fix for finding 9 had to correct the finding itself, which is the shape of thing a second pass should re-read.

**Shell-crate symbols for the macOS gate.** `bun run check:rust:macos` is the first real type check for: `sync::finalize_for_quit` (now `block_on(engine.finalize_task_leases_for_quit(keeper_sync::TASK_QUIT_SETTLE))`, `held: HashSet<String>`); `sync_ipc::task_vm` (new `unreadable_profiles: &[String]` parameter, `TaskHostFacts.profile_unreadable`); `sync_ipc::daemon_presence_probe` (new `async fn`, `tauri::async_runtime::spawn_blocking`, `DAEMON_PROBE_BUDGET`); `sync_ipc::daemon_unit_enabled` (rewritten, `DAEMON_PROBE_TIMEOUT`, `std::io::Read`); and the two commands' `?` propagation. `cargo fmt --check` parsed all of it, `keeper-sync`/`keeper-core` compile and their tests pass here, and `src/test/task-host-tick.test.ts` now scans `sync_ipc.rs` as well as `lib.rs`/`sync.rs` so the ordering, the propagation and the off-runtime probe are asserted where a check actually runs.

### 2026-08-30 — Review pass
- intent_gap: 0
- bad_spec: 2: (high 1, medium 1, low 0)
- patch: 11: (high 0, medium 3, low 8)
- defer: 1: (high 0, medium 1, low 0)
- reject: 0
- addressed_findings:
  - none

**Run, and not clean.** Step-04's Blind Hunter (`bmad-review-adversarial-general`) and Edge Case
Hunter (`bmad-review-edge-case-hunter`) were run in parallel, without prior conversation context, on
`git diff 8e4f933~1..6b3a9ad` — the three implementation commits, i.e. exactly `baseline_revision..`
minus the frontmatter stamp. 17 raw findings deduplicated to 14; severity below is assigned by
consequence for the app's user, not by either reviewer. **Nothing was fixed in this pass** — the
entry is a hand-off, and `addressed_findings` says `none` because that is the truth. The two
`bad_spec` findings are what stop this being a clean pass; per step-04's cascade the `patch`
findings are moot until they are resolved, and they are recorded here anyway so no evidence is lost.

**bad_spec (2)**

1. `[high]` **`finalize_for_quit` frees a task lease while that task's run is still executing.**
   `keeper/src/sync.rs:492-497`, reached from `keeper/src/lib.rs`'s `RunEvent::ExitRequested`.
   `stop_supervisor()` only `send(true)`s on the watch channel and returns; as this spec's own
   Design Notes observe, `self.tick().await` runs inside the `select!` *branch*, so a supervisor
   already inside `claim_and_run` cannot observe the signal. `release_task_leases()` is then called
   one line later on the quitting thread, and `db::release_host_leases` (`db.rs:3277-3291`) is
   unconditional for this host: it closes every `task_runs` row with `finished_ms IS NULL` as
   `Abandoned` and NULLs `running_host`/`lease_until_ms`. The process does not exit at that point —
   `lib.rs` then runs `block_on(timeout(3s, shutdown_all()))` — and the git child already spawned is
   a `std::process::Command` with no `kill_on_drop`, so it is orphaned and keeps writing the
   worktree. On the `DaemonPresence::Runs` configuration this story renders as a first-class host,
   `next_due_ms` has not been advanced yet (`finish_task_run` writes it), so the daemon's next 1 Hz
   tick satisfies `claim_task`'s `running_host IS NULL AND next_due_ms <= now` and starts a second
   concurrent run of the same task over the same git working tree — the serialization `claim_task`
   exists to make structurally impossible ("exactly one may pass"). A `release`-kind task deletes
   local content, so this is a content-risk window, not only a noisy one. Root cause is in **Design
   Notes**, *"The quit path was a race, and the fix is not only a join"*: `"idempotent, exactly what
   `finalize` does"` is true of the statement and false of the ordering — `Engine::finalize` is
   reached only after `run`'s loop has broken, which is the guarantee the move drops. The
   intent-contract matrix row ("the app quits → this host's task leases are handed back before
   exit") stays valid; what must be amended is the Design Notes' mechanism, to release only after
   the supervisor has acknowledged the stop, bounded by the quit budget the path already spends.
   Not covered by any test: `releasing_this_hosts_leases_lets_another_host_claim_the_task` claims a
   lease with no work in flight and then proves another host can take it — it demonstrates the
   hazard rather than excluding it; `src/test/task-host-tick.test.ts` is a source scanner and
   asserts only which call sites reach the verb.

2. `[medium]` **`HOST_SENTENCE_DAEMON` promises post-logout execution that `systemctl --user
   is-enabled` cannot establish.** `keeper-core/src/tasks.rs:64-65`, fed by
   `keeper/src/sync_ipc.rs:1860-1869`. A systemd **user** unit runs only while that user's systemd
   manager is up, and the manager is torn down at logout unless `loginctl enable-linger` has been
   run — a separate, non-default fact that `is-enabled` does not report and this code never asks
   about. AD-137 itself names it: `ARCHITECTURE-SCHEDULED-TASKS.md:117-119`, *"the systemd **user**
   unit … **with `loginctl enable-linger` for post-logout**"*. So on a box where the unit is enabled
   and shares the data directory but lingering was never enabled, the row asserts "runs this,
   **logged in or not**" while the nightly sweep in fact stops at logout — an over-claim of exactly
   the class this module's own doc calls the unsafe direction. Root cause is outside the
   intent contract in the sense that matters — the Always rule *"Every host claim on screen is
   true"* is unambiguous — but the fix cannot be a bare patch, because the matrix row quotes
   "logged in or not" verbatim: amending it is a spec edit. Two coherent repairs, and the choice is
   a product decision: probe `loginctl show-user "$USER" --property=Linger` and add a presence state
   for enabled-but-not-lingering, or weaken the sentence to what `is-enabled` actually establishes
   ("including while keeper is closed"). `grep -rn linger` finds the fact only in the architecture
   document and the packaging notes, never in code. **Unreachable on macOS**, where
   `daemon_presence_here` is `Absent` by construction. Note the coupling: story 57.7 owns the unit
   and `docs/sync.md` §13, so whichever repair is chosen must agree with what 57.7 installs.

**patch (11)** — none applied; each is described to be fixable without re-reading the diff.

3. `[medium]` **A failed profile read renders every folder-scoped task as `unhosted`.**
   `sync_ipc.rs:1929-1932` (`sync_tasks`), same shape at `:2082-2085` (`sync_task_save`).
   `engine.list_profiles().map_err(|err| sync_ipc_error(&err)).unwrap_or_default()` — the `map_err`
   builds an `IpcError` that is then discarded by `unwrap_or_default()`, unlogged and unbound, one
   line below a sibling call that propagates the same fault class with `?`. `task_vm` resolves
   `profile` by searching that vector, so an empty vector makes `task_host` gate 2
   (`profile_id.is_some() && profile.is_none()`) answer `unhosted(UNHOSTED_FOLDER_GONE)` for every
   scoped task at once, and the pane paints it `variant="destructive"` with "nothing will run this /
   it names a folder keeper does not sync". A second, non-transient route reaches the same lie:
   `db::list_profiles` (`db.rs:1533-1538`) *silently skips* a profile row it cannot deserialize
   ("skipping unreadable sync profile row"), so a task scoped to a healthy, actively-syncing folder
   whose row a newer keeper wrote renders red and wrong. This is the one thing AD-137 exists to
   prevent, produced by the projection rather than by the pure function — and `task_vm`'s own doc
   forbids the outcome for the adjacent case ("reporting it as *unhosted* would tell the user their
   configuration is broken when they had merely pressed pause"). **Fix:** propagate with `?` like
   the twelve sibling call sites in the same file, and keep "this id is in no profile list" distinct
   from "the profile list could not be established" so the second never renders as folder-gone. The
   identical swallow at `:1946-1949` (`engine.task_history(&row.id, 1).ok()`) turns a failed history
   read into `lastRun: null`, which the pane renders as "never run" for a task that has run — same
   fix, same lines.

4. `[medium]` **`task_faults` is only ever cleared by `TaskOutcome::Ok`, so a task that comes back
   into service keeps a stale fault and its next failure is silent.** `engine.rs:891`, `:1879-1893`;
   `Engine::forget_task` at `:7687-7689`. `grep task_faults engine.rs` returns only the declaration,
   the constructor and the two arms of `note_task_outcome`. Reaching sequence, entirely through
   verbs this story ships: task `nightly` fails (fault inserted, one toast — correct); the user
   forgets it (`sync_task_forget`, now a registered IPC command, or `keeper-syncd tasks forget`);
   the user re-creates the same id (`sync_task_save` uses a non-blank `req.id` verbatim — the
   documented edit-rather-than-duplicate path, and the CLI takes the id positionally); the new task
   fails. `insert` returns `false`, `onset` is `None`, and there is neither a notification nor the
   `tracing::warn!` line — a brand-new record's first failure is silent for the whole process
   lifetime. That is the invisible-failure shape this state was added to close. **The hole is wider
   than forget:** `db::upsert_task` already treats disabled→enabled and a schedule change as "a task
   coming back into service arms afresh" (`db.rs:3021-3030`) and clears `next_due_ms` for exactly
   this reason, but nothing clears the *fault* on those edges either — so disable-a-broken-task,
   fix-the-remote, re-enable, break-again is also silent. Secondary: the set grows for the process
   lifetime with ids of tasks that no longer exist. **Fix:** clear the id from `task_faults` in
   `forget_task`, and on the same three edges `upsert_task` already recognises. Not covered:
   `a_failing_task_notifies_once_per_onset_and_re_arms_only_on_a_success` drives the outcome variants
   and a second task id but never forgets, disables or re-creates one, so the only re-arm it
   exercises is the `Ok` one.

5. `[medium]` **The empty state names a CLI verb that does not exist, on a platform where the binary
   does not ship.** `tasks-pane.tsx:56-58` (`TASKS_PANE_EMPTY_SENTENCE`), rendered at `:416`:
   "`keeper-syncd task add` creates one". The daemon's clap tree has the group `tasks` — plural —
   (`keeper-syncd/src/commands.rs:445`, `:502`) with `list`, `status`, `run`, `set`, `enable`,
   `disable`, `forget`. There is no `task` group and no `add` verb; creation is `tasks set`. A user
   following it verbatim gets `ErrorKind::InvalidSubcommand`. This is load-bearing rather than
   cosmetic: nothing in this epic creates a task row on migration, on open or on first tick, so
   **every existing install opens ⌘8 to this sentence and nothing else**. Compounding it,
   `keeper-syncd` is Linux-first and unix-only and there is no launchd plist or macOS packaging
   anywhere in the tree, so on macOS — which this story explicitly targets — the only guidance the
   pane offers is to run a binary that cannot exist there. **Fix:** `keeper-syncd tasks set`, and a
   sentence that does not make that the sole affordance on a platform without the daemon.

6. `[low]` **The pane's clock freezes at the last read, and a test comment claims it does not.**
   `tasks-pane.tsx:336-352`. `now` is initialised at mount and written only in `refresh()`'s success
   branch; the single `useEffect` has no interval, no visibility listener and no subscription. Every
   relative string — `formatTaskDue(task.nextDueMs, now)`, `formatTaskAgo(...)` — is therefore
   measured from the instant the last read landed, and re-renders do not move it: a row that said
   "in 5 min" still says "in 5 min" an hour later, never reaches `due now`, and never shows the run
   that happened. `tasks-pane.test.tsx:282-285` states the intended property in a comment — *"a pane
   left open does not show a figure that froze when the read landed"* — but the test body only calls
   `formatTaskDue(NOW + 5 * 60_000, NOW)` with an explicit `now`, so the component's `now` is never
   advanced by any test and the stated property is false of the component. **Fix:** a coarse
   `setInterval` updating only `now` (the copy is minute-grained), cleared on unmount, and correct
   the test comment or assert the property. This does not touch the no-second-clock rule, which is
   scoped to the `keeper` crate.

7. `[low]` **The Run now in-flight guard is one shared slot, so it re-enables the wrong row.**
   `tasks-pane.tsx:329`, `:355-374`, `:426`. `runningId` is a single `string | null` and the button
   is `disabled={runningId === task.id}`; `runNow` sets it on entry and unconditionally clears it in
   `finally`. Click Run now on slow task A, then on B: `runningId` becomes `"B"`, re-enabling A while
   A is still in flight; A then settles and its `finally` clears the slot, re-enabling B while B is
   still in flight. A further click issues a second `syncTaskRunNow` for a task that already holds a
   lease, `run_task_now` answers `SyncError::Busy`, and the pane paints "somebody else is doing
   this" on a task the same user just started from this same pane. **Fix:** hold in-flight ids in a
   `Set<string>`, deleting only the id that settled. Not covered: both Run now tests render a
   listing with exactly one row.

8. `[low]` **Listing reads are unsequenced, so a stale response can overwrite a newer one.**
   `tasks-pane.tsx:338-347`. `refresh` has no request token, no abort and no in-flight guard, and
   three independent triggers: the mount effect, the Refresh button, and `runNow`'s `finally`. Click
   Refresh, then Run now before it resolves: if the pre-run `syncTasks` response lands after the
   post-run one, `setListing` is called last with the pre-run listing and the row shows "never run"
   immediately after a run that happened — the exact failure the `runNow` re-read exists to prevent.
   `setNow` is overwritten with the stale read's clock too, shifting every relative time backwards.
   **Fix:** an incrementing token in a ref, applied only when it is still the latest. Not covered:
   the re-read test asserts `syncTasks` call *count* against one `mockResolvedValue`, so ordering is
   unobservable.

9. `[low]` **A Run now refusal is never cleared by a re-read, so it can contradict the row above
   it.** `tasks-pane.tsx:328`, `:356-361`, `:425`. `refusals` is cleared at exactly one point — the
   top of `runNow`, for the one id being run. `refresh` never touches it. The daemon holds
   `nightly`'s lease, Run now is refused as busy, the daemon's run finishes, the user presses
   Refresh: `runningHost` is now null and `lastRun` shows the completed run, while the
   `role="alert"` still asserts the task is busy elsewhere, clearable only by running it again. Same
   shape for a "task is off" refusal after the task is enabled from the CLI. **Fix:** clear the
   `refusals` map on a successful `setListing` — the listing that follows an attempt is newer
   evidence than the attempt.

10. `[low]` **Unknown rows are keyed on an id that is not guaranteed non-empty.**
    `tasks-pane.tsx:438-441` uses `key={row.id}`, but `db::list_tasks` (`db.rs:3105-3115`) emits
    `UnknownTask { id: String::new(), reason: format!("unreadable task row: {err}") }` for a row
    whose `id` column will not read — and `db.rs:6077` asserts that shape. Two such rows give React
    two siblings keyed `""`: a duplicate-key warning, and reconciliation that can reuse one row's
    DOM for the other so the two distinct `reason` sentences swap or fail to update. Only the
    unknown list has this hole, and it is the list that exists to tolerate malformed rows
    (`validate_id` keeps `listing.tasks` safe). **Fix:** fall back to the array index for the key,
    and render a placeholder where the id is empty rather than an empty span.

11. `[low]` **The daemon probe blocks a tokio worker with an unbounded subprocess.**
    `sync_ipc.rs:1860-1869` and `:1885-1906`, called from `sync_tasks` `:1941` and `sync_task_save`
    `:2097`. Both commands are `async fn`, so their bodies run on a runtime worker; inside them
    `daemon_unit_enabled()` does `std::process::Command::new("systemctl").…output()` — a blocking
    fork/exec/wait with no deadline — plus two `std::fs::canonicalize` calls, with no
    `spawn_blocking`. The probe is deliberately uncached ("Probed per listing rather than cached"),
    so every Refresh and every save pays it. `let Ok(output) = … else { return false }` catches a
    spawn failure, never a slow one, and this file's own `sync_footprint` states the rule being
    broken: blocking work on the async runtime "would stall every other command sharing the thread".
    **Fix:** `tauri::async_runtime::spawn_blocking` and a wait deadline falling back to `Absent`,
    which is the safe direction the function already documents. This is a defect in *where* the
    probe runs — visible in the source — not a restatement of the residual risk that its answer is
    untested.

12. `[low]` **Three mock fixtures carry a schedule the real parser refuses.** `dev/mock-shell.ts`
    `:1682`, `:1748`, `:1766` use `"@daily 03:00"`, `"@daily 04:30"`, `"@daily 02:00"`.
    `TaskSchedule::parse` (`keeper-sync/src/tasks.rs:345-355`) strips `@` and matches the *entire*
    remainder against `"hourly" | "daily" | "weekly"`, returning `malformed()` otherwise — so no
    such row can exist in a real `sync.db`, and the dialect's way to say 03:00 daily is `0 3 * * *`.
    The fixture block's own header promises that "what a browser on Linux shows is what the real
    command would answer", and it is the only place a developer on this host can see the surface at
    all, so as written it teaches a syntax that does not exist. **Fix:** `0 3 * * *`, `30 4 * * *`,
    `0 2 * * *`.

13. `[low]` **This spec's own `task_host` pseudocode contradicts the implementation it documents.**
    Design Notes, the ````rust` block: it reads `if t.mode == "manual" { … } if t.schedule.is_none()
    { … } match daemon { … }`, so any mode that is neither `off` nor `manual` falls through to a
    *hosted* verdict when a schedule is present. `tasks.rs:397-409` instead gates on
    `if facts.mode == MODE_SCHEDULED { … }` and ends `unhosted(UNHOSTED_UNKNOWN_MODE)`. The two
    disagree on `mode="teleport"` with a schedule (spec: "keeper runs this" — the over-claim AD-137
    forbids; code: `unhosted`) and without one (spec: NO_SCHEDULE; code: UNKNOWN_MODE). **The code
    is the correct half**, and this spec's own prose gate list ("6. Any other mode spelling is
    unhosted") agrees with it — the contradiction is internal to the Design Notes, and a future edit
    made against the block would reintroduce the over-claim. **Fix:** make the block read
    `if t.mode == "scheduled" { … }` / `return unhosted(UNKNOWN_MODE);`. Related, same finding:
    gate 6's doc comment in `tasks.rs:385-386` calls it "NFR-43's tolerance applied to a row a newer
    keeper wrote", but `db::decode_task` (`db.rs:2965-2967`) files an unreadable `mode` under
    `TaskListing.unknown` before `task_host` is ever reached, so no production caller can pass one.
    The branch is sound as a defence against a future in-crate caller; the doc should say that,
    because the NFR-43 path belongs to `list_tasks` and the pane's "Written by a newer keeper"
    section.

**defer (1)**

14. `[medium]` `db::upsert_task`'s forward-compatibility guard reads the stored `kind` only, so a
    save over a row whose `mode` a newer keeper wrote silently rewrites that mode. `upsert_task` is
    untouched by this diff and the identical kind-only guard is present at `8e4f933~1`; what 57.5
    changes is reachability — it registers an arbitrary-id write verb and puts unknown rows' ids on
    screen. Recorded in `deferred-work.md`.

**Two further ledger entries, deliberately NOT counted above.** While this pass ran, story 57.7's
implementation surfaced two prose defects on Epic 57's own artifacts, and they are appended to
`deferred-work.md` in the same sweep because that ledger is shared rather than per-pass. They are
excluded from the `defer` count so it keeps meaning "what *this* review pass found": (a) AD-136's
*"the unit file that ships is a thin caller of `tasks run`, not the source of truth"*
(`ARCHITECTURE-SCHEDULED-TASKS.md:111`) is contradicted by the shipped timer — `run_task_now` uses
`TaskTrigger::Requested`, which maps to `due_at_most: None` (`engine.rs:2126-2133`) and drops
`claim_task`'s window predicate, so `tasks run` never reads the task's schedule and `OnCalendar` is
the cadence for that driver; (b) `tasks run`'s clap doc (`commands.rs:530`) promises "the schedule
is deliberately not moved" without the exception `next_task_window` implements — an *already-open*
window is consumed and re-armed (`engine.rs:2213-2219`), which the engine's own doc states and the
CLI's does not. Both are pre-existing and neither is a behavioural defect; the code is right in each
case and the prose is what needs to catch up.

**reject: 0.** Both reviewers were held to naming a reaching input, so nothing came back that was
style preference, speculation, or a restatement of the recorded residual risks. Two candidates I
considered myself and reject on the merits: Run now being offered on an `off` or `unhosted` row is
the intent contract's own matrix rows 15–16 (the refusal is the designed answer), and `task_faults`
not surviving a restart — so a still-failing task notifies once more after a relaunch — is stated
and reasoned in the field's own doc comment as a property of *this host's session*.

**The area the previous entry pointed at.** It named the Linux `systemctl`/data-directory probe and
`sync_task_save`'s handling of a row a newer keeper wrote. Both were real: the probe yielded findings
2 and 11, and the newer-keeper save yielded finding 14. The data-directory comparison itself is
sound — `daemon_data_dir` applies the XDG rule including the empty-and-relative refusals, and
`daemon_presence_here` canonicalizes both sides before a comparison `keeper_core::tasks`
deliberately does not do any I/O for.

## Auto Run Result

Status: done

**What was implemented.** The desktop app is now an honest task host and says so. It already ran
due tasks — `Engine::run`'s supervisor tick calls `run_due_tasks`, and `lib.rs` starts that
supervisor at boot under `#[cfg(desktop)]` — so nothing new schedules anything; what this story adds
is the lease handback on quit, the once-per-onset failure notification, five IPC verbs, and a Tasks
view at ⌘8 whose every row states the host that will actually run it.

**Files changed**
- `src-tauri/crates/keeper-core/src/tasks.rs` — **new.** Nine wire types plus `daemon_presence` and
  `task_host`, AD-137's pure decision, with the host matrix unit-tested exhaustively. In
  `keeper-core` so `cargo test -p keeper-core` regenerates every ts-rs binding on Linux.
- `src-tauri/crates/keeper-core/src/lib.rs` — `pub mod tasks;`.
- `src-tauri/crates/keeper-core/src/palette.rs` — the gated `Tasks` category and the `tasks-view`
  ⌘8 action, plus tests that the section is present iff the gate is on and that ⌘8 has one owner.
- `src-tauri/crates/keeper-sync/src/engine.rs` — `task_faults`, `note_task_outcome` (called from
  `claim_and_run`), `pub release_task_leases`, and three tests.
- `src-tauri/crates/keeper/src/sync_ipc.rs` — the five commands, the row→VM mapping, and the Linux
  `systemctl --user is-enabled` + data-directory probe that feeds `daemon_presence`.
- `src-tauri/crates/keeper/src/sync.rs` — `finalize_for_quit()`.
- `src-tauri/crates/keeper/src/lib.rs` — the five commands in the desktop splice; `ExitRequested`
  calls `finalize_for_quit` instead of `stop_supervisor`.
- `src/test/task-host-tick.test.ts` — **new.** The source scan: one interval in the shell, the quit
  path releases, the window-close path does not.
- `src/lib/ipc/client.ts` — five wrappers and eight type re-exports.
- `src/hooks/use-tasks-shortcut.ts` (+ test) — **new.** ⌘8/Ctrl+8 with the IME, Alt, typing-target
  and capability guards.
- `src/components/layout/tasks-pane.tsx` (+ test) — **new.** The rows, the formatters, the copy.
- `src/lib/stores/primary-view.ts`, `app-shell.tsx`, `sidebar-pane.tsx`, `actions.ts` (+ test) — the
  wiring and the registry↔handler cross-check.
- `dev/mock-shell.ts` — seven fixtures covering every branch the pane has.

**Verification**
- `cargo test -p keeper-sync -p keeper-core -p keeper-syncd`: **3736 passed, 0 failed** (baseline
  3704).
- `cargo clippy … -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings`:
  clean (one pre-existing future-compat note about `proc-macro-error2`).
- `cargo fmt --all --check`: no diff — which is also the only local syntax gate the shell crate has.
- `bun run typecheck`: clean. `bun run lint`: 4 warnings + 1 info, the baseline. `bun run test`:
  **300 files / 4966 tests passed** (baseline 297 / 4938).
- `src/lib/ipc/gen/` was re-emitted by the export test and is byte-identical, so the eight bindings
  match their Rust doc comments and nothing there was hand-written.
- Seventeen mutations were applied and reverted one at a time; each killed its owning test, and
  every restore was verified by `cmp` against a pristine copy plus `git diff`.

**Residual risks**
- The `keeper` shell crate cannot be compiled on this host (`gobject-sys`). Every symbol it gained
  was read back against its call site and `cargo fmt --check` parsed it, but `bun run check:rust:macos`
  is the first real type check.
- `daemon_unit_enabled` and `daemon_data_dir` are `#[cfg(target_os = "linux")]` and are exercised by
  no test: they spawn `systemctl` and read the environment. The failure direction is safe by
  construction (anything unexpected reads as *no daemon*, which under-claims rather than
  over-claims), and `daemon_presence` itself — the decision they feed — is fully unit-tested in
  `keeper-core`.
- Step-04's adversarial review pass has now been run out-of-band (see the triage log above) and it
  was **not clean**: 2 `bad_spec` and 11 `patch` findings, none of them fixed in that pass. The
  highest is that `finalize_for_quit` frees a task lease while that task's run is still executing.
  Nothing in this section's *Verification* is affected — every gate still passes — but this story's
  code is not finished, and `followup_review_recommended` stays `true` on that basis rather than on
  the absence of a review.

## Design Notes

**Where the source test lives, and why not in `keeper/src/lib.rs`.** The Code Map put the
no-second-clock scan in the shell crate. It is in
[`src/test/task-host-tick.test.ts`](../../src/test/task-host-tick.test.ts) instead, for the reason
`src/test/command-registration.test.ts` states in its own header: the `keeper` crate does not
compile on Linux (AD-55, AD-56), so a `#[cfg(test)]` assertion there runs only on the macOS gate —
prose on the machine most of this epic was written on, and on CI. Both facts the test guards are
about *source text* (how many intervals exist; which arm reaches the release), so a scanner is the
honest shape either way, and this repository already has three of them
(`tray-notes-labels`, `capture-capability`, `file-scheme-registration`). One rule, one place: it is
not also duplicated in Rust.

**The app was already a host; this story makes it an honest one.** `Engine::tick` calls
`run_due_tasks` (engine.rs:1904) and `lib.rs:600-604` starts that supervisor under `#[cfg(desktop)]`.
Adding a poll to the shell's 1 Hz tray tick would be the *second* scheduler over one git repository
that AD-62 forbids by name. So "the app runs due tasks on the tick it already owns" is satisfied by
the tick it already owns — asserted, not re-implemented.

**The quit path was a race, and the fix is an ordering.** `stop_supervisor` signals and returns; the
supervisor's `JoinHandle` was dropped at spawn (sync.rs:445-451), and `Engine::run`'s post-loop
`finalize()` → `release_task_leases` therefore raced process exit and usually lost. Worse,
`self.tick().await` runs inside the select *branch*, so a supervisor mid-tick cannot even observe
the signal.

The first repair — signal, then release directly through the `Arc` the quit thread holds — was
worse than the race it replaced, and review pass 1's finding 1 is why. *"Idempotent, exactly what
`finalize` does"* was true of the statement and false of the ordering: `finalize` is reached only
**after** `run`'s loop has broken, and that is the guarantee the move dropped. The process does not
exit at that point either (`lib.rs` then spends up to 3 s in `block_on(timeout(3s, shutdown_all()))`)
and a spawned git child is a `std::process::Command` with no `kill_on_drop` — so freeing the lease
let the daemon's next tick satisfy `claim_task`'s `running_host IS NULL AND next_due_ms <= now` and
start a **second concurrent run** over the same working tree. For a `release`-kind task that is a
content-risk window.

So `Engine::finalize_task_leases_for_quit(budget)` owns the sequence and the quit thread calls only
that: **settle** in-flight runs, bounded by `TASK_QUIT_SETTLE` (a run that ends inside it releases
its own lease with the true outcome, restoring `finalize`'s ordering without awaiting a handle
nobody kept), then **release conditionally** — every lease this process holds *except* the ones
whose run is still executing. Those are recorded `TaskOutcome::Abandoned`, because quitting mid-run
is abandoning the attempt, and their leases expire the ordinary `TASK_LEASE_MS` way. The asymmetry
decides it: an unreleased lease costs a delay, a released one costs the serialization `claim_task`
exists for. In-flight ids live in a `watch::Sender<HashSet<String>>` on the engine so the settle
awaits a drain rather than polling, and a `Drop` guard removes an id however the run ends.

**A task's onset is not a profile's.** `Engine::warn` keys on `SyncStatus.warning`, which is
per-profile — and a task may be host-wide, so it has no profile to be sticky on. `task_faults:
Mutex<HashSet<String>>` keyed by task id is the same rule in the same shape: insert-and-notify on
the `absent → present` edge, remove on `Ok`, and leave `Busy`/`Deferred` alone because a run that
did not happen is neither a failure nor a recovery.

**...and a task coming back into service arms the alarm afresh too** (review pass 1, finding 4).
`Ok` alone was not enough: `forget` + re-create, and disable → fix → enable, both left a stale
fault and made the returning task's first failure silent for the process's life. `db::upsert_task`
already recognised those edges for the *window*; it now decides them in Rust, returns
`db::TaskSave::{Created, Updated, Rearmed}`, and `Engine::save_task`/`forget_task` clear the fault
on them. One rule, one place, two consumers.

```rust
// The pure host decision. No clock, no database, no `cfg!`.
pub fn task_host(t: TaskHostFacts<'_>, daemon: DaemonPresence) -> TaskHostVm {
    if !t.enabled || t.mode == "off"        { return off(); }
    if t.profile_id.is_some() && t.profile.is_none() {
        // Two reasons, and the difference matters: a folder that is genuinely
        // not configured, versus one whose stored row could not be read
        // (review pass 1, finding 3). The second is a fault to surface.
        return unhosted(if t.profile_unreadable { FOLDER_UNREADABLE } else { FOLDER_GONE });
    }
    if t.mode == "manual"                    { return on_request(); }
    if t.mode == "scheduled" {
        if t.schedule.is_none()              { return unhosted(NO_SCHEDULE); }
        return match daemon { DaemonPresence::Runs => daemon_host(), _ => app_host() };
    }
    // Any other spelling. Unreachable in production — `db::decode_task` files an
    // unreadable mode under `TaskListing.unknown` before this is called — and
    // kept so a future in-crate caller cannot fall through to a HOSTED verdict.
    unhosted(UNKNOWN_MODE)
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
