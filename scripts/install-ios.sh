#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Build keeper for iOS on a macOS host, sign it, and install and launch it on
# the iPhone cabled to that Mac.
#
# The iPhone counterpart of `install-macos.sh`, and for the same reason: the
# Apple project, the signing identity and the cable are all on the Mac, so a
# Linux workstation that wants keeper on a phone has to drive the Mac. Until
# this script existed that drive was docs/ios.md read aloud, and it went wrong
# twice, each time costing a rebuild and a day:
#
#   1. `tauri ios build --debug` was used instead of `--export-method
#      debugging`. A debug profile is a dev build to Tauri: it embeds no
#      frontend, ships an empty `assets/` folder and points the webview at the
#      Vite dev server, which runs on nobody's phone. It signs, installs and
#      launches exactly like the real build and shows nothing — no Talk, no
#      wake phrase, no way to change it. lib/bundle-guard.sh refuses that
#      bundle before it reaches the phone, and this script runs it.
#   2. The free Personal Team entitlement wall. The generated project pins
#      `com.apple.developer.default-data-protection`, and Apple does not grant
#      that capability to a Personal Team, so signing fails until the
#      `entitlements:` block is removed from the generated project.yml and the
#      project regenerated. That is a stated decision here (`KEEPER_IOS_FREE_TEAM`),
#      applied to the remote copy only and verified after the fact.
#
# Every step below refuses with a sentence that names what happened and what
# to do about it; nothing continues past a failure.
#
# Usage:
#   scripts/install-ios.sh [host]        # default host: $KEEPER_MACOS_HOST or "hesperia"
#
# Environment:
#   APPLE_DEVELOPMENT_TEAM      the 10-character team id Tauri signs with
#                               (docs/ios.md). Unset: the script reads it off
#                               the Apple Development certificate and prints
#                               the line to export, rather than guessing.
#   KEEPER_IOS_FREE_TEAM=1      the team is a free Personal Team: drop the
#                               data-protection entitlement from the REMOTE
#                               copy of gen/apple/project.yml and regenerate.
#   KEEPER_IOS_REGISTER_DEVICE=1
#                               first install from this Mac: register the phone
#                               with the team before building, so a profile
#                               exists to sign against (a headless build never
#                               does this itself).
#   KEEPER_IOS_DEVICE=<udid>    pick the phone when several are cabled.
#   KEEPER_IOS_BUILD_ONLY=1     build, gate and stop before the phone.
#
# Requirements on the remote: Xcode with the iOS SDK, xcodegen and cocoapods
# from Homebrew, bun, a Rust toolchain with the aarch64-apple-ios target, an
# Apple Development identity in the login keychain, and an iPhone on a cable
# with Developer Mode on. Requirements at the phone that no script can meet
# are printed last, after the launch.
#
# Nothing is committed or pushed. No network destination is added: the only
# hosts contacted are the Mac over ssh and, by Xcode, Apple's signing service.

set -euo pipefail

HOST="${1:-${KEEPER_MACOS_HOST:-hesperia}}"
REMOTE_DIR="${KEEPER_MACOS_DIR:-keeper-check}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APPLE_DIR="src-tauri/crates/keeper/gen/apple"
IPA="\$HOME/$REMOTE_DIR/$APPLE_DIR/build/arm64/keeper.ipa"

say() { printf '\033[1m==>\033[0m %s\n' "$*"; }
fail() { printf '\033[31mFAIL:\033[0m %s\n' "$*" >&2; exit 1; }

# The bundle identifier is not written down twice: `keeper_bundle_id` reads it
# from tauri.conf.json, and only needs sed, so the macOS signing library is
# usable here on Linux for that one call.
. "$REPO_ROOT/scripts/lib/macos-signing.sh"
BUNDLE_ID="$(keeper_bundle_id)"

# `bun` lives in ~/.bun/bin and `xcodegen`/`pod` in Homebrew's prefix; a
# non-interactive ssh shell has neither on its PATH, and neither does the bash
# that Terminal.app runs the GUI payload with. xcodebuild's script phases
# inherit the PATH of whatever ran xcodebuild, not a login PATH — a previous
# attempt died inside the `Build Rust Code` phase with `bun: command not
# found` for exactly that reason, so the same PATH is exported inside the
# dispatched payload below.
REMOTE_ENV='export PATH="$HOME/.bun/bin:$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"'

