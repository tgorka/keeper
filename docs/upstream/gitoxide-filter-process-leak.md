# gitoxide: `process` filter driver children are never reaped

A defect report and contribution plan for [GitoxideLabs/gitoxide](https://github.com/GitoxideLabs/gitoxide).
Every claim below carries a file path and line number that was read, on `main`
unless stated otherwise. Line numbers marked *(vendored)* come from the crates.io
sources of `gix 0.86.0` / `gix-filter 0.33.0` / `gix-status 0.33.0` and may drift.

---

## 1. The defect

`gix_filter::driver::State::shutdown` is the only code in the entire workspace
that `wait()`s a long-running `process` filter child. **It has zero production
callers.**

- `gix-filter/src/driver/shutdown.rs:19` —
  `pub fn shutdown(self, mode: Mode) -> Result<Vec<(BString, Option<ExitStatus>)>, io::Error>`.
  Takes `self` **by value**, iterates `self.running`, and under
  `Mode::WaitForProcesses` calls `client.into_child()` then `child.wait()?`.
- A repo-wide code search for `shutdown::Mode` and `into_driver_state` returns
  exactly two hits: the definition, and `gix-filter/tests/filter/driver.rs`.
- `gix-filter/src/driver/mod.rs` — `State` has **no `Drop`**. Because `shutdown`
  consumes `self`, it could not be called from one as written.

`State`'s own doc comment acknowledges the hazard and leaves it to the caller:

> Note that `shutdown()` must be called to finalize long-running processes.
> Failing to do so will naturally shut them down by terminating their pipes, but
> finishing explicitly allows to wait for processes as well.

The field doc on `running` goes further, and is the sentence to correct:

> these processes are expected to shut-down once their stdin/stdout are dropped,
> so nothing else needs to be done to clean them up after drop

That is true of **termination** and false of **reaping**. On Unix, closing the
pipes makes the child exit; it then stays in the process table as a zombie until
its parent `wait()`s it. For a one-shot CLI nobody notices. For a long-lived
application it is an unbounded leak of process-table slots.

### It is worse than one leaked `State` per operation

`gix-filter/src/driver/mod.rs`, `impl Clone for State`, returns
`State { running: Default::default(), context: self.context.clone() }`. **Clones
start empty.** So every clone of a `Pipeline` is an *independent* child-owning
`State`, not an alias — and both hot paths clone per worker thread:

| Path | Clone site | Count |
|---|---|---|
| status, rename tracking | `gix-status/src/index_as_worktree_with_renames/mod.rs:133` *(vendored)* | 1 |
| status, tracked modifications | `…/mod.rs:143` *(vendored)*, moved into a spawned thread | 1 |
| status, per worker | `gix-status/src/index_as_worktree/function.rs:125` *(vendored)*, the `new_state` closure handed to `in_parallel_if` at `:147` | `thread_limit` |
| checkout, per worker | `gix-worktree-state/src/checkout/function.rs:111` *(vendored)* — `let ctx = ctx.clone()` | `thread_limit` |

So one `Repository::status()` abandons on the order of `N_threads + 2`
independent `State`s.

### Field evidence

A Tauri desktop sync app (`gix 0.86.0`, macOS 26.5, two repos with
`filter.lfs.process` registered) after **10 h 29 m** of uptime:

| | |
|---|---|
| Zombie children owned by the app | **274** |
| Live `process` filter helpers | 95 |
| Total processes on the machine | 1 218 |
| `kern.maxprocperuid` | 2 666 |

~26 zombies/hour is a four-day fuse to the point where nothing on the machine can
`fork`. Observed symptoms before the cause was found: the browser could not open
new tabs and app keyboard shortcuts stopped responding.

### Why a consumer cannot fix it themselves

`gix::Repository::status()` accepts no pipeline and returns none:

- `gix/src/status/mod.rs:99` — `pub fn status<P>(&self, progress: P) -> Result<Platform<'_, P>, Error>`.
  `Platform` has no field, builder or parameter for a `gix_filter::Pipeline`.
- `gix/src/status/index_worktree.rs:130` *(vendored)* — the sole owner is a local
  binding, `let resource_cache = crate::diff::resource_cache(...)`.
- `:150` *(vendored)* — moved into
  `gix_status::index_as_worktree_with_renames::Context { resource_cache, .. }`.
- `gix-status/src/index_as_worktree_with_renames/types.rs:41` — the returned
  `Outcome` does not carry it back. **This is the drop point.**
- `gix/src/status/iter/mod.rs:~127` *(vendored)* — all of this happens on a
  detached producer thread owning a thread-local `Repository` clone, so it is
  never visible to the `Iter` the user holds.

The one already-reapable path is `gix/src/repository/filter.rs:62`
(`filter_pipeline`) plus `gix/src/filter.rs:144` (`into_parts`) — a caller who
builds their *own* pipeline can call
`.into_parts().0.into_driver_state().shutdown(WaitForProcesses)`. That reaches
nothing that `status`, `checkout`, `diff` or `merge` created.

### A second, smaller bug in the same function

`shutdown.rs:19` propagates with `?` on `child.wait()`. The first child that
fails to be waited abandons **every remaining** child in the map. Worth fixing in
the same PR: collect errors, keep waiting.

---

## 2. Construction sites (production only, `main`)

| # | File | Line | Function | Disposition |
|---|---|---|---|---|
| 1 | `gix/src/diff.rs` | 240 | `resource_cache` (l.226) | returned, nested in `Platform.filter.worktree_filter` |
| 2 | `gix/src/filter.rs` | 135 | `filter::Pipeline::new` (l.134) | stored in `gix::filter::Pipeline.inner` (private) |
| 3 | `gix/src/config/cache/access.rs` | 347 | `checkout_options` (l.331) | stored in `checkout::Options.filters` (pub) |
| 4 | `gix/src/repository/merge.rs` | 48 | `merge_resource_cache` (l.22) | returned, nested in `Platform.filter.filter` |
| 5 | `gix/src/repository/worktree.rs` | 132 | `worktree_stream` (l.111) | created-consumed-dropped; swallowed by `from_tree`, **no accessor back out** |

Indirect: `gix/src/repository/filter.rs:62` (`filter_pipeline`) → site 2.

`gix/src/status/index_worktree.rs` constructs nothing — it obtains one from site 1.

Site 5 is the only one with no public reach to its pipeline, and is the hardest.

## 3. What is already public

**No ownership inversion is required.** The pipeline is publicly reachable at
every hop except site 5:

| Struct | File:line | Field | Visibility |
|---|---|---|---|
| `gix_diff::blob::Platform` | `gix-diff/src/blob/mod.rs:140` | `filter: Pipeline` | **pub**, by value |
| `gix_diff::blob::Pipeline` | `…:113` | `worktree_filter: gix_filter::Pipeline` | **pub**, by value |
| `gix_merge::blob::Platform` | `gix-merge/src/blob/mod.rs:151` | `filter: Pipeline` | **pub** |
| `gix_merge::blob::Pipeline` | `…:130` | `filter: gix_filter::Pipeline` | **pub** — note: spelled `filter`, not `worktree_filter` |
| `gix_status::index_as_worktree::Context` | `gix-status/src/index_as_worktree/types.rs:60` | `filter: gix_filter::Pipeline` | **pub**, by value |
| `gix_status::…_with_renames::Context` | `…/index_as_worktree_with_renames/types.rs:345` | `resource_cache: gix_diff::blob::Platform` | **pub**, by value |
| `gix_worktree_state::checkout::Options` | `gix-worktree-state/src/checkout/mod.rs:72` | `filters: gix_filter::Pipeline` | **pub**, by value |

What is missing is only that nothing *waits* and no `Outcome` hands it back.

Two constraints on any "return it in the `Outcome`" design:

- `index_as_worktree::Outcome` (`types.rs:67`) derives
  `Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd`. A `Pipeline` field
  would break all of them.
- Both `gix_diff::blob::Platform` and `…::Pipeline` derive `Clone`, so cloning a
  `Platform` silently forks an empty `driver::State`.

`gix-worktree-state/src/checkout/chunk.rs:182,201` already call
`ctx.filters.driver_state_mut()` in production — proof that a `&mut self`
shutdown slots into the existing shape with no ownership change.

---

## 4. The plan

### Step 0 — open a Discussion first

`CONTRIBUTING.md` asks for a GitHub Discussion before implementing anything over
~500 SLOC. Option A below is comfortably under; Options B/C are not. Open one
regardless: this touches a documented-behaviour boundary (the doc says the caller
must call `shutdown`), so the maintainer should choose the semantics.

State in the Discussion, and in the PR body, that an AI agent assisted —
`CONTRIBUTING.md` has an explicit **"Prevent agent impersonation"** rule.
`Assisted-by:` / `Co-authored-by:` trailers are welcome.

### Step 1 — the fix, three options

**Option A — `Drop for State` (recommended; smallest, fixes every path at once).**

- Add `fn shutdown_mut(&mut self, mode: Mode) -> Result<Vec<…>, io::Error>` that
  drains via `std::mem::take(&mut self.running)`; make the existing
  `shutdown(self, mode)` a thin wrapper over it, so no caller breaks.
- `impl Drop for State { fn drop(&mut self) { let _ = self.shutdown_mut(Mode::WaitForProcesses); } }`.
- Fix the `?`-on-first-error partial leak while you are in there.
- Correct the `running` field doc: the pipes closing is termination, not reaping.
- **Non-breaking.** Fixes status, checkout, diff, merge, `worktree_stream` and
  every future site, with no signature change anywhere.
- The objection to anticipate: a `Drop` that blocks in `wait()`. Mitigate by
  ordering — the child has already had its stdin closed, so `wait()` returns
  promptly — and say so in the doc comment. If the maintainer objects to any
  blocking in `Drop`, offer `Mode::WaitForProcesses` behind a
  `State::shutdown_on_drop(bool)` defaulting to on.

**Option B — reap at each call site.** Call `driver_state_mut().shutdown_mut(...)`
in `index_as_worktree`, `index_as_worktree_with_renames`, checkout's chunk
context, and the merge/diff platforms. Larger, must be repeated for every clone
site, and a future clone site silently reintroduces the leak. Only worth it if A
is rejected.

**Option C — stop cloning.** Make the per-thread `State` share one child pool
behind a `Mutex`, so there is one `State` to shut down. Best long-term shape,
biggest change, and it alters the concurrency story of the filter pool. Propose
in the Discussion, do not lead with it.

### Step 2 — the API gap that testability needs

`gix-filter/src/driver/process/mod.rs` — `Client.child: std::process::Child` is
**private**, and the only escape is `pub fn into_child(self) -> Child`
(`client.rs:~305`), which **consumes** the `Client`. A test cannot learn the pid
without destroying the thing under test.

Add, in the same `/// Lifecycle` block:

```rust
/// The process id of the filter this client talks to.
///
/// Enough to observe the child's lifetime without taking it: see
/// [`Self::into_child`] for the destructive form.
pub fn id(&self) -> u32 {
    self.child.id()
}
```

Additive, non-breaking, and it is what makes the regression test possible.

### Step 3 — tests

The library already has everything needed; **no new fixture program is required.**

- `gix-filter/tests/helpers/arrow.rs` is a real long-running `process` filter
  speaking the git protocol (`Server::handshake`, v2, `[clean, smudge, delay]`),
  looping `while let Some(request) = srv.next_request()?` — so it exits when its
  stdin closes. Exactly the "exits but is never reaped" condition.
- `gix-filter/tests/filter/driver.rs:71` — `driver_with_process()` returns the
  `Driver` naming it; `:526` — `driver_path()` resolves it via
  `env!("CARGO_BIN_EXE_gix-filter-test-arrow")`; `:22` — `extract_client()`
  already yields `&mut process::Client` from a `Process::MultiFile`.
- Home for the new tests: `mod shutdown` in the same file, which currently holds
  only `ignore_when_waiting()` (`:33`).

Add `libc` for the observation, matching the one existing precedent
(`gix-ref/tests/transaction_fd_limit.rs`, which uses `libc::setrlimit`). The
workspace `[workspace.dependencies]` table is empty, so declare it in
`gix-filter/Cargo.toml`:

```toml
[target.'cfg(unix)'.dev-dependencies]
libc = "0.2.186"
```

**Test 1 — `a_dropped_state_reaps_its_process_children` (the regression pin).**

```
launch the arrow `process` driver through State
pid = client.id()                       // needs Step 2
drop(state)                             // or shutdown_mut
// the child is gone AND was waited for: waitpid must not find a zombie
assert waitpid(pid, WNOHANG) == -1 && errno == ECHILD
```

`#[cfg(unix)]`. Mutation check: remove the `Drop` impl and this must fail with
`waitpid` returning `pid` (the zombie we just reaped) instead of `ECHILD`.

**Test 2 — `shutdown_waits_for_every_child_even_when_one_fails`.** Two drivers,
one of them the arrow's `next-invocation-returns-strange-status-…` trigger.
Assert both entries come back and both pids are reaped. This is the pin for the
`?`-on-first-error partial leak.

**Test 3 — `a_cloned_state_does_not_share_the_original_s_children`.** Not a fix,
a *documentation* test: it pins the surprising `Clone` semantics so a future
change to `Drop` or to cloning cannot silently regress the multiplication
described in §1.

**Test 4 (`gix-status`, optional but the one that proves the user-visible bug).**
Run `index_as_worktree` over a fixture whose attributes route files through the
arrow `process` driver, on a thread pool, then assert no zombies remain among the
pids observed. Heavier and platform-specific; offer it, let the maintainer decide.

Note: gix-filter's suite runs **twice** in CI, under `GIX_TEST_FIXTURE_HASH=sha1`
and `=sha256`. New tests must pass under both. `DEVELOPMENT.md` forbids `unwrap()`
**even in tests** — use `expect("why")` or return `crate::Result`.

### Step 4 — house style

| Rule | Source |
|---|---|
| MSRV **1.85**, edition 2024 | root `Cargo.toml:13`, `gix-filter/Cargo.toml` |
| Conventional commits, but **only** classify what belongs in the changelog; refrain from `chore:`/`refactor:` | `DEVELOPMENT.md` |
| Breaking = **`!` suffix on the type**, e.g. `fix(gix)!: …` (real example: `b76cc281`) | `DEVELOPMENT.md` |
| Crate **scope required** when a changelog-worthy commit touches paths outside that crate | `DEVELOPMENT.md` |
| **One self-contained commit**: a breaking change plus every workspace adaptation stays together and must pass CI alone | `DEVELOPMENT.md` |
| `CHANGELOG.md` is **generated** by `cargo smart-release` — never hand-edit; your commit message *is* the entry | `gix-filter/CHANGELOG.md`, `justfile` |
| Test-first: write the regression test before the fix | `DEVELOPMENT.md` |
| `thiserror` for errors, never `unwrap()` | `DEVELOPMENT.md` |

Before pushing:

```sh
just fmt      # nightly rustfmt + stable --check + just --fmt
just clippy   # 4 feature configurations
just test     # clippy check doc unit-tests doc-tests journey-tests… check-mode
```

`just fmt` requires a **nightly** toolchain (`cargo +nightly fmt --all --config-path
rustfmt-nightly.toml`). Reviewers are asked to run with `GIX_TEST_IGNORE_ARCHIVES=1`.

### Step 5 — suggested commit

Option A is not breaking, so no `!`:

```
fix(gix-filter): reap `process` filter children instead of orphaning them

`State::shutdown` was the only code that `wait`s a long-running filter
child, and nothing in the workspace called it. Closing a child's pipes
terminates it but does not reap it, so on Unix every `process` driver
gitoxide started became a zombie held by the parent until it exited.

`impl Clone for State` resets `running`, so each per-thread clone made by
`index_as_worktree`, `index_as_worktree_with_renames` and
`gix-worktree-state`'s checkout owned and abandoned its own children:
roughly `thread_limit + 2` per `status()` call. Reported from the field:
274 zombies after 10.5 hours in a desktop app, against a
`kern.maxprocperuid` of 2666.

`State` now reaps on drop, via a `&mut self` form of `shutdown` that the
existing by-value one delegates to, so no caller changes. `shutdown` also
stops abandoning the remaining children when one `wait` fails.

`process::Client::id()` is added so the reap can be asserted without
consuming the client.

Assisted-by: <agent>
```

### Step 6 — sequence

1. Fork, branch `fix/reap-process-filter-children`.
2. Open the Discussion; link this analysis.
3. `just nextest -p gix-filter` to get a green baseline (twice, both fixture hashes).
4. Add `libc` dev-dep, `Client::id()`, and **Test 1 — watch it fail.**
5. Add `shutdown_mut` + `Drop`; watch Test 1 pass. Mutate the `Drop` away and
   watch it fail again.
6. Tests 2 and 3; fix the `?`-on-first-error leak.
7. Correct the `running` field doc and the `State` doc comment.
8. `just fmt && just clippy && just test`.
9. One commit. PR referencing the Discussion, with the field numbers from §1 —
   maintainers act on measurements.

---

## Open questions for the maintainer

1. Is blocking in `Drop` acceptable, given the child's stdin is already closed?
   If not, is an opt-out (`shutdown_on_drop(false)`) the right shape?
2. Should `State: Clone` keep resetting `running`, or should cloning be removed
   in favour of a shared pool (Option C)? The current semantics are surprising
   and are what multiplies the leak.
3. `gix/src/repository/worktree.rs:132` (`worktree_stream`) is the one site with
   no public reach to its pipeline. `Drop` covers it; any call-site design does not.

## Not verified

- Exact `main` line numbers inside `gix-status` and `gix-worktree-state` clone
  sites (traced in vendored 0.33.0; the struct/field facts were re-checked on `main`).
- `gix/src/status/iter/mod.rs` thread-spawn line number (vendored).
- Whether `in_parallel_if` clones the state-factory closure or calls it N times —
  the outcome (one independent `State` per worker) is the same either way.
- The leak is established by ownership analysis plus the absence of any
  `shutdown` caller, and corroborated by the field measurement; it was not
  reproduced against a build of gitoxide itself.

---

## Resolution, as shipped in keeper (2026-08-27)

The fix is written, filed and **in use**: keeper's `gix` is pinned to
`tgorka/gitoxide`, branch `keeper/gix-filter-0.33-reap` — the two commits filed
as tgorka/gitoxide#1 (lifecycle tests, `process::Client::id()`) and #2
(`State::shutdown_mut` plus `impl Drop for State`), cherry-picked onto the
`gix-filter-v0.33.0` release tag. That tag is also where `gix` 0.86.0 and
`gix-quote` 0.7.2 were published, so the tree is the code crates.io serves plus
the `Drop`. See the long note on `gix` in `src-tauri/Cargo.toml` for why it is a
git source rather than a `[patch.crates-io]` entry on `gix-filter` alone.

### What the leak looked like, measured

The last build without the fix, on hesperia, 27 minutes after launch:

```
keeper pid 3300, app_age 27:43
  3424 ppid=3300 age=26:54   keeper lfs filter-process --repo /Volumes/merope/tgdrive
  3425 ppid=3300 age=26:54
  3426 ppid=3300 age=26:54
  3431 ppid=3300 age=26:53
```

Four helpers, each as old as the process that started them: launched in the
first seconds of the first walk and never let go. `STATUS_THREAD_LIMIT` is 4,
which is the `thread_limit` term in §1's arithmetic, visible directly.

### The same machine with the fix

`a_finished_status_walk_reaps_the_filter_children_it_launched`
(`crates/keeper-sync/tests/lfs_filter_process.rs`) is the mechanical guard. Run
on hesperia with one input changed between the two runs — the `gix` source:

```
### macOS, crates.io gix 0.86 (unpinned)
  the walk returned and left 1 unreaped child(ren) of its own: ["18555 [Z] <defunct>"]
  FAIL a_finished_status_walk_reaps_the_filter_children_it_launched

### macOS, fork pin restored
  PASS a_finished_status_walk_reaps_the_filter_children_it_launched
```

**The corpse is the shell, not the helper.** gitoxide runs a filter command
through `sh -c`, so the unreaped child of the walking process is the shell and
the helper itself is re-parented away. A check that looked only for the recorded
helper pid among its own children passed against the leak — worth knowing for
anyone reproducing this.

### And under real sync, A/B, on the machine that reported it

The unit guard is one `status_paths` call. This is the whole engine:
`keeper-syncd`, built twice from the same tree with only the `gix` source
changed, driving six sync rounds over a fixture in the shape tgdrive is in, with
the pass's children sampled every 20 ms (`/Users/tgorka/reap-ab.sh` on
hesperia).

