#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Build a locally installable keeper.app with a STABLE code signature.
#
# Why this exists: macOS TCC (Screen Recording, microphone, camera) remembers a
# grant against the app's *designated requirement*, not its path. A plain
# `bun run tauri:build` with no signing identity produces an ad-hoc,
# linker-signed bundle whose designated requirement is a bare `cdhash` — the
# hash of that exact binary. Every rebuild therefore looks like a brand-new app
# to TCC: the old grant stops matching, macOS re-prompts, and Privacy & Security
# slowly fills up with duplicate "keeper" rows that are checked but do nothing.
# That is the "infinite permission prompt" loop, and no amount of toggling fixes
# it because the stored requirement can never match the next build.
#
# Signing with a real certificate makes the designated requirement identity-based
# (`identifier "dev.tgorka.keeper" and anchor apple generic and certificate
# leaf[subject.CN] = "..."`), which is stable across rebuilds — so the grant is
# given once and then survives every future local build.
#
# Usage:
#   bun run tauri:build:signed              # build + verify
#   bun run tauri:build:signed -- --install # also replace /Applications/keeper.app
#
# The identity comes from $APPLE_SIGNING_IDENTITY, or is auto-detected when the
# login keychain holds exactly one codesigning identity. Keychain access needs a
# GUI login session: run this from Terminal.app on the Mac, not over SSH
# (codesign fails with `errSecInternalComponent` from a non-GUI session).
#
# From a Linux workstation, do not run this over ssh yourself — use
# `bun run install:macos`, which rsyncs the tree and then dispatches THIS script
# into the Mac's GUI session for exactly that reason.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

# BUNDLE_ID is resolved from tauri.conf.json below, once the library is sourced.
APP="src-tauri/target/release/bundle/macos/keeper.app"
INSTALLED="/Applications/keeper.app"

INSTALL=0
for arg in "$@"; do
  case "$arg" in
    --install) INSTALL=1 ;;
    *) echo "error: unknown argument: $arg" >&2; exit 2 ;;
  esac
done

if [ "$(uname -s)" != "Darwin" ]; then
  echo "error: macOS only (host: $(uname -s))." >&2
  exit 1
fi

# --- Resolve a stable signing identity -------------------------------------
# Identity resolution and the signature check live in lib/macos-signing.sh,
# because install-macos.sh has to make exactly the same two decisions and a
# second copy is how they drifted apart the first time.
. "$SCRIPT_DIR/lib/macos-signing.sh"

BUNDLE_ID="$(keeper_bundle_id)"
APPLE_SIGNING_IDENTITY="$(keeper_signing_identity)"
export APPLE_SIGNING_IDENTITY
echo "==> Signing identity: $APPLE_SIGNING_IDENTITY"
echo "==> Bundle identifier: $BUNDLE_ID"

# --- Build (sidecar signs itself off the exported identity) ----------------
# createUpdaterArtifacts is disabled for local builds: the updater's minisign
# private key is a release-only secret, and without it Tauri fails the build
# *after* bundling. Merged in as an overlay so the committed config stays clean.
# Bundles: `app` by default because the install path wants the bundle that goes
# straight into /Applications, and building the disk image as well adds minutes
# to every install. `release-macos.sh` needs the dmg and asks for it through
# `KEEPER_BUNDLES` — this used to be a comment claiming release-macos.sh "owns"
# the dmg with no way for it to say so, and the release failed at the
# `[ -f "$DMG" ]` check after a full signed build.
KEEPER_BUNDLES="${KEEPER_BUNDLES:-app}"
echo "==> Bundles: $KEEPER_BUNDLES"

