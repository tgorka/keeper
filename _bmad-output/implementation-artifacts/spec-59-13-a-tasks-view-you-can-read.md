---
title: 'Story 59.13: a Tasks view you can read, and a task you can create'
type: 'bugfix'
created: '2026-09-01'
status: 'done'
baseline_revision: '425ec18'
final_revision: '1964d32'
review_loop_iteration: 0
followup_review_recommended: false
warnings: ['oversized']
context:
  - '{project-root}/docs/project-context.md'
---

<intent-contract>

## Intent

**Problem:** Minutes after PR #310 merged, the owner reported *"tasks looks unusable now, also when
create new task - dont see it in the list"*. Story 59.12 mounted `<PanelStrip>` beside `<TasksPane>`
unconditionally (`app-shell.tsx:375`), and an **empty** strip claims a growth share plus its own
280px basis. Measured in a real browser against the real modules — the full shell, sidebar included,
`sync_tasks` answering an empty listing like the owner's own `sync.db` — the strip took **628 of
1024**, **702 of 1280** and **837 of 1550**, and the Tasks pane's detail region was left **28px**,
**102px** and **237px**. The add form renders inside that region (`tasks-pane.tsx:3049`), so at 1024
the form was **0px wide** with its narrowest control at **22px** and every note in it rendered at one
word per line (a 109-word paragraph over 109 lines). That is the second symptom: the form could not
be filled, and `select count(*) from tasks` on his machine is 0 with no save attempt in the log.

**Approach:** Make every region in the Tasks view a claimant with an honest floor, and stop the
empty strip from being a claimant at all. Four decisions, each argued in Design Notes: the strip is
mounted in the Tasks view only while some panel holds a target; the detail region gets a measured
floor and the pane claims the sum of its two columns' floors; the add form's rows wrap so a squeezed
row loses its label's line rather than its field's width; and the projected paced class rests behind
its own fold instead of drawing eight full-prose rows in a 320px column.

## Boundaries & Constraints

**Always:**
- Story 59.1's level 2 stays: the pane keeps its own detail region, and 59.12's lockstep (single
  click moves region and active panel together, only a double click parts them) is untouched.
- The Files, Sessions and Notes surfaces keep the strip they have. Their strip **is** their document
  area — an empty strip there advertises the only route to a document at all — and it must measure
  identically before and after.
