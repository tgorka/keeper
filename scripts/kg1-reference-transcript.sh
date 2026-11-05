#!/usr/bin/env bash
# Story 110.1 reference-run outer transcript, run ON delectra inside the shipped bundle:
#   bash <rootfs>/bundle/scripts/kg1-reference-transcript.sh <rootfs>
# Proof-only; never shipped.
#
# - Refuses, before creating anything, if a host or private ld.so.preload exists in any
#   form (a dangling symlink included), the evidence directory is not empty, a kg1-1101 unit is
#   loaded, or the bundle does not match its SHA256SUMS.
# - Stops at the first failed stage (set -e). A signal stops the running stage and the
#   transcript; nothing resumes.
# - The EXIT trap acts only on resources recorded in the run's ledger: units by exact
#   name, files by lstat identity. A path that is no longer the recorded file is left
#   alone and fails the gate.
# - The owned-resource gate checks the exact allowed set at the end; any leftover,
#   foreign or unrecorded resource fails the transcript.
#
# KG1_TEST_HOST_ETC exists only for the local self-test; real evidence records host_etc=/etc.
set -euo pipefail

B=$(cd "${1:?usage: kg1-reference-transcript.sh <rootfs>}" && pwd -P)
cd /  # no shell of this transcript keeps a cwd inside the bundle
HOST_ETC=${KG1_TEST_HOST_ETC:-/etc}
OUT=$B/bundle/out
LEDGER=$OUT/owned.ledger
LOG=$OUT/transcript.log
R=$B/bundle/scripts/graph-reference-run.py
HOST_PRELOAD=$HOST_ETC/ld.so.preload
PRIVATE_PRELOAD=$B/etc/ld.so.preload
UNIT_RE='^kg1-1101-(reference|preload-host|preload-chroot)-[0-9]+$'

refuse() { printf 'TRANSCRIPT result=REFUSED reason=%s\n' "$1" >&2; exit 3; }

kg1_units() {
  # Loaded kg1-1101 units; a failed query is an error, never an empty list.
  local listing
  listing=$(systemctl list-units --all --no-legend --plain 'kg1-1101-*') || return 1
  printf '%s\n' "$listing" | awk 'NF {print $1}'
}

tree_digest() {
  # Everything in the rootfs except the evidence directory: type, mode and path for all
  # entries, plus size and mtime for non-directories.
  (cd "$B" && find . -path ./bundle/out -prune -o -printf '%y %m %p\n' | sort &&
     find . -path ./bundle/out -prune -o ! -type d -printf '%s %T@ %p\n' | sort) | sha256sum | cut -d' ' -f1
}

# ---------------------------------------------------------------- preflight (creates nothing)
for p in "$HOST_PRELOAD" "$PRIVATE_PRELOAD"; do
  if [ -e "$p" ] || [ -L "$p" ]; then refuse "preexisting:$p"; fi
done
# The bundle ships bundle/out as an empty directory; anything else is a previous run.
if [ -L "$OUT" ] || [ ! -d "$OUT" ] || [ -n "$(ls -A "$OUT")" ]; then refuse "evidence-dir-not-empty:$OUT"; fi
units=$(kg1_units) || refuse 'systemctl-list-units-failed'
[ -z "$units" ] || refuse "preexisting-units:$(printf '%s' "$units" | tr '\n' ,)"
(cd "$B" && sha256sum -c --quiet --strict bundle/SHA256SUMS >/dev/null) || refuse 'bundle-checksum-mismatch'
mounts_before=$(findmnt -rn | sort | sha256sum | cut -d' ' -f1) || refuse 'findmnt-failed'
df_before=$(df -B1 --output=used "$B" | tail -1 | tr -d ' ')
etc_mode=$(stat -c %a "$B/etc")
tree_before=$(tree_digest)

# ---------------------------------------------------------------- owned state from here on
stage_pid=
signalled=
rc=0

log() { printf '%s\n' "$*" >>"$LOG"; printf '%s\n' "$*" 2>/dev/null || true; }

ancestors() {
  local pid=$$
  while [ "$pid" -gt 1 ] 2>/dev/null; do
    printf '%s\n' "$pid"
    pid=$(awk '{print $4}' "/proc/$pid/stat" 2>/dev/null) || break
  done
}

