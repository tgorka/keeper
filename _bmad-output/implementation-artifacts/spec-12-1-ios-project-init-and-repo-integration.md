---
title: 'iOS Project Init and Repo Integration'
type: 'feature'
created: '2026-07-11'
status: 'blocked'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
warnings: []
blocking_condition: 'intent gaps — story 12.1 AC#2 (Simulator boot) is not achievable without the desktop/mobile compile seam that story 12.2 explicitly owns; requires a coordinator scoping decision'
---

# Story 12.1 — iOS Project Init and Repo Integration

Planning surfaced a 12.1 ↔ 12.2 scoping contradiction; the coordinator resolved it as
Option A below. This spec is ready for re-drive under the amended scope.

## Coordinator resolution (2026-07-11): OPTION A — decided, not open

Story 12.1 is scoped to **init + repo integration only**. AC#2 is REPLACED with: "the
generated `gen/apple` project is structurally correct and regenerates losslessly from
`project.yml`; desktop quality gates stay green (`bun run check:all`); `cargo check
--target aarch64-apple-ios -p keeper-core` passes (core already compiles for iOS)."
The Simulator boot moves to story 12.2's exit criterion (12.2 owns the compile seam,
AD-26). Everything in "Intent (unambiguous portion)" below stands. Do NOT re-raise this
contradiction — it is settled. The section below is preserved as historical context only.

## Blocking Contradiction (RESOLVED — historical context)

**Story 12.1 AC#2 (verbatim intent):** _"When `tauri ios dev` runs, then the app opens in
the Simulator showing the existing login screen — no physical device required (FR-55)."_
The authoritative iOS research doc (`research-ios-2026-07-09.md` §7, story 12.1) states the
same AC.

**Story 12.2 (verbatim scope):** _"desktop-only surfaces cfg-gated out of the iOS build…
the `tray` module + `tray-icon` cargo feature, global-shortcut, autostart, updater,
window-state, and desktop deep-link registration sit behind `#[cfg(desktop)]`…"_ — i.e.
**the compile seam that first makes an iOS build compile is explicitly 12.2's deliverable**
(AD-26).

**Why these collide (hard evidence, not inference):** the current shell crate cannot compile
for the iOS target at all, so `tauri ios dev` cannot boot the Simulator until the 12.2 gating
is done:

- `src-tauri/crates/keeper/src/lib.rs:30` registers `tauri_plugin_global_shortcut::Builder::new().build()` **unconditionally**, but that crate is `#![cfg(not(any(target_os = "android", target_os = "ios")))]` (`tauri-plugin-global-shortcut-2.3.2/src/lib.rs:13`) → the crate exports **zero items** on iOS → unresolved-path compile error.
- `src-tauri/crates/keeper/src/lib.rs:38-41` registers `tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, …)` unconditionally, but that crate is `#![cfg(not(any(target_os = "android", target_os = "ios")))]` (`tauri-plugin-autostart-2.5.1/src/lib.rs:11`) → same failure.
- Additional desktop-only surface in the same file that will not compile for iOS: the `tray` module + the `tauri` `tray-icon` feature (`src-tauri/Cargo.toml:23`), `tauri_plugin_updater` (`lib.rs:48`), the native menu build/`set_menu`/`on_menu_event` (`lib.rs:78-82`), `hotkey::install` (`lib.rs:88`), the window hide/`CloseRequested` handler (`lib.rs:274-283`), and `RunEvent::Reopen` (`lib.rs:315`).

There is **no meaningfully small subset** of this gating that boots the Simulator: getting
`tauri ios dev` to launch requires essentially all of the 12.2 compile seam. So 12.1's AC#2
cannot be satisfied within a 12.1 that stays out of 12.2's scope.

### Options for the coordinator

- **Option A (recommended) — keep story boundaries by their titles; defer the Simulator boot
  to 12.2.** Scope 12.1 to init + repo integration only: run `tauri ios init`, commit
  `gen/apple` (minus `build/`), stable bundle id shared with macOS, signing via env var
  (no team id in git), add iOS rust targets to `rust-toolchain.toml`, write `docs/ios.md`
  prereqs, and prove the `.xcodeproj` regenerates losslessly from `project.yml`. Replace AC#2
  ("app boots in Simulator") with "the generated project is structurally correct and
  regenerates losslessly; desktop gates stay green." The Simulator boot becomes 12.2's exit
  criterion (12.2 owns the compile seam anyway). Matches the story titles cleanly; the
  epic's SM-7 on-device gate (12.6) is unaffected.

- **Option B — absorb the full compile seam into 12.1 so the Simulator boots here.** 12.1
  does `tauri ios init` **and** all `#[cfg(desktop)]`/target-gated plugin gating needed to
  compile + launch, leaving 12.2 with only the distinctive capability-handshake work
  (`CapabilitiesVm` + `useCapabilitiesStore`, `Platform::sidecar_path` → Unsupported on iOS,
  the `navigator.userAgent` convention test, web Clipboard API + native "open in browser").
  Preserves 12.1's literal AC#2 but moves ~the whole "Compile Seam" out of the story named
  "Compile Seam."

