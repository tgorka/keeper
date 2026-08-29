---
title: 'A clone that stopped says so'
type: 'bugfix'
created: '2026-08-29'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: true
final_revision: 'engine.rs tick gate + journalled WorkKind::Checkout + additive restore_missing_checkout + empty-index refusal in stage_and_commit'
context: []
warnings: []
---

<intent-contract>

## Intent

**Problem:** On the owner's machine (0.8.23, hesperia, 2026-08-29) a first clone of
`tgdrive-light` stopped three minutes in and left `/Users/tgorka/tgdrive` with commits on
`HEAD`, **zero index entries**, and 16 GB of files on disk. Nothing recorded a failure
(`profiles.state = 'idle'`, `last_error = NULL`, zero journal rows), nothing retried it, and
every status walk of that repository emitted one deletion per path in `HEAD` — the field log's
`entries=155625 scanned=0 elapsed_ms=4274 deleted=155625`, every two seconds, for ever. One
gate away from committing the deletion of 155 625 files.

**Approach:** Three guards and one repair. A profile whose first working copy was never made is
handed to the journal — not to `scan_is_due` — so it is retried on the engine's own backoff and
says so on both error surfaces. A repository with commits and an empty index is refused at the
one place a commit is made, before a single removal is staged. A profile the engine has marked
offline pays for no status walk, paced by the queue rather than a new timer.

## Boundaries & Constraints

**Always:** Refuse before repairing, and never let the repair be able to lift the refusal. The
repair only ever ADDS: it writes `HEAD` paths that are missing and never a byte over a file
that is there. The index is written only when the repair is provably whole. Every gate must have
a way back — a folder skipped by a gate must still own a journal row that can clear it.

**Block If:** (none triggered.)

