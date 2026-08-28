#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Produce and upload a complete macOS release from this Mac: the signed app, the
# dmg, and the three files the in-app updater needs — `<app>.tar.gz`, its `.sig`,
# and `latest.json`.
#
# Why this exists rather than CI. The release workflow refuses to publish an
# unsigned build and demands seven Apple secrets, three of which are notarization
# credentials from a paid Developer Program this project does not have (decision
# D-1) — so that job can never pass, and every release is produced here. The
# missing `latest.json` is what makes the app say "Update failed: Could not fetch
# a valid release JSON from the remote": the updater fetches
# `releases/latest/download/latest.json` and gets GitHub's 404.
#
# RUN IT IN A GUI SESSION ON THE MAC — Terminal.app, not a bare ssh shell. Two
# steps reach into the login keychain and a non-GUI session cannot: `codesign`
# fails with `errSecInternalComponent`, and `gh` — whose token lives in the
# keyring — fails with `401 Unauthorized`. Driving the release from another
# machine therefore means landing the command *in* that GUI session:
#
#   ssh mac 'osascript -e '"'"'tell application "Terminal" to do script
#     "cd ~/keeper-check && scripts/release-macos.sh v0.6.5 2>&1 | tee /tmp/release.log"'"'"''
#
# then poll /tmp/release.log over ssh. That is how v0.6.5 was published.
#
# Usage:
#   scripts/release-macos.sh v0.6.5              # build, sign, upload everything
#   scripts/release-macos.sh v0.6.5 --no-upload  # build and sign only
#
# The updater's minisign private key comes from 1Password
# (`op://tg/keeper-updater-signing-key/credential`), so it is readable, rotatable
# and recoverable; its public half is committed in tauri.conf.json and therefore
# baked into every build. Override with TAURI_SIGNING_PRIVATE_KEY /
# TAURI_SIGNING_PRIVATE_KEY_PASSWORD if the key lives somewhere else.

set -euo pipefail

TAG="${1:?usage: release-macos.sh <tag> [--no-upload]}"
UPLOAD=1
[ "${2:-}" = "--no-upload" ] && UPLOAD=0
VERSION="${TAG#v}"

# Named, not inferred: this script is usually run from an rsync'd copy of the
# tree with no `.git` (see the ssh recipe above), where `gh` cannot work out the
# repository and reports the misleading "release not found" instead.
REPO="${KEEPER_RELEASE_REPO:-tgorka/keeper}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

BUNDLE="src-tauri/target/release/bundle"
APP="$BUNDLE/macos/keeper.app"
DMG="$BUNDLE/dmg/keeper_${VERSION}_aarch64.dmg"
TARBALL="$BUNDLE/macos/keeper_${VERSION}_aarch64.app.tar.gz"
MANIFEST="$BUNDLE/latest.json"
OP_KEY="op://tg/keeper-updater-signing-key/credential"
OP_PW="op://tg/keeper-updater-signing-key/password"

say() { printf '\033[1m==>\033[0m %s\n' "$*"; }
fail() { printf '\033[31mFAIL:\033[0m %s\n' "$*" >&2; exit 1; }

[ "$(uname -s)" = "Darwin" ] || fail "macOS only (host: $(uname -s))"

# The tag names the version; the manifests are the truth. Same check the release
# workflow makes, for the same reason: v0.4.0 once shipped assets named 0.4.0
# around an app that reported 0.3.0, and every updater check re-offered an update
# the machine already had.
CONF_VERSION="$(python3 -c "import json;print(json.load(open('src-tauri/crates/keeper/tauri.conf.json'))['version'])")"
[ "$VERSION" = "$CONF_VERSION" ] ||
  fail "tag $TAG disagrees with tauri.conf.json version $CONF_VERSION"
say "version $VERSION"

