---
title: 'Find a git that works, not the first file called git'
type: 'bugfix'
created: '2026-07-29'
status: 'review'
baseline_revision: '1be95be'
---

<intent-contract>

## Intent

**Problem:** `SyncPlatform::git_program` answered with the first executable named `git` on `PATH`.
`Engine::open` then probed that binary and refused to open below its floor
(`MIN_GIT_MAJOR`/`MIN_GIT_MINOR` — 2.42). Two independent consumers, one asking "is there a file
called git" and the other asking "is there a git I can drive", and nothing reconciled them.

The visible consequence was a surface built over an engine that had already declined to serve it.
The `sync` capability flag was computed from the weaker question, so on a machine whose first `PATH`
git was 2.23 — a stock `/usr/local/bin/git`, a system git ahead of a Homebrew one — the Settings
Sync section and the whole Sync view rendered, offered to add folders, and sat over an engine that
would not open. The refusal was logged at `debug`, which is opt-in, so nobody saw it. AD-41 makes
git a declared hard prerequisite precisely so this is decided once and early; instead it was decided
twice, differently.

The second-order problem is diagnostic. A developer machine routinely has two or three gits — Xcode's,
Homebrew's, one from a package manager — and "no usable git found" is not an answer when the user can
type `git --version` and see 2.52. Neither is "using /opt/homebrew/bin/git" on the same machine: what
the user needs to know is which candidates were tried, which was rejected, and why. Nothing recorded
that, because nothing had looked at more than one.

**Approach:** Resolve by probing. `git::resolve` walks the candidates — an explicitly configured path
first, then `PATH` — runs `git --version` on each, and returns the one that clears the floor along
with a `GitRejection` for every one that did not, carrying which of the four failure kinds it was:
absent, not executable, unusable, or too old. Four rather than one boolean because they need four
different sentences — "nothing is there", "not an executable file", git's own stderr, and a version
below the floor are four different next steps for the person reading them. The app caches the
resolution per process and invalidates it when the setting changes; the daemon deliberately does not
cache (its own `git_resolution` doc says so), because a `doctor` run is a one-shot process.

`Engine::open` and the capability flag then read the *same* resolution, so they can no longer
disagree. `keeper-syncd doctor` reads it too, rather than resolving and then probing again, and its
OK line names what it skipped. An explicit path is stored per installation in the settings table
(`sync.git_path`), and honoured verbatim — including when it is rejected.

## Boundaries & Constraints

**Always:** the resolution probes; it never infers usability from a filename or from a path's
existence. The floor is `git::cli::MIN_GIT_MAJOR`/`MIN_GIT_MINOR`, read from one place. An explicit
path is tried first and, if it fails, is reported as a rejection rather than skipped — the setting
was an instruction, not a hint. `keeper-core` performs no validation: reading the setting must not
spawn a subprocess, so whether the path is a git that clears the floor is answered by probing it,
which that crate does not do.

**Block If:** (none) — every outcome is a report. There is no state in which resolution refuses to
answer; the answer may be "nothing usable, and here is each thing I tried".

**Never:** do not fall back to automatic resolution when an explicit path fails. A silent fallback
would make the setting untrustworthy: the user would have configured one binary and be running
another, with no way to tell. Do not resolve twice *within one answer* — one report, one probe per
candidate, so a single surface cannot quote two different versions of the same binary. (This rule is
narrower than it first read, and deliberately so: `Engine::open` still re-probes the program the
resolution chose, and `doctor` therefore resolves for its git check and again through
`Engine::open`. Those are separate answers rather than one self-contradicting answer, and closing
that redundancy is filed as deferred work because it crosses the `SyncPlatform` trait.) Do not make
this a per-profile setting.

## I/O & Edge-Case Matrix

| condition | `GitResolution` | `SyncGitState` | `capabilities.sync` |
| --- | --- | --- | --- |
| a `PATH` git clears the floor | chosen, with rejections for anything tried before it | `ok` | `true` |
| a broken git precedes a good one | the good one is chosen; the broken one is a rejection | `ok` | `true` |
| a too-old git precedes a good one | the good one is chosen; the old one is a rejection | `ok` | `true` |
| every candidate is below the floor | nothing chosen; each listed with its version | `tooOld` | `false` |
| every candidate exists and cannot run | nothing chosen; each listed | `unusable` | `false` |
| no candidate exists at all | nothing chosen | `missing` | `false` |
| a `PATH` directory contains no `git` | not a rejection — nothing was there to reject | — | — |
| a `PATH` entry holds a non-executable `git` | that candidate is a rejection (`NotExecutable`); a later usable one still wins | `ok` if one does, else `unusable` | `true` / `false` accordingly |
| the only `git` found is non-executable — a stray note, a directory | nothing chosen; reported as "not an executable file" | `unusable` | `false` |
| an explicit path clears the floor | chosen, and `is_explicit()` | `ok` | `true` |
| an explicit path is below the floor | **refuses**; does not fall back to `PATH` | `tooOld` | `false` |
| an explicit path is absent | reported as a rejection, not silently skipped | `missing` | `false` |
| the setting is empty or whitespace | treated as unset; `PATH` is searched | — | — |
| a vendor-suffixed version (`2.52.0 (Apple Git-…)`) | parsed and accepted | `ok` | `true` |
| this build has no folder sync (iOS) | not resolved at all | `unsupported` | `false` |