# A connect timeout because the whole point of the first check is to say
# quickly that the Mac is not there; ssh's own default waits two minutes.
SSH=(ssh -o BatchMode=yes -o ConnectTimeout=15)

# `caffeinate -i` because a release build outlasts the idle-sleep timer, and a
# laptop that sleeps mid-build drops the ssh connection and takes the build
# with it.
remote() { "${SSH[@]}" "$HOST" "caffeinate -i bash -c $(printf '%q' "$1")"; }

# --- 1. Preflight on the Mac ----------------------------------------------

say "target: $HOST:~/$REMOTE_DIR -> iPhone on $HOST's cable"
"${SSH[@]}" "$HOST" 'true' 2>/dev/null \
  || fail "cannot reach $HOST over ssh. The Mac is off, asleep or off the network; wake it, check \`ssh $HOST\` by hand, and re-run."

say "checking the toolchain on $HOST"
remote "$REMOTE_ENV && xcode-select -p >/dev/null 2>&1 && xcrun --find xcodebuild >/dev/null 2>&1" \
  || fail "no Xcode on $HOST (xcode-select -p / xcrun --find xcodebuild). Install Xcode with the iOS SDK, run \`xcodebuild -runFirstLaunch\` once, and re-run."
remote "$REMOTE_ENV && command -v xcodegen >/dev/null" \
  || fail "no xcodegen on $HOST. \`brew install xcodegen cocoapods\` there and re-run."
remote "$REMOTE_ENV && command -v pod >/dev/null" \
  || fail "no cocoapods on $HOST. \`brew install cocoapods\` there and re-run."
remote "$REMOTE_ENV && command -v bun >/dev/null" \
  || fail "no bun on $HOST (looked in ~/.bun/bin). Install bun there and re-run."
remote "$REMOTE_ENV && rustup target list --installed 2>/dev/null | grep -qx aarch64-apple-ios" \
  || fail "the Rust toolchain on $HOST has no aarch64-apple-ios target. Run \`rustup target add aarch64-apple-ios aarch64-apple-ios-sim\` there and re-run."

# Listing the identity works over ssh; USING it does not (docs/ios.md), which
# is why the build later runs in the GUI session. The count is enough here.
IDENTITY_COUNT="$(remote 'security find-identity -v -p codesigning | grep -c "Apple Development" || true')"
[ "${IDENTITY_COUNT:-0}" -ge 1 ] \
  || fail "no Apple Development identity in $HOST's login keychain. In Xcode: Settings > Accounts > (your Apple ID) > Manage Certificates > + > Apple Development, then re-run."

# The phone. `devicectl` writes JSON on request and `plutil -extract ... raw`
# reads it back, which spares a jq or python that a Mac may not have. What is
# printed back is one tab-separated line per device, plus one line saying
# whether an iPhone is on the USB bus at all: a plugged-in iPhone shows up in
# `ioreg -p IOUSB` BEFORE any pairing or Trust step, so an empty devicectl
# list with an empty bus is the cable, the port or the phone — not a prompt.
say "looking for the phone"
DEVICES="$(remote "$(cat <<'EOF'
json="$(mktemp /tmp/keeper-devicectl.XXXXXX)"
xcrun devicectl list devices --json-output "$json" >/dev/null 2>&1 || { rm -f "$json"; echo "devicectl-failed"; exit 0; }
# Walk the array until an index has no `identifier`, rather than trusting what
# `plutil` prints for a bare array; every device entry carries that key.
i=0
while plutil -extract "result.devices.$i.identifier" raw -o - "$json" >/dev/null 2>&1; do
  d="result.devices.$i"
  get() { plutil -extract "$d.$1" raw -o - "$json" 2>/dev/null || echo "unknown"; }
  printf 'device\t%s\t%s\t%s\t%s\t%s\n' \
    "$(get hardwareProperties.udid)" \
    "$(get connectionProperties.tunnelState)" \
    "$(get connectionProperties.pairingState)" \
    "$(get deviceProperties.developerModeStatus)" \
    "$(get deviceProperties.name)"
  i=$((i + 1))
done
rm -f "$json"
printf 'usb\t%s\n' "$(ioreg -p IOUSB -l 2>/dev/null | grep -c '"USB Product Name" = "iPhone"' || true)"
EOF
)")"

