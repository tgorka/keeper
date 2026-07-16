---
title: "Addendum: keeper PRD"
status: final
created: 2026-07-03
updated: 2026-07-16
---

# Addendum — keeper PRD

Depth referenced by the PRD that belongs in downstream documents (UX spec, architecture, epics) or supports FRs without earning PRD body space. The brief addendum (`_bmad-output/planning-artifacts/briefs/brief-keeper-2026-07-03/addendum.md`) remains authoritative for locked technical constraints; this file carries PRD-phase additions and points into it rather than duplicating.

## 1. Locked technical constraints (inherited — architecture must honor)

See brief addendum §1–§2 in full. Summary of what the PRD's FRs/NFRs silently assume:

- **Stack:** Tauri 2 (2.11.x) + Rust core + React 19 + TypeScript + shadcn/ui (Tailwind 4); Apache-2.0.
- **Matrix:** matrix-sdk / matrix-sdk-ui / matrix-sdk-sqlite 0.18+ embedded directly (no FFI); SyncService, RoomListService, Timeline, SendQueue, EventCache; one SDK `Client` per Account; Unified Inbox = merged RoomList streams in Rust (FR-18).
- **Sync:** Simplified Sliding Sync (MSC4186) only (FR-5, FR-8).
- **Auth providers behind one interface:** password, OIDC/MAS (MSC3861), Beeper email-code JWT ported from Apache-2.0 bbctl `api/beeperapi` (FR-1–FR-3).
- **IPC:** Tauri commands for actions; `tauri::ipc::Channel<T>` streaming `VectorDiff` batches; `keeper-media://` custom protocol for decrypted media (FR-13, NFR-9).
- **Undo-Send:** delay before SendQueue dispatch (FR-46); post-dispatch = Redaction (FR-47).
- **Drafts:** local SQLite + per-Room Matrix account data mirror (`Room::save_composer_draft` local; custom account_data cross-device) (FR-38–FR-39).
- **Quality gates:** Biome 2.x, rustfmt + clippy `-D warnings`, Vitest 4, cargo-nextest, cargo-deny (GPL/AGPL firewall), lefthook, pnpm 10, GitHub Actions macOS arm64 with tauri-action (NFR-12–NFR-13).

## 2. Network Risk Tier table (data source for FR-30, FR-44)

| Tier | Networks | In-product guidance |
|---|---|---|
| Low risk | Matrix (native), Telegram, Google Messages/Chat/Voice | Recommend by default; no warning beyond label |
| Maintenance-heavy | Signal, WhatsApp (personal use), Discord, Slack | Default-on with clear disclosure; expect session churn |
| Volatile / opt-in | Instagram, Messenger, LinkedIn, X Chat | Explicit ToS/ban acknowledgment before connect; expect login friction |
| Conditional | iMessage (user's own Mac only; v1.x) | "Advanced, macOS-only, may break on OS updates" |
| Out of scope | iMessage without a Mac, official X DM API, WeChat | Never promised |

Per-Network coupling caveats (FR-44) ride the same data structure — e.g., WhatsApp couples sending read receipts with seeing others'. DMA note: EU WhatsApp third-party interop (live Nov 2025, BirdyChat/Haiket) is monitored upside only; keeper does not build against it.

## 3. Beeper Account coverage surface (supports FR-3, FR-7, FR-29)

A keeper-connected Beeper Account sees: Matrix-native Chats + Beeper Cloud Bridge Rooms + bbctl self-hosted Bridge Rooms on matrix.beeper.com (hungryserv — partial C-S API; test early, PRD Open Question 3). It does **not** see On-Device Connection chats (WhatsApp/Signal in official Beeper apps since 2025-07; more Networks migrating through 2026). Future option (post-MVP, never a foundation): Beeper Desktop API companion mode (localhost:23373, OAuth+PKCE, MIT SDKs) reaches on-device chats when Beeper Desktop is installed.

Beeper auth flow (private, unversioned — provider-isolated per PRD §8): `POST /user/login` → `POST /user/login/email` → `POST /user/login/response` → JWT → `org.matrix.login.jwt` on matrix.beeper.com. Port from Apache-2.0 bbctl; UI labels it unofficial (FR-3).

## 4. FR → v1.x backlog traceability (MoSCoW continuity)

The PRD's §6.2 fast-follow list descends from market research §5 / Appendix A and brief addendum §5. Items promoted into MVP by owner requirement (vs. research's Should/Could): Approval Pane drafts (FR-38–41), Undo-Send (FR-46–47), Spaces as room-group views (FR-23). Items the research rated Must that the PRD keeps Must: everything in §6.1. Items research rated Should that stay v1.x: low-priority view, message requests, labels/filtered views, snooze/reminders, scheduled send (local), note-to-self, bridge health dashboard. Could (validate first): agent-proposed Drafts API/MCP, local Whisper transcription, iMessage helper, themes.