**Never:** No new scheduler, timer or cadence — the journal's existing `reschedule_after` backoff
is the only pacing added. No `git reset --hard`, no `git checkout --force`, no destination-wide
re-checkout: the folder holds the owner's bytes. No new `ProfileState` variant (owned by story
56.16 in the same worktree).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Clone interrupted, repair possible | `.git` present, `HEAD` has a tree, index empty, worktree files missing | Tick journals `WorkKind::Checkout`, drains only that kind; repair writes the missing files and the index | none |
| Clone interrupted, repair blocked | as above, plus a directory where `HEAD` holds a file | `SyncError::CheckoutUnfinished`; state `NeedsAttention`; `profiles.last_error` set; unit rescheduled with backoff | typed, retried |
| Empty index, files present | index deleted, 155 625 tracked files on disk | `commit_local` refuses before the walk; `stage_and_commit` refuses before the index write | typed; NO commit, NO index write |
| Repository never cloned | `.git` absent | Tick journals `WorkKind::Checkout`; `open_repo` clones or adopts | underlying error wrapped, retried |
| Profile offline, nothing due | `state = 'offline'`, no claimable unit | Tick returns; **zero** status walks | none |
| Profile offline, a unit is due | `state = 'offline'`, backoff elapsed | Tick proceeds exactly as before | as before |
| Profile idle and online | ordinary folder | Walks exactly once per tick, as before | as before |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/engine.rs` — `tick_profile` gates; `first_checkout_is_unfinished`, `enqueue_first_checkout`, `remote_within_reach`, `do_checkout`, `finish_first_checkout`; `set_error`/`state_of`; `clear_warning` and `record_failure` now persist `profiles.last_error`; `collect_stable_changes` pre-walk refusal; `claim_walk` counts `EngineCounters::status_walks`.
- `src-tauri/crates/keeper-sync/src/git/repo.rs` — `index_is_unpopulated`, `checkout_is_unfinished`, `index_entry_count` (12-byte header screen), `CheckoutRepair` / `restore_missing_checkout`.
- `src-tauri/crates/keeper-sync/src/git/commit.rs` — the innermost empty-index refusal in `stage_and_commit`, above the index write.
- `src-tauri/crates/keeper-sync/src/db.rs` — `WorkKind::Checkout` (+ `CHECKOUT`, `tag`, `covered_while_running`); `set_profile_error` replacing the dead `set_profile_runtime`; `has_ready_unit`.
- `src-tauri/crates/keeper-sync/src/error.rs` — `SyncError::CheckoutUnfinished { path, detail }`, `Transient`, code `checkoutUnfinished`.
- `src-tauri/crates/keeper-syncd/src/commands.rs` — `sync_exit_code` arm (`EXIT_FAILURE`).
- `src-tauri/crates/keeper/src/sync_ipc.rs` — `sync_ipc_error` arm (`IpcErrorCode::SyncUnavailable`). **Not compilable on this host** (the `keeper` shell crate does not link on Linux); reported as unverified.

## Tasks & Acceptance

**Execution:**
- [x] `git/repo.rs` — classify and repair the unfinished checkout, additively.
- [x] `git/commit.rs` — refuse a deletion derived from an empty index, before the index write.
- [x] `db.rs` — journal the checkout; give `profiles.last_error` a writer; add the ready-unit probe.
- [x] `error.rs` + the two exhaustive matches — one typed refusal with one human sentence.
- [x] `engine.rs` — the two tick gates, the checkout unit, the error surfaces, the walk counter.
- [x] tests — four required behaviours plus three that pin the repair's safety.

**Acceptance Criteria:**
- Given a first clone interrupted mid-checkout, when the supervisor ticks, then `profiles.last_error` and the status snapshot both carry a sentence naming what happened, and a `checkout` journal row exists.
- Given that folder once unblocked, when the next tick runs after the backoff, then the missing files are restored from the existing commit, the index is written, and **no commit is created**.
- Given a repository with commits and an empty index, when anything tries to commit, then it is refused and neither a commit nor an index is written.
- Given a profile the engine has marked offline with nothing due, when it ticks, then it performs **zero** status walks.
- Given an idle, online profile, when it ticks, then it walks exactly once, as before.

## Design Notes

### D2 — why keeper's walk saw 155 625 deletions on a folder whose `git status` is clean

Established from the code, not guessed. The log line is written at
`src-tauri/crates/keeper-sync/src/git/repo.rs:1891-1904` and its two numbers do not mean what
they look like:

- `entries` is `watchdog.beats()` — items **emitted** by the walk (`repo.rs:1407-1414`,
  incremented per item at `repo.rs:1827`), not the size of the index.
- `scanned` is `watchdog.scans()` — gix's own counter of index entries compared against the
  worktree (`ScannedEntries`, `repo.rs:1187-1194`).

So `entries=155625 scanned=0 … added=0 modified=0 deleted=155625` says: **zero index entries were
compared, and 155 625 items were emitted, every one of them a deletion.** An index-versus-worktree
comparison cannot produce a deletion it never compared, so all 155 625 came from the other half of
the walk — `Item::TreeIndex(gix::diff::index::Change::Deletion)`, filed into `RepoStatus::deleted`
at `repo.rs:1544-1546`. That is `HEAD`'s whole tree diffed against an **empty index**.

The cause is therefore not a stale or wrongly-resolved worktree path: it is the same broken
repository as D1, seen from the other side. `git status --porcelain` in that folder agrees — the
three `D` lines quoted in the report are simply the first three of 155 625, dot-files sorting
first. The path from there to disaster is
`engine.rs:5054` (`staged.deleted.extend(status.deleted)`) → `engine.rs:5541` (`self.commit`) →
`git/commit.rs` `stage_and_commit`, which removes the named entries from an already-empty index,
writes it, folds it into a tree, and commits a tree with nothing in it. Proven, not asserted:
mutation **M3** below shows `stage_and_commit` returning a real commit id from exactly that state.

The empty-index refusal is therefore the actual fix and the offline gate is the seatbelt, exactly
as the report framed it.

### Refuse first, repair second — and why both are safe together

Repairing is what the owner wants; refusing is what is safe. Both ship, in an order a later
caller cannot reverse:

1. **The refusal is unconditional and innermost.** `git/commit.rs` checks two integers
   (`sorted_len == 0 && !changes.deleted.is_empty()`) *above* the staging loop and above
   `index.write`, because that function writes the index before the commit on purpose — a guard
   one line later would already have staged 155 625 removals durably. It does not consult the
   repair and the repair cannot switch it off. `collect_stable_changes` repeats the check before
   the walk purely for cost (4.3 s and 155 625 emitted items to reach a conclusion two integers
   already hold); the inner one is the guarantee, and its own test reaches past the outer door on
   purpose so a mutation cannot hide behind it.
2. **The repair only ever adds.** `restore_missing_checkout` sets
   `destination_is_initially_empty = true`, which makes `gix_worktree_state` open every
   destination with `create_new` — an existing path fails `AlreadyExists` and is recorded as a
   collision instead of being truncated. This is load-bearing and counter-intuitive:
   `overwrite_existing = false` **alone does not protect the bytes**, because with
   `destination_is_initially_empty = false` the same code opens with
   `create(true).truncate(true)` (`gix-worktree-state/src/checkout/entry.rs:247-255`) and there is
   no freshness filter anywhere in that path — every entry is written. Mutation **M8** flips the
   one line and the safety test fails.
3. **The index is written last, and only when the repair is whole.** An index built from `HEAD`
   names every path; a worktree holding only half of them then reads the other half as deleted —
   the very catastrophe. So an interrupt, an IO error, or a collision that is not "a plain file is
   already here" leaves the index untouched and the refusal standing. A collision's *error kind* is
   not enough to decide this: a directory where `HEAD` holds a file collides with `AlreadyExists`
   on Linux exactly like a real file does, so each collision is `lstat`ed and only a file or
   symlink counts as present (mutations **M7**, **M9**).

### Why the retry is a journal unit and not a timer

`WorkKind::Checkout` is enqueued from `tick_profile` **above** both gates, and the tick then
drains that kind alone (`drain_kind`, the Story 56.13 door) and returns. Three consequences:
`scan_is_due` — whose job is deciding whether a *walk* is worth its cost — never decides whether a
clone is retried; the unit's `reschedule_after` backoff is the pacing, so no second clock exists;
and such a profile performs zero walks, because the only work it can do is the checkout.

`covered_while_running` includes `Checkout` so a 16 GB clone does not collect one duplicate row
per tick for the whole time it runs.

### Why `do_checkout` states the failure itself

`record_failure` lets the *cause's* retriability pick the state word, and for the cause that
actually happened — the network going away mid-clone — that word is `Offline`, whose arm records
no error at all. AD-49 is right about an ordinary folder (offline is a state, not a failure,
because local git keeps working) and wrong about this one: there is no local git here. So
`do_checkout` writes `NeedsAttention` and the sentence to both surfaces before returning, and
returns a `Transient` error so the ordinary backoff still applies. The sticky warning and the
native toast are deliberately left to `record_failure`'s existing run-of-failures threshold — a
blip during a first clone is ordinary.

### `profiles.last_error` had no writer

The column has existed since the schema was written and **nothing ever set it**:
`set_profile_state` writes `state` only, and the one function that touched `last_error` —
`set_profile_runtime` — had no callers in any crate. It is now `db::set_profile_error`, called
from `Engine::set_error` (and cleared by `clear_warning`, only when there was an error, so the
journal's hot path gains no write). Mutation **M2** proves it.

### The offline gate cannot deadlock

`Offline` is sticky: nothing in the supervisor's tick clears it except work succeeding. A gate
that read the word alone would be a trap — a folder that went offline with an empty queue could
never walk, so nothing would be enqueued, so nothing would succeed, so the word would never
change. `remote_within_reach` therefore asks the journal: `Offline` is only ever written by a
transient network failure, and a transient failure re-queues *its own* unit with backoff, so the
row that made the profile offline is the row that will clear it, and "a unit is due" is exactly
"the backoff says try the remote again now". The one case where no such row would exist — a first
clone that died on the network — is handled by `enqueue_first_checkout` running *above* this gate.

### Recovering `/Users/tgorka/tgdrive`, given what the code now does

With this build installed, **nothing manual is required and nothing should be deleted.** The
owner should:

1. **Install the new build and leave the folder exactly as it is.** Do not delete `.git`, do not
   run `git reset --hard`, do not re-add the folder, and do not move the 16 GB aside. Every one of
   those either throws away the objects already fetched or risks the worktree.
2. On the first tick, keeper sees `.git` present, the index empty and `HEAD` holding a tree, so it
   journals a `checkout` unit and drains only that. `restore_missing_checkout` rebuilds the index
   from `HEAD` and writes **only the paths that are missing from disk**; the 16 GB already there
   is opened with `create_new`, fails `AlreadyExists`, and is left byte-for-byte alone. The index
   is written only once every entry has a real file behind it.
3. Until that succeeds, nothing can commit: `commit_local` refuses before the walk and
   `stage_and_commit` refuses before the index write, so the 155 625 deletions cannot reach the
   object database by any path.
4. If it cannot finish — a directory in the way, a permission refusal — the folder now **says so**:
   `profiles.last_error` and the app's folder card both carry
   `/Users/tgorka/tgdrive: this folder's first copy never finished. …`, naming the first blocked
   path, and the unit retries on backoff. The manual remedy is then to clear that one named
   obstruction, not to touch the repository.
