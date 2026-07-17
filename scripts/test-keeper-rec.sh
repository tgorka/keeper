#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Run the keeper-rec sidecar's unit tests: the pure rotation policy +
# segment-path derivation (Rotation.swift, Story 17.1) AND the NFR-22
# gapless-concat gate (ConcatAssert*.swift, Story 17.4). The concat gate
# generates its fMP4 fixtures on the runner via AVAssetWriter and asserts the
# manifest's host-clock PTS bounds plus intra-file monotonicity — muxing only,
# no ScreenCaptureKit, so still no capture hardware and no code-signing; the
# whole suite runs anywhere macOS Swift does, including CI (the `recording`
# job).
set -euo pipefail

# Resolve the repo root from this script's location so it works from any cwd.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

echo "==> keeper-rec unit tests (swift test)"
swift test --package-path tools/keeper-rec
