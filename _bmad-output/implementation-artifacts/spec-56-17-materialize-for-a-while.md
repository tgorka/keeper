---
title: '56.17 Materialize for a while'
type: 'feature'
created: '2026-08-29'
status: 'done' # draft | ready-for-dev | in-progress | in-review | done | blocked
baseline_revision: 'c8c5c45'
review_loop_iteration: 0
final_revision: '830deba'
followup_review_recommended: false
context: []
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** Materialize is all-or-nothing. `keeper-syncd materialize`, the `sync_materialize_entry`
command and the Files row's **Materialize** action all fetch a path and leave it there until the
folder's own `releaseTtlMs` window (56.5) decides, and that window is per *profile*. The owner
looked for "pobranie na określony czas" in Files and there is nothing: no way to say "give me this
one file for two hours", no way to keep one file longer than the folder keeps everything else, and
no way to see how long a file he asked for has left that is different from the folder's default.

**Approach:** One per-path deadline in the `materialized` ledger, written by the one verb that
already exists, read by the one predicate that already decides eligibility. `release_due_at` (56.5)
learns a third input; `ReleaseSchedule` (56.9) learns a sixth-plus-one variant that carries the same
absolute instant through the same countdown; the CLI learns `--for`, refused at parse time; the row
learns a submenu on the verb it already has.

## Boundaries & Constraints

**Always:**
- **One predicate decides eligibility, and it is `release_due_at`.** The chosen deadline is a third
  input to that function, never a second decision anywhere. `Engine::release_expired` and
  `release_schedule` both keep asking exactly it, unchanged in shape.
- **FR-341 is asked before the chosen deadline is read.** In `release_due_at` the provenance branch
  — and therefore `row.synced_at_ms?` — runs **first**, unconditionally, and only then does the
  chosen deadline replace the TTL arithmetic. A locally authored path the remote has never been
  observed holding is not eligible **at any age**, and a duration must not become a way around
  that (FR-341, AD-131). Its own test, and its own mutation proof.
