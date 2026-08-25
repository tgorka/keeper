---
title: '56.7 The row says what it is, and what a delete will do'
type: 'feature'
created: '2026-08-25'
status: 'in-review' # draft | ready-for-dev | in-progress | in-review | done | blocked
baseline_revision: '5c3ed39'
final_revision: ''
review_loop_iteration: 0
followup_review_recommended: true
context: []
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** Epic 56 created three real states and keeper's surfaces can name one of them, badly. `browse::EntrySyncStatus::Virtual` exists (56.2) and `sync_ipc::sync_mark` deliberately collapses it onto `FilesSyncStatusVm::Synced` (`sync_ipc.rs:2205-2209`), because the wire enum has no variant for it; nothing anywhere expresses **materializing** or **materialized**, so a path whose content 56.3 fetched and 56.5 will release looks exactly like an ordinary file. The row never renders the modification time 56.2 put on the wire (`mtimeMs` appears zero times in `files-pane.tsx`). And the reason the collapse was chosen rather than shrugged is the finding this story exists to close: `FilesDeletePlanVm::compose`'s `travels` filter is a **non-exhaustive** `matches!` over `Synced | Waiting | Unknown` (`vm.rs:4473-4483`), so a new wire variant compiles silently into the *"stays on this machine"* bucket — keeper would promise a local deletion while removing the pointer that **is** the tracked content, and the deletion travels (AD-134, FR-345).

**Approach:** Give both vocabularies all three states, answer every exhaustive site deliberately, and make the delete plan classify the new states **explicitly**. `EntrySyncStatus` gains `Materializing` and `Materialized` beside 56.2's `Virtual`; `FilesSyncStatusVm` gains all three; `sync_mark`'s one collapsed arm becomes three arms with three Rust-composed sentences; the three exhaustive `Record<FilesSyncStatusVm, …>` maps in `sync-status-mark.tsx` each gain three entries so each state has an accessible name, a distinct glyph and a verified tone; the Files row grows the modification-time cell beside the size it already renders; and `compose`'s `travels` names `Virtual | Materializing | Materialized` alongside `Synced | Waiting | Unknown`.

## Boundaries & Constraints

**Always:**
- **Three states in both vocabularies, and every exhaustive site answered.** The one hard Rust compile error for a new `EntrySyncStatus` variant is `sync_mark` (`sync_ipc.rs:2168`) — nothing else in the workspace matches over it. The one hard TypeScript error set for a new `FilesSyncStatusVm` variant is the three `Record` maps (`sync-status-mark.tsx:40`, `:49`, `:71`). Everything else that reads either enum is `==`, `matches!` or a hand-maintained list and will compile wrong rather than fail: `compose`'s `travels` (`vm.rs:4476-4481`) and `unclear` (`:4471`), `files-pane.test.tsx:1262-1268`/`:1285-1291`, `vm.rs:7178-7181`'s `[Excluded, NotInRepository]` local-only loop. Each is visited by hand.
- **No partial `Record`.** `Record<FilesSyncStatusVm, …>` is total on purpose (`sync-status-mark.tsx:33-39`: "the map is total so a state added in Rust cannot reach the screen nameless"). Widening one to `Partial<>`, adding an index signature, or falling back through `??` on a missing key is forbidden.
- **Shape carries the distinction; colour is emphasis only.** The file's own rule (`sync-status-mark.tsx:5-11`, `:57-70`): "remove every class and the marks are still five different shapes with five different names" — that count becomes **eight**, and the eight glyphs must be pairwise distinct. New tones use existing verified tokens (`text-faint` is held to 3:1 for exactly this job); no opacity modifier, no new colour.
- **The words are composed in Rust, never in TypeScript** (`sync-status-mark.tsx:19-22`, `sync_ipc.rs` `sync_mark`). Each new state's sentence is written in `sync_mark` and rendered as the mark's accessible name; the label map's entry is the fallback for a `detail`-less mark, not a second wording.
- **A virtual sentence claims nothing about the remote.** `EntrySyncStatus::Virtual`'s doc (`browse.rs:270-275`) — "**A claim about here, not about the remote.**" A pointer whose object never reached the server is a valid blob with a clean `git status`; `verify --remote` is the check that earns that claim. The same rule binds the materialized and materializing sentences.
- **`materialized` is earned by two facts, never one.** (a) The `materialized` ledger holds a row for the path — the record that keeper put content there and can release it, which is exactly what `db::forget_materialized` retracts and what 56.5's sweep consults — and (b) the worktree bytes are **not** the committed pointer. `VirtualPolicy` is deliberately not the source: its own doc says a `Virtual` answer is an authorization, never an instruction, so a plain never-LFS-tracked file matching a pattern would read materialized falsely.
- **`materializing` is a re-reading of an existing fact, not new plumbing.** `PendingReason::Incoming` is a queued LFS download and its own doc states "a queued download always finds pointer text in the worktree" (`engine.rs:247-265`). So `Incoming` **and** pointer text on disk is materializing; `Incoming` over a path that is not pointer text (queued for a path since deleted, or an unlabelled `LFS object <oid…>` row) stays `Waiting`.
- **`materializing` must not invent a percentage and must promise no finish time.** The pane's own rule (`files-pane.tsx:726-728`): "a node with no listing yet IS the in-flight state … a second flag tracking the same fact is a second thing to get out of step". This state gets no client-side flag, no interval, and no denominator. It renders the way `settings/sync-section.tsx:561-577` already renders an unknown total — indeterminate, with the Rust sentence as the value text and **no `aria-valuenow`**.
- **A virtual or materialized path travels, explicitly** (FR-345, AD-134). `compose`'s doc already states the rule the new variants inherit: *"**[`FilesSyncStatusVm::Unknown`] counts as syncing, and says so.** The two available guesses are "this deletion stays on this machine" and "this deletion travels", and only one of them is safe to be wrong about. Silently picking the quiet one would be the same lie [`FilesSyncStatusVm::Unknown`] was introduced to refuse."* (`vm.rs:4458-4462`)
- **The honest size already crosses the wire and must stay that way** (FR-336, AD-127). `browse::list_resolved` substitutes the pointer's size for a `Virtual` row (`browse.rs:775-780`) and `files_listing_vm` forwards `size_bytes`/`lfs_oid`/`mtime_ms` untouched (`sync_ipc.rs:2069-2071`). Nothing in this story may reintroduce `fs::metadata().len()` for a virtual path, and `lfs_oid` stays `Some` exactly when `size_bytes` is the pointer's number.
- **`src/lib/ipc/gen/**` is generated.** Regenerate by running the ts-rs export test in `keeper-core` (`TS_RS_EXPORT_DIR` from `.cargo/config.toml:5-6`) and commit the result. Never hand-edit. No new 64-bit wire field is added by this story, so no new `#[ts(type = "number")]` is needed.
- **The shell crate cannot be compiled on this host.** `keeper::sync_ipc` and `keeper::sessions_ipc` edits are written to be reviewed without a compiler and every touched symbol is reported for the macOS gate. This story adds no `SyncProfile` field, so `EXPRESSED`/`PRESERVED` and `a_save_cannot_move_a_field_no_request_can_express` are untouched.

