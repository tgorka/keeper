#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Everything the macOS build paths need to know about signing, in one place.
#
# Why a shared file: `build-macos-signed.sh` (run on the Mac) and
# `install-macos.sh` (run from a Linux workstation over ssh) must agree exactly
# on which identity to use and on what counts as an acceptable signature. When
# they disagreed, one of them silently installed an ad-hoc bundle and the
# Screen Recording grant died on every install for weeks.
#
# Source this from a script running ON macOS:
#
#     . "$(dirname "${BASH_SOURCE[0]}")/lib/macos-signing.sh"
#
# Functions:
#   keeper_bundle_id                 the identifier, read from tauri.conf.json
#   keeper_signing_identity          resolve one identity, or fail with guidance
#   keeper_require_stable_signature  verify a bundle is identity-signed
#   keeper_gui_sh                    run a payload inside the GUI login session

# Where the repo is, resolved once when this file is sourced rather than inside
# the function that needs it.
#
# `${BASH_SOURCE[0]}` is unset under zsh, which is the login shell here, and an
# unset value silently resolves two directories above the CALLER instead of
# above this file — `/Users`, in the case that caught it. Falling back to `$0`
# covers a zsh `source`, and `$KEEPER_REPO_ROOT` lets a caller be explicit.
KEEPER_REPO_ROOT="${KEEPER_REPO_ROOT:-$(
  cd "$(dirname "${BASH_SOURCE[0]:-$0}")/../.." && pwd
)}"

# The identifier is not written down twice. `codesign` puts it in the
# designated requirement, TCC keys the grant off that requirement, and
# `tccutil` takes it as an argument, so a copy that drifts from the bundle is a
# grant that silently stops matching.
keeper_bundle_id() {
  local conf id
  conf="$KEEPER_REPO_ROOT/src-tauri/crates/keeper/tauri.conf.json"
  if [ ! -f "$conf" ]; then
    echo "error: no tauri.conf.json under $KEEPER_REPO_ROOT — set KEEPER_REPO_ROOT." >&2
    return 1
  fi
  id="$(sed -n 's/^  "identifier": "\(.*\)",$/\1/p' "$conf" | head -1)"
  case "$id" in
    *.*.*) printf '%s\n' "$id" ;;
    *)
      echo "error: could not read a bundle identifier from $conf" >&2
      return 1
      ;;
  esac
}

# Resolve the one codesigning identity in the login keychain.
#
# `$APPLE_SIGNING_IDENTITY` wins when set, because a machine with both an Apple
# Development and a Developer ID certificate has a real choice to make and this
# is not the place to guess at it.
keeper_signing_identity() {
  if [ -n "${APPLE_SIGNING_IDENTITY:-}" ]; then
    printf '%s\n' "$APPLE_SIGNING_IDENTITY"
    return 0
  fi

  # `security find-identity` prints `  1) <sha1> "Name"`; keep the quoted names.
  local identities count
  identities="$(security find-identity -v -p codesigning |
    sed -n 's/^ *[0-9]*) [0-9A-F]* "\(.*\)"$/\1/p')"
  count="$(printf '%s\n' "$identities" | grep -c . || true)"

  if [ "$count" -eq 0 ]; then
    cat >&2 <<'EOF'
error: no codesigning identity found in the login keychain.

Real capture needs a stable signature (see docs/recording.md). Create a free
Apple Development certificate (Xcode > Settings > Accounts > Manage Certificates
> + > Apple Development), then re-run. Without one, every rebuild re-prompts for
Screen Recording and the grant never sticks.
EOF
    return 1
  fi

  if [ "$count" -gt 1 ]; then
    echo "error: multiple codesigning identities found; set APPLE_SIGNING_IDENTITY to one of:" >&2
    printf '%s\n' "$identities" | sed 's/^/  "/;s/$/"/' >&2
    return 1
  fi

  printf '%s\n' "$identities"
}