- **The pin still beats everything.** `if row.pinned { return None }` stays the first statement of
  `release_due_at` (56.5's hard floor), so a chosen duration cannot outrank it in either direction.
- **The deadline overrides the folder's window in both directions.** Shorter than `releaseTtlMs`
  means the path goes sooner; longer means it stays longer. It is a replacement, not a `min`, not a
  `max`, and not a term added to the TTL.
- **The ledger carries it, by 56.2's additive idiom.** `materialized` gains an eighth late column,
  `release_at_ms INTEGER`, through `ensure_materialized_columns`' typed-pair loop. Nullable, no
  `DEFAULT`, **no `meta` marker** — the `ALTER TABLE` guarded by the column list is its own
  idempotence. `NULL` means *this path is on the folder's window*, which is what every existing row
  means by having no such column.
- **One writer, in `set_pinned`'s two-direction shape.** `db::set_release_at(conn, profile_id, path,
  at_ms: Option<i64>, now_ms)`: `Some` upserts, `None` is UPDATE-only so withdrawing an instruction
  the ledger never recorded inserts no phantom row. It names `release_at_ms` and nothing else on the
  conflict arm.
- **Its insert stamps `released_at_ms = now_ms`, and that is what keeps the queued request honest.**
  A duration is recorded when the person asks, which for an object this machine does not hold yet is
  *before* any content lands. An inserted row with `released_at_ms` `NULL` would tell
  `materialized_paths`, `materialized_rows` and `lfs::listing::collect` that this machine holds
  content it does not — the phantom-row defect `set_pinned`'s own doc records. `released_at_ms`
  means "content for this path is not here", which for a queued path is **true**, and
  `remember_materialized` / `observe_materialized` / `note_local_authorship` already clear it the
  moment content lands. The conflict arm never touches it: a live row must not be marked released.
- **`forget_materialized` clears `release_at_ms`.** The instruction is spent the moment the content
  goes; leaving it would make a path re-materialized indefinitely instantly eligible off a deadline
  in the past.
- **A duration reaches the engine through one code path.** `Engine::materialize_entry` and
  `Engine::materialize_entry_now` gain `keep_for_ms: Option<u64>`, resolve it **once** to an
  absolute instant against the injected clock, and hand that one `Option<i64>` to
  `materialize_held`, which writes it before its three arms diverge — so the already-held, the
  published and the queued request all record the same instruction, and
  `materialize_entry_now`'s second `materialize_held` call re-writes the identical instant rather
  than a later one.
- **`None` and a zero duration are the same thing: indefinite.** `keep_for_ms.filter(|ms| *ms > 0)`
  is where they meet, once, and the result is a `set_release_at(None)` that clears any standing
  deadline. Two spellings of "put this path back on the folder's window" must not mean two things.
  This is behaviourally invisible to every existing caller and every existing test: the clear is
  UPDATE-only and every pre-existing row's column is already `NULL`.
- **The CLI refuses a malformed `--for` at parse time, with the input quoted**, in
  `validate_quiet_time`'s manner (`profile/mod.rs:736`): a clap `value_parser` over a pure
  `parse_keep_for(&str) -> Result<u64, String>`, accepting `<whole number><m|h|d>` and bare `0`,
  refusing everything else — including an overflowing product — with one sentence naming the forms
  and quoting what was typed.
- **The deadline flows through the same `ReleaseSchedule` and therefore the same countdown.** A new
  variant `DueByRequest { at_ms }` beside `Due { at_ms }`, answering `Ok(at_ms)` from
  `instant_or_words` so `releases_after_ms()` is `Some` and `hold()` is `None`. `instant_or_words`
  stays the one exhaustive match, so the variant is a compile error in exactly one place and
  "both"/"neither" stay inexpressible. Its own `sentence()`, because the difference the person cares
  about is whose clock it is.
- **No new wire field, no new ts-rs type, no regenerated binding.** `FilesReleaseVm` already carries
  `releases_after_ms`, `hold` and `detail`; `DueByRequest` fills exactly those three.
  `git status --porcelain -- src/lib/ipc/gen` must stay empty.
- **The row offers the choice at the click, in this repo's own idiom.** `FilesRowAction` gains an
  optional `options: readonly FilesRowActionOption[]`. The hover cluster is unchanged and keeps
  firing `onSelect`; the Radix context menu renders a `ContextMenuSub` when a verb has options —
  `chat-row.tsx:509-536`'s construction, not a fifth idiom, and not a modal. Four choices: 1 hour,
  8 hours, 24 hours, Indefinitely.
- **The hover button's default is today's behaviour.** An icon button has no room to say a duration,
  and Materialize has meant "indefinitely" since 56.9. Every option, including Indefinitely, goes
  through `runRowVerb` so 56.14's per-burst serialization and the pane's one alert sink still apply.
- **One interval, one renderer.** `releaseIsCounting`, the pane's single `FILES_TICK_MS` interval,
  `formatReleaseIn` and the release cell are untouched. The test counts timers.
- **No test sleeps.** `TestPlatform::advance_ms` is the only clock movement, `tests/release_sweep.rs`
  the only home for the engine-side proofs.
- **The shell crate is touched by name and minimally**: `sync_ipc::sync_materialize_entry` gains one
  argument and forwards it. No new command, so `lib.rs` and `command-registration.test.ts` are
  untouched.

**Block If:**
- Honouring a per-file deadline would require a second scheduler, a thread or a timer. (It does not:
  the sweep already reads `release_due_at` on the success edge.)

**Never:**
- **A zero `releaseTtlMs` still switches the folder's automatic release off entirely, chosen
  deadline or not.** `release_is_due` returns `false` before arming a window and `release_expired`
  returns before reading a clock; both stay as they are, and `release_schedule` goes on answering
  `Indefinite` ("Manual") for such a folder. A folder with automatic release switched off has no
  clock for a per-file deadline to override, and resurrecting the sweep for it would defeat the one
  knob whose documented meaning is "keeper releases nothing here on its own". The deadline is
  recorded and starts being honoured the moment the folder's window is switched on. Pinned by a
  test that names the boundary.
- No second predicate, no second formatter, no second interval, no `min`/`max` blend of the two
  clocks, no per-file TTL *setting* on the profile or in a folder TOML layer.
- No modal, no date picker, no free-text duration in the app, no new row verb.
- No change to `release_mode_gate`, `release_path_gate`, `release_resolved`, the five refusals,
  `RELEASE_BUDGET_*`, the rotation, `note_use`/`note_arrival`/`note_local_authorship`/`note_synced`/
  `set_pinned`, `MaterializeOutcome`, `Materialization`, `EntrySyncStatus`, `FilesSyncStatusVm` or
  `sync-status-mark`.
- No new Tauri command, no new crate, no new dependency, no `meta` migration marker, no
  `SyncProfile` field, so `accepted_profile_keys`, `FOLDER_FIELD_RULES`, `EXPRESSED`/`PRESERVED` and
  `a_save_cannot_move_a_field_no_request_can_express` are untouched.
- No unpin, no "extend the deadline" verb, no way to read the chosen duration back other than the
  countdown the row already draws.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Shorter than the folder | `releaseTtlMs = 24 h`, path fetched `--for 1h` | released on the first successful sync after that hour; the folder's 24 h never applies | No error expected |
| Longer than the folder | `releaseTtlMs = 24 h`, fetched `--for 48h` | not a candidate at 25 h; a candidate after 48 h | No error expected |
| Pinned | `pinned = 1`, `release_at_ms` an hour in the past | never a candidate; `ReleaseSchedule::Pinned`, no countdown | No error expected |
| Locally authored, unconfirmed | `local_origin = 1`, `synced_at_ms` `NULL`, deadline long past, clock advanced 100 × TTL | **absent from the candidate set**; `ReleaseSchedule::Unconfirmed` | No error expected |
| Indefinite, explicitly | `--for 0`, or the row's Indefinitely, or no flag at all | `release_at_ms` cleared; the path is on the folder's window exactly as today | No error expected |
| A duration replaces a duration | `--for 1h` then `--for 8h` | the later instruction stands | No error expected |
| Queued request | object not in the store, `--for 2h` | the deadline is recorded now; the ledger row is invisible to `materialized_paths` / `materialized_rows` / the listing until content lands, then counts down from the instant it was asked for | No error expected |
| Released, then re-materialized | a path released after its deadline, materialized again with no duration | `release_at_ms` is `NULL`, so the folder's window applies from the new landing — never instantly eligible | No error expected |
| Untracked path, with a duration | `materialize x --for 2h` on a path with no committed pointer | `NotTracked`; **no ledger row written** — the deadline is recorded after the pointer is resolved | `SyncError::Refused`, exit 1 |
| Malformed `--for` | `1w`, `2`, `-1h`, `1.5h`, `2 h`, `h`, `` | refused before anything runs, one sentence naming `30m`/`2h`/`1d`/`0` and quoting the input | clap `value_parser`, exit 2 |
| Overflowing `--for` | `99999999999999d` | refused by the same sentence | clap, exit 2 |
| Well-formed `--for` | `30m`, `2h`, `1d`, `0` | 1 800 000 / 7 200 000 / 86 400 000 / 0 ms | No error expected |
| Row, live chosen deadline | `release = { releasesAfterMs: now + 1 h, hold: null, detail: the request sentence }` | the existing cell draws `1 hr` and speaks the sentence; **exactly one** 1000 ms interval for the whole pane | No error expected |
| Row, submenu | right-click a `virtual` row → Materialize → 8 hours | `syncMaterializeEntry(id, subpath, 28_800_000)` | refusal reaches the pane's one alert |
| Row, hover button | the promoted Materialize control | `syncMaterializeEntry(id, subpath, undefined)` — today's call | refusal reaches the pane's one alert |
| TTL switched off | `releaseTtlMs = 0`, a row carrying a chosen deadline | the sweep does not run and the row reads `Manual` — the folder releases nothing on its own | No error expected |
| Ledger write fails | SQLite error while recording the deadline | the request fails before any content is published or queued | propagated |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/db.rs` — `ensure_materialized_columns` `:299` and its typed-pair
  array `:306-314` (**gains** `("release_at_ms", "INTEGER")` and a paragraph); `remember_materialized`
  `:589`, `observe_materialized` `:658`, `note_local_authorship` `:810` (the three that clear
  `released_at_ms`; **not modified**); `set_pinned` `:892` (the two-direction shape to copy);
  `materialized_paths` `:926` and `materialized_rows` `:1011` (**the `SELECT` gains the column**);
  `MaterializedRow` `:958` (**gains `release_at_ms: Option<i64>`**); `forget_materialized` `:1146`
  (**also clears `release_at_ms`**). **Gains** `set_release_at`.