**Block If:**
- The three states cannot be told apart with every colour class removed (glyphs collide or two share an accessible name).
- Making `Materialized` reachable would require `browse` to open a git repository — the one thing the module forbids (`browse.rs:816-819`).

**Never:**
- **Story 56.9's work.** No row verb for materialize, release or pin — the single `actions` array (`files-pane.tsx:1695-1741`) gains **no entry**. No live TTL countdown, no `secondsLeft` helper, no per-pane or per-row interval, no absolute release deadline on the wire (56.9 adds `releases_after_ms` and owns the pane's one tick). The seams stay: the `actions` array remains the single list feeding both the hover cluster and the context menu, and `FilesEntryVm` keeps room for one more field.
- No animation on the materializing mark. Rows are windowed (`useWindowedRows`, `files-pane.tsx:1402`), so a spinner arms and disarms on every scroll, and the `motion-reduce` discipline belongs with 56.9's countdown.
- No new `LfsFileState` variant. `lfs::listing`'s three-way `Virtual | Materialized | Absent` vocabulary and the `keeper-syncd ls-files` JSON key set (FR-337) are unchanged; a fourth state there would silently vanish from `ls_files_lines`' equality-counted summary.
- No second wording of any sentence in TypeScript. No `Partial<Record<…>>`. No `_` arm added to `sync_mark`.
- No change to `verify`, `copy`, the release sweep, the CLI, `docs/sync.md` (56.8 owns the chapter), or the sessions tree's own size source.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| A settled virtual path | In a repository, not excluded, nothing pending, worktree bytes are the committed pointer | `EntrySyncStatus::Virtual` → `FilesSyncStatusVm::Virtual`; row size is the pointer's number, `lfsOid` is `Some` | No error expected |
| Content on its way | Same path, and the journal holds an `lfsDownload` labelled with it (`PendingReason::Incoming`) | `EntrySyncStatus::Materializing` → `FilesSyncStatusVm::Materializing`; mark renders indeterminate, no `aria-valuenow` | No error expected |
| Content here, releasable | Ledger holds a `materialized` row for the path and the worktree bytes are **not** pointer text | `EntrySyncStatus::Materialized` → `FilesSyncStatusVm::Materialized`; size is the worktree's, `lfsOid` is `None` | No error expected |
| Released since | Ledger row present **and** worktree holds pointer text | `Virtual` wins — the pointer probe is asked before the ledger | No error expected |
| `Incoming` for a path that is not pointer text | Queued download labelled for a deleted path, or an unlabelled `LFS object …` row | `Waiting { Some(Incoming) }`, unchanged | No error expected |
| A user-authored pointer-shaped file | Untracked file holding pointer text | `Waiting { Some(Untracked) }` — precedence rule 4 is preserved | No error expected |
| The ledger could not be read | Engine answered `pending` but the ledger read failed, or the caller knows nothing | Empty `MaterializedView` → the path reads `Synced`: true, less specific, and still travelling in the delete plan | Never fails a listing |
| Delete plan over one virtual path | `files = [(path, Virtual, VaultTrash)]` | `travels == 1`, `local == 0`; consequence says the deletion removes it from every machine that syncs the profile | No error expected |
| Delete plan over a materialized path | `files = [(path, Materialized, SystemTrash)]` | `travels == 1`, `local == 0` | No error expected |
| Delete plan mixing travelling and local | One `Materialized`, one `Excluded` | "1 of these 2 files sync …; the other 1 do not and go from this machine only." | No error expected |
| A row with no modification time | `mtimeMs` is `null` (unreadable `stat`) | The mtime cell renders nothing at all — not a dash, not 1970 | No error expected |
| A directory | `is_dir` | No pointer, no ledger row → never `Virtual`/`Materializing`/`Materialized`; keeps its `Waiting` roll-up; still carries an mtime, still no size | No error expected |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/browse.rs` — `EntrySyncStatus` `:249-292` (**gains** `Materializing`, `Materialized`; enum doc `:244-248` records the extension). `PendingView` `:300-311` and `waiting` `:333-350` (**unchanged** — the precedent for a view a caller must supply rather than default, `:646-650`). **Gains** `MaterializedView` beside it. `BrowseEntry` `:115-226`: `size_bytes` `:169` with the pointer-substitution doc `:136-140`, `lfs_oid` `:184`, `mtime_ms` `:205` (**all unchanged**). `browse` `:603-633`, `browse_root` `:651-659`, `list_resolved` `:668-810` (the bound `stat` `:731`, `mtime_ms` `:744`, the pointer closure `:758-771`, the size override `:775-780`, the struct literal `:781-791`), `status_of` `:877-899`, `classify` `:938-962` with its six-rung precedence doc `:901-937` and the `FnOnce` rationale `:927-937` — **all five signatures gain `materialized: &MaterializedView`, and `holds_pointer` becomes `FnMut` because two rungs may ask and at most one ever does.** Tests: `profile` `:973`, `no_excludes` `:982`, `nothing_pending` `:989`, `names` `:993`, `marks` `:1800-1809`, `pointer_text_is_virtual_and_every_more_specific_answer_beats_it` `:1508`, `each_state_comes_from_state_that_already_existed` `:1819`, `a_file_carries_its_byte_count_unless_its_bytes_are_a_pointer` `:1397`.
- `src-tauri/crates/keeper-sync/src/engine.rs` — `PendingReason::Incoming { size_bytes, replacing }` `:265` and its "a queued download always finds pointer text in the worktree" doc `:247-265` (**unchanged**, and the whole evidence for `Materializing`). `Engine::pending` `:7958`, its `held`/`queued_downloads` tail `:8128-8151`. `with_db` and `list_profiles` are the sync-accessor precedent. **Gains** `Engine::materialized_paths(&self, profile_id) -> Result<HashSet<String>>` — one `db::materialized_paths` read, sync like `list_profiles`, no walk.
- `src-tauri/crates/keeper-sync/src/db.rs` — `materialized_paths` `:738-753` (the narrow one-bit-per-path reader, doc `:706-710`); `forget_materialized` `:881-888` (the retraction that makes a row mean "content is here"). **Not modified.**
- `src-tauri/crates/keeper-sync/src/lfs/listing.rs` — `LfsFileState` `:45-70` and the "a path the Files pane calls virtual is a path this verb calls virtual" identity `:172-180`. **Not modified**; the pointer predicate stays `stage::worktree_pointer`.
- `src-tauri/crates/keeper-core/src/vm.rs` — `FilesSyncStatusVm` `:3809-3830` (**gains** three unit variants; stays `Copy`). `FilesEntrySyncVm` `:3839-3866`, `plain` `:3851`, `explained` `:3859` (**unchanged**). `FilesEntryVm` `:4022-4116` — `size` `:4057`, `lfs_oid` `:4074`, `mtime_ms` `:4095` (**all unchanged; no new wire field**). `FilesEntryFacts` `:4126-4148`, `new` `:4184-4222`. `FilesDeletePlanVm::compose` `:4463-4583` — **the defect site**: `unclear` `:4469-4472`, `travels` `:4473-4483`, `local` `:4484`, the four consequence arms `:4492-4533`; doc `:4446-4462` incl. the `Unknown` rule `:4458-4462`. Tests: `note` `:7093`, `loose` `:7105`, `a_delete_confirmation_says_whether_the_files_sync` `:7154-7205` with the local-only loop `:7178-7181`, `an_unreadable_sync_state_is_counted_as_syncing_and_admitted` `:7215`.
- `src-tauri/crates/keeper/src/sync_ipc.rs` — **shell crate, macOS gate.** `browse_marks_for` `:1900-1956` (**unchanged**: it caches the pending walk, and the ledger read is a single SELECT that must not join that cache). `sync_browse` `:1958-1996` (**gains** the ledger fetch and one argument). `files_listing_vm` `:2015-2133` and its `FilesEntryFacts` literal `:2063-2074` (**unchanged** — the facts already flow). `sync_mark` `:2167-2228` — the 56.7 debt doc `:2148-2166` and the collapsed `Virtual` arm `:2197-2209` are **replaced by three arms**. `sessions_sync_mark` `:2863-2868` (**unchanged**, doc already de-numbered). `sync_delete_plan` `:3212-3253` — the `status_of` call `:3238-3239` and the `.status` push `:3240-3244` (**gains** the ledger fetch and one argument). `sync_ipc_error` `:766-830`, `EXPRESSED` `:3557-3584`, `PRESERVED` `:3612-3622`, `a_save_cannot_move_a_field_no_request_can_express` `:3700-3813` — **all untouched**.
- `src-tauri/crates/keeper/src/sessions_ipc.rs` — `sessions_tree` `:175-277`, the `pending` build `:214-217`, the `status_of` call `:241-247`. **One argument.** Shell crate.
- `src-tauri/crates/keeper/src/notes_ipc.rs` — the `browse_root` call `:1377`, already `PendingView::Unavailable` so every row is `Unknown` before the new view is consulted. **One argument.** Shell crate.
- `src/components/layout/sync-status-mark.tsx` — header doc `:1-23`; `FILES_SYNC_MARK_LABEL` `:40-46`, `MARK_ICON` `:49-55` (lucide import `:26`), `MARK_TONE` `:71-77` with the token rationale `:57-70`; the component `:80-95` (`role="img"` `:85`, `aria-label` `:86`, `data-sync-status` `:89`). **Three entries per map, three new glyphs, and the indeterminate branch.**
- `src/components/layout/files-pane.tsx` — the size cell `:1867-1888` (`FILES_SIZE_SLOT` `:340`, the "a directory renders nothing" rule `:1873-1876`) is the template for **the new mtime cell**; `sizeId` `:1667` and the `aria-describedby` list `:1676-1682`; the sync-mark slot `:1889-1892`; the "no `loading` set" rule `:726-728`; the single `actions` array `:1695-1741` (**no entry added**); `useWindowedRows` `:1402-1418`.
- `src/components/sessions/session-tree.tsx` — `nowMs = Date.now()` `:212`, the age cell `:327`/`:432-438` with `formatDraftAge`, folded into `aria-describedby` `:329-330`. The exact pattern the Files row copies, adjusted to `!= null` because `FilesEntryVm.mtimeMs` is `number | null` rather than the `0` sentinel.
- `src/lib/format-time.ts` — `formatDraftAge(ms, now = Date.now())` `:84` (relative under a day, an absolute date beyond it, `""` for non-finite/non-positive). **Not modified.**
- `src/components/settings/sync-section.tsx` — the indeterminate precedent: `:488-490` (`null` total) and `:560-577` (no `aria-valuenow`, `aria-valuetext` reuses Rust's line).
- `src/components/layout/files-pane.test.tsx` — the de-facto `SyncStatusMark` suite: sentence constants `:1241-1248`, `marked` `:1250-1256`, `openMixedFolder` `:1259-1275`, `markOf` `:1277-1280`, the `states` table `:1285-1291`, `sizeOf` `:1490-1493`, the size describe `:1522-1743`, the delete-confirmation describe from `:2067` with the sentence assertion `:2143-2172`.
- `src/lib/ipc/gen/FilesSyncStatusVm.ts` `:25` — the five-member union; regenerated, never hand-edited. `src/lib/ipc/gen/FilesEntrySyncVm.ts`, `FilesEntryVm.ts`, `FilesDeletePlanVm.ts` — unchanged shapes.
- `dev/mock-shell.ts` — `browseEntry` `:192-213` (typed with no cast; `lfsOid: null` is the statement that `size` came off a `stat`). **Gains** a virtual and a materialized row in `ENTRIES` `:215-227` so the harness can show the new marks.
- `src-tauri/crates/keeper-sync/tests/lfs_listing.rs` — `init_repo` `:44`, `profile` `:53`, `commit` `:59`, `porcelain` `:81`, `seed` `:96`, the browse assertion `:150-181`. The fixture the new integration test reuses.
- `src-tauri/crates/keeper-sync/tests/materialize_entry.rs` — `engine_for` `:150`, the `LfsFileState::Materialized` assertion `:246-249`. The real-engine path that writes the ledger row `Engine::materialized_paths` must hand back.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-sync/src/browse.rs` — add `MaterializedView` (a `HashSet<String>` newtype with `from_paths`, `none`, `holds`) beside `PendingView`, documenting that an empty view degrades to `Synced` — true but less specific — which is the opposite polarity to `PendingView`'s empty-is-a-lie hazard. Add `EntrySyncStatus::Materializing` and `EntrySyncStatus::Materialized` with docs stating what each claims about *here* and not about the remote. Thread `materialized: &MaterializedView` through `browse`, `browse_root`, `list_resolved`, `status_of`, `classify`; change `holds_pointer` to `FnMut` and update the rationale doc. Add rung 3a (an `Incoming` reason over pointer text is `Materializing`) and rung 5a (a ledger row over non-pointer bytes is `Materialized`), and rewrite the precedence doc as eight rungs. -- The state vocabulary and the one place precedence is decided.
- [x] `src-tauri/crates/keeper-sync/src/browse.rs` (tests) — extend every call site with the new argument; add `nothing_materialized()` beside `nothing_pending()`; add tests: a queued `Incoming` over pointer text is `Materializing` while an `Incoming` over a plain file stays `Waiting`; a ledger row over real bytes is `Materialized`; a ledger row over pointer text is still `Virtual`; a `Materialized` row reports the worktree's size and no `lfs_oid`; `status_of` and the listing agree on all three new states. -- Precedence and the two new rungs are where a wrong answer is invisible.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` — add `Engine::materialized_paths(&self, profile_id) -> Result<HashSet<String>>` over `db::materialized_paths`, sync like `list_profiles`, documenting that it is one statement and deliberately not folded into the cached pending walk. -- The ledger fact has no reader outside the engine today.
- [x] `src-tauri/crates/keeper-sync/tests/lfs_listing.rs` — extend the real-git fixture with an integration test that materializes a committed pointer through `Engine::materialize_entry`, reads `Engine::materialized_paths`, and asserts `browse::browse` marks that path `Materialized` and the still-virtual sibling `Virtual`. -- Proves the state is reachable from the engine rather than only constructible in a test.
- [x] `src-tauri/crates/keeper-core/src/vm.rs` — add `Virtual`, `Materializing`, `Materialized` to `FilesSyncStatusVm` with docs; add all three to `compose`'s `travels` `matches!` **explicitly** and extend the doc to state the rule for them, quoting the same "only one of them is safe to be wrong about" reasoning. -- FR-345/AD-134: the one site that compiles wrong instead of failing.
- [x] `src-tauri/crates/keeper-core/src/vm.rs` (tests) — extend `a_delete_confirmation_says_whether_the_files_sync` so each of the three new states appears in a **travelling** assertion (never in the `[Excluded, NotInRepository]` local-only loop), plus a mixed case pairing `Materialized` with `Excluded`. -- The pinning test AD-134 asks for.
- [x] `src/lib/ipc/gen/**` — regenerate by running the `keeper-core` ts-rs export test; commit the result unedited. -- Generated bindings are produced, never written.
- [x] `src-tauri/crates/keeper/src/sync_ipc.rs` — replace `sync_mark`'s collapsed `Virtual` arm and its 56.7 debt doc with three arms and three sentences, none of which claims the remote holds anything and none of which names a duration or a percentage; fetch `Engine::materialized_paths` in `sync_browse` and `sync_delete_plan` and pass the view. -- The only exhaustive `match` over `EntrySyncStatus`; shell crate, macOS gate.
- [x] `src-tauri/crates/keeper/src/sessions_ipc.rs`, `src-tauri/crates/keeper/src/notes_ipc.rs` — pass the new argument (`MaterializedView::none()`, with a sentence saying what that means for each caller). -- Shell crate, macOS gate.
- [x] `src/components/layout/sync-status-mark.tsx` — add the three entries to each of `FILES_SYNC_MARK_LABEL`, `MARK_ICON` and `MARK_TONE`, with three glyphs pairwise distinct from the existing five and from each other, and tones drawn only from already-verified tokens; render `materializing` as an indeterminate progress role with the Rust sentence as its value text and **no** `aria-valuenow`; update the "five different shapes" sentence to eight. -- Shape carries the distinction, and the compiler is the checklist.
- [x] `src/components/layout/files-pane.tsx` — add the modification-time cell beside the size cell, copying the size cell's `data-slot`/`shrink-0`/one-line shape and the session tree's `formatDraftAge(entry.mtimeMs, nowMs)` with `nowMs = Date.now()` at render; render nothing when `mtimeMs == null`; join its id into the row's `aria-describedby`. Add **no** entry to `actions`. -- FR-336/FR-340: the row shows its true size and its modification time.
- [x] `src/components/layout/files-pane.test.tsx` — extend the mixed-state fixture and the `states` table to eight; assert the three new marks each have a distinct `data-sync-status`, a distinct accessible name and a distinct rendered glyph, and that the eight glyphs are pairwise distinct with tone classes ignored; assert the materializing mark carries no `aria-valuenow` and an accessible name containing no digit; assert a virtual row shows the pointer's size and its mtime and nowhere shows a ~130-byte figure; assert a delete confirmation whose plan counts a virtual and a materialized path as travelling renders that sentence. -- The perceptible distinction is what the story turns on.
- [x] `dev/mock-shell.ts` — add a virtual row (pointer-sized `size`, `lfsOid` set) and a materialized row so the dev harness renders the new marks. -- A state nothing can show is a state nobody reviews.

