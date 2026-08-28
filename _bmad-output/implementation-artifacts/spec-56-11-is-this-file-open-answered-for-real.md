---
title: 'Is this file open, answered for real'
type: 'feature'
created: '2026-08-28'
status: 'done' # draft | ready-for-dev | in-progress | in-review | done | blocked
baseline_revision: 'fc013af'
review_loop_iteration: 0
followup_review_recommended: true
context: []
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** `SyncPlatform::open_file_state` ships as a *provided* method whose body is
`OpenFileState::Unknown`, no platform overrides it, and AD-125 turns `Unknown` into a refusal — so
`keeper-syncd dehydrate`, the app's Release action and the 56.5 TTL sweep all refuse `OpenUnknown`
on **every** real host, and Epic 56's release half has never once run. The default was the right
decision; the implementation behind it is what is missing.

**Approach:** One shared probe in `keeper-sync::platform` — `probe_open_file_state` — answered on
Linux from `/proc/<pid>/fd`, by **device + inode identity** rather than by comparing readlink
strings, and answering `Unknown` for every case the walk cannot see. `LinuxPlatform` (the daemon)
and `ShellSyncPlatform` (the app) both override the trait method by delegating to it; the trait
default stays `Unknown`, so `TestPlatform`, iOS (which never links `keeper-sync`) and any future
implementor keep refusing until they opt in. macOS gets **no** implementation this story and stays
`Unknown` — argued, not skipped, in Design Notes.

## Boundaries & Constraints

**Always:**
- **Nothing may answer `Closed` without evidence** (NFR-40, AD-125). Every failure of the probe —
  no procfs, an unreadable `/proc`, a target that cannot be stat'ed, a process whose `/proc/<pid>`
  cannot be entered at all, a procfs in which this very process is invisible — resolves to
  `Unknown`, which refuses. `Open` is returned on the first match and is definitive.
- **Identity, not paths.** A match is `st_dev` + `st_ino` of `stat("/proc/<pid>/fd/<n>")`, which
  procfs resolves through the magic link to the *open file's* inode. That is what makes the
  ` (deleted)` readlink suffix, bind mounts, a differing mount namespace's path prefix, hardlinks
  and `..`/symlink spellings all non-issues, and it is why no `canonicalize` is needed.
- **The trait default stays `Unknown`.** Only the two shipping platforms override it. `TestPlatform`
  keeps its injectable answer; every existing test keeps its meaning.
- **No new dependency.** `std` only — no `libc`, no `nix`, no `rustix`, no `procfs` crate — so
  `unsafe_code = "deny"` holds unchanged in every crate touched, and the three dependency-firewall
  scripts and the cargo-deny licence firewall have nothing new to judge.
- **No cache.** 56.4 places this guard last precisely because it is the answer most likely to have
  changed while the steps above ran; a per-pass cache would move it back in time by a whole pass.
- One `SyncError` vocabulary, one refusal set: this story adds no variant, no VM type, no ts-rs
  binding, no config field, no CLI flag.

**Block If:**
- A tested macOS implementation is judged mandatory. It cannot be produced here: the `keeper` shell
  crate does not compile on this host at all, no macOS target/SDK exists on it, and no macOS runtime
  is reachable — so any macOS answer would ship with zero evidence. Recorded as deferred work with
  its shape, not hidden.

**Never:**
- Not `lsof`, on macOS or anywhere: a process spawn per candidate, absent on minimal systems, and
  refused by AD-125 by name.
- Not `libproc`: its build tree carries an unconditional `bindgen` **build** dependency (measured
  from the crates.io index: `libproc` 0.14.11 → `bindgen ^0.72.1`), i.e. a hard libclang
  requirement and ~30 lock entries for one function on a platform this run cannot test.
- Not hand-written Apple `proc_pidfdinfo` FFI under `#[allow(unsafe_code)]` in the shell crate. See
  Design Notes for why the audited-exception precedent points the other way.
- Not a new `SyncPlatform` method, not a capability probe, not a change to the guard order in
  `release_resolved`, not a change to `docs/sync.md`'s five-refusal vocabulary. Not the deferred
  `OpenUnknown`-declines-before-hashing optimisation (DW entry from 56.5) — that is a different
  change and it is now moot on Linux.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| This test holds it open | a `File` on the target is alive | `Open`; `dehydrate_entry` refuses `ContentRefusal::Open`; content intact | `SyncError::Refused` |
