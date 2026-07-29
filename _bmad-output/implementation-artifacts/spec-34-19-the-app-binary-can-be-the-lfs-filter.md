---
title: 'The app binary can be the LFS filter it registers itself as'
type: 'bugfix'
created: '2026-07-29'
status: 'review'
baseline_revision: '1be95be'
---

<intent-contract>

## Intent

**Problem:** `Engine::open` records `filter_program: std::env::current_exe().ok()`
(`engine.rs:374`) and `enforce_local_config_with_filter` writes it into every managed
repository's `.git/config` (`git/repo.rs:536-556`) as three values:

```
filter.lfs.clean    = "<exe>" lfs clean  --repo "<workdir>" %f
filter.lfs.smudge   = "<exe>" lfs smudge --repo "<workdir>" %f
filter.lfs.required = false
```

`current_exe()` is `keeper-syncd` in a CLI run and the app binary in a desktop run, and the
comment above it says "both understand `lfs clean|smudge`" (`engine.rs:372-373`). Only one
does. The daemon implements it at `commands.rs:1630`; `crates/keeper/src` has no `lfs`
subcommand and no argument parsing at all — a repo-wide grep finds `lfs_mode` and
`lfs_threshold_bytes` in `sync_ipc.rs` and nothing else. So on every desktop install the
repository is configured to call a program that cannot answer, and every invocation fails.

It fails **invisibly**, which is the part that matters. `required = false` is deliberate and
`repo.rs:499-501` says why: a worktree whose keeper binary has moved must still be
checkout-able — it would just get pointer files, which is recoverable — where a required filter
hard-fails every git command in the repository. The cost of that choice is that git cannot tell
a filter that failed from one that was never configured; in both cases it stores the bytes it
was handed. No error, no warning, no Activity row, and nothing in `.git/config` looks wrong.

What is lost is what `docs/sync.md` §8 "Working in the folder with plain `git`" promises:
"`git status` stays clean, `git checkout` restores real content rather than pointer text, and a
commit you make by hand stores a pointer and files the object into keeper's store, exactly as
keeper's own commits do." None of the three holds. A hand commit of a large file stores the raw
content as an ordinary git blob — the multi-gigabyte allocation §8's opening paragraph exists to
prevent, because gitoxide has no streaming object read. A checkout leaves pointer text in the
worktree. `git status` calls every LFS-tracked file modified the moment git's stat cache misses,
because the blob is the pointer and the worktree is not — and not identically on both platforms:
DW-121 records the same fixture, same code, same git 2.55, an entry genuinely racily clean (gix's
own `Stat::matches` and `Stat::is_racy`, both true) reading MODIFIED on Linux/ext4 and CLEAN on
macOS/APFS. That divergence forced story 34.13 to delete two integration tests rather than gate them.

**Approach:** The app learns the subcommand — the first of DW-121's two routes, for the reason
in Design Notes. The filter body moves out of `keeper-syncd` into `keeper_sync::lfs::filter`,
so there is one implementation rather than one per binary: `run`, generic over its input and
output streams, and `parse_args`, which recognises exactly the command line `git/repo.rs`
writes and nothing else. `keeper::run` calls `served_as_lfs_filter()` before the Tauri builder
exists and returns if it served, so git never starts a window. `cmd_lfs_filter` keeps its clap
surface and becomes a delegation. `engine.rs` and `git/repo.rs` are untouched: the registration
was always right, and its comment is now true rather than aspirational.

## Boundaries & Constraints

**Always:** stdout carries content and nothing else — not a log line, not a warning, not a
progress message — in both directions and from both binaries, because git reads every byte of
it as the file. Errors go to stderr: the app prints `keeper: lfs filter failed: {err}` and
exits 1. The parse demands `lfs` as the very first argument after `argv[0]`, and `--repo`
non-empty, because the object store's location is the one thing a filter cannot guess. Both
directions stream: a clean hashes into the store in 128 KiB chunks (`store.rs:33`,
`insert_streaming`) and nothing sizes a buffer from the file (AD-46, NFR-23); the single
bounded read in the whole module is the smudge's `MAX_POINTER_BYTES + 1` prefix, which is 1025
bytes. The object is published before the pointer naming it is emitted, so a crash between the
two costs a re-clean and never leaves a pointer to nothing.

