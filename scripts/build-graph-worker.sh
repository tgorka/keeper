#!/usr/bin/env bash
# Build the graph worker exactly as it ships and the headless driver bound to it
# (story 110.5; design v7 §10). Fails closed at every step.
#
#   1. `cargo build --locked --profile graph-worker -p keeper-graph-query --bins`
#      (`panic = "abort"`; no features, no examples exist);
#   2. the abort path: the worker imports none of `_Unwind_RaiseException`,
#      `_Unwind_ForcedUnwind`, `_Unwind_Resume_or_Rethrow`, `_Unwind_DeleteException`,
#      and panic_abort's `__rust_start_panic`/`__rust_abort` are linked;
#   3. `target/graph-worker/keeper-graph-query.sha256`;
#   4. the driver, with that digest embedded (`KEEPER_GRAPH_WORKER_SHA256`) and, when
#      `KEEPER_GRAPH_RUNTIME_LEDGER` names one, the runtime ledger.
#
# The output names the artifact by path and digest. A worker built any other way
# (plain `--release`, another profile) has another digest and the driver refuses it.
set -euo pipefail
cd "$(dirname "$0")/../src-tauri"

cargo build --locked --profile graph-worker -p keeper-graph-query --bins
worker="target/graph-worker/keeper-graph-query"
[ -x "$worker" ] || { echo "no worker at $worker" >&2; exit 1; }

case "$(uname -s)" in
  Linux)
    imports=$(nm -D --undefined-only "$worker" | awk '{print $NF}')
    symbols=$(nm "$worker" 2>/dev/null || true)
    ;;
  Darwin)
    imports=$(nm -u "$worker" | sed 's/^_//')
    symbols=$(nm "$worker" | sed 's/ _/ /')
    ;;
  *) echo "unsupported host" >&2; exit 1 ;;
esac
for unwind in _Unwind_RaiseException _Unwind_ForcedUnwind _Unwind_Resume_or_Rethrow _Unwind_DeleteException; do
  if grep -qx "$unwind" <<<"$imports"; then
    echo "the worker imports $unwind: it can unwind" >&2
    exit 1
  fi
done
# panic_abort's `__rust_start_panic` aborts through `__rust_abort`; both are present
# and no unwinder entry above is imported.
grep -q '__rust_start_panic' <<<"$symbols" || { echo "no __rust_start_panic in the worker" >&2; exit 1; }
grep -q '__rust_abort' <<<"$symbols" || { echo "no __rust_abort in the worker" >&2; exit 1; }

if command -v sha256sum >/dev/null; then
  digest=$(sha256sum "$worker" | cut -d' ' -f1)
else
  digest=$(shasum -a 256 "$worker" | cut -d' ' -f1)
fi
printf '%s\n' "$digest" > "$worker.sha256"

KEEPER_GRAPH_WORKER_SHA256="$digest" cargo build --locked --profile graph-worker -p keeper-graph-driver --bins
echo "worker $(pwd)/$worker sha256 $digest"
echo "driver $(pwd)/target/graph-worker/keeper-graph-driver"
