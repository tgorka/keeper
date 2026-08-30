---
title: 'Packaging and the chapter: a timer that calls the verb, and prose that is true'
type: 'feature'
created: '2026-08-30'
status: 'in-progress'
baseline_revision: '48f5475'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/planning-artifacts/epic-57-a-task-that-runs-when-it-should.md'
  - '{project-root}/_bmad-output/planning-artifacts/architecture/architecture-keeper-2026-07-03/ARCHITECTURE-SCHEDULED-TASKS.md'
warnings: ['multiple-goals']
---

<intent-contract>

## Intent

**Problem:** Waves 1–3 shipped the whole task feature — the record, the dialect, the lease, the
due-gate on each host's existing tick, seven CLI verbs with a four-number exit taxonomy, a release
task with three modes, and a Tasks view at ⌘8. Two things are missing and both are the difference
between a feature and a usable one. A Linux box with **no app and no GUI** has no packaged way to
put a task on a schedule: `packaging/` holds one unit, `keeper-syncd.service`, which runs `watch`.
And `docs/sync.md` — the authority every other surface defers to — does not mention tasks at all,
while several of its existing paragraphs are now **false**: it states as fact that there is no
`.timer` unit, that the release sweep's success edge is "the only release mechanism built here",
and it lists three exit codes where there are now four. (FR-352; AD-136, AD-137.)

**Approach:** Ship the systemd pair beside the existing unit — a `.timer` and a `oneshot` service
that calls `keeper-syncd tasks run <id>` and nothing else — and grow `docs/sync.md` a tasks
chapter, verified verb by verb against the source. Then repair every sentence this epic falsified.

## Boundaries & Constraints

**Always:**
- **The unit file is a thin caller of a verb, not a reimplementation.** keeper owns what a task
  *is*: its name, kind, target, mode, every refusal it walks and its whole record (AD-136).
- **Same posture as `keeper-syncd.service`, imitated rather than invented**: systemd **user** unit,
  `WorkingDirectory=%h`, `ExecStart=%h/.local/bin/keeper-syncd …`, `NoNewPrivileges=yes`,
  `PrivateTmp=yes`, and the same commented record of what is deliberately *not* set.
- **`RestartPreventExitStatus` honours the exit taxonomy wave 2 shipped.** `2` config, `3` missing
  prerequisite and `4` deferred are the three numbers a restart cannot help; `1` stays restartable.
  `EXIT_DEFERRED`'s own doc comment names this story and asks for exactly that.
- **Every verb, flag, exit code and JSON key in the chapter is read off the code**, not off the
  epic. Story 56.13 shipped a `--help` describing behaviour that had already been replaced, and it
  survived review because the prose sat far from the code.
- **Renumbering is re-resolved line by line**, in `docs/sync.md` and from `docs/decisions.md`, and
  the resolution is recorded here.
- No Rust *behaviour* change. Tests and doc comments may change; no shipped code path does.

**Block If:** nothing. Every fact is in this tree.

**Never:**
- No `update` task, no new `TaskKind`, no change to the dialect, the lease, `decide`, or any exit
  code.
- No system-wide (`/etc/systemd/system`) unit and no `launchd` plist — macOS has no daemon (AD-137),
  and a plist for a crate that does not build there would be a lie in a file.
- No change to `DAEMON_UNIT` in `sync_ipc.rs` or to `daemon_presence` — that is a behaviour change
  in a crate that cannot be compiled here. The chapter states the resulting limits instead.
- No edits outside `packaging/**`, `docs/sync.md`, `keeper-syncd/src/commands.rs` and this spec:
  `TasksFixes` owns `engine.rs`, `db.rs`, `sync.rs`, `keeper-core/src/tasks.rs`, `tasks-pane.tsx`,
  `mock-shell.ts` and `spec-57-5` on this branch concurrently.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| the timer fires, work is possible | drive attached, folder idle | `tasks run` performs the work, exits `0` | none |
