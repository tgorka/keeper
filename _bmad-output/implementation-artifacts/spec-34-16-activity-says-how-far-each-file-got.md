---
title: 'Activity says how far each file got'
type: 'feature'
created: '2026-07-29'
status: 'review'
baseline_revision: '1be95be'
---

<intent-contract>

## Intent

**Problem:** The Sync view's Activity list said what happened to a file **on this disk** and
nothing about whether the remote ever heard. A row carried a kind glyph (added, changed, deleted,
conflict copy), the path, the size and the age — four facts, all local. Whether the file arrived
was not among them.

That is a gap with a name. Delivery *is* recorded, in the work journal: a commit queues one
`lfsUpload` unit per over-threshold object and a `push` unit publishes the commit, and those rows
carry `state`, `attempts` and `last_error`. The information existed and the file list did not
consult it.

The consequence was measured in the field, on a host syncing to a self-hosted Forgejo. Three
`lfsUpload` units sat parked with `authentication rejected for electra…`, for objects of
70,843,648, 15,654,529 and 9,242,833 bytes. What the user could see was the Problems section
reporting `Large file upload · stopped after 7 attempts` — three times, identically, **naming no
file**. `syncParkedSummary` composes that line from `PARKED_KINDS[unit.kind]` and the attempt
count, and a journal unit knows its oid, not its path. Meanwhile the Activity list showed
`keeper-rec 2026-07-17 11.18.33.mp4` as a plain "Added" row, indistinguishable from a file that
had arrived. So the two halves of one answer — *which file* and *why it did not arrive* — were on
screen simultaneously, in different sections, with nothing connecting them, and the user's own
report of the defect was "the pointer is created in the repo but not uploaded".

**Approach:** Give each activity row the id of the journal unit whose success delivers it, and
read the delivery answer back out of the journal on every list. An LFS-tracked path names its own
upload; every other path names the push that publishes the commit, because git publishes a commit
whole or not at all. `activity` grows a nullable `unit_id` through the same additive migration
`size_bytes` uses, and `list_activity` `LEFT JOIN`s the journal to project a `DeliveryState`.

Nothing about delivery is stored. The state is a function of a journal row that changes underneath
it — a unit is claimed, fails, is retried, parks, or completes and is deleted — so writing an
answer down would mean writing it again on every one of those transitions, and getting one wrong
would leave a row asserting an outcome the journal disagreed with. The join costs one indexed
lookup per row and cannot drift.

The reason then goes where the file is. A row with anything recorded against its unit opens a
popover holding the path, the delivery word and the engine's message verbatim, and — only where
keeper has actually stopped — a Retry that drives the same `unpark` the Problems section does, for
the same unit, named by the file a human recognises.

## Boundaries & Constraints

**Always:** the delivery answer is derived from the joined journal row at read time, never
persisted. `unit_id` is written once, by the commit that records the row, and never updated. The
join is keyed on `journal.id` and additionally scoped to the same `profile_id`; ids are safe to
compare because `journal.id` is `INTEGER PRIMARY KEY AUTOINCREMENT`, which SQLite guarantees is
monotonic even across deletes, so a completed unit's id is never reissued to a different unit and
an old activity row can never find a stranger wearing it. Every user-visible string in the row is
an exported constant, so the tests assert against the same text the UI renders. The engine's
message is rendered verbatim: a reworded git or LFS error is one nobody can search for.

**Block If:** (none) — this story adds a column to a read that already existed and a glyph to a
row that already rendered; there is no state in which it must refuse to answer.

**Never:** do not report a delivery state for a row no unit is accountable for. `unknown` renders
no glyph at all — not a placeholder, not a neutral dot, not a reserved gap — because a row that
predates the column, or a conflict copy the merge just wrote, has no delivery fact and an icon
would invent one. Do not offer Retry for a unit that is still being retried; do not offer it for a
unit that has completed and left the journal, which is why `unit_id` is suppressed in that case.
Do not render a `deferred` unit's `last_error` as a failure.

## I/O & Edge-Case Matrix