## 5. Rationale records (PRD-phase decisions)

- **Local Archive vs. remote deletions (FR-36):** preserving remotely edited/deleted content locally is the product's core promise ("history belongs to people, not platforms") and applies only to the user's own device — equivalent to the user having read and saved the message when it arrived. The settings toggle ("honor remote deletions locally") exists for users who prefer norm-following behavior. Redaction is still honored in the *timeline view* either way; the divergence is only in archive retention. This is deliberately different from Matrix client convention and is disclosed in settings copy.
- **Explicit-approval invariant as product guardrail (FR-41):** framed as a PRD-level contract so the post-MVP agent API cannot erode it through implementation drift. Two dispatch triggers only (composer send, Approval Pane approve); both user-initiated.
- **Bridge health in MVP vs. dashboard in v1.x (FR-28):** the 60 s surfacing bar and re-login prompts are MVP because silent bridge death is a top-2 competitor complaint; the aggregate dashboard/alert-center is organization, not detection, and defers safely.
- **Flagship-three quality gate (SM-2):** "flawless" is scoped to Telegram/WhatsApp/Signal so the release gate is falsifiable; other Networks work best-effort through the same UX. Prevents the unbounded "every bridge perfect" trap.
- **Notification pipeline:** local sync loop only — no APNs/push infra on desktop (would violate client-only). *(Updated 2026-07-09: the iOS phase records push/NSE as an explicit paid-program decision gate — PRD §13.5; foreground-only notifications until that gate opens.)*

## 6. UX-phase pointers (not requirements)

- Beeper patterns worth studying (market research §1.5): Favorites vs. Pins two-tier, inbox-zero Archive flow, ⌘K palette scope, Spacebar network filters (v1.x), Incognito manual-release interaction.
- First-Run Wizard (FR-31) is the highest-leverage UX surface in the product; the setup-cliff mitigation order is: wizard → companion-stack docs (docker-compose, docs-only) → managed-host pointers (etke.cc-style) → Beeper Account path.
- Approval Pane (FR-40) should be designed with the future agent-proposal column in mind (proposer attribution, batch approve/discard) without shipping any of it in MVP.

## 7. iOS phase technical constraints (added 2026-07-09; source: `research-ios-2026-07-09.md`)

Locked by the iOS research (authoritative for the phase; PRD §13 states the *what*, this records the *how* architecture must honor):

- **Plan A confirmed (AD-24):** Tauri 2 mobile target reusing keeper-core and the existing IPC contract; Plan B (UniFFI + SwiftUI shell) shelved with recorded revisit triggers (PRD §13.8).
- **Plugin availability:** notification and deep-link work on iOS; global-shortcut, updater, autostart, window-state, tray are desktop-only → `#[cfg(desktop)]` seams in the shell crate; desktop-only plugin deps behind target-gated Cargo dependencies; clipboard falls back to the web Clipboard API; opener replaced by a minimal native call where needed.
- **Media transport:** wry implements `register_uri_scheme_protocol` via WKURLSchemeHandler on iOS — `keeper-media://` URL format identical to macOS; in-memory Range slicing carries a RAM cost (capped per NFR-16; disk-backed streaming is deferred work).
- **Stores:** `matrix-sdk-sqlite` (bundled rusqlite) builds for `aarch64-apple-ios` — the same stack Element X ships; file protection `NSFileProtectionCompleteUntilFirstUserAuthentication` (not `Complete` — WAL access after lock); DB dirs `isExcludedFromBackup`; all account state under one `data_dir()` root for a cheap future App Group move.
- **Keychain:** `keyring` v3 apple-native backend targets the iOS keychain; set `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`; contained fallback = direct `security-framework` generic-password calls behind the `Platform` port.
- **Lifecycle hooks:** webview `visibilitychange` as the zero-native stopgap; micro Swift plugin on `UIApplication` background/foreground notifications as the correct path; SyncService pause/offline-mode/resume from matrix-sdk-ui.
- **Signing/distribution:** free Personal Team (7-day profiles, ~3 devices, blocked entitlements: push, App Groups, iCloud, associated domains); automatic signing + re-sign-afterwards flow for shared IPAs (Sideloadly/zsign); AltServer auto-refresh as QoL; bundle ID stable and shared with macOS.
- **Toolchain:** full Xcode + CocoaPods + `aarch64-apple-ios{,-sim}` rust targets; `gen/apple` committed (minus `build/`), persistent edits in `project.yml`/`Info.plist` only; min iOS 16.0 set explicitly; CI PR gate `cargo check --target aarch64-apple-ios`.
- **Safe areas/keyboard:** `viewport-fit=cover` + `contentInsetAdjustmentBehavior = .never` + `env(safe-area-inset-*)` CSS vars; keyboard inset via `visualViewport` (evaluate `interactive-widget=resizes-content`).
- **Epic shape (research §5):** Epic 12 walking skeleton (UI-free, retires toolchain/signing/core risks) → Epic 13 phone shell ∥ Epic 14 platform behavior → Epic 15 fit-and-finish + paid-program decision-gate story.

