#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Refuse a keeper bundle that cannot render (AD-173).
#
# Why this exists: on 2026-09-03 an iOS bundle built with
# `bun run tauri ios build --debug` was signed, installed on a real iPhone and
# launched — to an almost empty screen. The bundle's `assets/` folder was
# empty, it carried no `index.html`, and the webview was pointed at
# `http://localhost:1420`, the Vite dev server, which does not run on a phone.
# Every step that could have noticed said nothing, because none of them looked.
# A check a human can skip is that defect waiting to happen again, so this one
# is run by the install paths themselves.
#
# Source this from a script:
#
#     . "$(dirname "${BASH_SOURCE[0]}")/lib/bundle-guard.sh"
#     keeper_require_renderable_bundle path/to/keeper.app   # or keeper.ipa
#
# It accepts an iOS or macOS `.app`, an `.ipa` (a zip whose `Payload/` holds
# the `.app`), or a bare executable (what CI's `tauri build --no-bundle`
# leaves behind). It refuses, with one sentence per cause, when:
#
#   1. the bundle's `assets/` folder is empty (iOS carries the frontend on disk
#      there; a `--debug` build ships the folder with nothing in it),
#   2. that folder has no `index.html`,
#   3. the executable embeds no frontend — and, when it also carries
#      `build.devUrl`, says so, because that is the webview's destination.
#
# On what "contains the dev-server URL" can and cannot prove: tauri-codegen
# compiles the WHOLE of tauri.conf.json into every keeper binary, `devUrl`
# included (`tauri-utils/src/tokens.rs`, `url_lit`), so `strings keeper |
# grep localhost:1420` matches a correct release build exactly as it matches
# the broken one. The URL alone is not evidence. What a dev build lacks is the
# embedded frontend: in dev mode codegen emits `EmbeddedAssets::default()`
# (`tauri-codegen/src/context.rs`) instead of the `phf_map` whose keys are the
# frontend's own paths — `/index.html`, `/assets/main-<hash>.js`, ... Those keys
# are plain string literals in the binary, and the hashed entry chunk is the
# one that cannot be there by accident. So the executable check asks for the
# module script that `index.html` itself names, and reports the dev URL beside
# it when found.
#
# Needs only `strings`, `unzip`, `sed`, `grep` and `find`: it runs on the Linux
# workstation as well as on the Mac.

# Resolved once when sourced; see lib/macos-signing.sh for why the `$0`
# fallback and the override exist.
KEEPER_REPO_ROOT="${KEEPER_REPO_ROOT:-$(
  cd "$(dirname "${BASH_SOURCE[0]:-$0}")/../.." && pwd
)}"