A coordinator decision (A or B, or an alternative split) is required before this story can
be planned to ready-for-dev.

## Intent (unambiguous portion — survives either option)

**Problem:** keeper is a Tauri 2 desktop app with iOS groundwork (`#[cfg_attr(mobile,
tauri::mobile_entry_point)]` already present, `keeper-core` already Tauri-free, crate-type
already `["staticlib","cdylib","rlib"]`) but no generated Apple project, no iOS toolchain
pinning, and no repo hygiene for `gen/apple`.

**Approach:** run `tauri ios init` for the `keeper` shell crate, integrate the generated
`gen/apple` under AD-32's rules (commit minus `build/`; persistent edits only in
`project.yml`, `Info.plist`, `*_iOS/` sources), pin the iOS rust targets, and document
prerequisites — keeping the macOS-shared bundle id and keeping team ids out of git.

## Verified Facts from Investigation (preserve for re-drive)

**Workspace layout**
- Workspace root: `src-tauri/Cargo.toml` — members `["crates/keeper-core", "crates/keeper"]`.
- Tauri shell crate: `keeper` at `src-tauri/crates/keeper`; lib name `keeper_lib`;
  crate-type already `["staticlib", "cdylib", "rlib"]`.
- `tauri.conf.json`: `src-tauri/crates/keeper/tauri.conf.json` — `identifier`
  = **`dev.tgorka.keeper`** (alphanumeric+dots, already valid for iOS and shared with macOS),
  `productName` = `keeper`, `build.devUrl` = `http://localhost:1420` (localhost-bound; device
  hot-reload would need `0.0.0.0`, but that is a 12.6/device concern, not simulator).
  No iOS/mobile config block exists yet. `bundle.macOS.minimumSystemVersion` = `11.0`.
- Tauri 2 (`@tauri-apps/cli ^2`, Rust `tauri = "2"`). Scripts use
  `--config src-tauri/crates/keeper/tauri.conf.json`.

**Repo hygiene / toolchain / docs**
- `rust-toolchain.toml`: channel `stable`, components `clippy`/`rustfmt`, **no `targets`**
  array → iOS targets not pinned (AD-32 wants them pinned for reproducible/CI builds).
- `.gitignore`: `src-tauri/.gitignore` has `/target/`, `/gen/schemas`,
  `/crates/keeper/gen/schemas` (paths relative to `src-tauri/`). **No `gen/apple/build/`
  ignore yet.** No `gen/` dir exists on disk yet.
- `docs/`: `credentials.md`, `release.md`, `egress.md`, `performance.md`,
  `constraints-and-limitations.md`, `project-context.md`. **No `docs/ios.md`.** House style:
  H1 + intro, H2 sections, operational tone, code/env blocks, < ~5 KB.
- `deny.toml`: `src-tauri/deny.toml` — strict permissive-license allowlist; AGPL/GPL denied.
- CI: `.github/workflows/{ci.yml,release.yml}` on `macos-latest`; jobs frontend / rust /
  licenses / build(`tauri build --no-bundle`). No iOS job (that is 12.5).

**Machine tooling (this dev box)**
- Xcode 26.6 (Build 17F113), `xcodebuild` present; rust targets `aarch64-apple-ios` +
  `aarch64-apple-ios-sim` **installed**.
- **CocoaPods (`pod`) NOT installed; XcodeGen NOT installed** — both are prerequisites for
  `tauri ios init` and must be installed (e.g. `brew install cocoapods xcodegen`) and
  documented in `docs/ios.md` regardless of which option is chosen.

## Code Map

- `src-tauri/crates/keeper/tauri.conf.json` — bundle identifier / build config; source of the iOS bundle id (`dev.tgorka.keeper`).
- `src-tauri/crates/keeper/src/lib.rs` — shell entry `run()`; where `tauri ios init` output and (Option B) cfg-gating land; already has `#[cfg_attr(mobile, tauri::mobile_entry_point)]`.
- `src-tauri/crates/keeper/Cargo.toml` + `src-tauri/Cargo.toml` — plugin deps to (Option B) target-gate.
- `rust-toolchain.toml` — add iOS `targets` for reproducibility (AD-32).
- `src-tauri/.gitignore` — add `/crates/keeper/gen/apple/build/`; commit the rest of `gen/apple`.
- `docs/ios.md` (new) — Xcode/CocoaPods/XcodeGen/rust-target prerequisites + build loop.
- `src-tauri/crates/keeper/gen/apple/` (generated by `tauri ios init`) — `project.yml` (min iOS 16.0, theme bg color), `Info.plist` (`CFBundleURLTypes` for `keeper://`), `*_iOS/` sources.

## Auto Run Result

Status: resolved-pending-redrive

Prior blocked result superseded by the coordinator's Option A resolution (2026-07-11):
12.1 = init + repo integration with the replacement AC above; Simulator boot is 12.2's
exit criterion. Re-drive per the amended scope.