| the timer fires, drive unplugged | `MediaAbsent` | exit `4`, recorded `deferred`, **no restart, no alert** | `RestartPreventExitStatus` |
| the timer fires, `watch` holds the lease | live lease elsewhere | exit `4`, recorded `busy`, no restart | as above |
| the task id is misspelt in the unit | `tasks run nightl` | exit `2`, **no restart** — a loop would bury the one line naming the fix | as above |
| no `git` on the box | prerequisite absent | exit `3`, no restart | as above |
| the sweep ran and failed | `failed` recorded | exit `1`, `Restart=on-failure` retries, bounded 3-in-600 s | bounded |
| the unit names a verb that does not exist | drift | **the build fails**: a test parses `ExecStart` through clap | test |
| the shipped cadence is retuned | `OnCalendar=` edited | **the build fails** until §14's quoted default matches | test |
| the chapter is renumbered | `§15` → `§16` | every `§N` in both files still resolves | line-by-line |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-syncd/packaging/keeper-syncd-tasks@.service` — **NEW.** A `oneshot`
  **template** unit; the instance name is the task id, so one file serves every task.
  `ExecStart=%h/.local/bin/keeper-syncd tasks run %i`.
- `src-tauri/crates/keeper-syncd/packaging/keeper-syncd-tasks@.timer` — **NEW.** The trigger, a
  template too, so `keeper-syncd-tasks@nightly.timer` drives `keeper-syncd-tasks@nightly.service`
  by systemd's own instance-name rule and no `Unit=` line is needed.
- `src-tauri/crates/keeper-syncd/src/commands.rs` — a `packaging` test module (six tests plus a
  strict INI reader), and one corrected doc comment on `TaskCommand::Run`.
- `docs/sync.md` — the new **§14 Tasks**; §14–§19 renumbered to §15–§20; the falsified sentences in
  §9, §13 and §20 repaired; `tasks` added to §13's verb list and `4` to its exit-code line.
- `docs/decisions.md` — checked, **not edited**: its only `docs/sync.md` references are §4 and §13,
  and both keep their numbers.

## Tasks & Acceptance

**Execution:**
- [x] `packaging/keeper-syncd-tasks@.service` — the oneshot caller: `Type=oneshot`,
      `Restart=on-failure`, `RestartSec=60`, `RestartPreventExitStatus=2 3 4`, the retry ceiling
      stated (`StartLimitIntervalSec=600`, `StartLimitBurst=3`) rather than inherited, the two
      hardening lines, no `[Install]`, and a comment recording why the daemon unit's stop contract
      is *not* copied.
- [x] `packaging/keeper-syncd-tasks@.timer` — `OnCalendar=daily`, `Persistent=true`,
      `RandomizedDelaySec=3600`, `AccuracySec=1m`, `WantedBy=timers.target`, and a header that
      states plainly that this cadence is the real one for this driver.
- [x] `keeper-syncd/src/commands.rs` — six tests over a strict INI reader.
      `systemd-analyze` and `systemctl` are **absent on this host** (verified); this is the
      substitute and is labelled as such in the reader's own doc comment.
- [x] `keeper-syncd/src/commands.rs` — correct `TaskCommand::Run`'s help, which claimed the
      schedule is never moved without the already-open exception `Engine::next_task_window`
      implements. Exactly the 56.13 failure class this story is chartered against.
- [x] `docs/sync.md` — the new §14, written against the source.
- [x] `docs/sync.md` — repair §9's "never a timer" and "only release mechanism" paragraphs, §13's
      exit-code line, verb list and install line, and §20's `update` bullet; renumber §14–§19 and
      re-resolve the one `§15` reference.

**Acceptance Criteria:**
- Given a Linux box with `keeper-syncd` in `~/.local/bin` and one stored release task, when the two
  packaging files are installed, lingering is enabled and
  `systemctl --user enable --now keeper-syncd-tasks@<id>.timer` is run, then the release task runs
  on a schedule with no app, no GUI and no `watch` daemon — and §14 contains that exact sequence.
- Given the packaging files, when `cargo test -p keeper-syncd` runs, then a test parses each file
  and a test feeds the service's `ExecStart` argv through the real clap parser.
- Given `docs/sync.md` and `docs/decisions.md` after the edit, when every `§N` is followed, then
  each resolves to the section it describes.
- Given the gates, then the Rust suite is above baseline with 0 failed, `cargo fmt --check` is
  clean, and lint/typecheck/test sit at their recorded baselines.

## Spec Change Log

### 2026-08-30 — the shipped timer's cadence, corrected against the code

**Finding (mine, during implementation).** The planned design — and the epic's own wording, and
`ARCHITECTURE-SCHEDULED-TASKS.md:111` — frame the shipped unit as "a thin caller of `tasks run`, not
the source of truth", which reads as *the timer only asks whether work is due*. It does not.
`Engine::claim_and_run` passes `due_at_most: None` for `TaskTrigger::Requested` (engine.rs:2126-2134),
so `tasks run` **performs the work unconditionally** and never reads the task's `schedule` column.

**Amended.** The first draft of the timer shipped `OnCalendar=hourly` with prose claiming
twenty-three of twenty-four daily calls would "find nothing due and exit 4". That is false: it would
have run a nightly release sweep twenty-four times a day. The shipped default is now
`OnCalendar=daily`, both unit headers state that this cadence is the real one for this driver, §14
says so bluntly with a `--mode` pairing table, and a test binds the documented default to the
shipped file.

**KEEP.** The AD-136 claim that survives is narrower and still load-bearing: what is forbidden is a
cadence with *no task row behind it*, because then `tasks list` and the Tasks view have nothing to
show. `ExecStart` naming a stored task is what prevents that, and a test asserts it.

### 2026-08-30 — the already-open window exception

**Finding (`TasksReview`, verified by me against engine.rs:2213-2219).** `next_task_window`'s
`Requested` arm preserves `next_due_ms` only while it is still in the future; an already-open window
is consumed and re-armed. Three sites claimed otherwise — the timer header, §14's `--mode` table,
and `TaskCommand::Run`'s own `--help`. All three now state the exception. Reachable precisely because
`Persistent=true` fires a catch-up run with no ordering against the daemon's first tick.

## Review Triage Log

## Design Notes

**Why a template unit (`@`) rather than one unit per task.** `tasks run` takes a task id, and a
non-template unit would have to hard-code one — so a second task means a second copy of the file,
each free to drift in its hardening lines and its `RestartPreventExitStatus`. systemd's own answer
is `%i`, and it costs nothing here: the instance name IS the task id, which is also the string
`tasks list` prints. `%i` and never `%I`: a task id normally contains hyphens (`release-nightly`),
and `%I` would unescape those into slashes and ask keeper for a task nobody stored.

**Why the tests parse rather than grep.** `systemd-analyze` is not installed on this host and
neither is `systemctl`, so there is no way to have systemd itself validate these files here. The
substitute is a strict reader: anything that is not a comment, a blank line, a `[Section]` header or
a `Key=Value` **inside** a section fails the test. Duplicates are kept rather than folded into a
map, so a second `ExecStart=` added below the first cannot hide. The strongest of the six is
`the_shipped_task_service_runs_a_verb_this_binary_has`, which expands `%h`/`%i`, feeds the argv to
the real `Cli` parser and requires the parse to come out as `Command::Tasks { TaskCommand::Run { .. } }`
with the instance name as its selector — not merely as something clap tolerates.

**Why `Restart=on-failure` on a `Type=oneshot`.** Permitted (only `always` and `on-success` are
not), and worth having: exit `1` means the work ran and failed, often on a transient remote, and the
next `OnCalendar` may be a day away. The ceiling is written into the file rather than inherited from
whatever `DefaultStartLimit*` a distribution ships.

**What was deliberately not copied from `keeper-syncd.service`.** `KillSignal=SIGTERM` and
`TimeoutStopSec=20` are there because `watch` installs a SIGTERM handler and finalizes in-flight
work under a 10 s bound (`run_supervisor`, commands.rs:1436-1443). `tasks run` installs no handler,
so those lines would document a stop contract this verb does not have. It needs none: a task is
idempotent and safely abandonable (NFR-42), and a killed run is closed `abandoned` by the next host
to reclaim its expired lease. `TimeoutStartSec` is likewise left at systemd's `Type=oneshot`
default of infinity — a host-wide sync task's duration is not knowable from a unit file, so any
number written there would eventually kill real work, and a one-shot still running when the timer
next fires is simply not started again.

## Verification

**Commands:**
- `GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=keeper GIT_AUTHOR_EMAIL=dev@keeper.local GIT_COMMITTER_NAME=keeper GIT_COMMITTER_EMAIL=dev@keeper.local cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync -p keeper-core -p keeper-syncd` — expected: 0 failed, at or above the 3736 baseline.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings` — expected: clean.
- `cargo fmt --manifest-path src-tauri/Cargo.toml --all --check` — expected: no diff. This also *parses* the shell crate, the only local syntax gate it has.
- `bun run lint && bun run typecheck && bun run test` — expected: lint 4 warnings + 1 info, typecheck clean, 300 files / 4966 tests.

**Manual checks (if no CLI):**
- `systemd-analyze` is **absent on this host** (`command -v systemd-analyze` → nothing; `systemctl`
  likewise), so no systemd-native validation was run and none is claimed. The six parsing tests are
  the substitute, and three of them were mutation-proven: a mangled verb, a dropped `4`, and a
  retuned `OnCalendar` each failed their owning test and only that test.
- Every `§N` reference in `docs/sync.md` and `docs/decisions.md` re-read against the renumbered
  heading list; the resolution is recorded under `## Auto Run Result`.
