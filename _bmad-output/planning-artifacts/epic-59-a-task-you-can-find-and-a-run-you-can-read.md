# Epic 59 — A task you can find, and a run you can read

created: '2026-08-31'
source: the owner's second Tasks-view pass, written while running v0.8.24 — the first release in which ⌘8 actually holds tasks. Grounded by six read-only repository scouts over `main` @ `0224d31` (`v0.8.24`), 2026-08-31: the run/execution surface, manual execution, list navigation and selection, the two new task fields, the script-kind ask, and the two form inputs. Every verdict below carries the `file:line` it was read from.
binds: FR-361…FR-368 (allocated here); AD-143…AD-145 (new); AD-135, AD-136, AD-137, AD-138…AD-142 (Epics 57/58, consumed here); AD-139 (the no-JSON-blob rule on `tasks`, honoured by 59.5); AD-C7 (the one-component-two-modes form rule, consumed by 59.5 and 59.7); AD-62 (untouched)
see-also: Epic 57 (`epic-57-a-task-that-runs-when-it-should.md`), Epic 58 (`epic-58-a-task-you-can-drive-and-a-window-that-passed.md`) — both merged and shipped in v0.8.24. Epic 60 (`epic-60-*`, not yet written) owns the general exec kind this epic deliberately does not build.

## What he said

> the task list it would be good to see the list of the saved names (for multichoose) -> list of the
> executions (sorted by date - latest highest) -> detail - so even if it's schesuled task i can still
> click execute it manualy (look github actions)
>
> For now i cant see details (output, data, status, etc) only description
> add also description in the task
> add possibmitilty to call script in kind (or other cli) with place for the cli and parameters
> for schedue add some UI help for creating the croon string
> for the folder add option of home dir
> if window missed when choose delay add option how much delay

Seven asks. **Five of them are one defect**, and the triage is what this epic is written from rather
than the report as worded.

## What the scouts found, and why this epic is not the report

| his ask | verdict | evidence |
| --- | --- | --- |
| run a scheduled task by hand | **already works** | `run_task_now` → `TaskTrigger::Requested` → `due_at_most: None`, so `claim_task`'s window predicate is dropped (`engine.rs:2381-2407`, `:8148-8151`). The app's button passes `Person` unconditionally (`sync_ipc.rs:2226`); it is rendered on every readable row with no gate but *this row's run is in flight* (`tasks-pane.tsx:942-950`), and a test drives it on a `mode: "scheduled"` fixture (`tasks-pane.test.tsx:158-159, 356-358`). It is `workflow_dispatch` already. |
| executions, newest first | **already exists** | `task_runs WHERE task_id = ?1 ORDER BY id DESC LIMIT ?2` (`db.rs:3748-3752`), `id DESC` and not `started_ms DESC` because "two runs can share a millisecond" (`db.rs:222-224`). 20 rows per read (`sync_ipc.rs:1745`) over a 50-run store cap (`db.rs:2863`). |
| output, data, status | **partly deliberate, partly absent** | A run holds `outcome`, `started_ms`, `finished_ms`, `detail`, `host` and nothing else (`db.rs:213-221`). There is no stdout, no counts, no structured data — and no *detail surface*: one execution is four inline spans in a list (`tasks-pane.tsx:734-859`). |
| a description on the task | **absent** | Ten columns, none free-text (`db.rs:191-202`, `:2867-2869`). Worse: the Add form sends `id: ""` so Rust mints a ULID (`sync_ipc.rs:2282`), and the edit note forbids changing an id ever, because `task_runs.task_id` joins on it (`task-form.tsx:94-96`). |
| a script / CLI kind | **absent, over a *Deferred* decision** | `TaskKind` has two variants (`tasks.rs:148-171`); `TaskRow` has no path or command field (`db.rs:2878-2898`). The deferral and its bill are quoted below. |
| cron help | **absent** | The dialect is exact and small (below). The form names it in prose and re-implements no parser on purpose (`task-form.tsx:150-158`). |
| home dir for the folder | **a picker convenience** | A task addresses a *sync profile or the host* (`profile_id: Option<String>`), never a path. A profile's `local_path` must be absolute (`profile/mod.rs:1158`) but is not forbidden from being `$HOME`; Add-folder already opens a native directory dialog (`add-folder-form.tsx:1208`); nothing anywhere expands `~`. |
| how much delay | **absent, cheap, coherent** | `TASK_MISSED_DELAY_MS` has exactly **one** production reader, and 58.4's anchor — *the instant a host noticed the window* — is what makes a per-task value meaningful rather than contradictory. |