# The dev-server URL, read from tauri.conf.json rather than written down here:
# a copy that drifts from `build.devUrl` is a guard that stops matching.
keeper_dev_url() {
  local conf url
  conf="$KEEPER_REPO_ROOT/src-tauri/crates/keeper/tauri.conf.json"
  if [ ! -f "$conf" ]; then
    echo "error: no tauri.conf.json under $KEEPER_REPO_ROOT — set KEEPER_REPO_ROOT." >&2
    return 1
  fi
  url="$(sed -n 's/^ *"devUrl": *"\([^"]*\)".*$/\1/p' "$conf" | head -1)"
  case "$url" in
    *://*) printf '%s\n' "${url%/}" ;;
    *)
      echo "error: could not read build.devUrl from $conf" >&2
      return 1
      ;;
  esac
}

# The `src` of the module script an index.html loads: Vite writes
# `<script type="module" crossorigin src="/assets/main-<hash>.js">`, and that
# path is also the key tauri-codegen embeds the chunk under.
keeper_entry_script() {
  sed -n 's/.*<script[^>]* src="\([^"]*\)".*/\1/p' "$1" | head -1
}

# The refusal text, shared by every cause so each names the same way out.
_keeper_bundle_refuse() {
  local recipe="$1"
  shift
  echo "error: $*" >&2
  echo "       That is what a --debug build produces. Build it again with: $recipe" >&2
  return 1
}

# Check one unpacked `.app` (or a bare executable). Internal: the public entry
# point below unpacks an .ipa and cleans up around this.
_keeper_check_unpacked() {
  local app="$1" name bin frontend html recipe marker dev_url pointed
  dev_url="$(keeper_dev_url)" || return 1

  if [ -f "$app" ]; then
    # A bare executable: the frontend lives only inside it. dist/ beside the
    # checkout is what the build just embedded, so its index.html names the
    # chunk to look for.
    bin="$app"
    frontend=""
    html="$KEEPER_REPO_ROOT/dist/index.html"
    recipe="bun run tauri:build"
  elif [ -d "$app/Contents/MacOS" ]; then
    # macOS: Tauri embeds the frontend in the executable and copies nothing to
    # Contents/Resources, so the only thing to inspect is the binary.
    name="$(basename "$app" .app)"
    bin="$app/Contents/MacOS/$name"
    frontend=""
    html="$KEEPER_REPO_ROOT/dist/index.html"
    recipe="bun run tauri:build:signed"
  elif [ -d "$app" ]; then
    # iOS: the Xcode project bundles `assets/` (a copy of frontendDist) as a
    # folder resource beside the executable, so the frontend is on disk too.
    name="$(basename "$app" .app)"
    bin="$app/$name"
    frontend="$app/assets"
    html="$frontend/index.html"
    recipe="bun run tauri ios build --export-method debugging"
  else
    echo "error: $app is neither a .app bundle nor an executable." >&2
    return 1
  fi

  if [ -n "$frontend" ]; then
    if [ ! -d "$frontend" ] || [ -z "$(find "$frontend" -mindepth 1 -print -quit)" ]; then
      _keeper_bundle_refuse "$recipe" \
        "$frontend is empty: the bundle carries no frontend, so the app opens to a blank screen."
      return 1
    fi
    if [ ! -f "$html" ]; then
      _keeper_bundle_refuse "$recipe" \
        "$frontend has no index.html: the webview has no document to load."
      return 1
    fi
  fi

  if [ ! -f "$bin" ]; then
    echo "error: no executable at $bin; is this a keeper bundle?" >&2
    return 1
  fi

  # A bundle with no index.html on disk to consult (macOS, bare binary) needs
  # the dist/ the build produced. Without it the guard cannot tell, and a
  # guard that cannot tell must not pass.
  if [ ! -f "$html" ]; then
    echo "error: $html does not exist, so there is nothing to check $bin against. Run the build (it writes dist/) before this check, or set KEEPER_REPO_ROOT." >&2
    return 1
  fi
  marker="$(keeper_entry_script "$html")"
  if [ -z "$marker" ]; then
    echo "error: $html names no module script; expected Vite's <script type=\"module\" src=\"/assets/...\">." >&2
    return 1
  fi

  # `grep -a` on the file itself, NEVER `strings -a … | grep -q`. Measured on
  # hesperia 2026-09-04 against the real 136 MB signed binary: `grep -q` exits
  # at its first hit, `strings` then dies of SIGPIPE, and under `set -o
  # pipefail` the pipeline returns 141 — which this guard read as "the chunk is
  # absent" and used to refuse a perfectly good release build. The small
  # fixtures never caught it because `strings` finished before `grep` quit, so
  # the bug only appeared on an artefact big enough to matter.
  if ! grep -a -qF -- "$marker" "$bin"; then
    pointed=""
    if grep -a -qF -- "$dev_url" "$bin"; then
      pointed=" Its webview is pointed at $dev_url (build.devUrl), a dev server that runs on nobody's phone and in nobody's /Applications."
    fi
    _keeper_bundle_refuse "$recipe" \
      "$bin embeds no frontend: $marker, the chunk index.html loads, is not in the binary.$pointed"
    return 1
  fi

  echo "==> Bundle renders: $bin embeds $marker${frontend:+, and $frontend carries index.html}."
}

# Public entry point. Unpacks an .ipa into a temporary directory for the
# duration of the check and removes it afterwards, pass or fail.
keeper_require_renderable_bundle() {
  local path="$1" tmp app status
  if [ -z "$path" ]; then
    echo "usage: keeper_require_renderable_bundle <keeper.app|keeper.ipa|executable>" >&2
    return 1
  fi
  if [ ! -e "$path" ]; then
    echo "error: $path does not exist." >&2
    return 1
  fi

  case "$path" in
    *.ipa)
      tmp="$(mktemp -d "${TMPDIR:-/tmp}/keeper-bundle-guard.XXXXXX")"
      if ! unzip -q -o "$path" -d "$tmp"; then
        rm -rf "$tmp"
        echo "error: $path is not a zip archive; an .ipa is one." >&2
        return 1
      fi
      app="$(find "$tmp/Payload" -mindepth 1 -maxdepth 1 -name '*.app' -print -quit 2>/dev/null)"
      if [ -z "$app" ]; then
        rm -rf "$tmp"
        echo "error: $path has no Payload/*.app inside it; an .ipa does." >&2
        return 1
      fi
      status=0
      _keeper_check_unpacked "$app" || status=$?
      rm -rf "$tmp"
      return "$status"
      ;;
    *)
      _keeper_check_unpacked "$path"
      ;;
  esac
}