</intent-contract>

## Code Map

**`keeper-sync/src/git/resolve.rs`** (new) — `GitRequest`, `GitResolution` (`chosen`, `rejected`,
`program`, `is_explicit`, `summary`, `problem`), `GitChoice`, `GitOrigin`, `GitReject` (the four
failure kinds — `Absent`, `NotExecutable`, `Unusable { detail }`, `TooOld { major, minor }`) and
`GitRejection`. `Absent` is recorded only for a *named* path: a `PATH` directory that simply holds no
`git` is not a rejection, or the report would list every directory on `PATH`. The `summary`/`problem`
prose is composed here so every surface renders the same sentence.

**`keeper-sync/src/git/cli.rs`** — `MIN_GIT_MAJOR`/`MIN_GIT_MINOR` and `probe`, including the
vendor-suffix tolerance.

**`keeper-sync/src/platform.rs`** — `git_program`'s doc contract, rewritten: an implementation must
answer with a binary that clears the floor, which means probing candidates rather than taking the
first file named `git`, and a host that returns a too-old one has not degraded sync — it has built a
surface the engine will not serve.

**`keeper-core/src/registry.rs`** — `SYNC_GIT_PATH_KEY` (`sync.git_path`), `get_sync_git_path`
(empty/whitespace reads as unset) and `set_sync_git_path`, in the same k/v table
`recording.destination_dir` uses and for the same reason: a user-chosen absolute path whose validity
is a runtime fact this crate must not probe.

**`keeper/src/sync.rs`** — `git_resolution` (the per-process cache, seeded from the setting) and
`invalidate_git_resolution`. The cache is the app's alone; `keeper-syncd::platform::git_resolution`
documents why the daemon has none.

**`keeper/src/ipc.rs`** — `SyncGitState`, `SyncGitVm`, `git_report` (deliberately engine-free: it has
to answer on the machines where the engine will not open), the `sync_git_status` /
`sync_git_path_set` commands, and the `sync:` capability now derived from the same resolution.
`git_report` collapses two of the four rejection kinds onto one state —
`matches!(r.cause, GitReject::Unusable { .. } | GitReject::NotExecutable)` → `SyncGitState::Unusable`
— which is why the fourth kind has to be named here rather than left to a wildcard: a `NotExecutable`
that fell through to a catch-all would report as `missing`, and "install git" is the wrong
instruction for a machine that has one shadowed by a file of the same name.

**`keeper-syncd/src/{commands,config,main,platform}.rs`** — `doctor`'s git check reads the daemon's
own resolution rather than asking for a program and probing it again, and the daemon's platform
honours the same explicit-path setting (normalising `gitPath = ""` to unset in `with_git_path`, so the
daemon and the app cannot disagree about what "cleared" means).

## Tasks & Acceptance

**Execution:**

1. Add `git::resolve`, probing candidates and recording every rejection with its kind.
2. Rewrite `git_program`'s contract and every implementation to answer with a *usable* git.
3. Store an explicit path per installation in `keeper-core`'s settings table.
4. Derive the `sync` capability from the same resolution the engine uses.
5. Project the report and the setter over IPC.
6. Make `doctor` read one resolution and name what it skipped.

**Acceptance Criteria:**

1. On a machine whose first `PATH` git is below the floor and a later one clears it, the later one is
   used and sync works.
2. On a machine with no usable git, `capabilities.sync` is `false` and the Sync surface does not
   render — no section whose every button would reject.
3. The report names every candidate tried and why each was rejected, in one sentence per candidate.
4. An explicitly configured path below the floor refuses and does **not** fall back.
5. `doctor`'s git line names the binary and version **its own** resolution chose, from that
   resolution's probe rather than a second `git --version` of its own. It does not yet share one
   probe with the engine: `Engine::open` re-probes the program it is handed, so a `doctor` run
   spawns `git --version` for the git check and again inside `check_engine`. On a box where the
   binary is being replaced mid-run those two can disagree, which is the residual gap this criterion
   used to overstate away; it is filed as deferred work.
