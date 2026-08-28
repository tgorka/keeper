#!/usr/bin/env bash
# End-to-end proof of Epic 56 virtual files through the SHIPPED keeper-syncd
# binary. No git-lfs CLI is used and none is needed: keeper owns
# `filter.lfs.process` in its own repositories, so an author clone driven by
# keeper produces the pointers and uploads the objects, exactly as a user's
# machine does.
#
#   author clone --keeper--> bare remote --keeper--> consumer clone (virtual policy)
#
# Proves, in order:
#   1. the consumer's authorized path arrives as POINTER TEXT and no object is
#      fetched onto that clone                                (FR-328, story 56.10)
#   2. a path below the size floor arrives as real content
#   3. the worktree is clean with a pointer standing in       (FR-331)
#   4. ls-files reports the honest size and the virtual state (story 56.2)
#   5. materialize lands real bytes on request, still clean   (story 56.3)
#   6. an OPEN descriptor refuses the release; closing it lets it through
#                                                             (56.4 + story 56.11)
set -uo pipefail

BIN="${1:?usage: vf-e2e.sh /path/to/keeper-syncd}"
ROOT="$(mktemp -d /tmp/vf-e2e.XXXXXX)"
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null
export GIT_AUTHOR_NAME=keeper GIT_AUTHOR_EMAIL=dev@keeper.local
export GIT_COMMITTER_NAME=keeper GIT_COMMITTER_EMAIL=dev@keeper.local
export XDG_CONFIG_HOME="$ROOT/config" XDG_DATA_HOME="$ROOT/data" XDG_STATE_HOME="$ROOT/state"

pass=0; fail=0
ok()   { printf '  \033[32mPASS\033[0m %s\n' "$1"; pass=$((pass+1)); }
bad()  { printf '  \033[31mFAIL\033[0m %s\n' "$1"; fail=$((fail+1)); }
step() { printf '\n== %s\n' "$1"; }
is_pointer() { head -c 45 "$1" 2>/dev/null | grep -q "version https://git-lfs"; }
quiet() { grep -vE "status walk finished|wrote the stat|merge-base|gix::|walk\{" ; }

step "fixture: one bare remote, one author clone, two LFS-tracked files"
git init -q --bare "$ROOT/remote.git"
mkdir -p "$ROOT/author/scans" "$ROOT/author/small" "$ROOT/work"
printf 'scans/*.bin filter=lfs diff=lfs merge=lfs -text\nsmall/*.bin filter=lfs diff=lfs merge=lfs -text\n' > "$ROOT/author/.gitattributes"
head -c 4194304 /dev/urandom > "$ROOT/author/scans/big.bin"    # 4 MiB, above the floor
head -c 65536   /dev/urandom > "$ROOT/author/small/tiny.bin"   # 64 KiB, below it
BIG_SHA=$(sha256sum "$ROOT/author/scans/big.bin" | cut -d' ' -f1)

step "config: written by keeper-syncd itself, policy patched onto the consumer"
"$BIN" init >/dev/null 2>&1
"$BIN" add --name author --path "$ROOT/author" --remote "$ROOT/remote.git" >/dev/null 2>&1 || { echo "add author failed"; exit 2; }
"$BIN" add --name media  --path "$ROOT/work"   --remote "$ROOT/remote.git" >/dev/null 2>&1 || { echo "add media failed"; exit 2; }
CFG="$XDG_CONFIG_HOME/keeper-sync/config.toml"
python3 - "$CFG" <<'PYEOF'
import sys, re
path = sys.argv[1]
blocks = open(path).read().split("[[profile]]")
out = [blocks[0]]
for b in blocks[1:]:
    b = re.sub(r"lfsThresholdBytes = \d+", "lfsThresholdBytes = 1048576", b)
    if 'name = "media"' in b:
        b = re.sub(r"virtualPatterns = \[\]", 'virtualPatterns = ["scans/**"]', b)
        b = re.sub(r"virtualOverBytes = \d+", "virtualOverBytes = 1048576", b)
        b = re.sub(r"releaseTtlMs = \d+", "releaseTtlMs = 60000", b)
    out.append(b)
open(path, "w").write("[[profile]]".join(out))
PYEOF
grep -E '^(name|virtualPatterns|virtualOverBytes|releaseTtlMs|settleMs|lfsThresholdBytes) =' "$CFG" | sed 's/^/    /'

step "author: prime the repository, then let keeper commit, clean and upload"
# The first pass only ADOPTS the folder and initializes the repository. A profile
# whose HEAD is unborn commits nothing and reports "0 file(s)" — a real,
# pre-existing defect, logged in deferred-work.md rather than tested here, so the
# fixture gives the repo the one root commit any real folder already has.
"$BIN" sync author --once >"$ROOT/author-init.log" 2>&1
git -C "$ROOT/author" commit -q --allow-empty -m "root"
# keeper's stability gate: a file must be observed by one scan and then stay
# unchanged for settleMs (5 s by default here) before a pass may commit it.
for i in 1 2 3 4; do
  sleep 6
  "$BIN" sync author --once >"$ROOT/author.log" 2>&1
  git -C "$ROOT/author" ls-files --error-unmatch scans/big.bin >/dev/null 2>&1 && break
