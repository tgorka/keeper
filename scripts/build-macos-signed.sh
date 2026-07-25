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
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

BUNDLE_ID="dev.tgorka.keeper"
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
if [ -z "${APPLE_SIGNING_IDENTITY:-}" ]; then
  # `security find-identity` prints `  1) <sha1> "Name"`; keep the quoted names.
  IDENTITIES="$(security find-identity -v -p codesigning | sed -n 's/^ *[0-9]*) [0-9A-F]* "\(.*\)"$/\1/p')"
  COUNT="$(printf '%s\n' "$IDENTITIES" | grep -c . || true)"
  if [ "$COUNT" -eq 0 ]; then
    cat >&2 <<'EOF'
error: no codesigning identity found in the login keychain.

Real capture needs a stable signature (see docs/recording.md). Create a free
Apple Development certificate (Xcode > Settings > Accounts > Manage Certificates
> + > Apple Development), then re-run. Without one, every rebuild re-prompts for
Screen Recording and the grant never sticks.
EOF
    exit 1
  fi
  if [ "$COUNT" -gt 1 ]; then
    echo "error: multiple codesigning identities found; set APPLE_SIGNING_IDENTITY to one of:" >&2
    printf '  %s\n' $(printf '%s\n' "$IDENTITIES" | sed 's/^/"/;s/$/"/') >&2
    exit 1
  fi
  APPLE_SIGNING_IDENTITY="$IDENTITIES"
fi
export APPLE_SIGNING_IDENTITY
echo "==> Signing identity: $APPLE_SIGNING_IDENTITY"

# --- Build (sidecar signs itself off the exported identity) ----------------
# createUpdaterArtifacts is disabled for local builds: the updater's minisign
# private key is a release-only secret, and without it Tauri fails the build
# *after* bundling. Merged in as an overlay so the committed config stays clean.
bash "$SCRIPT_DIR/build-keeper-rec.sh"
bunx tauri build \
  --config src-tauri/crates/keeper/tauri.conf.json \
  --config '{"bundle":{"createUpdaterArtifacts":false}}'

# --- Verify the signature is actually stable -------------------------------
# A silent fallback to ad-hoc would reintroduce the prompt loop, so treat a
# cdhash-based requirement as a hard failure rather than shipping it.
if [ ! -d "$APP" ]; then
  echo "error: expected bundle not found at $APP" >&2
  exit 1
fi
codesign --verify --strict "$APP"
DR="$(codesign -d -r- "$APP" 2>/dev/null | sed -n 's/^designated => //p')"
echo "==> Designated requirement: $DR"
case "$DR" in
  *cdhash*)
    echo "error: bundle is ad-hoc signed (cdhash requirement) — TCC grants will not survive a rebuild." >&2
    exit 1
    ;;
  *"identifier \"$BUNDLE_ID\""*) ;;
  *)
    echo "error: unexpected designated requirement; refusing to call this a stable signature." >&2
    exit 1
    ;;
esac
echo "==> Signature is identity-based and stable across rebuilds."

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
  echo "==> Installed. Launch with: open -a $INSTALLED"
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