**Acceptance Criteria:**
- Given a folder holding a committed pointer, a pointer with a queued download and a materialized object, when the Files pane lists it, then the three rows carry three different marks with three different accessible names, and the distinction survives with every colour class removed.
- Given the materializing state, when its mark renders, then it announces itself as in flight with no percentage, no `aria-valuenow` and no time remaining.
- Given a virtual row, when it renders, then the size cell shows the number the pointer names and the modification-time cell shows the worktree's mtime; no surface reports the ~130 bytes on disk.
- Given a delete over a virtual path and a delete over a materialized path, when the plan is composed, then each counts as travelling and the confirmation says the deletion removes it from every machine that syncs the profile.
- Given a path whose bytes are pointer text and whose ledger row exists, when it is classified, then it is `Virtual` and not `Materialized`.
- Given a queued LFS download over a path that is not pointer text, when it is classified, then it stays `Waiting`.
- Given the Files row, when it is rendered, then the `actions` array holds exactly the verbs it held before this story.
- Given `cargo clippy -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings`, when it runs, then it is clean; and the three crates' tests pass with no fewer than 3455 passing.
- Given `bun run typecheck`, `bun run lint` and `bun run test`, when they run, then typecheck is clean, lint is at baseline, and the suite is green with the new assertions.
- Given `git status --porcelain -- src/lib/ipc/gen`, when checked after regenerating, then the only diff is the one the ts-rs export test produced.

