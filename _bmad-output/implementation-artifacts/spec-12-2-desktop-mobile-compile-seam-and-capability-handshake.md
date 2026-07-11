---
title: 'Desktop/Mobile Compile Seam and Capability Handshake'
type: 'feature'
created: '2026-07-11'
status: 'done'
baseline_revision: 'ed2e5c7397d6275a385dd9a7d9c41eb1036e6d33'
final_revision: '234c18b7131f11748c9659d95a7a8546b8165634'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
warnings: [oversized]
---

<intent-contract>

## Intent

**Problem:** The `keeper` Tauri shell crate registers desktop-only surfaces (tray, global-shortcut, autostart, updater, native menu, window-close-to-hide, `RunEvent::Reopen`, bbctl sidecar) **unconditionally** in `src-tauri/crates/keeper/src/lib.rs`, and several of those plugin crates are `#![cfg(not(target_os="ios"))]` — so the workspace cannot compile for `aarch64-apple-ios` at all. There is also no machine-readable capability contract, so a future iOS UI would have to guess which surfaces exist.

**Approach:** Put every desktop-only surface behind `#[cfg(desktop)]`/`#[cfg(target_os)]` gates with target-gated Cargo deps so the single shell crate compiles for both desktop and iOS, and add one data-driven `CapabilitiesVm` served at startup over IPC into a `useCapabilitiesStore` zustand mirror — the reusable, per-platform handshake the phone-shell epic will consume.

## Boundaries & Constraints