[ "$DEVICES" != "devicectl-failed" ] \
  || fail "\`xcrun devicectl list devices\` failed on $HOST. That command ships with Xcode 15 and later; check \`xcode-select -p\` points at a current Xcode."

USB_IPHONES="$(printf '%s\n' "$DEVICES" | awk -F'\t' '$1 == "usb" {print $2}')"
ATTACHED="$(printf '%s\n' "$DEVICES" | awk -F'\t' '$1 == "device" && $3 == "connected"')"

if [ -z "$ATTACHED" ]; then
  if [ "${USB_IPHONES:-0}" -eq 0 ]; then
    fail "no iPhone is attached to $HOST: devicectl lists no connected device and no iPhone is on the USB bus (ioreg -p IOUSB). An iPhone appears on the bus before any Trust prompt, so this is the cable, the port or the phone, not trust — plug it in, try another cable or port, wake the phone, and re-run."
  fi
  fail "an iPhone is on $HOST's USB bus but devicectl cannot talk to it: it is not paired with this Mac. Unlock the phone, accept \"Trust This Computer\" on it (and the Mac's pairing dialog, if shown), then re-run."
fi

if [ -n "${KEEPER_IOS_DEVICE:-}" ]; then
  PICKED="$(printf '%s\n' "$ATTACHED" | awk -F'\t' -v u="$KEEPER_IOS_DEVICE" '$2 == u')"
  [ -n "$PICKED" ] || fail "KEEPER_IOS_DEVICE=$KEEPER_IOS_DEVICE is not one of the attached phones:
$(printf '%s\n' "$ATTACHED" | awk -F'\t' '{print "  " $2 "  " $6}')"
elif [ "$(printf '%s\n' "$ATTACHED" | wc -l)" -gt 1 ]; then
  fail "more than one iPhone is attached to $HOST; set KEEPER_IOS_DEVICE to one of:
$(printf '%s\n' "$ATTACHED" | awk -F'\t' '{print "  " $2 "  " $6}')"
else
  PICKED="$ATTACHED"
fi

UDID="$(printf '%s' "$PICKED" | cut -f2)"
PAIRING="$(printf '%s' "$PICKED" | cut -f4)"
DEVMODE="$(printf '%s' "$PICKED" | cut -f5)"
DEVICE_NAME="$(printf '%s' "$PICKED" | cut -f6)"

[ "$PAIRING" = "paired" ] \
  || fail "$DEVICE_NAME ($UDID) is attached but not paired with $HOST (pairingState: $PAIRING). Unlock the phone, accept \"Trust This Computer\" on it, then re-run."
[ "$DEVMODE" = "enabled" ] \
  || fail "$DEVICE_NAME ($UDID) has Developer Mode off (developerModeStatus: $DEVMODE). On the phone: Settings > Privacy & Security > Developer Mode, turn it on, and let it reboot — it asks again after the restart. The toggle only appears after a development tool has tried the phone once; if it is missing, do docs/ios.md \"First device install\" steps 3–4 with the phone at the Mac, then re-run."

say "phone: $DEVICE_NAME ($UDID), paired, Developer Mode on"

# --- 2. The team ------------------------------------------------------------
#
# Tauri reads `APPLE_DEVELOPMENT_TEAM` at build time; nothing in the tree
# carries the id (docs/ios.md, AD-32). When it is unset, the certificate's OU
# is the team id, and printing it as the line to export is honest in a way a
# silent guess is not: the person sees which team will sign.
if [ -z "${APPLE_DEVELOPMENT_TEAM:-}" ]; then
  TEAM_FROM_CERT="$(remote 'security find-certificate -c "Apple Development" -p 2>/dev/null | openssl x509 -noout -subject 2>/dev/null | sed -n "s/.*OU *= *\([A-Z0-9]\{10\}\).*/\1/p" | head -1')"
  if [ -n "$TEAM_FROM_CERT" ]; then
    fail "APPLE_DEVELOPMENT_TEAM is unset. The Apple Development certificate on $HOST belongs to team $TEAM_FROM_CERT; if that is the one to sign with, run:
  export APPLE_DEVELOPMENT_TEAM=$TEAM_FROM_CERT