## Spec Change Log

## Review Triage Log

### 2026-08-25 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 22: (high 1, medium 9, low 12)
- defer: 3: (high 0, medium 2, low 1)
- reject: 2: (high 0, medium 1, low 1)
- addressed_findings:
  - `[high]` `[patch]` `FILES_ROW_META_PX` was never charged for the new modification-time cell, so `filesRowActionsBudget` over-reported free space by 68px on every row with an mtime and the deficit came out of the only flexible child — the name group — re-opening the sub-floor squeeze `FILES_NAME_FLOOR_PX` exists to prevent. Raising the reserve to 132 was rejected: it costs a verb at 320px and breaks a previous story's pinned two-verb guarantee. Instead the modification time became the row's lowest-priority cell — `FILES_ROW_MTIME_PX` + `filesRowShowsModified`, charged to `planPriorityActions` only where the row can pay out of slack no verb wants, and rendered `sr-only` below that width so the fact is unpainted and never lost. `filesRowActionsBudget` and its four pinned figures are untouched. Mutation-proved: `FILES_ROW_MAX_ACTIONS = 0` fails the new threshold test *and* the previous story's 320px two-verb test.
  - `[medium]` `[patch]` `files-pane.test.tsx`'s shared `entry()` fixture returned its literal `as FilesEntryVm` and omitted `mtimeMs`, so every row-geometry test rendered a row the story's own cell could not reach — which is why the finding above was invisible to a green suite. The cast is gone and the literal is checked.
  - `[medium]` `[patch]` A directory carrying its own `materialized` ledger row was marked `Materialized`; the ledger rung now requires `!is_dir`, and `MaterializedView`'s doc names the rung as what enforces the property it had merely asserted. The existing test only covered a directory whose *descendant* had a row, so its name over-claimed; it now gives the directory itself one.
  - `[medium]` `[patch]` `Materialized` was earned by a negative: `holds_pointer() == false` also means "there are no readable bytes", so a path whose file vanished between the `read_dir` and the `stat` read `Materialized` with no size beside the sentence "This file's content is on this computer." The probe is now `impl FnMut() -> Option<bool>` (`None` = no readable regular file), the settled rungs share one call, and `Materialized` requires `Some(false)` — the positive fact its own doc claims.
  - `[medium]` `[patch]` The materializing sentence claimed an activity. `db::queued_downloads` takes every `lfsDownload` row whose state is not `parked`, including `deferred` — canonically a download whose removable remote was absent, waiting indefinitely for a volume re-attach — so a present-progressive claim is false for those rows. Reworded to a queued state, which is true of queued, running and deferred alike and leaves the indeterminate role exactly right; mirrored in the test constant and the dev harness.
  - `[medium]` `[patch]` `Materializing` had no real-engine proof, on the same argument the story used to demand one for `Materialized`. `tests/lfs_listing.rs` now drives `Engine::materialize_entry` against an emptied store so the download queues, asserts `Engine::pending` emits `Incoming` with the profile-relative `/`-joined label (not the `LFS object <oid…>` fallback), feeds that answer through `PendingView::from_pending` into `browse::browse`, and asserts the row is `Materializing` with the pointer's size and oid.
  - `[medium]` `[patch]` Both shell ledger reads swallowed their error with no log, and by `MaterializedView`'s own polarity argument the degradation is invisible on screen by design — so there was no evidence anywhere. Both now `tracing::warn!` with the profile and the error before degrading.
  - `[medium]` `[patch]` `BrowseEntry::lfs_oid`'s doc still claimed `None` for every mark that is not `Virtual`, which a `Materializing` row falsifies.
  - `[medium]` `[patch]` `BrowseEntry::size_bytes`' doc scoped the pointer substitution to `Virtual` alone and named as "the deliberate consequence" that a `Waiting { Incoming }` row shows the pointer text's length — inverted by the new rung. Both paragraphs restated.
  - `[medium]` `[patch]` `stage::worktree_pointer`'s doc claimed `classify` asks it "last, of a path whose mark would otherwise be `Synced` and of no other". The story rewrote that sentence in `browse.rs` and left it standing here.
  - `[low]` `[patch]` `aria-valuetext` duplicated `aria-label` verbatim, so a progressbar-aware screen reader announced the sentence twice; the cited precedent pairs a short distinct name with an informative value text and the doc claimed the pairing was copied. Dropped, doc corrected to what the precedent actually establishes.
  - `[low]` `[patch]` Two freshly written counts were already wrong four lines below the paragraph condemning stale counts: `MARK_TONE`'s doc said two settled states share the recessive tone where four do, and the new test comment said three. Both restated as properties.
  - `[low]` `[patch]` A future-dated mtime rendered "just now" — `formatDraftAge`'s deliberate skew clamp meeting a faithfully-carried SMB/NTFS or forward-stamped date — which is the one output the cell's own rule forbids. A named grace window folded into the single `modified` binding, with the skew case kept and the date case refused.
  - `[low]` `[patch]` The mtime tooltip was a second date spelling (`new Date(...).toLocaleString()`) in a codebase that converts dates in one place, and the session tree's identical cell has none. Dropped; `formatDraftAge` is already absolute beyond a day.
  - `[low]` `[patch]` The dev harness could not show `materializing` — the only state that takes the new progressbar role, the new glyph and the non-recessive tone. Third row added.
  - `[low]` `[patch]` `MaterializedView` derived `Default`, the exact spelling `none()`'s doc argues against; dropped, and the omission is now named.
  - `[low]` `[patch]` `std::collections::HashSet` was spelled in full at every new site in a module that already imports `BTreeMap`, so the new type's API read differently from `PendingView`'s beside it.
  - `[low]` `[patch]` `sync_browse`'s comment presented its ledger read as the only one on the path; `Engine::pending` reads the same table for `replacing` and that copy sits behind the marks cache, so the two can disagree about one row inside a listing. Stated, with why they are deliberately not one read.
  - `[low]` `[patch]` The whole-profile ledger read was paid even when `PendingView::Unavailable` guarantees every row answers `Unknown` two rungs earlier; skipped in that case.
  - `[low]` `[patch]` The sessions tree can render `materializing` and never `materialized`, because it passes `MaterializedView::none()`. The spec authorises that, so the asymmetry is now recorded at the call site with the one-line change a later story would make, rather than left to be rediscovered.
  - `[low]` `[patch]` `MaterializedView::none()` was constructed once per sessions-tree row instead of once per tree; hoisted.
  - `[low]` `[patch]` `dev/mock-shell.ts`'s `lfsEntry` re-spread `size` over a value `browseEntry` had already set, reading as though it were correcting something.