**Always:**
- Desktop build behavior stays **byte-identical**: all existing desktop gates (`bun run check:all`) stay green; gating is additive `#[cfg(desktop)]`, never a behavior change on macOS.
- `keeper-core` stays platform-free: **no `cfg(target_os)` in core business logic** (AD-26). Platform variance enters only through the `Platform` port and the shell crate. The `CapabilitiesVm` *struct* lives in `keeper-core::vm` but is *populated* per-platform in the shell crate.
- Reuse the existing `Unsupported` plumbing: `CoreError::Unsupported` → `IpcErrorCode::Unsupported` (`retriable:false`) via the single `to_ipc_error` funnel. **Add no new error variant.**
- `CapabilitiesVm` derives `serde` + `ts_rs::TS` with `#[serde(rename_all = "camelCase")]` + bare `#[ts(export)]` (no `export_to`); the generated `src/lib/ipc/gen/CapabilitiesVm.ts` is committed and drift-free (`bun run bindings:check`).
- The iOS shell registers only: notification, mobile/custom-scheme deep-link (`init` + `on_open_url`), the IPC `invoke_handler`, and the `keeper-media://` protocol. Clipboard stays a JS Web-API concern; "open in browser" stays `tauri_plugin_opener::open_url`.
- The frontend must **never** consult `navigator.userAgent`, `navigator.platform`, `import.meta.env`, or `@tauri-apps/plugin-os` for feature gating — a convention test enforces this repo-wide.
- Cargo dep gating uses real target cfgs `cfg(not(any(target_os = "ios", target_os = "android")))` (Cargo does not understand tauri's `desktop`/`mobile` cfgs); Rust source gating uses `#[cfg(desktop)]` (tauri-build injects it).

**Block If:**
- `cargo check --target aarch64-apple-ios` fails for a reason that would require adding a `cfg(target_os)` inside `keeper-core` business logic (violating AD-26) rather than at the shell/port seam — HALT rather than gating core logic.

**Never:**
- Do not implement any iOS UI, phone layout, or surface *hiding* — Epic 13 owns consuming `CapabilitiesVm` to hide surfaces. This story lands the mechanism (mirror hydrated + available), not the hiding.
- Do not add an in-app updater code path on iOS. Do not add a native clipboard plugin. Do not spawn child processes / sidecars on iOS.
- Do not change the bundle id or any desktop plugin behavior. Do not commit any Apple team id.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Capabilities on desktop | `capabilities` command, desktop build | `CapabilitiesVm` with desktop-only caps (tray/menuBar, globalHotkey, launchAtLogin, inAppUpdater, nativeMenuBar, bridgeSidecar, revealInFileManager) = `true` | No error expected |
| Capabilities on iOS | `capabilities` command, iOS build | Same `CapabilitiesVm` shape with those desktop-only caps = `false` | No error expected |
| `sidecar_path` on iOS | `IosPlatform::sidecar_path(name)` | `Err(CoreError::Unsupported(..))` → at command boundary `IpcError { code: Unsupported, retriable: false }` | Mapped by `to_ipc_error`; never panics |
| Startup hydration | frontend boot, `capabilities()` resolves | `useCapabilitiesStore` mirrors the `CapabilitiesVm` via `applySnapshot` | On reject: error logged, store left at its declared safe default; boot not blocked |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper/src/lib.rs` — shell entry `run()` (`:20`, `#[cfg_attr(mobile, tauri::mobile_entry_point)]` at `:19`); all unconditional desktop-only registrations to gate: global-shortcut (`:30`), autostart (`:38-41`), updater (`:48`), process (`:53`), native menu (`:78-82`), hotkey (`:88`), tray (`:12`,`:116`,`:316`), window `CloseRequested`/hide (`:274-283`), `RunEvent::Reopen` (`:315-318`); keep un-gated: opener (`:22`), deep-link `init`+`on_open_url` (`:23`,`:65-70`), dialog, notification (`:34`), `keeper-media` protocol (`:55-57`), `invoke_handler` (`:138-269`).
- `src-tauri/crates/keeper/src/{tray.rs,menu.rs,hotkey.rs}` — desktop-only modules; gate whole modules (`#![cfg(desktop)]` or gate the `mod` decls at `lib.rs:8,11,12`).
- `src-tauri/crates/keeper/src/ipc.rs` — `AppState::new` builds `Arc::new(DesktopPlatform)` (`:213-214`); `DesktopPlatform::sidecar_path` (`:434`); `to_ipc_error` funnel (`:613`); `app_ping` command exemplar (`:804-813`); desktop-only command bodies to gate/`Unsupported` (`launch_at_login_*`, `menu_bar_presence_*`, `reveal_path`/`reveal_item_in_dir` at `:1646`). Home for `IosPlatform` + the new `capabilities` command.
- `src-tauri/crates/keeper-core/src/vm.rs` — VM home; `PingVm` triad exemplar (`:63-67`); `IpcError` (`:1658`), `IpcErrorCode::Unsupported` (`:89`); ts-rs export `#[test]` module (`:2411`). Add `CapabilitiesVm`.
- `src-tauri/crates/keeper-core/src/{platform.rs,error.rs}` — `Platform` trait + `sidecar_path` (`platform.rs:52-54` → `Result<PathBuf, CoreError>`); `CoreError::Unsupported` (`error.rs:476`).
- `src-tauri/Cargo.toml` + `src-tauri/crates/keeper/Cargo.toml` — `tauri` `tray-icon` feature (`src-tauri/Cargo.toml:23`) and plugin deps (global-shortcut/autostart/updater/process) to target-gate.
- `src/lib/ipc/client.ts` — typed IPC wrapper; one-shot pattern `bbctlAvailability` (`:275-277`). Add `capabilities()`.
- `src/lib/stores/capabilities.ts` (new) — mirror `src/lib/stores/networks.ts` (`createStore` from `zustand/vanilla` + `useXStore` selector-hook, `applySnapshot`).
- `src/hooks/use-capabilities-hydrate.ts` (new) — mirror `src/hooks/use-session-restore.ts`; wired in `src/App.tsx`.
- `src/test/no-user-agent-gating.test.ts` (new) — repo-wide convention test (vitest, colocated pattern).

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/Cargo.toml` + `src-tauri/crates/keeper/Cargo.toml` -- move `tauri-plugin-global-shortcut`, `-autostart`, `-updater`, `-process` and the `tauri` `tray-icon` feature under `[target.'cfg(not(any(target_os = "ios", target_os = "android")))'.dependencies]` -- so iOS never links desktop-only crates.
- [x] `src-tauri/crates/keeper/src/lib.rs` -- gate all desktop-only registrations (list above) behind `#[cfg(desktop)]`; keep the iOS set (notification, deep-link `init`/`on_open_url`, `invoke_handler`, `keeper-media`, opener, dialog) un-gated; cfg-select the `Platform` impl injected into `AppState`.
- [x] `src-tauri/crates/keeper/src/{tray.rs,menu.rs,hotkey.rs}` -- add module-level desktop gate so they compile out on iOS.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- gate desktop-only command bodies so the crate compiles on iOS (compile them out on iOS or return `CoreError::Unsupported`); add `#[cfg(target_os = "ios")] struct IosPlatform` + `impl Platform` whose `sidecar_path` returns `CoreError::Unsupported`; cfg-select it in `AppState::new`; add `#[tauri::command] capabilities(state) -> Result<CapabilitiesVm, IpcError>` modeled on `app_ping`, register it in `generate_handler!` at `lib.rs`.
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- add `CapabilitiesVm` (boolean caps per gated surface; `serde` + `TS`, camelCase, bare `#[ts(export)]`); regenerate + commit `src/lib/ipc/gen/CapabilitiesVm.ts`.
- [x] `src/lib/ipc/client.ts` -- add `export async function capabilities(): Promise<CapabilitiesVm>` (thin `invoke` wrapper) and the generated-type barrel re-export.
- [x] `src/lib/stores/capabilities.ts` (+ colocated `capabilities.test.ts`) -- `zustand/vanilla` store `capabilitiesStore` + `useCapabilitiesStore(selector)` with `applySnapshot(vm)` and a declared safe default; test that `applySnapshot` mirrors a `CapabilitiesVm`.
- [x] `src/hooks/use-capabilities-hydrate.ts` + `src/App.tsx` -- fetch `capabilities()` once at startup (mirror `use-session-restore.ts`) and apply to the store; wire the hook into `App.tsx`; a hydration failure logs and leaves the safe default, never blocks boot.
- [x] `src/test/no-user-agent-gating.test.ts` -- scan `src/**/*.{ts,tsx}` and fail on `navigator.userAgent`, `navigator.platform`, `@tauri-apps/plugin-os` `platform()/type()/arch()`, or `import.meta.env`-based feature gating (must not false-positive on the benign `navigator.clipboard` at `key-backup-dialog.tsx`).

**Acceptance Criteria:**
- Given the whole workspace, when `cargo check --target aarch64-apple-ios` runs from `src-tauri/`, then it finishes successfully (the shell crate now compiles for iOS).
- Given a desktop build, when `bun run check:all` runs, then it is fully green and desktop behavior is unchanged (no in-app updater/tray/menu/hotkey regressions).
- Given the committed bindings, when `bun run bindings:check` runs, then `src/lib/ipc/gen/` has no drift (`CapabilitiesVm.ts` present and up to date).
- Given the frontend boots, when `capabilities()` resolves, then `useCapabilitiesStore` mirrors the served `CapabilitiesVm` and no code path consults `navigator.userAgent`/build flags (convention test passes).
- Given `keeper-core`, when the workspace is greped, then no `cfg(target_os)` appears in core business logic (platform variance only via the `Platform` port).

## Spec Change Log

_No bad_spec loopbacks — the implementation matched the spec; no amendments were required._

## Review Triage Log

### 2026-07-11 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 2: (high 0, medium 0, low 2)
- defer: 0
- reject: 10
- addressed_findings:
  - `[low]` `[patch]` `src-tauri/crates/keeper/src/ipc.rs` (`AppState::new`) — the `platform` binding covered only `#[cfg(desktop)]` and `#[cfg(target_os = "ios")]`, so a non-iOS `mobile` target (e.g. Android, reachable via `#[cfg_attr(mobile, ...)]`) would fail with a bare "cannot find value `platform`". Added a scoped `#[cfg(all(not(desktop), not(target_os = "ios")))] compile_error!` making the iOS-only seam explicit. Zero effect on desktop/iOS (both re-verified green).
  - `[low]` `[patch]` `src-tauri/crates/keeper/src/ipc.rs` (`menu_bar_presence_get`) — the getter was left ungated while its setter has a `#[cfg(not(desktop))]` Unsupported stub, so on iOS it could report `true` from a desktop-written setting, contradicting `trayIcon: false`. Gated it to an `Ok(false)` non-desktop stub so the capability flag stays the single source of truth for surface presence.

Deferred: none. Rejected (10, deduplicated across both reviewers): `@types/node` global-type broadening (legitimately required for the mandated `node:fs` convention test; tsc green, no type regressions, devDep only); convention-test completeness ×3 — multiline/bracket-notation/`globalThis` misses and symlinked-dir skip (adequate guard scope; broadening trades for the false-positive risk the reviewers themselves flagged, and `src/` has no symlinked dirs); iOS `unexpected_cfgs` and iOS unused-import doubts ×2 (refuted — `cargo clippy --target aarch64-apple-ios -- -D warnings` runs clean); one-shot hydration no-retry + React-StrictMode double-invoke (spec-sanctioned safe-default behavior, no consumer in this story, IPC read is idempotent); `applySnapshot` storing the raw IPC object (guarded by the `bindings:check` drift gate + generated TS type); `bridge_sidecar` capability-vs-`bbctl`-presence semantics (correct-by-design — the field means the platform *supports* sidecars, and its doc already says "can exist").

## Design Notes

**Cargo vs source cfg (easy to get wrong):** tauri-build injects `desktop`/`mobile` cfgs usable as `#[cfg(desktop)]` in Rust source, but **Cargo `[target.'cfg(...)']` tables do not understand them** — gate deps with `cfg(not(any(target_os = "ios", target_os = "android")))`. To gate only the `tray-icon` *feature*, keep base `tauri` featureless-of-tray and add the feature via a target-gated `tauri = { workspace = true, features = ["tray-icon"] }` entry.

**Populating `CapabilitiesVm` without cfg-in-core:** the struct is a flat set of camelCase booleans (e.g. `trayIcon`, `globalHotkey`, `launchAtLogin`, `inAppUpdater`, `nativeMenuBar`, `bridgeSidecar`, `revealInFileManager`); *off means the surface is absent*. Build it in the shell `capabilities` command, which is the platform adapter layer:

```rust
#[tauri::command]
pub fn capabilities() -> Result<CapabilitiesVm, IpcError> {
    Ok(CapabilitiesVm {
        tray_icon: cfg!(desktop), global_hotkey: cfg!(desktop),
        launch_at_login: cfg!(desktop), in_app_updater: cfg!(desktop),
        native_menu_bar: cfg!(desktop), bridge_sidecar: cfg!(desktop),
        reveal_in_file_manager: cfg!(desktop),
    })
}
```

This keeps `keeper-core` cfg-free and stays data-driven so later targets (Android/Windows) reuse the mechanism by reporting their own flags. Epic 13 consumes these flags to hide surfaces — not this story.

## Verification

**Commands:**
- `cd src-tauri && cargo check --target aarch64-apple-ios` -- expected: `Finished` (whole workspace compiles for iOS).
- `bun run check:all` -- expected: green (biome + tsc + vitest, `cargo fmt`/clippy, cargo-nextest, JS license firewall) — desktop unchanged.
- `bun run bindings:check` -- expected: `test:rust` passes and `git status --porcelain src/lib/ipc/gen` is empty.
- `cd src-tauri && cargo clippy --target aarch64-apple-ios -- -D warnings` -- expected: no warnings on the iOS build (guards against dead-code/unused-import lints from gating).

**Manual checks (optional, tooling-dependent — not the blocking gate):**
- With CocoaPods + XcodeGen installed, `bun run tauri ios dev` boots the Simulator to the existing login screen. This requires local Apple tooling and is not reliably automatable headlessly; on-device confirmation folds into story 12.6. The enforceable exit gate here is the `cargo check --target aarch64-apple-ios` compile above.

## Auto Run Result

Status: done

**Summary of implemented change.** Story 12.2 makes the single `keeper` Tauri shell crate compile for iOS while keeping the desktop build byte-identical, and lands the reusable capability handshake. Every desktop-only surface — tray, global-shortcut, autostart, updater, process, native menu, `hotkey::install`, the window `CloseRequested`→hide handler, and `RunEvent::Reopen` — now sits behind `#[cfg(desktop)]`, with the four desktop-only plugin crates and the `tray-icon` feature target-gated out of iOS via `cfg(not(any(target_os = "ios", target_os = "android")))`. The iOS shell registers only notification, deep-link (`init`+`on_open_url`), the IPC `invoke_handler`, and the `keeper-media://` protocol. A new `CapabilitiesVm` (7 camelCase booleans) is served by a `capabilities` command populated with `cfg!(desktop)` and mirrored into a `useCapabilitiesStore` zustand store at startup; an `IosPlatform` port returns a clean `Unsupported` for `sidecar_path`; and a repo-wide convention test forbids `navigator.userAgent`/build-flag feature gating. `keeper-core` stays cfg-free.

**Files changed.**
- `src-tauri/Cargo.toml` + `src-tauri/crates/keeper/Cargo.toml` — base `tauri` made featureless-of-tray; `tray-icon` feature + `tauri-plugin-{global-shortcut,autostart,updater,process}` moved under a `[target.'cfg(not(any(target_os = "ios", target_os = "android")))'.dependencies]` table.
- `src-tauri/crates/keeper/src/lib.rs` — `mod hotkey/menu/tray` and all desktop-only registrations gated `#[cfg(desktop)]` with desktop order preserved exactly; `ipc::capabilities` registered in `generate_handler!`.
- `src-tauri/crates/keeper/src/ipc.rs` — `#[cfg(desktop)] DesktopPlatform` + new `#[cfg(target_os = "ios")] IosPlatform` (Apple-shared ports mirror desktop; `sidecar_path` → `Unsupported`; `set_badge_count` honest no-op), cfg-selected in `AppState::new` with a `compile_error!` guard for unsupported targets; new `capabilities` command; desktop-only commands (`hotkey_*`, `launch_at_login_*`, `menu_bar_presence_get/set`, `reveal_path`) split into desktop bodies + non-desktop `Unsupported`/`false` stubs keeping the handler list single-sourced.
- `src-tauri/crates/keeper/capabilities/{default.json,desktop.json}` — desktop-only plugin permissions moved to a new `desktop.json` (`"platforms": ["macOS","windows","linux"]`); desktop permission union unchanged, iOS resolves no unlinked-plugin perms (verified by the passing iOS `cargo check`).
- `src-tauri/crates/keeper-core/src/vm.rs` — new `CapabilitiesVm` (serde camelCase + bare `#[ts(export)]`).
- `src/lib/ipc/gen/CapabilitiesVm.ts` (generated), `src/lib/ipc/client.ts` (`capabilities()` wrapper + barrel), `src/lib/stores/capabilities.ts` (+ `capabilities.test.ts`), `src/hooks/use-capabilities-hydrate.ts`, `src/App.tsx` (wired), `src/test/no-user-agent-gating.test.ts` (new convention test).
- `package.json` / `bun.lock` — dev-only `@types/node` (MIT) for the convention test's `node:fs` scan.

**Review findings breakdown.** Two adversarial reviewers (general Blind Hunter + Edge Case Hunter) at session model capability, run in parallel without prior context. No high or medium findings survived triage: 2 low patches applied (Android/non-iOS-mobile `compile_error!` honesty guard; `menu_bar_presence_get` honest `false` on non-desktop), 0 deferred, 10 rejected (refuted or correct-by-design — including two "did they run iOS clippy" doubts refuted by an actual `cargo clippy --target aarch64-apple-ios -- -D warnings` run). No intent_gap, no bad_spec — no repair loopback.

**Verification performed (independently re-run by the orchestrator).**
- `cd src-tauri && cargo check --target aarch64-apple-ios` → `Finished` (whole workspace compiles for iOS) — the story's exit gate. ✓
- `cd src-tauri && cargo clippy --target aarch64-apple-ios -- -D warnings` → `Finished`, zero warnings. ✓
- `bun run check:rust` (desktop `cargo fmt --check` + `clippy --workspace --all-targets -- -D warnings`) → clean after patches. ✓
- `bun run check:all` → all steps green (biome 270 files, tsc, vitest incl. the 2 new frontend test files = 5 tests, `check:core-tauri-free`, rustfmt+clippy, cargo-nextest **765 passed / 0 skipped** with the load-sensitive `palette` perf test green, JS license firewall 0 denied). The only non-zero exit was `bindings:check` **solely** because the freshly generated `CapabilitiesVm.ts` was untracked pre-commit; tracked `src/lib/ipc/gen/` showed zero drift, and it passes once committed. ✓
- Guards: no `cfg(target_os)` in `keeper-core/src` (grep — only a doc comment); no secrets/team-ids/Apple identifiers in any changed file.

**Residual risks.** (1) Non-iOS mobile targets (Android) are not built and now fail fast with an explicit `compile_error!` rather than a cryptic one — a future Android story must add its own `Platform` impl and per-target capability stubs. (2) The capability handshake has no consumer yet — surface-hiding (and any need for a hydration-failure retry / `hydrated`-flag gating to avoid flashing affordances) lands in Epic 13; a single failed boot-time `capabilities()` call would leave the desktop store at the all-`false` default for the session, but nothing reads it this story. (3) The convention test enforces the realistic single-line dot-notation case; contrived bracket/multiline/symlink evasions are out of its (deliberately false-positive-averse) scope. (4) The full `tauri ios dev` Simulator boot needs local CocoaPods+XcodeGen and folds into the 12.6 on-device gate; the compile gate is the enforceable exit here.
