#!/usr/bin/env bash
#
# Run the full Rust quality gate on a macOS host.
#
# Why this exists: the `keeper` shell crate cannot be built on Linux — its
# Tauri dependencies need GTK/glib development packages, and a container that
# lacks them (or lacks root to install them) fails in `glib-sys`' build script
# long before any of our code is compiled. That makes `bun run check:rust`
# unrunnable there for the one crate most likely to break, so the choice is
# between skipping the gate and running it somewhere it can actually run.
#
# This syncs the working tree to a macOS host over SSH and runs fmt, clippy and
# the tests there against the SAME sources, then reports the real exit status.
# Nothing is committed or pushed by this script.
#
# Usage:
#   scripts/check-macos.sh [host]          # default host: $KEEPER_MACOS_HOST or "hesperia"
#   KEEPER_MACOS_HOST=mac scripts/check-macos.sh
#
# Requirements on the remote: a Rust toolchain, git, Xcode (for the Swift
# recording sidecar, which `tauri-build` requires as an `externalBin` before the
# shell crate will build at all), and rsync.

set -euo pipefail

HOST="${1:-${KEEPER_MACOS_HOST:-hesperia}}"
REMOTE_DIR="${KEEPER_MACOS_DIR:-keeper-check}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

say() { printf '\033[1m==>\033[0m %s\n' "$*"; }
fail() { printf '\033[31mFAIL:\033[0m %s\n' "$*" >&2; exit 1; }

say "target: $HOST:~/$REMOTE_DIR"
ssh -o BatchMode=yes "$HOST" 'true' 2>/dev/null || fail "cannot reach $HOST over ssh"

# A full workspace build with matrix-sdk AND gitoxide is large. Running out of
# space mid-link produces an "IO failure on output stream" from LLVM that looks
# like a compiler bug, so check up front and say the real reason.
FREE_MB="$(ssh -o BatchMode=yes "$HOST" "df -m \"\$HOME\" | awk 'NR==2 {print \$4}'")"
if [ "${FREE_MB:-0}" -lt 6000 ]; then
  fail "only ${FREE_MB} MB free on $HOST; need ~6 GB. Try: ssh $HOST 'cargo clean --manifest-path $REMOTE_DIR/src-tauri/Cargo.toml'"
fi

# Debug info is the bulk of `target/` and buys nothing for a lint-and-test gate;
# dropping it roughly halves the build directory and keeps this runnable on a
# laptop that is nearly full.
CARGO_ENV='export PATH="$HOME/.cargo/bin:$PATH" CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0'


# Excluded on purpose:
#   target/, node_modules/  — rebuilt remotely; copying them is slower than
#                             compiling and would clobber the remote cache.
#   .git/                   — the gate checks the working tree, not history.
#   binaries/               — the sidecar is built remotely by Xcode; a Linux
#                             copy would be the wrong architecture.
say "syncing working tree"
rsync -az --delete \
  --exclude '.git/' \
  --exclude 'node_modules/' \
  --exclude 'target/' \
  --exclude 'dist/' \
  --exclude 'src-tauri/crates/keeper/binaries/' \
  --exclude 'src-tauri/crates/keeper/gen/apple/build/' \
  "$REPO_ROOT/" "$HOST:$REMOTE_DIR/"

# `tauri-build` refuses to run while `bundle.externalBin` points at a binary
# that does not exist, so the sidecar has to be built before anything else —
# even for `cargo fmt`, which would otherwise never get to run.
say "building the keeper-rec sidecar (required by tauri-build)"
ssh -o BatchMode=yes "$HOST" \
  "cd $REMOTE_DIR && bash scripts/build-keeper-rec.sh >/tmp/keeper-rec-build.log 2>&1" \
  || { ssh -o BatchMode=yes "$HOST" 'tail -30 /tmp/keeper-rec-build.log'; fail "sidecar build failed"; }

say "cargo fmt --check"
ssh -o BatchMode=yes "$HOST" \
  "cd $REMOTE_DIR/src-tauri && $CARGO_ENV && cargo fmt --all --check" \
  || fail "formatting"

say "cargo clippy --workspace --all-targets -- -D warnings"
ssh -o BatchMode=yes "$HOST" \
  "cd $REMOTE_DIR/src-tauri && $CARGO_ENV && cargo clippy --workspace --all-targets -- -D warnings" \
  || fail "clippy"

say "cargo test --workspace"
ssh -o BatchMode=yes "$HOST" \
  "cd $REMOTE_DIR/src-tauri && $CARGO_ENV && cargo test --workspace" \
  || fail "tests"

# ts-rs writes its bindings during the test run. Drift here means a view model
# changed without its generated TypeScript being committed, which CI's
# `bindings:check` would reject.
say "checking generated bindings for drift"
DRIFT="$(ssh -o BatchMode=yes "$HOST" "cd $REMOTE_DIR && git --git-dir=/dev/null diff --no-index --stat --quiet /dev/null /dev/null 2>/dev/null; ls -1 src/lib/ipc/gen | wc -l" || true)"
say "generated binding files present remotely: ${DRIFT// /}"
rsync -az "$HOST:$REMOTE_DIR/src/lib/ipc/gen/" "$REPO_ROOT/src/lib/ipc/gen/"
if ! git -C "$REPO_ROOT" diff --quiet -- src/lib/ipc/gen; then
  git -C "$REPO_ROOT" --no-pager diff --stat -- src/lib/ipc/gen
  fail "generated bindings drifted — commit the regenerated files above"
fi

say "macOS gate passed on $HOST"