## 8. Screen Recording phase technical constraints (added 2026-07-16; source: `research-recording-2026-07-16.md`)

Locked by the recording research (authoritative for the phase; PRD §14 states the *what*, this records the *how* architecture must honor):

- **Route (b) locked — Swift sidecar:** `keeper-rec` (SwiftPM, in-repo, Apache-2.0) owns SCK stream setup → sample buffers → dual-AVAssetWriter segmented fMP4 writing; spawned launch-on-demand via `Platform::sidecar_path` + Tauri `externalBin` (bbctl mechanism, Story 6.7 / AD-16); control protocol = NDJSON-RPC over stdio (`getCapabilities` version/feature/permission handshake, `listSources`, `start`, `stop`; interleaved `state`/`segmentClosed` events). Rejected alternatives: in-process Rust bindings (`screencapturekit-rs` has no writer half; cidre/objc2 = large unsafe surface vs. the function-level audited-unsafe policy; Cap's fork-heavy posture is the cautionary tale) and ffmpeg/avfoundation (no per-app capture, no system audio without a virtual driver, GPL vs. cargo-deny firewall).
- **API mapping:** full screen / selected app = `SCContentFilter` over `SCShareableContent`; system audio = `capturesAudio` (13+) scoped by the same filter, `excludeCurrentProcessAudio = true`; mic = in-stream `captureMicrophone` + `microphoneCaptureDeviceID` on 15+, parallel `AVCaptureSession` on 13–14 (same writer either way); webcam = `AVCaptureDevice.DiscoverySession` + second AVAssetWriter, host-clock anchored. `SCRecordingOutput` (15+) rejected as primary writer: one output per stream, no segment rotation.
- **Segmentation mechanics:** dual-writer gapless handover — start writer B at the next keyframe PTS, route to both until B's first keyframe lands, finalize A asynchronously; size trigger = bytes-budget deadline corrected against observable on-disk growth (fMP4 makes size observable live); duration-cap fallback. Container: `outputFileTypeProfile = .mpeg4CMAFCompliant`, ~4 s fragments; clean finalize defragments to ordinary MP4.
- **Session layout:** `<folder>/keeper-rec <local timestamp>/` with `manifest.json` (atomic rename on every segment close / status change; states `recording` → `finalized` | `recovered`), `screen-####.mp4`, `camera-####.mp4`. Recovery pass on startup and before each new recording.
- **TCC facts:** Screen Recording detected via `CGPreflightScreenCaptureAccess()`, requested via `CGRequestScreenCaptureAccess()` (one real prompt per app lifetime); Settings deep link `x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture`; `NSMicrophoneUsageDescription`/`NSCameraUsageDescription` in keeper's bundle Info.plist (Tauri infoPlist merge). Child-process TCC attribution = responsible process (keeper) — the sidecar must be spawned, never a LaunchAgent. macOS 15+: ad-hoc-signed binaries silently rejected for SCK (Cap #1722) → Apple Development-cert signing required for local dev of this feature; monthly re-auth nag for non-picker SCK accepted + disclosed (picker path and `persistent-content-capture` entitlement are later/gated).
- **Signing/CI:** explicitly codesign `keeper-rec` (hardened runtime + keeper's entitlements) in CI before `tauri build` (externalBin notarization rough edge, tauri#11992); aarch64-only, no lipo; SwiftPM build step rides the existing macOS signing runners.
- **Tray extension (Story 10.3 surface):** second/third tray icon assets (record-dot, warning badge) via `TrayIcon::set_icon`; 1 Hz menu-item text tick; recording temporarily forces tray presence with exact restore at stop; quit-while-recording = warn → `stop` RPC → flush → kill-timeout guard. The macOS purple capture pill is system-owned and untouched.
- **Epic shape (research §8):** R.1 walking skeleton (sidecar build/sign/bundle + RPC handshake + permission flow + full-screen+system-audio to one fMP4) → R.2 segmentation/manifest/recovery (+ soak) → R.3 tray states + honest quit → R.4 app-picker capture → R.5 device pickers + mic track + hot-unplug → R.6 webcam separate file → R.7 guards/settings/gating audit/docs (nag, pill, dev-signing).
