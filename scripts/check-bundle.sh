#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Refuse a keeper bundle that cannot render, from the command line.
#
# The check itself lives in lib/bundle-guard.sh and is run by the macOS
# install path (`build-macos-signed.sh`, which `install-macos.sh` dispatches).
# The iOS path has no script of its own — it is `tauri ios build` followed by
# `devicectl` or a tester's re-sign, both driven by hand from docs/ios.md — so
# this is the command that path runs between the two. An iOS install is not
# finished until it has passed.
#
# Usage:
#   scripts/check-bundle.sh src-tauri/crates/keeper/gen/apple/build/arm64/keeper.ipa
#   scripts/check-bundle.sh src-tauri/target/release/bundle/macos/keeper.app
#   scripts/check-bundle.sh src-tauri/target/release/keeper      # --no-bundle build
#
# Exit status is the verdict: 0 renders, 1 refused (the reason is on stderr).
set -euo pipefail

if [ $# -ne 1 ]; then
  echo "usage: $0 <keeper.app|keeper.ipa|executable>" >&2
  exit 2
fi

. "$(dirname "${BASH_SOURCE[0]}")/lib/bundle-guard.sh"
keeper_require_renderable_bundle "$1"