- `src-tauri/crates/keeper-sync/src/engine.rs` — `release_due_at` `:10445` (**the one predicate,
  extended**); `ReleaseSchedule` `:10492` and `instant_or_words` `:10555`, `sentence` `:10630`
  (**gain `DueByRequest`**); `release_schedule` `:10701` (**one arm split**); `release_expired`
  `:7106` and its `release_due_at` filter `:7180` (**not modified**); `release_is_due` `:2027`
  (**not modified**); `materialize_entry` `:7870`, `materialize_held` `:7975`,
  `materialize_entry_now` `:8298` (**gain `keep_for_ms` / the resolved instant**);
  `materialize_request` `:7896` (**not modified** — the deadline is written after the pointer is
  resolved).
- `src-tauri/crates/keeper-sync/tests/release_sweep.rs` — `Fixture` `:318`, `arrived` `:452`,
  `authored` `:481`, `candidates` `:295`, `open_the_release_window` `:420`, `TTL_MS` `:68`.
  **Gains** a `kept_for` helper and the four engine-side proofs.
- `src-tauri/crates/keeper-syncd/src/commands.rs` — `Command::Materialize` `:313` (**gains `--for`
  and its `--help` sentence**); the dispatch arm `:655`; `cmd_materialize` `:1700`;
  `materialize_lines` `:1565`; the clap parse tests `:2623`, `:2667`. **Gains** `parse_keep_for` and
  its unit tests.