| The handle is dropped | same path, handle dropped | `Closed`; the release proceeds and publishes the committed blob | No error expected |
| Nobody holds it | a freshly written file | `Closed`, so the refusal lifts | No error expected |
| An unrelated process holds a *different* file | same size, same name elsewhere | `Closed` — identity is dev+ino, not a name | No error expected |
| No procfs | probe root does not exist | `Unknown` → refusal `OpenUnknown` | `SyncError::Refused` |
| A procfs that shows nothing | probe root is an empty directory (this process is not in it) | `Unknown` — "no visible opener" over an empty visible set is vacuous | `SyncError::Refused` |
| A process that cannot be entered | a pid directory with mode `000` (the `hidepid=1` shape) | `Unknown` — a process keeper cannot even identify is a blind spot | `SyncError::Refused` |
| Another uid's descriptor table | `/proc/<pid>/fd` is `EACCES` but `/proc/<pid>/stat` reads | scan continues; claim narrowed and documented | not an error |
| A process exits mid-walk | `/proc/<pid>/fd` answers `ENOENT` | ignored — a process that is gone holds nothing | not an error |
| Target cannot be stat'ed | EACCES/ELOOP on the target | `Unknown` | `SyncError::Refused` |
| macOS / Windows / iOS | any path | `Unknown`, unchanged | `SyncError::Refused` |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/platform.rs` -- `OpenFileState` `:25`; the provided
  `open_file_state` `:134` with its fail-closed doc `:92-133` (the "why no platform answers it yet"
  paragraph is now false and must be rewritten, not deleted); `TestPlatform`
  `:242`/`set_open_file_state` `:315`; `machine_utc_offset_minutes` `:186` as the shape for a
  module-level helper beside the trait. **Gains** `probe_open_file_state` plus the Linux walk and
  its unit tests.
- `src-tauri/crates/keeper-sync/src/lib.rs` -- `pub use platform::SyncPlatform;` `:78`; gains
  `OpenFileState` so an implementor in a sibling crate needs one import.
- `src-tauri/crates/keeper-syncd/src/platform.rs` -- `impl SyncPlatform for LinuxPlatform` `:324`;
  `free_space` `:423` is the neighbouring "this crate carries no `libc`" note to stay consistent
  with. **Gains** the override.
- `src-tauri/crates/keeper/src/sync.rs` -- `impl SyncPlatform for ShellSyncPlatform` `:51`;
  `utc_offset_minutes` `:114` is the precedent for overriding a *provided* method here and saying
  why. **Gains** the override. Cannot be compiled on this host — macOS gate.
- `src-tauri/crates/keeper-sync/src/engine.rs` -- `release_resolved`'s `open_file_state` match
  `:7892-7902`; its guard-order doc `:7723-7724`. The comment "the trait default, and therefore the
  answer on every real host today" is now wrong. **No logic change.**
- `src-tauri/crates/keeper-sync/tests/dehydrate_entry.rs` -- the real-repository harness
  (`seed` `:136`, `engine_with` `:202`, `stamp_index_forward` `:183`); the two existing platform-injected
  refusal tests `:474`, `:510`. **Gains** the end-to-end tests driven by the *real* probe.
- `docs/sync.md` -- `:816-817` the refusal table, `:848-853` the "no shipping platform overrides it"
  paragraph. Both are now false for Linux.
- `_bmad-output/implementation-artifacts/deferred-work.md` -- the macOS entry, and the 56.5 entry at
  `:3575-3578` whose premise this story removes on Linux.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-sync/src/platform.rs` -- add `pub fn probe_open_file_state(path: &Path) -> OpenFileState`, `#[cfg(target_os = "linux")]`-delegating to a private `open_file_state_under_proc(proc_root: &Path, path: &Path)` and answering `Unknown` on every other target. Rewrite `open_file_state`'s "why no platform answers it yet" section into "what each platform answers, and why the default is still `Unknown`". Keep the fail-closed/`free_space` polarity paragraph verbatim.
