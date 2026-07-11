---
title: 'iOS Project Init and Repo Integration'
type: 'feature'
created: '2026-07-11'
status: done
baseline_revision: '3eef91c15d5912909517d3c6dba0e2bf09a62946'
final_revision: '0fe705e21ab5a97ff44134ef59a4ab1a918aee0a'
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

## Implementation Record (2026-07-11)

Implemented under Option A (init + repo integration only; no 12.2 compile-seam work; no Simulator boot). All changes left in the working tree, uncommitted.

**What landed:**
- Ran `CI=true bun run tauri ios init --config src-tauri/crates/keeper/tauri.conf.json` → generated `src-tauri/crates/keeper/gen/apple/` (33 tracked project files: `project.yml`, `Podfile`, `keeper_iOS/Info.plist`, entitlements, `Sources/`, `Assets.xcassets/`, `keeper.xcodeproj/`, `LaunchScreen.storyboard`, `ExportOptions.plist`).
- Bundle id preserved as `dev.tgorka.keeper` (shared with macOS).
- Deployment target set to **iOS 16.0** in `project.yml` + `Podfile` (generator defaulted to 14.0; Code Map requires 16.0) and regenerated.
- `rust-toolchain.toml` — pinned `targets = ["aarch64-apple-ios", "aarch64-apple-ios-sim"]` (channel/components preserved).
- `src-tauri/.gitignore` (AD-32) — ignores `gen/apple/{build,Externals,Pods,.xcode}/` and `**/xcuserdata/`; `project.yml` / `Info.plist` / `*_iOS/` sources stay tracked (verified via `git check-ignore`).
- `docs/ios.md` (new, 3.1 KB) — prereqs (Xcode, `brew install cocoapods xcodegen`, iOS rust targets), `APPLE_DEVELOPMENT_TEAM` env-var signing, regeneration loop; notes Simulator boot is 12.2.
- `biome.json` — added `!src-tauri/crates/*/gen` to file excludes (Biome otherwise lints/rewrites the generated Apple `Contents.json` asset catalogs and breaks lossless regen). Scoped, consistent with the pre-existing `!src-tauri/gen`.

**Acceptance verification (independently re-run):**
- Lossless regeneration: `xcodegen generate` twice → `git diff --quiet gen/apple` exit 0 (no drift). ✓
- `cargo check --target aarch64-apple-ios -p keeper-core` (from `src-tauri/`) → Finished, PASS. ✓
- `bun run check:all` → clean green run: 764/764 tests passed, JS license firewall passed. ✓
- No secrets: no real `DEVELOPMENT_TEAM`, no developer email in any tracked file; only the `XXXXXXXXXX` placeholder in `docs/ios.md`. ✓

**Notes / conservative deferrals (for reviewer + 12.2):**
- `check:all` is **flaky under machine load** on the pre-existing perf test `keeper-core palette::tests::latency_under_100ms_at_10k_entries` (hardcoded 100 ms threshold; observed 272 ms under full parallel load, but 0.03–0.07 s in isolation, 3/3). Unrelated to 12.1 — this story touches no Rust source. A clean green `check:all` was obtained. Candidate deferred-work item: recalibrate that threshold or mark the test load-tolerant.
- `keeper://` URL-scheme registration in the iOS `Info.plist` (`CFBundleURLTypes`) is **NOT added** here. The stock init `Info.plist` omits it, and deep-link registration is explicitly 12.2's scope (see Blocking Contradiction). Adding a handler-less scheme now would ship half of a 12.2 feature; deferred to 12.2.

## Review Triage Log

### 2026-07-11 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 1: (high 0, medium 0, low 1)
- defer: 3: (high 0, medium 0, low 3)
- reject: 10
- addressed_findings:
  - `[low]` `[patch]` `docs/ios.md` — pinned the XcodeGen version (2.45.4) next to the "lossless regeneration" guarantee (both reviewers noted the byte-for-byte claim is XcodeGen-version-sensitive) and clarified that `tauri ios init` is a one-time bootstrap while ongoing regeneration is via `xcodegen generate` (prevents re-init clobbering committed `project.yml`/`Podfile` edits).