**The through-line, and the epic's spine.** The Tasks pane is one flat, unbounded, unfoldable,
unsortable, unfilterable column of ~250px detail cards in a single scroller (`tasks-pane.tsx:1703-1850`),
and `list_tasks` returns every row `ORDER BY id` with no cap (`db.rs:3301-3302`). Level 1 (names) and
level 2 (executions) are fused into one page; level 3 (one execution) does not exist. Reaching the
eighth task's runs means scrolling past seven full cards. **Every capability he asked for is already
built and honest — none of it is findable.** So this epic is mostly navigation, plus three additive
fields and one closed-vocabulary kind.

**What this implies about the shipped tests, stated because it is the reason to trust the code and
not the surface.** Nothing asserted the wrong thing. The pane's own comment names the blind spot
verbatim: *"jsdom performs no layout, so no component test in this file could ever catch a control
that had left the screen"* (`tasks-pane.tsx:1042-1043`) — which is exactly why 58.3 moved the `Runs`
control off a `shrink-0` strip that already held three buttons. The owner's report is evidence that
three was still one too many. No mechanical check owns this class; 59.1 removes the class instead.

## The decision this epic does not build over

`architecture/architecture-keeper-2026-07-03/ARCHITECTURE-SCHEDULED-TASKS.md:364-369`, under
`## Deferred` — **not** under a Never:

> - **Arbitrary user commands as tasks** — a task kind is one of keeper's own verbs, never a shell
>   string. Running user-supplied commands from a sync daemon is a different security posture
>   (egress, credentials, `NoNewPrivileges=yes` in the unit) and needs its own decision. Revisit only
>   with a stated threat model.

It has **partial teeth**, and the teeth are specific rather than vague. Every configured remote is a
*disclosed egress destination — disclosed in `egress.md`, which the release workflow diffs against
the previous tag* (`docs/sync.md:2693-2699`). A user script can reach any host on the internet and
that diff would show nothing whatsoever. There is also no task timeout anywhere (only a one-hour
lease, `TASK_LEASE_MS`) and no stdout capture. The credential invariant and the unit's
`NoNewPrivileges=yes` survive an exec kind untouched.

**Resolved with the owner, 2026-08-31: both, in that order.** This epic ships 59.9 — a new *closed
vocabulary* kind that runs a verb keeper already owns — which reopens nothing. The general exec kind
is Epic 60, whose first story is the threat model, the egress answer and the timeout, because those
are the bill and not the feature. `update` stays refused absolutely, in all three places that refuse
it (`docs/sync.md:1913-1918`, `tasks.rs:134-139`, `engine.rs:2507-2511`).

## The schedule dialect, exactly — 59.7 is only as good as this table

Read from `tasks.rs:578-675`. A builder that offers anything else is offering a refusal.

| form | example | notes |
| --- | --- | --- |
| 5-field cron | `0 3 * * *` | minute hour day-of-month month day-of-week. Its finest resolution *is* one minute, so it satisfies the floor by construction (`tasks.rs:20-22`) |
| alias | `@hourly` `@daily` `@weekly` | desugared to cron, never to an interval, so nightly keeps meaning night rather than drifting to the last restart (`tasks.rs:608-618`) |
| interval | `every 90m` | units `s/m/h/d` and their long forms; seconds are in the grammar so `every 30s` is told about the floor rather than about an unknown unit (`tasks.rs:634-643`) |
| empty | `` | stores no schedule; a `scheduled` task with no schedule is refused at the write door |

Floor **60 s** (`MIN_SCHEDULE_INTERVAL_MS`), ceiling **one year** (`MAX_SCHEDULE_INTERVAL_MS`), and a
cron that parses but names no real date (`0 0 30 2 *`) is refused at save time in constant time
rather than by walking a calendar (`tasks.rs:660-673`). Four distinct refusals — malformed, below
floor, above ceiling, matches-no-instant — each quoting the original text the person typed, not its
lowercased copy (`tasks.rs:574-577`).