The journal row is the input. `attempts` is incremented by `claim_ready` at claim time, so
"pending and already attempted" is exactly "it was claimed once and did not get through".

| joined journal row | `DeliveryState` | `failure` | `unitId` | row renders | Retry |
| --- | --- | --- | --- | --- | --- |
| `unit_id` is `NULL` | `Unknown` | `null` | `null` | kind glyph only, no delivery glyph | no |
| no row for `unit_id` (unit completed and was deleted) | `Success` | `null` | `null` | `CircleCheck`, muted, "Reached the remote" | no |
| `state = 'running'` | `InProgress` | last error if any | the id | `CircleDashed`, muted, "On its way" | no |
| `state = 'pending'`, `attempts = 0` | `InProgress` | `null` | the id | as above | no |
| `state = 'deferred'` | `InProgress` | the wait reason | the id | as above; popover carries the reason, toned as the state | no |
| `state = 'pending'`, `attempts >= 1` | `Failed` | the error | the id | `CircleAlert`, destructive, "Failed, still retrying" | no |
| `state = 'parked'` | `Abandoned` | the error | the id | `CircleSlash`, destructive, "Stopped retrying" | **yes** |
| a `state` this build does not recognise | `Unknown` | — | — | no glyph, and a `debug` log | no |
| `kind` this build does not recognise | row skipped entirely | — | — | — | — |
| `size_bytes` is `NULL` | unaffected | — | — | no size rendered, never `0 B` | — |

</intent-contract>

## Code Map

**`keeper-sync/src/db.rs`** — `DeliveryState` (with the mapping table above as its doc comment)
and `DeliveryState::from_journal`, which reads one `state`/`attempts` pair. `ActivityEntry`
replaces the `(kind, path, size)` tuple `record_activity` took: four positional fields, two of
them optional integers, is a call site nobody can reorder safely. `ActivityRow` gains `delivery`,
`failure` and `unit_id`. `ensure_activity_columns` replaces `ensure_activity_size_column` and adds
both late columns from one `PRAGMA table_info` read — the read is the expensive half, and a second
copy of the loop is how the two would drift. `list_activity` carries the `LEFT JOIN` and the
three-way match on `(unit_id, state)`.

**`keeper-sync/src/engine.rs`** — `record_commit_activity` builds the entries and decides which
unit each path names, from a `HashMap<PathBuf, i64>` of LFS uploads with `push_unit` as the
fallback. `commit` enqueues the uploads **before** recording activity and threads `push_unit`
through `commit_local` and `execute`. `do_pull` and `sync_once` pass `None` deliberately. The
conflict-copy rows written in `do_pull` also pass `None`.

**`keeper-sync/src/db.rs`, `enqueue_unique`** — now returns the id of the unit that covers the
work, freshly created or already queued, instead of an `Option` every one of its four callers
discarded.

**`keeper/src/sync_ipc.rs`** — `SyncActivityVm` gains `delivery`, `failure` and `unitId`;
`delivery_str` writes the wire spelling out by hand, for the same reason `activity_kind_str` does.

**`src/components/layout/sync-pane.tsx`** — `SYNC_DELIVERY_STATES` maps each value to an icon, a
tone and a screen-reader word; `SyncDeliveryMark` renders the glyph, and wraps it in a `Popover`
only when `failure` is non-null. `SyncActivityList` takes `busy` and `onRetry` exactly as
`SyncProblemsSection` does, and `SyncProfileCard` passes one hoisted `retryUnit` to both.

## Tasks & Acceptance

**Execution:**

1. Add `unit_id` to `activity` through `ensure_activity_columns`, keeping every existing row.
2. Add `DeliveryState` and derive it in `list_activity` from a `LEFT JOIN` on the journal.
3. Change `record_activity` to take `ActivityEntry`, and `enqueue_unique` to return the covering
   unit id.
4. Enqueue the LFS uploads before recording activity, and link each recorded path to its unit.
5. Thread the executing push unit's id from `execute` down to `commit`.
6. Project the three new fields through `sync_ipc`.
7. Render the delivery glyph, the popover and the conditional Retry.

