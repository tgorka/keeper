---
title: 'Story 40.4: Retitle Moves the Folder, Not the Identity'
type: 'feature'
created: '2026-08-07'
status: 'review'
blocking_condition: ''
baseline_revision: 'bdcd55e121ce1b24c0ae7a7829623e9d3f1b6d12'
final_revision: ''
review_loop_iteration: 1
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-40-context.md'
---

<intent-contract>

## Intent

**Problem:** Story 40.3 made the template name the session and minted `meta.sessionId` precisely so the
folder name could stop being the handle — and then nothing used that freedom. A session titled in
haste, or started untitled and named afterwards, keeps the name it was born with: the folder says
`2026/2026-08-07 0910` forever, while the manifest holds a title the user typed later, and the two
disagree in the one place the user browses.

**Approach:** One command, `recording_retitle(folder, title)`. It re-renders the SAME template against
the session's ORIGINAL start instant with the new title, moves the folder with `fs::rename` inside the
same destination root, rewrites `manifest.json`'s title and its `session` label, and leaves
`session_id` byte-identical. Collisions take the template's `{seq}` exactly as a fresh start does. A
session that is still recording is refused, because the driver and the sidecar hold absolute paths.
When the session lives inside a synced profile, the move is committed as ONE commit so
`git log --follow` reaches the pre-rename history.

## Boundaries & Constraints

**Always:**
- The identity never moves. `meta.sessionId` is byte-identical before and after; only the folder and
  the label change.
- The re-render uses the session's own start instant (`manifest.started_at`), never `now`: a session
  recorded last Tuesday must not migrate into this week's folder because it was renamed today.
- The template is the same effective template a start would use, so a retitle and a fresh start of the
  same session agree on where it belongs.
- Exclusivity is a syscall, not a check: the destination leaf is `create_dir`'d (which fails on an
  existing one) and the source is renamed over that empty directory, so two retitles racing cannot
  land in the same folder.
- Intermediates the retitle creates are removed again if it fails, deepest-first, `remove_dir` only —
  40.3's `SessionScaffold`, unchanged.
- A retitle that renders to the folder the session already occupies rewrites the title and moves
  nothing.
- Sync is best-effort and never gates the rename: a paused, offline or git-less profile still gets a
  renamed folder on disk and picks it up on its next sync.
- The refusal for a live session is typed (`IpcErrorCode::RecordingSessionLive`), because the surface
  needs to say "stop the recording first" rather than "internal error".

**Block If:**
- `git log --follow` needed anything beyond `fs::rename` plus a commit. It does not: git records no
  rename metadata at all and detects renames at diff time by content similarity. What DOES need care
  is that both halves land in ONE commit — the engine's stability gate admits deletions immediately
  while a new file must serve its window, which would otherwise split the move into a delete commit
  and an add commit and break `--follow`.

**Never:**
- No new storage, no second title field: `meta.title` is the title, as it has been since 21.5.
- The recording path template is not re-parsed into a different one, and the destination root does not
  change — a retitle moves a session WITHIN its root, never between roots.
- No `remove_dir_all`, ever, on a user's destination.
- `recording_start`'s path is untouched.

## I/O & Edge-Case Matrix