**Block If:** (none) — and that is a decision, not an omission. Nothing here refuses to serve.
An unresolvable pointer and ordinary content both pass through rather than failing, because
`required = false` means a refusal degrades to the same stored bytes anyway while breaking the
checkout on the way. The tolerances are what keep a partial fetch usable.

**Never:** Never write anything but content to stdout. Never make the filter required
(`repo.rs:554` stays as it is; that is a separate decision with its own recorded reason). Never
let the argument parse swallow an ordinary launch — no `--help`, no usage text, no exit on an
unknown flag, because a GUI binary that grows a CLI able to eat what the OS passes it is its
own defect. Never buffer an object to size it. Never trust the store by name alone: `contains`
compares the length too (`store.rs:105-108`), so a truncated object is not handed to a smudge
as if it were the content. Do not change the registration in `engine.rs` or the command line
`git/repo.rs` writes — the defect was never there.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| The registered clean line | `lfs clean --repo /w/folder notes/a.bin` | `Some((Clean, /w/folder))`; `%f` is read and discarded | — |
| The registered smudge line | `lfs smudge --repo /w/folder notes/a.bin` | `Some((Smudge, /w/folder))` | — |
| Joined spelling | `lfs clean --repo=/w` | `Some((Clean, /w))` | — |
| No `%f` to give | `lfs clean --repo /w` | `Some((Clean, /w))` | — |
| An argument a future writer adds | valid head plus an unknown token | ignored; still filters | not a reason to refuse |
| Finder launch | `[]` | `None` — the app opens its window | — |
| Older macOS launch | `-psn_0_…` | `None` | — |
| Any other flag | `--flag` | `None` | — |
| Verb with no direction | `lfs` | `None` | — |
| Unknown direction | `lfs explode --repo /w` | `None` | — |
| Direction with no repository | `lfs clean` | `None` | the store cannot be guessed |
| Empty repository value | `lfs clean --repo ""` | `None` | filtered out explicitly |
| `lfs` not first | `serve lfs clean --repo /w` | `None` | — |
| Clean, any size | worktree bytes on stdin | object published at `<repo>/.git/lfs/objects/<a>/<b>/<oid>`, **then** the pointer naming it on stdout | `SyncError::io` up; git keeps its bytes |
| Clean below the threshold | a small file `.gitattributes` routes here | still a pointer — the threshold lives in `.gitattributes`, not in the filter | — |
| Clean of an empty file | zero bytes on stdin | the empty pointer renders to nothing, so an empty file stays empty (`pointer.rs:192-195`) | reasoned from code, not test-covered |
| Smudge, object held | a pointer the store can resolve | the object streamed to stdout | open/copy failure ⇒ `SyncError::io` |
| Smudge, object missing | a well-formed pointer, nothing in the store | the pointer bytes pass through unchanged, so a partial fetch stays usable | — |
| Smudge, object truncated | right name, wrong length | `contains` is false ⇒ pass-through, not a truncated file | — |
| Smudge, ordinary content | a note, not a pointer | passes through unchanged | — |
| Smudge, larger than a pointer | ≥ 1024 bytes on stdin | cannot be a pointer; prefix and remainder both streamed, output length equals input | the bounded read must not truncate |
| Filter fails at all | any error in either direction | app: stderr then exit 1; daemon: `CliError` | `required = false`, so git stores what it had |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/lfs/filter.rs` -- new, 342 lines. The module header (`:1-36`)
  records the defect, why the code is shared and the stdout contract. `Direction` (`:47`), `run`
  (`:67`) which builds the `LfsStore` from `<repo>/.git` and dispatches, `smudge` (`:82`) with the
  bounded prefix read and two pass-throughs, `clean` (`:126`), `parse_args` (`:168`), tests (`:208-341`).
- `src-tauri/crates/keeper-sync/src/lfs/mod.rs:83` -- `pub mod filter;`.
- `src-tauri/crates/keeper/src/lib.rs:54-96` -- `served_as_lfs_filter`, `#[cfg(desktop)]`, with a
  `#[cfg(not(desktop))]` twin returning `false` because an iOS build links no sync engine and can
  never be a filter. `run` (`:110-113`) calls it first and returns if it served.