- Every floor is a number with a reason, and the numbers live in one place each. The window minimum
  and the sum of the floors are connected by a test, not by hope (`window-minimum.test.ts`'s rule).
- 58.7's invariant holds: a projected row stays visibly not-a-task, carries no control, and the
  section still claims to be a complete inventory of what this host paces.
- No new IPC, no new read, no timer. The pane's one settled pass over two commands is unchanged.

**Block If:** nothing.

**Never:** no raising of `tauri.conf.json`'s `minWidth` (960) — the owner runs at 1024 and a window
minimum above his width would be a worse defect than the one being fixed; no stacking of the pane's
two levels; no auto-folding a column the user did not fold; no second panel-strip idiom; no deletion
of the pane's detail region; no removal of the double-click affordance without giving the gesture a
home.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Fresh Tasks view | Every panel `target === null` | No strip in the DOM; pane owns the surface; the open-beside gesture is advertised under the drawn task | No error expected |
| A task previewed or opened beside | Some panel holds any target | Strip mounted, ≥280px, and the pane keeps list ≥240 and detail ≥360 | No error expected |
| A file left open, then ⌘8 | A panel holds a `file` target | Strip mounted in the Tasks view — a target of any kind is a target | No error expected |
| Narrowest window with a panel open | 960px, sidebar collapsed | 48 + 240 + 360 + 280 = 928 ≤ 960; nothing overflows and nothing is clipped | Guard test fails if a floor moves |
| Add form at 1024 with a panel open | `adding === true` | Every control at its designed 224px; labels wrap onto their own line rather than shrinking the field | No error expected |
| Task created from the form | Submit with a typed id | Row appears in the list column and the region shows the new task | Refusals render where they already do |
| Host paces 8 things | `sync_paced_work` returns 8 rows | Three rows at rest and a *Show all 8* control; a row whose cadence is absent still says why | Refusal path unchanged |

</intent-contract>

## Code Map

- `src/components/layout/app-shell.tsx:336-376` -- the tasks branch and 59.12's retired-refusal
  comment. The strip is mounted here and nowhere else for this view.
- `src/components/layout/tasks-pane.tsx` -- `TASKS_COLUMN_CLASS` (list column), the pane root
  (`min-w-0 flex-1`, line 2754), the two-column row (2899), the detail `<section>` (3035-3037, no
  floor), the add-form card (3049-3070) and its recorded reasoning (3040-3048), `PacedWorkList`
  (1685-1794) and its `useFold(rows, { unfoldToAll: true })`.
- `src/components/layout/list-fold.tsx` -- `useFold`; `folded` is the global 10, which is why a
  projection of 8 rows renders whole and `FoldToggle` renders nothing.
- `src/components/layout/panel-strip.tsx:1055-1096` -- `PANEL_MIN_WIDTH_CLASS`, `PANEL_BASIS_CLASS`,
  and the comment explaining why the strip is a claimant. Unchanged; it is right for its own hosts.
- `src/components/sync/task-form.tsx:802-1048` -- ten rows of `flex items-center justify-between
  gap-2` with a `w-56` control each. The row is where a squeezed form loses its field.
- `src/lib/column-widths.ts:146-152` -- `tasks-list`: `defaultWidth: 320`, `minWidth: 240`.
- `src/lib/window-minimum.test.ts` -- the existing floor-vs-`minWidth` guard, written for Notes.
- `src/hooks/use-shell-layout.ts:4` -- `SIDEBAR_COLLAPSE_BREAKPOINT = 1080`, so below 1080 the
  sidebar is `FOLD_STRIP.widthClass` (48px) and at or above it is `w-[156px]`.
- `dev/probe/` -- the measuring harness this story is verified with; `dev/mock-shell.ts` answers it.

## Tasks & Acceptance

**Execution:**
- [ ] `dev/probe/{index.html,main.tsx}` -- the harness: renders the real `App` over the mock shell,
      drives real gestures, prints `PROBE key=value`. Committed, because the numbers in this spec
      are only checkable if the thing that produced them ships with it.
- [ ] `tasks-pane.tsx` -- export `TASKS_DETAIL_MIN_WIDTH_PX` (360) and `TASKS_PANE_MIN_WIDTH_PX`
      (derived: `columnMinWidth("tasks-list") + TASKS_DETAIL_MIN_WIDTH_PX`); apply them to the
      detail `<section>` and the pane root; amend the add-form comment; add
      `TASKS_OPEN_BESIDE_HINT` under the drawn task; name an empty task list in the column above the
      projection; pass the projection its own resting row count.
- [ ] `list-fold.tsx` -- `useFold` gains `foldedTo`, one number for a list whose rows are five lines
      tall. `FoldToggle` needs no change: its label already reads from the hook.
- [ ] `app-shell.tsx` -- mount the strip in the tasks branch only while a panel holds a target, and
      extend the retired-refusal comment with what the unconditional mount cost.
- [ ] `task-form.tsx` -- one row class constant, `flex-wrap`, and `shrink-0` on the controls.
- [ ] `window-minimum.test.ts` -- the Tasks clause, twice: collapsed sidebar against `minWidth`, and
      expanded sidebar against the collapse breakpoint.
- [ ] `app-shell.test.tsx` -- the strip's absence with no target and its presence with one.
- [ ] `tasks-pane.test.tsx` / `task-form.test.tsx` -- the structural guards listed in Verification.

**Acceptance Criteria:**
- Given the Tasks view at 1024, 1280 or 1550 with nothing open in a panel, when it is measured in a
  real browser, then no region renders a text block at one word per line and the detail region is at
  least `TASKS_DETAIL_MIN_WIDTH_PX` wide.
- Given the add form open at 1024 with a panel holding a target, when its controls are measured, then
  every control is at its designed 224px and the form is wide enough to hold one.
- Given the real "Add a task" trigger pressed, the real form filled and submitted against the mock
  shell, when the list re-reads, then the created task is a row in the list column.
- Given the Files and Notes views, when they are measured at the same three widths, then every number
  is identical to the pre-change measurement.

## Design Notes

**1. Why an empty strip is wrong here and right there.** The strip is `grow shrink basis-[280px]
min-w-[280px]` — deliberately a claimant, because in the Files and Notes surfaces it *is* the
document area, and Story 55.1 made it claim precisely so a note would stop receiving `surface - 560`
of whatever was left. Nothing about that is wrong. What is wrong is that the Tasks surface already
has a document area: Story 59.1's detail region. So the Tasks view is the one place where an empty
strip claims ~60% of the window to render a single sentence advertising a gesture, while the region
that draws the actual subject collapses. The rule is therefore local to this view and stated as a
condition on mounting, not as a change to `PanelStrip`: **in the Tasks view the strip is mounted only
while some panel holds a target.** A target of any kind counts — a file left open in the Files view
is still a document somebody asked to keep — because the strip's job does not depend on which
surface filled it.

Measured consequence at 1024 with an empty listing: detail 28 → 657. With a task open in a panel the
strip returns and the floors below decide the split.

**Where the gesture is advertised now.** `TASKS_PANEL_EMPTY_SENTENCE` — *"Double-click a task to open
it beside the list"* — was the only place the gesture was named, and it was named in a panel that
only exists once you have already performed the gesture. That is backwards, and it is why the
sentence cost 60% of the window to say nothing anyone needed yet. The gesture is now advertised in
the pane's detail region, under the drawn task: `TASKS_OPEN_BESIDE_HINT`, one muted line, rendered by
the **region** rather than by `TaskDetail`. Rendering it in the region is what makes it structurally
impossible for a task panel to advertise opening a task panel — the panel host passes no prop and
gets no hint, by construction rather than by a flag. It appears exactly where a reader looking at one
task might want two, and only when a task is drawn: not over the form, not over the empty state, not
over a multi-selection. The panel's own empty sentence stays: with two panels open and one emptied,
it is still reachable and still true.

**2. The floors, and why three columns do fit.** The pane root carried `min-w-0`, which is the
skill's own answer to why reading CSS keeps failing here: `min-width: 0` on the flex item that
*holds* the floor tells flexbox to ignore the floor and lets the children be squeezed to nothing.
The detail region had no `min-width` at all, so it was the one box in the row that could go to 28px.

Both are now claimants:
- detail region: `TASKS_DETAIL_MIN_WIDTH_PX = 360`. The hard floor is the add form: its widest
  control is `w-56` (224px), inside a `size="sm"` card (16px padding each side) inside the region's
  `m-6` (24px each side) — 304px before anything is readable. 360 buys the form's labels a full line
  of their own and the detail's `grid-cols-2` `dl` a cell that holds a value.
- pane root: `TASKS_PANE_MIN_WIDTH_PX = columnMinWidth("tasks-list") + TASKS_DETAIL_MIN_WIDTH_PX` =
  240 + 360 = 600. Derived rather than written, so the two cannot drift.

The rule, and the width it takes effect at: **nothing stacks and nothing auto-folds, because the
arithmetic fits.** At the app's own minimum window (`minWidth: 960`) the sidebar is collapsed — the
1080px breakpoint guarantees it — so 48 + 240 + 360 + 280 = **928 ≤ 960**. At the breakpoint itself
the sidebar is expanded and 156 + 240 + 360 + 280 = **1036 ≤ 1080**. Both are asserted. Stacking the
pane's two levels would invent a second layout for one surface, and auto-folding the list would take
away a column the user did not fold — the sidebar may do that (`app-shell.tsx:290-294`) because it is
chrome, not content. Where the arithmetic stops fitting is where a floor moved, and that is a build
failure rather than a user's discovery.

**3. The form gets its host by construction — and its rows learn to wrap.** The comment at
`tasks-pane.tsx:3040-3048` argued the form takes the region rather than sitting above the selected
task, because *"a form and a task's detail are two different answers to 'what am I looking at', and
720px of form does not fit in a 320px column"*. That reasoning is correct and is **kept**; what was
missing was that it assumed a region wide enough and nothing guaranteed one. The comment is amended
to say so and to name the floor that now makes it true.

The floor alone is not sufficient, though, and this is the second half of the measurement: a form row
is `flex items-center justify-between gap-2` with a label and a 224px control, and a flex item's
default `flex-shrink: 1` means the *control* is what gives. At 1024 the controls measured 22px. The
rows now carry `flex-wrap` and the controls `shrink-0`, so a row too narrow for label-beside-field
puts the label on its own line and the field keeps its designed width. **Sufficient width is "every
control at 224px and no label shredded"** — a form whose fields are narrower than the values they
take is unfillable whatever its outer box measures, which is exactly what 22px was.

**4. Tasks before the projection — measured, and the premise corrected.** The report was that *ALSO
PACED BY THIS HOST* renders above the tasks. Measured, it does not, at any width: `first_task_top` is
192 and `paced_heading_top` is 712 at 1550, and the projection is last inside the column's one
scroller exactly as `tasks-pane.tsx:3012-3020` says. Two other things are true and are what the
owner saw. First, with zero tasks the list column holds *only* the projection, so the projection is
what the eye lands on and there is nothing on screen saying the task list is empty — the full empty
state lives in the wide region by an earlier, correct decision. The column now names its own emptiness
in one short line above the projection, so the context cannot pose as the subject. Second, the
projection is **not** behind a fold in practice: `useFold`'s resting size is the global `folded` = 10,
the host paces 8 things, so all eight render whole and `FoldToggle` renders nothing at all
(`hidden === 0`). Measured height of that section in a 320px column: **1609px** — two screens of
context under half a screen of tasks. `useFold` gains `foldedTo` and the projection rests at three
rows with a *Show all 8* control. This is consistent with 58.7's own argument rather than against it:
that argument forbids capping the **expanded** view of a complete inventory, because a row dropped
there has no control left to reveal it. A resting count with a live control is what a fold is.

**5. What 59.12's verification failed to check, so this does not happen again.** 59.12 was verified
with a DOM probe that rendered the real modules, read the panels store, read the picker's option
values — and never called `getBoundingClientRect`. Every claim it made was true. jsdom performs no
layout, so the whole component suite is blind to width by construction, and the probe inherited that
blindness while looking like a browser check. A DOM probe answers *does this render*; only a measured
one answers *at what width*. The harness in `dev/probe/` is the corrected instrument, and it is
committed rather than described.

## Verification

**Commands:**
- `bun run test -- src/lib/window-minimum.test.ts src/components/layout/app-shell.test.tsx
  src/components/layout/tasks-pane.test.tsx src/components/sync/task-form.test.tsx` -- green.
- `bun run typecheck` -- clean. `bun run lint` -- 4 warnings + 1 info, exit 0 (the measured baseline;
  the `useTemplate` info at `markdown-preview.ts:424` is not this story's).
- `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync -p keeper-core -p keeper-syncd` --
  at or above 3846. `cargo clippy … --all-targets -- -D warnings`, `cargo fmt --all --check` -- clean.
- Mutation, each restored and the restore verified by reading `git diff`: drop the detail region's
  floor, drop the pane's floor, mount the strip unconditionally, remove `flex-wrap` from the form
  row, move the projection above the task list — each must turn a **named** test red.

**The structural guards (what CI can own).** jsdom lays out nothing, so CI cannot own a width. It
owns the facts the widths depend on:
1. `window-minimum.test.ts` -- collapsed sidebar + list floor + detail floor + one panel ≤
   `minWidth`, and expanded sidebar + the same three ≤ `SIDEBAR_COLLAPSE_BREAKPOINT`.
2. `app-shell.test.tsx` -- no strip in the Tasks view while every panel is empty; a strip once one
   holds a target.
3. `tasks-pane.test.tsx` -- the detail region carries `TASKS_DETAIL_MIN_WIDTH_PX`; the pane root
   carries `TASKS_PANE_MIN_WIDTH_PX`; the add form's host is the detail region; the projection
   follows the task list in document order; the projection rests at three rows.
4. `task-form.test.tsx` -- every form row wraps and every control refuses to shrink.

**The numbers themselves are not guarded, and cannot be.** No CI on this project runs a browser.
What CI guards is the *shape*; a person re-measures with one command:

```sh
# on a host with a browser, from the repo root — three commands, all committed
bun x vite --port 8133 --host 127.0.0.1 &     # serves the real modules + mock shell
bun run dev/probe/collector.ts &              # receives the probe's beacons
dev/probe/measure.sh after "view=tasks&act=create&tasks=none"    # 1024, 1280, 1550
dev/probe/measure.sh beside "view=tasks&act=beside-add&tasks=fixture"
dev/probe/measure.sh files "view=files&act=measure"
```

`dev/probe/main.tsx`'s module doc carries the parameters and why
`--virtual-time-budget` cannot be used on this host. Two traps already paid for: a harness entry
outside the repo root fails under rolldown-vite with `Missing field 'moduleType'`, and a long-lived
vite on the measuring host wedges (2.4GB RSS, port closed, process alive) — if every width reports
`NO RESULT`, restart vite before believing anything.

## Measured — before and after, real browser, real modules

The full shell, sidebar included, at the three widths. The sidebar is 48px below 1080 and 156px at
or above it, which is why the surface differs from the window. `sync_tasks` answers an empty listing,
which is the state the owner's own `sync.db` was in.

**Nothing open in a panel** (the state the owner met):

| | pane | list | detail | strip | add form | narrowest control |
|---|---|---|---|---|---|---|
| 1024 before | 349 | 320 | **28** | **628** | **0** | **22** |
| 1024 after | 976 | 320 | **656** | *absent* | **624** | **224** |
| 1280 before | 423 | 320 | 102 | 702 | 70 | 22 |
| 1280 after | 1124 | 320 | 804 | *absent* | 688 | 224 |
| 1550 before | 558 | 320 | 237 | 837 | 205 | 32 |
| 1550 after | 1394 | 320 | 1074 | *absent* | 688 | 224 |

**A task open beside** (the three-column case the floors exist for; after only — before, the strip
was there whether or not it held anything, so the before column is the table above):

| | pane | list | detail | strip | add form |
|---|---|---|---|---|---|
| 1024 | 648 | 287 | 360 | 328 | 328 |
| 1280 | 722 | 320 | 401 | 402 | 369 |
| 1550 | 857 | 320 | 536 | 537 | 504 |

Every region is at or above its floor at every width, and list + detail equals the pane (287+360=647
of 648) — the arrangement in which 80px of the detail region was laid out past the pane's own right
edge is what the list column's `shrink-0` was doing, and it was found by this table and not by
reading.

**Sufficient width for the form, named.** 224px per control and no label shredded. 224 is `w-56`,
the width all ten controls are designed at; the widest label measures 125, so a row needs 357 plus
the card's 32 and the region's 48 — 437 — to keep label and field on one line, and below that the
row wraps and the field keeps 224. The hard floor is 224+32+48 = 304, and the region's 360 clears it.
At 1024 with a task open beside, the form is 328 wide with every control at 224: fillable at the
narrowest arrangement the app can produce.

**One word per line, as a number.** The probe flags any text block of 4+ words rendered in more than
`words / 1.6` lines. Before: at 1024 the pane reported five shredded blocks including *No tasks yet…*
at **34 lines for 34 words**, and the open form reported ten more including a 109-word paragraph at
**109 lines**. After: `shredded=none` at 1024, 1280 and 1550, at rest, with the form open, and with
a task open beside. Two of the before-blocks were 58.7's projected cadence cells and were **not** the
strip's fault: `grid-cols-2 sm:grid-cols-4` put four tracks in a 320px column, because `sm:` is the
window's breakpoint and the box is a column. Fixed here too.

**The created task.** `create.row_in_list=yes` at 1024, 1280 and 1550: the probe presses the real
*Add a task* trigger, types an id into the real field through the native value setter, presses the
real *Add task*, and finds the row in the list column afterwards. The harness knob that answers an
empty listing filters the mock's own state to what this session created rather than answering `[]`
unconditionally — the first version did the latter and "proved" the symptom unfixed when it was the
harness that could not see the row.

**Files and Notes, unchanged.** Files strip 616 / 764 / 1034 and tree 360 at the three widths, before
and after, identically. Notes strip 416 / 564 / 834, rail 240, list 320, before and after,
identically. Neither surface's branch was touched.

**Mutation proofs.** Each mutation applied to the committed fix, each reverted, and each restore
verified by reading `git diff` — which is what caught the run that mattered. The first attempt was
made against an **uncommitted** tree, and `git checkout -- <file>` there restores HEAD rather than
the fix: from the second mutation onwards the mutations were being applied to `origin/main` and the
red tests proved nothing but their own absence. The tell was the `git diff` at the end coming back
empty, which reads as *restored* and means *the work is gone*. Never mutation-test uncommitted code.
The numbers below are from the re-run, against `1964d32`:

| mutation | result |
|---|---|
| drop the detail region's floor | 1 failed |
| drop the pane's basis and floor | 1 failed |
| mount the strip unconditionally | 1 failed |
| unwrap the form rows | 2 failed |
| move the projection above the task rows | 2 failed |
| give the projection back the global resting size | 2 failed |
| restore `shrink-0` on the list column | 1 failed |
| raise the detail floor past what the window minimum can hold | 2 failed |

## Auto Run Result

Status: done

**What shipped.** Four decisions, each measured rather than argued. (1) In the Tasks view the panel
strip is mounted only while some panel holds a target, because that surface already has a document
area and an empty strip there claimed ~60% of the window to advertise a gesture; the gesture moved to
`TASKS_OPEN_BESIDE_HINT` under the drawn task, rendered by the region so a panel cannot advertise
itself. (2) The detail region got a floor built from the add form's own widest control, the pane
became a claimant with a basis and a floor derived from its two columns, and the list column lost the
`shrink-0` that made the pane's real floor its remembered width. Nothing stacks and nothing
auto-folds: 48 + 240 + 360 + 280 = 928 fits the app's own 960px minimum, and a test asserts that
arithmetic on both sides of the sidebar's 1080px breakpoint. (3) The form's rows wrap and its
controls refuse to shrink, so a narrow form is taller rather than unfillable. (4) The projected paced
class rests at three rows instead of at the global ten — at ten it folded nothing on any real host —
and its two prose cells stopped being laid out in four tracks inside a 320px column.

**Files changed.** `app-shell.tsx` (+ test), `tasks-pane.tsx` (+ test), `task-form.tsx` (+ test),
`list-fold.tsx`, `use-shell-layout.ts`, `window-minimum.test.ts`, and `dev/probe/` — the harness,
committed, because the numbers above are only checkable if the instrument ships with them.

**Verification.** Frontend `bun run test` → 302 files / 5179 tests passed (baseline 5165, +14
guards). `bun run typecheck` clean. `bun run lint` → 4 warnings + 1 info, exit 0 — the measured
baseline. Rust `cargo test -p keeper-sync -p keeper-core -p keeper-syncd` → 0 failed; `cargo clippy`
on the same three crates with `-D warnings` clean; `cargo fmt --all` applied and changed nothing. No
Rust source and no shell-crate symbol was touched, so the macOS gate has nothing new to see.

**What Story 59.12's verification failed to check.** It was verified with a DOM probe that rendered
the real modules over the real mock shell and answered truthfully — and never once called
`getBoundingClientRect`. It read the panels store and the picker's option values, so it could prove
*this renders* and could not ask *at what width*. jsdom performs no layout, so the entire component
suite is blind to width by construction and the probe inherited that blindness while looking like a
browser check. A green suite plus a DOM probe is therefore not evidence about layout at all, and a
regression that took 60% of the window walked through both. The instrument that would have caught it
is `dev/probe/`, and it is committed rather than described.

**Residual risks.**
- The numbers are not in CI: no gate on this project runs a browser. What CI owns is the shape —
  eleven guards across four files, each mutation-proved above — and the spec carries the command
  that re-measures.
- Not verified on a running app: the `keeper` shell crate does not link on this Linux host. The
  measurements are of the real frontend modules and the real compiled CSS in a real browser, over
  `dev/mock-shell.ts` rather than over Rust.
- The strip's absence in the Tasks view is a mount condition, so a panel a reader closes down to
  empty takes the strip away with it. That is the same rule stated forwards, and the last panel
  cannot be closed, so no target ever becomes no strip by accident.
