#!/usr/bin/env bash
# Shared by CI and the on-disk red/green acceptance run (--ci).
set -euo pipefail
cd "$(dirname "$0")/.."
bun run check:core-frozen
bun run check:module-edges
bun run check:core-tauri-free
bun run check:core-sync-free
bun run check:syncd-lean
bun run check:paths-pure
bun run check:ported-pure
bun run check:llm-lean
bun run check:agent-tauri-free
bun run check:nse-lean
bun run check:graph-protocol-lean
bun run check:one-sqlite
bun run check:graph-boundary