# --- The updater key, before a 20-minute build rather than after --------------
if [ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
  command -v op > /dev/null || fail "1Password CLI not found, and TAURI_SIGNING_PRIVATE_KEY is unset"
  say "reading the updater key from 1Password"
  # `--no-newline`: `op read` appends one, and minisign keys are exact strings.
  TAURI_SIGNING_PRIVATE_KEY="$(op read --no-newline "$OP_KEY")" ||
    fail "could not read $OP_KEY — is the CLI signed in?"
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(op read --no-newline "$OP_PW" 2>/dev/null || true)"
fi
export TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD

# --- Build, signed with a real certificate -----------------------------------
# `build-macos-signed.sh` resolves the login keychain's Apple Development
# identity, builds, and then VERIFIES the designated requirement is identity-based
# rather than a bare cdhash. That verification is the point: an ad-hoc signature
# changes identity on every build, so TCC grants and keychain items stop matching.
say "building (signed, app + dmg)"
# The shared build defaults to `--bundles app` because that is what an install
# wants; a release needs the disk image too and has to ask for it. Without this
# the build succeeds, takes its full time, and the check below fails on an
# artifact that was never requested.
KEEPER_BUNDLES=app,dmg bash "$SCRIPT_DIR/build-macos-signed.sh"
[ -f "$DMG" ] || fail "no dmg at $DMG"

# --- The updater payload -----------------------------------------------------
# A gzipped tar of the .app, which is the shape Tauri's macOS updater expects and
# the shape v0.4.2/v0.5.0/v0.6.1 shipped. Built from the bundle that was just
# verified, so the update installs a properly signed app rather than an ad-hoc one.
#
# `--no-mac-metadata` is load-bearing, and its absence is invisible from this
# machine. macOS `tar` stores extended attributes as sidecar AppleDouble members
# — `._keeper.app`, `keeper.app/._Contents`, one per directory — and then hides
# them again when *it* reads the archive, merging them back into the file they
# describe. So `tar tzf` here lists a clean tree while the archive really holds
# twenty members, ten of them `._`.
#
# Tauri's updater unpacks with the Rust `tar` crate, which does no such merging.
# It meets `._keeper.app` as an ordinary member and dies: "failed to unpack
# `._keeper.app` into …", which is what v0.8.8 shipped. The payload had been
# built this way since v0.4.2 and nobody had seen it, because until v0.6.5 the
# updater could not fetch `latest.json` at all — the fetch failure masked an
# unpack failure waiting behind it.
say "packing the updater payload"
rm -f "$TARBALL" "$TARBALL.sig"
tar --no-mac-metadata -czf "$TARBALL" -C "$(dirname "$APP")" "$(basename "$APP")"

# Proof, not intent: read the archive with something that does not merge
# AppleDouble members, because the tool that wrote them is exactly the tool that
# would hide them from this check.
say "checking the payload the way the updater will read it"
python3 - "$TARBALL" <<'PY'
import sys, tarfile
names = tarfile.open(sys.argv[1]).getnames()
bad = [n for n in names if n.startswith("._") or "/._" in n]
if bad:
    print(f"FAIL: {len(bad)} AppleDouble member(s) the updater cannot unpack, "
          f"starting with {bad[0]}", file=sys.stderr)
    sys.exit(1)
print(f"    {len(names)} members, no AppleDouble sidecars")
PY

say "signing it with the updater key"
bunx tauri signer sign --private-key "$TAURI_SIGNING_PRIVATE_KEY" \
  --password "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}" "$TARBALL" > /dev/null
[ -f "$TARBALL.sig" ] || fail "no signature at $TARBALL.sig"

# The manifest the app actually fetches. `platforms` keys are Tauri's target
# triples; this project ships Apple Silicon only.
say "composing latest.json"
python3 - "$VERSION" "$TAG" "$TARBALL.sig" "$MANIFEST" <<'PY'
import datetime, json, sys
version, tag, sig_path, out = sys.argv[1:5]
url = (f"https://github.com/tgorka/keeper/releases/download/{tag}/"
       f"keeper_{version}_aarch64.app.tar.gz")
manifest = {
    "version": version,
    "notes": f"https://github.com/tgorka/keeper/releases/tag/{tag}",
    "pub_date": datetime.datetime.now(datetime.timezone.utc)
    .isoformat(timespec="seconds")
    .replace("+00:00", "Z"),
    "platforms": {"darwin-aarch64": {"signature": open(sig_path).read().strip(), "url": url}},
}
open(out, "w").write(json.dumps(manifest, indent=2) + "\n")
PY
cat "$MANIFEST"

if [ "$UPLOAD" -eq 0 ]; then
  say "built and signed; not uploading"
  exit 0
fi

# --- Upload ------------------------------------------------------------------
# `--clobber` because a re-run must replace an asset rather than fail on it, and
# because a stale dmg beside fresh notes is how a release ships a lie.
say "uploading to $TAG"
gh release upload "$TAG" --repo "$REPO" "$DMG" "$TARBALL" "$TARBALL.sig" "$MANIFEST" --clobber ||
  fail "upload failed — is the release published and gh authenticated?"

say "verifying the endpoint the app fetches"
sleep 2
if curl -sSfL "https://github.com/$REPO/releases/latest/download/latest.json" |
  python3 -c "import json,sys; d=json.load(sys.stdin); print('latest.json says version', d['version'])"; then
  say "done"
else
  fail "latest.json is still not reachable — is $TAG the newest non-prerelease release?"
fi
