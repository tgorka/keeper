---
title: 'Story 44.17: Sync Status in Files'
type: 'feature'
created: '2026-08-09'
status: 'review'
blocking_condition: ''
baseline_revision: '15a3b41'
final_revision: ''
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/epic-44-the-vocabulary-is-the-space.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-43-8-the-files-tab.md'
---

<intent-contract>

## Intent

**Problem:** the Files tab shows what is in a folder and says nothing about whether any of it has
left the machine. Every row looks the same whether it is committed and pushed, sitting in a settle
window, waiting on a commit, in a folder that has never been adopted as a repository, or matched by
a pattern that guarantees it will never sync at all. The last of those is the one that costs
something: 43.8 dropped an excluded entry from the listing entirely, so a user who put `*.psd` into
their own profile saw a hole where their files were, with nothing on screen connecting that hole to
the rule they typed.

**Approach:** the listing joins the dirents it already reads to the pending list the engine already
derives. `Engine::pending` is documented as computed-never-stored precisely so a visible answer
cannot drift from the real one; `sync_browse` calls it — the same call `sync_pending` makes for the
Sync pane's Pending card — and hands the result to `keeper_sync::browse` as data. `browse` stays
engine-free: it takes a `&SyncProfile`, an `ExcludeSet` and a `PendingView`, and the only new thing
it touches on disk is one `.exists()` for `.git`.

The exclusion question splits in two. Keeper's built-in noise corpus stays invisible, as it has been
since 43.8 — nobody chose `.DS_Store` or `.git/**`, and rendering them would be the browser nobody
scrolls twice. A pattern from the **profile's own** configuration now lists its files and marks
them excluded, because that is a rule a person wrote and is entitled to see working.

### What already existed, and what was reused rather than restated

| Existing behaviour | Where | Reused as |
|---|---|---|
| "What has this folder not synced yet, and why" — settling from `file_state`, plus a real `git status` walk, tier-0 filtered | `keeper-sync/engine.rs::Engine::pending` (Story 32.2) | the whole of the mark's truth; nothing is re-derived |
| The five reasons a path is pending | `engine.rs::PendingReason` | carried through `EntrySyncStatus::Waiting`, worded once in `sync_ipc::sync_mark` |
| "Does git have anything to say about this folder" | `Engine::pending`, L5057: `profile.local_path.join(".git").exists()` | the same expression, for `NotInRepository` |
| The compiled per-profile exclusion corpus | `keeper-sync/exclude.rs::ExcludeSet` | extended with a *narrower* question (`verdict`), not a second rule set |
| Lazy one-directory listing, containment, the cap, absent media | `keeper-sync/browse.rs` (43.8) | unchanged; one field added to `BrowseEntry` |
| The roving tabindex and its "row actions only while focused" rule | `files-pane.tsx` (43.8) | why the mark takes no tabindex at all |
| Sentences composed in Rust, never in TypeScript | `FilesListingVm::detail` (43.8) | `FilesEntrySyncVm::detail` follows it exactly |

## Boundaries & Constraints

**Always:**
- The mark is **read**. Nothing in the listing path opens a repository, runs a status walk, reads
  the journal, touches `file_state`, moves the scan clock or wakes the watcher.
- `Excluded` outranks every other state. A file that will never sync must never wear the mark of
  one that is about to.
- `Synced` is reachable only with a repository present. Absence from the pending list means nothing
  without one.
- An engine that could not answer produces `Unknown`, never a guess. Both available guesses are
  lies: one says the work is safe, the other says keep waiting.
- The words come from Rust. The frontend holds only a five-entry fallback label map for the case
  where Rust sent no sentence.

**Never:**
- No new dependency, frontend or Rust.
- No absolute path in anything the mark renders (FR-145); the mark renders no path at all.
- No second source of sync truth. The browser has no opinion about git; it has a `PendingView` it
  was handed.
