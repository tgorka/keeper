---
title: 'The build names the tree it came from'
type: 'bugfix'
created: '2026-09-10'
status: 'done'
review_loop_iteration: 1
baseline_commit: '5bf1002d7c41dced9b69c8acee45811e3e653989'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** the app's `build_identity` line named a commit the running code did not come from. On hesperia the 0.8.26 build of 2026-09-09 19:16 UTC logged `commit=0f96b59d1ba5` while it demonstrably carried `a9956950` (it emitted a log line that commit introduced); `~/keeper-check/src-tauri/crates/keeper/build-sha.txt` still held `0f96b59d1ba5` from an `install-macos.sh` run at 10:08, and `build.rs` prefers that file over any `git` answer — by design, because the on-Mac build directory's `.git` is itself stale (`220f8bde27d2`). `check-macos.sh` rsyncs the tree without rewriting the file, and `build-macos-signed.sh` / `release-macos.sh` build in place without touching it, so any build that is not an `install-macos.sh` run inherits whatever the last install stamped. The line exists so the next reader lands on the right diff; a wrong one cost a wrong diagnosis on 2026-09-09.

**Approach:** one shared shell function stamps `build-sha.txt` from the checkout a script actually runs from — sha, `-dirty` when the tree differs — and every script that rsyncs or builds calls it before doing either; where there is no `.git` to ask, the file is removed rather than left stale. `build.rs` says which source it used (`file`, `env`, `git`, `none`) as a cargo warning and bakes it into the binary, and the `build_identity` line prints it, so a stale stamp is at least visibly a stamp.

## Boundaries & Constraints

**Always:** `build.rs` keeps its precedence (file, then env, then `git`) and its rerun triggers; the stamp is written by the script's own checkout, never probed on the remote; the function lives in `scripts/lib/` beside `macos-signing.sh` and is sourced, not copied; `build-sha.txt` stays gitignored; the banner's existing tests keep passing; nothing here changes what is built, only what the build says about itself.

**Ask First:** none.

**Never:** make the build fail on a missing or stale stamp; write the stamp into a commit; touch the iOS build phase beyond what the shared function already covers through `install-ios.sh` if it rsyncs.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| `check-macos.sh` from a checkout | HEAD `abc`, clean | stamp `abc` written before the rsync; remote build logs `commit=abc sha_source=file` | — |
| Dirty checkout | uncommitted changes | stamp `abc-dirty` | — |
| Build directory with a stale stamp and no script run | `bun run tauri:build` in `~/keeper-check` | the stamp is used, and the warning and the banner say `sha_source=file` | — |
| No `.git` where the script runs | rsynced tree | the file is removed, `build.rs` falls to env, then `git`, then `unknown`; banner says which | — |
| `build.rs` warning | any build | one `cargo:warning=keeper build sha <value> from <source>` | — |

</frozen-after-approval>

## Code Map

- `src-tauri/crates/keeper/build.rs` -- the precedence (:31–60): `build-sha.txt` (:35), `KEEPER_BUILD_SHA` env (:36), `git rev-parse` (:42–50), `-dirty` probe (:57–66), `rustc-env` lines (:67–77), rerun triggers (:33–34, :91).
- `src-tauri/crates/keeper/src/build_identity.rs` -- `banner` (:38) reads `KEEPER_BUILD_SHA`/`KEEPER_BUILD_TIME`; tests :121–138 are the shape for a `sha_source` assertion.
- `scripts/install-macos.sh` -- the only stamping today (:52–66: sha, `-dirty`, env, file) — becomes a call to the shared function.
- `scripts/check-macos.sh` -- rsync at :61 with no stamp.
- `scripts/install-ios.sh` -- the same stamping block as `install-macos.sh` (:89–103) and a rsync at :243; the iOS build phase itself is untouched.
- `scripts/build-sha.test.ts` -- vitest over the library, in the `scripts/check-bundle.test.ts` mould.
- `scripts/build-macos-signed.sh` -- `bunx tauri build` at :81 with no stamp; `release-macos.sh` calls it.
- `scripts/lib/macos-signing.sh` -- the sourced-library shape (`. "$SCRIPT_DIR/lib/..."`).
- `.gitignore:54` -- `build-sha.txt` ignored.
- `docs/release.md` "How to cut a release" (:167) -- where the stamp is mentioned.

