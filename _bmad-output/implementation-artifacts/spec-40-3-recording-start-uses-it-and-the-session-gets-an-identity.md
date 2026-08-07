---
title: 'Story 40.3: `recording_start` Uses It, and the Session Gets an Identity'
type: 'feature'
created: '2026-08-06'
status: 'in-progress'
blocking_condition: ''
baseline_revision: '272b00493353f421e992983a7c361231cba16101'
final_revision: ''
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-40-context.md'
---

<intent-contract>

## Intent

**Problem:** The template is a setting with a live preview (40.2) and `recording_start` ignores it.
Measured in the field on hesperia, 2026-08-06, with `recording.path_template` saved as
`{yyyy}/{yyyy}-{mm}-{dd} {HH}.{MM} {slug}` and a session titled "Test": the preview promised
`~/Movies/keeper/2026/2026-08-06 15.36 test` and the recording landed in
`~/Movies/keeper/Test 2026-08-06 15.36.22`. An untitled session went to
`keeper-rec 2026-08-06 15.37.25`. The shell still formats its own name at `ipc.rs:4509-4531` from
`chrono::Local::now()` plus `sanitize_session_title`, and appends ` (2)` on collision. A preview that
disagrees with the recorder is worse than no preview: it is a promise the app breaks, silently, in the
one place the user goes looking for their file.

The session also has no identity. Its only handle is the folder name, which story 40.4 is about to
make editable and story 42 is about to key an archive on.

**Approach:** `recording_start` renders the effective template instead of formatting a name, and the
collision retry becomes the template's own `{seq}` ordinal (`for seq in 1..`), so the ordinal lands
where the template put it rather than always at the end. The rendered relative path may nest, so its
intermediate components are created on demand while the leaf keeps its `create_dir` uniqueness guard —
the same-second-restart guard survives, and "already exists" becomes a typed, retryable signal rather
than an opaque IO error. The session gains `session_id` — the device's ULID joined to a freshly minted
one — carried in `SessionMeta` and therefore in `manifest.json`, so a retitle can move the folder
without moving the identity (40.4) and an archive row can be keyed on it (42).

Nesting is not free: two recovery walks and one acknowledgement set are keyed on the destination
root's immediate children. The default template nests, so they are repaired in this story rather than
left to blind recovery the moment the feature ships.

## Boundaries & Constraints

**Always:**
- One renderer, one clock. The shell reads `chrono::Local::now()` ONCE per start and builds the same
  `RenderCtx` the preview builds (`preview_render_ctx`), so preview and recorder cannot disagree.
  `keeper-core` stays clock-free: the id and the timestamp arrive as parameters.
- The effective template is whatever `effective_path_template` resolves — absent, blank and
  unparseable all degrade to `DEFAULT_TEMPLATE` on READ, so a start never fails on a stored template.
- The leaf is created with `create_dir`, never `create_dir_all`: an existing session folder is never
  adopted. Only the INTERMEDIATE rendered components are created on demand.
- Every registry read moves ABOVE folder creation. Nothing between the created folder and the driver
  spawn may fail, and what can fail (the manifest write) unwinds what this attempt created.
- Rollback removes only what this attempt created, deepest-first, and only while empty (`remove_dir`,
  never `remove_dir_all`). A pre-existing `2026/` that already holds other sessions is never touched.
- The session id is one scalar, decomposable, and safe in a markdown link and in a shell argument:
  `<device ULID>-<session ULID>`, both Crockford (uppercase alphanumeric, `-`-free), so a split on the
  single `-` recovers the device id that epic 42 stores in its own column.
- The device identity is the one the device already has — `keeper-sync`'s `sync.db` ULID, the same id
  `Keeper-Device` publishes. A second device identity would be a second answer to "which machine made
  this".
- Reaching it must not require git: `keeper_sync::db::{open, device_identity}` directly, never
  `crate::sync::engine()`, whose construction legitimately fails without git.
- The manifest keeps holding no absolute path: `SegmentEntry.file` stays a basename (the Swift concat
  gate resolves it against the folder), `folder` stays `#[serde(skip)]`, and `session` stays the leaf
  basename — a label, not an identity.
