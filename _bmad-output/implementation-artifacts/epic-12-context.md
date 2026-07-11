# Epic 12 Context: iOS Walking Skeleton — Build, Sign, Run

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

This epic proves that keeper compiles, signs, and runs on a physical iPhone before any phone-UX work begins, retiring the three existential risks — toolchain, free-signing, and running the Rust core on iOS — up front. It is deliberately UI-free and simulator/compile-first: the same `crates/keeper` shell builds as an iOS staticlib, all desktop-only surfaces compile-gate out cleanly, the keychain and media protocol are proven through the existing platform ports, and CI gains a permanent iOS compile check. The epic ends at the SM-7 on-device gate (OIDC deep-link login, room list, E2EE text send/receive, relaunch-restore under free Personal Team signing); the phone-shell epics (13, 14) start only after that gate passes.

## Stories

- Story 12.1: iOS Project Init and Repo Integration
- Story 12.2: Desktop/Mobile Compile Seam and Capability Handshake
- Story 12.3: iOS Platform Port — Keychain Spike and Data Directory
- Story 12.4: Media Protocol on WKURLSchemeHandler with Capped Buffers
- Story 12.5: iOS Compile Check in CI
- Story 12.6: On-Device Walking-Skeleton Validation (SM-7 Gate)

## Requirements & Constraints

- The iOS target must build and run from the existing cargo workspace via `tauri ios`, reusing the same keeper-core crate and IPC contract (no separate iOS crate, no SwiftUI shell). Simulator/compile paths must work without a physical device wherever possible; only on-device validation (12.6) is human-in-the-loop and requires the owner's iPhone.
- Free Personal Team signing is the distribution baseline: 7-day provisioning-profile expiry (re-armed by re-running the dev deploy), ~3 devices, ~10 App IDs/window. Blocked entitlements: APNs push, App Groups, iCloud, universal (`https://`) links, most background modes. Custom-scheme `keeper://` deep links still work (need no entitlement). No team id may land in git.
- Sessions must survive relaunch and the 7-day re-sign cycle with all local data intact — this depends on a stable bundle identifier shared with macOS.
- No token, secret, or crypto/decrypted-media bytes may cross the IPC boundary or reach JS-accessible storage or logs (Rust-core confinement holds on iOS).
- The iOS compile check runs on the existing macOS CI runner, compile-only (no signing, no simulator build, no Apple credentials). It blocks by failure from 12.5 onward; flipping it to a *required* branch-protection status is deferred to Epic 15.
- Desktop build behavior must remain byte-identical throughout; all existing desktop quality gates (`bun run check`, `check:rust`, `test:rust`, `cargo deny check`) stay green.
- Success bar (SM-7 exit gate): on-device OIDC deep-link login, room list load, E2EE text send/receive in one room, and relaunch session-restore, all under free signing. Resume/lifecycle behavior is exercised (background/foreground/overnight suspension) and any blank-webview occurrence is recorded as input to a later hardening story rather than fixed here.

## Technical Decisions