**Acceptance Criteria:**

1. An install whose `activity` table predates `unit_id` upgrades in place, keeps every capped row,
   and those rows read back as `Unknown` with no delivery glyph — not as `Success`.
2. A file whose LFS upload is queued reads `inProgress` and names that upload's id, not the push's.
3. A file whose delivering unit has completed reads `success` and offers no unit id.
4. A file whose unit is parked reads `abandoned`, carries the engine's message verbatim, and offers
   Retry; activating it calls the existing retry command with that row's unit id and no other's.
5. A file whose unit failed and is being retried reads `failed`, shows the message, and offers **no**
   Retry.
6. A file whose unit is deferred reads `inProgress` and shows its wait reason without destructive
   tone.
7. A conflict copy, and any row with no unit, renders the kind glyph alone and none of the delivery
   words.
8. The Problems section still lists every parked unit.

## Design Notes

**Derived, not stored, and the join is what makes that cheap.** The alternative was a `delivery`
column written by whatever code path changed the unit's state. There are five such paths
(`claim_ready`, `reschedule`, `complete`, `unpark`, `recover_running`), none of them currently
knows anything about activity rows, and a missed one leaves a row asserting an outcome the journal
contradicts — the exact class of bug this story exists to end, reintroduced one layer over. Reading
through the id instead means there is one source of truth and the row cannot disagree with it.

**Why `deferred` is `inProgress` and not `failed`.** This is the entry worth defending. A push held
by `SyncError::LfsUploadPending` (story 34.15) is `deferred` and carries a `last_error` reading
"publishing is on hold until this folder's large files reach the remote (N outstanding)". That is a
wait, not a break. Rendering it as a failure — red, "Failed" — would accuse keeper of malfunctioning
at the exact moment it is doing the careful thing, and it is precisely the mistake the engine used
to make on the other side of the same fact: `record_failure` mapped every `Deferred` retriability to
`ProfileState::MediaAbsent`, so a held push told the user "Large files missing" and sent them looking
for an unplugged drive. Both halves are fixed together, and the popover's message is toned by the
state rather than always destructive, so the wait reason is legible without being alarming.

**Why `unknown` renders nothing.** The tempting alternative is a neutral glyph — a grey dash, a
hollow circle — so every row has something in the column. That would be an assertion: it says "the
delivery state of this file is: unremarkable". The truth is that no unit of work is accountable for
the row, which is not a state on the way to success. Two rows land here: one written before the
column existed, and a conflict copy, whose publication belongs to a commit that does not exist yet.
Rendering nothing is the only honest option, and it is also what makes the column scannable — a
list where most rows are fine should be quiet.

**Why Retry only for `abandoned`.** A `failed` unit is one keeper is still retrying on a backoff.
A button there would imply keeper had stopped, and pressing it would either do nothing or, if it
forced an immediate attempt, spend one of the unit's remaining attempts earlier than the backoff
intends. The popover says so in a sentence instead. `abandoned` is the state where keeper has
genuinely stopped and only a human can restart it — which is exactly what `unpark` is for and what
the Problems section's Retry already did.

**Why the enqueue moved ahead of the activity write.** `commit` previously recorded activity and
then journaled the uploads. A row cannot name an id that does not exist, so the order had to
invert. Both still happen strictly after the commit object exists, which is the ordering that
matters for durability: a crash before the commit loses nothing, and a crash between the enqueue
and the activity write costs a log row, not work. The reverse gap — activity written, enqueue lost
— would leave rows pointing at nothing, which reads as `Unknown` rather than as a lie.

**Why the Problems section was kept rather than emptied.** The original request was for the reason
to appear in the row's popover "instead of the lower", and it now does. But Problems was not
deleted, for two reasons. It is the only surface for work that belongs to no single file — a
`pull`, a `verify`, an `openPullRequest`, or a `push` for a commit whose activity rows have aged
out — so emptying it would make those failures unreachable. And the UI reads `SYNC_ACTIVITY_LIMIT`
(20) activity rows while the table retains `ACTIVITY_CAP` far more, so suppressing a parked unit
from Problems on the grounds that "a file row shows it" would hide it entirely the moment its row
fell past row 20. What was actually wrong was never that Problems existed; it was that the reason
lived *only* there, in a section that names the unit and never the path.

