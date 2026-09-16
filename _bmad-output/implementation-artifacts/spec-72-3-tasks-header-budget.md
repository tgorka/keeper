---
title: 'The Tasks header has a budget, and its prose moves into a hint'
status: review
baseline_revision: a8eb9c2
final_revision: ''
---

<intent-contract>

## Problem
Selecting a task mounts the panel strip while adding an unsqueezable word-button cluster to the Tasks header. The explanatory prose absorbs the entire shortfall. A task waiting for its first window also misleadingly reads `never run`.

## Approach
Adopt the shared PaneHeader and PriorityActions budget. Keep Add and Refresh as compact fixed icon controls (preserving add-form focus restoration), promote bulk actions only when they fit, and keep their overflow menu reachable. Move explanatory prose into the shared IconHint beside Tasks. Render a future first window using formatTaskDue.

## Always
- Selection must not change the header identity's width classes.
- Every bulk verb remains reachable and respects in-flight write disabling and destructive confirmation.
- The add form remains the detail region's exclusive content while open, not a competing sibling card.
- Preserve the existing empty-state creation guidance and both explanatory sentences.

## Block If
A shared API change requires agreement with its owner, or a fix would require wave-2 task-form changes.

## Never
No Rust policy, schedule changes, automatic folding, new width state, task-form edits, or changes to deliberate double-detail behavior. jsdom is not a layout engine.

## I/O & Edge-Case Matrix
| State/input | Observable result |
| --- | --- |
| No selection / loading / empty listing | Stable Tasks identity; Add and Refresh reachable; no bulk verbs |
| Selection with zero measured budget | Bulk verbs reachable through overflow; identity classes unchanged |
| Bulk write outstanding | Both promoted and overflow controls disabled; no second write |
| Info focused, readable tasks exist | Subtitle and Run now explanation reachable in hint, absent from flow |
| Info focused, empty listing | Subtitle reachable; no irrelevant Run now explanation |
| No run, future window | `not yet — first window <formatTaskDue>` in last-run and last-outcome cells |
| No run, absent or elapsed window | `never run` |
| Run exists, future window | Existing actual run/outcome wins |
| Pane at its declared floor | Fixed controls plus gaps and padding fit beside identity minimum; arithmetic guard |

</intent-contract>

## Code Map
- `src/components/layout/tasks-pane.tsx:2888-2989`: old hand-rolled header and bulk controls.
- `src/components/layout/tasks-pane.tsx:929-971,1623-1626`: relative times and absent-run fields.
- `src/components/layout/tasks-pane.tsx:2242-2249`: add-trigger focus restoration.
- `src/components/layout/pane-header.tsx:170-191,251-383`: shared measured budget (consumed, owned by Hints).
- `src/components/layout/priority-actions.tsx:167-190,217-331`: promotion and overflow (consumed, owned by Hints).
- `src/components/layout/tasks-pane.test.tsx:625-674,2414-2429,3432-3494`: copy, hint and layout contracts.
- `src/lib/window-minimum.test.ts:68-87`: arithmetic guards derived from real floors.

## Tasks & Acceptance
- [x] Adopt shared header, budgeted actions, and info hint.
- [x] Render future first-window wording without overriding actual runs.
- [x] Extend behavioral and arithmetic guards; run scoped tests.
- [x] Record browser probe handoff and design decisions.

**Acceptance (verbatim):** a test derived from the constants fails when the header's unsqueezable cluster exceeds `pane floor − identity minimum`; a test proves that selecting a task changes no width class on the header identity; the two sentences are still reachable (now in the hint) and their existing wording assertions move with them; a test proves a never-run row with a future window reads "not yet" and a never-run row with no window still reads `never run`; measured in `dev/probe` at 1024 and 1500 px with a task selected — no element narrower than its floor, nothing clipped.

## Design Notes
The pane now uses the same 40px PaneHeader band as other surfaces. Tasks, the info icon, and the selection count occupy the identity group; its width classes do not change on selection. Count remains an announced live status. No prose is a layout member.

