---
title: '56.9 The button, and the time you have left'
type: 'feature'
created: '2026-08-25'
status: 'done' # draft | ready-for-dev | in-progress | in-review | done | blocked
baseline_revision: '3810bc0'
review_loop_iteration: 0
followup_review_recommended: true
context: []
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** Epic 56 can put content on a machine, let one path go, and — since 56.7 — *say* which of the three states a row is in, and a person can still do none of it. The Files row's single `actions` array (`files-pane.tsx:1810-1855`) holds Open, Reveal and Copy path and nothing else, so `Engine::materialize_entry`, `Engine::dehydrate_entry` and `Engine::pin_entry` are reachable only from `keeper-syncd`'s CLI — two of them have no Tauri command at all. And 56.5's release clock is invisible: `release_due_at` (`engine.rs:8685`) already answers *the absolute instant a row becomes releasable*, the sweep is its only reader, and nothing on any surface tells the owner how long the content he asked for will still be there. The pane's own clock comment names the gap: "an interval would be a tick this pane deliberately does not own — Story 56.9 adds the one there is" (`files-pane.tsx:829-831`).

**Approach:** Three verbs as three entries in the one `actions` array, so the hover cluster and the context menu get them from the same list; two new Tauri commands beside 56.3's `sync_materialize_entry`; and **one absolute epoch-ms instant on the wire**, resolved in Rust by a thin classifier over `release_due_at`, rendered as a duration by a pure TypeScript helper against **one 1 s interval owned by the pane**. A row with no deadline carries Rust's own words for why instead of a fake timer.

## Boundaries & Constraints