- `src-tauri/crates/keeper/src/sync_ipc.rs` — `sync_materialize_entry` `:2676`. **Shell crate: one
  argument, forwarded.**
- `src/lib/ipc/client.ts` — `syncMaterializeEntry` `:3295`. **Gains** an optional third argument.
- `src/components/layout/files-pane.tsx` — `FilesRowAction` `:933`; the `actions` array `:2244` and
  the Materialize entry `:2305-2317`; the hover cluster `:2669-2689`; the context menu `:2737-2741`;
  `runRowVerb` `:1349`; `FILES_MATERIALIZE_LABEL` `:311`; `releaseIsCounting` `:458`; `FILES_TICK_MS`
  `:438`.
- `src/components/ui/context-menu.tsx` — `ContextMenuSub` / `SubTrigger` / `SubContent` `:240-242`;
  `src/components/chat/chat-row.tsx:509-536` is the shipped usage.
- `src/components/layout/files-pane.test.tsx` — the `vi.mock("@/lib/ipc/client")` factory `:11-45`,
  the verb allow-list, the interval-counting suite.
- `docs/sync.md` — §9 "Two release clocks, and which one applies" `:1019`, "`releaseTtlMs`, the
  per-pass budget…" `:1048`, "The verbs" `:1127`; §13 `:1642`.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-sync/src/db.rs` -- add the `release_at_ms` column, the
      `MaterializedRow` field, the `materialized_rows` narrowing, `set_release_at`, and
      `forget_materialized`'s clear -- the ledger has to hold the fact before anything can decide on
      it, and the row struct has to carry it before a test can even be written.
- [x] **Write the six proofs and record their pre-change failure text** -- four in
      `tests/release_sweep.rs`, one in `keeper-syncd/src/commands.rs`, one in
      `files-pane.test.tsx` -- before `release_due_at`, `parse_keep_for` and the submenu exist.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- extend `release_due_at`; add
      `ReleaseSchedule::DueByRequest` and its sentence; split the `release_schedule` arm; thread
      `keep_for_ms` through the three materialize functions -- one predicate, one classifier, one
      code path.
- [x] `src-tauri/crates/keeper-syncd/src/commands.rs` -- `parse_keep_for`, the `--for` argument, the
      dispatch, and the `--help` wording -- a duration must be refused before any work starts.
- [x] `src-tauri/crates/keeper/src/sync_ipc.rs`, `src/lib/ipc/client.ts` -- the optional duration on
      the existing command and its wrapper -- the app's door onto the same code path.
- [x] `src/components/layout/files-pane.tsx` -- `FilesRowActionOption`, the submenu in the context
      menu, the four choices on Materialize -- the choice belongs at the click.