- **One shell crate, cfg-gated.** iOS is the same `crates/keeper` crate built as a staticlib via `tauri ios` (`tauri::mobile_entry_point` — the CLI validates the Mach-O exports `start_app`). Desktop-only surfaces (tray module + `tray-icon` feature, global-shortcut, autostart, updater, window-state, desktop deep-link registration) sit behind `#[cfg(desktop)]`/`#[cfg(target_os)]` gates with target-gated Cargo deps. The iOS shell registers only notification + mobile deep-link + IPC + media protocol. Clipboard uses the web Clipboard API; "open in browser" uses a minimal native open call. No in-app updater code path exists on iOS.
- **keeper-core stays platform-free.** No `cfg(target_os)` in business logic; all platform variance enters through the `Platform` port only.
- **Capabilities handshake.** A single `CapabilitiesVm` in `keeper-core::vm` (serde + ts-rs, camelCase, exported to the generated IPC bindings dir) is served at startup into a `useCapabilitiesStore` zustand mirror, data-driven per platform. An off capability means the surface does not render at all — no dead buttons. The frontend never consults `navigator.userAgent` or build flags for feature gating (enforced by a convention test). `Platform::sidecar_path` returns a clean Unsupported `IpcError` on iOS (bbctl/sidecar bridge management is desktop-only; iOS cannot spawn child processes).
- **Generated Apple project.** `tauri ios init` generates the Apple project under the shell crate (i.e. `gen/apple` relative to the crate holding `tauri.conf.json`). Commit `gen/apple` with `build/` gitignored. Persistent edits live ONLY in `project.yml` (XcodeGen spec; set minimum iOS 16.0 explicitly, theme-matched background color), `Info.plist` (`CFBundleURLTypes` for `keeper://`), and `*_iOS/` sources — regenerating the `.xcodeproj` must lose nothing (never hand-edit the `.xcodeproj`). Signing via `bundle.iOS.developmentTeam` in conf or `TAURI_APPLE_DEVELOPMENT_TEAM` env. Bundle id stays alphanumeric+dots (no hyphens/underscores) and identical to macOS.
- **Toolchain prerequisites** (for docs): full Xcode 16.x, `xcode-select`/`runFirstLaunch`, rust targets `aarch64-apple-ios` + `aarch64-apple-ios-sim`, CocoaPods. Ensure the pinned toolchain includes the iOS targets or clean-machine/CI builds fail. `tauri ios dev` is the loop; vite must listen on `0.0.0.0` for device hot-reload.
- **Keychain (spike-first).** Sessions go through the existing keyring/apple-native `Platform` port targeting the iOS keychain with `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` (readable by a resumed sync loop, invisible to other apps, excluded from iCloud Keychain sync). The story records a spike verdict: keep `keyring` as-is, or switch to the contained fallback — direct `security-framework` generic-password calls behind the *same* port with call sites unchanged. On-device confirmation folds into 12.6.
- **Data directory & at-rest.** `Platform::data_dir()` resolves to the app container (Application Support), with ALL account state under that one root (a future App Group move becomes a path change, not a migration). DB directories carry `NSFileProtectionCompleteUntilFirstUserAuthentication` (never `Complete` — WAL access after lock would break a suspended sync loop) and are marked `isExcludedFromBackup` (re-syncable, potentially multi-GB).
- **Media protocol.** `keeper-media://` runs unchanged on iOS (wry → WKURLSchemeHandler, iOS 11+); the URL format is identical to macOS, so the frontend needs no media-URL helper. Range 200/206/416 seeking works in-memory from the SDK cache. The in-memory Range-slicing buffer must be capped (named constant + unit test asserting the cap) to survive jetsam limits; WebKit scheme-task invalidation is tolerated by the fire-and-forget responder; disk-backed streaming is recorded as deferred work, not implemented. A retry-on-cache-miss path must re-fetch and render media after a force-quit with a cold cache.
- **Sync is foreground-only** on iOS (no background socket without paid APNs) — relevant here only as the resume-smoke exercise in 12.6; the full lifecycle mechanism is a later epic. Sliding sync (SSS) resumes from cached UI instantly via the snapshot-then-diff mirror.

## Cross-Story Dependencies

- 12.1 has no story dependency (desktop Epics 1–11 are complete) and establishes the generated Apple project the rest build on.
- 12.2 depends on 12.1 and is the fan-out point: 12.3, 12.4, and 12.5 all depend on 12.2 (the compile seam + `cargo check --target aarch64-apple-ios` passing) and can proceed in parallel after it.
- 12.6 depends on all of 12.1–12.5 and is the human-in-the-loop SM-7 gate — the automation loop defers it to the coordinator rather than escalating; all other Epic 12 stories are device-free.
- The capability-handshake mechanism (`CapabilitiesVm`) lands here but its UI surface-hiding leg is completed in Epic 13; the keychain and resume/blank-webview observations from 12.3/12.6 feed the platform-behavior hardening in Epic 14.
