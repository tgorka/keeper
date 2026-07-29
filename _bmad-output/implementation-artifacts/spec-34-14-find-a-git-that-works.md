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
with a `GitRejection` for every one that did not, carrying which of the three failure kinds it was:
absent, unusable, or too old. The resolution is computed once per process and cached, invalidated
when the setting changes.

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
another, with no way to tell. Do not resolve twice in one answer — a second `git --version` could
report a different version than the one the engine will drive, on a machine where the binary is being
replaced. Do not make this a per-profile setting.

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
| an explicit path clears the floor | chosen, and `is_explicit()` | `ok` | `true` |
| an explicit path is below the floor | **refuses**; does not fall back to `PATH` | `tooOld` | `false` |
| an explicit path is absent | reported as a rejection, not silently skipped | `missing` | `false` |
| the setting is empty or whitespace | treated as unset; `PATH` is searched | — | — |
| a vendor-suffixed version (`2.52.0 (Apple Git-…)`) | parsed and accepted | `ok` | `true` |
| this build has no folder sync (iOS) | not resolved at all | `unsupported` | `false` |

</intent-contract>

## Code Map

**`keeper-sync/src/git/resolve.rs`** (new) — `GitRequest`, `GitResolution` (`chosen`, `rejected`,
`program`, `is_explicit`, `summary`, `problem`), `GitChoice`, `GitOrigin`, `GitReject` (the three
failure kinds) and `GitRejection`. The `summary`/`problem` prose is composed here so every surface
renders the same sentence.

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
`invalidate_git_resolution`.

**`keeper/src/ipc.rs`** — `SyncGitState`, `SyncGitVm`, `git_report` (deliberately engine-free: it has
to answer on the machines where the engine will not open), the `sync_git_status` /
`sync_git_path_set` commands, and the `sync:` capability now derived from the same resolution.

**`keeper-syncd/src/{commands,config,main,platform}.rs`** — `doctor`'s git check reads one
resolution, and the daemon's platform honours the same explicit-path setting.

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
5. `doctor` reports the same binary and version the engine will drive, from one probe.
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

**Why `doctor` reads one resolution rather than resolving and then probing.** The old shape asked the
platform for a program and then ran `probe` on it. That is two `git --version` invocations answering
the same question, and on a machine where the binary is being replaced under us — a Homebrew upgrade
mid-run — the second can report a different version than the one the engine will drive. Reading the
resolution's own probe result means `doctor` cannot describe a binary other than the one in force.

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

**Owed first, and it is the significant gap.** At the time this story shipped **no frontend consumed
`sync_git_status` or `sync_git_path_set`**. Both were implemented, registered in the `invoke_handler`,
typed through ts-rs, and called by nothing — a repo-wide grep for `syncGit` outside
`src/lib/ipc/gen/` found no hits. So the report that names every rejected candidate was computed and
discarded, and the setting that would fix a too-old git could only be written by a caller that did
not exist. That was logged as **DW-122** and closed afterwards by a follow-up commit
(`5e9720e`), which also found that DW-122's own suggested placement — inside Settings → Sync — was
unreachable, because that section is gated on the very capability the report explains. Nothing in
this story's own commit surfaced it.

**Unit tests, `keeper-sync/src/git/resolve.rs`** (11 tests), covering the matrix above directly:

- `a_broken_git_ahead_of_a_good_one_does_not_win` and `a_too_old_git_ahead_of_a_good_one_does_not_win`
  — the two orderings that produced the original defect.
- `the_three_ways_a_git_can_fail_are_reported_distinctly` — absent, unusable, too old.
- `a_path_directory_without_git_is_not_reported_as_a_rejection` — nothing was there to reject, so the
  report is not padded with every `PATH` entry.
- `an_explicit_path_below_the_floor_refuses_and_does_not_fall_back` and
  `an_explicit_path_that_is_not_there_is_reported_rather_than_skipped` — the no-fallback rule.
- `an_empty_search_says_there_is_no_git_and_how_to_get_one` and
  `a_search_that_found_only_unusable_gits_lists_each_one` — the prose.
- `a_chosen_git_is_summarized_the_way_doctor_reports_it` — one spelling across surfaces.
- `a_vendor_suffixed_version_is_accepted` — Apple Git.
- `probing_stops_at_the_first_git_that_clears_the_floor` — the cost bound.

**`keeper/src/sync.rs`** has tests driving `git_resolution` against `fake_git` fixtures: setting a
2.23 path, clearing it, and setting a 2.52 path, each with `invalidate_git_resolution`, asserting the
chosen program and `is_explicit()` change within one process.

**Not covered, explicitly:**

- No test asserts that `capabilities.sync` and `Engine::open` agree. They read the same resolution by
  construction, which is the fix, but nothing fails if a future change reintroduces a second probe.
- Nothing exercises `sync_git_path_set` end to end at this commit; it had no caller.
- No machine with a genuinely too-old git was used. The rejections are exercised through `fake_git`
  shell fixtures that print a chosen version string, not against a real 2.23 build.
- The whole workspace passed `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo test --workspace` on macOS/arm64.