and re-run."
  fi
  fail "APPLE_DEVELOPMENT_TEAM is unset and no team id could be read off the Apple Development certificate on $HOST. Find it in Xcode under Settings > Accounts > (your Apple ID) > Manage Certificates, then \`export APPLE_DEVELOPMENT_TEAM=<10 characters>\` and re-run."
fi
say "team: $APPLE_DEVELOPMENT_TEAM"

# --- Sync the tree -----------------------------------------------------------
#
# See check-macos.sh for why each of these is excluded. `.git` carries no
# trailing slash on purpose: in a git worktree it is a FILE holding an absolute
# path that exists only here, and copying it makes every git call on the remote
# fail. The four under gen/apple are XcodeGen's and CocoaPods' own output
# (docs/ios.md, AD-32): gitignored, so absent here, and `--delete` would
# otherwise wipe the Mac's copies on every run.
say "syncing working tree"
rsync -az --delete \
  --exclude '.git' \
  --exclude 'node_modules/' \
  --exclude 'target/' \
  --exclude 'dist/' \
  --exclude 'src-tauri/crates/keeper/binaries/' \
  --exclude "$APPLE_DIR/build/" \
  --exclude "$APPLE_DIR/Externals/" \
  --exclude "$APPLE_DIR/Pods/" \
  --exclude "$APPLE_DIR/.xcode/" \
  --exclude "$APPLE_DIR/xcuserdata/" \
  "$REPO_ROOT/" "$HOST:$REMOTE_DIR/"

# `--ignore-scripts` skips our own `prepare` (`lefthook install`), which wants a
# git repository this checkout deliberately does not have.
say "bun install"
remote "cd \$HOME/$REMOTE_DIR && $REMOTE_ENV && bun install --frozen-lockfile --ignore-scripts" \
  || fail "bun install on $HOST failed (output above)."

# --- 3. The Personal Team entitlement wall ----------------------------------
#
# `gen/apple/project.yml` pins `com.apple.developer.default-data-protection`
# to NSFileProtectionCompleteUntilFirstUserAuthentication, and a free Personal
# Team's automatic profile cannot carry that capability, so signing fails with
# `doesn't match the entitlements file's value for the
# com.apple.developer.default-data-protection entitlement`. Apple cannot be
# asked which kind of team an id is, so this is a decision the operator
# states: with `KEEPER_IOS_FREE_TEAM=1` the `entitlements:` block is removed
# from the REMOTE copy of project.yml and the project regenerated. The
# committed value stays as it is, on purpose: `crates/keeper/tests/
# entitlements_protection.rs` pins it, and iOS already defaults a third-party
# app's container to that same protection class, so a Personal-Team build
# loses nothing but the pin (docs/ios.md, "Signing on a free Personal Team").
#
# The regeneration is then verified rather than trusted: `xcodegen generate`
# is what once produced a project without Speech.framework (project.yml
# comment on `dependencies`), so it has to be shown to have dropped the
# entitlement AND kept the framework — four references when this was last
# done by hand.
if [ -n "${KEEPER_IOS_FREE_TEAM:-}" ]; then
  say "free Personal Team: dropping the data-protection entitlement from the remote project.yml"
  remote "cd \$HOME/$REMOTE_DIR/$APPLE_DIR && $REMOTE_ENV && $(cat <<'EOF'