## Tasks & Acceptance

**Execution:**
- [x] `scripts/lib/build-sha.sh` -- `keeper_stamp_build_sha <repo-root>`: with a readable `.git`, write `<sha>[-dirty]` to `src-tauri/crates/keeper/build-sha.txt` and print it; without one, remove the file and print nothing -- the one place the stamp is made.
- [x] `scripts/install-macos.sh`, `scripts/check-macos.sh`, `scripts/build-macos-signed.sh` -- source the library and stamp before the rsync / the build (`release-macos.sh` inherits through `build-macos-signed.sh`; add the call there too if it ever builds on its own) -- every path that produces a binary stamps.
- [x] `src-tauri/crates/keeper/build.rs` -- record the source that won (`file` / `env` / `git` / `none`) in `KEEPER_BUILD_SHA_SOURCE`, emit `cargo:warning=keeper build sha <sha> from <source>` -- the build says.
- [x] `src-tauri/crates/keeper/src/build_identity.rs` -- the banner prints `sha_source=<source>`; a test asserts the field is one of the four words -- the binary says.
- [x] `docs/release.md` -- one paragraph: what the stamp is, who writes it, how to read `sha_source` -- the record.

Review patches (step 04, loop 1 — amendments to the diff, the diff is kept). Verified fact for every comment: `~/keeper-check` has NO `.git`; `git -C ~/keeper-check rev-parse` walked up to `~/.git` (the dotfiles checkout, HEAD `220f8bde27d2`) — say this once, in `scripts/lib/build-sha.sh`'s header, and point at it elsewhere.
- [x] `scripts/build-macos-signed.sh` -- with `KEEPER_BUILD_SHA` unset (every `release-macos.sh` run) it re-stamps the Mac copy: with no `.git` that removes the rsynced stamp, with a leftover `.git` it overwrites it with a stale probe labelled `file`. Fix: re-stamp only when `$REPO_ROOT/.git` exists; otherwise keep an existing stamp and print the file's content as the build sha (both branches print the file, the value `build.rs` will read). `${KEEPER_BUILD_SHA:-}` everywhere — an empty export means "not stated" (the install scripts export it empty when the workstation has no `.git`).
- [x] `scripts/check-macos.sh`, `scripts/install-macos.sh`, `scripts/install-ios.sh` -- the rsync excludes `.git` but never removes a leftover on the remote; remove `$REMOTE_DIR/.git` on the host before the rsync so the copy never has a repository of its own and the stamp is its only word; `check-macos.sh` echoes what it stamped instead of `>/dev/null`; the pasted comment blocks in the two install scripts point at the library header instead of retelling it.
- [x] `src-tauri/crates/keeper/build.rs` -- a stamp left on a real checkout outranks its moving `.git`: when `../../../.git` exists, run the `git` probe too and, if it disagrees with the stamp, use `git` and warn `keeper build sha: stamp <x> disagrees with git <y>; using git`; validate a stated sha (`^[0-9a-f]{7,40}(-dirty)?$`, else treat as absent with a warning); add `../../../.git/logs/HEAD` to the rerun triggers so a same-branch commit reruns; fix the doc ("four sources"; the file is the primary channel).
- [x] `scripts/lib/build-sha.sh` -- `rm -f` and the write never fail the caller under `set -e` (`2>/dev/null` before the redirection target, `|| true` where needed, one stderr warning); the `--untracked-files=no` comment states the real trade-off (a never-added file reads clean) rather than the gitignored-stamp reason.
- [x] `scripts/build-sha.test.ts` -- vitest in the `scripts/check-bundle.test.ts` mould (spawn bash): clean → 12-hex; dirty → `-dirty`; no `.git` with a stale file → file removed, nothing printed; a `.git`-less dir nested inside another repository → nothing inherited; worktree (`.git` is a file) → stamped.
- [x] `src-tauri/crates/keeper/src/build_identity.rs` tests -- `commit` and `sha_source` agree: `none` ⇔ `unknown`; `git` ⇒ `^[0-9a-f]{12}(-dirty)?$`; and, gated on `GITHUB_ACTIONS`, `sha_source=git` with a 12-hex commit (CI builds from a real checkout with no stamp).
- [x] `docs/release.md` -- the paragraph gets its own heading ("What `commit=` and `sha_source=` mean"), names the four scripts, corrects the `release-macos.sh` claim to the keep-the-stamp rule, and says a stamp on a developer checkout is overridden by `git` when they disagree.
- [x] spec (non-frozen) -- Code Map names `install-ios.sh`; the Verification recipe copies `scripts/lib/build-sha.sh` into the clone (the file is untracked until this lands) or runs after the commit.

