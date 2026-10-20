#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Build keeper for Android, sign the APK with the owner's keystore, and install
# it on the device `adb` sees (Epic 98, story 98.3; docs/android.md § Sideload).
#
# Runs where the Android SDK, the NDK, a JDK and the keystore are — never in CI,
# which builds only the unsigned debug APK. The keystore never enters this
# repository; it comes from the environment:
#
#   KEEPER_ANDROID_KEYSTORE           path to the keystore file
#   KEEPER_ANDROID_KEYSTORE_PASSWORD  its password (read by apksigner from the
#                                     environment, never put on a command line)
#   KEEPER_ANDROID_KEY_ALIAS          the key's alias in it
#
# Also needs ANDROID_HOME (build-tools with zipalign and apksigner, and
# platform-tools with adb) and NDK_HOME, as `tauri android build` does.
#
# Options:
#   --no-install   build and sign, print the APK's path, skip adb
#   --serial <id>  the adb device to install on when several are attached
set -euo pipefail

say() { printf '\033[1m==>\033[0m %s\n' "$*"; }
fail() { printf '\033[31mFAIL:\033[0m %s\n' "$*" >&2; exit 1; }

INSTALL=1
SERIAL=""
while [ $# -gt 0 ]; do
  case "$1" in
    --no-install) INSTALL=0 ;;
    --serial) shift; SERIAL="${1:?--serial needs a device id}" ;;
    *) fail "unknown option: $1" ;;
  esac
  shift
done

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GEN="$REPO_ROOT/src-tauri/crates/keeper/gen/android"

: "${KEEPER_ANDROID_KEYSTORE:?set KEEPER_ANDROID_KEYSTORE to the keystore file (docs/android.md)}"
: "${KEEPER_ANDROID_KEYSTORE_PASSWORD:?set KEEPER_ANDROID_KEYSTORE_PASSWORD (docs/android.md)}"
: "${KEEPER_ANDROID_KEY_ALIAS:?set KEEPER_ANDROID_KEY_ALIAS (docs/android.md)}"
: "${ANDROID_HOME:?set ANDROID_HOME to the Android SDK}"
: "${NDK_HOME:?set NDK_HOME to the Android NDK}"
[ -f "$KEEPER_ANDROID_KEYSTORE" ] || fail "no keystore at $KEEPER_ANDROID_KEYSTORE"

# The newest build-tools carry zipalign and apksigner.
BUILD_TOOLS="$(ls -d "$ANDROID_HOME"/build-tools/*/ 2>/dev/null | sort -V | tail -n 1)"
[ -n "$BUILD_TOOLS" ] || fail "no build-tools under $ANDROID_HOME/build-tools"
ZIPALIGN="${BUILD_TOOLS}zipalign"
APKSIGNER="${BUILD_TOOLS}apksigner"
ADB="$ANDROID_HOME/platform-tools/adb"
[ -x "$ZIPALIGN" ] && [ -x "$APKSIGNER" ] || fail "zipalign/apksigner missing in $BUILD_TOOLS"
if [ "$INSTALL" = 1 ]; then
  [ -x "$ADB" ] || fail "no adb at $ADB (install platform-tools)"
fi

# The public PostHog client configuration (102.1 #5): a build without it can
# never report. Exported, and stamped as a file because a Gradle daemon started
# by an earlier shell does not see this one's environment.
. "$REPO_ROOT/scripts/lib/posthog-client.sh"
if ! keeper_resolve_posthog_client "$REPO_ROOT"; then
  fail "could not resolve the public PostHog client configuration; the build would ship unable to report anything. Export KEEPER_POSTHOG_HOST and KEEPER_POSTHOG_PROJECT_TOKEN, or sign in to gh."
fi
export KEEPER_POSTHOG_HOST="$POSTHOG_HOST" KEEPER_POSTHOG_PROJECT_TOKEN="$POSTHOG_TOKEN"

say "building the release APK (arm64)"
cd "$REPO_ROOT"
rm -rf "$GEN/app/build/outputs/apk"
keeper_stamp_posthog_client "$REPO_ROOT"
trap 'keeper_unstamp_posthog_client "$REPO_ROOT"' EXIT
bun run tauri android build --apk --target aarch64

UNSIGNED="$(find "$GEN/app/build/outputs/apk" -name '*-release-unsigned.apk' | head -n 1)"
[ -n "$UNSIGNED" ] || fail "the build left no *-release-unsigned.apk under $GEN/app/build/outputs/apk"

OUT="$(dirname "$UNSIGNED")"
ALIGNED="$OUT/keeper-aligned.apk"
SIGNED="$OUT/keeper-signed.apk"

say "aligning and signing $(basename "$UNSIGNED")"
"$ZIPALIGN" -p -f 4 "$UNSIGNED" "$ALIGNED"
"$APKSIGNER" sign \
  --ks "$KEEPER_ANDROID_KEYSTORE" \
  --ks-pass env:KEEPER_ANDROID_KEYSTORE_PASSWORD \
  --ks-key-alias "$KEEPER_ANDROID_KEY_ALIAS" \
  --out "$SIGNED" "$ALIGNED"
"$APKSIGNER" verify "$SIGNED" || fail "apksigner could not verify $SIGNED"
rm -f "$ALIGNED"

if [ "$INSTALL" = 0 ]; then
  say "signed: $SIGNED"
  exit 0
fi

say "installing on the device"
ADB_ARGS=()
[ -n "$SERIAL" ] && ADB_ARGS=(-s "$SERIAL")
"$ADB" ${ADB_ARGS[@]+"${ADB_ARGS[@]}"} install -r "$SIGNED" || fail "adb install failed (is USB debugging on, and is the device authorised?)"
say "installed: open keeper on the device"