- `MANIFEST_VERSION` stays 1. `session_id` is additive and optional on the wire, so a build that
  predates it still loads and still recovers what this build wrote.

**Block If:**
- The device identity cannot be read without constructing the sync engine. It can: `db::open` is a
  `create_dir_all` plus `Connection::open`, which is what `Engine::open` itself calls.

**Never:**
- No new IPC command, no new `IpcErrorCode`, no VM field, no regenerated bindings.
- `recording_start`'s sidecar contract is untouched: `SessionParams.output_path` stays the absolute
  file path the sidecar writes, and `RecordingStatusVm.output_path` stays the absolute session folder.
- No migration of existing session folders, and no retitle — that is 40.4.
- No `remove_dir_all` anywhere on a destination the user chose.

## I/O & Edge-Case Matrix

Now = 2026-08-06T15:36:22 local; destination root `/Users/alice/Movies/keeper`; device ULID
`01KYDKP6SN2HR4SJBJ9JTBVC2Z`.

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Default template, untitled | no stored template, no title | folder `<root>/2026/2026-08-06 1536`; the `2026` directory is created on demand | none |
| Default template, titled | title `Test` | folder `<root>/2026/2026-08-06 1536 test` — the preview's path, byte for byte | none |
| The field report | stored `{yyyy}/{yyyy}-{mm}-{dd} {HH}.{MM} {slug}`, title `Test` | `<root>/2026/2026-08-06 15.36 test` | none |
| Collision inside the minute | same template + title, second start | second folder carries the template's `{seq}` ordinal — the default template omits `{seq}`, so 40.1 appends it to the leaf as ` (2)`; both sessions exist; ids differ | none |
| Collision exhausted | 64 consecutive rendered paths all exist | `recording_start` fails naming the last rendered RELATIVE path; nothing created | shell `IpcError`, retriable |
| Read-only destination root | root exists, not writable | fails BEFORE the sidecar launches, naming the rendered relative path; no folder, no intermediate directory left behind | shell `IpcError`, retriable |
| Nested parent already holds sessions | `<root>/2026/` holds two sessions, this start fails after creating nothing | `2026/` is left exactly as it was | none |
| Manifest write fails after the leaf exists | leaf created, `manifest.json` write fails | the leaf (and any directory this attempt created) is removed; the error names the operation | `RecordingError::ManifestIo` |
| Registry unreadable | `keeper.db` read fails | fails before anything is created | funnelled `IpcError` |
| Session identity | any start | `manifest.json` carries `meta.sessionId` = `<device ULID>-<new ULID>`; two starts in the same minute carry different ids | none |
| Identity with no user metadata | untitled, no participants/note/tags | the `meta` block is still written, carrying `sessionId` alone | none |
| Device id absent | no `sync.db` yet | it is minted (one ULID, stable thereafter) and reused by sync when sync first runs | none |
| Manifest portability | any finished session | the serialized manifest contains the destination root nowhere; a `grep` for the root string finds nothing | none |
| Recovery of a nested session | app killed mid-session under `<root>/2026/…` | the startup pass marks it `recovered` and the Recording view surfaces its card | none |
| Acknowledging a nested session | two sessions, `<root>/2026/x` and `<root>/2027/x` | acknowledging one leaves the other surfaced — the seen-set is keyed on the root-relative path | none |
| Acknowledged before this story | a flat `keeper-rec …` basename already in the seen-set | still suppressed: a flat session's root-relative path IS its basename | none |
| A stray directory | `<root>/Screenshots/` with no manifest anywhere below | walked and ignored, no card, no error | none |
| Depth guard | a hand-made 20-deep tree under the root | the walk stops at 8 components and reports nothing below it | none |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper/src/ipc.rs` — `recording_start`: the naming block (4509-4531) becomes a
  render + `{seq}` retry; the six registry reads (4602-4623) move above creation; `sanitize_session_title`
  (4352) is deleted. New shell helpers: `start_render_ctx` (the seq-varying twin of
  `preview_render_ctx`), `session_folder_path` (root + `RelativePath` → `PathBuf`, the join
  `compose_path_preview` already does), `SessionScaffold` (what this attempt created, and its
  deepest-first unwind), `mint_session_id`, `device_ulid`.
- `src-tauri/crates/keeper/src/ipc.rs` — `scan_recovered_sessions` (5315) and
  `recovered_session_acknowledge` (5387): nested walk, seen-set keyed on the root-relative path.
- `src-tauri/crates/keeper-core/src/recording.rs` — `RecordingError::SessionFolderExists` for the
  `AlreadyExists` create; `SessionMeta.session_id`; `recover_orphaned_sessions` walks nested;
  `session_folder_name` (1721) and its test are deleted.
- `src-tauri/crates/keeper-core/src/error.rs` — the new variant's doc.
- `src-tauri/crates/keeper/Cargo.toml` — `ulid = { workspace = true }` (already in the workspace
  catalog; no new crate is resolved).
- `docs/recording.md` — the session-tree diagram and the naming paragraph.
- Read-only: `src-tauri/crates/keeper-core/src/recording/path_template.rs`,
  `tools/keeper-rec/Tests/keeper-recTests/{ConcatAssert,FixtureSegments}.swift` (both resolve
  `segments[].file` against the folder — the reason segment paths stay basenames).

## Tasks & Acceptance

**Execution:**
- [ ] `keeper-core/src/recording.rs` — `RecordingError::SessionFolderExists`, returned when the leaf
      `create_dir` fails with `AlreadyExists`, so the shell can tell "that ordinal is taken" from "the
      filesystem said no" without parsing a message.
- [ ] `keeper-core/src/recording.rs` — `SessionMeta.session_id`, optional and additive on the wire.
- [ ] `keeper-core/src/recording.rs` — `recover_orphaned_sessions` walks nested session folders,
      depth-capped, never descending into a folder that is itself a session.
- [ ] `keeper-core/src/recording.rs` — delete `session_folder_name` and its test; the template is the
      namer now.
- [ ] `keeper/src/ipc.rs` — render the effective template, retry on `{seq}`, create intermediates on
      demand, keep `create_dir` on the leaf.
- [ ] `keeper/src/ipc.rs` — move the six registry reads above creation; unwind what an attempt created.
- [ ] `keeper/src/ipc.rs` — mint `session_id` from the sync device ULID plus a fresh ULID; always
      write the `meta` block.
- [ ] `keeper/src/ipc.rs` — delete `sanitize_session_title`.
- [ ] `keeper/src/ipc.rs` — nested recovery scan; acknowledgement keyed on the root-relative path.
- [ ] `keeper/Cargo.toml` — `ulid` from the workspace catalog.
- [ ] `docs/recording.md` — the tree and the naming rule.
- [ ] Rust tests: default-template creation with the year created on demand; the field report's
      template reproduced exactly; two starts in one minute yielding two folders and two ids;
      exhaustion naming the rendered path; a read-only root leaving nothing behind; a manifest-write
      failure unwinding the leaf; the serialized manifest containing the root nowhere; nested recovery;
      the seen-set keyed on the relative path and still honouring a flat basename.

**Acceptance Criteria:**
- Given the default template and no title, when `recording_start` runs, then the folder is
  `<root>/2026/2026-08-06 1536` and the year directory exists.
- Given two starts inside the same minute with the same title, when both complete, then there are two
  folders — the second carrying the template's `{seq}` ordinal — and two distinct `sessionId`s.
- Given a session folder, when its `manifest.json` is read as text, then it contains the destination
  root nowhere.
- Given a read-only destination root, when `recording_start` runs, then it fails before the sidecar is
  launched with an error naming the rendered relative path, and no directory it would have created
  exists.
- Given a session recorded under a nesting template and a kill, when the app restarts, then the
  session is recovered and surfaces as a card; acknowledging it does not suppress a same-leaf session
  under a different parent.
- Given `cargo test --workspace` on macOS, when it runs, then it is green, `cargo clippy --workspace
  --all-targets -- -D warnings` is clean, and `git status --porcelain -- src/lib/ipc/gen` is empty
  (no binding changed).

## Design Notes

**The bug was a broken promise, not a missing feature.** 40.2 shipped a preview whose path the
recorder ignored: with `{yyyy}/{yyyy}-{mm}-{dd} {HH}.{MM} {slug}` saved and a session titled "Test",
the card promised `~/Movies/keeper/2026/2026-08-06 15.36 test` and the folder created was
`~/Movies/keeper/Test 2026-08-06 15.36.22`. So the test that pins the fix asserts the two SIDES
against each other — `compose_path_preview` versus `create_session_folder` — rather than either
against a literal. Nothing can drift them apart again without that test failing.

**The retry is the template's ordinal, not a suffix.** The old loop appended ` (2)` to the whole
name; `{seq}` lands where the template put it, and 40.1 appends it to the leaf only when the template
omits it. So the loop varies `RenderCtx.seq` and re-renders, and the leaf's `create_dir` — never a
prior `exists()` — decides, which is what makes two starts racing inside one minute impossible to
resolve wrongly. 64 attempts is a bound on attempts, not on a number: at 64 identical renders the
template is the problem, and the refusal names the path it tried so the user can see that.

**Everything fallible moved above the first `mkdir`.** The six registry reads used to sit BELOW the
folder creation, so a registry hiccup returned an error and left a `recording` manifest on disk for
the recovery pass to surface as an interrupted session that never started. Now nothing between the
created folder and the driver spawn can fail except the manifest write, and that unwinds.

**The unwind is deliberately narrow.** `SessionScaffold` records only directories `create_dir`
actually created — `AlreadyExists` is not "mine" — and removes them deepest-first with `remove_dir`,
never `remove_dir_all`, on a folder the user chose. A pre-existing `2026/` that already holds other
sessions is not a candidate, and one that is not empty is refused by the syscall rather than by a
check that could race. The review found the one hole: `SessionManifest::write` cleaned up its
`.manifest.json.tmp` on the rename branch but not on the write branch, so an ENOSPC mid-write left
residue that made `remove_dir` a no-op and stranded both the leaf and the year folder. Fixed in core,
where the temp file's name is known.

**The identity is the device's ULID plus the session's.** Device-scoped by AD-73, so two machines
recording into one synced folder in the same minute cannot collide however identical their rendered
paths are; one scalar because story 42 makes it a primary key; split on the single `-` to recover the
device half, which Crockford's alphabet (no `-`) makes unambiguous; safe in a markdown link and a
shell word because 42 offers it as copyable text and as a `session:` target. The device half is
`sync.db`'s existing device row — the id `Keeper-Device` already publishes — read through
`keeper_sync::db` directly rather than through `crate::sync::engine`, whose construction legitimately
fails without git. Cached in a `OnceLock`: uncached, every Record click opened and migrated a database
the sync engine also writes (with no busy timeout on that connection) and forked `hostname` for a
label `device_identity` only ever uses on a device's very first call.

**The `meta` block is now always written.** It used to be omitted when the user typed no metadata,
which would have meant a session with no identity at all — the subtlest trap in the story.

**Nesting broke recovery, and this story owns that.** Both the salvage pass
(`recover_orphaned_sessions`) and the card scan (`scan_recovered_sessions`) walked only the
destination root's immediate children, so under the default template every session was invisible to
both: a crash would have left `status: recording` on disk forever with no card. Both now walk
descendants, bounded by two constants EXPORTED from `keeper-core` (`RECOVERY_MAX_DEPTH`,
`RECOVERY_MAX_VISITS`) rather than duplicated — the pass that marks a session `recovered` and the scan
that surfaces it must never disagree about what is reachable. Depth alone was not a bound: the root is
whatever folder the user picked, and the pre-record pass runs on the Record click, so a visit budget
is what keeps a Photos-library-sized root from hanging the button; tripping it logs once and returns
what it found.

**`PathTemplate::parse` now refuses a template deeper than the walks reach.** Nothing coupled the two
before: a legal `{yyyy}/{mm}/{dd}/{HH}/a/b/c/d/{slug}` recorded into a folder no recovery pass could
ever visit, which is a silently unsalvageable recording. The cap is the recovery cap, imported.

**A directory is a session only when its manifest LOADS.** Both walks used to treat any directory
holding a `manifest.json` as a leaf before reading it, so one stray or corrupt manifest in an
intermediate directory would hide every session beneath it from salvage. Now the file only nominates a
candidate; the load decides, and a failure is logged and descended past.

**The acknowledgement is keyed on the identity when there is one.** Keying dismissals on the folder
path in the very story that mints an immutable id — because the path is about to move (40.4) — would
have orphaned every dismissal on the first retitle. The scan and the latch both prefer
`meta.sessionId` and fall back to the root-relative path, which keeps every entry written before this
story working: a pre-40.3 session is flat, and a flat session's root-relative path IS its basename.

**`manifest.session` stays the leaf basename.** It is a label, not a handle — that is what
`sessionId` is for — and `docs/recording.md` now says so rather than promising a folder name that
40.4 will change.

## Verification

**Linux (this workstation):** `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D
warnings`, `cargo check --workspace --all-targets` and `cargo test -p keeper-core` (1084 unit tests
plus every integration binary) all clean. The `keeper` shell crate cannot LINK here (no GTK/webkit),
so its tests were type-checked only.

**macOS (`hesperia`, 26.5.2, arm64) — `bun run check:rust:macos`, green.** Sidecar build,
`cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` including the shell
crate, and `cargo test --workspace`. `cargo test -p keeper --lib` there reports **261 passed**,
including every test this story added:
`the_default_template_nests_and_creates_the_year_on_demand`,
`a_start_lands_exactly_where_the_card_previewed`, `two_starts_in_one_minute_get_two_folders`,
`an_exhausted_ordinal_refuses_and_names_the_path_it_tried`,
`a_read_only_root_refuses_and_leaves_nothing_behind`,
`a_failed_creation_unwinds_the_directories_it_made`, `a_pre_existing_parent_is_never_unwound`,
`the_manifest_carries_the_session_id_and_never_the_destination_root`,
`the_session_id_is_device_scoped_and_never_repeats`, `scan_lists_a_nested_recovered_session`,
`scan_acknowledgement_is_keyed_on_the_root_relative_path`,
`scan_honours_a_pre_nesting_basename_acknowledgement`,
`scan_ignores_dot_dirs_and_manifest_less_strays`, `scan_stops_at_the_depth_cap`,
`scan_stops_at_its_visit_budget`, `scan_descends_past_an_unloadable_intermediate_manifest`,
`acknowledge_latches_the_session_id_and_survives_a_moved_folder`,
`acknowledge_falls_back_to_the_relative_path_without_a_session_id` and
`acknowledge_survives_an_unreadable_destination_setting`. The gate's drift check rsynced the
macOS-generated `src/lib/ipc/gen/` back and found the committed tree identical — no binding moved,
as the epic requires.

**Adversarial review.** Two independent passes over the uncommitted diff (start path, nesting
fallout). Findings acted on: the live-folder reservation was not refcounted, so a colliding retry
un-reserved a folder a live session still held `[high]`; the temp-file residue that defeated the
unwind `[high]`; an uncached `sync.db` open plus a `hostname` fork on every Record click `[medium]`;
an unbounded template depth against a hard recovery cap `[medium]`; two private copies of that cap
`[medium]`; a walk bounded in depth but not breadth on the Record-click path `[medium]`; the
acknowledgement keyed on a path this story makes mutable `[medium]`; a stray manifest able to hide
every session beneath it `[low]`; `recovered_session_acknowledge` gaining a failure mode its own doc
denied `[low]`; three stale `basename` docs in the registry `[low]`; the `desktop` cfg spelled two
ways `[low]`; a vacuous collision assertion, an untestable pre-existing-parent case, a permission
restore after its own assertion and eight leaked temp trees in the new tests `[low]`.

## Change Log

- 2026-08-06 — Story implemented: the template names the session, the ordinal is `{seq}`, the session
  gains `meta.sessionId`, and both recovery walks follow sessions into the folders the template nests
  them in.
- 2026-08-06 — Addressed thirteen review findings (two high, five medium, six low) and hardened the
  nine new start tests plus the recovery-scan set.
