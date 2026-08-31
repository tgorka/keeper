---
title: 'Packaging and the chapter: a timer that calls the verb, and prose that is true'
type: 'feature'
created: '2026-08-30'
status: 'done'
baseline_revision: '48f5475'
final_revision: '5d8b98d'
review_loop_iteration: 0
followup_review_recommended: true
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

### 2026-08-30 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 22: (high 3, medium 13, low 6)
- defer: 2: (medium 2)
- reject: 0
- addressed_findings:
  - `[high]` `[patch]` **`OnCalendar=` is a list, and §14's drop-in recipe did not reset it.** Both
    reviewers, independently. An operator following step 3 verbatim would have got a `release` task
    triggered at 00:00 **and** 03:00 — a content-deleting sweep at an hour they had explicitly
    configured away from, invisible because `list-timers` shows only the next elapse. The install
    block is now numbered 1–6 with the empty `OnCalendar=` assignment as its own commented step, and
    step 5 reads `TimersCalendar` back so both entries are visible if it is skipped anyway.
  - `[high]` `[patch]` **No `SuccessExitStatus=4`, so a deferral left the unit `failed`.** Both
    reviewers. `RestartPreventExitStatus=` suppresses the restart and not systemd's own verdict, so
    every night a drive was out ended red in `systemctl --user status`, listed in `--failed`, firing
    any `OnFailure=` hook — systemd raising the exact alert exit `4` exists to prevent, and breaking
    §14's own install step 6. Added to the unit, documented, and given a test of its own.
  - `[high]` `[patch]` **`Restart=` on `Type=oneshot` is refused before systemd 244.** Blind Hunter,
    with the v243 source message quoted. On Debian 10, Ubuntu 18.04 or RHEL 8 the unit never loads
    and the timer fires onto nothing every night — on precisely the headless-server class the pair
    was built for. The floor is now stated in the unit header and in §14, with the fallback (delete
    `Restart=`/`RestartSec=`), and a test binds the two statements together.
  - `[medium]` `[patch]` **The install block's binary path was wrong and no single cwd worked.**
    `target/release/keeper-syncd` does not exist; the workspace root is `src-tauri/`. Fixed to
    `src-tauri/target/release/…` with an explicit `cd` before the unit-file installs.
  - `[medium]` `[patch]` **`next_task_window` has a *second* exception and it fires first.** Edge
    Case Hunter. The `Busy | Deferred` arm precedes the `match trigger`, so a timer run that did
    nothing at 00:00 rewrites the window to 00:01 and the daemon sweeps three hours early. Named in
    the timer header and in the `--mode` table; the one-exception claim was false in both.
  - `[medium]` `[patch]` **`Persistent=true` and `RandomizedDelaySec=3600` appeared in neither the
    chapter nor the test bindings.** Both reviewers. A `03:00` drop-in actually fires at a random
    point in 03:00–04:00, and a laptop shut overnight sweeps after boot. Both now documented with
    their real values, and the chapter test asserts the values it quotes.
  - `[medium]` `[patch]` **An unreadable release *row* fails open where an unreadable *table* fails
    closed.** Edge Case Hunter. `release_governance` folds only over parsed rows, so a stored `off`
    row a newer keeper wrote is ignored and the §9 edge deletes. Added as a fifth row of the
    three-mode table with the verification step (`tasks list` on the older binary). The code
    behaviour itself is deferred — see below.
  - `[medium]` `[patch]` **"rename the task" — there is no rename verb.** Edge Case Hunter. The only
    implementation is `forget` + `set`, which destroys the run history the chapter opens by calling
    the whole point of a task. Reworded to *choose the id when you create it*, with the cost stated.
  - `[medium]` `[patch]` **`/` was presented as the only unusable id.** Both reviewers. `validate_id`
    imposes no character set and no length bound, so `night sweep`, `sync@home` and `réveil` are all
    storable and none can be an instance name; the 256-byte unit-name cap applies too. The chapter
    and the unit header now give the real character set and the invisible failure it produces (exit
    `2`, no run row).
  - `[medium]` `[patch]` **An overlapping trigger is dropped with no run recorded.** Edge Case
    Hunter. Unlike a held lease or reservation, which both record `busy`, this writes nothing and
    `Persistent=` will not catch it up — invisible non-execution, the thing the epic exists to close.
    Documented in both files with the defence: keep the cadence coarser than the work.
  - `[medium]` `[patch]` **"a run where nothing looked exits 4" over-claimed.** Edge Case Hunter.
    `perform_release_task` returns `Ok`/exit `0` when no folders are configured at all. The sentence
    now carries that exception and what a wrapper should watch instead.
  - `[medium]` `[patch]` **The strongest test could not tell `%i` from a hard-coded id.** Blind
    Hunter. Substituting `%i` with `nightly` made a unit that had *dropped* `%i` produce a
    byte-identical argv. Now substitutes sentinels (`specifier-i-under-test`), and splits before
    expanding so the argv is the one systemd builds. Mutation-proven: hard-coding the id fails it.
  - `[medium]` `[patch]` **The start-limit assertion permitted `StartLimitBurst=0`.** Blind Hunter —
    which is the unbounded every-minute loop the bound exists to prevent. Now asserts the values,
    and the chapter quotes them as `StartLimitBurst=3` / `StartLimitIntervalSec=600` so the prose
    breaks with the unit.
  - `[medium]` `[patch]` **The chapter-quote assertion was unanchored.** Both reviewers. §14 already
    prints `OnCalendar=*-*-* 03:00:00` as an example, so retuning the shipped timer to that value
    would have kept the test green over a stale "the shipped default is" sentence — the exact
    staleness it was written to catch. Now anchored to the sentence.
  - `[medium]` `[patch]` **Nothing constrained `[Service]` keys the tests did not name.** Edge Case
    Hunter: `RemainAfterExit=yes` would leave the instance active and turn every later trigger into
    a silent no-op, and it passed all six tests. Given its own test.
  - `[medium]` `[patch]` **§18 omitted §14 entirely.** Blind Hunter. A reader consulting the status
    section concluded Tasks was designed but not built. §18 now records tasks as real with the
    macOS background-host limit as a fourth bullet.
  - `[medium]` `[patch]` **§14 documented exit `3`; `tasks run --help` did not.** Blind Hunter. `3`
    is reachable (`Engine::open` resolves `git` first) and the shipped unit lists it, so a wrapper
    author reading only `--help` was the one consumer left unable to handle it. Added to the help,
    and `tasks_run_help_names_the_exit_codes_it_can_produce` now requires it.
  - `[medium]` `[patch]` **The one-minute retry evicts the 50-run history.** Edge Case Hunter. Fifty
    minutes of a drive being out wipes every earlier row, so the record §14 calls the source of truth
    cannot answer the question it exists for. Documented with the two mitigations.
  - `[low]` `[patch]` Two assertions that could not fail (`!prevented.contains(&EXIT_FAILURE)` after
    an exhaustive `assert_eq!`) read as independent guards; folded into the `assert_eq!` message.
  - `[low]` `[patch]` The `Restart=` assertion message misstated which values `Type=oneshot` accepts,
    contradicting the unit file's own correct comment. Corrected.
  - `[low]` `[patch]` `expanded.split_whitespace()` modelled expand-then-split; systemd splits then
    expands. Reordered, which matters for any instance name containing a space.
  - `[low]` `[patch]` "enabling a timer-driven oneshot runs it once at login" was stated as
    observable behaviour, but the shipped service has no `[Install]` and cannot be enabled at all.
    Restored to the conditional the unit header already used, with what `systemctl` actually says.
  - `[low]` `[patch]` `docs/decisions.md` D-3 named "needs the daemon crate to build there first" as
    the launchd blocker, which §14 now contradicts with evidence (`release.yml:242`). D-3's platform
    paragraph and revisit trigger rewritten to the true blocker; its `§4`/`§13` citations untouched
    and still resolving.
  - `[medium]` `[defer]` The unreadable-release-row behaviour itself: `release_governance` should
    arguably fail closed on an unknown row the way it fails closed on an unreadable table. Ledgered
    rather than fixed — it is a behaviour change in `keeper-sync/src/engine.rs`, which this story is
    scoped out of and which a sibling agent held during this run.
  - `[medium]` `[defer]` `ARCHITECTURE-SCHEDULED-TASKS.md:111` ("the unit file that ships is a thin
    caller of `tasks run`, **not the source of truth**") is contradicted by the shipped artifact,
    since `OnCalendar` is the source of truth for that driver's cadence. Ledgered; the planning doc
    is not this story's to rewrite.

**Both reviewers reported clean on the areas most at risk.** Every JSON key name and count, exit-code
mapping, outcome spelling, lease and retry constant, `unhosted` reason and gate order, and the whole
schedule dialect — Edge Case Hunter fed every boundary input through `TaskSchedule::parse`,
`CronSpec::parse` and `parse_field` and found the chapter's grammar matching the parser in every
case. The `TimeoutStartSec`-is-infinite-for-`oneshot` claim and the `%i`-versus-`%I` reasoning both
verified correct in both directions.

## Auto Run Result

Status: done

**What was implemented.** A Linux box with no app and no GUI can now put a keeper task on a
schedule, and `docs/sync.md` finally describes the feature the epic built. Two template unit files
in `packaging/`, a §14 chapter written verb-by-verb against the source, and repairs to every
sentence Epic 57 falsified elsewhere in that document.

**Files changed**
- `src-tauri/crates/keeper-syncd/packaging/keeper-syncd-tasks@.service` — **new.** `Type=oneshot`,
  `ExecStart=%h/.local/bin/keeper-syncd tasks run %i`, `Restart=on-failure` + `RestartSec=60` +
  `RestartPreventExitStatus=2 3 4`, the retry ceiling written down (`StartLimitIntervalSec=600`,
  `StartLimitBurst=3`) rather than inherited, `NoNewPrivileges=yes`, `PrivateTmp=yes`, no
  `[Install]`, and a comment block recording what is deliberately absent and why.
- `src-tauri/crates/keeper-syncd/packaging/keeper-syncd-tasks@.timer` — **new.** `OnCalendar=daily`,
  `Persistent=true`, `RandomizedDelaySec=3600`, `AccuracySec=1m`, `WantedBy=timers.target`, and a
  header that states which cadence is real and which `--mode` to pair with it.
- `src-tauri/crates/keeper-syncd/src/commands.rs` — a `packaging` test module (six tests over a
  strict INI reader) and two corrected doc comments on `TaskCommand::Run`.
- `docs/sync.md` — §14 Tasks (≈370 lines); §14–§19 → §15–§20; repairs in §9, §13 and §20.
- `docs/decisions.md` — deliberately **unchanged**; see the renumbering record below.

**The renumbering, re-resolved line by line.** §14 was inserted after the daemon chapter, so §13 and
everything below it kept their numbers while §14–§19 became §15–§20. Every `§N` in both files was
followed: references exist to §1, §4, §5, §6, §7, §8, §9, §10, §11, §12, §13 (all unmoved) and to
§15 (moved). Exactly **one** reference needed changing — the Troubleshooting table's credential row,
`(§15)` → `(§16)` for Security posture. No reference to §17–§20 exists anywhere. `docs/decisions.md`
cites `docs/sync.md` §4 (iCloud placeholders, D-2) and §13 (`update` never installs by itself, D-3);
both still resolve, so that file was not touched. A script check confirms the heading sequence is
`1..20` with no gap and that all 36 code fences balance.

**What was cross-checked against the source, and where.** Every claim in §14 was read off the code,
not off the epic:
- the seven verbs and `tasks set`'s six flags — `commands.rs:502-642` (`TaskCommand`, `TaskSetArgs`)
- the exit codes `0/1/2/3/4` and that `4` is reachable only from `tasks run` — `commands.rs:17-26`,
  `:58-87`, `task_exit_code` `:2962`
- the dialect: 5-field cron, the `@`-aliases' exact desugaring, `every <n>` units, the four
  refusals, the one-minute floor and one-year ceiling — `keeper-sync/src/tasks.rs:31-42`, `:302-412`;
  the field grammar (`*`, `n`, `low-high`, `/step`, no wrap, no names, `7`=Sunday) — `:571-623`
- `every` measuring from the **end** of the previous run — `tasks.rs:259-272`
- the eleven task-document keys, the seven run keys and conditional `unknownOutcome`, and all five
  envelopes — `commands.rs:3340-3465`
- the five outcome spellings — `tasks.rs:208-218`; the 50-run cap — `db.rs:2794`
- the three release modes and the `(None, _)` = Epic-56-unchanged arm — `engine.rs:7957-8071`
- a release run where nothing looked is `Deferred` — `engine.rs:2504-2516`, including the real
  `detail` string quoted in the chapter's JSON example
- lease `1 h` / retry `1 min` — `engine.rs:507`, `:517`; `<device-id>#<pid>` — `engine.rs:2527-2529`
- the three `unhosted` reasons and the gate order — `keeper-core/src/tasks.rs:93-105`, `:387-410`
- `update` has no `TaskKind` and a stored `"update"` row reads unknown — `tasks.rs:81-86`

**Two corrections the cross-check forced, both of which the epic got wrong.**
1. **`tasks run` never consults a schedule.** `claim_and_run` passes `due_at_most: None` for
   `TaskTrigger::Requested` (`engine.rs:2126-2134`), so the verb performs the work every time. The
   epic's and `ARCHITECTURE-SCHEDULED-TASKS.md:111`'s framing of the shipped unit as a caller that
   merely *asks whether work is due* is wrong. Caught before shipping: the first draft's
   `OnCalendar=hourly` would have run a nightly release sweep twenty-four times a day. See the Spec
   Change Log.
2. **`keeper-syncd` does build on macOS.** `release.yml:229-282` builds and publishes
   `keeper-syncd-aarch64-apple-darwin` with a checksum, and `platform.rs:867-875` has an explicit
   macOS gate. The prose in this tree that says the crate "does not build there" is a half-stated
   premise reaching the right conclusion. The provable AD-137 fact is narrower: **no launchd plist
   exists anywhere in the repository**, so keeper ships no background host on macOS even though the
   binary and its one-shot verbs work there. §14 and the launchd paragraph say that version.

**Verification**
- `cargo test -p keeper-sync -p keeper-core -p keeper-syncd`: **3756 passed, 0 failed** (baseline
  3736; eight of the new ones are mine, the rest a sibling's concurrent story). The first commit
  measured 3742 before the review fixes added two more tests.
- `cargo clippy … -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings`:
  **clean**, zero warnings, with commit `87f63ff` in the tree. One pre-existing future-compat note
  about `proc-macro-error2`, the same one 57.5 recorded. Run by `TasksFixes` on the shared worktree
  and handed over rather than duplicated: cargo was serialised between us by agreement, and their
  run covers this commit plus their own changes, which is strictly stronger than an isolated one.
- `cargo fmt --all --check`: no diff — also the only local syntax gate the shell crate has.
  `cargo fmt -p keeper-syncd --check` was additionally run alone, because `--all` was transiently
  red on a sibling's in-progress files and I did not run the formatter over their edits.
- `bun run typecheck`: clean. `bun run lint`: 4 warnings + 1 info, the baseline. `bun run test`:
  **300 files / 4966 tests passed**, the baseline exactly — no frontend file was touched.
- `cargo test -p keeper-syncd --bin keeper-syncd -- tasks_run_help shipped chapter`: **7 passed**,
  including `tasks_run_help_names_the_exit_codes_it_can_produce` — so the rewritten
  `TaskCommand::Run` prose was proved to survive clap's rendering with `0/1/2/4/deferred` intact,
  executed rather than asserted.
- **`systemd-analyze` is absent on this host, and so is `systemctl`** (`command -v` finds neither).
  No systemd-native validation was run and none is claimed. The substitute is six tests over a
  strict INI reader that fails on anything which is not a comment, a blank line, a `[Section]`
  header, or a `Key=Value` inside a section, and which keeps duplicate keys so a second `ExecStart=`
  cannot hide.
- Four mutations applied and reverted one at a time, each restore verified by `cmp` against a
  pristine copy: `tasks run` → `tasks runn` failed only the clap-parse test; dropping `4` from
  `RestartPreventExitStatus` failed only the exit-taxonomy test; retuning `OnCalendar` to `hourly`
  failed only the chapter-binding test; and hard-coding
  `nightly` in place of `%i` failed only the ExecStart test, which is the mutation the first draft of
  that test did *not* catch and the review found.

**Residual risks and handoffs**
- Nothing here was executed by systemd. The units are proved to *parse* and to name a verb that
  exists with those flags; that a real `systemctl --user` accepts them is untested on this host, and
  §14's install block is the sequence a person should run once on a real box.
- `Restart=on-failure` on a `Type=oneshot` is documented-valid (only `always` and `on-success` are
  refused for that type) but was not exercised by a live systemd.
- The app's Tasks view will not name the timer as a host, because `daemon_presence` probes
  `keeper-syncd.service` only. Left as-is deliberately — changing it is a behaviour change in a
  crate that cannot be compiled here — and §14 states the limit instead.
- `daemon_presence` did not check lingering either, so its *logged in or not* sentence was optimistic
  on a non-lingering box (found by `TasksReview`, logged against 57.5). **Closed after this story
  landed**, which is what it was waiting for: with the user timer shipped, 57.5's review pass 3 gave
  `DaemonPresence` a `RunsUntilLogout` state fed by one `stat` of `/var/lib/systemd/linger/$USER`,
  and §14's blind-spot paragraph is replaced by the two sentences the view now shows. §14 keeps
  lingering as a required, verified install step.
- `tasks-pane.tsx`'s empty state told users to run `keeper-syncd task add`, which has never existed.
  Found by `TasksReview`, owned by `TasksFixes`, who confirmed the pane will quote §14's real
  spelling (`keeper-syncd tasks set <id> --kind …`). Not fixed here: that file belongs to 57.5's
  triage, and my chapter commits to the correct spelling so the two cannot drift again.
- `ARCHITECTURE-SCHEDULED-TASKS.md:111` and `docs/decisions.md` D-3's "needs the daemon crate to
  build there first" are both now contradicted by the shipped artifacts (see the two corrections
  above). `TasksFixes` is putting both into `deferred-work.md`; neither was edited here.

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