## Design Notes

**Why `Materialized` needs a fact from outside `browse`.** `browse` opens no repository — deliberately, because opening is where trust levels, config enforcement and index refreshes live (`browse.rs:816-819`). Without an index it cannot tell a materialized LFS path from an ordinary file: both are regular files whose bytes are not pointer text. The `materialized` ledger is the fact that closes the gap and it is also the *right* fact — a row there is precisely "keeper put content at this path and can release it", which is what `db::forget_materialized` retracts and what 56.5's sweep consults, and what 56.9's Release verb will act on. It arrives as a parameter for the reason `pending` does (`browse.rs:646-650`): a caller that knows nothing must say so rather than be handed a default that reads as an answer.

**The polarity difference, stated so it is chosen.** An empty `PendingView::Known` marks every entry `Synced` and is a lie. An empty `MaterializedView` marks a materialized path `Synced` — *true*, merely less specific, and still travelling in the delete plan. So `MaterializedView` needs no `Unavailable` variant: engine failure is already `PendingView::Unavailable`, which returns `Unknown` two rungs earlier.

**Golden example — the two new rungs.**

```rust
if let Some(reason) = pending.waiting(relative_path, is_dir) {
    // A queued LFS download whose worktree bytes are still the pointer is not
    // "waiting to sync": it is this content arriving. `Incoming`'s own doc says
    // a queued download always finds pointer text, so the probe confirms the
    // path is the one being replaced rather than a label for a deleted one.
    if in_repository
        && matches!(reason, Some(PendingReason::Incoming { .. }))
        && holds_pointer()
    {
        return EntrySyncStatus::Materializing;
    }
    return EntrySyncStatus::Waiting { reason };
}
if !in_repository {
    return EntrySyncStatus::NotInRepository;
}
match worktree_bytes() {
    Some(true) => EntrySyncStatus::Virtual,
    // Earned by bytes that were READ: `None` also means "no readable regular
    // file here", and a vanished path must not claim to hold content.
    Some(false) if !is_dir && materialized.holds(relative_path) => {
        EntrySyncStatus::Materialized
    }
    _ => EntrySyncStatus::Synced,
}
```

