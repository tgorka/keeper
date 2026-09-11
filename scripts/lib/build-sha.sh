#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Stamp the tree with the commit it came from, so the build can name it.
#
# Why this exists: `crates/keeper/build.rs` bakes a commit sha into the binary
# and reads it first from `src-tauri/crates/keeper/build-sha.txt`, a file this
# library is the one writer of. The file is the primary channel because the
# tree that gets built is usually not a repository at all — and when a tree is
# not a repository, `git` does not say so, it answers for the nearest one.
#
# The verified fact behind all of it, checked on hesperia 2026-09-09:
# `~/keeper-check` — the directory every remote script rsyncs into and builds
# in — has NO `.git`. `git -C ~/keeper-check rev-parse HEAD` walked up the
# parents to `~/.git`, the dotfiles checkout, and answered with ITS head,
# `220f8bde27d2`. That is the commit a freshly installed 0.8.x build logged on
# 2026-09-05 as `220f8bde27d2-dirty`: not a stale copy of keeper, a different
# repository altogether. So a probe on the Mac can never be trusted, the stamp
# has to be made on the workstation from the checkout the script runs in, and
# it has to be REFRESHED by every path that produces a binary: on 2026-09-09
# only `install-macos.sh` wrote it, `check-macos.sh` rsynced around it and
# `build-macos-signed.sh` built beside it, so a 0.8.26 build logged
# `commit=0f96b59d1ba5` from a stamp two days old while demonstrably running
# `a9956950`. The wrong line cost a wrong diagnosis that day.
#
# So there is one function, sourced by every script that rsyncs the tree or
# builds it, and it is called BEFORE the rsync or the build. It reads the
# checkout the script actually runs from — never the remote — and writes what
# it finds; where there is no `.git` to ask it removes the file, because a
# missing stamp makes `build.rs` fall back honestly (env, then `git`, then
# `unknown`) while a stale one sends the next reader to the wrong diff. The
# other scripts point here rather than retelling this.
#
# Source this from a script:
#
#     . "$(dirname "${BASH_SOURCE[0]}")/lib/build-sha.sh"
#     keeper_stamp_build_sha "$REPO_ROOT"
#
# Functions:
#   keeper_stamp_build_sha <repo-root>   write `<sha>[-dirty]` to the stamp file
#                                        and print it; with no `.git`, remove the
#                                        file and print nothing

# Where the stamp lives, relative to the repository root. `build.rs` reads it
# from its own directory, and `.gitignore` keeps it out of every commit.
KEEPER_BUILD_SHA_FILE="src-tauri/crates/keeper/build-sha.txt"

# Stamp `<repo-root>` from its own `.git`. Never fails the caller: every caller
# runs under `set -e`, a tree that cannot be stamped still builds and merely
# says `unknown`, and a build refused for the sake of a log line would be the
# wrong trade — so nothing in here returns non-zero, and the one thing that can
# go wrong (an unwritable file) is one line on stderr.
keeper_stamp_build_sha() {
  local root="${1:?usage: keeper_stamp_build_sha <repo-root>}"
  local file="$root/$KEEPER_BUILD_SHA_FILE"
  local sha

  # `-e`, not `-d`: in a git worktree `.git` is a FILE holding the path of the
  # real repository. The explicit check matters because `git -C` would
  # otherwise walk up the parents and happily answer for whatever repository an
  # rsynced tree happens to sit inside (the header says which one it found).
  if [ ! -e "$root/.git" ] ||
    ! sha="$(git -C "$root" rev-parse --short=12 HEAD 2>/dev/null)" ||
    [ -z "$sha" ]; then
    rm -f "$file" 2>/dev/null || true
    return 0
  fi

  # A build from a modified tree is not the commit it names, and a stamp that
  # claimed otherwise would send the next reader to the wrong diff.
  # `--untracked-files=no` is a trade-off, stated: a file that was never
  # `git add`ed does not make the tree dirty, so a build carrying a brand-new
  # source file can still say it is exactly the commit it names. The
  # alternative — counting untracked files — marks every tree with a scratch
  # file or a generated bundle dirty forever, which teaches the reader to
  # ignore the suffix. `build.rs` asks the same question the same way, so the
  # two answers cannot drift from each other.
  if [ -n "$(git -C "$root" status --porcelain --untracked-files=no 2>/dev/null)" ]; then
    sha="$sha-dirty"
  fi

  # `2>/dev/null` BEFORE the file redirection: redirections apply left to
  # right, and it is the shell, not printf, that complains when the target
  # cannot be opened — the earlier one is what silences it.
  if ! printf '%s\n' "$sha" 2>/dev/null > "$file"; then
    echo "warning: could not write $file; the build will say what it can" >&2
    return 0
  fi
  printf '%s\n' "$sha"
}