# Fail unless the bundle's designated requirement is identity-based.
#
# A silent fallback to ad-hoc is the whole failure this file exists to prevent,
# so it is an error rather than a warning: an ad-hoc bundle that reaches
# /Applications costs the user every TCC grant and every keychain "Always
# Allow" on the app, and the damage is invisible until they try to record.
keeper_require_stable_signature() {
  local app="$1" id dr
  id="$(keeper_bundle_id)" || return 1

  if [ ! -d "$app" ]; then
    echo "error: expected bundle not found at $app" >&2
    return 1
  fi

  codesign --verify --strict "$app" || return 1
  dr="$(codesign -d -r- "$app" 2>/dev/null | sed -n 's/^designated => //p')"
  echo "==> Designated requirement: $dr"

  case "$dr" in
    *cdhash*)
      echo "error: bundle is ad-hoc signed (cdhash requirement) — TCC grants will not survive a rebuild." >&2
      return 1
      ;;
    *"identifier \"$id\""*)
      echo "==> Signature is identity-based and stable across rebuilds."
      ;;
    *)
      echo "error: designated requirement does not name $id; refusing to call this a stable signature." >&2
      return 1
      ;;
  esac
}

# Run a shell payload (read from stdin) inside the GUI login session, stream its
# output here, and exit with its status.
#
# Why this exists: `codesign` needs the identity's private key out of the login
# keychain, and only a process in the user's GUI session can have it. Over ssh
# it fails with `errSecInternalComponent`; `launchctl asuser` needs root;
# `security unlock-keychain` needs the password. Telling Terminal.app to run the
# work is the one route that does not need a secret or a privilege — Terminal is
# already in that session, so its children inherit it.
#
# Measured on 2026-08-09: over ssh, a bare `codesign` gives
# errSecInternalComponent and the identical command dispatched this way exits 0.
keeper_gui_sh() {
  local tag payload runner log status tailpid code
  tag="$$-$(date +%s)"
  payload="/tmp/keeper-gui-$tag.sh"
  runner="/tmp/keeper-gui-$tag.run.sh"
  log="/tmp/keeper-gui-$tag.log"
  status="/tmp/keeper-gui-$tag.status"

  cat >"$payload"

  # Two files, not one, and nothing but a path inside the AppleScript string.
  #
  # AppleScript string literals understand \" \\ \n \t and nothing else, so a
  # `do script` carrying the real command would have to survive bash quoting
  # AND AppleScript quoting: `${PIPESTATUS[0]}` alone is a syntax error there
  # ("Expected \" but found unknown token"). Writing the pipeline into $runner
  # leaves `do script` holding only letters, digits and slashes, which cannot
  # be misread by either layer.
  cat >"$runner" <<RUNNER
#!/bin/bash
bash "$payload" 2>&1 | tee "$log"
echo "\${PIPESTATUS[0]}" > "$status"
RUNNER

  # `exit` so Terminal closes the tab on success per the user's own preference;
  # the full transcript is in $log either way, and a failure leaves it behind.
  if ! osascript -e "tell application \"Terminal\" to do script \"bash $runner; exit\"" >/dev/null; then
    echo "error: could not reach Terminal.app to run the signed build." >&2
    rm -f "$payload" "$runner"
    return 1
  fi

  # Stream the window's output back to whoever called us — over ssh that is the
  # Linux workstation, which would otherwise watch a ten-minute build in silence
  # with no way to tell a slow link step from a stalled one.
  : >"$log"
  tail -n +1 -f "$log" 2>/dev/null &
  tailpid=$!
  while [ ! -f "$status" ]; do sleep 2; done
  sleep 1
  kill "$tailpid" 2>/dev/null || true
  wait "$tailpid" 2>/dev/null || true

  code="$(cat "$status")"
  rm -f "$payload" "$runner" "$status"

  # Keep the transcript only when it is worth reading. A successful build's log
  # is 12 000 lines nobody will open, and one per install accumulates in /tmp
  # forever; a failed one is the only record of what the Terminal window said
  # before it closed, so name it rather than delete it.
  if [ "$code" -eq 0 ]; then
    rm -f "$log"
  else
    echo "error: the GUI build failed; transcript: $log" >&2
  fi
  return "$code"
}