owned_processes() {
  # Processes whose root, executable or cwd lies inside the bundle, other than this
  # transcript and its ancestors (the shells that started it).
  local skip
  skip=" $(ancestors | tr '\n' ' ') $BASHPID "
  # shellcheck disable=SC2016
  sudo -n sh -c 'cd / && for d in /proc/[0-9]*; do
      p=${d#/proc/}
      for l in root exe cwd; do
        t=$(readlink "$d/$l" 2>/dev/null) || continue
        case "$t" in "$1"|"$1"/*) echo "$p:$l:$t"; break;; esac
      done
    done' sh "$B" | while IFS= read -r line; do
      case "$skip" in *" ${line%%:*} "*) ;; *) printf '%s\n' "$line";; esac
    done
}

cleanup() {
  local status=$? gate=PASS kind f1 f2 f3 id now n listing f
  set +e
  trap '' INT TERM HUP
  [ "$rc" -ne 0 ] || rc=$status
  [ -z "$signalled" ] || log "SIGNAL $signalled stage_stopped=yes resumed=no"
  [ "$(stat -c %a "$B/etc")" = "$etc_mode" ] || { chmod "$etc_mode" "$B/etc"; log "RESTORE private_etc_mode=$etc_mode"; }
  # Ledger: what this run created, and nothing else, is acted upon.
  touch "$LEDGER"
  while read -r kind f1 f2 f3; do
    case "$kind" in
      unit)
        if ! [[ $f1 =~ $UNIT_RE ]]; then log "GATE ledger_unit_name=FAIL $f1"; gate=FAIL; continue; fi
        sudo -n systemctl kill --signal=SIGKILL "$f1.scope" >/dev/null 2>&1
        n=0
        while :; do
          now=$(systemctl show -p LoadState --value "$f1.scope" 2>/dev/null) && [ "$now" = not-found ] && break
          n=$((n + 1)); [ "$n" -lt 60 ] || break; sleep 0.5
        done
        if [ "$now" = not-found ]; then log "OWNED unit=$f1 gone=yes"; else log "GATE unit_gone=FAIL $f1 state=${now:-query-failed}"; gate=FAIL; fi
        ;;
      file)
        if [ "$f3" != "$HOST_PRELOAD" ] && [ "$f3" != "$PRIVATE_PRELOAD" ]; then
          log "GATE ledger_file_allowed=FAIL $f3"; gate=FAIL; continue
        fi
        if [ -e "$f3" ] || [ -L "$f3" ]; then
          id=$(stat -c '%d %i' "$f3")
          if [ "$id" = "$f1 $f2" ]; then
            if [ "$f3" = "$HOST_PRELOAD" ]; then sudo -n rm -f "$f3"; else rm -f "$f3"; fi
            log "OWNED file=$f3 removed=yes"
          else
            log "GATE file_identity=FAIL $f3 recorded=$f1:$f2 now=${id/ /:} kept=yes"; gate=FAIL
          fi
        fi
        ;;
      *) log "GATE ledger_line=FAIL $kind $f1 $f2 $f3"; gate=FAIL;;
    esac
  done <"$LEDGER"
  # Owned-resource gate: the exact allowed set is the evidence directory; every
  # transient resource must be gone and nothing else may have changed.
  for f in "$HOST_PRELOAD" "$PRIVATE_PRELOAD"; do
    if [ -e "$f" ] || [ -L "$f" ]; then log "GATE preload_absent=FAIL $f"; gate=FAIL; else log "GATE preload_absent=PASS $f"; fi
  done
  if listing=$(kg1_units); then
    if [ -z "$listing" ]; then log 'GATE no_kg1_units=PASS'; else log "GATE no_kg1_units=FAIL $(printf '%s' "$listing" | tr '\n' ,)"; gate=FAIL; fi
  else
    log 'GATE no_kg1_units=FAIL query-failed'; gate=FAIL
  fi
  listing=$(owned_processes)
  if [ -z "$listing" ]; then log 'GATE no_owned_processes=PASS'; else log "GATE no_owned_processes=FAIL $(printf '%s' "$listing" | tr '\n' ,)"; gate=FAIL; fi
  if [ "$(stat -c %a "$B/etc")" = "$etc_mode" ]; then log 'GATE private_etc_mode=PASS'; else log 'GATE private_etc_mode=FAIL'; gate=FAIL; fi
  now=$(findmnt -rn | sort | sha256sum | cut -d' ' -f1)
  if [ "$now" = "$mounts_before" ]; then log 'GATE mounts_unchanged=PASS'; else log 'GATE mounts_unchanged=FAIL'; gate=FAIL; fi
  now=$(tree_digest)
  if [ "$now" = "$tree_before" ]; then log 'GATE rootfs_unchanged_outside_out=PASS'; else log 'GATE rootfs_unchanged_outside_out=FAIL'; gate=FAIL; fi
  log "DF used_before=$df_before used_after=$(df -B1 --output=used "$B" | tail -1 | tr -d ' ')"
  if [ "$gate" = FAIL ] && [ "$rc" -eq 0 ]; then rc=8; fi
  log "TRANSCRIPT result=$([ "$rc" -eq 0 ] && echo PASS || echo FAIL) rc=$rc gate=$gate"
  exit "$rc"
}

on_signal() {
  set +e  # nothing in the signal path may abort before the stage is stopped and rc is set
  signalled=$1
  trap '' INT TERM HUP
  rc=$((128 + $2))
  if [ -n "$stage_pid" ]; then
    kill -TERM "$stage_pid" 2>/dev/null
    local n=0
    while kill -0 "$stage_pid" 2>/dev/null && [ "$n" -lt 240 ]; do sleep 0.5; n=$((n + 1)); done
    kill -KILL "$stage_pid" 2>/dev/null
    wait "$stage_pid" 2>/dev/null
  fi
  exit "$rc"
}

trap cleanup EXIT
trap 'on_signal INT 2' INT
trap 'on_signal TERM 15' TERM
trap 'on_signal HUP 1' HUP
[ -z "$(ls -A "$OUT")" ]
: >"$LEDGER"
log "TRANSCRIPT start host_etc=$HOST_ETC rootfs=$B mounts=$mounts_before etc_mode=$etc_mode tree=$tree_before"

stage() {
  local name=$1
  log "STAGE $name start"
  # Background + wait, so a signal is handled at once rather than after the stage.
  (cd "$B" && exec env KG1_OWNED_LEDGER="$LEDGER" PYTHONDONTWRITEBYTECODE=1 python3 "$R" "$name" "$OUT/$name" \
     >"$OUT/$name.stdout" 2>"$OUT/$name.stderr") &
  stage_pid=$!
  local status=0
  wait "$stage_pid" || status=$?
  stage_pid=
  log "STAGE $name exit=$status"
  if [ "$status" -ne 0 ]; then rc=$status; exit "$status"; fi
}

stage preflight
stage preload-chroot
stage preload-host
stage reference
