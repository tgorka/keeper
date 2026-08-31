# Epic 58 Context: A task you can drive, and a window that passed while nobody was home

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Epic 57 shipped a complete scheduled-task backend — save, forget, history, per-run detail — with no door into it: those commands are implemented, registered, typed, wrapped in TypeScript and mocked, and have zero production callers, while the Tasks view (⌘8) can only list, inspect and run. Epic 58 makes the mechanism drivable from the app, gives each task an explicit policy for a window that passed while its host was absent (run now, run after a delay, or skip), makes a declined window an observable fact rather than a silence, closes the one real double-run hole, and projects the other periodic work each host already paces into the same view as a read-only class. It stacks on Epic 57, which is not merged.

## Stories

- Story 58.1: A task you can create and edit
- Story 58.2: The row says what the run said
- Story 58.3: A list of runs you can open
- Story 58.4: A window that passed while nobody was home (the three-way missed-window policy)
- Story 58.5: A window nobody ran is still a fact
- Story 58.6: Two triggers, one missed window, one run
- Story 58.7: Everything else this host paces
- Story 58.8: A sync task that governs instead of duplicating
- Story 58.9: The chapter grows a policy (docs)

## Requirements & Constraints

- A task can be created, edited and deleted from the app: one component in two modes, the backend's refusal shown verbatim, a confirm before a record is forgotten.
- A row states what the run itself reported, not merely that it ended; a run with no detail renders as absence, never as an empty string.
- A task's runs are openable as a bounded, newest-first list carrying outcome, time, host and detail per run.
- Each task carries a missed-window policy, writable from the CLI **and** from the app **in the same story**, defaulting to today's behaviour so no install changes meaning on upgrade.
- A declined window is recorded as a closed run with its own outcome, so *skip* and *delay* move the last-run line instead of going quiet.
- One missed window yields one run even when a daemon and an external timer both drive the task on one host; a deliberate manual run on a task that is *not* overdue keeps working unchanged.
- The view also shows the other work this host paces, read-only, with its real cadence — no schedule editor, no Run now.
- A sync task governs the profile's existing pacing rather than adding a second driver beside it.
- No policy setting may enumerate more than one missed window, and none may decline a window without leaving a record.
- Nothing this epic adds registers a clock, a due-gate or a second pacer.
- A failed read is a fault to report, not a fact to invent: show the refusal, keep the last good data.

## Technical Decisions

- **Exactly-once catch-up already exists; do not rebuild it.** The stored next-due instant is a single scalar, not a queue, so overdue-by-one and overdue-by-two-hundred are the same state; the window is recomputed from a run's *finish*, never enumerated. Correctness across sleep comes from the wall-clock contract, not a wake handler. A policy may *govern* that window, never model missed windows as a backlog.
- **Wave 1 is frontend-only:** no Rust, no schema, no new IPC, no new architecture decision. Create sends a blank id so the store mints the identifier; edit sends the row's id verbatim and seeds from the view model the pane already holds, with no extra read. Edit is not offered on a row whose kind this build does not understand — the store would refuse the write, so the control could only fail.
- **Validation stays in Rust.** Id, schedule floor/ceiling and dialect, scheduled-with-no-schedule, and the stored-row guard all refuse rather than coerce. The form re-implements none of them, does not trim input, and carries no client-side cron regex.
- **Policy storage is one additive, defaulted text column** on the tasks table, added through a newly written additive task-column migration on the shape of its existing siblings (read table info, drop that statement before executing on the same connection) and called beside them in the migrator. The default is mandatory, not tidy: the upsert names its columns. *delay* needs no second column — lateness is derivable from the stored window.
- **The policy is decided in the pure decision function, not at the claim.** The claim's due condition passes throughout a delay window, and a *requested* trigger bypasses it entirely, so a delay enforced at claim time is no delay. The action enum gains a variant so every host's exhaustive match is forced to decide; *skip* must **re-arm** rather than return "nothing to do", or the past window stands and the task reports itself scheduled while nothing runs. The first-sight arm statement cannot be reused for that write.
- **A declined window needs a new outcome spelling.** The three existing non-success outcomes all presuppose a host was present and reached the task, and the retry-soon one is consumed to retry within a minute — reusing it would silently turn *skip* into *retry*. Unreadable policy or outcome spellings are skipped and listed, never fatal.
- **The double-run mechanism:** a requested trigger passes no upper bound on the due instant and so skips the claim's window condition, while the tick claims the same past window as scheduled. Both claims succeed. Reachable in normal operation, and Linux-only — no daemon host exists on macOS.
- **Projected work is projection, never migration.** Work with its own standing look-gate cannot be driven by a task row. Only three paced items have a real identity and cadence: the per-profile scan, the hourly scratch sweep, the notes cadence. No task row is written for any of them; the notes cadence stays deferred.
- **Governance is the only sanctioned way a task row may drive pre-existing paced work:** fold every row for a target into one least-permissive mode over an explicitly spelled rank, and *modulate the existing gate rather than adding a driver beside it* — the schedule drives it **and** the success edge keeps working. The cost of getting this wrong is not a corrupt repository (both routes reserve, and the lease serializes hosts) but duplicated work and a lying run record.
- **Visibility is not a clock.** The one-clock-per-host rule constrains schedulers over a repository; a read-only projection registers none. A source scan asserts exactly one interval in the shell crate — add none. Relative times render from the pane's existing display clock.

