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
# - The EXIT trap acts only on resources registered in the run's ledger. A preload path is
#   registered (`intent`) before its exclusive creation and then by lstat identity
#   (`file`); units by exact name before systemd-run. A path with another identity, or
#   with only an intent and not our exact signature, is left alone and fails the gate.
# - Cleanup ignores further signals, restores files before touching units, and runs every
#   helper under a SIGKILL deadline.
# - The owned-resource gate checks the exact allowed set at the end; any leftover,
#   foreign or unrecorded resource, or a failed observation (census, unit query, mounts,
#   rootfs digest), fails the transcript.
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
LINK_TARGET=/nonexistent/kg1-1101-preload-target

refuse() { printf 'TRANSCRIPT result=REFUSED reason=%s\n' "$1" >&2; exit 3; }

# Every external helper runs under a hard deadline. SIGKILL, because cleanup ignores
# TERM and the ignored disposition is inherited by its children.
t() { local secs=$1; shift; timeout -s KILL "$secs" "$@"; }

kg1_units() {
  # Loaded kg1-1101 units; a failed or timed-out query is an error, never an empty list.
  local listing
  listing=$(t 10 systemctl list-units --all --no-legend --plain 'kg1-1101-*') || return 1
  printf '%s\n' "$listing" | awk 'NF {print $1}'
}

tree_digest() {
  # Everything in the rootfs except the evidence directory: type, mode and path for all
  # entries, plus size and mtime for non-directories. Fails (nonzero) rather than guessing.
  # shellcheck disable=SC2016
  t 300 bash -c 'set -o pipefail; cd "$1" && { find . -path ./bundle/out -prune -o -printf "%y %m %p\n" | sort &&
     find . -path ./bundle/out -prune -o ! -type d -printf "%s %T@ %p\n" | sort; } | sha256sum | cut -d" " -f1' bash "$B"
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
mounts_before=$(t 30 findmnt -rn | sort | sha256sum | cut -d' ' -f1) || refuse 'findmnt-failed'
df_before=$(t 30 df -B1 --output=used "$B" | tail -1 | tr -d ' ') || refuse 'df-failed'
etc_mode=$(stat -c %a "$B/etc")
tree_before=$(tree_digest) || refuse 'rootfs-digest-failed'

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

