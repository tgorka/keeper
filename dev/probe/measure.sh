#!/bin/sh
# Drive dev/probe in a real browser at real widths and print what it measured.
#
#   usage: dev/probe/measure.sh <label> "<query>" [widths...]
#   e.g.:  dev/probe/measure.sh after "view=tasks&act=create&tasks=none"
#
# Needs, in this order:
#   bun x vite --port 8133                 # serves the real modules + mock shell
#   bun run dev/probe/collector.ts         # receives the probe's beacons
#
# CHROME defaults to the macOS install path; override it anywhere else.
# `--use-mock-keychain` because Chrome otherwise blocks on a login-keychain
# prompt that no headless run can answer.
set -eu

CHROME="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
OUT="${PROBE_OUT:-/tmp/probe-out}"
LABEL="$1"
QUERY="$2"
shift 2
WIDTHS="${*:-1024 1280 1550}"

for w in $WIDTHS; do
  file="$OUT/$LABEL-$w.txt"
  rm -f "$file"
  "$CHROME" --headless=new --window-size="$w",900 --user-data-dir="/tmp/probe-ud-$w" \
    --no-first-run --disable-gpu --use-mock-keychain --disable-extensions \
    "http://127.0.0.1:8133/dev/probe/index.html?label=$LABEL-$w&$QUERY" \
    >/dev/null 2>&1 &
  pid=$!
  waited=0
  while [ "$waited" -lt 90 ]; do
    sleep 1
    waited=$((waited + 1))
    if [ -f "$file" ] && grep -q 'done=true' "$file"; then break; fi
  done
  kill -9 "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  echo "### $LABEL width=$w (${waited}s) $QUERY"
  if [ -f "$file" ]; then sed 's/^PROBE //' "$file"; else echo "NO RESULT"; fi
done