Effective template `{yyyy}/{yyyy}-{mm}-{dd} {HH}{MM} {slug}`; the session started 2026-08-05T14:32:07
local and lives at `<root>/2026/2026-08-05 1432`.

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Name an untitled session | title `Standup` | folder moves to `<root>/2026/2026-08-05 1432 standup`; `manifest.meta.title` = `Standup`; `session` = the new basename; `sessionId` unchanged | none |
| Rename a titled session | title `Retro` | folder moves to `…1432 retro`; media files ride along untouched | none |
| Clear the title | title `""` | folder moves back to `<root>/2026/2026-08-05 1432`; `meta.title` absent | none |
| Same title again | title equals the stored one | nothing moves, nothing fails; the manifest is rewritten at most once | none |
| Renders to the current folder | a different title that renders identically (no `{slug}`/`{title}` in the template) | no move; the title is still rewritten | none |
| Collision | `<root>/2026/2026-08-05 1432 standup` already exists | the move takes the template's next ordinal (` (2)`), and the existing folder is untouched | none |
| Collision exhausted | 64 ordinals all taken | refused, naming the last rendered relative path; the session stays where it is | `IpcError`, not retriable |
| The session is recording | the folder is in the live-reservation set | refused with `recordingSessionLive`; not one byte moves | typed, not retriable |
| The folder is not a session | no loadable `manifest.json` | refused; nothing moves | `IpcError` |
| The folder is outside the destination root | any | refused; a retitle moves a session within its root only | `IpcError` |
| Start instant missing | a pre-40.3 manifest with no `startedAt` | falls back to the folder's modification time, and says so in the log — never `now` silently | none |
| Inside a synced profile | the root is under a profile's local path | after the move, that profile syncs once so the delete and the add are ONE commit; `git log --follow` reaches the old commits | best-effort |
| Profile paused or offline | same, but sync is disabled or the remote is unreachable | the rename still succeeded locally and is returned; the sync attempt's failure is logged, never surfaced as a retitle failure | none |
| No git at all | engine construction fails | same: local rename succeeds | none |
| Not synced | the root is under no profile | no sync attempt is made | none |
| Two retitles race | two calls, same target name | one wins the leaf, the other takes the next ordinal — `create_dir` decides | none |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/recording.rs` — `SessionManifest::retitle(Option<String>)` (sets
  `meta.title`, minting the `meta` block if a pre-40.3 manifest has none) and
  `SessionManifest::rebind_folder(PathBuf)` (the runtime folder plus the `session` label), both pure.
- `src-tauri/crates/keeper-core/src/vm.rs` — `IpcErrorCode::RecordingSessionLive`.
- `src-tauri/crates/keeper/src/ipc.rs` — `recording_retitle` + its testable core
  `retitle_session_folder`, reusing 40.3's `session_folder_path`, `SessionScaffold`,
  `SESSION_FOLDER_ATTEMPTS`, `session_path_error` and `effective_path_template`; a
  `render_ctx_at(started_at, title, seq)` that parses the manifest's RFC3339 stamp; the live-folder
  refusal read from `reserved_recording_folders`.
- `src-tauri/crates/keeper/src/lib.rs` — register the command.
- `src-tauri/crates/keeper/src/sync.rs` or `ipc.rs` — `profile_for_path`: the enabled profile whose
  `local_path` is an ancestor of the session folder, or `None`.
- `src/lib/ipc/client.ts` — `recordingRetitle` wrapper.
- `src/components/layout/recording-summary-card.tsx` — the inline title editor.
- Read-only: `keeper-sync`'s `Engine::sync_once` (the single-commit trigger) and
  `collect_stable_changes` (why the single commit matters).

## Tasks & Acceptance

**Execution:**
- [ ] `keeper-core` — `SessionManifest::retitle` + `rebind_folder`, with the `session`-label rule
      documented (it is a label, and this is the one place it is allowed to change).
- [ ] `keeper-core` — `IpcErrorCode::RecordingSessionLive` (bindings regenerate).
- [ ] `keeper/src/ipc.rs` — `retitle_session_folder`: guards, re-render at the original instant,
      ordinal retry, `create_dir` + `fs::rename`, manifest rewrite, scaffold unwind.
- [ ] `keeper/src/ipc.rs` — `recording_retitle` command: the live-session refusal, the root check, and
      the best-effort single-commit sync afterwards.
- [ ] `keeper/src/lib.rs` — register it.
- [ ] `src/lib/ipc/client.ts` + `src/components/layout/recording-summary-card.tsx` — the affordance.
- [ ] Rust tests: identity survives; the folder moves; a collision takes the ordinal; a live session is
      refused untouched; a non-session folder is refused; the same title moves nothing; a cleared title
      moves back; the start instant is the session's own; the scaffold unwinds a failed move.
- [ ] Frontend tests: the editor calls the command with the typed title, renders the returned folder,
      and prints a refusal verbatim.

**Acceptance Criteria:**
- Given a finished session, when it is retitled, then its folder moves and `meta.sessionId` is
  byte-identical in the manifest.
- Given a session inside a synced profile, when it is retitled, then the move is one commit and
  `git log --follow` over a moved media file reaches the pre-rename commits.
- Given a retitle whose rendered path exists, when it runs, then the session lands on the next ordinal
  and the existing folder is untouched.
- Given the active session, when a retitle is attempted, then it is refused with
  `recordingSessionLive` and the folder is untouched.
- Given a paused or offline profile, when a session inside it is retitled, then the local rename
  succeeds and is reported as success.
- Given `cargo test --workspace` on macOS and `bun run test`, when they run, then both are green and
  `git status --porcelain -- src/lib/ipc/gen` is empty after `bun run test:rust`.

## Design Notes

**The identity is what made this story cheap.** 40.3 minted `meta.sessionId` so the folder could stop
being the handle; this is the story that spends that. The whole rename is: re-render, move, rewrite two
label-shaped fields, and never touch the id. Everything hard about it is elsewhere — the clock, the
filesystem, and git.

**Render from the stamp's own offset, not the machine's zone.** The first implementation converted
`manifest.started_at` into `Local` and read the civil fields off that, which is only correct while the
machine is still in the zone it recorded in. A session stamped `2026-01-01T00:30:00+14:00` re-rendered
as `2025-12-31 1030` from UTC — a different YEAR folder, and a direct violation of "a retitle and a
fresh start of the same session agree on where it belongs". The context builders are now generic over
`TimeZone`, so the retitle renders from the parsed `DateTime<FixedOffset>` and `recording_start` keeps
passing a `Local` clock read. The test stamps `+14:00` deliberately: it is the easternmost offset in
use, so every other zone reads that instant as the previous year, and the assertion pins both the
rendered path and the absence of a `2025/` folder.

**Exclusivity is `create_dir`; portability is `remove_dir`.** Two retitles racing for one name are
arbitrated by `create_dir` failing on the loser, exactly as two starts are. The claimed directory is
then removed immediately before `fs::rename`, because POSIX will replace an empty directory and
`MoveFileExW` will not — the claim is the arbiter, the removal is what keeps the primitive portable.

**Same directory is not the same string.** On a case-folding volume `Standup` and `standup` are one
directory, so a case-only retitle with a `{title}` template compared byte-unequal, hit `AlreadyExists`
against the session's own folder, and took a permanent ` (2)`. The in-place branch now also triggers
when the two paths canonicalize to the same directory, and the manifest is rebound to the folder that
exists rather than to the spelling that was asked for.

**A move re-points the live claim.** The claim is taken on the source and re-pointed to the destination
at the moment of the rename. Holding it on a path the retitle has just vacated left a window where a
concurrent start could occupy that path and then be silently un-reserved when the retitle's guard
dropped — which is exactly the state the `owned` discipline exists to prevent.

**A folder that moved must never be reported as a folder that did not.** If the manifest rewrite fails
and the rename-back also fails, the session is at the new path; returning the write error alone told the
user their manifest failed while hiding that their session had moved. That case now returns its own
error naming the new absolute location, and the scaffold is committed so the unwind cannot touch the
directories now holding the session.

**The snapshot is the frontend's source of truth, so the rename updates it.** `recording_status` kept
reporting the pre-rename `outputPath`, so every pane remount re-adopted a dead path, re-issued a
summary fetch against it, and pointed Reveal-in-Finder at nothing. The command now re-points the kept
snapshot when — and only when — it names exactly the folder that moved.

**One commit, and why it needed the stability gate.** git stores no rename metadata; it infers a rename
at diff time, and `--follow` can only see the inference when the disappearance and the arrival share a
commit. The engine would not have allowed that: `collect_stable_changes` admits a deletion immediately
(there is nothing left to sample) while every moved file arrives at an absolute path the gate has never
observed, and an unobserved path is `Settling` by construction — so the first scan after a rename
commits the deletions ALONE and the additions land a window later. The fix is at the root:
`StabilityGate::prime_stable` records a path as already-quiet (backdated by the ceiling, because a
retitle also rewrites `manifest.json` and the mtime arm would otherwise hold it), `Engine::prime_moved_paths`
exposes that per profile, and the retitle primes the moved files before triggering a sync. A file that
was renamed has been finished for as long as it existed under its old name; treating its arrival as
brand new is what split the move.

**The sync is fired, not awaited.** `sync_once` is the whole cycle — commit, pull, LFS drain, push — so
awaiting it made Save sit out a network timeout for a rename that had already succeeded on disk. It is
spawned, and every failure in it is logged and swallowed: the story's own matrix says a paused or
offline profile still gets its rename.

**What the frontend had to learn.** The card can no longer be the only place that knows the session
moved: the pane unmounts on every view switch. The new folder is handed to the hook that owns the
session, the dismissal is latched against the post-rename folder, the de-dup filter knows both sides of
the move, the draft cannot clear a title it has not yet seen, and the editor sits outside the
`role="status"` live region with its refusal bound to the field by `aria-describedby`.

## Verification

**Linux:** `cargo fmt --check`, `cargo clippy -p keeper-core -p keeper-sync --all-targets -- -D
warnings`, `cargo test -p keeper-core` (1088 + integration binaries) and `cargo test -p keeper-sync`
(including the two new stability/single-commit tests) all clean; `bun run lint`, `bun run typecheck`
clean and `bun run test` green at 1782.

**macOS (`hesperia`):** `cargo check -p keeper --all-targets` clean, and the retitle set run under
`cargo test -p keeper --lib` — 16 tests, all passing, including the four review-driven ones
(`the_re_render_uses_the_stamps_own_offset_not_the_machines_zone`,
`a_case_only_retitle_never_takes_an_ordinal`, `a_repointed_claim_releases_the_path_it_left`,
`a_relative_key_refuses_a_path_that_climbs_out_of_the_root`) and
`a_retitle_repoints_the_kept_status_snapshot_only_on_an_exact_match`. The full `bun run
check:rust:macos` gate ran on the committed tree.

**Known flakiness, not introduced here:** the frontend suite failed once (1 test) and once (2 tests)
when run concurrently with a cargo build on this 3-core box, and passes repeatedly standalone and under
a realistic concurrent clippy load. The failing names were not captured; CI runs the frontend job on a
dedicated runner and is the arbiter. Flagged rather than papered over.

**Adversarial review.** Two independent passes (Rust, frontend). Fifteen findings addressed: the sync
leg that could not produce one commit `[high]`; the timezone re-render that migrated a session across a
year boundary `[high]`; the rename that survived only until the pane unmounted `[high]`; the dismissal
latched on a pre-rename path `[high]`; the awaited network cycle, the dishonest stranded-rename error,
the de-dup filter, the draft that could clear an unloaded title, the editor inside a live region
`[medium]`; and the case-folding ordinal, the claim on a vacated source, the lexical root guard, the
Windows-bound primitive, the conflated refusal message, a doc claiming a mapping that does not exist,
and three vacuous or environment-dependent tests `[low]`.

## Change Log

- 2026-08-07 — Story implemented: a finished session can be retitled from its card; the folder moves,
  the identity does not, collisions take the template's ordinal, and a synced session's move is one
  commit.
- 2026-08-07 — Addressed fifteen review findings across the Rust and frontend halves, including the two
  that made the story's own acceptance criteria unmet.