The probe answers `Option<bool>`, not `bool`: `None` is "there is no readable regular file here", `Some(true)` is "those bytes are the committed pointer", `Some(false)` is the positive fact `Materialized`'s own doc claims. It is `FnMut` because two rungs may ask it; only one branch can reach the question, so the syscall is still paid at most once — the cost argument the `FnOnce` doc makes is preserved, and the type no longer forbids the second reader. **The review pass changed both of these**: the first draft asked a two-valued `holds_pointer()` and let a vanished path with a ledger row read `Materialized` with no size, and it had no `!is_dir` guard, so a directory carrying its own stale ledger row read `Materialized` too.

**Why the mark, and not a progress bar, carries "materializing".** The pane owns no tick and gains none here (56.9 owns the one interval). An indeterminate progress role on the existing mark is the whole of the in-flight rendering: it says "in flight, total unknown" in the vocabulary a screen reader already has, exactly as `sync-section.tsx` does for a sync with no known total, and it invents nothing.

## Verification

**Commands:**
- `cargo fmt --manifest-path src-tauri/Cargo.toml --all` -- expected: no diff afterwards.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings` -- expected: clean.
- `GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=keeper GIT_AUTHOR_EMAIL=dev@keeper.local GIT_COMMITTER_NAME=keeper GIT_COMMITTER_EMAIL=dev@keeper.local cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync -p keeper-core -p keeper-syncd` -- expected: 0 failed, no fewer than 3455 passing.
- `bun run typecheck` -- expected: clean.
- `bun run lint` -- expected: baseline (4 warnings + 1 info, the same five files).
- `bun run test` -- expected: green, with the new files-pane assertions.
- `git status --porcelain -- src/lib/ipc/gen` -- expected: only `FilesSyncStatusVm.ts`, produced by the ts-rs export test.

**Manual checks (if no CLI):**
- `src-tauri/crates/keeper/**` cannot be compiled or tested on this host. Every touched shell symbol is reported for `bun run check:rust:macos`: `sync_ipc::sync_mark`, `sync_ipc::sync_browse`, `sync_ipc::sync_delete_plan`, `sessions_ipc::sessions_tree`, `notes_ipc`'s `browse_root` call site.
- Mutation proof, reported as a table: (a) drop the three new variants from `compose`'s `travels` and confirm the pinning test fails, restore, confirm it passes — verifying the restore by reading `git diff`, not from memory; (b) break one `Record` entry in `sync-status-mark.tsx` and confirm the owning files-pane test fails, restore the same way.

## Auto Run Result

Status: done

**What was implemented.** `browse::EntrySyncStatus` gained `Materializing` and `Materialized` beside 56.2's `Virtual`; the wire enum `FilesSyncStatusVm` gained all three; `sync_ipc::sync_mark`'s one collapsed arm became three arms with three Rust-composed sentences, none claiming the remote holds anything and none naming a duration or a percentage. `browse::MaterializedView` carries the `materialized` ledger into `classify` the way `PendingView` carries the pending list, with the opposite polarity stated: an empty view degrades a materialized path to `Synced`, which is true and merely less specific, where an empty `PendingView::Known` would be a lie. `classify` gained two rungs — a queued LFS download over pointer text is content *arriving*; a ledger row over bytes that were read and are not a pointer is content *held* — and its probe became three-valued so `Materialized` is earned by a positive fact. `FilesDeletePlanVm::compose`'s `travels` filter names all three new variants explicitly (FR-345, AD-134): the filter is a `matches!` and therefore non-exhaustive, so each had to be visited by hand or the confirmation would have promised a local deletion while removing content only the remote holds. The Files row gained a modification-time cell, charged to the row's width budget only where it costs no verb the row could already promote and rendered `sr-only` below that width so the fact is unpainted and never lost.

**Files changed.**
- `src-tauri/crates/keeper-sync/src/browse.rs` — `MaterializedView`; two `EntrySyncStatus` variants; the three-valued probe; two new precedence rungs; five widened signatures; seven new/extended tests.
- `src-tauri/crates/keeper-sync/src/engine.rs` — `Engine::materialized_paths`, one statement over `db::materialized_paths`.
- `src-tauri/crates/keeper-sync/src/lfs/stage.rs` — `worktree_pointer`'s caller-ordering doc, corrected. No code.
- `src-tauri/crates/keeper-sync/tests/lfs_listing.rs` — two real-git, real-engine integration tests: one for `Materialized` through `Engine::materialize_entry`, one for `Materializing` through `Engine::pending`'s own inbound half.
- `src-tauri/crates/keeper-core/src/vm.rs` — three `FilesSyncStatusVm` variants; `travels` names each; the AD-134 pinning test.
- `src-tauri/crates/keeper/src/sync_ipc.rs` — `sync_mark`'s three arms and its rewritten doc; the ledger read in `sync_browse` and `sync_delete_plan`. **Shell crate: no compiler ran on this host.**
- `src-tauri/crates/keeper/src/sessions_ipc.rs`, `src-tauri/crates/keeper/src/notes_ipc.rs` — one argument each, with the reason each knows nothing about the ledger. **Shell crate.**
- `src/lib/ipc/gen/FilesSyncStatusVm.ts` — regenerated by the ts-rs export test. Never hand-edited.
- `src/components/layout/sync-status-mark.tsx` — three entries in each of the three exhaustive `Record` maps; three glyphs; the indeterminate progress role.
- `src/components/layout/files-pane.tsx` — the modification-time cell, its width policy and its honesty guards.
- `src/components/layout/files-pane.test.tsx` — eight-state mark suite, the checked `entry()` fixture, and the width/absence/confirmation tests.
- `dev/mock-shell.ts` — a virtual, a materializing and a materialized row, so all three marks can be looked at.

**Review findings.** intent_gap 0, bad_spec 0, patch 22 (1 high, 9 medium, 12 low) all applied, defer 3, reject 2. See the Review Triage Log.

**Verification.** `cargo fmt --check` clean. `cargo clippy -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -D warnings` clean. Rust **3483 passed / 0 failed** (baseline 3455 required; the pre-story figure by the same summation was 3475). `bun run typecheck` clean. `bun run lint` at baseline — 4 warnings + 1 info, the same five files. `bun run test` **297 files / 4877 tests passed**, no flake in this run including the load-flaky `waits for a real list before deciding a profile is gone`. `git status --porcelain -- src/lib/ipc/gen` shows only `FilesSyncStatusVm.ts`.

**Mutation proofs.** (a) Each of the three new variants dropped from `compose`'s `travels` in turn → `a_virtual_or_materialized_deletion_is_told_to_travel` FAILS ("None of these 2" instead of "1 of these 2"); restored, verified by reading `git diff`, passes. (b) One `MARK_ICON` entry collapsed onto another's glyph → the eight-shape test FAILS naming the glyph axis (7 ≠ 8); restored the same way. (c) The indeterminate branch removed → the indeterminate test FAILS on `role`. (d) The ledger rung reverted to its pre-patch shape → both the directory test and the no-readable-bytes test FAIL. (e) `FILES_ROW_MAX_ACTIONS` set to 0 → the new width-threshold test FAILS *and* so does the previous story's pinned 320px two-verb test, which is the regression the width policy exists to prevent.

**Residual risks.** The `keeper` shell crate cannot be compiled or tested on this host. Symbols needing `bun run check:rust:macos`: `sync_ipc::sync_mark`, `sync_ipc::sync_browse`, `sync_ipc::sync_delete_plan`, `sessions_ipc::sessions_tree`, and the `browse_root` call site in `notes_ipc`. They were checked three other ways — rustfmt parses all three files, every changed region was re-read with fresh line numbers, and a workspace-wide grep confirms no call site of `browse::browse`, `browse::browse_root` or `browse::status_of` was left at the old arity. Three findings were deferred with evidence in `deferred-work.md`: the sync mark is not in the row's `aria-describedby` (pre-existing, and identical in the sessions tree); a queued download over a path whose worktree holds the real bytes still reads `Waiting` (pre-existing classification and sentence); and `Engine::materialized_paths` reads a profile's whole ledger table unfiltered by the listed cone.