census() {
  # Every live, non-zombie process whose root, executable or cwd lies inside the bundle
  # (OWNED), or which exists but could not be inspected (UNREADABLE). A kernel thread has
  # no executable. The census ends with CENSUS_OK; a missing sentinel is a failed census.
  # shellcheck disable=SC2016
  t 60 sudo -n sh -c 'bundle=$1; cd / || exit 1
    for d in /proc/[0-9]*; do
      p=${d#/proc/}
      st=$(cat "$d/stat" 2>/dev/null) || continue
      [ -n "$st" ] || continue
      # shellcheck disable=SC2086
      set -- ${st##*) }
      [ "$1" = Z ] && continue
      kthread=$(( $7 & 0x200000 ))
      for l in root exe cwd; do
        if tgt=$(readlink "$d/$l" 2>/dev/null); then
          case "$tgt" in "$bundle"|"$bundle"/*) echo "OWNED $p $l $tgt"; break;; esac
        elif [ -e "$d" ] && ! { [ "$l" = exe ] && [ "$kthread" -ne 0 ]; }; then
          echo "UNREADABLE $p $l"
        fi
      done
    done
    echo CENSUS_OK' sh "$B"
}

process_gate() {
  # PASS only on a completed census with no owned and no uninspectable process. This
  # transcript and its ancestors (the shells that started it) are excluded by pid.
  local out status skip line bad=
  skip=" $(ancestors | tr '\n' ' ') $BASHPID "
  out=$(census); status=$?
  if [ "$status" -ne 0 ] || [ "$(printf '%s\n' "$out" | tail -n 1)" != CENSUS_OK ]; then
    log "GATE no_owned_processes=FAIL census-failed status=$status"; return 1
  fi
  while IFS= read -r line; do
    case "$line" in
      CENSUS_OK|'') ;;
      OWNED\ *) set -- $line; case "$skip" in *" $2 "*) ;; *) bad="$bad,$line";; esac;;
      *) bad="$bad,$line";;
    esac
  done <<<"$out"
  if [ -n "$bad" ]; then log "GATE no_owned_processes=FAIL ${bad#,}"; return 1; fi
  log 'GATE no_owned_processes=PASS'
}

is_signature() {
  # The exact entry a case creates: an empty regular file, or our dangling link.
  if [ -L "$1" ]; then [ "$(readlink "$1")" = "$LINK_TARGET" ]; else [ -f "$1" ] && [ ! -s "$1" ]; fi
}

remove_owned() {
  if [ "$1" = "$HOST_PRELOAD" ]; then t 30 sudo -n rm -f "$1"; else rm -f "$1"; fi
}

cleanup() {
  local status=$? gate=PASS kind f1 f2 f3 id now listing f deadline u
  local -a units=()
  local -A last_kind=() last_id=()
  set +e
  trap '' INT TERM HUP  # cleanup always runs to its end; further signals are ignored
  [ "$rc" -ne 0 ] || rc=$status
  [ -z "$signalled" ] || log "SIGNAL $signalled stage_stopped=yes resumed=no"
  [ "$(stat -c %a "$B/etc")" = "$etc_mode" ] || { chmod "$etc_mode" "$B/etc"; log "RESTORE private_etc_mode=$etc_mode"; }
  # Ledger: what this run registered, and nothing else, is acted upon. Per path, the last
  # entry wins: `intent` (registered before the exclusive creation) or `file` (identity).
  touch "$LEDGER"
  while read -r kind f1 f2 f3; do
    case "$kind" in
      unit)
        if [[ $f1 =~ $UNIT_RE ]]; then units+=("$f1"); else log "GATE ledger_unit_name=FAIL $f1"; gate=FAIL; fi;;
      intent|file)
        f=$f1; [ "$kind" = file ] && f=$f3
        if [ "$f" != "$HOST_PRELOAD" ] && [ "$f" != "$PRIVATE_PRELOAD" ]; then log "GATE ledger_path_allowed=FAIL $f"; gate=FAIL; continue; fi
        last_kind[$f]=$kind; last_id[$f]="$f1 $f2";;
      *) log "GATE ledger_line=FAIL $kind $f1 $f2 $f3"; gate=FAIL;;
    esac
  done <"$LEDGER"
  # 1. Host and private files FIRST, so no unit helper can block their restoration.
  for f in "${!last_kind[@]}"; do
    [ -e "$f" ] || [ -L "$f" ] || continue
    if [ "${last_kind[$f]}" = file ]; then
      id=$(stat -c '%d %i' "$f")
      if [ "$id" != "${last_id[$f]}" ]; then log "GATE file_identity=FAIL $f recorded=${last_id[$f]/ /:} now=${id/ /:} kept=yes"; gate=FAIL; continue; fi
    elif ! is_signature "$f"; then
      # Registered intent, no identity yet: absent at preflight and created exclusively,
      # so only our exact signature is ours.
      log "GATE intent_signature=FAIL $f kept=yes"; gate=FAIL; continue
    fi
    if remove_owned "$f" && [ ! -e "$f" ] && [ ! -L "$f" ]; then log "OWNED file=$f via=${last_kind[$f]} removed=yes"
    else log "GATE file_removed=FAIL $f"; gate=FAIL; fi
  done
  # 2. Units, each helper bounded, all of them within one 60 s deadline.
  deadline=$((SECONDS + 60))
  for u in "${units[@]}"; do
    t 10 sudo -n systemctl kill --signal=SIGKILL "$u.scope" >/dev/null 2>&1
    now=
    while [ "$SECONDS" -lt "$deadline" ]; do
      now=$(t 5 systemctl show -p LoadState --value "$u.scope" 2>/dev/null) && [ "$now" = not-found ] && break
      now=
      sleep 0.5
    done
    if [ "$now" = not-found ]; then log "OWNED unit=$u gone=yes"; else log "GATE unit_gone=FAIL $u state=${now:-query-failed-or-deadline}"; gate=FAIL; fi
  done
  # 3. Owned-resource gate: the exact allowed set is the evidence directory; every
  # transient resource must be gone and nothing else may have changed.
  for f in "$HOST_PRELOAD" "$PRIVATE_PRELOAD"; do
    if [ -e "$f" ] || [ -L "$f" ]; then log "GATE preload_absent=FAIL $f"; gate=FAIL; else log "GATE preload_absent=PASS $f"; fi
  done
  if listing=$(kg1_units); then
    if [ -z "$listing" ]; then log 'GATE no_kg1_units=PASS'; else log "GATE no_kg1_units=FAIL $(printf '%s' "$listing" | tr '\n' ,)"; gate=FAIL; fi
  else
    log 'GATE no_kg1_units=FAIL query-failed'; gate=FAIL
  fi
  process_gate || gate=FAIL
  if [ "$(stat -c %a "$B/etc")" = "$etc_mode" ]; then log 'GATE private_etc_mode=PASS'; else log 'GATE private_etc_mode=FAIL'; gate=FAIL; fi
  if now=$(t 30 findmnt -rn) && [ "$(printf '%s\n' "$now" | sort | sha256sum | cut -d' ' -f1)" = "$mounts_before" ]; then log 'GATE mounts_unchanged=PASS'
  else log 'GATE mounts_unchanged=FAIL'; gate=FAIL; fi
  if now=$(tree_digest) && [ "$now" = "$tree_before" ]; then log 'GATE rootfs_unchanged_outside_out=PASS'
  else log 'GATE rootfs_unchanged_outside_out=FAIL'; gate=FAIL; fi
  log "DF used_before=$df_before used_after=$(t 30 df -B1 --output=used "$B" | tail -1 | tr -d ' ')"
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