5. Only if the owner wants to verify by hand: `git -C /Users/tgorka/tgdrive ls-files | wc -l`
   should go from `0` to the full tracked count, and `git status --porcelain` should stop reporting
   `D` lines. Both are reads.

The one thing a human must **not** do is what the pre-fix state invites: running `git add -A` or
letting any older keeper commit from that folder, either of which records the deletion of every
tracked path.

## Verification

**Commands:**
- `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync -p keeper-core -p keeper-syncd` — **3596 passed, 0 failed** (baseline 3579).
- `cargo clippy --manifest-path src-tauri/Cargo.toml -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings` — clean.
- `cd src-tauri && cargo fmt --check` — clean.

**Mutation proof** (each guard removed, its owning test re-run, then restored and the restore
verified by reading `git diff`):

| # | Guard mutated | Owning test | Observed failure |
|---|---|---|---|
| M1 | `tick_profile`'s first-checkout gate → `if false` | `an_interrupted_first_clone_says_so_and_is_retried` | `a tick never raises: CheckoutUnfinished { … }` — no retry unit is ever journalled |
| M2 | `set_error` stops writing `profiles.last_error` | same | `and it must be in the profile row… left: None` |
| M3 | `stage_and_commit`'s empty-index guard → `if false` | `a_deletion_out_of_an_empty_index_is_refused_and_never_staged` | `an empty index cannot authorize a deletion: Some(Sha1(f5dd859…))` — it commits the mass deletion |
| M4 | `collect_stable_changes`' pre-walk refusal → `if false` | `an_empty_index_is_refused_and_stages_no_deletion` | `the refusal is taken before the walk… left: 3 right: 2` |
| M5 | `remote_within_reach` → always `Ok(true)` | `an_offline_profile_walks_nothing` | `an offline folder must not buy a full-tree walk… left: 3 right: 2` |
| M6 | `remote_within_reach` → always `Ok(false)` | `an_idle_online_profile_still_walks` | `the ordinary tick of an ordinary folder walks once… left: 2 right: 3` |
| M7 | repair counts any collision as "kept" (no `lstat`) | `an_interrupted_first_clone_says_so_and_is_retried` | `a clone that did not finish must leave an error the UI can read` |
| M8 | `destination_is_initially_empty = false` | `a_repair_writes_what_is_missing_and_overwrites_nothing` | `the path that was already there was left alone… left: 0 right: 1` |
| M9 | index written even when the repair is unfinished | `a_repair_that_cannot_finish_writes_no_index` | `NO INDEX: half a checkout plus a full index is exactly the state that reads as a mass deletion` |

**Manual checks:**
- `src-tauri/crates/keeper/src/sync_ipc.rs`'s new `sync_ipc_error` arm cannot be compiled on this
  host (the `keeper` shell crate does not link on Linux — `gobject-sys`). Symbols touched there:
  `sync_ipc_error`, one arm mapping `SyncError::CheckoutUnfinished` to
  `IpcErrorCode::SyncUnavailable`. Must be compiled on macOS before release.
- `git status --porcelain -- src/lib/ipc/gen` is empty: no ts-rs binding drift (no exported type
  changed).