set -euo pipefail
awk 'skip && /^    [^ ]/ { skip = 0 } /^    entitlements:/ { skip = 1 } !skip' project.yml > project.yml.free
mv project.yml.free project.yml
grep -q '^    entitlements:' project.yml && { echo "error: the entitlements: block is still in project.yml after the edit" >&2; exit 1; }
xcodegen generate
if grep -q CODE_SIGN_ENTITLEMENTS keeper.xcodeproj/project.pbxproj; then
  echo "error: project.pbxproj still carries CODE_SIGN_ENTITLEMENTS after xcodegen generate; the entitlement was not dropped" >&2
  exit 1
fi
speech="$(grep -c 'Speech.framework' keeper.xcodeproj/project.pbxproj || true)"
if [ "$speech" -eq 0 ]; then
  echo "error: project.pbxproj no longer references Speech.framework after xcodegen generate; a build from it would have no SFSpeechRecognizer" >&2
  exit 1
fi
echo "==> Regenerated without CODE_SIGN_ENTITLEMENTS; Speech.framework referenced $speech times (4 when last done by hand)."
EOF
)" || fail "could not drop the entitlement on $HOST (output above). The remote project.yml is a fresh copy on every run, so nothing is left half-edited; fix the cause and re-run."
fi

# --- 4. Build and sign, inside the Mac's GUI login session --------------------
#
# `codesign` needs the identity's private key out of the login keychain, and
# over ssh it fails with `errSecInternalComponent`; `keeper_gui_sh`
# (lib/macos-signing.sh) runs the payload through Terminal.app for exactly
# that reason, and streams the transcript back here. The transcript is also
# kept locally, because two of the ways this build fails are expected states
# with a named remedy, and the remedy is only printable if the text is caught.
#
# Registration: a headless `tauri ios build` never registers the device with
# the team, and without a registration there is no profile to sign against.
# The one-off xcodebuild that mints the profile is expected to abort in its
# `Build Rust Code` phase (docs/ios.md), so its exit status is not the
# verdict; the real build that follows is.
#
# `--export-method debugging`, never `--debug`: see the header.
say "building and signing in the GUI session on $HOST (Terminal.app opens there)"
BUILD_LOG="$(mktemp "${TMPDIR:-/tmp}/keeper-ios-build.XXXXXX.log")"
REGISTER=""
if [ -n "${KEEPER_IOS_REGISTER_DEVICE:-}" ]; then
  REGISTER="echo '==> Registering $UDID with team $APPLE_DEVELOPMENT_TEAM (this xcodebuild is expected to stop at Build Rust Code)'
(cd $APPLE_DIR && xcodebuild -project keeper.xcodeproj -scheme keeper_iOS -configuration debug -destination id=$UDID -allowProvisioningUpdates -allowProvisioningDeviceRegistration DEVELOPMENT_TEAM=$APPLE_DEVELOPMENT_TEAM build) || true"
fi
if ! remote "cd \$HOME/$REMOTE_DIR && . scripts/lib/macos-signing.sh && keeper_gui_sh <<'PAYLOAD'
set -euo pipefail
cd \$HOME/$REMOTE_DIR
$REMOTE_ENV
export APPLE_DEVELOPMENT_TEAM=$APPLE_DEVELOPMENT_TEAM
$REGISTER
bun run tauri ios build --config src-tauri/crates/keeper/tauri.conf.json --export-method debugging
PAYLOAD" 2>&1 | tee "$BUILD_LOG"; then
  if grep -q 'com.apple.developer.default-data-protection entitlement' "$BUILD_LOG"; then
    fail "signing failed on the data-protection entitlement: team $APPLE_DEVELOPMENT_TEAM is a free Personal Team and Apple does not grant it that capability. Re-run with KEEPER_IOS_FREE_TEAM=1, which drops the entitlement from the remote copy of the generated project (the committed value stays)."
  fi
  if grep -q "couldn't find any iOS App Development provisioning profiles" "$BUILD_LOG"; then
    fail "no provisioning profile for $BUNDLE_ID: this phone is not registered with team $APPLE_DEVELOPMENT_TEAM from this Mac. Re-run with KEEPER_IOS_REGISTER_DEVICE=1, which mints the profile before building."
  fi
  if grep -q 'errSecInternalComponent' "$BUILD_LOG"; then
    fail "codesign could not reach the login keychain even from Terminal.app. Log in to $HOST's GUI session (the screen must be unlocked once) and re-run."
  fi
  if grep -q 'Developer Mode disabled' "$BUILD_LOG"; then
    fail "the phone reports Developer Mode disabled. On the phone: Settings > Privacy & Security > Developer Mode, turn it on and let it reboot, then re-run."
  fi
  if grep -q 'bun: command not found' "$BUILD_LOG"; then
    fail "xcodebuild's script phase could not find bun. It is looked for in ~/.bun/bin on $HOST; if bun is installed elsewhere, put it on that PATH."
  fi
  fail "the iOS build failed on $HOST; transcript kept at $BUILD_LOG (and on $HOST in /tmp/keeper-gui-*.log)."
