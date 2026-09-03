#!/usr/bin/env bash
#
# Build the app on a macOS host, SIGN it, and install it into /Applications there.
#
# The counterpart of `check-macos.sh`, and for the same reason: the recording
# sidecar is Swift + Xcode, the bundle is a `.app`, and signing needs an
# identity in a macOS keychain. None of those cross-compile, so a Linux
# workstation that wants a running Mac app has to build it on the Mac.
#
# The reason is the BUNDLE and more: measured on 2026-09-03 in this dev
# container, `cargo build -p keeper` does NOT succeed on Linux either — there is
# no `pkg-config` and no glib development headers, so `gio-sys` fails its build
# script before the shell crate is reached. What IS usable on Linux is
# `keeper-core` and `keeper-sync` (`cargo nextest run -p keeper-core -p
# keeper-sync`) plus the whole frontend. The shell crate's only gates are CI's
# Rust job (macos-latest) and this script's host. An earlier version of this
# comment claimed the opposite, and a whole epic's shell code was written
# against that claim before anyone ran the command.
#
# The build runs inside the Mac's GUI login session, because that is the only
# session that can reach the signing identity's private key. A Terminal.app
# window opens on the Mac and its output is streamed back here; it closes when
# the build succeeds. There is no unsigned path: see the long comment further
# down for what an ad-hoc install costs.
#
# Usage:
#   scripts/install-macos.sh [host]        # default host: $KEEPER_MACOS_HOST or "hesperia"
#   KEEPER_MACOS_BUILD_ONLY=1 scripts/install-macos.sh    # bundle, do not install
#
# Requirements on the remote: a Rust toolchain, bun, Xcode, rsync, a codesigning
# identity in the login keychain — and ~6 GB free, because a release build of
# matrix-sdk plus gitoxide is not small. `$APPLE_SIGNING_IDENTITY` picks one when
# the keychain holds several; with exactly one it is found automatically.
#
# Nothing is committed or pushed. The previous bundle is kept beside the new one
# as `keeper.app.previous` until the next install replaces it.

set -euo pipefail

HOST="${1:-${KEEPER_MACOS_HOST:-hesperia}}"
REMOTE_DIR="${KEEPER_MACOS_DIR:-keeper-check}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUNDLE="\$HOME/$REMOTE_DIR/src-tauri/target/release/bundle/macos/keeper.app"
DEST="/Applications/keeper.app"

say() { printf '\033[1m==>\033[0m %s\n' "$*"; }
fail() { printf '\033[31mFAIL:\033[0m %s\n' "$*" >&2; exit 1; }

# `bun` lives in ~/.bun/bin, which a non-interactive ssh shell does not have on
# its PATH; and debug info in a release bundle is a gigabyte that buys nothing.
REMOTE_ENV='export PATH="$HOME/.bun/bin:$HOME/.cargo/bin:$PATH" CARGO_PROFILE_RELEASE_DEBUG=0'

# `caffeinate -i` because a release build outlasts the idle-sleep timer, and a
# laptop that sleeps mid-build drops the ssh connection and takes the build
# with it.
remote() { ssh -o BatchMode=yes "$HOST" "caffeinate -i bash -c $(printf '%q' "$1")"; }

say "target: $HOST:~/$REMOTE_DIR -> $DEST"
ssh -o BatchMode=yes "$HOST" 'true' 2>/dev/null || fail "cannot reach $HOST over ssh"

FREE_MB="$(ssh -o BatchMode=yes "$HOST" "df -m \"\$HOME\" | awk 'NR==2 {print \$4}'")"
if [ "${FREE_MB:-0}" -lt 6000 ]; then
  fail "only ${FREE_MB} MB free on $HOST; need ~6 GB. Try: ssh $HOST 'cargo clean --manifest-path $REMOTE_DIR/src-tauri/Cargo.toml'"
fi

# See check-macos.sh for why each of these is excluded. `.git` carries no
# trailing slash on purpose: in a git worktree it is a FILE holding an absolute
# path that exists only here, and copying it makes every git call on the remote
# fail — including the `prepare` script's `lefthook install`.
say "syncing working tree"
rsync -az --delete \
  --exclude '.git' \
  --exclude 'node_modules/' \
  --exclude 'target/' \
  --exclude 'dist/' \
  --exclude 'src-tauri/crates/keeper/binaries/' \
  --exclude 'src-tauri/crates/keeper/gen/apple/build/' \
  "$REPO_ROOT/" "$HOST:$REMOTE_DIR/"

# `--ignore-scripts` skips our own `prepare` (`lefthook install`), which wants a
# git repository this checkout deliberately does not have.
say "bun install"
remote "cd \$HOME/$REMOTE_DIR && $REMOTE_ENV && bun install --frozen-lockfile --ignore-scripts" \
  || fail "bun install"

# --- Build, sign and install, inside the Mac's GUI login session -----------
#
# Everything below happens through `keeper_gui_sh`, and it has to. Signing
# needs the identity's private key out of the login keychain, which only a
# process in the user's GUI session can have: over ssh `codesign` fails with
# `errSecInternalComponent`, `launchctl asuser` needs root, and
# `security unlock-keychain` needs the password. Telling Terminal.app to do the
# work needs none of the three.
#
# This script used to build over ssh and merely WARN that the result was ad-hoc.
# The warning was correct and useless: an ad-hoc bundle's designated
# requirement is a bare cdhash, so every install looked like a brand-new app to
# macOS, the Screen Recording grant stopped matching, Privacy & Security grew
# another dead "keeper" row, and every keychain "Always Allow" was void. That
# ran for weeks. Building unsigned is no longer an option this script offers.
#
# The payload is `build-macos-signed.sh`, unchanged and shared with the
# run-it-on-the-Mac path, so there is one build, one identity lookup, one
# signature check, one check that the bundle can render (lib/bundle-guard.sh:
# a `--debug` build signs fine and opens to a blank window) and one install
# rather than a second implementation here.
say "building and signing in the GUI session on $HOST (Terminal.app opens there)"
GUI_ARGS=""
if [ -z "${KEEPER_MACOS_BUILD_ONLY:-}" ]; then
  GUI_ARGS="--install"
fi
remote "cd \$HOME/$REMOTE_DIR && . scripts/lib/macos-signing.sh && keeper_gui_sh <<'PAYLOAD'
set -euo pipefail
cd \$HOME/$REMOTE_DIR
$REMOTE_ENV
bash scripts/build-macos-signed.sh $GUI_ARGS
PAYLOAD" || fail "signed build"

if [ -n "${KEEPER_MACOS_BUILD_ONLY:-}" ]; then
  say "build only; leaving $DEST alone"
  exit 0
fi

say "installed and running: $DEST"
