#!/usr/bin/env bash
# Story 110.1 reference run, driven from superset-host (proof-only; never shipped):
#   scripts/kg1-reference-delectra.sh <local bundle rootfs> <new local evidence dir>
# 1. ship: refuse an existing remote directory; create it, record its identity, unpack and
#    verify the bundle's SHA256SUMS against the local copy.
# 2. run: the remote kg1-reference-transcript.sh (its exit is the run's verdict).
# 3. collect: hash the remote evidence remotely, copy it back, verify every hash locally.
# 4. remove: only after a verified collection, and only the directory recorded in step 1.
# A signal stops here; nothing resumes and nothing on delectra is removed.
# KG1_REMOTE (default `ssh delectra`) and KG1_REMOTE_DIR exist for the local self-test.
set -euo pipefail

SRC=$(cd "${1:?usage: kg1-reference-delectra.sh <bundle rootfs> <evidence dir>}" && pwd -P)
EVID=${2:?usage: kg1-reference-delectra.sh <bundle rootfs> <evidence dir>}
read -r -a REMOTE <<<"${KG1_REMOTE:-ssh delectra}"
DIR=${KG1_REMOTE_DIR:-kg1-1101-ref}
[[ $DIR =~ ^[A-Za-z0-9._-]+$ ]] || { echo "bad remote dir $DIR" >&2; exit 2; }
remote() { "${REMOTE[@]}" "$1"; }

trap 'echo "ORCHESTRATOR signal=INT stopped resumed=no remote_kept=$DIR" >&2; exit 130' INT
trap 'echo "ORCHESTRATOR signal=TERM stopped resumed=no remote_kept=$DIR" >&2; exit 143' TERM
trap 'echo "ORCHESTRATOR signal=HUP stopped resumed=no remote_kept=$DIR" >&2; exit 129' HUP

[ ! -e "$EVID" ] && [ ! -L "$EVID" ] || { echo "evidence dir $EVID exists" >&2; exit 2; }
mkdir -p "$EVID"
note() { printf '%s\n' "$*" | tee -a "$EVID/orchestrator.log"; }

# 1. ship
[ "$(basename "$SRC")" = rootfs ] || { echo "bundle path must end in /rootfs" >&2; exit 2; }
remote "test ! -e $DIR && test ! -L $DIR && mkdir $DIR"
identity=$(remote "stat -c '%d %i' $DIR")
note "SHIP remote_dir=$DIR identity=$identity"
tar -C "$(dirname "$SRC")" -cf - rootfs | remote "tar -C $DIR -xpf -"
shipped=$(remote "cd $DIR/rootfs && sha256sum -c --quiet --strict bundle/SHA256SUMS >/dev/null && sha256sum bundle/SHA256SUMS")
[ "${shipped%% *}" = "$(sha256sum "$SRC/bundle/SHA256SUMS" | cut -d' ' -f1)" ] || { note 'SHIP verify=FAIL'; exit 4; }
note "SHIP verify=PASS sums=${shipped%% *}"

# 2. run
status=0
remote "bash $DIR/rootfs/bundle/scripts/kg1-reference-transcript.sh \"\$(cd $DIR/rootfs && pwd -P)\"" || status=$?
note "RUN transcript_exit=$status"

# 3. collect and verify
collected=FAIL
if remote "cd $DIR/rootfs/bundle && if [ -d out ]; then find out -type f -print0 | sort -z | xargs -0 -r sha256sum; fi" >"$EVID/remote.sha256"; then
  if remote "cd $DIR/rootfs/bundle && tar -cf - SHA256SUMS TOPOLOGY.json rootfs-manifest.json \$(test -d out && echo out)" | tar -C "$EVID" -xf -; then
    local_count=$( (cd "$EVID" && { [ -d out ] && find out -type f || true; }) | wc -l)
    if (cd "$EVID" && sha256sum -c --quiet --strict remote.sha256) && [ "$local_count" -eq "$(wc -l <"$EVID/remote.sha256")" ]; then
      collected=PASS
    fi
  fi
fi
note "COLLECT verify=$collected files=$(wc -l <"$EVID/remote.sha256")"
(cd "$EVID" && find . -type f ! -name orchestrator.log -print0 | sort -z | xargs -0 sha256sum) >"$EVID/LOCAL.sha256"

# 4. remove, only after a verified copy, only the directory recorded at ship time
if [ "$collected" = PASS ]; then
  if remote "test \"\$(stat -c '%d %i' $DIR)\" = '$identity' && rm -rf $DIR && test ! -e $DIR"; then
    note "REMOVE remote_dir=$DIR removed=yes"
  else
    note "REMOVE remote_dir=$DIR removed=no reason=identity-or-removal-failed"
    [ "$status" -ne 0 ] || status=10
  fi
else
  note "REMOVE remote_dir=$DIR removed=no reason=collection-not-verified"
  [ "$status" -ne 0 ] || status=9
fi
note "ORCHESTRATOR exit=$status"
exit "$status"