Add and Refresh stay fixed 32px icon controls. This preserves Add's existing focus-restoration ref and in-flight-save disabling while reducing their horizontal cost. The third fixed control is the overflow trigger; the three bulk actions promote in Enable / Disable / Forget order through PriorityActions, with the same handlers in overflow. Destructive confirmation is unchanged. The menu also keeps Add and Refresh reachable with named text. Hints agreed and implemented optional `PriorityAction.disabled`; this slice consumes it and applies the same disabled state to overflow items. No file owned by Hints was edited by this slice.

The fixed group costs 112px (three 32px controls and two 8px gaps). At the 600px pane floor, 48px horizontal padding leaves 552px content; the shared 160px identity minimum and 8px inter-group gap leave 384px for actions. The arithmetic guard derives these values from rendered constants, not a second handwritten floor. Bulk promotions are charged against the remainder by the shared planner. This is arithmetic, not a browser measurement.

Both sentences are the info control's accessible name and its full hint label. The subtitle is available while loading and empty; the Run-now sentence joins it only once readable task rows exist, preserving the prior relevance gate. Focus drives the real shared hint in the component test. Empty-state CLI/copy assertions remain unchanged because that content neither moved nor became false. No existing test was deleted; the old in-flow Run-now reachability/wording test now reads the hint.

Future-first-window wording applies to both Last run and Last outcome when no run exists, avoiding a contradictory `not yet` / `never run` pair. A recorded run wins regardless of the next due date; absent, due-now, and past windows keep `never run`. The pane's existing clock and formatTaskDue own all relative-time behavior.

The add form already exclusively replaces detail content inside the floored region; its existing containment test passes. No task-form or deliberate double-detail behavior changed. The real-browser gate may still reveal unrelated detail-header/grid pressure; this slice does not claim jsdom ruled that out.

## Verification
### Scoped commands actually run

`bunx vitest run src/components/layout/tasks-pane.test.tsx src/lib/window-minimum.test.ts`

- Baseline after implementation: **2 files passed, 128 tests passed** (123 Tasks tests, 5 window-minimum tests), exit 0; duration 21.27s.
- Mutation run of the same command: exit 1, **3 expected failures**. Enlarging the real fixed controls to 220px failed the arithmetic guard (`expected 676 to be less than or equal to 384`); replacing the first-window sentence with `never run` failed the future-window consumer test; dropping Run now from the hint failed the hint reachability test. No sibling files were mutated.
- All three anchors restored exactly; the source snapshot returned to the same pre-mutation tool hash `A364`. No git commands were used.
- Restored run: **2 files passed, 128 tests passed** (123 + 5), exit 0; duration **10.82s**. After avoiding unused first-window string construction for tasks that already ran, the final identical command again passed **128/128**, exit 0, duration **16.92s**.

### Coordinator's real-browser gate — NOT RUN by this slice

Start the repository Vite server and collector on a browser-capable host (using supervised process launches):

```
bun x vite --port 8133
bun run dev/probe/collector.ts
dev/probe/measure.sh epic72-tasks-header \"view=tasks&act=beside-add&tasks=fixture\" 1024 1500
```

Prerequisites identified and handed to Main: the existing probe locates Add by button text; the new icon needs its `aria-label` lookup. The current `beside` path dispatches only `dblclick`, so the probe must first click the task row to establish selection. It must additionally report the header identity/actions rectangles and clipping; the old probe's pane/detail measurements alone do not prove this story.

Required output at both widths: selected task and panel strip mounted; pane ≥ `TASKS_PANE_MIN_WIDTH_PX`, list ≥ `columnMinWidth(\"tasks-list\")`, detail ≥ `TASKS_DETAIL_MIN_WIDTH_PX`, panel ≥ its declared floor; header identity ≥ `PANE_HEADER_IDENTITY_MIN_PX`; action group within its computed budget, overflow trigger in view, no clipped element; Add opens the exclusive detail-region form without violating those floors. Focus/hover the info icon and open the overflow menu to check both explanatory sentences and bulk action reachability on the actual surface.

This real-browser measurement is **owed by the coordinator and was not run here**. jsdom performs no layout. No repository-wide gate, formatter, Rust build, package operation, or git operation was run by this slice.

## Shipped in

PR #363 of stack #364 (epic 72), branch `epic72/tasks`. The macOS gate (`bun run check:rust:macos`) passed on hesperia over the stack tip, which is where the `keeper` shell crate compiles at all.