done
echo "  passes=$i"
quiet < "$ROOT/author.log" | tail -2 | sed 's/^/    /'
git -C "$ROOT/author" ls-files scans small | sed 's/^/    tracked: /'

step "consumer: sync with the policy in force"
for i in 1 2 3; do
  "$BIN" sync media --once >"$ROOT/sync1.log" 2>&1
  [ -e "$ROOT/work/scans/big.bin" ] && break
  sleep 2
done
echo "  passes=$i"
quiet < "$ROOT/sync1.log" | tail -2 | sed 's/^/    /'

step "1. the authorized path arrived as POINTER TEXT, and no object came with it"
if is_pointer "$ROOT/work/scans/big.bin"; then
  ok "scans/big.bin holds pointer text — FR-328, the half that did not exist before story 56.10"
  head -3 "$ROOT/work/scans/big.bin" | sed 's/^/    /'
else
  bad "scans/big.bin is $(stat -c%s "$ROOT/work/scans/big.bin" 2>/dev/null || echo 'missing') bytes, not pointer text"
fi
objs=$(find "$ROOT/work/.git/lfs/objects" -type f 2>/dev/null | wc -l)
if [ "$objs" -eq 0 ]; then ok "no LFS object on this clone (.git/lfs/objects empty)"
else bad "$objs object(s) fetched despite the policy"; fi

step "2. the path below the size floor arrived as real content"
sz=$(stat -c%s "$ROOT/work/small/tiny.bin" 2>/dev/null || echo 0)
if [ "$sz" = "65536" ]; then ok "small/tiny.bin is real content ($sz bytes)"
else bad "small/tiny.bin is $sz bytes, expected 65536"; fi

step "3. the worktree is CLEAN with a pointer standing in for content"
cd "$ROOT/work" || exit 1
p=$(git status --porcelain | head -5)
if [ -z "$p" ]; then ok "git status is clean (FR-331)"; else bad "dirty: $p"; fi

step "4. ls-files reports the honest size and the state"
"$BIN" ls-files media --json > "$ROOT/ls1.json" 2>"$ROOT/ls1.err" || true
if command -v jq >/dev/null && [ -s "$ROOT/ls1.json" ]; then
  jq -c '.. | objects | select(has("path")) | {path, state, size: .sizeBytes}' "$ROOT/ls1.json" 2>/dev/null | sed 's/^/    /'
  big=$(jq -r '.. | objects | select(has("path")) | select(.path|test("big")) | "\(.state) \(.sizeBytes)"' "$ROOT/ls1.json" 2>/dev/null | head -1)
  case "$big" in
    virtual*4194304) ok "big.bin: state=virtual, size=4194304 — the pointer's size, not ~130 bytes" ;;
    "") bad "no row for big.bin" ;;
    *)  bad "big.bin reported as '$big'" ;;
  esac
else
  head -5 "$ROOT/ls1.err" | sed 's/^/    /'; bad "ls-files --json produced nothing"
fi

step "5. materialize lands real bytes on request, and the path stays clean"
"$BIN" materialize media scans/big.bin >"$ROOT/mat.log" 2>&1; echo "  rc=$?"
quiet < "$ROOT/mat.log" | tail -2 | sed 's/^/    /'
now=$(sha256sum "$ROOT/work/scans/big.bin" 2>/dev/null | cut -d' ' -f1)
if [ "$now" = "$BIG_SHA" ]; then ok "content is byte-identical to what the author wrote"
else bad "content differs (${now:0:16}… vs ${BIG_SHA:0:16}…)"; fi
p=$(git status --porcelain | head -3)
if [ -z "$p" ]; then ok "git status still clean after materialize"; else bad "dirty after materialize: $p"; fi

step "6. an OPEN descriptor refuses the release; closing it lets it through"
exec 9<"$ROOT/work/scans/big.bin"
out=$("$BIN" dehydrate media scans/big.bin 2>&1); rc=$?
printf '%s\n' "$out" | quiet | tail -2 | sed 's/^/    /'
if [ $rc -ne 0 ] && printf '%s' "$out" | grep -qi "open"; then
  ok "refused while a descriptor is held — story 56.11's /proc probe answering, not Unknown"
else
  bad "did not refuse for an open file (rc=$rc)"
fi
exec 9<&-
out=$("$BIN" dehydrate media scans/big.bin 2>&1); rc=$?
printf '%s\n' "$out" | quiet | tail -2 | sed 's/^/    /'
if [ $rc -eq 0 ] && is_pointer "$ROOT/work/scans/big.bin"; then
  ok "released once closed; the path is the committed pointer again"
else
  bad "release failed after closing the descriptor (rc=$rc)"
fi
p=$(git status --porcelain | head -3)
if [ -z "$p" ]; then ok "git status clean after release (no MODIFIED-forever — DW-140's shape)"
else bad "dirty after release: $p"; fi

printf '\n== %d passed, %d failed\n' "$pass" "$fail"
echo "   tree kept: $ROOT"
[ "$fail" -eq 0 ]