## UX & Interaction Patterns

- The form is **one component in two modes**, mounted **inline** in the pane header and empty state rather than in a dialog, following the existing add-folder form: mode derived from whether a subject was passed, seeded **once** (re-syncing from the prop would overwrite typing), refusals rendered verbatim.
- Enabled and mode are **separate controls** — two questions. Kind and mode vocabularies that no IPC enumerates ship as frontend constants in the existing directions-constant pattern. The profile picker comes from live profiles plus an explicit whole-machine option; an unmatched profile is the one thing the backend does not refuse and comes back as unhosted.
- Deletion uses the existing destructive-confirm dialog idiom and says it deletes a record, never content.
- The run list extends the existing activity-list component rather than a third list idiom: caps heading, null-versus-empty distinction, fold-based truncation, unknown-kind fallback, and the CLI's settled column set and empty-state wording.
- The three empty-state constants that currently say creation is impossible are rewritten, with their assertions, in the same change as the button that makes it possible.
- The projected class is visually distinct, states it is *paced, not scheduled*, and its scan row states that two of its three triggers are filesystem events. Its absent Run now and schedule editor are asserted as negatives.

## Cross-Story Dependencies

- **Wave 1 (58.1–58.3) is disjoint and parallelisable**, colliding only in the Tasks pane file — one story owns it, the others coordinate. 58.1 landed first and widened the row component with editing/deleting/writing props and a per-row edit disclosure, so 58.2 and 58.3 extend that larger row.
- **58.4 → 58.5 is a strict chain:** the recorded outcome is what makes the policy observable; the policy alone is the invisible-non-execution shape. 58.4 also closes Wave 1's lost update with a baseline compare-and-set on the store-side write — same table, same write path, same migration. A frontend-only story could not do it, and a UI-only mitigation still loses the race.
- **58.6** depends on 58.4 only for vocabulary; the hole exists today, but the fix belongs after the policy makes "which trigger may consume a window" readable.
- **58.7** depends on nothing in Wave 2. **58.8 depends on 58.7** — the projected class is where a governed sync task must appear once it stops being a second driver.
- **58.9 last:** a documented policy whose form control does not exist is worse than no documentation.
- Recurring pitfall: proving a claim through a pure function while the risk lives in the impure shell. 58.4 must prove a manual run *during* a delay does not run; 58.6 must drive **both trigger kinds against one overdue window** (not two connections against one lease, which proves something else); 58.5 and 58.7 must assert negatives.