# --- Say which commit this is ----------------------------------------------
# `build.rs` reads the sha from `crates/keeper/build-sha.txt` first, and this
# script runs in two very different places (why the file, and why the Mac's
# copy must never be probed: the header of lib/build-sha.sh):
#
#   - on a developer's checkout (`bun run tauri:build:signed`), where `.git`
#     is real and a build in place has to refresh the stamp or it ships
#     whatever the last script left — two days stale on 2026-09-09;
#   - on the Mac's rsynced copy, dispatched by `install-macos.sh` or run by
#     hand for `release-macos.sh`, where there is NO `.git` and the stamp the
#     workstation wrote before the rsync is the only true word about the tree.
#
# So the rule is: re-stamp only where there is a `.git` to ask; otherwise keep
# the stamp as it is. Calling the library unconditionally would REMOVE the
# rsynced stamp on the Mac (that is what it does with no `.git`), and every
# `release-macos.sh` run arrives with `KEEPER_BUILD_SHA` unset, so the env
# cannot be the thing that decides. Either way, what is printed is the file —
# the value `build.rs` will actually read — and `${KEEPER_BUILD_SHA:-}` is
# only consulted when there is no file: the install scripts export it EMPTY
# when the workstation itself had no `.git`, and empty means "not stated".
. "$SCRIPT_DIR/lib/build-sha.sh"
if [ -e "$REPO_ROOT/.git" ]; then
  keeper_stamp_build_sha "$REPO_ROOT" >/dev/null
fi
BUILD_SHA_FILE="$REPO_ROOT/$KEEPER_BUILD_SHA_FILE"
if [ -s "$BUILD_SHA_FILE" ]; then
  echo "==> Build sha: $(cat "$BUILD_SHA_FILE") (from $KEEPER_BUILD_SHA_FILE)"
elif [ -n "${KEEPER_BUILD_SHA:-}" ]; then
  echo "==> Build sha: $KEEPER_BUILD_SHA (from KEEPER_BUILD_SHA; no stamp file)"
else
  echo "==> Build sha: not stated (no stamp file); build.rs will probe git if it can, else say unknown"
fi

bash "$SCRIPT_DIR/build-keeper-rec.sh"
bunx tauri build \
  --config src-tauri/crates/keeper/tauri.conf.json \
  --config '{"bundle":{"createUpdaterArtifacts":false}}' \
  --bundles "$KEEPER_BUNDLES"

# --- Verify the signature is actually stable -------------------------------
keeper_require_stable_signature "$APP"

# --- Verify the bundle can render at all -----------------------------------
# A `--debug` build signs and installs exactly like this one and opens to a
# blank window, because its executable embeds no frontend and points the
# webview at the Vite dev server. The signature check above cannot see that;
# this one refuses the bundle before it reaches /Applications (AD-173).
. "$SCRIPT_DIR/lib/bundle-guard.sh"
keeper_require_renderable_bundle "$APP"

# --- Optional install ------------------------------------------------------
if [ "$INSTALL" -eq 1 ]; then
  echo "==> Quitting a running keeper (graceful — a live recording finalizes)"
  osascript -e 'tell application "keeper" to quit' 2>/dev/null || true
  for _ in $(seq 1 20); do
    pgrep -f "$INSTALLED/Contents/MacOS/keeper" >/dev/null || break
    sleep 0.5
  done
  echo "==> Installing to $INSTALLED"
  rm -rf "$INSTALLED"
  cp -R "$APP" "$INSTALLED"
  # Relaunch, because this script quit a running app two lines up: leaving the
  # user with no keeper at all is a worse end state than the one they started
  # in, and `install-macos.sh` reports the app as running on the strength of it.
  echo "==> Launching $INSTALLED"
  open -a "$INSTALLED"
fi

cat <<EOF

Done.

If Screen Recording still re-prompts (only expected the FIRST time after moving
from an ad-hoc build, or after switching certificates), clear the stale entries
once and grant again — this is the scripted equivalent of removing the row with
"-" in Privacy & Security:

    tccutil reset ScreenCapture $BUNDLE_ID

Then start a recording and approve the prompt. With a stable signature the grant
persists across every later rebuild.
EOF