- [x] `src-tauri/crates/keeper-sync/src/lib.rs` -- re-export `OpenFileState` beside `SyncPlatform`.
- [x] `src-tauri/crates/keeper-syncd/src/platform.rs` -- override `open_file_state` by delegating to the probe, documenting that the daemon is Linux-first (AD-52) so this is the platform that answers.
- [x] `src-tauri/crates/keeper/src/sync.rs` -- override `open_file_state` by delegating to the same probe, documenting that the app is the host on macOS and gets `Unknown` there, and that the delegation is what makes a Linux desktop app's Release button work.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- correct the two stale comments at the `open_file_state` match and in the guard-order doc. No behaviour change.
- [x] `src-tauri/crates/keeper-sync/src/platform.rs` (tests) -- unit tests over the **real** `/proc` and a real descriptor: a file this test holds open is `Open`; the same path after the drop is `Closed`; a file nobody opened is `Closed`; a second file with identical content is not confused with the first. Over a real *probe root* that is not procfs: absent root → `Unknown`; empty root → `Unknown`; a pid directory chmod `000` → `Unknown`. All `#[cfg(target_os = "linux")]`.
- [x] `src-tauri/crates/keeper-sync/tests/dehydrate_entry.rs` -- add a `RealOpenPlatform` wrapper (delegates every `SyncPlatform` method to `TestPlatform`, but answers `open_file_state` from the real probe) and two end-to-end tests through `Engine::dehydrate_entry`: with a live descriptor the release refuses `Open` and the bytes are intact; with no descriptor the release *succeeds*, publishes the committed blob and leaves `git status --porcelain` empty.
- [x] `docs/sync.md` -- correct the platform paragraph: Linux answers from `/proc` by inode identity, with the one stated narrowing; macOS/Windows still refuse; say what a release now does and does not do on each.
- [x] `_bmad-output/implementation-artifacts/deferred-work.md` -- log the macOS gap with its shape and the two rejected options; log the narrowed-claim limitation; note that 56.5's `OpenUnknown`-before-hashing entry is now moot on Linux.

**Acceptance Criteria:**
- Given a materialized LFS path in a real repository with a real filesystem remote holding the object, when a `std::fs::File` on that path is alive and `Engine::dehydrate_entry` is asked through a platform using the **real** probe, then it refuses `ContentRefusal::Open` and the four mebibytes are still on disk byte for byte.
- Given the same fixture with no descriptor held, when `dehydrate_entry` is asked, then the release succeeds, the worktree holds the committed blob byte for byte, the ledger row is retracted and `git status --porcelain` is empty.
- Given the implementation mutated to answer `NotOpen`/`Closed` unconditionally, when the open-descriptor test runs alone, then it FAILS; and the restore is verified by reading `git diff`, not by recollection.
- Given any probe root that is not a working procfs, when the probe runs, then it answers `Unknown` and the release refuses `OpenUnknown` — no path answers `Closed` without a completed walk that included this process.
- Given `cargo clippy -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings`, then clean; Rust tests ≥ 3532 passed / 0 failed; `cargo fmt` applied; frontend gates at baseline; all three dependency firewalls pass with no dependency added.

## Spec Change Log

## Review Triage Log