**Acceptance Criteria:**
- Given a checkout at `abc` with uncommitted changes, when any of the three scripts runs, then `build-sha.txt` reads `abc-dirty` before the rsync or build starts.
- Given a build, when it runs, then cargo prints one warning naming the sha and its source, and the running app's banner carries `sha_source=`.
- Given a tree with no `.git`, when a script stamps, then no stale file survives.

## Verification

**Commands:**
- `bash -n scripts/lib/build-sha.sh scripts/install-macos.sh scripts/install-ios.sh scripts/check-macos.sh scripts/build-macos-signed.sh` -- expected: no syntax errors.
- `tmp=$(mktemp -d) && git clone -q . "$tmp" && mkdir -p "$tmp/scripts/lib" && cp scripts/lib/build-sha.sh "$tmp/scripts/lib/" && (cd "$tmp" && . scripts/lib/build-sha.sh && keeper_stamp_build_sha "$tmp" && cat src-tauri/crates/keeper/build-sha.txt)` -- expected: the clone's HEAD short sha (the `cp` is needed until the library is committed: a clone of the baseline has no `scripts/lib/build-sha.sh`); edit a tracked file in the clone and repeat -- expected: `-dirty` suffix.
- `bun run test -- scripts/build-sha.test.ts` -- expected: the library's cases pass (clean, dirty, never-added file, no `.git`, nested in another repository, worktree, unwritable).
- `cargo build --manifest-path src-tauri/Cargo.toml -p keeper 2>&1 | grep "keeper build sha"` -- expected: one warning line naming the source (on this Mac the shell crate builds).
- `cargo nextest run --manifest-path src-tauri/Cargo.toml -p keeper build_identity` -- expected: banner tests pass, including the new `sha_source` one.

## Suggested Review Order

**The one writer of the stamp**

- Entry point: the function every script calls; the header states the verified fact (`~/keeper-check` has no `.git`; `git -C` walked up to the dotfiles checkout).
  [`build-sha.sh:53`](../../scripts/lib/build-sha.sh#L53)

**What the build says about itself**

- Precedence kept (file, env, git, none); a stated value must look like a sha; a live `.git` overrides a disagreeing stamp, with a warning.
  [`build.rs:113`](../../src-tauri/crates/keeper/build.rs#L113)
- A same-branch commit reruns the script.
  [`build.rs:82`](../../src-tauri/crates/keeper/build.rs#L82)
- The banner carries `sha_source=`.
  [`build_identity.rs:188`](../../src-tauri/crates/keeper/src/build_identity.rs#L188)

**Every path that produces a binary stamps — and never probes the copy**

- The signed build re-stamps only on a real checkout; on the Mac copy it keeps the rsynced stamp and prints the file's content.
  [`build-macos-signed.sh:102`](../../scripts/build-macos-signed.sh#L102)
- The rsync scripts remove a leftover `.git` on the host before syncing, so the copy has no repository of its own.
  [`check-macos.sh:62`](../../scripts/check-macos.sh#L62)

**The record**

- What `commit=` and `sha_source=` mean, who writes the stamp, and the two override rules.
  [`release.md:167`](../../docs/release.md#L167)

**Tests**

- The library under `set -euo pipefail`: clean, dirty, never-added file, no `.git` with a stale file, nested in another repository, worktree, unwritable target.
  [`build-sha.test.ts:76`](../../scripts/build-sha.test.ts#L76)
- The banner: `none ⇔ unknown`, a probed sha's shape, and `sha_source=git` on CI.
  [`build_identity.rs:207`](../../src-tauri/crates/keeper/src/build_identity.rs#L207)
