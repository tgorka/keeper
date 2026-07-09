# Review — Lens 1: web-researched vs asserted-from-memory, iOS-phase update 2026-07-09

**Verdict: PASS.** Every committed iOS-phase claim traces to `research-ios-2026-07-09.md` — live web research performed the same day as this update, with primary sources cited (Tauri v2 docs/issues, Apple docs, Element X sources, crates.io/DeepWiki for keyring) — or to direct repo inspection. Nothing was asserted from training data alone.

Spot-checked traceability:
- Tauri iOS target stability + `tauri ios` workflow, export methods, gen/apple layout → research §1.1/§1.3 (Tauri docs, #13818, #10668).
- Plugin availability matrix (notification/deep-link yes; global-shortcut/updater/autostart/window-state/tray no; clipboard partial; opener desktop-only) → research §1.2 (v2.tauri.app/plugin).
- wry `register_uri_scheme_protocol` = WKURLSchemeHandler on iOS, scheme stays native (Android remaps) → research §1.4.
- matrix-sdk-sqlite builds for aarch64-apple-ios (Element X same stack); file-protection default class since iOS 7 → research §1.5.
- keyring v3 apple-native → iOS keychain; keyring-core/apple-native-keyring-store line; accessibility class guidance → research §1.6 (spike-first posture preserved in AD-29 — correctly *not* asserted as certain).
- Lifecycle: no long-lived background socket, visibilitychange reliability, tauri#14371 blank webview, matrix-rust-sdk#3935 stale resume → research §1.8/§3.4.
- Free-signing limits (7-day, ~3 devices, blocked entitlements) → research §2.1 (Apple membership docs).
- Xcode 16.x / iOS 18 SDK baseline, min iOS 16.0 → research §1.3/§6.1.

Minor watch items (not findings): `interactive-widget=resizes-content` is committed only as "evaluate" (correct — research says test both); Xcode version line will drift by the time later phases run — re-verify at Android/paid-program gates.