**The precedent 59.7 must reuse, and it is not a JavaScript cron library.**
`recording-destination-controls.tsx:16-25` renders a **Rust-composed preview** through
`recording_path_preview`, on the stated rule that *both the clock and the renderer belong to Rust*.
A schedule helper shows the next few instants **Rust** computed; `nextDueMs` already exists on
`TaskVm`, so the first one is a read the pane already makes.

## Stories

    Wave 1 — the view becomes navigable (no Rust, no schema; this is the owner's real complaint)
    59.1  a list of names, and one task at a time   (level 1/level 2 split: the flat card column becomes a master list + a detail region)
    59.2  a run you can open                        (level 3: one execution as a surface, and a Runs control that looks like one)
    59.3  the row says its mode, and Run now says what it does  (the discoverability half of an ask that already works)
    59.4  several tasks at once                     (multi-select, and only the bulk actions the store can honestly do)

    Wave 2 — the three additive fields
    59.5  a task you can name                      (description TEXT, nullable; the full column ripple)
    59.6  how long is the delay                    (a per-task override of TASK_MISSED_DELAY_MS)
    59.7  help for writing a schedule              (a Rust-composed preview of the next instants, and the dialect made selectable)

    Wave 3 — the two smallest asks, and the chapter
    59.8  home, in one click                       (a Home choice in the folder picker, and `~` accepted)
    59.9  a task that runs a verb keeper already owns  (TaskKind::Verify — a closed vocabulary, no column, no decision reopened)
    59.10 the chapter grows a view                 (docs/sync.md §14)

**Wave 1 is one file and therefore one owner.** All four stories rewrite `tasks-pane.tsx`; 59.1
restructures it and the other three extend the result. They are **not** parallelisable across agents
the way epic 58's Wave 1 was — 58.1/58.2/58.3 collided in that file and the epic said so, and this
time the collision is the whole story rather than one `Field` each. 59.1 lands first and alone;
59.2–59.4 follow it and may then run together.

59.1 is also the story that must **not** lose what the flat row got right: every fact epic 58 put on
the row is a fact somebody needs, so this is a re-siting, not a deletion. The host badge and its
Rust-composed sentence, the unhosted reason, the refusal paragraph and the paced section all keep
their current wording.

Wave 2's three are genuinely independent: 59.5 and 59.6 each add one column and touch the same five
places (measured from 58.4's `on_missed` diff, which is the honest sizing), and 59.7 adds an IPC read
and no column. 59.6 changes the shape of what the form-note mirroring guard can assert — that guard
exists because this exact sentence shipped wrong once (`task-form.tsx:162-180`), so the story owns
re-pointing it at a per-task value rather than deleting it.

59.8 is the only story outside the Tasks surface: it lands in `add-folder-form.tsx` and, if `~` is to
be accepted anywhere, in one place that expands it. Nothing in the tree expands `~` today.

59.9 is deliberately last of the code stories and deliberately small. It is one variant, one arm in
an exhaustive match, and one docs row.

## Acceptance, per story

**59.1** — Opening ⌘8 shows one line per task: kind, its name, the host that will run it, and when it
is next due — enough to scan twenty tasks without scrolling past their details. Selecting one shows
everything the card shows today, re-sited and not reworded. The list keeps `list_tasks`' order until
a story says otherwise, and an unreadable row (`TaskListing.unknown`) keeps its own section and its
own explanation. No IPC changes.

**59.2** — A task's runs are reachable by a control that reads as a control — a count and a chevron —
not a dotted underline below the host block. Selecting one run shows its outcome word, when it
started, when it finished, how long it took, which host ran it, and its full report, with absence
rendered as absence and never as an empty string (`taskReportText`'s existing rule). The 20-of-50
bound becomes visible rather than silent: the view says how many runs it is showing and that older
ones are trimmed.

**59.3** — Given a `scheduled` task, the row states that it is scheduled, and the pane states in words
that **Run now performs the work whether or not a window is open, and does not move the schedule** —
the sentence `docs/sync.md:2507-2512` already owns, said where the button is. No Rust changes, and
nothing that makes Run now consult the window: 58.6's paired tests exist to stop exactly that.

**59.4** — Several tasks can be selected, and the actions offered on a selection are only the ones the
store can perform honestly, one write per task, each reporting its own refusal — with a confirm that
names the count for anything destructive. If the store has no bulk path, the story adds none: it
loops, and says so.

**59.5** — A task carries an optional description, writable from the app and from `tasks set`, shown
under its name, blank meaning nothing rendered. Given a task written before this column existed, it
reads as having no description rather than as an empty one — `ensure_journal_columns`' nullable rule
(`db.rs:434-446`), not a `DEFAULT ''`.

**59.6** — A task whose missed-window policy is `delay` carries its own delay, defaulting to the
constant's 30 minutes, honoured by the one production reader, and stated in the form's note computed
from the effective value rather than from a mirrored literal. Given a stored value below the grace
period, the write is refused with the reason — a delay shorter than the interval that concludes
nobody was home is not a delay.

**59.7** — The schedule field offers the dialect rather than requiring recall of it, and shows the next
instants **Rust** computed for what is currently typed. A refusal still arrives from
`TaskSchedule::parse` and is still shown verbatim; the browser gains no second parser and no cron
regex. Given `0 0 30 2 *`, the preview says it matches no instant, in Rust's own words.

**59.8** — Adding a folder offers Home as a choice, and a typed `~` resolves. Given a path that expands
to the home directory, nothing refuses it — and the form says once, plainly, what syncing a home
directory will pull in.

**59.9** — A task of the new kind runs a verb keeper already owns and records its outcome exactly as
`sync` and `release` do. `TaskKind` stays a closed vocabulary; a stored kind this build cannot read
is still listed-not-run (NFR-43), and the ARCHITECTURE deferral on arbitrary commands is left
standing and unweakened.

**59.10** — §14 describes the view as it now is — the levels, the run detail, what Run now means beside
the button, the description, the per-task delay and the new kind — and the paced-rows section keeps
the four standings it gained in 58.7.

## Design notes

**Why level 3 is a surface and not a bigger row.** Epic 58 grew the row from 57.6's five cells to ten
stacked blocks, and the owner's report is what that costs. Adding run detail to the same row would
repeat it. The app already has the master/detail idiom this needs — `files-pane.tsx` with
`PanelStrip`, `sessions-pane.tsx` with its detail — and `app-shell.tsx:336-344` renders `<TasksPane />`
alone with the comment *"No panel strip: a task is not a document, and there is nothing here to open
in an editor."* That comment is **scope, not teeth**: it argued against an editor, not against a
detail region, and a run report is exactly a thing to open. 59.1 may site the detail inside the pane
rather than in the panel strip, and should say which it chose and why.

**What `output` will and will not mean.** He asked for output. A `sync` or `release` task has no
stdout — it has a composed report, and `detail` is that report. 59.2 therefore ships *everything a run
records*, and the honest words for it are "report", not "output". Real captured stdout arrives only
with Epic 60's exec kind, where it needs a size bound and a truncation rule; promising it here would
be the over-claim epic 58's paced rows exist to avoid.

**The name he means already exists.** `validate_id` refuses only an empty or whitespace-padded id
(`tasks.rs:963-976`), so `nightly backup` is a legal id today and the row already renders it
(`tasks-pane.tsx:935`). What is missing is that the Add form hides that: it sends `""` to get a ULID.
59.5 owns the description; **59.1 owns making the form's id field read as the name it is**, and neither
may make an id editable after the fact — `task_runs.task_id` joins on it.

**What stays out of this epic, on purpose.**

- **The general exec kind** — Epic 60, threat model first. Named in the story list nowhere else so it
  cannot be smuggled into 59.9.
- **A task targeting an arbitrary path** — the owner chose the picker reading of the home-dir ask.
  A task addresses a profile or the host; changing that changes what every kind means and belongs
  with Epic 60's addressing work if it is ever wanted.
- **Sorting and filtering the task list** — 59.1 delivers the level that makes twenty tasks scannable;
  ordering beyond `list_tasks`' own is not asked for and a control nobody asked for is a control to
  maintain.
- **Editing an id.** Refused, and the reason is a join.