- [x] `docs/sync.md` -- the per-file duration in §9's clocks section, in the verbs table and in §13.
- [x] Mutate each new condition away, confirm the owning test fails, restore, verify by reading
      `git diff`.

**Acceptance Criteria:**
- Given a folder whose window is 24 h and a path fetched for 1 h, when the clock advances an hour
  and a sync succeeds, then the path is released and the worktree holds the committed pointer.
- Given the same folder and a path fetched for 48 h, when the clock advances 25 h and a sync
  succeeds, then the path is untouched; and when it advances past 48 h, then it is released.
- Given a pinned path carrying a deadline an hour in the past, when the sweep runs, then the path is
  not in the candidate set at all.
- Given a locally authored path with no `synced_at_ms` and a deadline long past, when the clock
  advances a hundred windows, then the path is not in the candidate set at any age.
- Given `keeper-syncd materialize docs a.mp4 --for 1w`, when the process starts, then it exits 2
  with a message quoting `1w`, and nothing on disk is touched.
- Given a Files row whose wire deadline is an hour away, when the pane renders it, then the existing
  release cell counts it down and exactly one 1000 ms interval is armed for the whole pane.
- Given the existing indefinite Materialize, when the whole suite runs, then every test 56.3/56.9/
  56.13/56.14 wrote passes untouched.

## Spec Change Log

## Review Triage Log

## Design Notes

**Why the deadline replaces the arithmetic rather than joining it.** `release_due_at`'s body is four
lines and its shape is *pick a clock, add the window*. A per-file deadline is not a different window
over the same clock — it is an instant the person named — so it belongs where the sum is, not inside
it. That keeps FR-341 exactly where it was, above both, and keeps the whole rule readable:

```rust
pub fn release_due_at(row: &db::MaterializedRow, ttl_ms: u64) -> Option<i64> {
    if row.pinned {
        return None;                      // 56.5's hard floor, before any clock
    }
    // FR-341 first and unconditionally: a chosen duration is not a way past it.
    let since = if row.local_origin {
        row.synced_at_ms?
    } else {
        row.last_used_ms.unwrap_or(row.at_ms)
    };
    // 56.17: the instant the person named REPLACES the folder's window, in both
    // directions. Shorter goes sooner, longer stays longer.
    Some(
        row.release_at_ms
            .unwrap_or_else(|| since.saturating_add(ttl_ms as i64)),
    )
}
```

**Why a new `ReleaseSchedule` variant and not a bare narrowing of `Due`.** Narrowing costs nothing
and says nothing: the countdown would be identical and the *sentence* would claim the folder's window
is what is holding the file, which is exactly the thing this story makes untrue. The epic's own house
rule is that the wordy variants exist because "each is a different sentence to the person who asked".
`DueByRequest` carries the instant, so it goes through `instant_or_words`' `Ok` arm and the wire, the
cell, the tick and the formatter are all unchanged — the compile-error-in-one-place property is what
makes adding it safe.

**Why the insert stamps `released_at_ms`.** A duration is a standing instruction, so it is recorded
when the person asks — and for an object this machine does not hold, that is before the download
lands. `set_pinned`'s pin arm inserts a row with `released_at_ms` `NULL` and its own doc records what
that costs: `materialized_paths` then reports content that is not here. Here the honest value is
available for free, because the column's documented meaning is exactly "the content is not here", and
the three landing writers already clear it. So the queued case creates a row nothing present-tense
can see, and the deadline is waiting when the bytes arrive.

**Why the row gets a submenu and not a modal.** The row's verbs are one typed `FilesRowAction[]`
feeding both the hover cluster and the Radix menu; a modal would need a second surface, a second
focus trap and a second place for a refusal to land. `chat-row.tsx` already spells the submenu, so
the array grows one optional field and the menu grows one branch. The hover button keeps its single
meaning because an icon has no room for four, and its meaning is the one it has had since 56.9.

## Verification