**Always:**
- **One `actions` array, three new entries, both surfaces.** A virtual row (`entry.sync.status === "virtual"`) gains **Materialize**; a materialized row (`"materialized"`) gains **Release** and **Pin**. They are entries in the existing `readonly FilesRowAction[]` (`files-pane.tsx:1810`) — the array the hover cluster slices (`:2092-2112`) and the Radix menu spells in full (`:2159-2168`). No fourth idiom, no `destructive`/`disabled` field, no verb reachable from only one surface. Order is priority (a promoted prefix): the state verbs go **after** Open/Reveal/Copy, so no existing row's cluster reorders.
- **The row's verbs are fire-and-report, not fire-and-forget.** The three existing row verbs swallow (`.catch(() => undefined)`); these three must not, because a refusal is Rust's answer to something the person just did. Each `.catch` funnels into the pane's existing single sink — `setWriteError(isIpcError(error) ? error.message : String(error))` (`:1406-1408`) rendered in the one destructive `Alert role="alert"` (`:2367-2376`). No toast, no second channel, no per-row error slot.
- **`AlreadyPointer` is a success, not a red alert.** `keeper-syncd`'s release door reports it as `nothing_to_release` (`lfs/hydrate.rs:206-215`); `sync_release_entry` must do the same or a no-op release paints an error. Every other `ContentRefusal` reaches the user as its own sentence, verbatim, through `sync_ipc_error`'s existing `Refused` arm (`sync_ipc.rs:804`) — nothing is added to the error mapper and no typed refusal payload is invented.
- **The wire carries a moment, never a duration and never a rendered string.** `FilesEntryVm.release: Option<FilesReleaseVm>` where `releases_after_ms: Option<i64>` is an absolute epoch-ms instant and **must** carry `#[ts(type = "number | null")]`, for the reason `mtime_ms` states four lines above it (`vm.rs:4130-4134`): ts-rs emits `bigint` otherwise, which no `JSON.parse` produces and against which every comparison in the countdown is a type error. This is the story's most likely defect and it has its own mutation proof.
- **The TTL policy is not re-implemented in TypeScript.** The frontend never sees `release_ttl_ms`, never learns which of 56.5's two clocks applies, and never learns that `LfsMode::PointerOnly` is the only mode that releases. `keeper_sync::engine::release_schedule` — a pure classifier **over** `release_due_at`, not a second copy of it — answers `Due { at_ms } | Pinned | Unconfirmed | Indefinite | ModeKeeps`, and `Engine::release_schedules` applies it once per profile.
- **Exactly one of the instant and the words is present, and that invariant is proven where a compiler runs.** `ReleaseSchedule::releases_after_ms()` and `ReleaseSchedule::hold()` are `Option`s that are never both `Some` and never both `None`; a `keeper-sync` test asserts it over every variant. The shell then reads three fields and cannot get the pairing wrong.
- **`materializing` promises no finish time** (56.7). `FilesEntryVm::new` drops `release` unless the row is `FilesSyncStatusVm::Materialized` **and** `!is_dir` — the same shape in which it already drops a directory's `size` (`vm.rs:4247-4251`), so the rule is enforced in the crate that compiles here rather than in the shell that does not.
- **One tick for the pane, never one per row.** Rows are windowed (`useWindowedRows`, `files-pane.tsx:1489-1507`), so a per-row interval arms and disarms on every scroll. The pane's single `Date.now()` read (`:831`) becomes `useState(() => Date.now())` plus **one** `setInterval(…, 1000)` in an effect whose dependency is a **boolean** derived from the rows ("does any visible row still count down"), copying `undo-send-pill.tsx:39-48` — including its early `return` when there is nothing to count, so a pane with no countdown arms no timer at all.
- **The remaining time is rendered by pure functions.** `formatReleaseIn(deadlineMs, now)` and `formatReleaseSpoken(deadlineMs, now)` join `formatDraftAge` in `src/lib/format-time.ts`, injectable clock, `""` for anything unrenderable, `Math.max`-clamped so a skewed clock reads `due` and never a negative. The ladder is `formatSyncWaited`'s house vocabulary inverted (`sync-pane.tsx:586-606`), with seconds in the last minute so a 1 s tick has something to move.
- **The countdown reads as text under `motion-reduce`, because it has no motion at all.** No ring, no spinner, no transition — the numerals are `figures` (tabular) so a ticking figure does not change width. A test asserts the cell carries no `animate-`/`transition-` class, which is the mechanical form of the rule.
- **The width budget is honoured and its stated order is extended, not bypassed.** `filesRowActionsBudget` and `FILES_ROW_ACTION_PX` are unchanged. `filesRowShowsModified` is **replaced** by one exported pure planner, `filesRowCellPlan`, that spends the row in a stated order: the name's floor (already inside the budget) → the release cell → the verbs → the date. Two changes, both forced and both documented in the code: the release cell is charged **before** the verbs (at 360px, the shipped default, the all-verbs rule would make the story's headline fact unpaintable on every row), and the date's gate asks **this row's** verb count instead of a global maximum (`FILES_ROW_MAX_ACTIONS` is deleted: with five possible verbs a uniform maximum unpaints the date at every shipped width, which is 56.7's pinned 480px guarantee lost for rows that gained nothing).
- **Nothing that cannot be painted is lost.** The release cell, like the date, keeps its element, its id and its place in `aria-describedby` and goes `sr-only` when the row cannot draw it. Every verb is in the menu at every width.
- **`src/lib/ipc/gen/**` is generated.** Regenerate with `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core export_bindings` (`TS_RS_EXPORT_DIR`, `.cargo/config.toml:5-6`) and commit unedited. Expect exactly two files: `FilesEntryVm.ts` and a new `FilesReleaseVm.ts`.
- **No `SyncProfile` field.** `release_ttl_ms` and its `("releaseTtlMs", Allowed)` folder rule already exist (`profile/mod.rs:922`, `folder.rs:144`), so `accepted_profile_keys`, `folder_field_rules_cover_every_profile_field`, `EXPRESSED`/`PRESERVED` and `a_save_cannot_move_a_field_no_request_can_express` are untouched.
- **The shell crate cannot be compiled here.** Every touched `keeper::sync_ipc` / `keeper::lib` symbol is reported for the macOS gate, and every rule that could be written in `keeper-sync` or `keeper-core` is written there instead. The two new commands are registered in the **desktop** `keeper_with_commands!` splice only, with a `client.ts` wrapper whose command name is a string literal — the three-way join `src/test/command-registration.test.ts` pins and the only gate that runs on Linux.

**Block If:**
- The absolute deadline cannot be resolved in Rust without either re-deriving 56.5's clock selection or adding `local_origin` to `lfs::listing::LfsFile`'s pinned wire key set. (It can: `db::materialized_rows` carries `local_origin` and `release_due_at` is already `pub`.)
- Charging the release cell ahead of the verbs would push the name below `FILES_NAME_FLOOR_PX`. (It cannot: the floor is subtracted inside `filesRowActionsBudget` before any cell is charged, and the cell is refused outright when the budget cannot cover it.)

**Never:**
- **No duration and no pre-rendered time on the wire.** No `releases_in_ms`, no `"23 hr"` from Rust, no server-side "expired" boolean.
- **No second interval, no per-row timer, no `requestAnimationFrame`, no polling of the Files tree.** Its listings are on demand, and this story does not change that: nothing re-browses on a tick.
- **No claim that the countdown reaching zero released anything.** The sweep runs on the first successful sync after an hourly due-gate, budgeted to 32 objects per pass (`engine.rs:400`, `:434`), so `due` means *eligible*, and the cell's Rust sentence says so.
- **No unpin verb, no `pinned` bit on the wire, no toggle.** Pin is one-way and idempotent (`pin_entry`'s own doc); the wire cannot tell a pinned row from an unconfirmed one without Rust's words, and inventing a second field to carry it is 56.9 growing an ask nobody made. Recorded as deferred work with the one-line change a later story would make.
- **No new `EntrySyncStatus` or `FilesSyncStatusVm` variant, no change to `sync_mark`'s three sentences, no change to the marks, the delete plan, `browse`, `verify`, `copy`, the sweep, the CLI, `docs/sync.md` (56.8 owns the chapter) or `lfs::listing`'s key set.**
- **No new dependency, no new crate, no `sonner`, no animation library.**
- No `Partial<Record<…>>`, no `any`, no cast in a test fixture that stands in for a wire type.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| A virtual row's verb | `sync.status === "virtual"` | `actions` holds Materialize; it appears in the hover cluster (wide column) and in the context menu | No error expected |
| A materialized row's verbs | `sync.status === "materialized"` | `actions` holds Release **and** Pin, both surfaces, from the one array | No error expected |
| A plain synced row | `"synced"` | `actions` is exactly Open/Reveal/Copy — no state verb | No error expected |
| A folder row | `is_dir` | no state verb, no release cell, whatever the ledger holds | No error expected |
| Materialize accepted | `sync_materialize_entry` resolves | the parent folder is re-browsed; the row's mark becomes `materializing`; no alert | No error expected |
| Release refused | `sync_release_entry` rejects with `Refused(OpenUnknown)` | the pane's alert shows Rust's sentence verbatim: "keeper cannot tell whether …" | rendered, not replaced |
| Release of a pointer | `Refused(AlreadyPointer)` | treated as success: folder re-browsed, **no** alert | swallowed deliberately |
| Pin accepted | `sync_pin_entry(id, subpath, true)` resolves | re-browse; the row's release cell becomes the word `Pinned` and the countdown is gone | No error expected |
| A live deadline | `release = { releasesAfterMs: now + 23.5 h, hold: null }` | cell draws `23 hr`, speaks "Releases in 23 hr"; decrements as the pane ticks | No error expected |
| One tick, many rows | three rows with future deadlines | exactly **one** 1000 ms interval for the whole pane; all three texts move on one tick | No error expected |
| No countdown anywhere | no row has a deadline | **no** 1000 ms interval is armed | No error expected |
| The last deadline passes | every deadline crosses zero | cells read `due`; the interval is cleared | No error expected |
| Pinned | ledger row `pinned = 1` | `hold = "Pinned"`, `releasesAfterMs = null`; cell shows the word, **no** timer | No error expected |
| Never confirmed upstream | `local_origin = 1`, `synced_at_ms IS NULL` | `hold = "Not sent"`; the sentence names FR-341's guarantee; **no** timer at any age | No error expected |
| TTL disabled | `release_ttl_ms = 0` | `hold = "Kept"` with the indefinite sentence for every materialized row | No error expected |
| Mode keeps content | `LfsMode::Materialize` or `Disabled` | `hold = "Kept"` with the mode sentence — the sweep releases nothing in this mode | No error expected |
| Materializing row | `sync.status === "materializing"`, ledger row present | `release` is `None`: indeterminate, no deadline, no words | No error expected |
| Virtual row with a stale ledger row | `"virtual"` + a `materialized` row | `release` is `None` — content is not here to release | No error expected |
| Skewed / expired deadline | `releasesAfterMs` in the past | `due`, never a negative figure | No error expected |
| Unrenderable deadline | non-finite, `<= 0`, beyond `MAX_DATE_MS` | the cell renders nothing at all and names no id in `aria-describedby` | No error expected |
| The ledger read fails | `Engine::release_schedules` errors | `tracing::warn!` and an empty map: rows carry no `release`, the listing still lists | never fails a listing |
| Narrow column, 220px | budget 0 | no verbs promoted, release cell and date both `sr-only` with their ids intact; every verb in the menu | No error expected |
| Default column, 360px | budget 122 | a materialized row draws its countdown and promotes one verb; a plain row still promotes all three and still only speaks its date | No error expected |
| Wide column, 700px | budget 462 | a materialized row draws the countdown, promotes all five verbs, and draws its date | No error expected |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/engine.rs` — `release_mode_gate` `:8639-8654` (the mode fact, reused not copied); `release_due_at` `:8685-8695` with its FR-341 doc `:8656-8684` (**not modified**); the sweep's one reader `:6293-6297`; `Engine::materialized_paths` `:8178-8183` and its doc `:8159-8177` naming the wider need. **Gains** `pub enum ReleaseSchedule` + `release_schedule(row, ttl_ms, mode_keeps)` beside `release_due_at`, and `Engine::release_schedules(profile_id)` beside `materialized_paths`.
- `src-tauri/crates/keeper-sync/src/db.rs` — `MaterializedRow` `:767-801` (all eight columns, `local_origin` `:800`); `materialized_rows` `:811` (the rich reader, one statement); `set_pinned` `:712`. **Not modified.**
- `src-tauri/crates/keeper-sync/src/profile/mod.rs` — `release_ttl_ms` `:922`, `effective_release_ttl_ms` `:1089-1091` (`None` = the sweep is off). **Not modified.**
- `src-tauri/crates/keeper-sync/tests/release_sweep.rs` — `engine_with` (custom TTL), `remember`, `ledger_conn`, `pin`, the real-git fixture. **Gains** the real-engine integration test for `release_schedules`.
- `src-tauri/crates/keeper-core/src/vm.rs` — `FilesEntryVm` `:4066-4156` with `mtime_ms`'s `#[ts(type = "number | null")]` reasoning `:4130-4134`; `FilesEntryFacts` `:4167-4189`; `FilesEntryVm::new` `:4225-4262` and the directory-drops-its-size shape `:4247-4251`; `FilesSyncStatusVm`; the eight in-crate `FilesEntryFacts` literals in `mod tests` (`:7026`, `:7047`, `:7510`, `:7581`, `:7617`, `:7630`, `:7664`, `:7695`). **Gains** `FilesReleaseVm`, the field, the fact, the gated pass-through.
- `src-tauri/crates/keeper/src/sync_ipc.rs` — **shell crate, macOS gate.** `sync_browse` `:1959-2037` and the 56.7 ledger read `:1998-2013` (the shape and the skip the new read copies); `files_listing_vm` `:2056-2138` and its facts literal `:2099-2110` (**`entry.relative_path` is moved at `:2101` — look the schedule up first**); `sync_materialize_entry` `:2411-2441` (the command template, `spawn_blocking`); `sync_ipc_error` `:766-810` with its `Refused` arm `:804` (**unchanged**). **Gains** the schedule read, one `files_listing_vm` argument, `sync_release_entry`, `sync_pin_entry`.
- `src-tauri/crates/keeper/src/lib.rs` — **shell crate.** `keeper_with_commands!` `:700-702`; the desktop splice `:908-1085` with 56.3's entry `:933-937`; the nine-entry `#[cfg(not(desktop))]` splice `:1091-1101` (**nothing added: `mod sync_ipc` is desktop-gated, and the test asserts mobile ⊆ desktop, never the reverse**).
- `src/test/command-registration.test.ts` — the three-way join; `invoke<T>("name"` must be a string literal `:53-58`; desktop registration is parsed from the exact `#[cfg(desktop)]\n    let builder = keeper_with_commands!(` anchor `:78-95` — do not reflow those lines. **Not modified; must pass.**
- `src/lib/ipc/client.ts` — `invoke` `:434-449`; `syncMaterializeEntry` `:3284-3286` (two-arg void template); `sessionsSetPinned` `:6175-6181` (three-arg boolean template). **Gains** `syncReleaseEntry`, `syncPinEntry`.
- `src/components/layout/files-pane.tsx` — `FILES_SIZE_SLOT` `:341`, `FILES_MTIME_SLOT` `:347`, `FILES_MTIME_FUTURE_GRACE_MS` `:366`; the budget constants `:556-650` (`FILES_ROW_META_PX` `:604`, `FILES_ROW_MTIME_PX` `:623` and its stated order, `FILES_ROW_MAX_ACTIONS` `:636` **deleted**, `FILES_NAME_FLOOR_PX` `:650`); `filesRowActionsBudget` `:679-693` (**unchanged**); `filesRowShowsModified` `:710-715` (**replaced** by `filesRowCellPlan`); `FilesRowAction` `:718-728`; the IPC imports `:141-149`; `writeError` `:861` and its sink `:1406-1408`; the clock `:827-831`; `nodes` `:1195-1198`; `load` `:939-970`; `treeWidth` `:1356`; the ids `:1754-1757`, `modified` `:1779-1782`, `describedBy` `:1789-1797`, `actions` `:1810-1855`, `showsModified` `:1859-1860`, `promoted` `:1875-1882`; the size cell `:2005-2016`, the mtime cell `:2034-2046`, the mark `:2050`, the hover cluster `:2092-2112`, the menu `:2146-2170`; the alert `:2367-2376`.
- `src/components/chat/undo-send-pill.tsx` — `secondsLeft` `:25-28`, the one shared interval `:39-48` (lazy `useState`, scalar dependency, early return), `now` as a prop `:85-96`, the `figures` numerals. **The shape copied; not modified.**
- `src/lib/format-time.ts` — `MAX_DATE_MS` `:26`, `formatDraftAge` `:84`. **Gains** `formatReleaseIn` and `formatReleaseSpoken`; `src/lib/format-time.test.ts` gains their cases.
- `src/components/layout/sync-pane.tsx` — `formatSyncWaited` `:586-606`, the coarse duration ladder and its `Math.max(0, …)` skew clamp. **The vocabulary inverted; not modified.**
- `src/components/layout/files-pane.test.tsx` — the exhaustive `vi.mock("@/lib/ipc/client")` factory `:11-39` (**a new wrapper must be added to both lists**); the uncast `entry()` fixture `:208-233` with `...extra` last; `withTreeWidth` `:974-991` (**hoisted to module scope for reuse**); `verbs(row)` `:1016-1020` (a hardcoded allow-list — new verbs are invisible until added); the geometry tests `:1022-1145`; the menu suite `:2127+`; `mtimeOf` `:1663-1666`; the local fake-timer discipline `:2300-2305`.
- `dev/mock-shell.ts` — `browseEntry` `:193-211` (typed, uncast), `lfsEntry` `:227-235`, the three 56.7 rows `:267-286`. **Gains** `release` on every literal and two reviewable rows: a counting one and a pinned one.
- `src/components/export/export-controls.test.tsx` — the third uncast `FilesEntryVm` fixture `:94-120`. **One field.**

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` — add `pub enum ReleaseSchedule { Due { at_ms: i64 }, Pinned, Unconfirmed, Indefinite, ModeKeeps }` with `releases_after_ms()`, `hold()` and `sentence()`, and the pure `release_schedule(row, ttl_ms: Option<u64>, mode_keeps: bool)` beside `release_due_at`: pin first (an absolute floor, asked here as well as inside `release_due_at`), then the mode, then the TTL, then `release_due_at`'s answer — whose remaining `None` can only be FR-341's never-confirmed row, stated in a comment. Add `Engine::release_schedules(&self, profile_id) -> Result<HashMap<String, ReleaseSchedule>>` beside `materialized_paths`, over `db::get_profile` + `effective_release_ttl_ms` + `release_mode_gate` + `db::materialized_rows`. -- The deadline is resolved once, in Rust, from the only row type that carries `local_origin`.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` (tests) — unit-test every variant: pinned wins over a due clock and over the mode; a locally-authored row with `synced_at_ms IS NULL` is `Unconfirmed` at any age; a remote-origin row is `Due` at `last_used_ms + ttl` and at `at_ms + ttl` when `last_used_ms` is `NULL`; `ttl_ms = None` is `Indefinite`; `mode_keeps` is `ModeKeeps`; and **`releases_after_ms().is_some() == hold().is_none()` for all five variants**. -- The invariant the shell then relies on, proven where a compiler runs.
- [x] `src-tauri/crates/keeper-sync/tests/release_sweep.rs` — a real-git, real-engine test: materialize a committed pointer, read `Engine::release_schedules`, assert the path's `Due { at_ms }` is exactly the clock the ledger row names plus the profile's TTL; `Engine::pin_entry` it and assert the same path reads `Pinned` with no instant. -- The epic's rule: a state nothing reaches from a real engine is a state nobody has.
- [x] `src-tauri/crates/keeper-core/src/vm.rs` — add `FilesReleaseVm { releases_after_ms: Option<i64> with #[ts(type = "number | null")], hold: Option<String>, detail: String }`, documenting the exactly-one invariant and where it is proven, and why a countdown is the one string Rust cannot own. Add `FilesEntryVm.release: Option<FilesReleaseVm>` after `mtime_ms`, the twin on `FilesEntryFacts`, and the gated pass-through in `new` — `None` unless `!is_dir && sync.status == FilesSyncStatusVm::Materialized` — plus the constructor-doc paragraph. Extend the eight in-crate facts literals. -- One field, one gate, in the crate that compiles here.
- [x] `src-tauri/crates/keeper-core/src/vm.rs` (tests) — assert the gate: a `Materializing` row, a `Virtual` row and a directory each drop a `release` that was handed in, and a `Materialized` file keeps it. -- 56.7's rule that materializing promises no finish time, enforced rather than asserted.
- [x] `src/lib/ipc/gen/**` — regenerate with `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core export_bindings`; commit unedited. Confirm `releasesAfterMs: number | null` (**not `bigint`**). -- Generated bindings are produced, never written.
- [x] `src-tauri/crates/keeper/src/sync_ipc.rs` — read `Engine::release_schedules` beside the 56.7 ledger read, with the same `tracing::warn!`-and-degrade and the same `unavailable` skip; pass it to `files_listing_vm` as one argument and look each entry up **before** `relative_path` is moved. Add `sync_release_entry` (`dehydrate_entry` is `async` — `.await` it, no `spawn_blocking`; `AlreadyPointer` returns `Ok(())` with an `info!`) and `sync_pin_entry` (blocking — `spawn_blocking`, `sync_materialize_entry`'s body verbatim), both `warn!`ing the refusal and returning `sync_ipc_error(&err)`. -- Shell crate, macOS gate.
- [x] `src-tauri/crates/keeper/src/lib.rs` — register `sync_ipc::sync_release_entry` and `sync_ipc::sync_pin_entry` in the **desktop** splice beside `sync_ipc::sync_materialize_entry`, with a comment naming the story and FR-343. Nothing in the `#[cfg(not(desktop))]` splice. -- Shell crate; the outage this macro exists to prevent.
- [x] `src/lib/ipc/client.ts` — add `syncReleaseEntry(id, subpath)` and `syncPinEntry(id, subpath, pinned)` beside `syncMaterializeEntry`, command names as string literals, TSDoc ending in the `Rejects with:` enumeration and the sentence that each rejection's message is Rust's own words shown verbatim. -- The third leg of the join `command-registration.test.ts` pins.
- [x] `src/lib/format-time.ts` (+ `format-time.test.ts`) — add `formatReleaseIn(deadlineMs, now = Date.now())` returning `""` / `due` / `Ns` / `N min` / `N hr` / `1 day`|`N days`, and `formatReleaseSpoken` returning the sentence form; both pure, both clamped, both guarded by `MAX_DATE_MS`. Test each rung, the boundaries, a past deadline, a skewed clock and every unrenderable input. -- A duration is the frontend's job and a pure function is where it belongs.
- [x] `src/components/layout/files-pane.tsx` — replace the single `Date.now()` with `useState(() => Date.now())` and **one** effect arming one 1000 ms interval, gated on a boolean derived from `nodes`; delete `FILES_ROW_MAX_ACTIONS` and `filesRowShowsModified` and add `FILES_ROW_RELEASE_PX` and the exported pure `filesRowCellPlan`, rewriting the stated-order docs to name the two forced changes and why; add the three verbs to the one `actions` array with `setWriteError(null)` before and a re-browse of the parent after; add the release cell (own id in `aria-describedby`, own slot, `figures`, `aria-hidden` figure plus `sr-only` words, Rust's sentence as `title`, `sr-only` when unpainted, absent when the formatted string is `""`); import the three client wrappers. -- FR-343: the verbs and the countdown, inside the budget.
- [x] `src/components/layout/files-pane.test.tsx` — hoist `withTreeWidth`; extend the IPC mock factory and `verbs()`'s allow-list; extend `entry()` with `release: null`. Add: the verb sets per state, each verb present in **both** the cluster (wide column) and the menu; a Materialize/Release/Pin that re-browses; a refused Release whose sentence reaches the alert verbatim; an `AlreadyPointer` release that shows no alert; **exactly one 1000 ms interval for three counting rows, counted by spying on `setInterval`**; no interval when nothing counts; every row's text moving on one tick; the same listing showing less time left after the clock advances; a pinned row and a never-confirmed row showing the word and no digit; a materializing row showing no release cell; the cell carrying no `animate-`/`transition-` class; and the retuned geometry expectations at 220/320/360/480/700. -- The claims of this story are perceptible ones, so they are asserted through the rendered row.
- [x] `dev/mock-shell.ts`, `src/components/export/export-controls.test.tsx` — add `release` to every `FilesEntryVm` literal; give the harness's materialized row a live deadline and add a pinned row, so the countdown and the words can both be looked at. -- A state nothing can show is a state nobody reviews.

**Acceptance Criteria:**
- Given a virtual row and a materialized row, when the tree is wide enough to promote them, then Materialize appears on the first and Release and Pin on the second, in **both** the hover cluster and the context menu, from the one `actions` array.
- Given a materialized row with a live TTL, when the pane ticks, then its countdown decrements, and a test that counts timers proves exactly one 1000 ms interval exists for the whole pane however many rows are counting.
- Given a pane where no row has a deadline, when it renders, then no 1000 ms interval is armed at all.
- Given one listing and a clock advanced by an hour, when the row re-renders, then it shows less time remaining — the deadline is an instant, not a duration.
- Given a pinned row, a `synced_at_ms IS NULL` row and a profile whose `release_ttl_ms` is `0`, when they render, then none shows a countdown or any digit, and each says in Rust's words why.
- Given a materializing row, when it renders, then it carries no release cell and promises no finish time.
- Given a Release that hits one of 56.4's five refusals, when it is refused, then the pane shows that refusal's own sentence verbatim rather than a generic failure.
- Given `src/lib/ipc/gen/FilesEntryVm.ts` and `FilesReleaseVm.ts`, when read after regeneration, then `releasesAfterMs` is `number | null` — not `bigint`, not a string, not a duration.
- Given `bun run test`, when it runs, then `command-registration.test.ts` passes with the two new commands registered on the desktop call site and wrapped in `client.ts`.
- Given `cargo clippy -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings`, when it runs, then it is clean; and the three crates' tests pass with no fewer than 3483 passing.
- Given `bun run typecheck`, `bun run lint` and `bun run test`, then typecheck is clean, lint is at baseline (4 warnings + 1 info, the same five files), and the suite is green with the new assertions.

## Spec Change Log

## Review Triage Log

### 2026-08-25 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 20: (high 2, medium 7, low 11)
- defer: 5: (high 0, medium 2, low 3)
- reject: 4: (high 0, medium 1, low 3)
- addressed_findings:
  - `[high]` `[patch]` The pane's clock FROZE for the whole session on any tree with no live countdown, because `nowMs` became state advanced only by the interval and the interval is armed only while a row counts — which the ordinary Files tree never does. Every relative date was stale by however long the pane had been open, and story 56.7's future-mtime guard made a file written more than a minute after mount render **no date at all** and drop out of `aria-describedby`. Before this story every render re-read the clock. The tick's only job is now to CAUSE a paint (`tickedMs` state, read by nothing but `counting`); `nowMs` is a fresh read per paint again, which is what 56.7's rule asked for — one read per PAINT, never one per row. Pinned by a test that ages a date on a tree with nothing to count.
  - `[high]` `[patch]` A materialized row at 320px promoted **zero** verbs, breaking the pinned two-verb guarantee this file's own doc still asserted four lines above the new constant — and making the plan non-monotonic in width, so widening 305px→306px took a verb away. The release cell is now charged only when the budget covers the cell **and one verb** (104px), so at 320px the cell goes `sr-only` and the row keeps both verbs, while the shipped 360px default still paints the countdown and promotes one. `FILES_ROW_MTIME_PX`'s stale paragraph was restated to describe the guarantees that hold now.
  - `[medium]` `[patch]` The date's new per-row gate made a **folder** draw a date the files beside it could not, between 378px and 413px — inside the drag range, and the exact raggedness the doc this story replaced forbade — for a row that gained neither a cell nor a verb. The gate now floors the verb count at an ordinary file's three (`FILES_ROW_BASE_ACTIONS`), so a folder and a plain file are bit-identical to their pre-story threshold (414px) and only a row that GAINED verbs pays more, which is what "paid for by the row that gained a cell" was supposed to mean.
  - `[medium]` `[patch]` `ReleaseSchedule::ModeKeeps` told an LFS-**disabled** folder that it "is set to keep large-file content on this computer" — the opposite of its configuration, sending the reader to look for a setting that reads the other way. `release_mode_gate` refuses `Materialize` and `Disabled` alike and the boolean collapsed them. Split into `ModeKeeps` and `LfsOff`, each with its own sentence and the same word; `release_schedule` now takes `LfsMode` and does the mapping in an exhaustive `match`, so a fourth mode must decide rather than inherit. Reachable: a folder switched away from `PointerOnly` keeps its ledger rows and `classify` still marks them materialized.
  - `[medium]` `[patch]` `formatReleaseIn` emitted `"60s"`, so the last minute counted **up** in unit terms — `1 min` → `60s` → `59s` — because the seconds rung used `Math.ceil` under a `< 60_000` boundary. Now `Math.max(1, Math.floor(…))`: one floor rule shared with the minutes, hours and days rungs, the last millisecond still reads `1s`, and a test walks the whole final minute asserting the figure never increases rather than pinning two endpoints.
  - `[medium]` `[patch]` Neither formatter guarded `now`, so a `NaN` or `±Infinity` clock made every rung comparison false and the cell rendered `"NaN days"`. Guarded beside the deadline guard, `""` either way; `formatReleaseSpoken` inherits both through its single call rather than repeating them, and its doc now says so.
  - `[medium]` `[patch]` `counting` armed the interval for deadlines the cell refuses to draw — a `releasesAfterMs` past `MAX_DATE_MS` ticked the pane once a second for its whole life painting nothing — and for a row carrying a `hold` beside a deadline, over a word that can never change. One module-scope `releaseIsCounting` now asks exactly the cell's question, so the effect and the cell cannot drift.
  - `[medium]` `[patch]` A counting row never SPOKE Rust's caveat. `release.detail` reached nothing but the `title`, which a reader with a description in hand generally does not hear — and that sentence is the whole reason the countdown exception costs nothing, because it is what says reaching zero makes the content *eligible* rather than gone. A counting row now speaks the duration and the sentence together; a held row still speaks the sentence alone.
  - `[medium]` `[patch]` The exactly-one invariant was asserted by a hand-written five-variant array, so a sixth variant answering `None` to both accessors would compile, ship, and leave a materialized row with a release cell and nothing in it — while `releases_after_ms`' own doc claimed the pairing was proven over every variant. Both accessors now read one exhaustive `instant_or_words(&self) -> Result<i64, &'static str>`: "both" and "neither" are no longer expressible, and a new variant is a compile error in exactly one place.
  - `[low]` `[patch]` `Engine::release_schedules` took two separate database locks for one answer, so a TTL or mode edit landing between them classified every row in a listing against a folder shape that no longer existed. Folded into one `with_db`.
  - `[low]` `[patch]` Four of the five `ReleaseSchedule` variants carried no doc comment, in a crate where every struct field gets a paragraph — and the two that most needed one are the two that render the same word and differ only in a sentence. All six documented, each of the three sharing `"Kept"` naming what tells it apart.
  - `[low]` `[patch]` One field, two guards: `releaseShort` treated `hold` nullishly while `releaseSpoken` tested it strictly, so a `hold` of `undefined` gave the eye a countdown and the ear a sentence. Normalised once, and the comment now frames the pair as a defence downstream of Rust's proof rather than claiming complementarity.
  - `[low]` `[patch]` The `AlreadyPointer`-is-a-success path had no test at all, though this spec's own task list and its matrix both require one and the Rust half is in the crate that cannot be compiled here. Added: no alert, and the folder re-read.
  - `[low]` `[patch]` Only Materialize asserted the re-browse that is these verbs' only feedback, so `runRowVerb`'s success arm was proven for one verb of three. Release and Pin now assert it too.
  - `[low]` `[patch]` Every test in the new suite ran at 700px, so the `sr-only` branch that the "nothing that cannot be painted is lost" invariant lives in was never rendered, and neither was the 360px shape this spec's own matrix calls the headline case. Added the 320px, 360px and 480px rows through the real DOM.
  - `[low]` `[patch]` Two matrix rows had no test on either side: a row that reads `due` with the interval cleared, and an unrenderable deadline drawing no cell and naming no id. Both added.
  - `[low]` `[patch]` Both zero-interval assertions were made synchronously right after the arrangement resolved, while the matching presence assertion conceded it needed `waitFor` — an interval armed one state update later satisfied both. They now settle, assert, advance five periods, and assert again.
  - `[low]` `[patch]` The `setInterval` spy was filtered on the 1000 ms period, which is the right defence against unrelated timers but let any mutation that changes the period escape the check entirely — including a variant of this story's own recorded mutation proof. The one-interval test now also pins the TOTAL against the same tree with nothing counting, and a mutation at a 250 ms period was confirmed to fail only that line.
  - `[low]` `[patch]` The classifier's unit test omitted the confirmed locally-authored row — the one case FR-341 permits — leaving the `synced_at_ms = Some(t)` clock covered only by delegation. Added, with a huge `last_used_ms` beside it so a fallback to the use clock fails.
  - `[low]` `[patch]` `FILES_ROW_MTIME_PX`'s doc still presented the 320px two-verb guarantee as unconditional immediately above the constant that interacts with it. Restated to describe the order that now holds, with the new rung named.

## Design Notes

**Why a classifier and not a second clock.** `release_due_at` already answers the only hard question — which of 56.5's two clocks applies, and that a locally-authored unconfirmed row is never eligible. `release_schedule` adds no arithmetic: it asks the pin, the mode and the TTL (three facts the sweep also asks, in the same order the sweep asks them), then defers to `release_due_at` and names its remaining `None`.

```rust
pub fn release_schedule(
    row: &db::MaterializedRow,
    ttl_ms: Option<u64>,
    mode_keeps: bool,
) -> ReleaseSchedule {
    // The pin is an absolute floor, asked here as well as inside
    // `release_due_at`: a surface must say "pinned" rather than "never", and a
    // second read of one boolean is cheaper than a classification that depends
    // on another function's internals.
    if row.pinned {
        return ReleaseSchedule::Pinned;
    }
    if mode_keeps {
        return ReleaseSchedule::ModeKeeps;
    }
    let Some(ttl_ms) = ttl_ms else {
        return ReleaseSchedule::Indefinite;
    };
    match release_due_at(row, ttl_ms) {
        Some(at_ms) => ReleaseSchedule::Due { at_ms },
        // `release_due_at` answers `None` for exactly two rows: a pinned one,
        // refused above, and FR-341's locally-authored row the remote has never
        // been observed holding. So this arm is that row and only that row.
        None => ReleaseSchedule::Unconfirmed,
    }
}
```

**Why the exactly-one invariant lives in `keeper-sync`.** The shell crate does not compile on this host, so a mapping written there is a mapping no test can reach. `ReleaseSchedule::releases_after_ms()` / `hold()` are the mapping, they are `Option`s with a proven complementary emptiness, and `sync_ipc` is left with three field reads.

**Why the countdown is charged before the verbs, and the date against this row's own count.** `filesRowShowsModified`'s doc asks the date against the *full* verb count, so a date never appears on one row and vanishes on the next. Both halves of that rule break once the verb count depends on the row's sync state, and they break in opposite directions: keeping the uniform maximum (now five) unpaints the date at every shipped width, including the 480px row 56.7 pinned; keeping the date ahead of the release cell unpaints the countdown at the 360px default, which is the story. So the order is stated afresh — name floor, release, verbs, date — the release cell is refused outright when the budget cannot cover it (so the floor is never touched), and the date asks this row's own count, which is exactly the raggedness the old doc argued against, now paid for by the row that gained a cell. Every figure is still spoken at every width and every verb is still in the menu, so what changes is which facts are *painted*.

**Why there is no live region.** `undo-send-pill` seeds a frozen `aria-live` region because a pill appears for a few seconds and the deadline is the whole point of it. A Files tree renders a windowed list of hundreds of rows: a live region per counting row would announce on every scroll, which is worse than the per-second announcement the rule exists to prevent. The rule's *purpose* — a tick never announces — is met by having no live region at all, and the countdown is spoken on demand through the row's `aria-describedby`, which is how the size, the count and the date are already spoken. The rest of the pill's shape is copied exactly: a module-scope pure helper taking an injected clock, one container-owned interval with a scalar dependency and an early return, `figures` numerals, and the text rendering whatever the motion preference is.

**Why Pin is one-way.** `pin_entry` is idempotent and the wire carries no pin bit; a row's pinned-ness reaches the frontend only as Rust's word. An Unpin verb therefore needs either a new wire field or a verb that cannot say whether it will pin or unpin. Neither is this story's ask, and the countdown vanishing into the word `Pinned` after a Pin is the feedback that matters. `keeper-syncd unpin` remains the door; deferred with the field it would add.

## Verification

**Commands:**
- `cargo fmt --manifest-path src-tauri/Cargo.toml --all` -- expected: no diff afterwards.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings` -- expected: clean.
- `GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=keeper GIT_AUTHOR_EMAIL=dev@keeper.local GIT_COMMITTER_NAME=keeper GIT_COMMITTER_EMAIL=dev@keeper.local cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync -p keeper-core -p keeper-syncd` -- expected: 0 failed, no fewer than 3483 passing.
- `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core export_bindings` -- expected: `src/lib/ipc/gen/FilesEntryVm.ts` and `FilesReleaseVm.ts` only.
- `bun run typecheck` -- expected: clean.
- `bun run lint` -- expected: baseline (4 warnings + 1 info, the same five files).
- `bun run test` -- expected: green, including `src/test/command-registration.test.ts`.
- `git status --porcelain -- src/lib/ipc/gen` -- expected: exactly the two files the export test produced.

**Manual checks (if no CLI):**
- `src-tauri/crates/keeper/**` cannot be compiled or tested here. Symbols for `bun run check:rust:macos`: `sync_ipc::sync_browse`, `sync_ipc::files_listing_vm`, `sync_ipc::sync_release_entry`, `sync_ipc::sync_pin_entry`, and the two `lib.rs` registration lines.
- Mutation proof, four rows, each with its own single test run and each restore verified by reading `git diff`: (a) drop `#[ts(type = "number | null")]` from `releases_after_ms`, regenerate, and record the emitted TypeScript type and the failure; (b) move the interval from the pane into the row and confirm the one-interval test fails; (c) treat the wire value as a duration and confirm the staleness test fails; (d) drop the hold branch and confirm the no-fake-timer test fails.

## Auto Run Result

Status: done

**What was implemented.** The Files row gained the verbs its state has and a live countdown to automatic release. A virtual row offers **Materialize**; a materialized row offers **Release** and **Pin**; all three are entries in the pane's single `actions` array, so the promoted hover cluster and the Radix context menu get them from one list, with one handler, one accessible name and one width budget. Unlike the three verbs already there they report rather than swallow: on success the parent folder is re-read — the row's mark and its countdown are the feedback — and on refusal story 56.4's own sentence reaches the pane's one `role="alert"` Alert verbatim, with `ContentRefusal::AlreadyPointer` answered `Ok` because there was nothing to release.

The countdown crosses the boundary as **one absolute epoch-ms instant**. `keeper_sync::engine::ReleaseSchedule` classifies each `materialized` ledger row **over** 56.5's `release_due_at`, adding no arithmetic: the pin first, then the folder's LFS mode, then the TTL, then the clock — and the remaining `None` can only be FR-341's locally-authored row the remote has never been observed holding. Six variants, and the ones with no instant carry Rust's own word plus its own sentence. The pairing is structural rather than tested: one exhaustive `instant_or_words(&self) -> Result<i64, &'static str>` feeds both accessors, so "both" and "neither" are not expressible and a seventh variant is a compile error in exactly one place. `keeper-core` carries the resolved pair on `FilesEntryVm.release`, typed `#[ts(type = "number | null")]`, and `FilesEntryVm::new` drops it for a directory and for every sync state but materialized — the way it already drops a directory's size — so a materializing row promises no finish time.

TypeScript renders the duration, because a countdown is stale the instant it is serialized and the Files tree does not poll at all. One shared 1 s interval owned by the pane, armed only while a row still counts and never one per windowed row, plus two pure formatters beside `formatDraftAge`. No animation, so the figure reads as text under any motion preference; no live region, so a tick never announces itself and the fact is spoken on demand through the row's `aria-describedby` like every other cell.

`filesRowCellPlan` replaced `filesRowShowsModified` as the row's one cell planner. Its order is the name's floor, then the release cell, then the verbs, then the date; the release cell is charged only where the row can also pay for one verb, so no row gives up its last promoted verb for it, and the date is charged against at least an ordinary file's three verbs, so a folder never draws a date its file siblings cannot.

**Files changed.**
- `src-tauri/crates/keeper-sync/src/engine.rs` — `ReleaseSchedule` (six variants, `instant_or_words`, three accessors), the pure `release_schedule` classifier, `Engine::release_schedules` over one `with_db`, and their unit tests.
- `src-tauri/crates/keeper-sync/tests/release_sweep.rs` — a real-git, real-engine test: the instant the sweep itself would use, and a `pin_entry` that replaces it.
- `src-tauri/crates/keeper-core/src/vm.rs` — `FilesReleaseVm`, the field, the fact, the gated pass-through, and the test that pins the gate.
- `src-tauri/crates/keeper/src/sync_ipc.rs` — the schedule read beside 56.7's ledger read, one `files_listing_vm` argument, `sync_release_entry` and `sync_pin_entry`. **Shell crate: no compiler ran on this host.**
- `src-tauri/crates/keeper/src/lib.rs` — both commands on the desktop `keeper_with_commands!` splice, nothing on the non-desktop twin. **Shell crate.**
- `src/lib/ipc/gen/FilesEntryVm.ts`, `src/lib/ipc/gen/FilesReleaseVm.ts` — regenerated by the ts-rs export test, never hand-edited.
- `src/lib/ipc/client.ts` — `syncReleaseEntry`, `syncPinEntry`, and `FilesReleaseVm` re-exported.
- `src/lib/format-time.ts` (+ its test) — `formatReleaseIn` and `formatReleaseSpoken`, both pure, both clamped, both guarded on the deadline and on the clock.
- `src/components/layout/files-pane.tsx` (+ its test) — the pane's one tick, the three verbs, `runRowVerb`, the release cell, `filesRowCellPlan`, `releaseIsCounting`, and the retuned width policy.
- `dev/mock-shell.ts`, `src/components/export/export-controls.test.tsx` — the new wire field, plus a counting row and a pinned row so both shapes of the cell can be looked at.

**Review findings.** intent_gap 0, bad_spec 0, patch 20 (2 high, 7 medium, 11 low) all applied, defer 5, reject 4. See the Review Triage Log. The two high findings were regressions this story introduced into a previous story's work: a clock that froze for the session on any tree with nothing to count, and a materialized row that promoted zero verbs at 320px.

**Verification.** `cargo fmt --check` clean. `cargo clippy -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -D warnings` clean. Rust **3488 passed / 0 failed** (floor 3483). `bun run typecheck` clean. `bun run lint` at baseline — 4 warnings + 1 info, the same five files. `bun run test` **297 files / 4916 tests passed** (baseline 4877; +26 from the story, +13 from the review patches). `git status --porcelain -- src/lib/ipc/gen` shows exactly `FilesEntryVm.ts` modified and `FilesReleaseVm.ts` added, both produced by `cargo test -p keeper-core export_bindings`.

**Mutation proofs**, four rows, each its own single test run, each restore verified by reading `git diff`:

| mutation | observed |
|---|---|
| `#[ts(type = "number \| null")]` dropped from `releases_after_ms`, bindings regenerated | the binding emitted `releasesAfterMs: bigint \| null`; `bun run typecheck` failed in **four** places including both of the countdown's own formatter calls (`files-pane.tsx`, `Argument of type 'number \| bigint' is not assignable to parameter of type 'number'`) and both uncast fixtures. Vitest does not typecheck, so the vitest suite still passed — the guard is the typecheck gate, and a throwaway probe confirmed the runtime consequence: `formatReleaseIn(BigInt(…))` answers `""`, so a `bigint` on the wire deletes the countdown entirely rather than mis-rendering it. |
| the tick moved from the pane into the row (one interval per counting row) | `arms exactly one interval …` FAILED, `expected 1, + 15` |
| the deadline treated as a duration — the figure computed once per listing rather than per paint, which is exactly what a duration serialized by Rust gives | `shows less time left after the clock advances over one unchanged listing` FAILED at the advance assertion: `"3 hr"` was right at first paint and still read `"3 hr"` an hour later instead of `"2 hr"` |
| the hold branch dropped from the row's release derivation | `draws a word, no digit and no timer for a row on no clock` FAILED — `expected null not to be null`: the pinned row lost its cell and its words |

The review patches carry their own proofs, recorded by the agents that made them: the shipped `budget >= FILES_ROW_RELEASE_PX` rung fails the new 320px test with `expected [Open, Reveal], received []`; dropping `FILES_ROW_BASE_ACTIONS` reproduces the review's own 378-vs-414px numbers; reverting the clock to interval-only state fails the new ageing-date test with `received undefined`; a second interval planted at a **250 ms** period fails only the new total-pin line, which is the hole the period filter left.

**Shell-crate symbols for the macOS gate** (`bun run check:rust:macos`): `sync_ipc::sync_browse`, `sync_ipc::files_listing_vm`, `sync_ipc::sync_release_entry`, `sync_ipc::sync_pin_entry`, and the two `lib.rs` registration lines. Checked three other ways here: rustfmt parses both files, every changed region was re-read with fresh line numbers, and the only caller of `files_listing_vm` in the workspace is `sync_browse` — so no call site was left at the old arity. `src/test/command-registration.test.ts` passes, which is what pins the registration of both commands and their `client.ts` wrappers on Linux.

**Residual risks.** The `keeper` shell crate cannot be compiled or tested on this host. `SyncPlatform::open_file_state` still answers `Unknown` on every real host, so a manual Release refuses with that sentence today — story 56.4's recorded and deliberate consequence, and the reason one deferred entry argues for a disabled-with-a-reason affordance rather than a hidden verb. Five findings were deferred with evidence in `deferred-work.md`: Release offered where the wire already says it will refuse; `"Kept"` shared by a releasable and an unreleasable row; `sync_browse` scanning the `materialized` table twice per listing; `dehydrate_entry`'s whole-file hash running on the async runtime with no yield point; and a second row verb pressed before the first resolves erasing the first's sentence. Two load-sensitive flakes were observed on this box under parallel-agent load and neither reproduced in isolation: `markdown-preview.test.ts > constructs a real EditorView over a mermaid fence` and `keeper-sync`'s `git::resolve::tests::probing_stops_at_the_first_git_that_clears_the_floor` (three clean isolated re-runs each, and two clean full-suite runs of each gate afterwards). Both are unrelated to this story's surfaces and join the flakes the epic already logged.