- `src-tauri/crates/keeper-syncd/src/commands.rs:1619-1641` -- `cmd_lfs_filter` is now a
  delegation to `filter::run`; the inline clean/smudge body and the
  `keeper_sync::lfs::{pointer, store}` imports it needed are gone. The clap surface
  (`Lfs` / `LfsDirection`, `:260-292`, `#[command(hide = true)]`) is unchanged.
- Read and deliberately unchanged: `engine.rs:268` and `:372-374` (the registration and the
  comment that was half-false and is now true); `git/repo.rs:485-556`
  (`enforce_local_config_with_filter` — the writer of the command line `parse_args` accepts, and
  the source of `required = false`); `lfs/store.rs:99-150`; `lfs/pointer.rs:106-115` and `:192-195`;
  `docs/sync.md` §8.

## Tasks & Acceptance

**Execution:**
1. `lfs/filter.rs` -- `Direction`, and `run(repo, direction, input, output)` generic over
   `Read`/`Write`: opens `LfsStore::in_git_dir(repo.join(".git"))`, ensures the layout, dispatches.
2. `lfs/filter.rs` -- `clean`: `insert_streaming`, then `Pointer::new(oid, size).render()` and
   flush. Object first, pointer second, by construction rather than by comment.
3. `lfs/filter.rs` -- `smudge`: read at most `MAX_POINTER_BYTES + 1` through `Read::by_ref` so the
   reader survives for the pass-through; resolve only a parsed pointer whose object the store
   holds at the right length, else write the prefix and `io::copy` the remainder.
4. `lfs/filter.rs` -- `parse_args`: `lfs` first, then the direction, then a scan for `--repo` in
   either spelling; everything else ignored, an empty or absent repo refused.
5. `keeper/src/lib.rs` -- `served_as_lfs_filter` over `env::args().skip(1)`, serving on locked
   stdio, printing failures to stderr and exiting 1; the first statement of `run`.
6. `keeper-syncd/src/commands.rs` -- `cmd_lfs_filter` reduced to a direction match and one call.
7. Tests -- six, all in `filter.rs`; named in Verification.

**Acceptance Criteria:**
1. Given a repository keeper has configured, when git runs `filter.lfs.clean` on a tracked file,
   then stdout is a valid pointer and the object it names is already in `.git/lfs/objects`
   addressed by that digest — the digest, the pointer and the store path agree.
2. Given that pointer, when git runs `filter.lfs.smudge` on it, then the original bytes come back
   byte-exactly and at the original length.
3. Given a pointer whose object the store does not hold, when it is smudged, then the pointer text
   passes through unchanged and the checkout completes.
4. Given content that is not a pointer — including input larger than the 1024-byte ceiling — when
   it is smudged, then every byte passes through and the output length equals the input length.
5. Given the app binary started by Finder (no arguments) or by older macOS (`-psn_…`), when it
   starts, then it opens its window: no argument consumed, no CLI error printed, stdout untouched.
6. Given it started as `lfs clean|smudge --repo <dir> [%f]`, then it serves the invocation and
   returns without constructing a Tauri builder.
7. Given a filter failure in either binary, then the message is on stderr and stdout carries no
   diagnostic byte.

## Design Notes

**Which of DW-121's two routes, and why the other was rejected.** The ledger recorded both and
said explicitly that choosing between them was not a decision to make in passing. Either the app
learns the subcommand — a CLI surface inside a GUI binary, which needs its own argument-parsing
decision and must stay invisible to normal launches — or `enforce_local_config_with_filter`
refuses to register a program that cannot serve, which is honest and trades a silent failure for
a loud absence.

The first, because the second is not a smaller fix: it contradicts something the product already
promises. §8 says registering the filter "is what lets you use ordinary `git` inside a synced
folder" and lists the three behaviours that follow, so refusing would have meant deleting that
paragraph — resolving a defect by withdrawing the feature it breaks. The absence would be loud
only in the sense of being discoverable; the folder still has a broken `git status`, now
permanently. The chosen route costs one hidden subcommand and the risk that it eats an
OS-supplied argument; that risk is bounded by a parse rule and asserted by a test.