**One retry path, hoisted.** `SyncActivityList` and `SyncProblemsSection` now both take `onRetry`,
and `SyncProfileCard` passes the same `retryUnit` closure to each. Two identical inline closures in
one JSX tree is how the two surfaces would come to take different busy locks or skip each other's
refresh.

## Verification

**Rust, in `keeper-sync/src/db.rs`:**

- `a_file_reports_the_state_of_the_unit_that_has_to_deliver_it` walks one row through the whole
  journal lifecycle — queued, claimed, failed-and-retrying, deferred, parked, completed — asserting
  the `DeliveryState`, the `failure` text and whether `unit_id` is offered at each step. It is the
  matrix above, executed.
- `an_activity_table_predating_the_late_columns_upgrades_in_place` plants the pre-34.6 schema,
  migrates, and asserts the old row survives reading back with no size and no delivery claim, then
  that a row written afterwards carries both late columns.
- `one_kind_of_deferred_work_can_be_released_without_disturbing_the_rest` and
  `outstanding_work_counts_the_parked_units_too` cover the `undefer_kind`/`outstanding_count`
  helpers this story added alongside story 34.15's gate.

**Engine:** `a_pointer_is_not_published_until_its_object_is_on_the_remote` (story 34.15's test)
also asserts this story's linking end to end on a real repository: after a commit carrying one
over-threshold file, `clip.mp4` names the upload unit and `.gitattributes` names the push, both
read `InProgress`, the `.gitattributes` row carries a failure text containing "on hold", and once
the upload completes `clip.mp4` reads `Success` with no unit id left to retry.

**Frontend, in `src/components/layout/sync-pane.test.tsx`, `describe("SyncPane activity delivery")`:**

- `says how far each file got, and says nothing where nothing is accountable` — a five-row fixture,
  one per contract value; asserts each word, that the `unknown` row carries the kind svg alone and
  none of the delivery words, and that a `success` row is a plain icon rather than a control.
- `names the file, the state and the engine's own message when asked why` — the popover.
- `retries exactly the unit the abandoned row was waiting on` — fires with that row's unit id and
  exactly once.
- `offers no retry on a row keeper has not given up on` — the `failed` popover shows the
  still-retrying sentence and contains no button at all.
- `shows a held file's reason for waiting without dressing it as a failure` — the held-push case:
  the wait reason is present, toned as `inProgress` and not as `failed`, with no Retry.

**Not covered, explicitly:**

- No test asserts the *ordering* of the enqueue against the activity write directly. It is covered
  transitively — the engine test's row would name no unit if the enqueue had not happened first —
  but nothing fails if the two are swapped and the ids happen to line up.
- The frontend tests mock the IPC client, so no end-to-end call checks `delivery_str`'s five wire
  spellings against the strings `SYNC_DELIVERY_STATES` indexes by. Two earlier claims here were
  wrong and are worth naming: `tsc` cannot check them, because the generated binding types
  `delivery` as a bare `string`; and `the_visibility_types_cross_the_ipc_boundary_as_camel_case`
  does not either, because it serializes `db::ActivityRow` through serde's derive — a different code
  path from the hand-written `delivery_str`. What does check them is
  `every_delivery_state_keeps_the_camel_case_spelling_the_ui_indexes_by` in `sync_ipc.rs`'s tests,
  which asserts the five literals `delivery_str` emits. An end-to-end invoke is still not exercised.
- Nothing measures the `LEFT JOIN`'s cost. It is one indexed lookup per returned row against
  `journal`'s primary key, on a read already bounded to `SYNC_ACTIVITY_LIMIT` rows for the UI, so it
  was reasoned rather than profiled.
- The field case that motivated the story — the three parked uploads on `hesperia` — has **not**
  been seen rendered. That host still runs 0.6.3; the rows it would produce are the `abandoned`
  fixture in the tests above, not an observation.
