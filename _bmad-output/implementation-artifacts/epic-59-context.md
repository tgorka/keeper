# Epic 59 Context: A task you can find, and a run you can read

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

The Tasks pane (⌘8) works, but nothing it does is findable: the task list and the execution history are fused into one flat, unbounded, unfoldable column of detail cards, and there is no level for a single execution at all. The owner's second Tasks-view pass reported seven asks; triage showed five of them are one defect — every capability is built and honest but none of it is reachable. Epic 59 is therefore mostly navigation (a master list of tasks, a detail region, a run surface you can open), plus three additive fields (description, per-task missed-window delay, a schedule-writing helper) and one closed-vocabulary task kind. It deliberately does not build the general "run any command" task kind — that is Epic 60, threat model first — and it does not touch `update`, which stays refused everywhere.

## Stories

- Story 59.1: a list of names, and one task at a time (master list + detail region split)
- Story 59.2: a run you can open (one execution as a surface)
- Story 59.3: the row says its mode, and Run now says what it does
- Story 59.4: one task at a time, and why not several (single-select, refusal with a reason)
- Story 59.5: a task you can name (optional description column)
- Story 59.6: how long is the delay (per-task override of the missed-window delay)
- Story 59.7: help for writing a schedule (Rust-computed preview of next instants)
- Story 59.8: home, in one click (Home choice in the folder picker, `~` accepted)
- Story 59.9: a task that runs a verb keeper already owns (TaskKind::Verify)
- Story 59.10: the chapter grows a view (docs/sync.md §14)

## Requirements & Constraints

- Wave 1 (59.1–59.4) is one file and one owner: all four stories rewrite `tasks-pane.tsx`. 59.1 lands first and alone; 59.2–59.4 may then run together. Wave 2 (59.5–59.7) is genuinely independent. 59.8 is the only story outside the Tasks surface; 59.9 is deliberately last of the code stories.
- 59.1 must not lose what the flat row got right: every fact epic 58 put on the row keeps its current wording — it is a re-siting, not a deletion. The unreadable-row case keeps its own section and explanation. No IPC changes in wave 1.
- A run has no stdout — it has a composed report. The honest word for it is "report", never "output"; real captured stdout belongs to Epic 60.
- An id may not be edited after creation (task runs join on it). The Add form hides that ids can be human-readable; 59.1 owns making the id field read as the name it is.
- Anti-poll invariant holds: reads happen on selection only, one fetch in flight, no timer, no frontend-imposed limit — Rust's own limit (20 shown of a 50-run store) is made visible, not replaced.
- The existing form-note mirroring guard shipped wrong once; 59.6 must re-point it at the per-task effective value rather than delete it.
- `update` stays refused absolutely, in all the places that refuse it.
- Out of scope on purpose: the general exec kind (Epic 60), a task targeting an arbitrary path, sorting/filtering the task list beyond its natural order, and editing ids.

## Technical Decisions

- Master/detail is already decided: the master column is the app's existing surface-column convention; the detail is a plain sibling region inside the tasks pane — a task is not a document and must never become a panel target. The code comment refusing a "panel strip" stays, amended so it does not read as refusing a detail region.
- 59.4 ships single-select and says why out loud: every task write in the stack is single-id (one optimistic baseline per write, scalar ids through all IPC verbs and the CLI), so a checkbox column would be state whose only action is a loop of N independent writes. Inventing a second selection idiom is forbidden; if bulk verbs are ever wanted, their first story is a batched verb with per-id receipts, and only then a selection model copied from the Files pane.
- The schedule dialect is exactly: 5-field cron; the `@hourly`/`@daily`/`@weekly` aliases (desugared to cron, never to an interval, so nightly keeps meaning night); intervals in s/m/h/d with long forms; or empty (no schedule). Floor 60 s, ceiling one year. A cron that parses but names no real date is refused at save time in constant time. Four distinct refusals — malformed, below floor, above ceiling, matches-no-instant — each quoting the text the person typed, not a lowercased copy.
- The schedule helper must reuse the recording-destination precedent: the clock and the renderer both belong to Rust. The browser gains no second parser and no cron regex; refusals arrive from the Rust parser and are shown verbatim. `nextDueMs` already exists on the view model, so the first instant is a read the pane already makes.
- A new description column is nullable, not `DEFAULT ''` — a task written before the column existed must read as having no description, not an empty one.
- `TaskKind` stays a closed vocabulary. 59.9 adds one variant that runs a verb keeper already owns — no arbitrary commands, no reopened security decision; a stored kind this build cannot read stays listed-not-run. The architecture's Deferred section on arbitrary user commands (egress, credentials, no task timeout, `NoNewPrivileges`) is left standing and unweakened.
- Consumed from Epic 57/58 architecture decisions: a task is a record in two tables, never the journal; a schedule is a due-gate on the host's existing tick, validated at save and leased at run time; which host runs a task is a platform fact the UI must not pretend otherwise; missed-window catch-up is exactly-once by construction and a policy governs the window without enumerating it; other paced work is projected read-only into the view, never migrated into the table; the one-clock-per-host rule is about clocks, not visibility.

## Cross-Story Dependencies

- Wave 1's four stories all rewrite the same file: 59.1 restructures it and 59.2–59.4 extend the result — they are not parallelisable across agents the way epic 58's wave 1 was.
- 59.2 builds on 59.1's detail region; 59.3 and 59.4 both state things where the 59.1 list leaves them.
- 59.5 and 59.6 each add one column and touch the same five places; 59.7 adds an IPC read and no column, so it is independent of both.
- 59.9's docs row and 59.10's chapter section both depend on the code stories ahead of them describing the view as it finally is.