**Why the body moved into `keeper-sync` instead of being written twice.** The obvious minimal
change was to copy the daemon's inline body into `keeper/src/lib.rs`. AD-52's whole argument for
`keeper-syncd` is that the daemon and the app share one engine verbatim rather than growing
parallel implementations; that does not stop applying one layer down. This duplication would
have been worse than most: both copies encode what a pointer is, when an object counts as
present, and when to pass bytes through untouched. Two filters that drifted on any of those
would produce a repository whose answer depends on which binary git happened to invoke — less
legible than the failure being fixed, and visible only on a host running both. So the daemon
lost its copy rather than the app gaining one.

**Why `run` is generic over its streams rather than locking stdio itself.** This is the note
that explains the whole defect. The old body took `stdin.lock()` and `stdout.lock()` inside the
function, so the only way to exercise it was to spawn a process, hand it a pipe and read back
what came out. Nobody does that, and nobody did: before this story the filter had no test of any
kind — `git show 4cebaa2^` of `commands.rs` finds `cmd_lfs_filter` and no test naming it. Code
with no cheap way to be tested does not merely go untested; it goes unfinished, because nothing
makes the gap visible. `&mut impl Read` and `&mut impl Write` cost one line of signature and
turn the round trip into a `Vec<u8>` assertion. The two call sites pass locked stdio, which is
where locking belongs — at the process boundary.

**Why the parse is hand-rolled and not `clap`.** The daemon is already a CLI: the filter is one
more `#[command(hide = true)]` subcommand there and stays that way. The app is not, and giving
it one would have consequences out of all proportion to the single invocation shape it must
recognise. clap owns `--help` and `--version`, prints its own text — to stdout, which for this
process is file content — and has opinions about unknown flags, which for a binary the OS
launches with `-psn_0_774…` is precisely the wrong opinion. So `parse_args` recognises the exact
command line `enforce_local_config_with_filter` writes and answers `None` to everything else.
`None` is not an error; it is the ordinary answer for an ordinary launch, and the app proceeds to
build its window. The refusal rule is positional and strict — `lfs` must be the very first
argument after `argv[0]` — so no OS-supplied argument can reach the direction match. That is
asserted over eight argv shapes, because an untested version of this rule is how a GUI binary
starts failing to launch. The parse is strict about the head and lax about the tail: unknown
trailing arguments are ignored, since `%f` is already one and a future writer may add more. It is
not lax about `--repo` — a direction with no repository is unserviceable.

**Why the `%f` path is parsed and then thrown away.** The object is addressed by digest;
nothing in either direction consults the path. It stays in the registered command line and in
the accepted grammar because it is what makes a failing filter legible in a `GIT_TRACE` log,
which is the only diagnostic anyone gets for a non-required filter.

**Why stdout carries content and nothing else.** git treats the filter's entire stdout as the
file. A single log line, a warning about a missing object, or a helpfully printed panic would be
committed into the user's blob or written into their worktree. The tolerant branches make this
concrete: a smudge that cannot resolve a pointer says so by *writing the pointer back*, not an
explanation. Errors go to stderr, which git shows under `GIT_TRACE`, and the app's handler exits
1 immediately rather than falling through to the Tauri builder — a filter process that opened a
window after failing would be a second defect wearing the first one's clothes.

**What the pre-fix binary did when git spawned it was never characterised.** The ledger records
the outcome git observed — the bytes stored unchanged, the failed-filter fallback — not the
mechanism. Nothing here should be read as a report that a window was ever seen to open.

## Verification

**Owed first, because a reader must not assume otherwise.** The end-to-end racily-clean coverage
story 34.13 had to delete over this defect — `a_working_clean_filter_settles_the_racily_clean_case_before_the_guard_does`
in full, and the racily-clean half of `a_racily_clean_pointer_…` — **can exist again now and was
not re-attempted.** DW-121's macOS/Linux divergence should be moot once both binaries serve, but
"should" is the honest word: nothing here re-ran that fixture on either platform. Until someone
does, the 34.13 guard stays covered only as a unit, and this story's claim to have removed the
obstacle is a claim about the cause, not a measurement of the effect.