**Commands:**
- `cargo fmt --manifest-path src-tauri/Cargo.toml --all` -- expected: applied, `--check` clean after.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings` -- expected: clean.
- `GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=keeper GIT_AUTHOR_EMAIL=dev@keeper.local GIT_COMMITTER_NAME=keeper GIT_COMMITTER_EMAIL=dev@keeper.local cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync -p keeper-core -p keeper-syncd` -- expected: 0 failed, at or above 3596 passing.
- `bun run typecheck` -- expected: clean.
- `bun run lint` -- expected: the recorded baseline, 4 warnings + 1 info.
- `bun run test` -- expected: green, at or above 4935 tests, `command-registration.test.ts` included and untouched.
- `bun run check:core-tauri-free`, `check:core-sync-free`, `check:syncd-lean` -- expected: pass; no dependency is added.

**Manual checks (if no CLI):**
- `git status --porcelain -- src/lib/ipc/gen` -- must be **empty**: no wire type changes.
- `git diff --stat -- src-tauri/crates/keeper` -- must show `sync_ipc.rs` only, one argument.
- Shell-crate symbols for `bun run check:rust:macos`: `sync_ipc::sync_materialize_entry`.
- Smoke-test the real `keeper-syncd` binary: `materialize --for 1w` (exit 2, `1w` quoted),
  `--for 2h` (accepted), `--for 0` (accepted), `--help` naming the forms.

## Auto Run Result

Status: done

### What was implemented

A person can now fetch one file **for a stated time** and keep it that long whatever the folder's
own release window says, and the row counts it down.

**One ledger column, one predicate.** `materialized` grew an eighth late column, `release_at_ms`,
by 56.2's additive `ensure_*_columns` idiom and with no `meta` marker. `release_due_at` — 56.5's
one pure predicate and still the only thing that decides eligibility — reads it *instead of* the
folder's `releaseTtlMs` arithmetic, so a chosen hour inside a day-long window goes twenty-three
hours sooner and a chosen two days stay a day longer. A replacement, never a `min` or a `max`.

**Both floors are above it, and the ordering is the story's whole risk.** The pin's early return is
still the function's first statement. The provenance branch — and therefore FR-341's
`row.synced_at_ms?` — still runs **before** the chosen deadline is read, so a locally authored path
the remote has never been observed holding is on no clock at any age and a duration is not a way
around the one barrier protecting bytes that live on one machine. Both orderings are mutation-proven
below.

**One writer, and the queued case is honest.** `db::set_release_at` is `set_pinned`'s two-direction
shape: an upsert to set, UPDATE-only to withdraw, naming `release_at_ms` and nothing else on the
conflict arm. Its **insert stamps `released_at_ms`**, which is what stops a deadline recorded for
content that has not landed yet from becoming the phantom "this machine holds it" row `set_pinned`'s
own doc records the cost of — `released_at_ms` means *the content is not here*, which for a queued
path is true, and the three landing writers already clear it. `forget_materialized` clears
`release_at_ms` beside its stamp: the instruction is served the moment the content goes, and a
deadline left in the past would make the next indefinite materialize instantly eligible.

**Three doors, one code path.** `KeepFor { Unspecified, Indefinitely, Ms(u64) }` is the request
vocabulary; `KeepFor::from_ms` is the one reading of an optional millisecond count that both doors
share. The third variant earns its place: *said nothing* is not *said indefinitely*, and folding
them would have let the copy planner — which hydrates a path only so `copy` can read real bytes —
silently discard the two hours its owner asked for. The engine resolves a duration to an absolute
instant **once**, at the door, so `materialize_entry_now`'s second observing pass records the same
deadline rather than one moved forward by the transfer, and writes it above the three arms so the
already-held, the published and the queued request all record it.

**The countdown is the one that already existed.** `ReleaseSchedule::DueByRequest { at_ms }` carries
the same instant through the same `instant_or_words` `Ok` arm, so `releases_after_ms` / `hold` /
`detail` are the same three wire fields, the same cell draws it, the same pure formatter renders it
and the same single pane interval ticks it. **No new wire type, no regenerated binding.** What
differs is the sentence, which is exactly why it is a variant rather than a narrowing: `Due`'s
sentence describes a folder-wide window the person may never have heard of, and repeating it over a
file they personally asked to keep for two hours would attribute their own instruction to a setting.

**The choice is at the click.** `FilesRowAction` grew an optional `options`; the Radix context menu
renders a `ContextMenuSub` for a verb that has them (`chat-row.tsx`'s shipped idiom, not a fifth
one), the hover cluster is untouched and keeps the verb's default. Four choices — 1 hour, 8 hours,
24 hours, Indefinitely — bracketing the 24 h folder default on both sides. No modal.

**One boundary is stated rather than crossed.** `releaseTtlMs = 0` still switches the folder's
automatic release off entirely: the gate arms no window and the sweep returns before reading a
clock, so `release_schedule` goes on answering `Indefinite`/"Manual". A folder with no clock has no
window for a per-file deadline to override, and resurrecting the sweep for it would defeat the one
knob whose documented meaning is "keeper deletes nothing here on its own". The instruction stays
recorded and is honoured the moment the interval is switched on. Pinned by a test.

### Files changed

- `src-tauri/crates/keeper-sync/src/db.rs` — the `release_at_ms` column, `MaterializedRow`'s field,
  the `materialized_rows` narrowing, `set_release_at`, `forget_materialized`'s clear, three tests.
- `src-tauri/crates/keeper-sync/src/lfs/hydrate.rs` — `KeepFor` and `KeepFor::from_ms`.
- `src-tauri/crates/keeper-sync/src/engine.rs` — `release_due_at` extended; `ReleaseSchedule::
  DueByRequest` and its sentence; the `release_schedule` arm split; `ReleaseInstruction`;
  `Engine::release_instruction`; `keep_for` threaded through `materialize_entry`,
  `materialize_held` and `materialize_entry_now`; two unit tests.
- `src-tauri/crates/keeper-sync/src/lfs/listing.rs`, `tests/{materialize_entry,lfs_listing,
  virtual_arrival}.rs` — the new field and the new argument at existing call sites, all
  `KeepFor::Unspecified`.
- `src-tauri/crates/keeper-sync/tests/release_sweep.rs` — `candidates_with`, `Fixture::kept_for`
  (through the production verb, not a planted column), and five real-git, injected-clock proofs.
- `src-tauri/crates/keeper-syncd/src/commands.rs` — `parse_keep_for`, the `--for` argument and its
  `--help`, the dispatch, `cmd_materialize`, and the parse tests.
- `src-tauri/crates/keeper/src/sync_ipc.rs` — one argument on `sync_materialize_entry`, forwarded.
  **Shell crate: no compiler ran on this host.**
- `src/lib/ipc/client.ts`, `src/components/layout/files-pane.tsx` (+ its test) — the optional third
  argument, `FILES_MATERIALIZE_DURATIONS`, `FilesRowActionOption`, the submenu.
- `docs/sync.md` — §9's clocks section, the `0`-stays-`0` paragraph, the verbs table and its prose,
  the row-actions paragraph, and §13.

### Verification performed

- `cargo fmt --all` applied, `--check` clean. `cargo clippy -p keeper-core -p keeper-sync
  -p keeper-syncd --all-targets -- -D warnings` clean.
- Rust over the three buildable crates: **3607 passed / 0 failed / 1 ignored** (baseline 3596).
- `bun run typecheck` clean. `bun run lint` at baseline — 4 warnings + 1 info. `bun run test`
  **297 files / 4938 tests** (baseline 4935). All three dependency firewalls pass.
  `git status --porcelain -- src/lib/ipc/gen` **empty**.
- `git diff --stat -- src-tauri/crates/keeper` shows `sync_ipc.rs` only.
- Shipped-binary smoke test: `materialize --help` names the four forms; `--for 1w`, `2`, `1.5h`,
  `0m`, `""` and an overflowing day count each exit **2** with clap's `invalid value '<input>' for
  '--for <DURATION>'` plus the sentence naming `30m`, `2h`, `1d` and `0`; `--for 2h` and `--for 0`
  parse and reach the engine.
- **No test sleeps.** `TestPlatform::advance_ms` is the only clock movement in the engine proofs.

### Pre-change failures and mutation proofs

Each restore verified by SHA-256 identity against a pre-mutation snapshot, by `git diff --numstat`,
and by `grep -rn MUTATION` returning nothing.

| what was removed / inverted | site | owning test | observed |
|---|---|---|---|
| the chosen deadline itself (**this is the pre-change state**) | `engine.rs` `release_due_at` | four release-sweep tests | `a_path_asked_for_an_hour_…` FAILED at `release_sweep.rs:1847` `left: [] right: ["clip.mp4"]`; `a_path_asked_for_two_days_…` FAILED at `:1895` "past the folder's window and not a candidate"; `a_pin_outranks_…` FAILED at `:1979` `left: [] right: ["clip.mp4"]`; `a_chosen_duration_does_not_release_…` FAILED at `:2069` `left: [] right: ["clip.mp4"]` |
| the deadline read **above** the provenance branch | `engine.rs` `release_due_at` | `a_chosen_duration_does_not_release_a_path_the_remote_has_never_confirmed` | FAILED at `:2046` — "so it is absent from the candidate set at any age: a duration is not a way around FR-341" |
| the pin's early return deleted | `engine.rs` `release_due_at` | `a_pin_outranks_a_chosen_duration_and_survives_it` | FAILED at `:1960` — "and it is still not a candidate: the pin is asked before any clock, and a duration is a clock" |
| `ReleaseInstruction::Leave` folded into `Clear` | `engine.rs` `materialize_held` | `asking_again_without_a_duration_leaves_the_one_already_given` | FAILED at `:2107` `left: None right: Some(1700007200000)` |
| `released_at_ms` dropped from `set_release_at`'s insert | `db.rs` | `a_deadline_for_content_that_has_not_landed_is_invisible_until_it_does` | FAILED at `db.rs:3744` — "nothing here claims this machine holds the content" |
| `release_at_ms = NULL` dropped from `forget_materialized` | `db.rs` | `releasing_a_path_withdraws_the_deadline_that_asked_for_it` | FAILED at `db.rs:3787` `left: Some(2000) right: None` |
| `--for` removed from the CLI (**the pre-change state**) | `commands.rs` | `for_is_parsed_into_milliseconds_and_a_typo_is_refused_with_the_value_quoted` | FAILED — "`--for 30m` should parse: error: unexpected argument '--for' found" |
| `parse_keep_for`'s unit match coerced to minutes | `commands.rs` | same | FAILED — "`--for 2` must be refused: … keep_for: Some(120000)" |
| `choice.keepForMs` replaced by `undefined` | `files-pane.tsx:2412` | `a virtual row offers the four durations under Materialize, and choosing one asks for it` | FAILED — `expected "vi.fn()" to be called with arguments: [ Array(3) ]` |

Two of the six mandated proofs are guards rather than new behaviour — a pin and an unconfirmed
locally authored path were *already* refused before this story, so each was given a second half that
is red pre-change: lifting the pin makes the path a candidate twenty-two hours before the folder's
window would, and confirming the upload makes the chosen hour (already past) the clock rather than
the folder's day. Both halves are in the table above. The frontend countdown test is likewise a pin
on machinery this story does not change — it passed pre-change and is red under a second
formatter, a second cell or a per-row timer, which is what it exists to catch.

### Shell-crate symbols for `bun run check:rust:macos`

`sync_ipc::sync_materialize_entry` — one added parameter (`keep_for_ms: Option<u64>`), one
`KeepFor::from_ms` call, and the `spawn_blocking` closure now passing three arguments to
`Engine::materialize_entry`. Nothing else under `src-tauri/crates/keeper` was touched: no command
was added, so `lib.rs`'s two `keeper_with_commands!` splices and
`src/test/command-registration.test.ts` are unchanged and the latter passes here.

### Generated bindings

**None need the Mac.** This story adds no ts-rs type and changes no wire type's fields or docs:
`FilesReleaseVm` already carries `releases_after_ms`, `hold` and `detail`, and `DueByRequest` fills
exactly those three. `git status --porcelain -- src/lib/ipc/gen` is empty.

### Residual risks

- **The shell crate is unverifiable here.** One parameter and one forwarded call, read in full and
  reviewed by hand; the macOS gate has exactly that to confirm.
- **`releaseTtlMs = 0` ignores a chosen deadline**, deliberately and with a test naming it. A folder
  whose automatic release is off releases nothing, per-file instruction or not.
- **The row's *Indefinitely* and its promoted icon are two answers**, `0` and absent. They differ on
  exactly one row — a path asked for "for two hours" whose bytes have not landed — and are
  indistinguishable everywhere else. Documented at all three layers.
- **A duration is honoured only where the sweep runs at all**: everything 56.5 recorded still holds,
  including that `SyncPlatform::open_file_state` answers `Unknown` on macOS and Windows, so those
  hosts refuse a release by name whatever clock says it is due.
