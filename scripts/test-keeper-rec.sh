#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Run the keeper-rec sidecar's unit tests (Story 17.1): the pure rotation
# policy + segment-path derivation in Rotation.swift. Foundation-only logic —
# no capture hardware, no code-signing — so this runs anywhere macOS Swift
# does, including CI (the `recording` job). Story 17.4 later extends this
# harness with the gapless-concat gate.
set -euo pipefail

# Resolve the repo root from this script's location so it works from any cwd.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

echo "==> keeper-rec unit tests (swift test)"
swift test --package-path tools/keeper-rec
