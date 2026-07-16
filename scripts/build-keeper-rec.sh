#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Build the keeper-rec capture sidecar and install it under the name Tauri's
# bundle.externalBin expects: keeper-rec-<triple>. aarch64-only, no lipo.
#
# Used by dev (`bun run tauri:dev`), CI (`bun run tauri:build -- --no-bundle`),
# and the release pipeline — the sidecar must exist in binaries/ before Tauri
# resolves externalBin, so this always runs first.
set -euo pipefail

# Resolve the repo root from this script's location so it works from any cwd.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

# keeper ships aarch64-macOS-only (no lipo). Fail loudly on any other host so a
# non-Apple-Silicon build gets a clear message here instead of a cryptic
# "matching sidecar not found" from Tauri's externalBin resolver later.
if [ "$(uname -s)" != "Darwin" ] || [ "$(uname -m)" != "arm64" ]; then
  echo "error: keeper-rec builds on Apple Silicon macOS only (host: $(uname -s)/$(uname -m))." >&2
  echo "       Recording is aarch64-apple-darwin only this phase; no universal/lipo build." >&2
  exit 1
fi

PACKAGE_PATH="tools/keeper-rec"
DEST_DIR="src-tauri/crates/keeper/binaries"
DEST="$DEST_DIR/keeper-rec-aarch64-apple-darwin"

echo "==> Building keeper-rec (release, arm64)"
swift build -c release --arch arm64 --package-path "$PACKAGE_PATH"

BIN_DIR="$(swift build -c release --arch arm64 --package-path "$PACKAGE_PATH" --show-bin-path)"
PRODUCT="$BIN_DIR/keeper-rec"

if [ ! -x "$PRODUCT" ]; then
  echo "error: expected product not found at $PRODUCT" >&2
  exit 1
fi

echo "==> Installing to $DEST"
mkdir -p "$DEST_DIR"
cp "$PRODUCT" "$DEST"
chmod +x "$DEST"

# Smoke check: prove the stdin→stdout contract holds before we ship it as a
# build input. Delegated to a standalone script so it can also be run alone.
bash "$SCRIPT_DIR/smoke-keeper-rec.sh" "$DEST"

echo "==> keeper-rec ready: $DEST"
