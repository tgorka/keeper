#!/usr/bin/env bash
#
# Build the app on a macOS host and install it into /Applications there.
#
# The counterpart of `check-macos.sh`, and for the same reason: the `keeper`
# shell crate cannot be built on Linux (Tauri needs GTK/glib) and the recording
# sidecar cannot be built anywhere but macOS (Swift + Xcode). So a Linux
# workstation that wants a running Mac app has to build it on the Mac.
#
# Usage:
#   scripts/install-macos.sh [host]        # default host: $KEEPER_MACOS_HOST or "hesperia"
#   KEEPER_MACOS_BUILD_ONLY=1 scripts/install-macos.sh    # bundle, do not install
#
# Requirements on the remote: a Rust toolchain, bun, Xcode, rsync — and ~6 GB
# free, because a release build of matrix-sdk plus gitoxide is not small.
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

# `--bundles app` because the dmg is not wanted here, and
# `createUpdaterArtifacts: false` because the updater tarball is signed with
# TAURI_SIGNING_PRIVATE_KEY — a release secret this host does not have, and
# asking for it anyway turns a perfectly good build into a non-zero exit after
# the bundle is already on disk. `--config` merges in order, so the second one
# overrides the first.
say "tauri build --bundles app (sidecar, vite, cargo release)"
remote "cd \$HOME/$REMOTE_DIR && $REMOTE_ENV && bun run rec:build && bunx tauri build --config src-tauri/crates/keeper/tauri.conf.json --config '{\"bundle\":{\"createUpdaterArtifacts\":false}}' --bundles app" \
  || fail "app build"

remote "test -d $BUNDLE" || fail "no bundle at $BUNDLE"
say "built: $(remote "/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' $BUNDLE/Contents/Info.plist") ($(remote "stat -f '%Sm' $BUNDLE/Contents/MacOS/keeper"))"

if [ -n "${KEEPER_MACOS_BUILD_ONLY:-}" ]; then
  say "build only; leaving $DEST alone"
  exit 0
fi

# Quit-then-replace, not replace-in-place: a running app keeps its old inode, so
# copying over it leaves the user driving the previous build while the disk says
# otherwise. /Applications is group-writable by admin, so this needs no sudo.
say "installing"
remote "$(cat <<REMOTE
set -uo pipefail
if pgrep -f "$DEST/Contents/MacOS/keeper" > /dev/null; then
  osascript -e 'quit app "keeper"' 2>/dev/null
  for _ in \$(seq 1 20); do
    pgrep -f "$DEST/Contents/MacOS/keeper" > /dev/null || break
    sleep 0.5
  done
  pgrep -f "$DEST/Contents/MacOS/keeper" > /dev/null && pkill -f "$DEST/Contents/MacOS/keeper"
  sleep 1
fi
rm -rf "$DEST.previous"
[ -d "$DEST" ] && mv "$DEST" "$DEST.previous"
ditto "$BUNDLE" "$DEST" || { [ -d "$DEST.previous" ] && mv "$DEST.previous" "$DEST"; exit 1; }
# The quarantine bit is for downloads; this bundle was built on this machine.
xattr -dr com.apple.quarantine "$DEST" 2>/dev/null
open -a "$DEST"
sleep 4
pgrep -f "$DEST/Contents/MacOS/keeper" > /dev/null || { echo "installed but not running"; exit 1; }
REMOTE
)" || fail "install"

say "installed and running: $DEST"