- No focusable control inside a row that is not an action (43.8's tab order).

## I/O & Edge-Case Matrix

`classify(relative_path, is_dir, verdict, pending, in_repository)` — `keeper-sync/browse.rs`.
Precedence runs top to bottom; the first row that matches wins.

| # | Input | Result | Why |
|---|---|---|---|
| 1 | Profile pattern matches (file or directory) | `Excluded` | The story's core. Also the only state that survives an engine that could not answer — a file that will never sync does not become uncertain because something else broke |
| 2 | Built-in corpus matches and no profile pattern does | *entry is not listed at all* | 43.8's rule, unchanged. `.git/`, `.DS_Store`, `*.part` |
| 3 | `PendingView::Unavailable` | `Unknown` | The engine failed. Every other value would be a claim with nothing behind it |
| 4 | Pending list names this exact path | `Waiting { reason: Some(r) }` | `r` is the engine's own `PendingReason` |
| 5 | Directory, and some pending path starts with `dir/` | `Waiting { reason: None }` | Roll-up. The folder is not itself untracked; something in it is, and borrowing that word for the folder is a small confident lie |
| 6 | Directory `notes`, pending path `notes-archive/late.md` | `Synced` (not waiting) | The probe appends `/` before the range scan, so a sibling sharing a prefix is not a descendant |
| 7 | Not pending, `.git` absent at the profile root | `NotInRepository` | An unadopted folder reporting `Synced` is the lie this state exists to stop |
| 8 | Not pending, `.git` present | `Synced` | |
| 9 | Settling file in a folder with no `.git` | `Waiting` (4 beats 7) | Both are true; "the engine is holding this file right now" is the more specific and the more actionable |
| 10 | Empty directory in a repository | `Synced` | Nothing inside it is waiting. Git does not track empty directories, so it will not appear on the other machine — see *Deliberately Not Done* |
| 11 | Deleted file in the pending list | *no row* | It is not on disk, so no dirent produces it. Its reason is worded anyway, for completeness |
| 12 | A profile root row (the tree's top level) | *no mark* | It carries no `FilesEntryVm`; its children answer for themselves |
| 13 | Entry beyond `LISTING_CAP` | *no row* | Unchanged; the cap is still reported |

`sync_mark(status, engine_failure)` — `keeper/src/sync_ipc.rs`. One sentence per state:

| Status | `detail` |
|---|---|
| `synced` | `null` — a file that is where it should be has no story |
| `waiting` / `Settling` | "keeper is waiting for this file to finish being written." |
| `waiting` / `Untracked` | "This file is new and has not been committed yet." |
| `waiting` / `Modified` | "This file has changed and has not been committed yet." |
| `waiting` / `Added` | "This file is staged and has not been committed yet." |
| `waiting` / `Deleted` | "This file has been deleted and the deletion has not been committed yet." |
| `waiting` / roll-up | "Something in this folder is waiting to sync." |
| `excluded` | "A pattern in this folder's sync settings excludes it, so keeper will never copy it." |
| `notInRepository` | "This folder is not a repository yet. The first sync sets one up and takes everything in it." |
| `unknown` | "keeper could not read this folder's sync state: {the engine's own words}" |

`SyncStatusMark` — `src/components/layout/sync-status-mark.tsx`:

| Input | Rendered |
|---|---|
| `{ status, detail }` | `<span role="img" data-sync-status={status} aria-label={detail ?? SHORT_LABEL[status]}>` holding one of five distinct glyphs |
| `detail: null` | Falls back to the short label; only `synced` reaches this in practice |
| any status | No `tabindex`, no `onClick`, no `<button>` |

</intent-contract>

## Code Map

| File | Change |
|---|---|
| `keeper-sync/src/exclude.rs` | `ExcludeVerdict` + `ExcludeSet::verdict`, backed by two more compiled sets holding the profile's own patterns only. `is_excluded` / `is_excluded_directory` are untouched, so every staging, queueing and counting caller sees the same boolean it always did |
| `keeper-sync/src/browse.rs` | `EntrySyncStatus`, `PendingView`, `BrowseEntry::sync`, `classify`, and a fourth `browse` parameter. A profile-excluded entry is now listed rather than dropped |
| `keeper-core/src/vm.rs` | `FilesSyncStatusVm`, `FilesEntrySyncVm`, `FilesEntryVm::sync`, and a fifth argument to `FilesEntryVm::new` |
| `keeper/src/sync_ipc.rs` | `sync_browse` asks `engine.pending` once per listing; `sync_mark` words the result; `files_listing_vm` threads the engine's failure sentence |
| `src/lib/ipc/gen/*` | Regenerated by `cargo test -p keeper-core --lib export_bindings`; never hand-written |
| `src/lib/ipc/client.ts` | Two type re-exports |
| `src/components/layout/sync-status-mark.tsx` | New. The mark |
| `src/components/layout/files-pane.tsx` | One import, one element after `</RowName>` |

## Design Notes

### Why the shell fetches the pending list and `browse` does not

`browse`'s module doc already says why it takes a `&SyncProfile` and not an `&Engine`: so it
*cannot* spend the stability gate's verdict, clear `file_state` or wake the watcher. That property
is worth more than the convenience of self-service, so the shape is unchanged — the shell owns the
one call to `Engine::pending`, and `browse` receives a `PendingView` it can only read.

The alternative considered and rejected was for `browse` to open the repository itself and run
`git::repo::status_paths`. It is one function call and it would have been wrong twice over: it is a
second answer to a question `Engine::pending` already answers (so the Files tab and the Pending card
would eventually disagree about the same file), and `git::repo::open` is where trust levels, local
config enforcement and index handling live — precisely the neighbourhood a browser should not be in.

### Why an excluded file is now visible

This reverses one sentence of 43.8, deliberately. 43.8's rule was "minus the noise sync already
knows to skip", and for the built-in corpus that is exactly right. It is wrong for the patterns a
user typed into their own profile: dropping those rows makes the user's own configuration invisible,
and the story's acceptance criterion — "an excluded file says excluded rather than waiting forever"
— cannot be met by a row that is not there. The split is in `ExcludeSet::verdict`, which is one
question against the same strings rather than a second rule set: the profile sets are built in the
same loop, from the same patterns, by the same two helper functions.

Precedence inside `verdict` is profile-first. If a user's pattern happens to also match something in
the corpus, the row is shown — a rule someone wrote wins the naming.

### Why there is a fifth state

The story names four. `Unknown` is the fifth and it exists for the reason `BrowseListing` separates
`MediaAbsent` from `Missing` from an empty `Listed`: when the answer is not available, every
available value is a claim. `Engine::pending` can fail — an unreadable or corrupt repository is the
reachable case — and a folder whose repository is broken is exactly the folder somebody most needs
to look inside. So the listing still returns, marked `unknown`, carrying the engine's own words.

### The cost of the mark

One `Engine::pending` per `sync_browse` call, which is one full `git status` walk of the profile's
worktree plus one `file_state` read. On a hundred-thousand-file pendrive that is seconds, and it is
paid per directory expansion. This is stated rather than hidden because it is the honest price of
not keeping a second copy of sync truth, and because the same walk already runs on every Pending
poll. Two things bound it: the call is made once per listing rather than once per row, and it runs
before `spawn_blocking` on the async runtime — see *Deliberately Not Done* for what was not done
about either.

### The mark is pull-based

The mark is part of the listing, so it is exactly as fresh as the listing. Refresh re-reads every
open folder and the marks move with it. The pane does not subscribe to sync progress: it is
deliberately lazy (43.8), and a pushed update would re-list every open folder on a pendrive on every
sync tick — which is the behaviour 44.10 exists to prevent, arriving from the other direction.

### Directories

A folder's mark is a roll-up over the pending list, answered with one ordered-map range probe
against `dir/` rather than a scan per row, so a folder of a thousand entries stays linear against a
long pending list. The `/` is what makes `notes` and `notes-archive` different folders, and it is
asserted.

## Verification

Commands run: `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync --lib browse::`
(23 passed) and `bun run test src/components/layout/files-pane.test.tsx` (36 passed).
`cargo test -p keeper-core --lib export_bindings` was run once to regenerate the TypeScript
bindings, as `bindings:check` requires.

### The real-repository fixture

The four states are not proved through a hand-built `PendingView`. `browse::tests::committed_fixture`
builds a real bare remote with `gix::init_bare`, opens a real `Engine` over a real `TestPlatform`,
writes real files, and drives two real `sync_once` passes a settle window apart so the completeness
gate actually commits and pushes. Then:

- **synced** — `tracked.md` and `archive/old.md` are committed and clean; a real `git status` walk
  inside `Engine::pending` does not report them.
- **waiting / settling** — `settling.bin` is written and one further `sync_once` opens a settle
  episode without the clock advancing, so the gate is genuinely holding it and a real `file_state`
  row exists.
- **waiting / untracked** — `notes/fresh.md` is written after the last scan, so only git can answer
  for it, and the folder `notes` rolls up to `Waiting { reason: None }`.
- **excluded** — `drop.tmp` against the profile's own `*.tmp`, listed and marked, with `.DS_Store`
  beside it staying hidden.
- **not in a repository** — a second profile over a folder that was never adopted, whose real
  `Engine::pending` returns an empty list; the test asserts the emptiness first, so it is asserting
  that *this exact* emptiness does not read as success.

`a_listing_changes_no_byte_of_the_engine_or_the_repository` snapshots every byte of the profile's
`.git` and of the engine's data directory, runs three listings, and compares. It refuses to run
vacuously: both snapshots must be non-empty first.

### Mutation proof

Each mutation was applied, the command re-run, and the source restored.

| Mutation | Caught by |
|---|---|
| `Excluded` loses its precedence in `classify` | `a_profile_pattern_lists_the_file_and_marks_it_rather_than_hiding_it`, `an_engine_that_could_not_answer_marks_every_entry_unknown`, `each_state_comes_from_state_that_already_existed` |
| `PendingView::Unavailable` reads as "nothing pending" | `an_engine_that_could_not_answer_marks_every_entry_unknown` |
| No repository still reports `Synced` | `a_folder_with_no_repository_never_reports_its_files_as_synced`, `a_profile_pattern_lists_the_file_and_marks_it_rather_than_hiding_it` |
| A profile-excluded entry is hidden again (43.8's rule restored) | the same three as the first row |
| The roll-up probe drops its trailing `/` | `a_directory_rolls_up_its_own_descendants_and_not_its_neighbours` |
| `browse` writes one file into `.git` | `a_listing_changes_no_byte_of_the_engine_or_the_repository` |
| `excluded` is given the `waiting` glyph | `gives each state its own mark, so an excluded file never reads as waiting` |
| `excluded` is given the `waiting` sentence | that test, plus `renders the sentence Rust composed rather than one of its own` |
| The mark takes `tabIndex={0}` | `stays out of the tab order, on the focused row and every other one` |
| A profile root is given a mark | `gives a profile root no mark, because its children answer for themselves` |
| The mark freezes its status at first render | `shows the new mark once sync has moved on` |

### What could not be proved here, and why

- **`keeper/src/sync_ipc.rs` was not compiled.** The `keeper` shell crate does not build on Linux —
  `cargo check -p keeper` fails in `gobject-sys`'s build script for want of `pkg-config`, before any
  of this crate's own code is type-checked. So `sync_browse`, `files_listing_vm` and `sync_mark` are
  **unverified by any tool in this session**. Everything they do that can live below the shell does:
  the classification, the precedence, the roll-up and the exclusion split are all in `keeper-sync`
  and all asserted against a real repository. What remains in the shell is one `engine.pending`
  call, one `match` over five variants and the sentences. This needs a macOS build before the story
  is believed.
- **The data-directory half of the perturbation test is not mutation-proved.** The `.git` half is:
  writing one file into `.git` from inside `browse` fails the test. `browse` has no handle on the
  engine's data directory at all — it never learns where it is — so there is no reachable mutation
  that writes there, which is itself the structural guarantee. The assertion uses the identical
  comparator and the identical non-emptiness precondition as the half that is proved.
- **Nothing was measured on removable media, and nothing was measured on a large tree.** The cost
  paragraph above is reasoning about `Engine::pending`'s existing behaviour, not a measurement. The
  seconds-on-a-pendrive figure is `[INFERENCE]`.
- **`gix::status` was observed not to rewrite `.git`** in this fixture — the perturbation test would
  have failed if `Engine::pending` had been inside the snapshot window, but it is not: `pending` is
  called before the snapshot is taken. Whether `Engine::pending` itself perturbs the index was not
  measured, and it is pre-existing behaviour either way — `sync_pending` has called it on every
  Pending poll since Story 32.2.

## Deliberately Not Done

- **No caching of the pending list across listings.** A cache would be the second source of sync
  truth this story is written to avoid, and a stale mark is the "waiting forever" failure wearing a
  different hat. The cost is one `Engine::pending` per directory expansion, stated above.
- **No pathspec-scoped status walk.** `Engine::pending` walks the whole worktree. Scoping the walk
  to the listed subtree would cut the cost of expanding a deep folder, but it belongs to
  `Engine::pending` and to a story that can measure it — narrowing it from a browser would fork the
  one answer into two.
- **No push subscription.** The mark does not update itself while a sync runs; Refresh and
  re-expansion are the update path. See *The mark is pull-based*.
- **An empty directory reads `Synced`.** Nothing in it is waiting, which is true, and git does not
  track empty directories, so it will not appear on the other machine — which makes `Synced` beside
  the point rather than wrong. Answering it properly needs a walk per directory, which is the cost
  this story is already trying not to pay twice.
- **"Committed but not yet pushed" is not its own state.** `Engine::pending` does not distinguish
  it, and inventing the distinction here would mean asking git a question the engine does not ask —
  a second source of truth. A file whose push is still journaled reads `Synced`, which is what the
  Pending card says about it too.
- **Built-in-excluded entries are still not listed.** Showing them marked would fill every folder
  with rows about keeper's own housekeeping. `BUILTIN_EXCLUDES` is public so a surface can show the
  corpus in one place; a file browser is not that place.
- **No colour-only distinction, and no legend.** Five glyphs and five sentences; tone is emphasis
  only. A legend for five self-describing marks is a surface that needs explaining.
- **The profile root row has no roll-up mark.** It would need the pending list for a whole profile
  reduced to one word, and "this folder is partly waiting" is not a useful word.