Deferred (logged to `deferred-work.md` as DW-105/106/107): Podfile `keeper_macOS` target absent from the generated project (DW-105); `x86_64-apple-ios` Simulator arch not pinned for Intel hosts (DW-106); `ExportOptions.plist method=debugging` for a future signed-release story (DW-107). Rejected (noise/refuted/premised on unused CocoaPods): bundle-id double-prefix (refuted — effective id is `dev.tgorka.keeper`), `Externals/` source-path warning (stock Tauri; Rust pre-build recreates it; lossless regen verified), `Podfile.lock`/`pod install`/`.xcworkspace` sequencing (no pods in use), biome `crates/*/gen` glob breadth (only `keeper/gen` exists; excluding any crate's `gen` is fine), `project.yml` trailing-whitespace/EOF-newline (stock output, reproduces losslessly), inner/outer gitignore asymmetry (union correct), sim-target and min-Xcode doc nits, and device-export teamID (docs already cover the env-var path).

## Auto Run Result

Status: done

**Summary of implemented change.** Story 12.1 (Option A — init + repo integration only) is complete. Ran `tauri ios init` for the `keeper` shell crate and integrated the generated `gen/apple` Apple project under AD-32: committed the project sources, gitignored the ephemeral build/tooling output, pinned the iOS Rust targets, added `docs/ios.md`, and kept the macOS-shared bundle id (`dev.tgorka.keeper`) with no signing team id in git. The Simulator boot and desktop/mobile compile seam remain 12.2's scope, per the coordinator's Option A resolution.

**Files changed.**
- `src-tauri/crates/keeper/gen/apple/**` (new, 33 tracked files) — generated Apple project: `project.yml` (source of truth, iOS 16.0), `Podfile`, `keeper_iOS/Info.plist`, entitlements, `Sources/`, `Assets.xcassets/`, `keeper.xcodeproj/`, `LaunchScreen.storyboard`, `ExportOptions.plist`, nested `.gitignore`.
- `rust-toolchain.toml` — pinned `targets = ["aarch64-apple-ios", "aarch64-apple-ios-sim"]` (channel/components preserved).
- `src-tauri/.gitignore` — AD-32 ignores for `gen/apple/{build,Externals,Pods,.xcode}/` and `**/xcuserdata/`; project sources stay tracked.
- `biome.json` — added `!src-tauri/crates/*/gen` exclude so Biome does not lint/rewrite the generated Apple asset catalogs (would otherwise break lossless regeneration).
- `docs/ios.md` (new) — prerequisites, secret-free signing via `APPLE_DEVELOPMENT_TEAM`, generate/regenerate loop with pinned XcodeGen version, and the iOS core-compile check.
- `_bmad-output/implementation-artifacts/deferred-work.md` — appended DW-105/106/107.

**Review findings breakdown.** Two adversarial reviewers (general + edge-case) at session model capability, run in parallel without prior context. No high or medium findings. 1 patch applied (docs, low), 3 deferred (low), 10 rejected. No intent_gap, no bad_spec — no repair loopback.

**Verification performed (independently re-run by the orchestrator).**
- Lossless regeneration: `xcodegen generate` → `git diff --quiet gen/apple` exit 0 (byte-identical `project.pbxproj`); Blind Hunter independently reproduced byte-identical output on XcodeGen 2.45.4.
- `cargo check --target aarch64-apple-ios -p keeper-core` (from `src-tauri/`) → Finished, PASS.
- `bun run check:all` → clean green run: 764/764 tests passed, JS license firewall passed. (Note: this gate is intermittently flaky under heavy machine load on the pre-existing perf test `keeper-core palette::tests::latency_under_100ms_at_10k_entries` — 272 ms under saturation vs 0.03–0.07 s in isolation, 3/3; unrelated to 12.1, which touches no Rust source. Logged as a candidate deferred item in the Implementation Record.)
- Secrets: no `DEVELOPMENT_TEAM`, no 10-char team id, no developer email/username/absolute paths in any tracked file; only the `XXXXXXXXXX` placeholder in `docs/ios.md`.
- Repo hygiene: `git check-ignore` confirms `build/`, `Externals/`, `Pods/`, `.xcode/`, `xcuserdata/` ignored while `project.yml`/`Info.plist`/`*_iOS/` sources are tracked.

**Residual risks.** (1) The pre-existing `palette` latency perf test can trip its 100 ms threshold under heavy parallel load — flaky, not caused by this story; recalibration is a candidate deferral. (2) Lossless-regen is XcodeGen-version-sensitive (now documented and version-pinned in `docs/ios.md`). (3) Latent iOS boilerplate inconsistencies (Podfile macOS target, Intel-sim arch, debug ExportOptions) are deferred to the stories that will actually exercise them (12.2 / device / release). None block the story's amended acceptance criteria, all of which pass.