6. Clearing the setting returns to searching `PATH`, in the same process.

## Design Notes

**Why per installation and not per profile.** Every profile shares one `Engine`, and `Engine::open`
resolves git once and holds a single `GitCli`. A per-profile knob would offer a choice the engine has
no way to honour — two profiles naming different binaries would silently both get whichever opened
the engine. The setting lives beside `recording.destination_dir` in the same key/value table, which
is where installation-wide user-chosen paths already live.

**Why `keeper-core` validates nothing.** `get_sync_git_path` returns the string and filters only
emptiness. Validating would mean spawning `git --version` from the crate that must stay free of both
tauri and the sync engine, and the answer would be stale by the time anyone used it. This mirrors
`get_recording_destination_dir`, which likewise does not check that the directory exists: whether a
configured path is usable is a runtime fact, answered where the probe lives.

**Why `doctor`'s own git check reads one resolution rather than resolving and then probing.** The old
shape asked the platform for a program and then ran `probe` on it. That is two `git --version`
invocations answering the same question, and on a machine where the binary is being replaced under us
— a Homebrew upgrade mid-run — the second can report a different version than the one the engine will
drive. Reading the resolution's own probe result means `doctor`'s git *line* cannot describe a binary
other than the one that line resolved.

**What that does not achieve, stated plainly.** It does not reduce a `doctor` run to one probe.
`check_git` resolves, and `check_engine` opens an `Engine`, which asks `SyncPlatform::git_program` for
a program — resolution number two — and then calls `git.capabilities()`, spawning `git --version` a
third time on a binary the resolution had already probed. `GitChoice` carries the
`GitCapabilities` that would make that redundant, and nothing hands it forward: the trait method
returns a `PathBuf`, so the capabilities die at the platform boundary. The consequence is small and
real — an extra process per engine open, and a second TOCTOU window where the binary can change
between the resolution and the probe — and the fix is a `SyncPlatform` signature change, which is why
it is deferred work rather than part of this story.

**Why the OK line names what was rejected.** "Using /opt/homebrew/bin/git" is not an answer on a box
with three gits; it leaves an operator wondering why their shell's `git --version` disagrees with the
daemon's. Naming the skipped candidates and their versions turns a puzzle into a fact, and it is the
same information the failure path needs, so it is composed once in `resolve` rather than twice at the
two call sites.

**Why an explicit path does not fall back.** The tempting behaviour on a bad configured path is to
search `PATH` anyway, so sync keeps working. That trades a loud, fixable misconfiguration for a
silent, invisible one: the user believes they pinned a binary and is running a different one, and
nothing on any surface says so. Refusing — and reporting the configured path as the thing that was
rejected — keeps the setting meaning what it says. The report also keeps the value in
`configured_path` so the field can show it, rather than clearing itself.

**Why the capability is derived rather than probed separately.** This is the defect, stated as a
rule. Two questions with two answers is how the surface came to exist over an engine that had
refused; one resolution, read by both, makes the disagreement unrepresentable.

**Why the resolution is cached, and what invalidates it.** Probing every candidate costs a subprocess
per candidate, and `capabilities` is called on every settings open. The cache is per process, and
`invalidate_git_resolution` is called when the setting is written — without that, a path change would
be reported against the previous binary until restart, which is exactly the kind of stale answer this
story exists to remove.

## Verification

**Owed first, and it was the significant gap.** At the time this story's own commit shipped **no
frontend consumed `sync_git_status` or `sync_git_path_set`**. Both were implemented, registered in the
`invoke_handler`, typed through ts-rs, and called by nothing — a repo-wide grep for `syncGit` outside
`src/lib/ipc/gen/` found no hits. So the report that names every rejected candidate was computed and
discarded, and the setting that would fix a too-old git could only be written by a caller that did
not exist. That was logged as **DW-122** and closed by the follow-up commit `5e9720e`, which added
`src/components/settings/sync-git-row.tsx` and mounted it in `settings-dialog.tsx` — and which also
found that DW-122's own suggested placement, inside Settings → Sync, was unreachable, because that
section is gated on the very capability the report explains. The row therefore sits *beside* that
gate, and the dialog suite asserts exactly that: the report renders with the capability off while the
Sync section does not. Nothing in this story's own commit surfaced it.

**Unit tests, `keeper-sync/src/git/resolve.rs`** (13 tests), covering the matrix above directly:

- `a_broken_git_ahead_of_a_good_one_does_not_win` and `a_too_old_git_ahead_of_a_good_one_does_not_win`
  — the two orderings that produced the original defect.