fi
rm -f "$BUILD_LOG"

# --- 5. Gate the artefact before it reaches the phone --------------------------
#
# Both checks reuse the macOS path's own functions rather than a second copy:
# lib/bundle-guard.sh refuses a bundle that cannot render (the `--debug`
# defect in the header), and `keeper_require_stable_signature` refuses an
# ad-hoc one whose designated requirement is a bare cdhash. The unpacked .app
# is kept in a temporary directory on the Mac until the proofs are printed.
say "gating the IPA on $HOST"
UNPACKED="$(remote "cd \$HOME/$REMOTE_DIR && $REMOTE_ENV && $(cat <<EOF
set -euo pipefail
[ -f "$IPA" ] || { echo "error: no IPA at $IPA; the build did not export one" >&2; exit 1; }
. scripts/lib/bundle-guard.sh
keeper_require_renderable_bundle "$IPA" >&2
tmp="\$(mktemp -d /tmp/keeper-ios-proof.XXXXXX)"
unzip -q -o "$IPA" -d "\$tmp"
app="\$(find "\$tmp/Payload" -mindepth 1 -maxdepth 1 -name '*.app' -print -quit)"
. scripts/lib/macos-signing.sh
keeper_require_stable_signature "\$app" >&2
echo "\$app"
EOF
)")" || fail "the IPA on $HOST is not fit for the phone (reason above). Nothing was installed."

if [ -n "${KEEPER_IOS_BUILD_ONLY:-}" ]; then
  remote "rm -rf \"$(dirname "$(dirname "$UNPACKED")")\""
  say "build only; the IPA is at $HOST:$IPA and passed both gates"
  exit 0
fi

# --- 6. Install and launch -------------------------------------------------------
#
# Two of the ways this stops are states of the phone, not faults, and each
# has one sentence iOS uses for it (docs/ios.md, measured 2026-09-03).
say "installing on $DEVICE_NAME"
INSTALL_LOG="$(mktemp "${TMPDIR:-/tmp}/keeper-ios-install.XXXXXX.log")"
if ! remote "xcrun devicectl device install app --device $UDID \"$IPA\"" 2>&1 | tee "$INSTALL_LOG"; then
  if grep -q 'Developer Mode' "$INSTALL_LOG"; then
    fail "the phone refused the install because Developer Mode is off. On the phone: Settings > Privacy & Security > Developer Mode, turn it on and let it reboot, then re-run."
  fi
  fail "devicectl could not install on $DEVICE_NAME (output above; kept at $INSTALL_LOG). The IPA passed both gates, so this is the phone or the cable: unlock the phone and re-run."
fi
rm -f "$INSTALL_LOG"

say "launching $BUNDLE_ID on $DEVICE_NAME"
LAUNCH_LOG="$(mktemp "${TMPDIR:-/tmp}/keeper-ios-launch.XXXXXX.log")"
LAUNCH_CMD="xcrun devicectl device process launch --device $UDID $BUNDLE_ID"
if ! remote "$LAUNCH_CMD" 2>&1 | tee "$LAUNCH_LOG"; then
  if grep -q 'has not been explicitly trusted by the user' "$LAUNCH_LOG"; then
    fail "keeper is installed but iOS will not launch it until the developer certificate is trusted. On the phone: Settings > General > VPN & Device Management, tap your Apple ID under \"Developer App\", choose Trust (the phone must have been online once to verify it). Then launch it by tapping the icon, or from here: ssh $HOST '$LAUNCH_CMD'."
  fi
  if grep -qiE 'profile.*(expired|no longer valid|invalid)|expired.*profile' "$LAUNCH_LOG"; then
    fail "iOS refused the launch for a lapsed provisioning profile. A free Personal Team's profile lasts 7 days; re-arm it as docs/ios.md \"The 7-day re-arm ritual\" says (re-running this script re-signs and reinstalls with a fresh profile, and the phone keeps its data)."
  fi
  fail "devicectl could not launch $BUNDLE_ID on $DEVICE_NAME (output above; kept at $LAUNCH_LOG). It is installed; tap the icon to see what the phone says."
