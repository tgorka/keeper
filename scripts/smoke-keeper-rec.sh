#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Smoke-test the keeper-rec sidecar's stdio contract without any hardware:
#   1. getCapabilities echoes the request id and carries a protocolVersion.
#   2. Malformed input exits 0 (never panics/hangs), producing no garbage.
#
# Usage: smoke-keeper-rec.sh [path-to-binary]
# Defaults to the installed per-triple binary.
set -euo pipefail

BIN="${1:-src-tauri/crates/keeper/binaries/keeper-rec-aarch64-apple-darwin}"

if [ ! -x "$BIN" ]; then
  echo "smoke: binary not found or not executable: $BIN" >&2
  exit 1
fi

echo "==> smoke: getCapabilities echoes id + protocolVersion"
OUT="$(echo '{"id":7,"method":"getCapabilities"}' | "$BIN")"
echo "    response: $OUT"
# Parse the response as JSON and assert the exact contract (id is echoed verbatim,
# result.protocolVersion is present) rather than a brittle substring that would
# also match "id":70 or a stray protocolVersion fragment.
printf '%s' "$OUT" | python3 -c '
import json, sys
r = json.load(sys.stdin)
assert r.get("id") == 7, "id not echoed verbatim: " + repr(r.get("id"))
pv = r.get("result", {}).get("protocolVersion")
assert isinstance(pv, int), "result.protocolVersion missing or non-int: " + repr(pv)
' || { echo "smoke FAIL: getCapabilities response did not match the contract" >&2; exit 1; }

echo "==> smoke: malformed input exits 0 with no garbage"
set +e
BAD_OUT="$(printf 'not-json\n' | "$BIN")"
BAD_RC=$?
set -e
if [ "$BAD_RC" -ne 0 ]; then
  echo "smoke FAIL: malformed input exited $BAD_RC (expected 0)" >&2
  exit 1
fi
if [ -n "$BAD_OUT" ]; then
  echo "smoke FAIL: malformed input produced output: $BAD_OUT" >&2
  exit 1
fi

echo "==> smoke: unknown method exits 0"
set +e
printf '{"id":2,"method":"start"}\n' | "$BIN" >/dev/null
UNK_RC=$?
set -e
if [ "$UNK_RC" -ne 0 ]; then
  echo "smoke FAIL: unknown method exited $UNK_RC (expected 0)" >&2
  exit 1
fi

echo "==> smoke: EOF / empty stdin exits 0"
set +e
: | "$BIN" >/dev/null
EOF_RC=$?
set -e
if [ "$EOF_RC" -ne 0 ]; then
  echo "smoke FAIL: EOF exited $EOF_RC (expected 0)" >&2
  exit 1
fi

echo "==> smoke: all checks passed"
