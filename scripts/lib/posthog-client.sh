#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# The public PostHog client configuration every installed build embeds
# (`option_env!` in `crates/keeper/src/telemetry_ipc.rs`, validated by
# `crates/keeper/build.rs`). A build that never saw the pair can report
# nothing, whatever the person using it consents to — which is how the iPhone
# shipped from 0.8 to 0.10.5 (epic 102, story 102.1): `install-ios.sh` never
# exported it.
#
# Two channels, because the environment does not survive every hop of a phone
# build: the iOS build reaches `cargo` through an Xcode script phase run in a
# GUI login session, and Android's through a Gradle daemon that may predate
# this shell (build.rs measures the iOS case for `KEEPER_BUILD_SHA`). So the
# pair is also written to `src-tauri/crates/keeper/posthog-client.env`, which
# build.rs reads when its environment has neither value. The file is
# gitignored and removed again by the script that wrote it.
#
# Order of resolution: the environment, then the repository's own public client
# file for the host, then the GitHub repository variable the release workflow
# reads for the token. Both values end up readable inside every distributed
# binary; neither is a secret.

KEEPER_POSTHOG_CLIENT_FILE="src-tauri/crates/keeper/posthog-client.env"

# Sets POSTHOG_HOST and POSTHOG_TOKEN; returns 1 when either stays empty.
keeper_resolve_posthog_client() {
  local root="${1:?usage: keeper_resolve_posthog_client <repo-root>}"
  local client_env="$root/deploy/posthog/client.env.1p"
  POSTHOG_HOST="${KEEPER_POSTHOG_HOST:-}"
  POSTHOG_TOKEN="${KEEPER_POSTHOG_PROJECT_TOKEN:-}"
  if [ -z "$POSTHOG_HOST" ] && [ -r "$client_env" ]; then
    POSTHOG_HOST="$(sed -n 's/^KEEPER_POSTHOG_HOST=//p' "$client_env" | head -1)"
  fi
  if [ -z "$POSTHOG_TOKEN" ] && command -v gh >/dev/null; then
    POSTHOG_TOKEN="$(gh variable get KEEPER_POSTHOG_PROJECT_TOKEN --repo tgorka/keeper 2>/dev/null || true)"
  fi
  [ -n "$POSTHOG_HOST" ] && [ -n "$POSTHOG_TOKEN" ]
}

# Writes the resolved pair where build.rs looks when the environment is silent.
keeper_stamp_posthog_client() {
  local root="${1:?usage: keeper_stamp_posthog_client <repo-root>}"
  printf 'KEEPER_POSTHOG_HOST=%s\nKEEPER_POSTHOG_PROJECT_TOKEN=%s\n' \
    "$POSTHOG_HOST" "$POSTHOG_TOKEN" >"$root/$KEEPER_POSTHOG_CLIENT_FILE"
}

# Removes the file again, so a later local build does not embed a stale pair.
keeper_unstamp_posthog_client() {
  local root="${1:?usage: keeper_unstamp_posthog_client <repo-root>}"
  rm -f "$root/$KEEPER_POSTHOG_CLIENT_FILE"
}