fi
rm -f "$LAUNCH_LOG"

# --- 7. The proofs, from the bundle that was installed ----------------------------
#
# Each line below is read off the unpacked .app, not off this script's
# intentions. The two usage strings are the sentences iOS will show in its
# permission dialogs; they must name the phone, because the macOS bundle's
# Info.plist ("on this Mac") is merged UNDER Info.ios.plist during the build
# and wins whenever the iOS file forgets a key (docs/ios.md).
say "proofs from the installed bundle"
remote "$(cat <<EOF
set -euo pipefail
app="$UNPACKED"
bin="\$app/\$(basename "\$app" .app)"
plist="\$app/Info.plist"
if otool -L "\$bin" | grep -q 'Speech.framework'; then
  echo "  Speech.framework:               linked (otool -L)"
else
  echo "  Speech.framework:               NOT linked — voice will answer NoRecognizer; check project.yml's dependencies and regenerate"
fi
for key in NSMicrophoneUsageDescription NSSpeechRecognitionUsageDescription; do
  text="\$(/usr/libexec/PlistBuddy -c "Print :\$key" "\$plist" 2>/dev/null || echo '(absent — the app will crash on requestAuthorization)')"
  echo "  \$key:"
  echo "      \$text"
  case "\$text" in
    *"this Mac"*) echo "      WARNING: this names the Mac, not the phone; crates/keeper/Info.ios.plist must restate the key (docs/ios.md)" ;;
  esac
done
# PlistBuddy prints an array as "Array {" / one indented item per line / "}".
modes="\$(/usr/libexec/PlistBuddy -c "Print :UIBackgroundModes" "\$plist" 2>/dev/null | sed -n 's/^    //p' | paste -sd, -)"
echo "  UIBackgroundModes:              \${modes:-(absent — an armed session would end when keeper leaves the front)}"
dr="\$(codesign -d -r- "\$app" 2>/dev/null | sed -n 's/^designated => //p')"
case "\$dr" in
  *cdhash*) echo "  Designated requirement:         bare cdhash — this is NOT a signed build, whatever the launch said"; exit 1 ;;
  *) echo "  Designated requirement:         \$dr" ;;
esac
rm -rf "\$(dirname "\$(dirname "\$app")")"
EOF
)" || fail "the proofs could not be read off the bundle (output above)."

say "keeper is running on $DEVICE_NAME"
cat <<EOF

What is still yours to do, on the phone, in this order — none of it can be
done from here:

  1. Open Bots and tap Talk once. iOS asks for the microphone and for speech
     recognition; grant both. Until then the port refuses with the permission
     sentence, not silence.
  2. Have the dictation language downloaded on the phone: Settings > General >
     Keyboard > Enable Dictation, and the language listed under Dictation
     Languages. Recognition is on-device only; a locale whose model is missing
     gets a sentence naming the language to download, never a server.
  3. Arm the wake phrase. It is OFF on a fresh install, on purpose (a fresh
     install that listened would be the silent always-on listener the design
     refuses, keeper-core/src/registry.rs). The switch is "Listen for a
     phrase", in Settings > Bots and inside the "Bot and model" sheet on the
     Bots surface; the phrase beside it is "nixie" until you change it. The
     orange microphone indicator stays lit while it is armed.

The certificate is a free Personal Team's: the app stops launching after 7
days until this script is run again (docs/ios.md, "The 7-day re-arm ritual").
EOF