- `the_three_ways_a_git_can_fail_are_reported_distinctly` — misnamed by one: it asserts all **four**
  kinds, `Absent`, `NotExecutable`, `TooOld` and `Unusable` (the last checked for carrying git's own
  "bad config line 44" rather than a flattened "exited non-zero"). The name is left alone rather than
  churned, and recorded here so a reader counting kinds from it is not misled.
- `a_path_directory_without_git_is_not_reported_as_a_rejection` — nothing was there to reject, so the
  report is not padded with every `PATH` entry.
- `an_explicit_path_below_the_floor_refuses_and_does_not_fall_back` and
  `an_explicit_path_that_is_not_there_is_reported_rather_than_skipped` — the no-fallback rule.
- `an_empty_search_says_there_is_no_git_and_how_to_get_one` and
  `a_search_that_found_only_unusable_gits_lists_each_one` — the prose.
- `a_chosen_git_is_summarized_the_way_doctor_reports_it` — one spelling across surfaces.
- `a_vendor_suffixed_version_is_accepted` — both vendor shapes, `2.50.1 (Apple Git-155)` and
  `2.45.1.windows.1`, end to end through a resolution rather than through the parser alone.
- `a_binary_that_prints_a_version_but_is_not_git_does_not_win` — a candidate whose `--version` prints
  `Python 3.13.0` is `Unusable` and the real git still wins; no resolution ever reports `git 3.13`.
  Version-shaped output from something that is not git is exactly how a shadowed `PATH` produces a
  confident wrong answer.
- `a_relative_candidate_is_resolved_once_against_the_working_directory` — a relative candidate is
  pinned to an absolute path *before* probing, so `summary()` and `program()` name the file that will
  actually be executed when git runs under `current_dir(repo)`. Probing one file and executing
  another is the whole class of defect this module exists to close, one level down.
- `probing_stops_at_the_first_git_that_clears_the_floor` — the cost bound.

**`keeper-sync/src/git/cli.rs`** — `a_version_from_something_that_is_not_git_is_not_a_git_version`:
`parse_version` requires git's own `git version` wording on line 1 before it scans for numbers, and
the rejection detail says "which is not git reporting its version" rather than "which is not a
version", because the two send a reader to different places.

**`keeper/src/sync.rs`** has tests driving `git_resolution` against `fake_git` fixtures: setting a
2.23 path, clearing it, and setting a 2.52 path, each with `invalidate_git_resolution`, asserting the
chosen program and `is_explicit()` change within one process. And
`repointing_at_another_git_rebuilds_the_engine_and_re_arms_background_sync`, which is the one that
makes the setting mean anything at runtime: pinning a bad path leaves no engine and no supervisor,
repairing it builds both, pinning a *different* good git yields an engine that is not
`Arc::ptr_eq` to the first, and breaking it again empties both slots. Without `reset_engine` the
third leg fails — which was the live defect: the report and `capabilities.sync` changed while every
push, merge and worktree call kept driving the previous binary.

**Not covered, explicitly:**

- No test asserts that `capabilities.sync` and `Engine::open` agree. They read the same *setting* and
  the same resolver by construction, which is the fix. They do not share one probe: `Engine::open`
  re-probes the chosen program, so the second answer is merely extremely likely to match the first,
  and nothing fails if a future change makes it a third question.
- `sync_git_path_set` is exercised end to end since `5e9720e`: `sync-git-row.test.tsx` drives the
  field and the dialog suite asserts the row renders beside the capability gate. The engine now
  follows the setting too (`repoint_engine`, covered above), with two bounds that are not covered:
  the teardown only *signals* the outgoing supervisor, so it finishes its current unit and can
  briefly run beside the new loop — safe by `db::claim_ready`'s single-`UPDATE` claim, the same
  property that lets the app and `keeper-syncd` coexist, and a unit the outgoing loop returns to the
  queue is re-driven as the ordinary "interrupted, so repeat it" path. Blocking the IPC call until
  the old loop joined was rejected: minutes of frozen Settings during a large push, to close a
  window the journal already covers. And nothing re-points the engine when the pinned binary is
  moved or deleted *outside* the app — a success is cached for the life of the process by design,
  and the next real git call fails with git's own diagnostic.
- No machine with a genuinely too-old git was used. The rejections are exercised through `fake_git`
  shell fixtures that print a chosen version string, not against a real 2.23 build.
- The whole workspace passed `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo test --workspace` on macOS/arm64 — **as this story originally shipped.** The corrections
  made during the epic-34 review (2026-07-30) added tests and narrowed claims after that run; the
  workspace has not been re-run green as one command since, and saying so is cheaper than implying
  it.