### 2026-08-28 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 8: (high 2, medium 3, low 3)
- defer: 3: (high 0, medium 1, low 2)
- reject: 1: (high 0, medium 1, low 0)
- addressed_findings:
  - `[high]` `[patch]` **The new daemon test would have failed the macOS gate.** `the_daemon_answers_the_open_file_question` was the one added test not gated to Linux, and `scripts/check-macos.sh` runs `cargo test --workspace` (and `cargo clippy --workspace`) on the macOS host while `keeper-syncd` is built for `aarch64-apple-darwin` in the release workflow — where the shared probe answers `Unknown` by design. Gated to `target_os = "linux"` with the reason in its doc, plus a note on the override itself that it compiles on macOS and answers `Unknown` there.
  - `[high]` `[patch]` **The fail-closed discipline was applied only in the outer loop.** `fds.flatten()` discarded every `Err` the `<pid>/fd` iterator yields, and the descriptor `stat` used a catch-all `else { continue }` — so a table that stopped enumerating part-way, or a descriptor whose `stat` answered `ESTALE`/`EIO`/`EACCES`, was silently counted as "did not match" and the walk could answer `Closed` from a truncated scan. That is the one direction AD-125 forbids. Both arms now return `Unknown`; only `NotFound` (the descriptor is gone, so it holds nothing) continues. Covered by a new test that builds both shapes for real — a `<pid>/fd` that is a regular file (`ENOTDIR`) and a descriptor symlinked into a mode-`000` directory (`EACCES`) — and both mutants now fail it.
  - `[medium]` `[patch]` **The recovery argument the whole design leans on was false.** The port doc, `docs/sync.md` and this spec all said a missed opener's next open is recoverable "from the local LFS store … with no network trip". `lfsPruneLocal` defaults to on and releases the store copy *precisely* when the worktree holds the content, so a materialized path is exactly the state in which no local copy is left — and `docs/sync.md`'s own "no trust-the-local-store escape" rule five paragraphs earlier says so. Corrected in all four places (including the deferred-work entry) to: recoverable by asking for the path again, materialized from the remote whose ability to serve that exact object refusal 3 had just proved. The argument survives; its cost is a download, not nothing.
  - `[medium]` `[patch]` **`hidepid=2` was documented as refusing, and it does not.** Both reviewers found it independently. `hidepid=2` is `hidepid=1` plus making *other users'* `<pid>` directories invisible — a process always sees its own — so an unprivileged keeper enumerates only its own user's processes, reads all of them, and answers `Closed`. Corrected in the walk's doc, the trait doc, the daemon's override doc, `docs/sync.md` and the deferred-work entry, with the reason the answer is nonetheless inside the stated `Closed` claim: what vanished from the listing is exactly the set whose descriptor tables keeper was never permitted to read. `hidepid=1` does refuse, and that is now stated as the operational consequence it is (every release on such a host).
  - `[medium]` `[patch]` **The self-check compared a bare pid number.** `own_seen` matched `std::process::id()`, which is this process's pid in *its* PID namespace, against a `proc_root` that may belong to another one — so an unrelated process carrying the same number satisfied it and the check was vacuously true over a process table saying nothing about us. Now `own` is read from `read_link("<proc_root>/self")`, procfs's own answer to "who is reading me", and a root with no resolvable `self` answers `Unknown` before looking at anything. Two new assertions cover it (an empty root, and a root whose `self` dangles).
  - `[low]` `[patch]` **Three more docs still asserted the old behaviour** in files this change already touched: `release_expired`'s "on a real host today every one of these therefore refuses `OpenUnknown`", `tests/virtual_arrival.rs`'s "the trait default refuses on every real host", and one of the new tests' own doc comments. All three now name macOS and Windows instead of "every real host". The fourth site, `keeper/src/sync_ipc.rs:2518`, is owned by a concurrent story and was deferred rather than edited — the coordinator confirmed it is carried into that story's brief.
  - `[low]` `[patch]` **A mode-`000` fixture leaked on assertion failure.** `restore()` ran on the success and root-runner paths but not through a panicking `assert_eq!`, so a failing run left an undeletable directory under the system temp directory (`TempDir::drop` swallows the error by design). Both such tests now read the answer, restore, then assert.
  - `[low]` `[patch]` **The live-`/proc` tests had no host-capability skip.** On a Linux box whose `/proc` is masked or mounted `hidepid=1` the probe correctly answers `Unknown` and the assertions have nothing falsifiable to say. Added `procfs_is_readable()` in both `platform.rs` and `tests/dehydrate_entry.rs`, deliberately phrased as two `read_dir` calls against the **host** rather than as a question put to the code under test — a guard that asked the probe whether it works would skip silently over the very defect this story fixes.

## Design Notes

**Why `/proc/*/fd` is not the `lsof` snapshot AD-125 refuses, and where AD-125 is amended.**
AD-125 says *"'Open' must be a kernel fact, not an `lsof` snapshot"*, and 56.4 read that as refusing
`/proc` scanning by name. Two of the three objections do not survive contact:

1. **It *is* the kernel's own answer.** `lsof` is a process spawn that parses text. `stat` on
   `/proc/<pid>/fd/<n>` is procfs resolving a magic link straight to the `struct file`'s inode —
   there is no more authoritative source on Linux, and `lsof` itself reads exactly this.
2. **TOCTOU is not avoidable by any primitive, so it is not the discriminator.** Without a lease
   (`F_SETLEASE`, which needs `libc`, `unsafe` and file ownership) every "is it open" answer is a
   snapshot the instant after it is read. The real question is which way the snapshot is allowed to
   be wrong, and that is answered by mapping every blind spot to `Unknown`.
3. **What survives is the permission objection**, and it is narrowed rather than dismissed — below.