| binary | `shutdown_mut` in it | walks | zombie sightings |
| --- | --- | --- | --- |
| crates.io `gix 0.86` | 0 | 120 | **1245** (192-225 per round) |
| fork pin | 2 | 150 | **0** |

**The fixture is the hard part, and two obvious ones do not work.** Committing
LFS files and then `touch`ing them proves nothing: keeper's own
`refresh_index_stat` answers the stat drift before any content comparison, and
`stage_and_commit` writes pointer bytes keeper computed itself (AD-46), so gix's
filter is never asked — measured, 83 walks and 58 commits with zero driver
launches. Changing the file bodies does not help either, for the same reason.
What does work is stripping the stat data from the index entry
(`git update-index --cacheinfo 100644,<pointer-oid>,<path>`) before each pass:
the walk then cannot take the shortcut and must clean-filter the worktree file
to find out whether it matches.

**On macOS a reaped-but-unwaited child shows no command**, only `<defunct>`, so
attribute by parent pid and state rather than by matching `lfs filter-process`
in the command column — which is why the per-round helper counts above read 0 on
both sides while the zombie counts differ by 1245.

### And in the shipped app

`shutdown_mut` is present in the installed binary (2 occurrences) and absent
from the previous bundle (0), with `lfs filter-process` at 1 in both as the
control that the search method works.

The app-level before/after is the helper *age*: without the fix its helpers were
as old as the process (26:54 against an app age of 27:43); with the fix the only
helpers alive are the four belonging to the tgdrive walk currently in flight,
with zero zombies over 1h37m. A tgdrive pass takes 66-72 minutes
(`elapsed_ms=3992162`, `4330534`), so the release of those four is observable
only when it ends; `/tmp/keeper-reap-watch.log` and `/tmp/tgdrive-pass.log` on
hesperia record that transition. The A/B above is what settles the mechanism
without waiting for it.

Retire the pin when the fix is released upstream: restore a version requirement
in `src-tauri/Cargo.toml`, drop the `allow-git` line in `deny.toml`, and re-run
the guard.