**The six unit tests, all in `keeper-sync/src/lfs/filter.rs`, and the first tests this code has
ever had.**
- `the_registered_command_line_is_recognised` — both directions of the exact line
  `enforce_local_config_with_filter` writes, plus the `--repo=` joined form and the form with no
  `%f`. Getting this wrong means the filter silently does nothing, which is the defect itself.
- `an_ordinary_launch_is_not_a_filter_invocation` — eight argv shapes that must all answer `None`:
  empty, `-psn_0_…`, `--flag`, `lfs` alone, an unknown direction, a direction with no repo, an
  empty `--repo`, and `lfs` in second position.
- `a_clean_stores_the_object_and_emits_the_pointer_naming_it` — 300 000 bytes, several chunks'
  worth so the streaming loop runs rather than one read; asserts the pointer's size, that the
  store `contains` the object at the point the pointer was emitted, and the stored bytes.
- `a_smudge_returns_the_content_the_pointer_names` — 200 000 bytes cleaned then smudged, byte-exact.
- `a_smudge_passes_through_what_it_cannot_resolve` — ordinary text, and a well-formed pointer for
  an object the store does not hold; both come back unchanged.
- `a_smudge_of_something_far_too_large_streams_all_of_it` — three pointer ceilings of data,
  asserting length and bytes: the failure mode of the bounded prefix read is a truncated file.

**Smoke-tested with the real app binary on macOS (`hesperia`, arm64), which is the only thing
that proves the story rather than the module.** A 3 MiB file was cleaned through the app binary
itself: the emitted pointer carried
`oid sha256:9949c9033a2e28f41c9e65aaf4d88be941c299c8af4fcf8f7e8e55cf8e6f5567`, which equalled
the source file's own sha256, and the object landed at `.git/lfs/objects/99/49/…` — the shard
`store.rs`'s `shard()` derives from that same digest. Those three agreeing is the whole
contract: the digest is the address, so a pointer whose oid matches the source and a store path
that matches the oid means the filter stored the content it claimed to. Smudging that pointer
back through the same binary returned 3145728 bytes, byte-exact. A pointer for an object the
store did not hold passed through unchanged, so a partial fetch stays usable.

**Not covered, explicitly:**
- Nothing in the `keeper` crate is unit-tested here. `served_as_lfs_filter`'s early return, the
  stderr-and-exit-1 path and the ordering against the Tauri builder are covered only by the macOS
  smoke run above; the parse they delegate to is covered, the wiring is not.
- `cmd_lfs_filter`'s delegation has no test of its own, and no test asserts that clap's grammar
  and `parse_args`'s accept the same registered line — that they do was checked by reading
  `LfsDirection` (`commands.rs:274-292`) against the format strings in `repo.rs:542-546`.
- The clean of an empty file is reasoned from `pointer.rs:192-195` and `contains`'s behaviour on
  a missing path; no test exercises it through `filter::run`.
- No test run was performed while writing this document. The test names and bodies above were
  read from the tree at `4cebaa2`; the macOS figures are the implementing host's.
- `filter.lfs.required` stays `false`, so a future regression here will be exactly as silent as
  the one just fixed. That is the accepted cost of the checkout-must-not-hard-fail decision
  (`repo.rs:499-501`), not an oversight — and it is why this defect lasted until someone went
  looking for something else.

**Checked by reading:**
- `git/repo.rs:536-556` — the quoting and word order of the two command lines, against
  `parse_args`'s grammar. The value is quoted because an install directory may contain spaces and
  git splits on whitespace; how git re-splits it is git's own rule and was not verified here.
- `lfs/store.rs:105-108` (`contains` compares length as well as name), `:119-150`
  (`insert_streaming` hashes and writes 128 KiB chunks to a temp file, publishing under the digest
  it turns out to have), `:322-324` (`shard`).
- `lfs/pointer.rs:106-115` — `parse` refuses anything at or above `MAX_POINTER_BYTES`, so the
  smudge's `+ 1` prefix read is exactly the discriminator; `:192-195` for the empty case.