Load-bearing, and the reason a snapshot is adequate *for this guard*: **refusal 2 does not carry
NFR-40.** Refusals 1 (content identity) and 3 (per-object remote proof at the moment of deletion)
do. A release is a `rename(2)` and AD-125 forbids truncation, so a reader that already holds the
file keeps reading the old inode intact; the harm from a missed opener is that its *next* open sees
~130 bytes of pointer text, and the content comes back by asking for the path again — materialized
from the remote whose ability to serve that exact object refusal 3 had just proved. **Not** from the
local store: `lfsPruneLocal` defaults to on and releases the store copy precisely when the worktree
holds the content, so a materialized path is exactly the state in which there may be no local copy
left — the review caught an earlier draft of this paragraph claiming otherwise, and `docs/sync.md`'s
own "no trust-the-local-store escape" rule five paragraphs earlier says the same thing. So the cost
of being wrong here is a reader that has to ask again, and a download. The cost of `Unknown` forever
is the whole feature.

**The one stated narrowing.** `/proc/<pid>/fd` is mode `0500`, so a process running as another uid —
including root — has a descriptor table keeper cannot read. Requiring *total* completeness makes the
answer `Unknown` on every Linux box that has ever booted (pid 1 alone guarantees it), which is the
refuse-everything failure mode this story exists to end. So the claim `Closed` makes is: **no process
whose descriptor table this process is permitted to read holds this inode open.** The blind spot is
another uid's process, and it is stated here, in the port's doc and in `docs/sync.md` rather than
discovered.

What is *not* narrowed: a process keeper cannot even **identify** — a listed `/proc/<pid>` it cannot
enter, so even the world-readable `stat` file is `EACCES`. That is the `hidepid=1` shape, and on such
a host keeper refuses **every** release, correctly. **`hidepid=2` is not that case**, and an earlier
draft of this spec and of the code's own docs got it backwards: it makes other users' `<pid>`
directories *invisible* rather than unreadable, so an unprivileged keeper enumerates only its own
user's processes and reads all of them — it answers `Closed`, and that is inside the stated claim
rather than a violation of it.

**Whose process table is it.** The self-check reads `<proc_root>/self` rather than comparing against
`std::process::id()`: that is this process's pid in *its* PID namespace, and the procfs at
`proc_root` may belong to another one, in which case an unrelated process carrying the same number
would satisfy a numeric comparison and the check would be vacuously true. A root with no resolvable
`self` is not a process table this walk can reason about, and answers `Unknown`.

**The fail-closed discipline is applied at every level, not just the outer loop.** A dirent error
inside a `<pid>/fd` enumeration, and a `stat` on a descriptor that fails for any reason other than
"it is gone", both answer `Unknown`. A partially enumerated descriptor table is the same defect as a
partially enumerated `/proc`: it finds no opener because it stopped looking.

**Why not `canonicalize` + readlink strings** (the obvious implementation): the readlink of a
deleted-but-open file carries a ` (deleted)` suffix, a process in another mount namespace reports a
different path for the same inode, and `/proc` gives no way to tell a suffix a filename genuinely
ends in from one procfs appended. `stat` on the magic link sidesteps all three and is one syscall
either way.

**macOS answers `Unknown`, and that is the honest answer this run can produce.** Three options,
all rejected with reasons:

- **`libproc` (the safe wrapper, mirroring this crate's own `rlimit` precedent).** Rejected on the
  dependency: an unconditional `bindgen` build dependency puts libclang on the critical path of
  every macOS release build and ~30 crates into `Cargo.lock` for one function.
- **Hand-written `proc_listpids`/`proc_pidfdinfo` FFI in the shell crate** under the audited
  `#[allow(unsafe_code)]` precedent. `libc` supplies `proc_listpids`, `proc_pidfdinfo`,
  `proc_fdinfo`, `vnode_info`, `vinfo_stat`, `PROC_PIDLISTFDS` and `PROX_FDTYPE_VNODE`, but **not**
  `proc_fileinfo` or the `PROC_PIDFDVNODEINFO` flavour, so the struct that carries the answer must
  be declared by hand — and it would land in the one crate that cannot be compiled, let alone run,
  on this host. A wrong layout silently reads the wrong offsets. The workspace's own note beside
  `rlimit` in this crate settles it: *"this crate denies `unsafe_code`, and one syscall is not worth
  an exception to that."*
- **`lsof`.** A spawn per candidate, not present on every system, and AD-125 refuses it by name.

The consequence, stated: **on macOS the app's Release button and the TTL sweep still refuse
`OpenUnknown`.** The epic's safety property is untouched — `Unknown` refuses — and the deliverable
that changes is real: `keeper-syncd` on Linux (AD-52's whole reason to exist) and the desktop app on
Linux both release for the first time. The macOS shape is logged as deferred work: it needs a story
run on a macOS host, where the FFI can be compiled and a real descriptor can be held.

iOS needs no clause: `keeper-sync` is behind
`cfg(not(any(target_os = "ios", target_os = "android")))` in the `keeper` manifest, so no
`SyncPlatform` exists there and the question cannot be asked.

**Cost, measured, and why there is no cache.** Measured by calling the shipped
`probe_open_file_state` in a release build on this host: **1.10 µs per open descriptor** — 545
descriptors across 37 processes → **0.602 ms** for a `Closed` answer, and **0.350 ms** for an `Open`
answer, which short-circuits on the first match. Cost is linear in the machine's *total* open
descriptors and independent of the candidate's size. A busy desktop at ~60 000 descriptors is
therefore ~66 ms per candidate, so 56.5's 32-object budget is ~2 s of walking per sweep pass —
against a pass already allowed to hash `RELEASE_BUDGET_BYTES` = 1 GiB of the same candidates'
content, which costs more. A manual release pays one walk. So: **no cache.** Caching would
contradict the reason 56.4 puts this guard last (it is the answer most likely to have changed while
the hash and the round trip ran) and would buy a bounded fraction of a cost that is already the
smaller half of the pass.

**One term no test can make bite, stated rather than hidden.** The match is
`ino == ino && dev == dev`. The `dev` term is what makes the comparison correct — an inode number is
unique only within a filesystem — but it cannot be falsified by a unit test: making it bite needs two
files on two different devices that happen to share an inode number, and neither creating a second
filesystem nor choosing an inode number is available to a test. It stays because dropping it is a
real bug (a `clip.mp4` on a USB volume could shadow one on the internal disk), and it is recorded
here so a reviewer does not read its absence from the mutation sweep as an oversight.

## Verification

**Commands:**
- `GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=keeper GIT_AUTHOR_EMAIL=dev@keeper.local GIT_COMMITTER_NAME=keeper GIT_COMMITTER_EMAIL=dev@keeper.local cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync -p keeper-core -p keeper-syncd` -- expected: 0 failed, total ≥ 3532.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings` -- expected: clean.
- `cargo fmt --manifest-path src-tauri/Cargo.toml --all` -- expected: no diff afterwards.
- `bun run lint`, `bun run typecheck`, `bun run test` -- expected: exactly the recorded baseline (4 warnings + 1 info; 297 files / 4916 tests).
- `bun run check:core-tauri-free`, `bun run check:core-sync-free`, `bun run check:syncd-lean` -- expected: pass; no dependency added.
- **Mutation proof:** replace the `Open` return in the Linux walk with a fall-through (always
  `Closed`), run the open-descriptor unit test and the open-descriptor engine test alone, confirm
  both FAIL; restore; confirm both pass; verify the restore by reading `git diff`.

**Manual checks (if no CLI):**
- `git status --porcelain -- src/lib/ipc/gen` -- must be empty; this story adds no ts-rs type.
- `git diff -- src-tauri/crates/keeper` -- exactly one method added to `ShellSyncPlatform`; report the
  symbol for the macOS gate.
- `src-tauri/Cargo.toml` and every crate manifest -- unchanged; `Cargo.lock` unchanged.

## Auto Run Result

Status: done

**What was implemented.** Epic 56's release path refused on every real host because
`SyncPlatform::open_file_state` shipped as a provided method whose body was `OpenFileState::Unknown`
and AD-125 turns `Unknown` into a refusal. `keeper_sync::platform::probe_open_file_state` now answers
it on Linux, and both shipping platforms delegate to it, so `keeper-syncd dehydrate`, the app's
Release action on a Linux desktop and 56.5's TTL sweep reach the `rename(2)` for the first time.

The answer is **device + inode identity**, not a `read_link` string comparison: `stat` on
`/proc/<pid>/fd/<n>` makes procfs resolve the magic link to the open file's own inode, which dissolves
the ` (deleted)` suffix, another mount namespace's path prefix, hardlinks and `..`/symlink spellings
in one stroke, and needs no `canonicalize`. `Open` returns on the first match and is definitive.

**Everything the walk cannot see refuses.** Ten guards, each answering `Unknown`: a target that
cannot be `stat`ed; a `proc_root` with no resolvable `self` (the walk reads `<root>/self` rather than
trusting `std::process::id()`, because the procfs may belong to another PID namespace); a `/proc`
that cannot be listed; an unenumerable dirent in `/proc` or inside a `<pid>/fd`; a descriptor whose
`stat` fails for any reason but "it is gone"; a listed `<pid>` that cannot be entered even to read
its world-readable `stat` file (`hidepid=1`, where keeper therefore refuses every release); an
unclassified `read_dir` failure; and a walk that never found the pid `self` named. **One stated
narrowing:** `<pid>/fd` is mode `0500`, so another uid's descriptor table — root's included — is
unreadable, and `Closed` claims exactly *"no process whose descriptor table this process is permitted
to read holds this inode open"*. That is an accepted cost with a reason: demanding total completeness
answers `Unknown` on every Linux box ever booted (pid 1 alone), and refusal 2 is not what carries
NFR-40 — refusals 1 and 3 do, and a release is a `rename(2)` with truncation forbidden, so an
existing reader keeps its inode.

**macOS answers `Unknown`, argued rather than skipped.** `libproc` (the safe wrapper mirroring this
crate's own `rlimit` precedent) carries an unconditional `bindgen` **build** dependency; hand-written
`proc_pidfdinfo` FFI would land in the one crate that cannot be compiled or run on this host, and
`libc` supplies `vnode_info`/`vinfo_stat`/`proc_fdinfo` but not `proc_fileinfo`, so the struct
carrying the answer would be hand-laid-out and a wrong layout reads wrong offsets in silence; `lsof`
is refused by AD-125 by name. iOS needs no clause — `keeper-sync` is not linked there.

**Platform-by-platform.**

| platform | `open_file_state` answers | what a release does |
|---|---|---|
| Linux (`keeper-syncd`) | `Open` / `Closed` from `/proc` by inode identity; `Unknown` on any blind spot | releases; refuses while a reader holds it |
| Linux (desktop app) | identical — the same `probe_open_file_state` | releases; the Files pane's Release action works |
| macOS (desktop app) | `Unknown` | refuses `OpenUnknown`, deliberately |
| Windows (desktop app) | `Unknown` | refuses `OpenUnknown`, deliberately |
| iOS | not asked — no `SyncPlatform` is linked | no sync surface at all |
| any other implementor | the trait default `Unknown` | refuses until it opts in |

**Files changed.**
- `src-tauri/crates/keeper-sync/src/platform.rs` — `probe_open_file_state` (Linux arm + an
  `Unknown` arm for every other target), `PROC_ROOT`, `open_file_state_under_proc`; the trait doc's
  "why no platform answers it yet" replaced with what each platform answers, why the default stays
  `Unknown`, and why a snapshot is adequate for this guard; 7 new unit tests.
- `src-tauri/crates/keeper-sync/src/lib.rs` — `OpenFileState` re-exported beside `SyncPlatform`.
- `src-tauri/crates/keeper-syncd/src/platform.rs` — `LinuxPlatform::open_file_state` delegating to
  the shared probe, beside `free_space`'s "no `libc` here" note; 1 new unit test (Linux-gated).
- `src-tauri/crates/keeper/src/sync.rs` — `ShellSyncPlatform::open_file_state`, the same delegation.
  **Not compilable on this host**; macOS-gate symbol below.
- `src-tauri/crates/keeper-sync/src/engine.rs` — 5 stale doc/comment sites corrected. No logic change.
- `src-tauri/crates/keeper-sync/src/lfs/hydrate.rs` — `ContentRefusal::OpenUnknown`'s doc.
- `src-tauri/crates/keeper-sync/tests/dehydrate_entry.rs` — `RealOpenPlatform` (delegates every
  method to `TestPlatform` except the one under test) plus 2 end-to-end tests through
  `Engine::dehydrate_entry` driven by the machine's real answer.
- `src-tauri/crates/keeper-sync/tests/virtual_arrival.rs` — one stale doc claim.
- `docs/sync.md` — the open-file paragraph rewritten; §13, the sweep's sentence and §17's status
  bullet corrected.
- `_bmad-output/implementation-artifacts/deferred-work.md` — 6 entries.

**Symbols for the macOS gate** (`src-tauri/crates/keeper/**`, uncompilable here):
`<keeper::sync::ShellSyncPlatform as keeper_sync::platform::SyncPlatform>::open_file_state`, and the
added `OpenFileState` name on the existing `use keeper_sync::{…}` import in `src/sync.rs`. One file,
28 insertions, no unsafe, no dependency.

**Verification.**
- `cargo test -p keeper-sync -p keeper-core -p keeper-syncd` — **3542 passed / 0 failed / 1 ignored**
  (baseline 3532; +10). One unrelated pre-existing flake was observed once and is already logged:
  `git::resolve::tests::a_too_old_git_ahead_of_a_good_one_does_not_win` failing with `Text file busy`
  under fork pressure; it passed alone and in two subsequent full runs.
- `cargo clippy -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings` — clean.
- `cargo fmt --all` — applied, no residual diff.
- `bun run lint` — 4 warnings + 1 info, exactly the baseline. `bun run typecheck` — clean.
  `bun run test` — 297 files / 4916 tests passed, exactly the baseline.
- `check:core-tauri-free`, `check:core-sync-free`, `check:syncd-lean` — all pass. No dependency added:
  `git diff` over `src-tauri/Cargo.toml`, `Cargo.lock` and every crate manifest is empty.
- `git status --porcelain -- src/lib/ipc/gen` — empty; no ts-rs type added.
- **Mutation sweep, all ten guards, verified by SHA-256 rather than by recollection.** Each guard was
  replaced with a `Closed`-or-continue mutant, that test run alone, and the file restored and checked
  byte-for-byte against the pre-mutation digest. Eight of ten are caught:

  | guard | mutant | test that fails |
  |---|---|---|
  | the `Open` return | fall through | `a_descriptor_this_test_holds_makes_the_answer_open`, `a_descriptor_on_a_different_file_is_not_confused` |
  | target `metadata` failed | `Closed` | `a_target_that_cannot_be_stat_ed_cannot_be_answered_about` |
  | no resolvable `<root>/self` | `Closed` | `a_probe_root_that_is_not_procfs_cannot_answer` |
  | `read_dir(proc_root)` failed | `Closed` | `a_probe_root_that_is_not_procfs_cannot_answer` |
  | descriptor `stat` error ≠ gone | `continue` | `a_descriptor_table_that_cannot_be_read_through_refuses` |
  | unclassified `<pid>/fd` error | `{}` | `a_descriptor_table_that_cannot_be_read_through_refuses` |
  | the identifiability check | `Closed` | `a_process_that_cannot_be_identified_refuses` |
  | `own_seen` false → `Unknown` | `Closed` | `a_probe_root_that_is_not_procfs_cannot_answer` |
  | `Err` dirent in `/proc` | `continue` | **not caught — see below** |
  | `Err` dirent in `<pid>/fd` | `continue` | **not caught — see below** |

  The mutation that matters most also fails the end-to-end engine test: with the `Open` return
  removed, `a_real_descriptor_makes_a_real_release_refuse` reports the release *succeeding*
  (`Release { path: "clip.mp4", size_bytes: 4194304 }`), which is precisely the data-touching bug.
- **Measured cost** of the shipped function in a release build: 1.10 µs per open descriptor — 545
  descriptors across 37 processes → 0.602 ms for `Closed`, 0.350 ms for `Open` (short-circuits). No
  cache, argued in Design Notes.

**Residual risks.**
- **macOS and Windows still refuse `OpenUnknown`.** The epic's safety property is intact and the
  reason is recorded in three places, but the platform keeper is built for first still cannot release
  from the app. Deferred with the shape, including a runtime layout self-check so a hand-declared
  Apple struct would fail closed.
- **The stated narrowing**: another uid's process — root's included — is invisible to the walk, so
  `Closed` is a claim about the readable subset. Deferred with the reason it is not closed here.
- **A descriptor on a hung hard-NFS or unresponsive FUSE mount blocks the walk with no bound.** New
  exposure (the method used to return a constant), though `release_resolved` already reads every byte
  of the candidate on the same thread. The two obvious mitigations were considered and rejected — a
  `read_link` prefilter reintroduces the string comparison and fails toward a false `Closed`;
  `spawn_blocking` relocates the stall without bounding it. Deferred with a bounded-probe shape.
- **Two of the ten guards are unprovable here** (an `Err` from a `ReadDir` iterator needs `getdents`
  to fail part-way, which no unprivileged fixture can arrange), as is the `dev` half of the identity
  match (it needs two devices sharing an inode number). All three stay because removing them is a
  real defect; recorded so their absence from the sweep is not read as an oversight.
- **`keeper/src/sync_ipc.rs:2518`** still carries the old "every host today" sentence. Owned by a
  concurrent story; the coordinator confirmed the reword is carried into that story's brief.

