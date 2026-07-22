---
stepsCompleted: [1, 2, 3, 4]
inputDocuments:
  - _bmad-output/planning-artifacts/prds/prd-keeper-2026-07-03/prd.md
  - _bmad-output/planning-artifacts/prds/prd-keeper-2026-07-03/addendum.md
  - _bmad-output/planning-artifacts/architecture/architecture-keeper-2026-07-03/ARCHITECTURE-SPINE.md
  - _bmad-output/planning-artifacts/ux-designs/ux-keeper-2026-07-03/DESIGN.md
  - _bmad-output/planning-artifacts/ux-designs/ux-keeper-2026-07-03/EXPERIENCE.md
  - _bmad-output/planning-artifacts/research-ios-2026-07-09.md
  - _bmad-output/planning-artifacts/research-recording-2026-07-16.md
  - docs/project-context.md
generated: 2026-07-03
updated: 2026-07-16 (Screen Recording phase — Epics 16–20 appended)
mode: headless
storyCount: 116
epicCount: 20
---

# keeper - Epic Breakdown

## Overview

This document provides the complete epic and story breakdown for keeper, decomposing the PRD (FR-1–FR-54, NFR-1–NFR-14), the Architecture Spine (AD-1–AD-25), and the UX design contract (DESIGN.md + EXPERIENCE.md) into implementable stories. Epics are ordered for incremental delivery on the existing Tauri + React scaffold (matrix-sdk 0.18 already wired in). Epic 1 produces a usable walking skeleton and doubles as the PRD OQ-1 exit gate (SSS + timeline channel + send/receive in a release build). Every story is sized for one dev session, carries acceptance criteria mapped to FR/NFR ids, and lists explicit dependencies on previous stories only.

Post-MVP items (PRD §6.2) are flagged in the "Post-MVP — Not Storied" section at the end and deliberately have no stories.

**Phase 2 increment (2026-07-09):** with all 11 desktop epics (63 stories) done, Epics 12–15 add the iOS/iPhone client per PRD §13 (FR-55–FR-65, NFR-15–NFR-18), the Architecture Spine iOS increment (AD-26–AD-32, AD-24 Plan A confirmed), the UX phone-tier contract (EXPERIENCE.md `Responsive & Platform` + DESIGN.md phone tokens), and the iOS technical research (`research-ios-2026-07-09.md` §5/§7/Appendix A). Epic 12 is a UI-free walking skeleton that retires the toolchain/signing/core-on-iOS risks and ends at the SM-7 on-device gate; Epics 13 (phone shell) and 14 (platform behavior) can proceed in parallel after it; Epic 15 hardens release hygiene. Stories are implementable without a physical device wherever possible (simulator and compile gates); exactly two stories are explicitly human-in-the-loop (12.6 on-device skeleton validation, 15.6 final device install) so the automation loop defers them to the coordinator rather than escalating.

**Phase 3 increment (2026-07-16):** Epics 16–20 add the macOS Screen Recording phase per PRD §14 (FR-66–FR-76, NFR-19–NFR-22) + addendum §8, the Architecture Spine recording increment (AD-33–AD-39, no PRD amendment required), the UX `Screen Recording (macOS — Phase 3)` contract (EXPERIENCE.md recording surfaces/state table + DESIGN.md recording-red token and components), and the recording technical research (`research-recording-2026-07-16.md` §8 story sketch R.1–R.7, whose route/format/floor are adopted, not relitigated). The route is locked: a first-party Swift sidecar `keeper-rec` (ScreenCaptureKit + AVAssetWriter) spawned launch-on-demand over NDJSON-RPC stdio (the bbctl precedent), with a platform-free `keeper-core::recording` module owning the state machine, manifest, ledger, and recovery. Epic 16 is a walking skeleton that retires the existential risks — TCC permissions, sidecar signing, capture-to-file — ending at the R.1 / SM-9-seed exit (a real recording plays back); Epics 17 (segmentation & recovery) and 18 (tray & loud failures) build on it; Epic 19 adds sources/devices; Epic 20 adds webcam, guards, docs, and the SM-9/SM-10 phase acceptance. The capture surfaces render only behind the `recording` capability flag (FR-66), macOS ≥ 13.0; iOS never records. Recording adds zero network destinations (FR-76). Stories are implementable with compile gates, unit tests, and stub sidecars wherever possible; exactly three stories are explicitly human-in-the-loop — 16.6 (first real capture on dev-signed hardware), 20.5 (4 h soak + CPU/memory envelope on reference hardware), 20.6 (SM-9/SM-10 phase acceptance) — because macOS 15+ silently rejects ScreenCaptureKit for ad-hoc-signed binaries (Cap #1722), so real capture requires an Apple Development certificate and a physical Mac; the automation loop defers those three to the coordinator rather than escalating.

## Requirements Inventory

### Functional Requirements

- FR-1: Password login (homeserver + username + password; well-known discovery; inline errors naming the cause)
- FR-2: OIDC login via MAS/MSC3861 (system browser flow; cancel leaves no partial Account)
- FR-3: Beeper email-code JWT login (unofficial-API label; distinct "Beeper login unavailable" failure state)
- FR-4: Unlimited concurrent multi-account (no count-gated code path; all Accounts merge into the Unified Inbox)
- FR-5: Homeserver SSS capability verification at login, failing before Account creation with actionable error
- FR-6: Account management (list, per-Account state, sign out with explicit keep/delete Local Archive choice)
- FR-7: Beeper On-Device Connection coverage disclosure (pre-completion + persistent in settings)
- FR-8: Sync via Simplified Sliding Sync only; clean resume across restarts and offline periods
- FR-9: Send/receive text with local echo, offline-resilient queue, visible per-message states (sending/sent/failed-retry)
- FR-10: Replies rendered inline both directions, incl. bridged; jump-to-original
- FR-11: Edits (edit own; received edits render latest + "Edited" marker; archive keeps priors per FR-36)
- FR-12: Reactions (add/remove; aggregated counts)
- FR-13: Media and files (send/receive; thumbnails; progress; decrypted media via custom protocol, never base64 IPC)
- FR-14: E2EE with Cross-Signing, Device Verification (emoji/SAS + QR), key backup restore; explicit UTD states
- FR-15: Redaction (delete for everyone) with best-effort bridged framing
- FR-16: Read receipts and typing indicators, display + emission, subject to Incognito
- FR-17: History pagination — Local Archive first, then Homeserver; no UI freeze at 10k events
- FR-18: Unified Inbox — single chronological list across all Accounts and Networks
- FR-19: Unread management (unread/mention badges; manual mark read/unread)
- FR-20: Archive view with auto-return on new activity; state persists and syncs where representable
- FR-21: Favorites — always-visible section, one interaction from anywhere
- FR-22: Pins — top strip, out of chronological flow, user-orderable
- FR-23: Spaces as room-group views (view and filter only)
- FR-24: Network and Account attribution on every Chat row/header; simple per-Network filter
- FR-25: Bridge discovery on each Homeserver with status, zero-config on standard mautrix deployments
- FR-26: Native Bridge login via bridgev2 provisioning API (QR/code rendered natively; distinct states)
- FR-27: Bridge Bot command driving fallback (same native flow; raw bot chat never hidden)
- FR-28: Bridge Session health monitoring; surfaced ≤ 60 s; persistent unhealthy state; one-click re-login path
- FR-29: bbctl integration for Beeper self-hosted Bridges (optional sidecar; guided install when absent)
- FR-30: Network Risk Tier labeling, data-driven; volatile tier requires explicit acknowledgment
- FR-31: First-Run Wizard (add Account → Bridge discovery → per-Bridge login; every step skippable/re-enterable)
- FR-32: Start new Chats via Bridge resolve-identifier; clear not-found state
- FR-33: Persist all synced events (incl. decrypted E2EE content) in the Local Archive for every Account
- FR-34: Offline FTS across everything with sender/Chat/Network/date filters; < 200 ms first results at 100k+ events
- FR-35: Export Chat/Account/full archive to lossless JSON + Markdown, background with progress
- FR-36: Archive durability against remote rewrites (edit version chains; redactions mark, never erase; configurable)
- FR-37: Archive survives sign-out unless explicitly deleted; FTS/Export keep working
- FR-38: Persistent per-Chat Drafts across switches, restarts, crashes; visible draft markers
- FR-39: Cross-device Draft mirroring via per-Room account data; local-wins conflict handling
- FR-40: Approval Pane — all pending Drafts across Accounts; edit/approve/discard per Draft
- FR-41: Explicit-approval invariant — exactly two user-initiated dispatch triggers; no programmatic send path
- FR-42: Incognito read receipts (`m.read.private`) with global/per-Account/per-Chat scopes, deterministic precedence
- FR-43: Incognito typing/presence suppression
- FR-44: Per-Network coupled-behavior disclosure at toggle time (data-driven)
- FR-45: Manual read release (explicit public `m.read` on demand)
- FR-46: Undo-Send Window (default 10 s, 0–60 s) held locally pre-dispatch; cancel restores Draft; countdown affordance
- FR-47: Post-dispatch delete for everyone via Redaction with best-effort framing
- FR-48: Command Palette (⌘K) over Chats, contacts, actions; ≤ 100 ms per keystroke at 10k Chats; full parity gate
- FR-49: Keyboard navigation + Quick-Switcher; zero-pointer triage loop; in-app cheat sheet
- FR-50: Configurable global hotkey with conflict detection
- FR-51: Native macOS notifications from local sync loop; preview toggle; ≤ 5 s from receipt
- FR-52: Mute per Chat/Network, mention-only mode, global DND; muted Chats still accumulate unread
- FR-53: Background sync + notify with window closed; opt-in launch-at-login; honest quit semantics
- FR-54: Notification click-through to exact Chat/Account/message

*iOS phase (PRD §13):*

- FR-55: iOS app target — builds and runs via `tauri ios` from the existing workspace (keeper-core as staticlib, React in WKWebView); free Personal Team signing; stable bundle id shared with macOS; on-device walking-skeleton gate; CI iOS compile check as required PR gate
- FR-56: Desktop-only code compile-gated out of the iOS build (tray, global-shortcut, autostart, updater, window-state, desktop deep-link registration); iOS shell registers notification + mobile deep-link + IPC + media protocol only; updates arrive by reinstall/re-sign, no in-app updater path on iOS
- FR-57: Platform capability flags over the IPC handshake; unsupported surfaces (bbctl, global hotkey, updater controls, tray/launch-at-login) never render — no dead buttons; bridge management otherwise fully functional on iOS; flags data-driven per platform for later targets
- FR-58: Phone layout tier (< 768 px) — single-pane navigation stack Inbox → Room → Detail reusing existing components and selection state; desktop/tablet tiers unchanged ≥ 768 px; sidebar becomes a drawer; palette maps to pull-down search
- FR-59: Safe areas and keyboard avoidance — edge-to-edge rendering respecting iOS safe-area insets on every surface; composer never covered by the on-screen keyboard; no stranded offsets or launch/rotation flash
- FR-60: Touch idioms — long-press opens the same context menus as right-click; edge-swipe back; row swipe actions (archive/mute); pull-to-refresh kicks sync; every tappable ≥ 44 pt; rem-based text scaling degrades gracefully
- FR-61: Lifecycle-aware sync with honest disclosure — graceful pause on background, immediate sync on foreground; no background delivery claimed anywhere (no fake "push while closed" promise)
- FR-62: Foreground notifications + app icon badge = all-accounts unread aggregate updated per sync; visible-Chat suppression, previews-off, and mute/mention-only semantics identical to macOS
- FR-63: iOS keychain sessions — after-first-unlock, this-device-only accessibility through the existing platform seam; never synced off-device; survive relaunch and 7-day re-sign cycles
- FR-64: Media protocol on WKURLSchemeHandler — same `keeper-media://` URL format as macOS incl. Range (200/206/416) seeking; decrypted bytes never cross IPC JSON
- FR-65: Backup exclusion + file protection for local stores — DB directories excluded from iCloud/device backup; complete-until-first-user-authentication protection class; all account state under one data-directory root

*Screen Recording phase (PRD §14):*

- FR-66: Recording capability gating — `recording` capability flag over the IPC handshake, present only on desktop macOS ≥ 13.0; every recording surface (Settings section, tray affordances, palette actions) renders only when on; data-driven per platform; app-wide `minimumSystemVersion` stays 11.0; iOS never
- FR-67: Permission pre-flight with honest states — live-detect Screen Recording (plus Microphone/Camera when those sources are enabled), request via system prompt where allowed, deep-link to the exact System Settings pane otherwise; states granted / not-yet-requested / denied-with-fix-path detected at render; Start disabled naming the blocking permission; discloses relaunch-after-grant and the macOS 15+ monthly re-confirm; mic/camera requested only when enabled
- FR-68: Source selection — full display (with its audio) or a single running application; live picker of displays and apps with names/icons, re-enumerated as apps launch/quit; app-scoped capture excludes keeper, other apps, and notification banners; vanished-source picks fail clearly at Start; one capture target per Recording Session
- FR-69: Audio sources — system audio toggle (default on) + microphone picker (default system default input); each enabled source written as its own AAC track, never premixed; keeper's own notification sounds excluded; mic hot-unplug never aborts (video + system audio keep rolling, mic track silence-filled, fallback to default input, persistent warning)
- FR-70: Optional webcam as a separate synchronized file — camera picker (built-in/external/Continuity Camera; default off) recording `camera-####` files in the same session folder, time-anchored and rotated at the same segment boundaries; aligned within one frame; webcam off touches no Camera permission; camera loss mid-recording never aborts; no PiP burn-in this phase
- FR-71: Recording Session output — chosen folder (default `~/Movies/keeper`), one timestamped session folder per recording with segment files and a `manifest.json` (capture target, devices, segment list, status); folder validation (exists/writable/free space) before Start; atomic manifest updates; cleanly finalized segments are ordinary `.mp4` (H.264 + AAC)
- FR-72: Continuous segmented recording with size-based rotation — rotate at the configured segment size (default 500 MB) with a duration-cap fallback (default 30 min); rotation gapless; segment size user-configurable; N segments concatenate with no missing/duplicated frames and continuous timestamps (bar NFR-22)
- FR-73: Crash safety and startup recovery — crash-safe fragmented format losing at most the last fragment (~4 s); startup and pre-recording scan marks interrupted sessions recovered in their manifests and surfaces a once-per-session notice; recovered files play as-is, no remux
- FR-74: Tray/menu-bar recording state — idle/recording/warning-error with live elapsed time and current-segment info, one-click Stop Recording and Open Recordings Folder; recording forces the tray visible even when the FR-53 opt-in toggle is off, restoring prior state exactly at Stop; quit-while-recording warns then finalizes; macOS's own capture pill untouched
- FR-75: Loud failure surfacing — every fault (recorder crash/exit, writer stall, permission revocation, device loss, disk-guard) surfaces via tray error state + native notification within 5 s with one-click restart; non-fatal warnings persist until resolved/acknowledged; NFR-5's no-silent-loss extends so every session reaches finalized / recovered / failed
- FR-76: Local-only recording — recordings, manifests, and settings never leave the machine; zero new network destinations (NFR-11 egress diff empty for the phase); no upload/share/transcription/cloud affordance anywhere in the recording UI

### NonFunctional Requirements

- NFR-1: Cold start < 2 s to interactive Unified Inbox (cached render first)
- NFR-2: FTS first results < 200 ms at 100k+ events, offline
- NFR-3: Idle RSS ≤ 500 MB @ 5 Accounts / ≤ 300 MB @ 1 Account (assumption-tagged; measured, not yet gating)
- NFR-4: Chat switch < 150 ms; composer < 16 ms/frame; 60 fps inbox scroll at 10k Chats
- NFR-5: No silent message loss — terminal visible states for outgoing; every synced event lands in the Local Archive
- NFR-6: Bridge Session drop surfaced + notified ≤ 60 s
- NFR-7: Notification ≤ 5 s from local sync receipt
- NFR-8: Crash safety — WAL/atomic writes; recovery to consistent state, zero lost persisted events
- NFR-9: Rust-core confinement — no crypto, message DB, or tokens in JS
- NFR-10: At-rest passphrase encryption for SDK stores (first-run choice); archive.db at-rest is v1.x per AD-22 amendment
- NFR-11: Egress honesty — only user-configured endpoints + Beeper API + update endpoint; no telemetry; documented and diffable
- NFR-12: Signed + notarized macOS builds, signed auto-updates, reproducible CI
- NFR-13: Apache-2.0 licensing firewall (cargo-deny; no GPL/AGPL; provenance notes on ported code)
- NFR-14: Baseline accessibility — keyboard-only operable, labeled for VoiceOver, WCAG 2.1 AA contrast both themes

*iOS phase (PRD §13.3, measured on-device, release build, real accounts):*

- NFR-15: Cold start on device < 3 s to interactive Unified Inbox (cached Chats rendered, input accepted) — authored bar; owner confirmation required before it becomes release-gating (PRD §13.8)
- NFR-16: Memory hygiene under jetsam — droppable caches (image memory cache, media byte buffers) released on backgrounding and memory warnings; media Range-slicing buffer capped; 24 h suspended soak with a large account survives without a jetsam kill; memory returns near baseline (Instruments-verified)
- NFR-17: Flaky-network resilience — UI always renders instantly from the local mirror; SSS offline mode with backoff, exited immediately on demand; airplane-mode toggles and Wi-Fi↔cellular handovers recover unaided; stale resume shows cached UI, kicks sync, surfaces a subtle "connecting" state incl. the sync-loop restart guard (matrix-rust-sdk#3935)
- NFR-18: Resume integrity — resuming from background (incl. overnight suspension) never leaves a blank or unresponsive webview (tauri#14371); reload guard detects a jettisoned webview process and restores the UI; acceptance-tested from the walking skeleton onward

*Screen Recording phase (PRD §14.3, measured on Apple Silicon, release build, dev-signed per §14.7; authored bars owner-sign-off at phase release, mirroring the AD-22/NFR-3 posture):*

- NFR-19: Long-run capture stability — a 4 h continuous recording (1080p-class, 30 fps, system audio + microphone) completes with zero recorder crashes, writer stalls, or A/V desync and no unbounded memory growth; sample-buffer queues bounded with drop-oldest-video (audio never dropped); sustained dropping raises a warning (FR-75). [ASSUMPTION] 4 h bar authored, confirm before release-gating
- NFR-20: Disk-space guard — warns below a warning threshold and gracefully stops-and-finalizes below a hard floor; never runs the disk to exhaustion or dies mid-write. [ASSUMPTION] warn < 10 GB free, stop < 2 GB; authored pending confirmation
- NFR-21: Recording performance envelope — 1080p-class at 30 fps with both audio tracks adds < 100% of one core average CPU and < 400 MB combined RSS (sidecar + keeper overhead), and messaging bars NFR-1–NFR-4 still hold while recording. [ASSUMPTION] numbers authored, measure on reference hardware before gating
- NFR-22: Segment handover gaplessness — rotation cuts on keyframes with continuous host-clock-anchored timestamps; concatenating a session's segments yields monotonic timestamps with no gap/overlap exceeding one frame, and screen↔camera alignment holds within one frame across the session; an automated concatenate-and-assert test gates release

### Additional Requirements

From the Architecture Spine (AD-1–AD-25) — decisions that materially shape stories:

- AD-6 workspace split: `src-tauri/` becomes a cargo workspace with `crates/keeper-core` (tauri-free) + `crates/keeper` (Tauri shell). **Lands in Epic 1 Story 1.1** (the repo scaffold exists; this is the architecture-mandated restructure, the "starter template" step of this project).
- AD-7 ts-rs generated IPC types in `src/lib/ipc/gen/`, CI-diffed.
- AD-8 IPC conventions: `domain_verb` commands, `IpcError` envelope, snapshot-then-diff channels, `keeper://kebab-case` events.
- AD-9 zustand vanilla mirror stores, one per stream domain.
- AD-10 storage layout: `accounts/<ulid>/sdk/`, `keeper.db`, `archive.db`; secrets only in macOS Keychain; WAL everywhere; logout deletes SDK dir + Keychain only.
- AD-11 archiver task, version chains, mark-never-erase, single serialized archive writer.
- AD-12 FTS5 trigram tokenizer; 200 ms bar is a CI perf test.
- AD-13 outbox ahead of SendQueue; `send::submit(trigger ∈ {ComposerSend, ApprovalPaneApprove})` is the only dispatch path.
- AD-14 `signals` module is the sole outbound-signal emitter (receipts/typing/presence).
- AD-15 drafts: local truth, debounced mirror (`dev.keeper.draft` + `Room::save_composer_draft`), local-wins conflicts.
- AD-16 `BridgeTransport` trait (Provisioning + BotDriver); 3-source discovery; data-driven risk tiers in versioned JSON.
- AD-17 `AuthProvider` trait (password/oidc/beeper); Beeper HTTP isolated, failures contained.
- AD-18 local notification rules engine; click payload `(account_id, room_id, event_id)`.
- AD-19 per-account supervision (`AccountManager`/`AccountHandle`); no global mutable state.
- AD-20 inbox + palette index computed in Rust, windowed VM streams to UI.
- AD-21 `thiserror` per module → `CoreError` → `IpcError` mapped once; `tracing` only; no content/tokens in logs.
- AD-22 at-rest posture: SDK-store passphrase in MVP; archive.db plaintext (FileVault posture) honestly stated.
- AD-23 GitHub Actions macOS arm64 + tauri-action; signing, notarization, updater key, egress diff note.
- AD-24 `Platform` port keeps keeper-core platform-free.
- AD-25 settings live in `keeper.db` behind `keeper-core::settings`; no tauri-plugin-store/sql.
- Epic-gating tests (not amendments): OQ-1 walking-skeleton release-build spike = Epic 1 exit gate; OQ-3 hungryserv surface verification against a real Beeper Account = Epic 2 exit check (degrade per-feature with disclosure).
- Identity/DTO/date conventions per the spine's Consistency Conventions table (ULID account ids, `Vm` suffix, camelCase serde, ms-epoch timestamps).

From the Architecture Spine iOS increment (AD-26–AD-32; AD-24 Plan A confirmed — Tauri mobile reusing keeper-core and the same IPC contract; Plan B shelved with recorded revisit triggers):

- AD-26 one shell crate: iOS is the **same** `crates/keeper` crate built as a staticlib via `tauri ios` (`tauri::mobile_entry_point`) — no `keeper-ios` crate ever; desktop-only surface (tray module + `tray-icon` feature, global-shortcut, autostart, updater, window-state, desktop deep-link registration) behind `#[cfg(desktop)]`/target-gated Cargo deps; iOS registers notification + mobile deep-link + IPC + media protocol only; clipboard via web Clipboard API, opener replaced by a minimal native open call; `keeper-core` stays platform-free (variance only through the `Platform` port).
- AD-27 a single `CapabilitiesVm` in `keeper-core::vm` (serde + ts-rs), served over the IPC handshake at startup, data-driven per platform; off capability ⇒ surface does not render at all; `Platform::sidecar_path` returns a clean Unsupported `IpcError` on iOS; the frontend never consults `navigator.userAgent`/build flags.
- AD-28 `keeper-media://` runs unchanged on iOS (wry → WKURLSchemeHandler, identical URL format, no frontend media-URL helper); in-memory Range slicing **capped** (NFR-16); scheme-task invalidation tolerated by the fire-and-forget responder; disk-backed streaming is deferred work.
- AD-29 secrets through the **existing** keyring/apple-native `Platform` port targeting the iOS keychain — spiked in the walking skeleton, contained fallback = direct security-framework generic-password calls behind the same port; `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`; DB dirs `NSFileProtectionCompleteUntilFirstUserAuthentication` (never `Complete`) + `isExcludedFromBackup`; all account state under the one `Platform::data_dir()` root (future App Group move = path change, not migration).
- AD-30 foreground-only sync, honestly disclosed: graceful `SyncService` pause on background, resume + immediate sync on foreground rendering the cached mirror instantly; detection enters Rust through **one** lifecycle command (`visibilitychange` first; micro Swift plugin only as the correctness upgrade — same Rust entry either way); blank-webview reload guard (tauri#14371) mandatory from the walking skeleton onward; iOS notifications are foreground-local + badge-on-sync only, reusing AD-18; badge = the Unified Inbox unread aggregate from `inbox` (AD-20), never a second count.
- AD-31 phone tier = third `phone` tier (< 768 px) in the existing `useShellLayout`; stack container is a **projection of existing zustand selection state** reusing InboxList/ChatView/DetailPanel — no routing library this phase (`history.pushState` optional enhancer); safe areas via `viewport-fit=cover` + `contentInsetAdjustmentBehavior = .never` + `env(safe-area-inset-*)` as theme CSS vars; keyboard via a `visualViewport`-driven `--kb-inset` var (evaluate `interactive-widget=resizes-content`).
- AD-32 `gen/apple` generated under `crates/keeper` and committed (`build/` gitignored); persistent edits **only** in `project.yml`, `Info.plist` (`keeper://` CFBundleURLTypes), and `*_iOS/` sources; minimum iOS 16.0 set explicitly; Personal Team via `bundle.iOS.developmentTeam` or `TAURI_APPLE_DEVELOPMENT_TEAM` env (team id out of git); bundle id stable and shared with macOS; CI adds `cargo check --target aarch64-apple-ios` as a required PR gate (compile-only, existing macOS runner).
- Phase gates: **SM-7** walking skeleton (on-device OIDC deep-link login, room list, E2EE text send/receive, relaunch-restore + keyring spike + resume reload guard exercised) must pass before phone-UX epics; **SM-8** phone daily-driver ≥ 2 consecutive weeks. NFR-15's 3 s bar is not release-gating until the owner confirms it (PRD §13.8).
- Distribution posture: free Personal Team signing (7-day profiles re-armed from the owner's Mac, ~3 devices, blocked entitlements); test IPAs shared via per-tester re-signing (Sideloadly/zsign); AltServer auto-refresh optional; the paid Apple Developer Program is an explicit deferred decision gate (PRD §13.5), never an omission.

From the Architecture Spine Screen Recording increment (AD-33–AD-39; extends the frozen AD-1..32, no PRD amendment; route/format/floor locked by the recording research, adopted not relitigated):

- AD-33 recording split: `keeper-core::recording` owns the session state machine (`idle → preflight → recording → rotating → stopping → finalized | recovered | failed`), manifest schema, segment ledger, folder validation, and recovery reconciliation — platform-free, no `tauri`, no Apple API; the sidecar spawn and stdio framing live in the `keeper` shell behind a `Recorder` Platform-style port (a trait beside `Platform`, AD-24); the macOS impl spawns `keeper-rec` via `Platform::sidecar_path`, every non-macOS impl and iOS returns `CoreError::Unsupported`; core never holds a process handle.
- AD-34 `keeper-rec` sidecar & NDJSON-RPC stdio contract: a SwiftPM binary (ScreenCaptureKit + AVAssetWriter) spawned launch-on-demand via `Platform::sidecar_path` + Tauri `externalBin` (the bbctl precedent, AD-16 / Story 6.7); one JSON object per line; commands `getCapabilities` (version/feature-flags/per-TCC-permission handshake), `listSources`, `start{filter, audio, mic, camera, dir, segmentMB, fps}`, `stop`; events `state{recording, elapsedSec, segmentIndex, bytes, warning}`, `segmentClosed{path, bytes, track}`, `error{code, message, fatal}`; the contract shape is the invariant, exact field lists code-owned via AD-7.
- AD-35 recording capability gating: `CapabilitiesVm` (AD-27) gains a `recording` flag (serde + ts-rs, AD-7), true only on desktop macOS ≥ 13.0 (the system-audio floor); the macOS 15+ in-stream-mic branch lives inside `keeper-rec`, invisible to the flag; every recording surface renders only when on (AD-27 "no dead buttons"); the frontend never sniffs `navigator.userAgent`/build flags.
- AD-36 recording permissions/TCC: a pre-flight through the `Recorder` port → `keeper-rec` `getCapabilities` probe, surfaced as `RecordingPermissionVm` (`keeper-core::vm`, ts-rs) tracking three TCC classes distinctly — Screen Recording (`CGPreflightScreenCaptureAccess`/`CGRequestScreenCaptureAccess`, one real prompt per app lifetime, Settings deep link), Microphone (`NSMicrophoneUsageDescription`), Camera (`NSCameraUsageDescription`); mic/camera probed only when enabled; usage strings in keeper's bundle `Info.plist` (Tauri `bundle.macOS.infoPlist` merge); the sidecar is spawned, never a LaunchAgent (TCC attributes the child to keeper); quirks disclosed honestly; revocation mid-recording is a loud failure (AD-39).
- AD-37 recording format, segmentation ownership & recovery: fragmented MP4 (`.mpeg4CMAFCompliant`, ~4 s fragments), H.264 + up to two unmixed AAC tracks (48 kHz), 30 fps default at source resolution (60 selectable), clean finalize defragments to ordinary `.mp4`; dual-AVAssetWriter gapless size-based rotation lives entirely in `keeper-rec` (start writer B at the next keyframe PTS, dual-route until B's first keyframe, finalize A async; bytes-budget deadline corrected against on-disk growth; duration-cap fallback); `keeper-core` owns only the segment ledger + manifest (`<folder>/keeper-rec <local ts>/`, atomic-rename on every `segmentClosed`/status change; `screen-####.mp4` and, webcam on, `camera-####.mp4` from a second in-sidecar writer, host-clock anchored, no PiP); a startup recovery pass (and one before each new recording) marks stale `recording` manifests `recovered` and plays the orphaned tail as-is.
- AD-38 `keeper-rec` source layout, build, codesign & CI: a top-level SwiftPM package `tools/keeper-rec/` deliberately outside `src-tauri/crates/` (no Cargo/SwiftPM collision), first-party Apache-2.0 linking only Apple system frameworks (cargo-deny AD-5 untouched, no ffmpeg); `bundle.externalBin` = `binaries/keeper-rec`, Tauri appends the triple so the runtime name is `keeper-rec-aarch64-apple-darwin`; CI on the existing macOS signing runner does `swift build -c release --arch arm64` → explicit codesign (hardened runtime + keeper's entitlements) before `tauri build` (externalBin notarization rough edge, tauri#11992), aarch64-only, no lipo; dev-signing requirement (not a product blocker): local builds exercising recording need an Apple Development certificate (macOS 15+ ad-hoc SCK rejection, Cap #1722).
- AD-39 tray recording state & honest quit (extends Story 10.3 / AD-18): the opt-in tray (`crates/keeper/src/tray.rs`, single mutex-guarded `TrayIcon` slot) gains states `idle → recording → warning/error` via `TrayIcon::set_icon` (record-dot + warning-badge assets); a ~1 Hz tick updates a disabled menu line and the menu adds Stop Recording + Open Recordings Folder; recording temporarily forces tray presence and restores the exact prior state at stop; quit-while-recording = warn → `stop` RPC → flush → kill-timeout guard (never orphans `keeper-rec`); every fault is loud via the AD-18 notification pipeline within 5 s with one-click restart; NFR-5's no-silent-loss extends so every session reaches `finalized | recovered | failed`. Reliability envelope: buffer-bounding/drop policy/rotation correctness live in `keeper-rec`; the disk-guard policy (pre-start validation, warn threshold, hard-floor stop-and-finalize) lives in `keeper-core::recording` driven by free-space on `state` events; the CI perf/concat harness is the gate (extends AD-21 measurement hooks).

### UX Design Requirements

From DESIGN.md + EXPERIENCE.md (behavioral + brand-layer deltas; each must be covered by a story):

- UX-DR1: Brand theme tokens in `src/index.css` — keeper green / held amber / incognito violet / bridge-health trio / search highlight, light + dark, macOS system type stack, radii scale; everything else inherits shadcn.
- UX-DR2: Three-pane frame [sidebar 260 | chat list 320 | conversation ≥ 480 | detail 320 toggleable]; overlay titlebar with traffic-light insets; min window 940×600; sidebar rail collapse < 1080 px; detail panel → Sheet < 1280 px; chat list resizable ±25 % with persistence.
- UX-DR3: Chat row (64 px): avatar + 16 px network badge overlay, account hue 3 px edge bar, unread = weight 600 only, right-aligned unread badge / draft marker / mute glyph / health dot.
- UX-DR4: Pins strip (circular 44 px, drag reorder, overflow scroll) and FAVORITES labeled section between Pins and inbox.
- UX-DR5: Message bubbles (outgoing primary / incoming muted, 14 px radius, grouped same-sender), per-message state captions (Held/Sending…/Sent/Queued/Failed — Retry), reaction pills, "Edited" caption.
- UX-DR6: Undo-send pill — floating amber pill, radial countdown + "Sending in Ns — Undo"; reduced-motion numeric fallback; stacks oldest-first.
- UX-DR7: Incognito chip (violet outline) showing *effective* scope; violet composer focus ring while incognito applies.
- UX-DR8: Bridge card (health dot with pulse-twice-then-steady, state word, tier badge, unhealthy 3 px red edge); QR login panel on a white card in both themes; risk-tier badges per tier table.
- UX-DR9: Command Palette 640 px, two modes (fuzzy chats/contacts + `>` actions), kbd chips, context-aware ranking, ⌘Enter peek.
- UX-DR10: Voice & tone rules — sentence case, no exclamation marks, honest state narration, Glossary-capitalized nouns; state copy per the EXPERIENCE State Patterns table.
- UX-DR11: Persistent (never toast-only) treatment for loss-risk states: failed sends, bridge unhealthy, export failure, device unverified (dismiss-to-badge).
- UX-DR12: Accessibility floor — VoiceOver labels with dynamic state, aria-live regions (polite results, assertive bridge health), roving tabindex, focus return on overlay close, universal Esc chain, reduced-motion variants, pane landmarks.
- UX-DR13: Empty states for Inbox, filters, Archive, Favorites (hidden until first), Approval Pane, palette no-matches, search no-results, bridge discovery empty.
- UX-DR14: Keyboard primitive set — ⌘1–4 views, ⌘K/⌘⇧F/⌘F/⌘,/⌘I/⌘N, ⌃Tab chat cycling, ⌥⌘↓/↑ unread walk, j/k lists, single-key list verbs (e/u/p/f/m), composer Enter/⇧Enter/↑-edit, ⌘⇧Z undo-send, ⌘⇧I incognito, Esc walk-up chain.
- UX-DR15: Cheat sheet (⌘?) generated from the same action registry as the palette; native macOS menu bar mirrors every command.
- UX-DR16: Wizard stepper — Welcome → Add Account (3 tabs + honest no-homeserver fork) → Bridge discovery → per-Bridge login → Done; progress dots; every step "Skip for now"; Esc asks once.
- UX-DR17: Trust surfaces — permanent "Unofficial API" subtitle on Beeper tab, coverage card pre-completion, best-effort delete framing naming the Network, archive-divergence disclosure in Settings, rendered egress list in Settings → About, "Nothing sends without you" copy.
- UX-DR18: Sidebar structure — primary views with badges (Approval amber count, Bridges health roll-up), SPACES group, NETWORKS filter chips with health dots, account switcher footer with hue dots + sync glyphs + global offline pill.
- UX-DR19: Detail panel (⌘I) — chat info, members, shared media, per-chat controls (mute/mention-only, incognito override, archive, export, open raw Bridge Bot chat).
- UX-DR20: Draft-conflict chip above composer ("Edited on another device — Use that version"); sign-out AlertDialog with keep-default and typed-account-name destructive path.

Phone-tier increment (EXPERIENCE.md `Responsive & Platform` + DESIGN.md phone tokens; the phone tier is a projection of the desktop spine, not a second product):

- UX-DR21: Phone-tier tokens — `phone-breakpoint` 768 px, `touch-target-min` 44 pt, `safe-area` CSS vars (`--safe-*`, viewport-fit=cover), `--kb-inset` keyboard var, `phone-header` 52 px (back chevron + previous-level title, flat 1 px-border pane language), `swipe-action` surfaces (archive = primary, read-toggle = secondary, mute = muted, discard = destructive; label past half-swipe threshold). Same tokens, same components, same density — no restyling.
- UX-DR22: Navigation-stack behavior — three full-screen levels (Inbox → Room → Detail); push slides ~250 ms ease-out with under-level shift/dim, pop reverses, reduced-motion cuts; back affordance priority: header chevron → edge-swipe back (tracks finger, commits past 50 %/flick) → optional history integration; back always returns to Inbox preserving scroll position; opening a Chat does not auto-focus the composer; deep links set selection state and render at the right level.
- UX-DR23: Leading drawer = the entire desktop sidebar in a Sheet (views/SPACES/NETWORKS/account switcher/settings gear/sync status), opened by the header avatar button (worst-state bridge-health dot overlay) or edge-swipe at level 0 only; Inbox-header status cluster: amber Approval chip (pending count > 0, deep-links to Approval Pane) + magnifier + compose; **no bottom tab bar** (decision on the record); quiet header when healthy.
- UX-DR24: Merged full-screen Search surface replaces ⌘K + ⌘⇧F on phone — segmented scopes Chats / Messages (FTS with filter chips, deep-link to match) / Actions (full registry, context-aware), `>` prefix jumps to Actions; entered via header magnifier or pull-down on the Inbox list, with pull-to-refresh past the reveal threshold as one continuous axis; in-chat search via Room overflow → "Search in chat" (Messages pre-filtered); palette parity remains the release gate on phone.
- UX-DR25: Phone composer — bottom-anchored above `calc(var(--kb-inset) + env(safe-area-inset-bottom))`; **send is a ≥ 44 pt button** (tap = FR-41 approval trigger #1), on-screen return key inserts a newline, hardware keyboard follows the desktop setting; autogrow to 5 lines then scroll; attach via + → system photo library/camera/Files; undo-send pill tap replaces ⌘⇧Z; bottom-pinned timeline stays pinned across keyboard open/dismiss.
- UX-DR26: Touch idiom mapping table is normative — long-press = right-click everywhere (identical ContextMenus; bubble menu: React row, Reply, Edit, Delete ▸, Copy, Jump-to-original); row swipes: trailing → Archive + More (mute ▸), leading → read/unread; full-swipe commits first action; long-press-drag reorders Pins; Approval Pane touch: row tap → inline editor, explicit per-row Approve button ≥ 44 pt, trailing swipe → Discard with 5 s undo toast, still no approve-all; system callout/tap-highlight suppressed where custom menus exist; cheat sheet hidden on phone.
- UX-DR27: Capability honesty surfaces — absent capabilities removed then disclosed once: Settings → About "On this iPhone" rendered list (foreground-only sync, no bbctl, no global hotkey, updates by reinstall/7-day signature, link to docs/ios.md); lifecycle honesty card on iOS first run + permanent Settings → Notifications copy ("…nothing here pretends to be push"); Archive & Storage line: phone Local Archive excluded from backup, the Mac remains the durable exportable copy.
- UX-DR28: iOS accessibility + phone states — VoiceOver focus moves to the new level's header on push and returns on pop; escape gesture = back at every level; **no gesture is the sole path** (row swipes duplicated as VoiceOver custom actions + context menu; pull-to-refresh duplicated as "Sync now"); rem-based scaling holds at ~130 % text size; phone-tier state table honored (stale-resume "Connecting…" pill, reload-guard restore, queued-send caption "Queued — sends when keeper is open and back online", notification-permission-denied persistent state with Open Settings link, offline pull-to-refresh resolves to the offline pill, never an error toast).

Screen Recording increment (EXPERIENCE.md `Screen Recording (macOS — Phase 3)` + DESIGN.md recording-red token and recording components; every treatment renders only behind the `recording` capability flag, FR-66):

- UX-DR29: Recording view (`⌘5`, sidebar entry only when the flag is on, carrying a `recording-dot` while capture is live) — a single non-chat utility surface (no timeline, no composer, no chat list) flipping in place between *pre-record setup* (a stack of shadcn `Card` sections — Source, Audio, Webcam, Destination, Segmenting, collapsed Advanced-fps), *active recording*, and *completion/recovery*; centered `content-max-width` single column, not a pane frame; Start gated on the permission pre-flight; it lives beside Bridges and Settings, not in the inbox.
- UX-DR30: Recording-red token — the live-capture color (`recording` / `recording-dark`, `#E5322D`), used ONLY as the `recording-dot`, the active-recording banner edge, the tray record badge, and the loud error banner; deliberately warmer/brighter than `destructive`/`bridge-disconnected` so a live indicator never reads as a delete button, and the two never share a surface; never on buttons, text, hovers, or decoration.
- UX-DR31: Active-recording banner + segment meter — `active-recording-banner` pinned to the top of the Recording view, persistent while capture is live or faulted (never toast-only): record dot + `mono` "Recording — 12:34 · segment 3 · 412 MB" + Stop (destructive-outline); the `segment-meter` fills toward the segment size and resets at each gapless rotation; warning variant (mic unplug, low disk) and error variant (recorder exit, writer stall, permission revoked, disk floor) are the loud-failure surface with "Restart recording"; the in-app twin of the tray; Pause absent this phase.
- UX-DR32: Tray recording states — `tray-recording` icons idle / recording (record-dot badge) / warning-error (amber outline / recording-red filled badge); a `mono` elapsed·segment·size disabled menu line ticking ~1 Hz (live < 1 s of Start), Stop Recording, and Open Recordings Folder above Show keeper / Quit; recording forces the tray visible even when the FR-53 opt-in toggle is off and restores the prior state exactly at Stop; macOS's own purple screen-recording pill left untouched (the tray adds what the pill lacks — elapsed, segment, Stop, errors).
- UX-DR33: Permission pre-flight rows — one `permission-row` per required permission (Screen Recording always; Microphone / Camera only when those sources are enabled), each live-detected at render (never cached optimistically), re-detected on focus/return; request via system prompt where allowed, deep-link to the exact System Settings pane otherwise; Start disabled until every required grant is green, naming the blocking permission; honest `note-line`s: relaunch-may-be-needed, macOS 15+ monthly re-confirm, and the subtle dev-facing "ad-hoc dev builds may be blocked on macOS 15+ — sign with an Apple Development certificate".
- UX-DR34: Completion / recovery card + recording voice — on Stop, a `Card`: "Saved N segments · {size}" + session-folder path (`mono`) + primary **Reveal in Finder**, no preview/trim/share; the recovery notice ("A recording was interrupted; N segments were saved") uses the same card shape with a `bridge-degraded`-tinted edge, surfaced once per interrupted session and linking the folder; recording voice per the State Patterns table — "Recorded locally. Nothing uploads.", app-scoped disclosure ("only {App}'s windows and audio — keeper, other apps, and notification banners are excluded"), sentence case, no exclamation marks, Glossary-capitalized "Recording Session"/"segment".

### FR Coverage Map

| FR | Epic | Notes |
|---|---|---|
| FR-1, FR-5 | Epic 1 | Password login + SSS gate |
| FR-8, FR-9 | Epic 1 | Sync, text send/receive, offline queue |
| FR-2, FR-3, FR-4, FR-7 | Epic 2 | OIDC, Beeper, multi-account, disclosure |
| FR-6 | Epic 2 + Epic 5 | Management UI in E2; keep/delete-archive semantics complete in Story 5.7 |
| FR-10–FR-16 | Epic 3 | Rich messages, E2EE, media, redaction, receipts |
| FR-17 | Epic 3 + Epic 5 | Homeserver pagination in E3; archive-first in Story 5.6 |
| FR-18 | Epic 2 (merge) + Epic 4 (surface) | Rust-side multi-account merge lands with FR-4; inbox organization completes it |
| FR-19–FR-24 | Epic 4 | Unread, archive view, favorites, pins, spaces, attribution |
| FR-33–FR-37 | Epic 5 | Local Archive, FTS, export, durability, sign-out survival |
| FR-25–FR-32 | Epic 6 | Bridges, wizard, start-chat, bbctl |
| FR-38–FR-41 | Epic 7 | Drafts + Approval Pane + invariant |
| FR-42–FR-47 | Epic 8 | Incognito + undo-send + post-dispatch delete |
| FR-48–FR-50 | Epic 9 | Palette, keyboard, global hotkey |
| FR-51–FR-54 | Epic 10 | Notifications + background |
| FR-28 | Epic 6 (detection/UI) + Epic 10 (native notification leg) | Split is deliberate: pipeline exists only in E10 |
| FR-44 | Epic 8 (UI) with data file from Story 6.1 | Same data structure as risk tiers |
| NFR-10 | Epic 2 (Story 2.6) | SDK-store passphrase choice per AD-22 |
| NFR-11–NFR-13 | Epic 11 | Egress list, packaging, licensing gates |
| NFR-1–NFR-4, NFR-8 | Epic 11 (gates) + designed-in throughout | CI perf harness makes them release gates |
| FR-55, FR-56 | Epic 12 | Init + compile seam; CI check in 12.5; on-device gate = Story 12.6 (SM-7) |
| FR-57 | Epic 12 (CapabilitiesVm + handshake) + Epic 13 (surface hiding, Story 13.7) | Split is deliberate: mechanism before UI |
| FR-58, FR-59, FR-60 | Epic 13 | Stack navigation, safe areas/keyboard, touch idioms |
| FR-61 | Epic 14 | Mechanics in 14.1, honesty copy in 14.2 |
| FR-62 | Epic 14 | Foreground notifications + all-accounts badge |
| FR-63 | Epic 12 | Keychain spike-first per AD-29 (Story 12.3) |
| FR-64 | Epic 12 | WKURLSchemeHandler media with capped buffers (Story 12.4) |
| FR-65 | Epic 14 | Backup exclusion + file protection (Story 14.7) |
| NFR-15 | Epic 15 | Measured on-device in 15.6; authored bar pending owner confirmation |
| NFR-16, NFR-17, NFR-18 | Epic 14 | Memory hygiene, network resilience, resume integrity |
| FR-66 | Epic 16 | CapabilitiesVm `recording` flag + gated surface (Story 16.3) |
| FR-67 | Epic 16 (Screen leg, 16.5) + Epic 20 (Mic/Camera rows, 20.2) | Pre-flight mechanism before source-enabled probes |
| FR-68 | Epic 16 (full-screen leg, 16.6) + Epic 19 (app picker, 19.1) | Full-screen first, app-scoped after the core is trustworthy |
| FR-69 | Epic 16 (system-audio leg, 16.6) + Epic 19 (toggle 19.2 / mic 19.3 / hot-unplug 19.4) | Split is deliberate: capture core before device breadth |
| FR-70 | Epic 20 | Webcam separate synchronized file (Story 20.1) |
| FR-71 | Epic 16 (single-file leg, 16.6) + Epic 17 (session folder/manifest, 17.2) + Epic 19 (folder chooser, 19.5) | Manifest/ledger in E17; destination UI in E19 |
| FR-72 | Epic 17 | Segmented rotation + size/duration settings (17.1, 17.5); fps in Epic 19 (19.5) |
| FR-73 | Epic 17 (recovery, 17.3) + Epic 20 (recovery notice UI, 20.3) | Recovery pass in core; once-per-session notice in the view |
| FR-74 | Epic 18 | Tray state/elapsed/Stop/Open Folder (18.1); forced-visibility + honest quit (18.2) |
| FR-75 | Epic 18 | Loud-failure triad (18.4); banner variants (18.3); disk-guard leg (18.5) |
| FR-76 | Epic 20 | Zero-egress audit + local-only invariant (Story 20.4) |
| NFR-19 | Epic 20 | 4 h soak measured on reference hardware (Story 20.5); authored bar pending owner confirmation |
| NFR-20 | Epic 18 | Disk-space guard policy in `keeper-core::recording` (Story 18.5) |
| NFR-21 | Epic 20 | CPU/memory envelope measured (Story 20.5); authored numbers pending owner confirmation |
| NFR-22 | Epic 17 | Gapless dual-writer rotation (17.1) + automated concat-assert CI gate (17.4) |

## Epic List

### Epic 1: Walking Skeleton — Sign In and Chat on Matrix
A user can add one password-login Account on a standard SSS-capable homeserver, see their room list, open a timeline, and send/receive text — in a release build, on the architecture's final crate layout. This is the PRD OQ-1 exit gate.
**FRs covered:** FR-1, FR-5, FR-8, FR-9 (+ AD-6/7/8 foundation, UX-DR1/2 shell)

### Epic 2: Every Account, One Inbox — Multi-Account, OIDC & Beeper
A user can run unlimited concurrent Accounts across password, OIDC, and Beeper email-code logins, merged into one inbox, with honest Beeper coverage disclosure. Exit check: OQ-3 hungryserv surface verification.
**FRs covered:** FR-2, FR-3, FR-4, FR-6 (management), FR-7, FR-18 (merge); NFR-10

### Epic 3: Trusted, Full-Fidelity Conversations — E2EE & Rich Messages
A user can verify devices, restore key backup, and use replies, edits, reactions, media/files, redaction, receipts/typing, and history pagination — including in encrypted and bridged Chats.
**FRs covered:** FR-10, FR-11, FR-12, FR-13, FR-14, FR-15, FR-16, FR-17 (homeserver leg)

### Epic 4: Unified Inbox Organization
A user can triage at Beeper grade: unread management, Archive view with auto-return, Favorites, Pins, Space filtering, and unambiguous Network/Account attribution.
**FRs covered:** FR-19, FR-20, FR-21, FR-22, FR-23, FR-24 (completes FR-18 surface)

### Epic 5: Local Archive, Search & Export — History That Survives
Every synced event persists on disk, searchable offline in < 200 ms, exportable to JSON/Markdown, durable against remote rewrites and sign-out.
**FRs covered:** FR-33, FR-34, FR-35, FR-36, FR-37; completes FR-6, FR-11 (edit history), FR-17 (archive-first)

### Epic 6: Bridge Management & First-Run Wizard
A user can discover Bridges, log them in natively (provisioning API or Bridge Bot fallback), see honest risk tiers, catch dead sessions inside 60 s, run bbctl Bridges, start new Chats, and get all of it guided on first run.
**FRs covered:** FR-25, FR-26, FR-27, FR-28 (detection + in-app surfacing), FR-29, FR-30, FR-31, FR-32

### Epic 7: Drafts & Approval Pane — The Airlock
Composer text persists as Drafts everywhere, mirrors across devices, and the Approval Pane lists every pending Draft — nothing sends without explicit approval, ever.
**FRs covered:** FR-38, FR-39, FR-40, FR-41

### Epic 8: Incognito & Undo-Send — Privacy on the User's Terms
Read receipts go private, typing/presence stay suppressed, receipts release only on demand, and every approved send can be pulled back inside the window; post-dispatch deletion falls back to honest Redaction.
**FRs covered:** FR-42, FR-43, FR-44, FR-45, FR-46, FR-47

### Epic 9: Command Palette, Hotkeys & Keyboard Mastery
Every Chat and action is one ⌘K away; the whole triage loop runs pointer-free; a global hotkey summons keeper from anywhere.
**FRs covered:** FR-48, FR-49, FR-50 (+ NFR-14 keyboard superset)

### Epic 10: Notifications & Background Operation
Reliable native notifications from the local sync loop, with mutes/mention-only/DND, background sync, and click-through into the exact Chat — bridge-health alerts included.
**FRs covered:** FR-51, FR-52, FR-53, FR-54; completes FR-28 (notification leg)

### Epic 11: Packaging, Release & Quality Gates
Signed, notarized, auto-updating builds from reproducible CI, with the licensing firewall, the rendered egress list, and the performance/reliability bars turned into release gates.
**FRs covered:** — (NFR-11, NFR-12, NFR-13; NFR-1–NFR-4/NFR-8 as CI gates)

### Epic 12: iOS Walking Skeleton — Build, Sign, Run
keeper compiles, signs, and runs on iPhone from the same workspace, UI-free: `gen/apple` committed, desktop-only code cfg-gated out with the CapabilitiesVm handshake, keychain spiked through the existing port, `keeper-media://` proven on WKURLSchemeHandler, a CI compile gate — ending at the SM-7 on-device gate before any phone-UX investment.
**FRs covered:** FR-55, FR-56, FR-57 (mechanism), FR-63, FR-64 (+ AD-26–AD-29, AD-32; SM-7 exit gate)

### Epic 13: iPhone Shell — Single-Pane Navigation
The desktop shell projects onto a phone tier: navigation stack Inbox → Room → Detail from existing selection state, leading drawer with the status cluster, merged full-screen Search, safe-area/keyboard-aware composer, full touch idioms, and capability-honest surfaces with the "On this iPhone" disclosure.
**FRs covered:** FR-58, FR-59, FR-60; FR-57 (surface leg); FR-48/FR-34 parity on phone (+ UX-DR21–UX-DR28)

### Epic 14: iOS Platform Behavior
The phone behaves honestly as an iOS citizen: foreground-only sync through one Rust lifecycle entry with plain disclosure, foreground notifications + all-accounts badge, resume integrity under webview jettison, memory hygiene under jetsam, flaky-network resilience, and backup exclusion + file protection for the local stores.
**FRs covered:** FR-61, FR-62, FR-65; NFR-16, NFR-17, NFR-18

### Epic 15: iOS Polish & Release
Ship the phase: icons and launch assets, the free-signing walkthrough in docs/ios.md, a shareable IPA path for re-signing, the iOS CI gate wired as required, the paid-program decision gate recorded, and the final on-device acceptance that opens SM-8 dogfooding.
**FRs covered:** — (FR-55 assets/docs/CI legs; NFR-15 measured; SM-8; PRD §13.5 decision record)

### Epic 16: Recording Walking Skeleton — Sidecar, Permissions, Capture to File
keeper spawns the first-party `keeper-rec` Swift sidecar, gates a `recording` capability on macOS ≥ 13.0, negotiates the NDJSON-RPC handshake, runs an honest Screen Recording permission pre-flight, and records a full display with system audio to a single fMP4 in a chosen folder — driven from a ⌘5 Recording view with Start/Stop and elapsed. This retires the TCC, signing, and capture-to-file risks. Exit: a real recording plays back (R.1 / SM-9 seed).
**FRs covered:** FR-66, FR-67 (Screen leg), FR-68 (full-screen leg), FR-69 (system-audio leg), FR-71 (single-file leg) (+ AD-33–AD-36, AD-38; SM-9 seed)

### Epic 17: Segmentation & Recovery — Hours-Long, Crash-Safe Capture
Continuous recording rotates gaplessly into size-bounded segments, each session gets a folder + atomic `manifest.json` + a segment ledger, interrupted sessions recover on startup, and an automated concatenate-and-assert test proves the handover is gapless.
**FRs covered:** FR-72, FR-71 (session folder/manifest leg), FR-73 (recovery leg); NFR-22 (+ AD-37)

### Epic 18: Tray & Loud Failures — The Menu Bar Tells the Truth
The tray carries recording / warning / error states with a live elapsed·segment·size line, Stop, and Open Recordings Folder; recording forces the tray visible and restores it; quitting while recording finalizes cleanly; and every fault surfaces loudly across tray, notification, and banner — including the disk-space guard.
**FRs covered:** FR-74, FR-75; NFR-20 (+ AD-39, AD-18)

### Epic 19: Sources & Devices — Choose What and Whom to Capture
A live application/window picker (SCShareableContent) with app-scoped audio, a system-audio toggle, a microphone picker written as a separate track with hot-unplug resilience, a destination-folder chooser, and an advanced fps control.
**FRs covered:** FR-68 (app-picker leg), FR-69 (toggle / mic / hot-unplug legs), FR-71 (folder-chooser leg), FR-72 (fps leg) (+ AD-34, AD-36)

### Epic 20: Webcam & Polish — Ship the Phase
Optional webcam as a separate synchronized file, the Microphone/Camera pre-flight rows, palette actions + optional global hotkey + the capability-gating and zero-egress audits, docs/recording.md, the reliability envelope (4 h soak + CPU/memory), and the SM-9/SM-10 phase acceptance with retrospective inputs.
**FRs covered:** FR-70, FR-67 (Mic/Camera rows), FR-76; NFR-19, NFR-21 (+ SM-9, SM-10 acceptance; AD-37)

## Epic 1: Walking Skeleton — Sign In and Chat on Matrix

Prove the whole vertical slice on the final architecture: `keeper-core`/`keeper` crate split, typed IPC, password login gated on Simplified Sliding Sync, streaming room list, timeline, and text send/receive with visible states. Exit gate (PRD OQ-1): all of the above working in a `tauri build` release build against a real Synapse ≥ 1.114.

### Story 1.1: Cargo Workspace Split and Typed IPC Foundation

As a keeper developer,
I want the Rust backend restructured into `keeper-core` (tauri-free) and `keeper` (Tauri shell) crates with a generated TypeScript binding pipeline and shared IPC conventions,
So that every later story lands on the architecture's hexagonal seam instead of being refactored onto it.

**Requirements:** AD-6, AD-7, AD-8, AD-21, NFR-9, NFR-13
**Dependencies:** none

**Acceptance Criteria:**

**Given** the existing `src-tauri/` scaffold with `keeper_lib`
**When** the workspace restructure is complete
**Then** `src-tauri/Cargo.toml` is a workspace with members `crates/keeper-core` and `crates/keeper`, the app builds and launches via `bun run tauri dev`, and `keeper-core` has no `tauri` dependency anywhere in its tree (enforced by a `cargo tree` check or unit test)
**And** `keeper-core` exposes the `Platform` port trait and a `CoreError` root per AD-21/AD-24.

**Given** a sample view-model type in `keeper-core::vm` deriving `serde` + `ts_rs::TS` with `#[ts(export)]` and camelCase rename-all
**When** the cargo test export step runs
**Then** TypeScript bindings are emitted to `src/lib/ipc/gen/` and a CI-runnable check fails if committed bindings differ from generated ones (AD-7).

**Given** the IPC conventions (AD-8)
**When** a demo `app_ping` command and a demo snapshot-then-diff channel subscription are invoked from a thin typed wrapper in `src/lib/ipc/`
**Then** fallible commands return the `IpcError` envelope `{ code, message, accountId?, retriable }`, and the channel delivers a full snapshot batch before any diff batch
**And** `bun run check`, `bun run check:rust`, `bun run test:rust`, and `cargo deny check` all pass.

### Story 1.2: App Shell — Three-Pane Frame and keeper Theme

As a user,
I want keeper to open as a native-feeling macOS window with the three-pane layout and keeper's visual identity,
So that every later feature renders inside the final frame instead of a placeholder UI.

**Requirements:** UX-DR1, UX-DR2, UX-DR18 (skeleton), NFR-14 (focus visibility)
**Dependencies:** 1.1

**Acceptance Criteria:**

**Given** DESIGN.md's brand-layer tokens
**When** the app renders in light and dark mode
**Then** `src/index.css` defines keeper green primary, held amber accent, incognito violet, the bridge-health trio, and search-highlight tokens for both themes, the macOS system font stack, and the 5/7/10/14 px radii scale, with all unlisted tokens inheriting shadcn defaults (UX-DR1)
**And** light/dark follow the system by default.

**Given** the window frame
**When** keeper opens
**Then** the layout is [sidebar 260 px | chat list 320 px | conversation ≥ 480 px] with a toggleable 320 px detail-panel slot, an overlay titlebar whose sidebar header reserves the 78×12 px traffic-light inset, and 1 px borders between panes with no inter-pane shadows (UX-DR2)
**And** the minimum window size 940×600 is enforced and the sidebar auto-collapses to a 48 px icon rail below 1080 px width.

**Given** keyboard use
**When** focus moves through the shell
**Then** every focusable control shows the visible focus ring, and pane placeholders (sidebar view list, empty chat list, empty conversation) render without any Matrix data.

### Story 1.3: Password Login with Sliding-Sync Verification

As a user,
I want to add an Account with my homeserver address, username, and password — refused up front if the server can't do Simplified Sliding Sync,
So that I get a syncing Account or an actionable error, never a half-configured one.

**Requirements:** FR-1, FR-5; AD-2, AD-3, AD-10, AD-17 (password impl), AD-21
**Dependencies:** 1.1, 1.2

**Acceptance Criteria:**

**Given** a reachable SSS-capable homeserver (Synapse ≥ 1.114) with password login enabled
**When** the user submits valid homeserver + username + password on the login screen
**Then** an `AuthProvider::password` flow produces a logged-in `matrix_sdk::Client` with its store at `accounts/<ulid>/sdk/`, access/refresh tokens stored only in the macOS Keychain (service `dev.tgorka.keeper`), and an account registry row in `keeper.db`
**And** entering a bare domain resolves the homeserver via `/.well-known/matrix/client` when present (FR-1).

**Given** invalid input or an incapable server
**When** login fails
**Then** the inline error names the specific cause — bad credentials vs. unreachable server vs. unsupported login type — and a non-SSS server fails **before** any account state is created with an error naming Simplified Sliding Sync and linking docs (FR-5)
**And** the SSS check result is logged per Account via `tracing` with no tokens or credentials in the log.

**Given** the code review
**Then** no token, password, or crypto material crosses IPC or reaches TypeScript-accessible storage (NFR-9).

### Story 1.4: Sliding-Sync Room List

As a user,
I want my Chats to appear in the chat list, newest first, streaming live as messages arrive,
So that the app is a functioning messenger surface immediately after login.

**Requirements:** FR-8 (room-list leg); AD-2, AD-4, AD-8, AD-9, AD-19, AD-20 (seed); UX-DR3 (minimal row)
**Dependencies:** 1.3

**Acceptance Criteria:**

**Given** a logged-in Account
**When** the frontend subscribes to the room-list channel
**Then** `SyncService` + `RoomListService` run under the account's supervision task, and `keeper-core` streams a windowed `RoomListVm` (visible range + buffer, with totals) as a snapshot batch followed by diff batches into a zustand mirror store (AD-8/9/20)
**And** re-subscribing at any time yields a fresh snapshot without duplication.

**Given** the chat list UI
**When** rooms render
**Then** each 64 px row shows avatar, display name, last-message preview, and timestamp per the chat-row spec (unread badge and network overlays arrive in later epics), and rows are full-width click/Enter targets
**And** an incoming message on any room moves that room to the top within 2 s of sync delivery.

**Given** ordering logic
**Then** recency ordering is computed in Rust only — the TS store applies diffs and never re-sorts (AD-20).

### Story 1.5: Timeline View — Receive Text

As a user,
I want to open a Chat and read its message history as it updates live,
So that I can follow conversations in keeper.

**Requirements:** FR-8 (timeline leg), FR-9 (receive); AD-4, AD-8, AD-9; UX-DR5 (bubbles)
**Dependencies:** 1.4

**Acceptance Criteria:**

**Given** a selected Chat
**When** the conversation pane opens
**Then** a per-room timeline channel streams `TimelineItemVm` items (snapshot, then diffs) from the SDK `Timeline`, and text messages render as bubbles — incoming muted, outgoing primary, 14 px radius, consecutive same-sender messages grouped with a single avatar (UX-DR5)
**And** the timeline text column is capped at 720 px and centered in wider panes.

**Given** a Chat previously synced
**When** it is reopened in the same session
**Then** the cached timeline renders without waiting on a network round-trip, targeting the < 150 ms switch bar (NFR-4)
**And** closing a Chat tears down its subscription without leaking the account's other streams.

**Given** live activity
**When** a new remote message arrives in the open Chat
**Then** it appears via a diff batch without re-rendering the whole list.

### Story 1.6: Send Text with Local Echo and Visible Send States

As a user,
I want to type and send messages that appear instantly and honestly report their state,
So that I always know whether a message actually went out.

**Requirements:** FR-9; FR-41 (gate seed), AD-13 (submit gate), NFR-5; UX-DR5, UX-DR10
**Dependencies:** 1.5

**Acceptance Criteria:**

**Given** the composer in an open Chat
**When** the user presses Enter (⇧Enter inserts a newline)
**Then** the message dispatches through `send::submit(text, trigger = ComposerSend)` — established in this story as the **only** function that feeds the SDK `SendQueue` — and appears immediately as local echo with a "Sending…" caption that resolves to "Sent" (AD-13)
**And** the composer autogrows to 8 lines then scrolls.

**Given** a send that permanently fails
**When** the SendQueue reports failure
**Then** the message shows a persistent destructive "Failed — Retry" caption that never disappears on its own, and Retry re-enters the same submit gate (NFR-5, UX-DR11)
**And** state captions follow the microcopy table (sentence case, no error codes) (UX-DR10).

**Given** the audit for FR-41
**Then** a Rust test asserts `send::submit` is the sole public dispatch entry point in `keeper-core::send`.

### Story 1.7: Offline Resilience — Queued Sends and Reconnect Convergence

As a user,
I want messages composed offline to queue visibly and send themselves when I'm back,
So that flaky Wi-Fi never silently eats a message.

**Requirements:** FR-8, FR-9; NFR-5; UX-DR10, UX-DR18 (offline pill)
**Dependencies:** 1.6

**Acceptance Criteria:**

**Given** the machine is offline
**When** the user sends a message
**Then** it renders with the amber "Queued — sends when you're back online" caption and dispatches automatically on reconnect, resolving to "Sent" (FR-9)
**And** the sidebar footer shows a persistent "Offline — showing your local archive. Messages queue until you're back." pill while disconnected, with no toast spam on connection flapping.

**Given** a 24 h offline gap (simulated)
**When** the app reconnects
**Then** the room list converges to server state with no duplicate and no missing Chats (FR-8).

**Given** a force-quit while messages are queued
**When** the app relaunches
**Then** queued messages are still visible in their queued state and dispatch on connectivity (NFR-5, NFR-8).

### Story 1.8: Session Restore and Sign-Out

As a user,
I want keeper to restore my session instantly on relaunch and let me sign out cleanly,
So that the account lifecycle is complete end to end.

**Requirements:** FR-6 (single-account slice), FR-8; AD-10; NFR-1 (path), NFR-8
**Dependencies:** 1.3, 1.4, 1.5, 1.6

**Acceptance Criteria:**

**Given** a signed-in Account and a force-quit
**When** keeper relaunches
**Then** the session restores from the SDK store + Keychain without re-login, previously synced Chats render from local cache before network round-trips complete, and sync resumes via SSS (FR-8, NFR-1 path)
**And** all SQLite stores run in WAL mode so the force-quit loses no previously persisted state (NFR-8).

**Given** the account row in settings/sidebar footer
**When** the user chooses Sign out and confirms
**Then** keeper deletes exactly `accounts/<ulid>/sdk/` and that account's Keychain entries — nothing else — stops the account's supervision tasks, and returns to the login screen (AD-10)
**And** relaunching after sign-out lands on login with no residual session.

**Epic 1 exit gate (OQ-1):** all Story 1.1–1.8 ACs pass in a `tauri build` release build against a real SSS homeserver.

## Epic 2: Every Account, One Inbox — Multi-Account, OIDC & Beeper

Break the account cap: unlimited concurrent Accounts behind one `AuthProvider` interface (password, OIDC/MAS, Beeper email-code JWT), merged into a single inbox with per-account attribution groundwork, honest Beeper disclosures, and the at-rest encryption first-run choice. Exit check (OQ-3): verify the hungryserv surface against a real Beeper Account and record per-feature degradations.

### Story 2.1: Account Manager — Unlimited Concurrent Accounts

As a user,
I want to add a second (and Nth) Account and see all my Chats merged in one list,
So that I escape account caps for free.

**Requirements:** FR-4, FR-18 (merge); AD-3, AD-17 (trait extraction), AD-19, AD-20
**Dependencies:** Epic 1

**Acceptance Criteria:**

**Given** the Epic 1 single-account code
**When** this story completes
**Then** login flows run through an `AuthProvider` trait (password as first impl), and `AccountManager` owns a registry of `AccountHandle`s, each supervising its own Client, SyncService, and streams with per-account `tracing` spans (AD-17, AD-19).

**Given** ≥ 2 Accounts signed in (same or different homeservers)
**When** the chat list renders
**Then** it shows all Chats from all Accounts merged by recency — computed in `keeper-core::inbox` from N RoomList streams, streamed as one windowed VM — and send/receive works independently on each Account (FR-4)
**And** each Account is assigned a hue from the 8-hue wheel at add time, rendered as the 3 px row edge bar (UX-DR3).

**Given** the codebase
**Then** no code path enforces an account-count limit — adding a 6th Account behaves identically to a 2nd (FR-4), and an account's sign-out tears down only its own tasks and rows (AD-19, AD-10).

### Story 2.2: OIDC Login (MAS / MSC3861)

As a user,
I want to sign in to OIDC-native homeservers like matrix.org through my system browser,
So that modern Matrix auth works without manual token handling.

**Requirements:** FR-2, FR-5; AD-17 (oidc impl)
**Dependencies:** 2.1

**Acceptance Criteria:**

**Given** a MAS-enabled homeserver
**When** the user picks it on the login screen
**Then** keeper opens the system browser for the OIDC flow, completes login on the `keeper://oauth/callback` deep link, and yields a syncing Account with tokens only in the Keychain — no manual token handling (FR-2).

**Given** the user cancels or abandons the browser flow
**When** keeper regains focus
**Then** the login screen shows a quiet inline "Login cancelled" note, and no partial Account, store directory, or Keychain entry exists (FR-2).

**Given** a non-SSS OIDC server
**Then** the SSS gate from FR-5 applies identically before account creation.

### Story 2.3: Beeper Email-Code Login

As a Beeper user,
I want to add my Beeper Account with just my email and the emailed code,
So that my Beeper chats join keeper without a password or token dance.

**Requirements:** FR-3; AD-17 (beeper impl, containment §8); UX-DR17
**Dependencies:** 2.1

**Acceptance Criteria:**

**Given** the Add Account surface
**When** the Beeper tab renders
**Then** it is permanently subtitled "Unofficial API — may break without notice" as part of the form, not a dismissible hint (FR-3, UX-DR17).

**Given** a valid Beeper email and emailed code
**When** the user completes the flow
**Then** keeper runs `/user/login` → `/user/login/email` → `/user/login/response` → JWT → `org.matrix.login.jwt` against matrix.beeper.com and produces a syncing Beeper Account showing Matrix-native, cloud-Bridge, and bbctl-Bridge Chats (FR-3)
**And** all api.beeper.com HTTP lives in the `auth::beeper` module only (AD-17).

**Given** the Beeper API rejects, times out, or changes shape
**When** login fails
**Then** the UI shows the distinct "Beeper login unavailable — this is an unofficial API and may have changed." state with Retry and a status link — never a hang, spinner, or crash — and the failure is unobservable from non-Beeper Accounts (FR-3, AD-17).

**Given** a real Beeper Account (OQ-3 exit check)
**Then** the hungryserv surface is verified (`thirdparty/protocols`, custom account data, `m.read.private`, push rules) and gaps are recorded as per-feature degradation notes for later epics.

### Story 2.4: Beeper Coverage Disclosure

As a Beeper user,
I want keeper to tell me before login completes which of my chats will not appear,
So that missing On-Device chats read as honesty, not breakage.

**Requirements:** FR-7; UX-DR17
**Dependencies:** 2.3

**Acceptance Criteria:**

**Given** the Beeper login flow
**When** authentication succeeds but before completion
**Then** a disclosure card states plainly that On-Device Connection chats are invisible to keeper — naming the broken expectation ("WhatsApp connected in the official Beeper app will not appear here.") — and points to self-hosted Bridges as the parity path, requiring acknowledgment to continue (FR-7).

**Given** a connected Beeper Account
**When** the user opens that Account's settings
**Then** the same disclosure is permanently accessible there (FR-7)
**And** the copy follows the voice rules (sentence case, consequence-naming, no softening) (UX-DR10).

### Story 2.5: Account Switcher and Per-Account State

As a user,
I want to see every Account's state and manage each from the sidebar,
So that a multi-account setup stays legible and controllable.

**Requirements:** FR-6 (list/state/sign-out UI), FR-4; UX-DR18, UX-DR20 (dialog shell)
**Dependencies:** 2.1

**Acceptance Criteria:**

**Given** ≥ 2 connected Accounts
**When** the sidebar footer renders
**Then** the account switcher lists every Account with avatar, hue dot, homeserver, and sync-state glyph (syncing spinner / synced / offline gray), plus an "Add Account" entry that is always present and never count-gated (FR-4, FR-6).

**Given** an Account row
**When** the user clicks it
**Then** the inbox filters to that Account (click again to clear), and its DropdownMenu offers Settings and "Sign out…" opening an AlertDialog whose default is "Sign out, keep local archive" (destructive archive deletion is completed in Story 5.7 — until then the dialog performs sign-out per Story 1.8 semantics and labels archive retention as the default) (FR-6, UX-DR20).

**Given** sync state changes (offline, re-auth needed)
**Then** the glyph updates from the account status stream within one sync cycle, with no toast spam.

### Story 2.6: At-Rest Encryption First-Run Choice

As a security-conscious user,
I want to opt into passphrase encryption for my Matrix stores when I add my first Account,
So that my session and crypto state are protected at rest beyond FileVault.

**Requirements:** NFR-10 (as amended by AD-22); AD-10
**Dependencies:** 2.1

**Acceptance Criteria:**

**Given** the first Account add on a fresh install
**When** login succeeds
**Then** a first-run choice offers passphrase-based at-rest encryption for SDK stores (default off per the FileVault posture), and choosing it creates the store with matrix-sdk-sqlite's native passphrase, generated and kept only in the Keychain (NFR-10, AD-22).

**Given** the setting exists
**When** the user reads Settings → Archive & Storage
**Then** the copy states honestly that `archive.db`/`keeper.db` are not passphrase-encrypted in this version and rely on FileVault (AD-22, UX-DR17).

**Given** subsequent Account adds
**Then** the chosen posture applies consistently to new SDK stores without re-prompting.

## Epic 3: Trusted, Full-Fidelity Conversations — E2EE & Rich Messages

Bring messaging to table stakes: transparent E2EE with verification and key backup, replies, edits, reactions, media and files over the `keeper-media://` protocol, redaction with honest bridged framing, receipts/typing, and smooth history pagination.

### Story 3.1: Encrypted Rooms — Decrypt, Encrypt, and Honest UTD States

As a user,
I want encrypted Chats to just work, and undecryptable messages to say so plainly,
So that E2EE is transparent when healthy and honest when not.

**Requirements:** FR-14 (encryption leg); AD-1, NFR-9; UX-DR10
**Dependencies:** Epic 2 (works with ≥ 1 account from Epic 1 onward)

**Acceptance Criteria:**

**Given** an E2EE Room
**When** messages arrive and are sent
**Then** keeper encrypts outgoing and decrypts incoming transparently in `keeper-core` (e2e-encryption feature), with plaintext and key material never crossing into JS (NFR-9)
**And** sending into an encrypted Room from keeper is decryptable by another Matrix client in the Room.

**Given** an event that cannot be decrypted yet
**When** it renders
**Then** the timeline shows an explicit stub — "Can't decrypt yet — verify this device or restore key backup" — with an inline action to the verification flow, never a blank (FR-14, UX-DR10).

**Given** a freshly logged-in unverified device
**When** the app opens
**Then** a global banner "Verify this device to read encrypted history" appears; dismissing collapses it to a persistent Settings badge, not gone (UX-DR11).

### Story 3.2: Device Verification — Emoji/SAS and QR

As a user,
I want to verify my keeper login from an existing session and vice versa,
So that my devices trust each other and encrypted history unlocks.

**Requirements:** FR-14 (verification leg)
**Dependencies:** 3.1

**Acceptance Criteria:**

**Given** an existing verified session (e.g., Element) on the same Account
**When** the user starts verification from either side
**Then** keeper completes interactive verification via emoji/SAS comparison or QR scan/display, and afterwards the keeper device shows as trusted from both ends (FR-14).

**Given** the verification flow UI
**When** it runs
**Then** each state (waiting, comparing, confirmed, cancelled, failed) renders distinctly using the SDK's flow vocabulary (Element-X-style patterns, no novel crypto UX), and the flow is fully keyboard-operable (NFR-14).

**Given** successful verification
**Then** previously undecryptable events re-render decrypted where keys arrive via the now-trusted session, and the unverified banner clears.

### Story 3.3: Key Backup — Enable and Restore

As a user,
I want key backup set up and restorable with my recovery key,
So that a fresh login can read my encrypted history.

**Requirements:** FR-14 (backup leg)
**Dependencies:** 3.2

**Acceptance Criteria:**

**Given** an Account without key backup
**When** the user enables it from Settings
**Then** keeper creates/joins the server-side backup and displays the recovery key exactly once in `mono` type with an explicit "save this" step; the key is storable in the Keychain at the user's choice.

**Given** a fresh keeper login on an Account with existing backup
**When** the user restores with a valid recovery key
**Then** historical encrypted messages decrypt after restore (FR-14), and an invalid key produces a named inline error, not a generic failure.

**Given** backup state
**Then** Settings shows current backup status (enabled / not set up / error) sourced from the Rust core.

### Story 3.4: Replies and Edits

As a user,
I want to reply to specific messages and edit my own,
So that conversations keep their structure across Matrix and Bridges.

**Requirements:** FR-10, FR-11 (timeline leg; archive priors arrive in Story 5.2)
**Dependencies:** 3.1

**Acceptance Criteria:**

**Given** any message in the timeline
**When** the user replies (hover/focus action bar or `r` with the message selected)
**Then** the sent reply renders with the quoted original inline, arrives as a reply on the remote Network in a bridged Chat (given Bridge support), and clicking a received reply's quote jumps to the original message in the timeline (FR-10).

**Given** the user's own sent message
**When** they edit it (action bar, or `↑` in an empty composer for the last own message)
**Then** the timeline updates in place with an "Edited" caption, and the edit propagates to the remote Network where the Bridge supports it (FR-11)
**And** received edits render the latest content with the "Edited" caption.

**Given** edit/reply composition
**When** the user presses Esc
**Then** the pending edit/reply context cancels without losing composer text.

### Story 3.5: Reactions

As a user,
I want to react to messages and see aggregated reactions,
So that lightweight signals work across networks.

**Requirements:** FR-12
**Dependencies:** 3.1

**Acceptance Criteria:**

**Given** a message
**When** the user adds an emoji reaction from the action-bar Popover
**Then** the reaction appears in a pill row under the bubble, round-trips correctly in Matrix-native and bridged Chats, and removing it (click own reaction) retracts it remotely (FR-12).

**Given** multiple reactors on one message
**When** reactions render
**Then** counts aggregate per emoji with the user's own reaction visually highlighted, and click toggles it.

**Given** incoming reaction events
**Then** they render within the normal diff stream without full timeline re-render.

### Story 3.6: Receive Media — Thumbnails, Protocol Streaming, Preview

As a user,
I want images, video, audio, and files I receive to render with thumbnails and open instantly,
So that rich conversations work without the UI ever choking on bytes.

**Requirements:** FR-13 (receive leg); AD-4, NFR-9
**Dependencies:** 3.1

**Acceptance Criteria:**

**Given** an incoming media message (including in E2EE rooms)
**When** it renders
**Then** a thumbnail appears before full download, decrypted bytes are served exclusively via the Range-capable `keeper-media://` protocol from the Rust media cache — never as base64/JSON over IPC (AD-4, NFR-9) — and download progress shows on the bubble with a retry affordance on failure.

**Given** a media bubble
**When** the user clicks it (or presses Enter on it)
**Then** a Quick-Look-style preview overlay opens (Esc closes, focus returns to the timeline), with video/audio playable via the protocol URL (FR-13).

**Given** received audio messages
**Then** they play back inline (voice-note *recording* is post-MVP per PRD assumption).

### Story 3.7: Send Media and Files

As a user,
I want to attach, paste, or drop files into a Chat with visible upload progress,
So that sending media is as reliable as sending text.

**Requirements:** FR-13 (send leg); NFR-5
**Dependencies:** 3.6

**Acceptance Criteria:**

**Given** an open Chat
**When** the user attaches via the composer button, pastes an image, or drags a file onto the conversation pane
**Then** the send shows upload progress on the bubble, is cancelable during upload, and produces a playable/openable message on the receiving side — verified with a 25 MB video (FR-13).

**Given** an upload that fails
**When** the failure is terminal
**Then** the message shows the persistent "Failed — Retry" state like text sends (NFR-5).

**Given** an E2EE room
**Then** sent media is encrypted and decryptable by other clients in the Room.

### Story 3.8: Delete for Everyone — Redaction

As a user,
I want to delete my own messages for everyone, with honest cross-network framing,
So that removal works where it can and says so where it can't.

**Requirements:** FR-15; UX-DR10, UX-DR17
**Dependencies:** 3.1

**Acceptance Criteria:**

**Given** the user's own message
**When** they choose Delete (action bar or ⌫ with message selected) and confirm in the AlertDialog
**Then** keeper issues a Matrix Redaction, and the timeline shows a redaction stub for all Matrix clients in the Room (FR-15).

**Given** a bridged Chat
**When** the delete confirmation renders
**Then** it names the Network and states removal there is best-effort ("Deletes your copy on this Mac. … Removal on Telegram is best-effort.") (FR-15, UX-DR17).

**Given** received redactions
**Then** the affected message renders as a stub in the timeline (local archive retention is governed by Story 5.2).

### Story 3.9: Receipts, Typing, and History Pagination

As a user,
I want to see who's read and who's typing, have my own signals sent, and scroll deep history smoothly,
So that conversations feel live and the past stays reachable.

**Requirements:** FR-16, FR-17 (homeserver leg); AD-14 (module seed); NFR-4
**Dependencies:** 3.1

**Acceptance Criteria:**

**Given** normal (non-Incognito) operation
**When** the user reads a Chat or types in the composer
**Then** public `m.read` receipts and typing notifications are emitted exclusively through the new `keeper-core::signals` module — established here as the only module allowed to call SDK receipt/typing/presence APIs (AD-14 seed; Incognito policy logic lands in Epic 8)
**And** received typing indicators and read states render within 2 s of the event: ticks on own messages, micro-avatars at others' read positions (FR-16).

**Given** a Chat with ≥ 10k events
**When** the user scrolls back
**Then** back-pagination from the homeserver proceeds without UI freeze (NFR-4), showing an inline boundary row "Older history loads from your homeserver" with a spinner while paginating (FR-17)
**And** when offline, the boundary says so and stops instead of spinning forever.

**Given** a code audit
**Then** no module other than `signals` calls SDK receipt/typing/presence APIs (enforced by convention test or lint).

## Epic 4: Unified Inbox Organization

Turn the merged list into the category-defining triage surface: unread management, Archive view with auto-return, Pins, Favorites, Space filtering, and per-row Network/Account attribution with a simple Network filter.

### Story 4.1: Unread Management

As a user,
I want accurate unread and mention badges and manual read/unread control,
So that the inbox reflects exactly what still needs me.

**Requirements:** FR-19; UX-DR3
**Dependencies:** Epic 2 (inbox projection)

**Acceptance Criteria:**

**Given** synced Accounts
**When** the inbox renders
**Then** each Chat row shows its unread state — filled primary badge for mentions, neutral dot otherwise, name at weight 600 (bold means unread and nothing else) — matching server-side read-marker state after sync convergence (FR-19, UX-DR3).

**Given** any Chat row
**When** the user chooses "Mark read" / "Mark unread" from the context menu (single-key `u` arrives with Epic 9)
**Then** the state updates locally within one frame and round-trips to the server read marker.

**Given** unread counts
**Then** they are computed in `keeper-core::inbox` and streamed — never derived in TS (AD-20).

### Story 4.2: Archive View with Auto-Return

As a user,
I want to archive Chats out of my inbox and trust them to come back on new activity,
So that inbox zero is a flow, not a risk.

**Requirements:** FR-20; UX-DR13, UX-DR18
**Dependencies:** 4.1

**Acceptance Criteria:**

**Given** a Chat in the Unified Inbox
**When** the user archives it (context menu; `e` arrives with Epic 9)
**Then** it leaves the inbox and appears in the Archive view (sidebar entry, later ⌘2), and unarchiving returns it to chronological position (FR-20).

**Given** an archived Chat
**When** a new message arrives in it
**Then** it automatically returns to the Unified Inbox (FR-20).

**Given** restarts and other clients
**Then** archive state persists across relaunch and syncs via low-priority tag semantics where representable, and the empty Archive view shows "Nothing archived. `E` archives a chat and keeps it searchable." (UX-DR13).

### Story 4.3: Pins

As a user,
I want my most important Chats pinned to a strip at the top, in my order,
So that they're always one glance away regardless of activity.

**Requirements:** FR-22; UX-DR4
**Dependencies:** 4.1

**Acceptance Criteria:**

**Given** a Chat
**When** the user pins it
**Then** it renders as a circular 44 px avatar (network badge overlaid) in the Pins strip at the top of the chat list and leaves the chronological flow below; unpinning returns it to chronological position (FR-22, UX-DR4).

**Given** multiple pinned Chats
**When** the user drags a pin
**Then** the order updates and persists across restarts (FR-22)
**And** overflow beyond 8 pins scrolls horizontally.

**Given** activity in unpinned Chats
**Then** pinned Chats stay at top irrespective of newer activity elsewhere (FR-22).

### Story 4.4: Favorites

As a user,
I want a curated always-visible Favorites section,
So that key people are one interaction away from anywhere.

**Requirements:** FR-21; UX-DR4, UX-DR13
**Dependencies:** 4.1

**Acceptance Criteria:**

**Given** a Chat marked as Favorite (context menu / detail panel)
**When** the chat list renders
**Then** a FAVORITES `section-label` group of compact 48 px rows sits between the Pins strip and the inbox scroll, visible regardless of inbox scroll position — one interaction from anywhere (FR-21, UX-DR4).

**Given** favorite state
**When** the app restarts or the user re-logs in
**Then** Favorites persist (server-side tag where representable) (FR-21)
**And** the section's collapse/expand state persists.

**Given** no Favorites yet
**Then** the section is hidden entirely and a one-time hint appears in the chat-row context menu instead (UX-DR13).

### Story 4.5: Spaces as Room-Group Views

As a user,
I want to filter my inbox to any Matrix Space I belong to,
So that contexts like a client or a team become one click.

**Requirements:** FR-23; UX-DR18
**Dependencies:** 4.1

**Acceptance Criteria:**

**Given** Accounts belonging to Spaces
**When** the sidebar renders
**Then** a SPACES group lists each Space, and selecting one filters the Unified Inbox to that Space's Rooms; the active filter renders as a dismissible chip above the chat list, and clearing it (chip or Esc from the list) restores the full inbox (FR-23).

**Given** Space membership changes on the homeserver
**When** sync delivers them
**Then** the SPACES group and filter results update accordingly (FR-23).

**Given** scope discipline
**Then** no create/edit/join/leave/hierarchy management exists anywhere — view and filter only (FR-23)
**And** an empty filtered list shows "No chats in {filter}." with a Clear filter action (UX-DR13).

### Story 4.6: Network & Account Attribution and Network Filter

As a user,
I want every Chat to show exactly which Network and Account it lives on, and to filter by Network,
So that identical contacts across accounts are never ambiguous.

**Requirements:** FR-24; UX-DR3, UX-DR18
**Dependencies:** 4.1, 4.5 (filter composition)

**Acceptance Criteria:**

**Given** any Chat row and Chat header
**When** they render
**Then** both carry a 16 px Network badge (bottom-right avatar overlay with 2 px ring) and an Account marker (3 px hue edge bar on rows; account initial chip in the header), so two Chats with the same remote contact via different Accounts always differ visibly (FR-24, UX-DR3).

**Given** the sidebar NETWORKS group
**When** the user selects a Network chip
**Then** the inbox filters to that Network; one Network filter and one Space filter may compose (AND); the active combination renders as dismissible chips (FR-24).

**Given** network identity discipline
**Then** Network identity appears only as badges — never as per-network coloring of rows, panes, or bubbles (DESIGN Don'ts).

## Epic 5: Local Archive, Search & Export — History That Survives

The trust pillar: every synced event lands in `archive.db`, edits keep version chains, deletions mark but never erase (configurable), FTS answers in < 200 ms offline, exports are lossless, and the archive outlives sign-out.

### Story 5.1: Archive Ingestion Pipeline

As a user,
I want every message keeper ever syncs persisted on my disk,
So that my history stops depending on any platform's retention.

**Requirements:** FR-33; AD-10, AD-11; NFR-5, NFR-8
**Dependencies:** Epic 3 (post-decryption events incl. media metadata)

**Acceptance Criteria:**

**Given** connected Accounts
**When** events flow through sync
**Then** a per-account archiver task consumes post-decryption events and appends normalized rows (event id, account_id, room, sender, origin ts, type, content JSON, media metadata) to `archive.db` — one database for all Accounts, written by a single serialized writer task, WAL mode (FR-33, AD-11, NFR-8).

**Given** an app restart with network disabled
**When** the archive is queried
**Then** every event previously visible in any timeline is present and queryable (FR-33, NFR-5).

**Given** media messages
**Then** locally cached media files remain openable offline, and message text/metadata retention is independent of any media cache policy (FR-33).

### Story 5.2: Durability Against Remote Rewrites + Edit History

As a user,
I want my local copy to survive remote edits and deletions, with the history inspectable,
So that the platform's rewrite loses to my archive.

**Requirements:** FR-36, FR-11 (edit history UI); AD-11; UX-DR17
**Dependencies:** 5.1

**Acceptance Criteria:**

**Given** a message that is remotely edited
**When** the edit syncs
**Then** the archive holds both versions as a version chain, the timeline shows the latest with the "Edited" caption, and clicking it opens the edit-history popover fed by the Local Archive (FR-36, FR-11).

**Given** a remote Redaction or network-side deletion
**When** it syncs
**Then** the timeline shows the redaction stub (always honored in the view), while the pre-redaction content remains retrievable via archive search/export — unless "Honor remote deletions locally" is enabled (FR-36).

**Given** Settings → Archive & Storage
**When** it renders
**Then** it carries the plain disclosure that keeper keeps local copies of remotely edited/deleted messages by default, that this affects only this Mac, and the "Honor remote deletions locally" toggle (FR-36, UX-DR17).

### Story 5.3: Offline Full-Text Search Engine

As a user,
I want my entire archive indexed for instant offline search,
So that any message from any network is milliseconds away.

**Requirements:** FR-34 (engine), NFR-2; AD-12
**Dependencies:** 5.1

**Acceptance Criteria:**

**Given** archive ingestion
**When** rows are appended
**Then** an FTS5 external-content table with `tokenize="trigram"` (case-insensitive) indexes message text incrementally at ingest — CJK-capable by construction (AD-12).

**Given** a 100k+-event archive and network disabled
**When** a search command runs
**Then** first results return in < 200 ms (p95 over a standard query set), verified by a CI perf test (NFR-2)
**And** queries under 3 characters fall back to trigram-accelerated `LIKE` (AD-12).

**Given** the search command surface
**Then** it accepts sender / Chat / Network / Account / date-range filters and returns results with `(account_id, room_id, event_id)` for deep-linking (FR-34).

### Story 5.4: Search UI — Global and In-Chat

As a user,
I want ⌘⇧F to search everything and ⌘F to search this Chat, deep-linking into timelines,
So that finding beats scrolling, even offline.

**Requirements:** FR-34 (UI); UX-DR13; DESIGN search-highlight
**Dependencies:** 5.3

**Acceptance Criteria:**

**Given** the global search surface (⌘⇧F)
**When** the user types a query and adds filter chips (sender, Chat, Network, Account, date range)
**Then** results group by Chat with matches tinted in the search-highlight token, the header states "Searching your local archive", and everything works offline (FR-34).

**Given** a result
**When** the user presses Enter
**Then** keeper deep-links into the containing Chat's timeline at the matched message, highlighted for 2 s (FR-34).

**Given** the open Chat
**When** the user presses ⌘F
**Then** the same engine runs scoped to that Chat from the same affordance (FR-34)
**And** no-results shows "No matches in your archive." with active filter chips removable one-tap (UX-DR13).

### Story 5.5: Export to JSON and Markdown

As a user,
I want to export any Chat, Account, or everything to JSON and Markdown in the background,
So that my history is portable and provable.

**Requirements:** FR-35; AD-11 (reads archive.db only); UX-DR11
**Dependencies:** 5.1, 5.2

**Acceptance Criteria:**

**Given** the Export dialog (detail panel / search results)
**When** the user picks scope (this Chat / this Account / everything), formats (JSON, Markdown), include-media, and destination
**Then** the export runs as a background job reading `archive.db` only, with a progress toast showing counts and Cancel, and messaging is never blocked (FR-35).

**Given** a 10k-message Chat export
**When** it completes
**Then** the JSON is complete and well-formed (event count matches the archive), the Markdown transcript is chronologically ordered with sender, timestamp, edits (final text), and media as relative file links, and the toast offers Reveal in Finder (FR-35).

**Given** an export failure
**Then** a persistent alert appears in the Export surface (not toast-only) noting partial-file cleanup (UX-DR11).

### Story 5.6: Archive-First Pagination

As a user,
I want scrollback served from my local archive before touching the network,
So that history is instant and works offline.

**Requirements:** FR-17 (archive-first completion); NFR-1, NFR-4
**Dependencies:** 5.1

**Acceptance Criteria:**

**Given** a Chat with history in the Local Archive
**When** the user scrolls back
**Then** archived events render immediately from `archive.db` before any homeserver pagination, with the seam invisible in normal use (FR-17).

**Given** scrollback past archived history
**When** older events require network
**Then** the visible boundary row indicates homeserver loading, and while offline it states that older history needs a connection and stops (FR-17).

**Given** the 10k-event scroll test
**Then** pagination stays freeze-free and scroll stays smooth (NFR-4).

### Story 5.7: Archive Survives Sign-Out — and Deletes Only on Command

As a user,
I want sign-out to keep my archive by default, with deletion a separate deliberate act,
So that leaving an account never silently destroys my history.

**Requirements:** FR-37, FR-6 (completion); AD-10; UX-DR20
**Dependencies:** 5.3, 5.5 (FTS/Export must exist to verify survival)

**Acceptance Criteria:**

**Given** the sign-out dialog from Story 2.5
**When** the user signs out with the default option
**Then** the SDK store and Keychain entries are deleted, `archive.db` is untouched, and FTS and Export over that Account's history still work with no active session (FR-37, FR-6)
**And** the honest-copy caveat applies: content never synced-and-decrypted before sign-out is not recoverable, stated in the dialog copy.

**Given** the destructive option "…and delete this Account's archive"
**When** the user selects it
**Then** confirmation requires typing the Account name, and only that Account's archive rows (and FTS entries) are deleted — other Accounts' data untouched (FR-6, UX-DR20, AD-10).

**Given** either path
**Then** the action is logged (ids only) and the account switcher updates immediately.

## Epic 6: Bridge Management & First-Run Wizard

The reason keeper exists: zero-config Bridge discovery, native login through provisioning API or driven Bridge Bot, data-driven risk honesty, ≤ 60 s session-health surfacing with one-click re-login, bbctl for Beeper self-hosting, new-chat via resolve-identifier, and the First-Run Wizard tying it together.

### Story 6.1: Bridges Surface with Data-Driven Risk Tiers

As a user,
I want a Bridges view where every Network carries its honest risk label,
So that I know what I'm signing up for before I connect anything.

**Requirements:** FR-30; AD-16 (data files); UX-DR8, UX-DR18
**Dependencies:** Epic 2

**Acceptance Criteria:**

**Given** the repo
**When** this story lands
**Then** `crates/keeper-core/data/` contains versioned JSON for risk tiers, coupling caveats, and the known-bot registry, matching the addendum §2 table — consumed by the core, never hardcoded in UI (FR-30, AD-16).

**Given** the Bridges view (sidebar entry, later ⌘4)
**When** it renders
**Then** each Network × Account shows a Bridge card: network glyph, name, risk-tier badge per the tier→badge mapping, health dot placeholder, and a primary action (UX-DR8)
**And** the sidebar Bridges entry carries a worst-state health roll-up dot (UX-DR18).

**Given** a volatile-tier Network
**When** the user initiates connect
**Then** an AlertDialog with the tier badge and plain-language ToS/ban copy from the data file requires "I understand the risk — connect" before proceeding; low-risk Networks show only the label (FR-30).

### Story 6.2: Bridge Discovery

As a user,
I want keeper to find the Bridges on my homeserver by itself,
So that I never have to know a bot's Matrix ID.

**Requirements:** FR-25; AD-16 (3-source discovery)
**Dependencies:** 6.1

**Acceptance Criteria:**

**Given** a homeserver with mautrix-whatsapp and mautrix-telegram registered
**When** discovery runs for a connected Account
**Then** both Bridges appear in the Bridge list with status (configured / logged in / not logged in) without the user naming bot IDs, using merged results from `GET /_matrix/client/v3/thirdparty/protocols`, the known-bot MXID probe registry, and a scan of existing bot DMs/portal rooms (FR-25, AD-16).

**Given** a homeserver with no Bridges
**When** the Bridges view renders
**Then** it shows "No bridges found on {homeserver}." with a companion-stack docs link (FR-25, UX-DR13).

**Given** multiple Accounts
**Then** discovery runs per Account and cards are keyed Network × Account.

### Story 6.3: Native Bridge Login via Provisioning API

As a user,
I want to log a Bridge into a Network entirely inside keeper — QR on screen, codes in native fields,
So that `!wa login` never happens.

**Requirements:** FR-26; AD-16 (`Provisioning` transport); UX-DR8
**Dependencies:** 6.2

**Acceptance Criteria:**

**Given** a Bridge exposing the bridgev2 provisioning API
**When** the user clicks Connect on its card
**Then** the login stepper (Sheet) drives the provisioning JSON state machine natively: choosing method → waiting → QR panel or code-entry `InputGroup` → success/failure — each state rendered distinctly (FR-26)
**And** the transport is the `Provisioning` impl of the `BridgeTransport` trait (AD-16).

**Given** the WhatsApp QR flow
**When** the QR renders
**Then** it sits on a white card ≥ 240 px with quiet zone (both themes), with the per-network instruction line and a live state word; scanning it completes login end-to-end and the state flips to "Linked ✓" in bridge-healthy green with auto-advance (FR-26, UX-DR8)
**And** QR expiry regenerates in place with a subtle "QR refreshed" note.

**Given** a provisioning failure
**Then** the failure state shows the Bridge's own error message verbatim with Retry (FR-26).

### Story 6.4: Bridge Bot Fallback Driver

As a user,
I want the same native login flow even on Bridges without a provisioning API,
So that legacy deployments don't dump me into a bot chat.

**Requirements:** FR-27; AD-16 (`BotDriver` transport); UX-DR19
**Dependencies:** 6.3

**Acceptance Criteria:**

**Given** a Bridge without a provisioning endpoint
**When** the user runs login/list-logins/logout/set-relay operations
**Then** the `BotDriver` transport sends and parses Bridge Bot commands programmatically with timeouts, and the user sees the *same* stepper states (QR/code rendered natively) — indistinguishable from the provisioning path (FR-27, AD-16).

**Given** any Bridge
**When** the user looks for the raw bot
**Then** the Bridge Bot Chat remains accessible (Bridge card menu "Open Bridge Bot chat" + detail panel), and the stepper's failure state offers it as the manual escape hatch (FR-27, UX-DR19).

**Given** unparseable bot output
**Then** the stepper fails with the bot's raw reply shown verbatim rather than guessing.

### Story 6.5: Bridge Session Health and Re-Login Prompts

As a user,
I want a dying Bridge session to be impossible to miss and one click to fix,
So that no network silently eats my messages for days.

**Requirements:** FR-28 (detection + in-app surfacing; native notification leg completes in Story 10.4), NFR-6; AD-16; UX-DR8, UX-DR11
**Dependencies:** 6.3

**Acceptance Criteria:**

**Given** a logged-in Bridge Session
**When** its state changes (e.g., device unlinked from the phone)
**Then** a per-session state machine (healthy / degraded / disconnected) fed by bridgev2 state events with bot-ping fallback reflects the change in keeper within 60 s of it reaching the homeserver (FR-28, NFR-6).

**Given** an unhealthy session
**When** surfaced
**Then** the state is persistent until resolved — card state word + dot (pulse twice, then steady), sidebar Bridges roll-up, health dot on affected Chat rows, and a non-dismissible inline banner in affected conversations: "Signal disconnected — messages may not arrive. Re-link" (FR-28, UX-DR8, UX-DR11).

**Given** the prompt or banner
**When** clicked
**Then** the user lands directly in the re-login flow for that exact Bridge (FR-28).

### Story 6.6: Start New Chats via Bridge

As a user,
I want to start a chat with a phone number or username on any bridged Network,
So that keeper originates conversations, not just receives them.

**Requirements:** FR-32; UX-DR14 (⌘N)
**Dependencies:** 6.3

**Acceptance Criteria:**

**Given** the new-chat dialog (⌘N)
**When** the user picks Network + Account (defaulting to last used) and enters an identifier (phone, username, Matrix ID)
**Then** keeper resolves it through the Bridge's resolve-identifier with a visible resolving state and opens the resulting Chat with composer focused (FR-32).

**Given** an unresolvable identifier
**When** resolution fails
**Then** an inline "Not found on {Network} — check the number or username." appears with input retained for correction — no dialog dismissal (FR-32).

**Given** a Network whose Bridge lacks resolve support
**Then** the dialog says so upfront instead of failing late (FR-32).

### Story 6.7: bbctl Integration for Beeper Self-Hosted Bridges

As a Beeper user,
I want keeper to register and run my own bridges via bbctl,
So that I get network parity without a terminal.

**Requirements:** FR-29; AD-16 (sidecar)
**Dependencies:** 6.3, 2.3 (Beeper Account)

**Acceptance Criteria:**

**Given** a connected Beeper Account and bbctl available
**When** the user picks a Network in the "Run your own bridge" section
**Then** keeper drives `bbctl` register/run as a launch-on-demand Tauri sidecar with a log-free progress stepper, and the resulting Bridge appears in the Bridge list with status — end to end from "no Signal bridge" to logged-in without leaving keeper (FR-29).

**Given** bbctl is absent
**When** the section renders
**Then** it offers guided install instructions, and everything else in keeper functions fully without it (FR-29).

**Given** sidecar lifecycle
**Then** scope is launch-on-demand + status surfacing only (auto-restart policies and log viewer are v1.x, flagged post-MVP).

### Story 6.8: First-Run Wizard

As a new user,
I want first launch to walk me from zero to a bridged inbox — or let me skip any step,
So that the setup cliff becomes a staircase.

**Requirements:** FR-31; UX-DR16, UX-DR17
**Dependencies:** 6.2, 6.3, 6.4 (reuses login flows and stepper)

**Acceptance Criteria:**

**Given** first launch with no Accounts
**When** keeper opens
**Then** the Wizard replaces the frame: Welcome → Add Account (three tabs: Homeserver login / OIDC / Beeper, reusing Epic 1–2 flows) → Bridge discovery (found list with tier badges) → per-Bridge login (reusing the stepper) → Done, landing in the Inbox (FR-31, UX-DR16).

**Given** a user without a homeserver
**When** they reach Add Account
**Then** the honest fork renders in order: companion-stack docs, managed-host pointers, Beeper Account path — no fake sign-up (FR-31, UX-DR17).

**Given** any step
**When** the user chooses "Skip for now" (or Esc, which asks once)
**Then** they proceed without lock-in, skipping everything lands in an empty Inbox with an "Add an account to start" card, and the Wizard is re-enterable from Settings (FR-31)
**And** a prepared-homeserver user reaches an inbox with ≥ 1 bridged Network logged in without leaving the Wizard.

## Epic 7: Drafts & Approval Pane — The Airlock

Unsent text becomes a first-class object: persisted locally, mirrored across devices, listed in one Approval Pane, and guarded by the product's hardest invariant — nothing sends without explicit approval.

### Story 7.1: Persistent Per-Chat Drafts

As a user,
I want everything I type to survive chat switches, restarts, and crashes,
So that no half-written thought is ever lost.

**Requirements:** FR-38; AD-15 (local truth); UX-DR3 (draft marker)
**Dependencies:** Epic 3 (composer complete)

**Acceptance Criteria:**

**Given** text typed in any composer
**When** the user switches Chats, force-quits, or the app crashes
**Then** the text persists instantly to the `drafts` table in `keeper.db` (the source of truth, AD-15) and is restored in the same Chat's composer on return/relaunch (FR-38).

**Given** Chats with pending Drafts
**When** the inbox renders
**Then** their rows show the amber draft marker (pencil glyph + "Draft" prefix in the preview line) (FR-38, UX-DR3).

**Given** a sent or cleared composer
**Then** the Draft row is removed and the marker disappears.

### Story 7.2: Cross-Device Draft Mirroring with Local-Wins Conflicts

As a user,
I want drafts to follow my account across devices without ever clobbering local text,
So that midnight-me on the laptop and morning-me on the desktop stay in sync.

**Requirements:** FR-39; AD-15; UX-DR20
**Dependencies:** 7.1

**Acceptance Criteria:**

**Given** a Draft written in keeper
**When** it persists
**Then** it mirrors debounced and best-effort to per-Room account data (custom type `dev.keeper.draft`), with `Room::save_composer_draft` additionally written for Element-family interop (FR-39, AD-15)
**And** editing the Draft updates the mirror.

**Given** the Draft changed on another device
**When** keeper syncs the remote version while local unsent text exists
**Then** the local version wins, and a quiet chip above the composer offers "Edited on another device — Use that version" for one-tap adoption — local text is never silently destroyed (FR-39, UX-DR20).

**Given** mirror failures (e.g., hungryserv gaps recorded in OQ-3)
**Then** local persistence is unaffected and the degradation is invisible except for the missing cross-device echo.

### Story 7.3: Approval Pane

As a user,
I want one surface listing every pending Draft across all Accounts, where I edit, approve, or discard each,
So that writing and sending are deliberately separate acts.

**Requirements:** FR-40; AD-15 (cross-account query); UX-DR13, UX-DR18
**Dependencies:** 7.1

**Acceptance Criteria:**

**Given** Drafts in ≥ 3 Chats across ≥ 2 Accounts
**When** the Approval Pane opens (sidebar entry with amber count badge, later ⌘3)
**Then** all pending Drafts list grouped by Account then Chat, each row showing Chat, Network badge, Account hue, Draft preview, and age — a cross-account query over drafts + pending outbox rows (FR-40, AD-15).

**Given** a Draft row
**When** the user acts
**Then** Enter opens inline editing, approve dispatches through `send::submit(draft, trigger = ApprovalPaneApprove)` — the second and last legal trigger — honoring the Undo-Send Window once Epic 8 lands, and discard removes the Draft locally and from mirrored account data with a 5 s undo toast (FR-40).

**Given** MVP scope
**Then** no approve-all/select-all-and-send affordance exists; the layout reserves a leading proposer column rendering "You" silently (post-MVP agents), and the empty state reads "Nothing waiting. Drafts you write stay here until you approve them — nothing sends without you." (FR-40, UX-DR13).

### Story 7.4: Explicit-Approval Invariant — Enforced and Tested

As a user,
I want a hard guarantee that keeper can never send anything I didn't explicitly approve,
So that the airlock is a property of the system, not a promise.

**Requirements:** FR-41; AD-13; UX-DR17
**Dependencies:** 7.3

**Acceptance Criteria:**

**Given** the `keeper-core::send` module
**When** audited and tested
**Then** `send::submit(text|draft, trigger)` with `trigger ∈ {ComposerSend, ApprovalPaneApprove}` is the only public path into any account's SendQueue; Rust tests assert both that the two triggers dispatch and that no other public API can cause dispatch (FR-41, AD-13).

**Given** the invariant's documentation
**When** the story completes
**Then** the contract is documented in `keeper-core::send` rustdoc as binding on future agent-proposal features (agents may propose; only the user approves) (FR-41).

**Given** the UI
**Then** Settings → Privacy carries "Nothing sends without you." and no surface anywhere offers scheduled, background, or bulk dispatch (FR-41, UX-DR17).

## Epic 8: Incognito & Undo-Send — Privacy on the User's Terms

Beeper's paywalled privacy, free: private read receipts with deterministic scoping, typing/presence suppression with per-network honesty, manual receipt release, the pre-dispatch Undo-Send Window, and honest post-dispatch deletion.

### Story 8.1: Incognito Read Receipts with Scoped Policy

As a user,
I want to read without leaking read receipts — globally, per Account, or per Chat,
So that I answer on my terms, not social pressure's.

**Requirements:** FR-42; AD-14; UX-DR7
**Dependencies:** Epic 3 (Story 3.9 signals module), Epic 7 (settings surface conventions)

**Acceptance Criteria:**

**Given** Incognito Mode enabled at any scope (global via Settings, per-Account via Account menu, per-Chat via header chip / later ⌘⇧I)
**When** the user reads a Chat where Incognito applies
**Then** `keeper-core::signals` resolves effective policy (Chat > Account > Global) at emission time and emits `m.read.private` instead of `m.read` — the remote party's client keeps showing the message unread, while the user's own read position still syncs across their devices (FR-42, AD-14).

**Given** the effective state
**When** the Chat renders
**Then** the header shows the violet incognito chip with the *effective* scope ("Incognito — this chat overrides account"), and the composer focus ring tints violet while Incognito applies (UX-DR7).

**Given** precedence
**Then** scope resolution is deterministic and covered by unit tests for all eight combinations (FR-42).

### Story 8.2: Typing/Presence Suppression, Coupling Caveats, and Manual Release

As a user,
I want typing and presence suppressed too, honest warnings where networks couple behaviors, and a way to release a receipt when I choose,
So that suppression is complete, informed, and reversible on demand.

**Requirements:** FR-43, FR-44, FR-45; AD-14; data file from Story 6.1
**Dependencies:** 8.1

**Acceptance Criteria:**

**Given** Incognito applies to a Chat
**When** the user types a long message
**Then** zero typing events leave the machine (verifiable at the homeserver), and presence is withheld where the protocol allows — all through the `signals` module, with no other code path able to emit (FR-43, AD-14).

**Given** a Network with coupled behaviors (e.g., WhatsApp)
**When** the user toggles Incognito on such a Chat
**Then** the coupling caveat surfaces inline at toggle time ("you may also stop seeing others' read receipts"), sourced from the same data file as risk tiers (FR-44).

**Given** an Incognito Chat the user wants to acknowledge
**When** they trigger "Mark read publicly" from the chip
**Then** `signals::release_receipt(room)` emits exactly one public `m.read` at the current read position; without it, only private receipts are ever sent while Incognito applies (FR-45).

### Story 8.3: Undo-Send Window

As a user,
I want every approved send held locally for a few seconds I control,
So that I can un-embarrass myself before anything leaves the machine.

**Requirements:** FR-46; AD-13; UX-DR6; NFR-8
**Dependencies:** Epic 7 (both dispatch triggers exist)

**Acceptance Criteria:**

**Given** an approved send (composer or Approval Pane) with window > 0 (default 10 s, 0–60 s in Settings; 0 disables holding)
**When** dispatch is requested
**Then** the message inserts into the `outbox` table with `dispatch_at = approval_time + window`, renders in the timeline as a distinct amber `held` state, and the undo-send pill floats above the composer with radial countdown + "Sending in Ns — Undo" (numeric-only under reduced motion; multiple pending sends stack oldest-first) (FR-46, AD-13, UX-DR6).

**Given** the countdown running
**When** the user clicks Undo or presses ⌘⇧Z
**Then** the outbox row is deleted with zero network dispatch (verifiable at the homeserver) and the full text returns to that Chat's composer as a Draft (FR-46).

**Given** the window elapses
**Then** the scheduler moves the row into that Account's SendQueue and normal send states take over
**And** after crash or offline: elapsed rows dispatch on startup/reconnect, unelapsed rows resume their countdown (FR-46, NFR-8).

### Story 8.4: Post-Dispatch Delete for Everyone

As a user,
I want to delete an already-sent message everywhere it can reach, told honestly where that ends,
So that damage control works without false promises.

**Requirements:** FR-47; UX-DR17; FR-36 interplay
**Dependencies:** 8.3 (distinguishes held-cancel from post-dispatch delete)

**Acceptance Criteria:**

**Given** a message already dispatched (window elapsed or zero)
**When** the user deletes it for everyone
**Then** keeper issues a Matrix Redaction, and in bridged Chats the confirmation names the Network and states removal there is best-effort (FR-47).

**Given** the user's own deletion
**When** the archive processes it
**Then** the Local Archive treats it per FR-36 semantics (mark, keep priors unless "honor remote deletions" is on) (FR-47).

**Given** a message still in its undo window
**Then** the same user intent resolves as an undo (Story 8.3) rather than a Redaction — no network event exists to redact.

## Epic 9: Command Palette, Hotkeys & Keyboard Mastery

The Texts/Beeper heritage: ⌘K over everything, single-key list verbs, an Esc chain that always makes sense, a generated cheat sheet and menu bar, and a global hotkey that summons keeper from anywhere in macOS.

### Story 9.1: Command Palette

As a user,
I want ⌘K to fuzzy-find any Chat, contact, or action instantly,
So that everything in keeper is one keystroke away.

**Requirements:** FR-48; AD-20 (Rust index); UX-DR9, UX-DR13
**Dependencies:** Epics 4–8 surfaces (actions to register)

**Acceptance Criteria:**

**Given** the palette open (⌘K, 640 px panel)
**When** the user types ≥ 2 characters
**Then** results filter across Chats (all Accounts, with network badge + account hue dot), contacts, and the registered action list, served by a Rust in-memory index via command with results within 100 ms per keystroke at 10k Chats (FR-48, AD-20).

**Given** the `>` prefix
**When** typed
**Then** the palette switches to action mode (Archive, Toggle Incognito, Open Approval Pane, Start Export, Bridge operations, …) with kbd chips, context-aware ranking (open-Chat actions first), Enter executes, ⌘Enter on a Chat peeks without closing (FR-48, UX-DR9).

**Given** the parity requirement
**Then** an action-registry module is the single source for palette actions (cheat sheet and menu bar consume it in 9.3), every MVP feature registers at least one action, and no-matches shows the top registered actions plus a `>` hint (FR-48, UX-DR13).

### Story 9.2: Keyboard Navigation and Quick-Switcher

As a user,
I want to run the entire triage loop — walk unreads, archive, reply, next — without touching the mouse,
So that 40 chats fall in four minutes.

**Requirements:** FR-49; UX-DR14, UX-DR12 (roving tabindex, Esc chain); NFR-14
**Dependencies:** 9.1 (Quick-Switcher rides the palette index)

**Acceptance Criteria:**

**Given** the full shortcut set
**When** implemented
**Then** ⌘1–4 switch views; ⌃Tab/⌃⇧Tab cycle Chats; ⌥⌘↓/⌥⌘↑ jump next/previous unread; ↑/↓ and j/k move list selection; Enter opens with composer focused; and the single-key list verbs work with the chat list focused: `e` archive, `u` read/unread, `p` pin, `f` favorite, `m` mute menu (FR-49, UX-DR14).

**Given** the Esc chain
**When** Esc is pressed anywhere
**Then** it walks up exactly: overlay → composer → timeline → clear filter → chat list (UX-DR14)
**And** timeline focus supports ↑/↓ select, `r` reply, `e` edit own, ⌫ delete dialog.

**Given** the UJ-3 triage loop
**When** executed end to end (walk unreads → archive → reply → next)
**Then** it completes with zero pointer use, with roving tabindex in chat list, timeline, and Approval Pane (FR-49, NFR-14).

### Story 9.3: Cheat Sheet and Native Menu Bar from the Action Registry

As a user,
I want ⌘? to show every shortcut and the macOS menu bar to mirror every command,
So that discovery is native and the reference can never drift from reality.

**Requirements:** FR-49 (cheat sheet), NFR-14; UX-DR15
**Dependencies:** 9.1, 9.2

**Acceptance Criteria:**

**Given** ⌘?
**When** pressed
**Then** a searchable overlay lists all shortcuts, generated from the same action registry as the palette — no hand-maintained list (FR-49, UX-DR15).

**Given** the macOS menu bar
**When** the app runs
**Then** every registered command appears as a native menu item with its shortcut, giving full-keyboard-access and VoiceOver users standard discovery (NFR-14, UX-DR15).

**Given** a release audit
**Then** a checklist (or test) verifies palette parity: every MVP feature with a UI surface is reachable through at least one palette action (FR-48 release gate).

### Story 9.4: Global Hotkey

As a user,
I want a system-wide hotkey that summons or hides keeper,
So that triage is one chord away from any app.

**Requirements:** FR-50; AD-25 (global-shortcut plugin)
**Dependencies:** 9.2

**Acceptance Criteria:**

**Given** the default assignment ⌃⌥Space
**When** pressed while keeper is backgrounded or hidden (with macOS permissions granted)
**Then** the main window raises with focus in the Unified Inbox chat list; pressed while focused, it hides the window (FR-50).

**Given** Settings → Shortcuts
**When** the user reassigns the hotkey
**Then** conflicts with existing system shortcuts are detected at assignment time with a warning (FR-50).

**Given** permission not yet granted
**Then** the setting explains what to enable instead of failing silently.

## Epic 10: Notifications & Background Operation

Reliability is the feature: native notifications straight from the local sync loop, mute/mention-only/DND that actually holds, background sync with honest quit semantics, and click-through that lands exactly right — bridge-health alerts included.

### Story 10.1: Native Notifications from the Sync Loop

As a user,
I want native macOS notifications for new messages within seconds, with privacy control,
So that I can trust keeper while it's in the background.

**Requirements:** FR-51, NFR-7, NFR-11; AD-18
**Dependencies:** Epic 3 (decrypting sync loop)

**Acceptance Criteria:**

**Given** a message arriving while keeper is backgrounded
**When** the local sync loop receives it
**Then** `keeper-core::notify` applies its rules and posts a native notification with sender, Chat, and preview within 5 s of sync receipt, E2EE content rendered only from the local decrypting loop (FR-51, NFR-7, AD-18).

**Given** previews disabled in Settings
**When** notifications post
**Then** they show sender/Chat but no content (FR-51).

**Given** the egress posture
**Then** no notification is ever routed through project-operated or third-party push infrastructure (NFR-11).

### Story 10.2: Mutes, Mention-Only, and Do-Not-Disturb

As a user,
I want granular quiet — per Chat, per Network, mention-only, or everything,
So that keeper interrupts exactly as much as I allow.

**Requirements:** FR-52; AD-18, AD-25
**Dependencies:** 10.1

**Acceptance Criteria:**

**Given** mute controls (chat context menu / detail panel / network chip menu)
**When** a Chat or Network is muted
**Then** it produces zero notifications while its Chats continue updating in the inbox and accumulating unread state, with a mute glyph on the row (FR-52).

**Given** mention-only mode on a Chat
**When** events arrive
**Then** only mentions and replies-to-user notify (FR-52).

**Given** rules persistence
**Then** rules live in settings via `keeper-core::notify`, mapped to Matrix push rules where representable and evaluated locally otherwise, consistent across restarts (FR-52, AD-18)
**And** a global DND toggle in the sidebar footer menu silences everything without losing unread accumulation.

### Story 10.3: Background Operation and Honest Quit

As a user,
I want keeper to keep syncing with the window closed — and to tell me the truth about quitting,
So that "running" always means exactly what it says.

**Requirements:** FR-53; AD-18, AD-25 (autostart plugin)
**Dependencies:** 10.1

**Acceptance Criteria:**

**Given** the window closed (⌘W) with the app running
**When** messages arrive
**Then** sync and notifications behave identically to foreground, optional menu-bar presence keeps keeper reachable, and the dock badge shows unread count per its Setting (all unreads / mentions only / off) (FR-53).

**Given** launch-at-login
**When** offered in Settings
**Then** it is opt-in, off by default (FR-53).

**Given** ⌘Q
**When** the user quits
**Then** sync fully stops and Settings copy says exactly that — no fake "push while quit" promise anywhere (FR-53, UX-DR17).

### Story 10.4: Click-Through and Bridge-Health Alerts

As a user,
I want every notification to land me in exactly the right place — including a dead bridge's fix-it flow,
So that acting on a notification is one click, never a hunt.

**Requirements:** FR-54, FR-28 (notification leg complete), NFR-6, NFR-4; AD-18
**Dependencies:** 10.1, 6.5 (health states)

**Acceptance Criteria:**

**Given** a message notification for Account B's Chat while Account A's Chat is open
**When** clicked
**Then** keeper restores/summons the window and switches to the exact Chat and Account with the relevant message in view, within the interaction-latency bar (FR-54, NFR-4) — payload `(account_id, room_id, event_id)` (AD-18).

**Given** a Bridge Session drop (from Story 6.5's state machine)
**When** it occurs
**Then** a native notification posts within 60 s ("Signal disconnected — re-link to keep receiving messages.") riding the same pipeline, and clicking it lands directly in that Bridge's re-login flow — completing FR-28 end to end (FR-28, NFR-6).

**Given** notification grouping
**Then** notifications group per Chat so a burst doesn't flood Notification Center (FR-51).

## Epic 11: Packaging, Release & Quality Gates

Ship it like a product: signed and notarized builds with signed auto-updates from reproducible CI, the licensing firewall and egress honesty enforced per release, and the PRD's performance/reliability bars wired in as gates.

### Story 11.1: Signed, Notarized Release Pipeline

As a user,
I want keeper to install and launch like any trustworthy macOS app,
So that Gatekeeper, notarization, and provenance are non-issues.

**Requirements:** NFR-12, NFR-13; AD-23, AD-5
**Dependencies:** all feature epics buildable (can land any time after Epic 1; final validation at release)

**Acceptance Criteria:**

**Given** the GitHub Actions release workflow (macOS arm64, tauri-action)
**When** a release tag builds
**Then** it produces a Developer-ID-signed, hardened-runtime, notarized Apple Silicon dmg via the App Store Connect API key in secrets (NFR-12, AD-23).

**Given** PR checks
**When** any PR runs
**Then** `cargo deny check`, biome/tsc/vitest, rustfmt/clippy `-D warnings`, cargo-nextest, and a `tauri build --no-bundle` are required checks — the licensing firewall blocks GPL/AGPL in Rust and npm alike (NFR-13, AD-5).

**Given** ported code
**Then** the PR template carries the provenance checklist (NFR-13).

### Story 11.2: Signed Auto-Updates and Egress Honesty

As a user,
I want updates to arrive signed and every network endpoint keeper talks to listed in the app,
So that trust is verifiable, not asserted.

**Requirements:** NFR-11, NFR-12; AD-5, AD-23; UX-DR17
**Dependencies:** 11.1

**Acceptance Criteria:**

**Given** the updater
**When** a new release publishes
**Then** updater artifacts are signed with the Tauri updater key, and the running app detects, downloads, verifies, and applies the update via the updater plugin (NFR-12).

**Given** Settings → About
**When** rendered
**Then** it shows the rendered egress list — the user's Homeservers/Bridges, api.beeper.com if a Beeper Account exists, and the update endpoint — as UI, not a doc link (NFR-11, UX-DR17).

**Given** each release
**Then** the release job emits an egress diff note, and no telemetry/analytics/crash-reporting exists without explicit opt-in (NFR-11, AD-23).

### Story 11.3: Performance and Reliability Release Gates

As a maintainer,
I want the PRD's hard numbers measured in CI on reference hardware,
So that regressions fail builds instead of reaching users.

**Requirements:** NFR-1, NFR-2, NFR-3 (measure), NFR-4, NFR-8; SM-3/SM-4
**Dependencies:** 11.1; Epic 5 (FTS), Epic 9 (palette)

**Acceptance Criteria:**

**Given** the CI perf harness on Apple Silicon
**When** it runs against a release build with a seeded 100k+-event archive
**Then** it gates: cold start to interactive inbox < 2 s (NFR-1), FTS first results < 200 ms p95 (NFR-2, extends Story 5.3's test), palette results ≤ 100 ms at 10k chats (FR-48), and records idle memory for NFR-3 sign-off (measured, flagged if over the assumed budgets).

**Given** crash-safety validation
**When** the harness kills the process mid-write (archive ingest, outbox insert, settings write)
**Then** relaunch recovers to a consistent state with zero lost previously-persisted events (NFR-8).

**Given** induced bridge-session drops in the test environment
**Then** the ≤ 60 s surfacing bar (NFR-6) is verified as part of the release checklist (SM-3).

## Epic 12: iOS Walking Skeleton — Build, Sign, Run

Prove keeper-on-iPhone before any UX investment, exactly as AD-24 Plan A prescribes: the same `crates/keeper` shell builds as an iOS staticlib, desktop-only code compile-gates out cleanly, the keychain and media protocol work through the existing ports, and CI guards the target forever after. The epic is deliberately UI-free and simulator/compile-first — only Story 12.6 needs a physical device. Exit gate (SM-7): on-device OIDC deep-link login, room list, E2EE text send/receive, and relaunch-restore, on free Personal Team signing.

### Story 12.1: iOS Project Init and Repo Integration

As a keeper developer,
I want `tauri ios init` run and its generated Apple project integrated into the repo under the architecture's rules,
So that the iOS target exists reproducibly, with a stable identity and no hand edits that regeneration can destroy.

**Requirements:** FR-55 (init leg); AD-32
**Dependencies:** none (desktop Epics 1–11 complete)

**Acceptance Criteria:**

**Given** the existing cargo workspace
**When** `tauri ios init` generates `gen/apple` under `crates/keeper`
**Then** `gen/apple` is committed with `build/` gitignored, and persistent edits live only in `project.yml` (minimum deployment target iOS 16.0 set explicitly, theme-matched background color), `Info.plist` (`CFBundleURLTypes` for `keeper://`), and the `*_iOS/` sources — regenerating the `.xcodeproj` loses nothing (AD-32)
**And** the bundle identifier is the same as macOS, and signing uses `bundle.iOS.developmentTeam` or the `TAURI_APPLE_DEVELOPMENT_TEAM` env var so no team id ever lands in git.

**Given** the iOS Simulator on the dev machine (Xcode 16.x, CocoaPods, rust targets `aarch64-apple-ios{,-sim}` — prerequisites noted for docs/ios.md)
**When** `tauri ios dev` runs
**Then** the app opens in the Simulator showing the existing login screen — no physical device required (FR-55).

**Given** the desktop target
**When** the branch builds
**Then** desktop behavior is unchanged and `bun run check`, `bun run check:rust`, `bun run test:rust`, and `cargo deny check` all pass.

### Story 12.2: Desktop/Mobile Compile Seam and Capability Handshake

As a keeper developer,
I want desktop-only surfaces cfg-gated out of the iOS build and a single CapabilitiesVm served over the IPC handshake,
So that one shell crate serves both platforms and the UI never has to guess what exists.

**Requirements:** FR-56, FR-57 (mechanism); AD-26, AD-27, AD-7
**Dependencies:** 12.1

**Acceptance Criteria:**

**Given** the `crates/keeper` shell
**When** the seam is complete
**Then** the `tray` module + `tray-icon` cargo feature, global-shortcut, autostart, updater, window-state, and desktop deep-link registration sit behind `#[cfg(desktop)]`/`#[cfg(target_os)]` gates with target-gated Cargo dependencies, the iOS shell registers notification + mobile deep-link + IPC + media protocol only, clipboard needs are served by the web Clipboard API on iOS, and "open in browser" uses a minimal native open call (AD-26, FR-56)
**And** `cargo check --target aarch64-apple-ios` passes for the whole workspace locally, no in-app updater code path exists on iOS, and desktop build behavior is byte-identical (regression: desktop quality gates green).

**Given** the IPC handshake (AD-27)
**When** the frontend starts
**Then** a single `CapabilitiesVm` in `keeper-core::vm` (serde + ts-rs, camelCase, exported to `src/lib/ipc/gen/`) is served at startup into a `useCapabilitiesStore` zustand mirror, data-driven per platform so later targets reuse the mechanism (FR-57)
**And** `Platform::sidecar_path` returns a clean Unsupported `IpcError` on iOS, and no TypeScript consults `navigator.userAgent` or build flags for feature gating (convention test).

**Given** `keeper-core`
**Then** it remains free of `cfg(target_os)` in business logic — platform variance enters only through the `Platform` port (AD-26/AD-24).

### Story 12.3: iOS Platform Port — Keychain Spike and Data Directory

As a user,
I want my session tokens in the iOS keychain and keeper's data in its app container,
So that sessions survive relaunches and re-signs without ever leaving my device.

**Requirements:** FR-63; AD-29 (spike-first), NFR-9
**Dependencies:** 12.2

**Acceptance Criteria:**

**Given** the iOS branch of the `Platform` impl
**When** it lands
**Then** `data_dir()` resolves to the app container (Application Support) with all account state under that one root (future App Group move = path change, not migration), and the existing keyring/apple-native port targets the iOS keychain with `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` — readable by a resumed sync loop, invisible to other apps, excluded from iCloud Keychain (FR-63, AD-29).

**Given** the AD-29 spike
**When** keychain set/get/delete is exercised in the Simulator
**Then** the spike verdict is recorded: keep `keyring` as-is, or switch to the contained fallback — direct security-framework generic-password calls behind the **same** `Platform` port with call sites unchanged (on-device confirmation folds into Story 12.6).

**Given** an app relaunch in the Simulator
**Then** the session restores from keychain + SDK store without re-login, and no token or secret appears in logs or crosses IPC (NFR-9).

### Story 12.4: Media Protocol on WKURLSchemeHandler with Capped Buffers

As a user,
I want encrypted images and videos to render and seek on iPhone exactly as on the Mac,
So that media works without an iOS-specific transport fork or unbounded memory use.

**Requirements:** FR-64; AD-28, AD-4, NFR-9, NFR-16 (buffer cap)
**Dependencies:** 12.2

**Acceptance Criteria:**

**Given** the existing `keeper-media://` protocol
**When** it runs on iOS (wry → WKURLSchemeHandler)
**Then** the URL format is identical to macOS and the frontend needs no media-URL helper; an encrypted image renders in the timeline and a video plays **and seeks** in the Simulator — the Range 200/206/416 path exercised — with decrypted bytes never passing through IPC JSON (FR-64, NFR-9, AD-28).

**Given** the in-memory Range-slicing path
**When** large media streams
**Then** the slicing buffer is capped (named constant + unit test asserting the cap), WebKit scheme-task invalidation is tolerated by the fire-and-forget responder, and disk-backed streaming is recorded in the deferred-work ledger, not implemented (AD-28, NFR-16).

**Given** a force-quit with a cold media cache
**When** the app relaunches and the timeline re-renders
**Then** the retry-on-cache-miss path re-fetches and renders the media (FR-64).

### Story 12.5: iOS Compile Check in CI

As a maintainer,
I want every PR compile-checked against the iOS target,
So that desktop work can never silently break the port.

**Requirements:** FR-55 (CI leg); AD-32, AD-23
**Dependencies:** 12.2

**Acceptance Criteria:**

**Given** the existing GitHub Actions macOS runner
**When** the workflow lands
**Then** a job runs `cargo check --target aarch64-apple-ios` for the whole workspace on every PR — compile-only: no signing, no simulator build, no Apple credentials in CI (AD-32).

**Given** current main
**Then** the job is green, and a deliberately broken cfg seam on a scratch branch demonstrably fails it (spot-checked once, evidence in the PR).

**Given** phase sequencing
**Then** flipping this check to a *required* branch-protection status is left to Story 15.4 — the job itself exists and blocks by failure from this story onward.

### Story 12.6: On-Device Walking-Skeleton Validation (SM-7 Gate)

As the owner,
I want the whole vertical slice proven on my actual iPhone under free signing,
So that the three existential risks — toolchain, signing, core-on-iOS — are retired before phone-UX work begins.

**Requirements:** FR-55 (on-device gate), FR-63 (device leg); SM-7; AD-30 (resume smoke), NFR-18 (first exercise)
**Dependencies:** 12.1, 12.2, 12.3, 12.4, 12.5
**Human-in-the-loop:** **yes** — requires the owner's physical iPhone and free Personal Team signing (Developer Mode enabled, personal-team certificate trusted on device). The automation loop defers this story to the coordinator instead of escalating; all other Epic 12 stories are device-free.

**Acceptance Criteria:**

**Given** the owner's iPhone with Developer Mode enabled
**When** `tauri ios dev` deploys with Personal Team signing
**Then** the app installs, launches, and the certificate-trust flow is completed and its steps recorded for docs/ios.md (FR-55).

**Given** the SM-7 gate checklist
**When** executed on-device
**Then** OIDC login completes via the `keeper://` deep-link callback, the room list loads, text send/receive works in one E2EE Room, and app relaunch restores the session without re-login (FR-55, FR-63, SM-7).

**Given** lifecycle reality
**When** the app is backgrounded, foregrounded, and left suspended overnight
**Then** the resume behavior is exercised and any blank-webview occurrence (tauri#14371) is recorded as direct input to Story 14.4's guard — this is NFR-18's first acceptance exercise per AD-30 — and on-device media rendering (Story 12.4 paths) is spot-checked.

**Given** the 7-day profile expiry (validated later in the phase, noted here)
**Then** re-signing restores launch with all local data intact, confirming the stable bundle identifier.

**Epic 12 exit gate (SM-7):** all Story 12.6 ACs pass on-device. Phone-UX epics (13, 14) start only after this gate.

## Epic 13: iPhone Shell — Single-Pane Navigation

Project the desktop shell onto the phone tier: same components, same tokens, same IPC — one new arrangement container. Everything here is pure frontend plus small `gen/apple` glue, verifiable in the iOS Simulator or any < 768 px webview; no physical device required. Runs after the SM-7 gate, in parallel with Epic 14.

### Story 13.1: Phone Layout Tier and Navigation Stack

As an iPhone user,
I want keeper to render one pane at a time — Inbox, Room, Detail — as a navigation stack,
So that the desktop's three-pane product fits my phone without becoming a second app.

**Requirements:** FR-58; AD-31; UX-DR21, UX-DR22 (projection)
**Dependencies:** Epic 12 (SM-7 gate; capabilities plumbing from 12.2)

**Acceptance Criteria:**

**Given** `useShellLayout`
**When** the viewport is < 768 px
**Then** a third `phone` tier activates rendering a stack container — level 0 Inbox (Pins strip → FAVORITES → inbox, scoped by the active view/filter), level 1 Room (header → timeline → composer), level 2 Detail as a pushed page — while desktop and tablet tiers are unchanged at ≥ 768 px (regression-tested) (FR-58, AD-31).

**Given** the component trees
**When** the stack renders
**Then** it reuses the existing InboxList/ChatView/DetailPanel trees unchanged (no forked chat components), driven by the existing zustand selection state (`selectedRoomId`, detail-open); no routing library is added (`history.pushState` integration is an optional enhancer, not a dependency), and a deep link (notification payload `(account_id, room_id, event_id)`) sets selection state and renders at the right level with back leading to the Inbox (FR-58, AD-31).

**Given** navigation back to the Inbox
**Then** the Inbox scroll position is preserved, and opening a Chat on the phone does **not** auto-focus the composer (UX-DR22).

### Story 13.2: Phone Header, Push/Pop Transitions, and Edge-Swipe Back

As an iPhone user,
I want a real back affordance at every level — chevron, edge-swipe, or system gesture,
So that moving through the stack feels native, reversible, and accessible.

**Requirements:** FR-58 (back), FR-60 (edge-swipe); UX-DR21 (`phone-header`), UX-DR22, UX-DR28 (focus rules)
**Dependencies:** 13.1

**Acceptance Criteria:**

**Given** the 52 px `phone-header` bar
**When** a level renders
**Then** the back chevron carries the previous level's title ("Inbox", or the Chat name on Detail) with a full 44 pt hit area, and the Room header reads: back → avatar + network badge → name + Account chip → incognito chip when applicable → overflow (⋯) with Search in chat, Mute ▸, Mention-only, Incognito for this Chat, Archive, Export — tapping the identity block pushes Detail (replacing ⌘I) (UX-DR21/22, FR-58).

**Given** push and pop
**When** levels change
**Then** the new level slides in from the trailing edge over ~250 ms ease-out while the level beneath shifts back ~25 % and dims; pop reverses; reduced-motion renders cuts (UX-DR22).

**Given** the leading screen edge
**When** the user swipes from it at level ≥ 1
**Then** an interactive edge-swipe back tracks the finger, commits past 50 % travel or on a flick, and cancels otherwise (WKWebView grants no native swipe to an in-page stack); at level 0 the same edge is reserved for the drawer (FR-60, UX-DR22).

**Given** VoiceOver
**Then** every push moves focus to the new level's header (back button first in swipe order), every pop returns focus to the pushing element, and the escape gesture triggers the same back action at every level (UX-DR28).

### Story 13.3: Leading Drawer with Status Cluster

As an iPhone user,
I want the entire desktop sidebar reachable as a drawer, with the states that must never hide pinned to the Inbox header,
So that navigation chrome stays out of the way without hiding honesty.

**Requirements:** FR-58 (rail leg); UX-DR23; reuses UX-DR13/18
**Dependencies:** 13.1

**Acceptance Criteria:**

**Given** the Inbox header's top-leading avatar button
**When** tapped (or on edge-swipe from the leading edge at level 0 only)
**Then** the entire desktop sidebar renders verbatim inside a leading Sheet — primary views (Inbox / Archive / Approval Pane with amber count / Bridges with health roll-up), SPACES, NETWORKS chips with health dots, Account switcher footer with the settings gear, and sync/offline status — and selecting a view or filter closes the drawer and applies it, with the active filter chip above the chat list exactly as on desktop (UX-DR23, FR-58).

**Given** drawer dismissal
**When** the user taps the scrim, edge-swipes, or selects a row
**Then** the drawer closes and focus returns to the drawer button (UX-DR28).

**Given** the Inbox header status cluster
**When** state warrants it
**Then** the avatar button carries a worst-state bridge-health dot overlay and the Account-filter state, an amber Approval chip shows the pending-Draft count whenever > 0 and deep-links to the Approval Pane, and magnifier + compose buttons trail — the header is quiet when everything is healthy, and **no bottom tab bar exists anywhere** (UX-DR23).

### Story 13.4: Merged Full-Screen Search Surface

As an iPhone user,
I want one Search surface covering chats, messages, and actions,
So that ⌘K and global search survive the trip to a keyboard-less device.

**Requirements:** FR-48/FR-34 (parity on phone), FR-58 (palette mapping), FR-60 (pull-down); UX-DR24
**Dependencies:** 13.1, 13.3 (header magnifier)

**Acceptance Criteria:**

**Given** the Search surface
**When** opened via the header magnifier or a short pull-down on the Inbox list (pull past the reveal threshold becomes pull-to-refresh — one continuous axis, spinner beyond the field)
**Then** it renders full-screen with segmented scopes: **Chats** (fuzzy chats/contacts across Accounts, network badge + account hue dot per row), **Messages** (offline FTS with the same filter chips, results deep-linking into timelines at the match), **Actions** (the full context-aware action registry) — all on the same engines and the same ≤ 100 ms / offline bars (FR-48, FR-34, UX-DR24).

**Given** desktop muscle memory
**When** `>` is typed as the first character
**Then** the surface jumps to Actions scope; in-chat search maps to Room overflow → "Search in chat", opening Messages pre-filtered to the open Chat (UX-DR24).

**Given** the parity release gate
**Then** every registered action is reachable from Actions scope on the phone, and desktop-only actions are unregistered via capabilities so no dead entries appear (FR-48, FR-57).

### Story 13.5: Safe Areas and the Keyboard-Avoiding Composer

As an iPhone user,
I want edge-to-edge rendering that respects the notch and a composer the keyboard can never cover,
So that typing on the phone feels solid instead of glitchy.

**Requirements:** FR-59; AD-31 (glue), AD-32 (`gen/apple` patch); UX-DR21, UX-DR25
**Dependencies:** 13.1, 13.2 (header insets)

**Acceptance Criteria:**

**Given** the iOS webview
**When** the app renders
**Then** `viewport-fit=cover` is set, `contentInsetAdjustmentBehavior = .never` is pinned via the committed `gen/apple` Swift patch, and `env(safe-area-inset-*)` values are exposed as theme CSS vars padding the header, composer, drawer, sheets, and overlays in portrait and landscape — no unstyled bands at the notch or home indicator, and the window/launch background matches the active theme with no flash on launch or rotation (FR-59, AD-31/32).

**Given** the on-screen keyboard
**When** it opens and closes
**Then** the composer sits at `bottom: calc(var(--kb-inset, 0px) + env(safe-area-inset-bottom))` with `--kb-inset` driven by `visualViewport` listeners (`interactive-widget=resizes-content` evaluated as the simpler path, decision recorded), a timeline already at bottom stays pinned to bottom, and dismissal restores layout with no stranded offsets or overshoot; the timeline scroller uses `overscroll-behavior: contain` (FR-59, UX-DR21).

**Given** the phone composer deltas
**Then** a ≥ 44 pt primary-tinted send button trails the field (tap = FR-41 approval trigger #1), the on-screen return key inserts a newline while a hardware keyboard follows the desktop Enter setting, autogrow caps at 5 lines then scrolls, attach goes via + → system photo library/camera/Files, and the undo-send pill floats above the composer with tap replacing ⌘⇧Z (UX-DR25).

### Story 13.6: Touch Idioms — Long-Press, Row Swipes, Pull-to-Refresh

As an iPhone user,
I want every desktop action reachable by touch with proper iOS idioms,
So that triage on the phone is as complete as at the desk.

**Requirements:** FR-60; UX-DR26, UX-DR28 (gesture alternatives)
**Dependencies:** 13.1, 13.4 (shared pull axis), 13.5 (scroll interplay)

**Acceptance Criteria:**

**Given** any surface with a desktop context menu
**When** the user long-presses (rows, message bubbles, pins)
**Then** the identical ContextMenu opens — the bubble menu leading with the emoji React row, then Reply, Edit (own), Delete ▸, Copy, Jump-to-original — with `-webkit-touch-callout`/tap-highlight suppressed where custom menus exist, and long-press-drag reordering the Pins strip (FR-60, UX-DR26).

**Given** inbox and Approval rows
**When** the user swipes
**Then** trailing swipe reveals Archive + More (mute ▸) and leading swipe toggles read/unread, styled per the `swipe-action` tokens with the label appearing past the half-swipe commit threshold and full-swipe committing the first action; Approval rows get row-tap → inline editor, an explicit per-row Approve button ≥ 44 pt, and trailing swipe → Discard with the 5 s undo toast — still no approve-all (FR-60, FR-41, UX-DR26).

**Given** pull-to-refresh on the Inbox
**When** pulled past the search-reveal threshold
**Then** it visibly kicks the sync loop (the same operation as foreground resume); offline, the spinner resolves into the persistent offline pill, never an error toast (FR-60, UX-DR28).

**Given** the accessibility hard rule
**Then** every tappable is ≥ 44 pt (icon buttons padded regardless of glyph), row swipe actions are duplicated as VoiceOver custom actions and in the long-press menu, and no gesture is the sole path to any action (UX-DR28, NFR-14).

### Story 13.7: Capability-Gated Surfaces and "On this iPhone" Disclosure

As an iPhone user,
I want unsupported features removed — not broken — and one honest list of what this device can't do,
So that iOS limits read as facts, never as bugs.

**Requirements:** FR-56 (surface leg), FR-57; AD-27; UX-DR27
**Dependencies:** 13.1, 13.3 (drawer/settings surfaces)

**Acceptance Criteria:**

**Given** capabilities off on iOS
**When** the UI renders
**Then** the bbctl "Run your own bridge" panel, the Shortcuts/global-hotkey settings section, updater controls, tray/menu-bar + launch-at-login options, and the hotkey cheat sheet do not render at all — no dead buttons, no error-on-tap — while bridge management stays fully functional: discovery, native provisioning login, Bridge Bot fallback, health + re-login, risk tiers, start-new-Chat (FR-57, FR-25–FR-28/FR-30–FR-32 unchanged).

**Given** the gating mechanism
**Then** all hiding flows exclusively from the capabilities store (never platform sniffing — convention test), desktop renders unchanged with all capabilities on, desktop-only palette actions are unregistered on iOS so Actions scope simply lacks them, and programmatic reach of a disabled capability returns the clean "unsupported on this platform" `IpcError` (FR-57, AD-27).

**Given** Settings → About on iOS
**When** rendered
**Then** an "On this iPhone" rendered list states: syncs and notifies only while open (background notifications await an explicit future decision), no self-hosted bridge runner (manage from your Mac), no global hotkey, updates arrive by reinstall with a signature renewing every 7 days — plus a link to docs/ios.md; Settings → Archive & Storage adds the line that the phone's Local Archive is excluded from device backup and the Mac remains the durable, exportable copy this phase (UX-DR27, FR-65 disclosure).

## Epic 14: iOS Platform Behavior

Make keeper an honest iOS citizen: sync pauses and resumes through one Rust entry, nothing pretends to be push, notifications and the badge work in the foreground, and the phase's reliability bars — resume integrity, jetsam hygiene, flaky-network recovery, backup posture — are engineered in, not hoped for. Runs after the SM-7 gate, in parallel with Epic 13; automated/simulator verification wherever possible, with on-device soaks folded into SM-8 dogfooding rather than blocking stories.

### Story 14.1: Lifecycle Pause/Resume Through One Rust Entry

As an iPhone user,
I want sync to stop cleanly when I leave and pick up instantly when I return,
So that keeper behaves like the OS expects while never showing me stale confusion.

**Requirements:** FR-61 (mechanics); AD-30
**Dependencies:** Epic 12 (SM-7 gate)

**Acceptance Criteria:**

**Given** the app backgrounds
**When** the lifecycle signal fires
**Then** detection enters Rust through **one** lifecycle command in the shell (`lifecycle.rs`) — webview `visibilitychange` as the zero-native stopgap, with the micro Swift plugin on `UIApplication` notifications recorded as the upgrade path behind the same Rust entry — and `SyncService` pauses gracefully (stop/offline mode) within seconds instead of letting the sliding-sync long-poll die mid-flight (FR-61, AD-30).

**Given** the app foregrounds
**When** the same entry fires resume
**Then** cached state renders instantly from the zustand mirrors (snapshot-then-diff), an immediate sync kicks, and new messages appear within 2 s on Wi-Fi (Simulator-verifiable) (FR-61, NFR-17).

**Given** pull-to-refresh (Story 13.6)
**Then** it converges on the same sync-kick operation as foreground resume — one code path, no second lifecycle truth (AD-30).

### Story 14.2: Honest No-Background-Sync Disclosure

As an iPhone user,
I want keeper to tell me plainly that it only works while open,
So that missed messages while closed read as physics, not betrayal.

**Requirements:** FR-61 (copy leg); UX-DR27, UX-DR10/17
**Dependencies:** 14.1

**Acceptance Criteria:**

**Given** the iOS first run (Wizard Done step, or first Inbox render for an existing Account)
**When** the disclosure shows
**Then** a one-time card states: "On iPhone, keeper syncs and notifies only while open. Close it and messages wait on your homeserver until you return — nothing is lost, and nothing here pretends to be push." — voice rules applied, shown once, and the same copy lives permanently in Settings → Notifications (FR-61, UX-DR27).

**Given** the whole iOS surface
**When** audited (copy-string sweep)
**Then** no surface anywhere implies background delivery — extending FR-53's honesty rule — and the badge copy notes it is not live while suspended (FR-61, FR-62).

**Given** docs
**Then** docs/ios.md's limitations section (Story 15.2) matches this copy one-to-one, so app and docs never diverge on the promise.

### Story 14.3: Foreground Notifications and the All-Accounts Badge

As an iPhone user,
I want notifications while I'm in the app and a truthful home-screen badge,
So that the phone's attention surfaces work exactly as far as iOS allows.

**Requirements:** FR-62; AD-30, AD-18 (reuse)
**Dependencies:** 14.1

**Acceptance Criteria:**

**Given** new messages while the app is active
**When** the notification rules engine evaluates them
**Then** local notifications post via the notification plugin with the same content, preview toggle, and mute/mention-only semantics as FR-51/FR-52, and notifications for the currently visible Chat are suppressed by the reused desktop logic (FR-62, AD-18).

**Given** the app icon badge
**When** sync completes or the app foregrounds
**Then** the badge equals the Unified Inbox unread aggregate across all Accounts — sourced from `inbox` (AD-20), never a second count — and refreshes on foreground resume without pretending to be live while suspended (FR-62, AD-30).

**Given** notification permission denied at the OS level
**When** Settings → Notifications renders
**Then** a persistent inline state says "Notifications are off for keeper in iOS Settings." with an Open Settings deep link, notes the badge needs the same permission, and never re-prompts on its own (UX-DR28).

### Story 14.4: Resume Integrity — Blank-Webview Guard and Stale-Resume Pill

As an iPhone user,
I want keeper to come back alive every single time I return to it,
So that an overnight suspension never greets me with a blank screen or stale silence.

**Requirements:** NFR-18, NFR-17 (restart guard); AD-30
**Dependencies:** 14.1 (findings from 12.6 feed in)

**Acceptance Criteria:**

**Given** a jettisoned or unresponsive webview process on resume (tauri#14371)
**When** the app foregrounds
**Then** the reload guard detects it and restores the UI to the last stack level from cached state — never a blank or unresponsive screen — with the guard covered by an automated test wherever process termination can be simulated, the upstream fix tracked, and Story 12.6's on-device findings incorporated (NFR-18, AD-30).

**Given** a stale resume (last sync minutes old)
**When** the app foregrounds
**Then** cached UI renders at once, sync kicks immediately, a quiet "Connecting…" pill shows under the Inbox header and clears on the first sync response, and the sync-loop restart guard handles the known stale-session edge (matrix-rust-sdk#3935) (NFR-17, UX-DR28).

**Given** SM-8 dogfooding
**Then** the overnight-suspension scenario sits on the dogfooding checklist with findings ledgered — the guard is acceptance-tested continuously from here on (NFR-18).

### Story 14.5: Memory Hygiene Under Jetsam

As an iPhone user,
I want keeper to shed weight when backgrounded,
So that iOS doesn't kill it while suspended and my session survives the day.

**Requirements:** NFR-16; AD-28, AD-30
**Dependencies:** 14.1, 12.4 (buffer cap)

**Acceptance Criteria:**

**Given** `didEnterBackground` or a memory warning
**When** the signal reaches the shell
**Then** droppable caches — the image memory cache and media byte buffers — are released, with automated tests asserting the drop hooks fire, and memory returns near baseline after backgrounding (Instruments-verified on Simulator) (NFR-16).

**Given** large-media playback
**When** the Range-slicing path streams
**Then** the Story 12.4 buffer cap holds under sustained seeking (no unbounded growth in the memory graph), and disk-backed streaming of large video remains a deferred-work ledger entry (NFR-16, AD-28).

**Given** the 24 h suspended soak with a large account
**Then** it is executed as part of SM-8 on-device dogfooding (not a story-blocking device step) with the outcome recorded — survival without a jetsam kill is the bar (NFR-16).

### Story 14.6: Flaky-Network Resilience

As an iPhone user,
I want keeper to shrug off airplane mode, dead spots, and Wi-Fi-to-cellular hops,
So that mobile networking never costs me a message or a restart.

**Requirements:** NFR-17; AD-30, AD-8
**Dependencies:** 14.1, 14.4

**Acceptance Criteria:**

**Given** connectivity loss and restoration (simulated via Network Link Conditioner / link toggling in the test environment)
**When** the network flaps
**Then** the sync loop enters SSS offline mode with backoff and exits it immediately on demand (foreground resume or pull-to-refresh), the UI keeps rendering instantly from the local mirror throughout, the offline pill appears and clears with no toast spam, and recovery needs no app restart and never blanks the UI (NFR-17, AD-30/AD-8).

**Given** messages sent while disconnected
**When** the app is backgrounded before dispatch
**Then** they carry the caption "Queued — sends when keeper is open and back online" and dispatch on foreground reconnect (an already-elapsed undo window dispatches immediately) — NFR-5's no-silent-loss promise extended to iOS (UX-DR28).

**Given** on-device network scenarios (airplane-mode toggle, real Wi-Fi↔cellular handover)
**Then** they sit on the SM-8 dogfooding checklist with unaided recovery as the bar, findings ledgered (NFR-17).

### Story 14.7: Backup Exclusion and File Protection

As an iPhone user,
I want keeper's re-syncable gigabytes out of my backups and encrypted at rest without breaking sync,
So that the storage posture is deliberate on iOS, not accidental.

**Requirements:** FR-65; AD-29
**Dependencies:** Epic 12 (12.3 data-dir root)

**Acceptance Criteria:**

**Given** keeper's database directories (SDK stores, `keeper.db`, `archive.db`)
**When** they are created or opened on iOS
**Then** each carries the `isExcludedFromBackup` resource flag — verified by reading the resource value back in a test — so multi-gigabyte re-syncable state never bloats device/iCloud backups (FR-65, AD-29).

**Given** file protection
**When** the stores are provisioned
**Then** they use `NSFileProtectionCompleteUntilFirstUserAuthentication` — never `Complete` — so WAL access keeps a resumed sync loop working after screen lock; the class is asserted in code and lock-screen behavior is validated in SM-8 dogfooding (FR-65, AD-29).

**Given** the layout invariant
**Then** all account state remains under the one `Platform::data_dir()` root (the future App Group move stays a path change), and the Story 13.7 Archive & Storage disclosure matches the actual flagging — the phone archive is excluded from backup; the Mac remains the durable, exportable copy (FR-65).

## Epic 15: iOS Polish & Release

Ship the phase like the product it is: real icons and a flash-free launch, a signing walkthrough that makes the 7-day ritual cheap, a shareable IPA path, the CI gate made mandatory, the paid-program question answered on the record — and the final on-device acceptance that starts the SM-8 dogfooding clock.

### Story 15.1: App Icons and Launch Assets

As an iPhone user,
I want keeper to look like keeper from the home screen to first paint,
So that the phone build reads as finished, not sideloaded scaffolding.

**Requirements:** FR-55 (assets), FR-59 (no-flash); AD-32
**Dependencies:** Epic 12

**Acceptance Criteria:**

**Given** the iOS icon set and launch configuration
**When** they land in `gen/apple`
**Then** the full icon set renders on the home screen, Settings, and the app switcher, and the launch screen/window background matches the active theme in light and dark — no white/black flash on launch or rotation (FR-55, FR-59).

**Given** AD-32's regeneration rule
**Then** assets and launch config survive `.xcodeproj` regeneration — persistent edits only in `project.yml` and committed asset catalogs (AD-32).

**Given** the desktop build
**Then** it is unaffected (icons/bundling unchanged; quality gates green).

### Story 15.2: docs/ios.md — Free Signing Walkthrough

As the owner (and any hand-provisioned tester),
I want one document that takes a Mac and an iPhone to a running keeper and keeps it running,
So that the 7-day re-arm ritual costs minutes, not archaeology.

**Requirements:** FR-55 (docs leg); PRD §13.5 posture; SM-8 support
**Dependencies:** 12.6 (validated flow to document)

**Acceptance Criteria:**

**Given** `docs/ios.md`
**When** written
**Then** it covers: toolchain prerequisites (Xcode 16.x, CocoaPods, rust targets), Personal Team setup (`bundle.iOS.developmentTeam`/`TAURI_APPLE_DEVELOPMENT_TEAM`, Developer Mode, on-device certificate trust — as validated in 12.6), the **7-day re-arm ritual** with its expected per-week cost in minutes, the AltServer auto-refresh option, and the **Sideloadly/zsign re-sign alternative** for installing shared test IPAs without Xcode (FR-55, PRD §13.5).

**Given** the limitations section
**Then** it lists the platform limits one-to-one with the "On this iPhone" disclosure (no push/background sync, no bbctl, no global hotkey, reinstall updates, archive-backup posture) so app and docs never diverge (UX-DR27, Story 14.2 tie-in).

**Given** repo rules
**Then** the doc is English, honest in tone per the voice rules, and contains no team ids, credentials, or secrets.

### Story 15.3: Shareable IPA Build Path — Unsigned Export for Re-Signing

As a maintainer,
I want a repeatable build that produces an IPA anyone can re-sign,
So that hand-provisioned testers can install keeper without my Mac in the loop.

**Requirements:** FR-55/FR-56 (distribution posture); AD-32
**Dependencies:** Epic 12, 15.2 (doc integration)

**Acceptance Criteria:**

**Given** the documented build command/script
**When** it runs on the dev machine
**Then** it produces a release-configuration IPA suitable for per-tester re-signing via Sideloadly/zsign (unsigned export, or dev-signed with signature replacement documented), with the exact re-sign steps appended to docs/ios.md (FR-55).

**Given** the artifact
**Then** it contains no desktop-only plugin symbols (FR-56 seam verified against the shipped binary) and no signing material, team ids, or provisioning profiles land in the repo or CI (AD-32).

**Given** validation
**Then** a re-signed install is verified on a device as part of Story 15.6's checklist (the build path itself is device-free).

### Story 15.4: Required iOS CI Gate and Release Hygiene

As a maintainer,
I want the iOS compile check promoted to a required gate and the release checklist to know iOS exists,
So that the port's integrity is enforced, not remembered.

**Requirements:** FR-55 (CI required); AD-32, AD-23
**Dependencies:** 12.5

**Acceptance Criteria:**

**Given** the Story 12.5 `cargo check --target aarch64-apple-ios` job
**When** this story completes
**Then** it is wired as a **required** PR status (branch-protection/merge-queue configuration recorded in the repo docs), remaining compile-only — no signing, no simulator — so PR latency stays acceptable (AD-32).

**Given** the release checklist (Epic 11's process)
**When** updated
**Then** it gains the iOS items: IPA build path exercised (15.3), docs/ios.md current (15.2), NFR-15 measurement recorded with its owner-confirmation status (PRD §13.8), and the egress posture note that iOS adds no new endpoints (NFR-11 unchanged).

**Given** CI docs
**Then** contributor documentation states the gate's scope and how to reproduce it locally (`cargo check --target aarch64-apple-ios`).

### Story 15.5: Paid-Program Decision Gate Recorded

As the product owner,
I want the $99 Apple Developer Program question answered on the record as a deliberate deferral,
So that push, TestFlight, and the NSE are a decision waiting for a trigger — never an omission.

**Requirements:** PRD §13.5; spine Deferred items
**Dependencies:** Epic 12 (phase reality to record against)

**Acceptance Criteria:**

**Given** the decisions ledger
**When** the record lands
**Then** it captures: what the paid program uniquely unlocks (APNs push, the NSE — with its 24 MB memory ceiling and App-Group store-layout implications — TestFlight, App Groups, AltStore PAL notarization), the opening trigger (push becomes a product goal), and the PRD-level constraint it will force — push must ride a homeserver operator's gateway, Beeper's, or a user-run Sygnal, never project infrastructure (NFR-11) (PRD §13.5).

**Given** the cheap-now mitigations
**Then** the record notes what this phase already paid for: the single `data_dir()` root making the App Group move a path change (AD-29), and Plan B's revisit triggers staying recorded (PRD §13.8).

**Given** scope discipline
**Then** this story changes no code — it is a decision record with PRD/architecture cross-references, closing the phase's single deliberate deferral.

### Story 15.6: Final Device Install and Phase Acceptance

As the owner,
I want the release build on my iPhone through the documented path, measured and accepted,
So that SM-8 dogfooding starts on evidence and the phase retrospective has its inputs.

**Requirements:** SM-8 (start), NFR-15 (measure), FR-55; retrospective inputs
**Dependencies:** 15.1, 15.2, 15.3, 15.4 (+ Epics 13 and 14 complete)
**Human-in-the-loop:** **yes** — requires the owner's physical iPhone (install via free Personal Team signing or a re-signed IPA per docs/ios.md). The automation loop defers this story to the coordinator instead of escalating; it is the phase's second and final device step.

**Acceptance Criteria:**

**Given** the release build and docs/ios.md
**When** the owner installs on their iPhone via the documented path (including one install through the Sideloadly re-sign flow to validate 15.3)
**Then** the app launches with final icons and a flash-free launch, and an on-device spot-check passes the Epic 13/14 surface: safe areas at the notch and home indicator, keyboard avoidance, stack navigation and gestures, drawer and Search, foreground notifications and badge, lifecycle pause/resume, resume after overnight suspension.

**Given** NFR-15
**When** cold start is measured on-device (release build, real accounts, cached inbox)
**Then** the launch → interactive Unified Inbox time is recorded, and the owner confirms or adjusts the 3 s bar — resolving PRD §13.8 open question 1 before the bar becomes release-gating (NFR-15).

**Given** SM-8
**When** acceptance passes
**Then** the two-week phone daily-driver window opens with its checklist: 7-day re-arm cost tracked in minutes, the 24 h jetsam soak (14.5), airplane/handover scenarios (14.6), lock-screen store access (14.7), and a zero-silent-loss watch (NFR-5 extended to iOS).

**Given** the phase retrospective
**Then** its inputs are recorded: outcomes against the §13.7 risk register (blank webview, keyring, keyboard quirks, 7-day friction, media RAM), the deferred-work ledger entries opened this phase (disk-backed streaming, micro Swift lifecycle plugin, Dynamic Type), and the pointer to the 15.5 paid-program gate.

**Epic 15 exit:** phase accepted on-device; SM-8 window running; retrospective inputs on file.

## Epic 16: Recording Walking Skeleton — Sidecar, Permissions, Capture to File

Prove the whole recording vertical slice on the locked architecture before any feature breadth: the `keeper-rec` Swift sidecar built, codesigned, and bundled; a platform-free `keeper-core::recording` state machine behind a `Recorder` port; the `recording` capability flag; the NDJSON-RPC handshake; an honest Screen Recording permission pre-flight; and a full display + system audio captured to a single fMP4 from a ⌘5 Recording view. Exit (R.1 / SM-9 seed): a real recording plays back. This epic retires the existential risks — TCC, sidecar signing, capture-to-file — that PRD §14.1 and research §8 name first.

### Story 16.1: keeper-rec SwiftPM Scaffold, Codesign & externalBin Wiring

As a keeper developer,
I want the `keeper-rec` Swift capture sidecar built, codesigned, and bundled as a Tauri `externalBin` from CI,
So that every later recording story spawns a real signed capture binary instead of a stub.

**Requirements:** AD-38, AD-34 (package shape); NFR-13 (licensing firewall); FR-66 (build/toolchain)
**Dependencies:** none

**Acceptance Criteria:**

**Given** the recording route locked to a first-party Swift sidecar (addendum §8)
**When** the scaffold lands
**Then** a SwiftPM package lives at top-level `tools/keeper-rec/` (`Package.swift` + `Sources/keeper-rec/`), deliberately outside `src-tauri/crates/` so Cargo and SwiftPM tooling don't collide, is Apache-2.0, and links only Apple system frameworks (ScreenCaptureKit/AVFoundation) — no ffmpeg, so `cargo deny check` (AD-5) is untouched (AD-38).

**Given** CI on the existing macOS signing runner
**When** the build job runs
**Then** it does `swift build -c release --arch arm64`, then **explicitly codesigns** `keeper-rec` (hardened runtime + keeper's entitlements) **before** `tauri build` (the `externalBin` notarization rough edge, tauri#11992), aarch64-only with no lipo, and `bundle.externalBin` is declared as `binaries/keeper-rec` so the bundled/runtime name resolves to `keeper-rec-aarch64-apple-darwin` matching `DesktopPlatform::sidecar_path` (AD-38).

**Given** the minimal binary
**Then** `keeper-rec` answers a `getCapabilities` line on stdio and exits cleanly, and the dev-signing requirement (local builds exercising recording need an Apple Development certificate — macOS 15+ silently rejects ad-hoc SCK, Cap #1722) is captured in a code comment and the release/dev docs as a DevEx note, explicitly not a product blocker (AD-38).

### Story 16.2: recording Core Module & Recorder Port

As a keeper developer,
I want a platform-free `keeper-core::recording` state machine and a `Recorder` shell port,
So that recording logic sits on the hexagonal seam and a capture crash can never reach the messaging core.

**Requirements:** AD-33, AD-24, AD-21
**Dependencies:** 16.1

**Acceptance Criteria:**

**Given** the `keeper-core` platform-free rule (AD-6/AD-24)
**When** the module lands
**Then** `keeper-core::recording` owns the session state machine (`idle → preflight → recording → rotating → stopping → finalized | recovered | failed`) with **no `tauri` and no Apple API** anywhere in its tree (enforced by a `cargo tree`/unit-test check), and errors flow `thiserror` → `CoreError` per AD-21 (AD-33).

**Given** the port seam
**When** the shell wires recording
**Then** a `Recorder` trait sits beside `Platform` (AD-24): the macOS impl (`crates/keeper/src/recorder.rs`, `#[cfg(desktop)]`) spawns `keeper-rec` via `Platform::sidecar_path`, and every non-macOS impl and iOS returns `CoreError::Unsupported` (mirroring `sidecar_path` honesty, AD-27) (AD-33).

**Given** the isolation invariant
**Then** the core state machine never holds a process handle — the port parses sidecar events and feeds them in — and a unit test drives the state machine through a full lifecycle with a fake `Recorder` (AD-33).

### Story 16.3: recording Capability Flag & Gated Recording Surface

As a user on an unsupported platform,
I want no recording affordance to appear at all,
So that recording is absent, never a dead button.

**Requirements:** FR-66, AD-35, AD-27, AD-7; UX-DR29 (shell)
**Dependencies:** 16.2

**Acceptance Criteria:**

**Given** `CapabilitiesVm` served over the IPC handshake (AD-27)
**When** it is computed
**Then** it gains a `recording` flag (serde + ts-rs, AD-7) that is **true only on desktop macOS ≥ 13.0** (the system-audio floor); the app-wide `minimumSystemVersion` stays 11.0; iOS never (AD-35).

**Given** the recording surfaces
**When** the app renders
**Then** the ⌘5 Recording sidebar entry, the Settings → Recording section, and the Command Palette recording actions render **only when the flag is on** (AD-27 "no dead buttons"), and the frontend never consults `navigator.userAgent`/build flags (AD-35).

**Given** the flag on
**Then** an empty Recording view shell renders behind ⌘5 — the centered `content-max-width` setup card stack (Source/Audio/Webcam/Destination/Segmenting/Advanced placeholders), no timeline or composer — ready for later stories (UX-DR29).

### Story 16.4: NDJSON-RPC Handshake — getCapabilities & listSources

As a keeper developer,
I want the host↔sidecar wire protocol with a version handshake and source enumeration,
So that keeper and `keeper-rec` never drift and the UI can list capture sources.

**Requirements:** AD-34, AD-7
**Dependencies:** 16.1, 16.2

**Acceptance Criteria:**

**Given** the NDJSON-RPC contract (AD-34)
**When** host and sidecar communicate
**Then** the wire format is **one JSON object per line on stdio**, `getCapabilities` (host→rec, id-correlated) returns macOS version + feature flags + per-TCC permission states **and carries the protocol-version handshake**, and a version mismatch yields a clean `Unsupported`/error surface, never a crash (AD-34).

**Given** source enumeration
**When** `listSources` is invoked
**Then** it returns displays, applications, microphones, and cameras, surfaced as ts-rs VMs (AD-7); the contract *shape* is the invariant while exact field lists stay code-owned (AD-34).

**Given** the parser
**Then** stdio framing and event parsing (`state`, `segmentClosed`, `error`) have unit tests against a recorded fixture stream, so the protocol is testable without a live signed capture (AD-34).

### Story 16.5: Screen Recording Permission Pre-flight

As a user,
I want keeper to show the true Screen Recording permission state and route me to fix it,
So that I never hit a silent grant failure or a spinner waiting on a grant that will never come.

**Requirements:** FR-67 (Screen Recording leg), AD-36; UX-DR33
**Dependencies:** 16.3, 16.4

**Acceptance Criteria:**

**Given** the pre-flight through the `Recorder` port → `keeper-rec` `getCapabilities` probe
**When** the Recording setup renders
**Then** a `RecordingPermissionVm` (`keeper-core::vm`, ts-rs) tracks the Screen Recording class distinctly as granted / not-yet-requested / denied-with-fix-path, **detected at render time via `CGPreflightScreenCaptureAccess`, never cached optimistically**, and re-detected on focus/return (FR-67, AD-36).

**Given** a missing grant
**When** the user acts
**Then** keeper requests via `CGRequestScreenCaptureAccess` where the OS allows (one real prompt per app lifetime) and deep-links to `x-apple.systempreferences:…Privacy_ScreenCapture` when only manual granting remains, with honest `note-line`s stating the macOS quirks plainly — relaunch-may-be-needed, macOS 15+ monthly re-confirm, and the subtle dev-facing "ad-hoc dev builds may be blocked on macOS 15+ — sign with an Apple Development certificate" (FR-67, UX-DR33).

**Given** an ungranted permission
**Then** Start is disabled with the blocking permission named (FR-67); the sidecar is spawned (never a LaunchAgent) so TCC attributes the child to keeper, using keeper's usage strings (AD-36). *(Real grant validation on hardware rides Story 16.6.)*

### Story 16.6: Full-Screen + System-Audio Capture to a Single fMP4

As a user,
I want to record a full display with its audio to a video file and stop it cleanly,
So that keeper proves it can capture end to end.

**Requirements:** FR-68 (full-screen leg), FR-69 (system-audio leg), FR-71 (single-file leg); AD-37 (format), AD-34 (start/stop); UX-DR29, UX-DR30
**Dependencies:** 16.5
**Human-in-the-loop:** **yes** — requires a physical Mac, a real Screen Recording grant, and an **Apple Development-signed build** (macOS 15+ silently rejects ad-hoc ScreenCaptureKit, Cap #1722). This is the recording phase's first device step; the automation loop defers it to the coordinator instead of escalating.

**Acceptance Criteria:**

**Given** a granted Screen Recording permission and a dev-signed build
**When** the user sets Source = a full display with system audio on and presses Start
**Then** `keeper-rec` builds an `SCContentFilter` over `SCShareableContent`, captures with `capturesAudio` + `excludeCurrentProcessAudio = true`, and writes a **single fragmented MP4** (H.264 + one AAC system-audio track, ~4 s fragments) to the chosen folder; the Recording view flips to *active* with a `recording-red` record dot and a `mono` elapsed line ticking, and macOS posts its own purple pill in parallel (FR-68/FR-69, AD-37, UX-DR29/UX-DR30).

**Given** an active recording
**When** the user presses Stop
**Then** the current file finalizes (defragmenting to an ordinary `.mp4`) and plays back in QuickTime with continuous audio and video, and keeper's own notification sounds are absent from the audio (FR-69, AD-37).

**Given** the walking-skeleton exit (R.1 / SM-9 seed)
**Then** the full cycle — pre-flight → full-screen + system-audio capture → single playable fMP4 in the chosen folder → clean Stop — runs on a Development-signed build on macOS 13+ hardware.

**Epic 16 exit gate (R.1 / SM-9 seed):** a real recording plays back on dev-signed hardware.

## Epic 17: Segmentation & Recovery — Hours-Long, Crash-Safe Capture

Turn the single-file skeleton into hours-long, crash-safe capture: gapless size-based rotation in the sidecar, a per-session folder with an atomic `manifest.json` and a segment ledger in `keeper-core`, startup recovery of orphaned segments, the automated concatenate-and-assert gate for gaplessness (NFR-22), and the segment-size/duration-cap settings. This is research R.2.

### Story 17.1: Dual-Writer Gapless Size-Based Rotation in keeper-rec

As a user,
I want long recordings to roll over into new files without a hiccup,
So that a crash costs at most a few seconds and files stay a manageable size.

**Requirements:** FR-72 (rotation), AD-37, NFR-22 (mechanics)
**Dependencies:** Epic 16

**Acceptance Criteria:**

**Given** the dual-AVAssetWriter mechanism (addendum §8, AD-37)
**When** the current segment reaches the configured size
**Then** `keeper-rec` starts writer B at the next keyframe PTS, routes to both writers until B's first keyframe lands, and finalizes A asynchronously — a **gapless handover** with no pause, no dropped audio, and no user-visible hiccup; the size trigger is a bytes-budget deadline corrected against observable on-disk growth, with a duration-cap fallback so low-motion recordings still rotate (FR-72, AD-37).

**Given** each rotation
**When** a segment closes
**Then** `keeper-rec` emits a `segmentClosed{path, bytes, track}` event, and all PTS are host-clock-anchored so timestamps stay continuous across the cut (NFR-22).

**Given** the container
**Then** it stays fragmented MP4 (`.mpeg4CMAFCompliant`, ~4 s fragments) throughout rotation so size is observable live and a mid-segment kill loses at most the last fragment (AD-37).

### Story 17.2: Session Folder, manifest.json & Segment Ledger

As a user,
I want every recording to produce one self-describing folder,
So that an external tool — or keeper's recovery — can always read a consistent picture of the session.

**Requirements:** FR-71 (session folder/manifest leg), AD-37, AD-33
**Dependencies:** 17.1

**Acceptance Criteria:**

**Given** a started recording
**When** the session is created
**Then** `keeper-core::recording` creates `<folder>/keeper-rec <local timestamp>/` and owns a `manifest.json` describing capture target, devices, segment list, and status, plus a segment ledger fed from the sidecar's `segmentClosed`/`state` events (FR-71, AD-33).

**Given** any segment close or status change
**When** the manifest updates
**Then** it is written by **atomic rename** so an external reader never sees a torn file, segment names are local-time-stamped, filesystem-safe, and lexicographically ordered, and the status transitions `recording → finalized` on clean Stop (FR-71, AD-37).

**Given** a cleanly finalized session
**Then** its segments are ordinary `.mp4` (H.264 + AAC) playable everywhere with no keeper-specific tooling (FR-71).

### Story 17.3: Startup Recovery of Orphaned Segments

As a user,
I want a crashed recording's segments to be found and marked, not lost,
So that an interruption costs the tail fragment, never the meeting.

**Requirements:** FR-73 (recovery leg), AD-37
**Dependencies:** 17.2

**Acceptance Criteria:**

**Given** an unfinalized session (stale `recording` manifest) from a recorder/keeper/power crash
**When** keeper starts up or is about to begin a new recording
**Then** a recovery pass reconciles orphaned segments: the stale manifest is marked `recovered`, the orphaned tail fMP4 plays as-is with no remux, and every earlier segment is untouched (FR-73, AD-37).

**Given** a force-kill of the recorder mid-segment
**When** the partial segment is inspected
**Then** it is playable up to the last complete fragment (~4 s bound), verified by an induced-kill test on committed fixture output (FR-73).

**Given** the recovery result
**Then** the `recovered` state is recorded for the once-per-session notice UI in Story 20.3 — recovery is the safety net; the live loud-failure notification is Story 18.4 (FR-73).

### Story 17.4: Automated Gapless-Concat Test (NFR-22)

As a maintainer,
I want a test that proves segment handover is gapless,
So that A/V-sync regressions across rotation fail the build, not the user's recording.

**Requirements:** NFR-22, AD-37, AD-21
**Dependencies:** 17.1, 17.2

**Acceptance Criteria:**

**Given** a session's segments
**When** the concat-assert test runs
**Then** it concatenates them and asserts **monotonic timestamps with no gap or overlap exceeding one frame duration**, and the test is wired into the CI perf/concat harness as a **release gate** (extends AD-21 measurement hooks) (NFR-22).

**Given** the CI signing constraint
**When** the test executes
**Then** it runs against committed fixture segments (or output produced on the signed runner) so gaplessness is gated without being blocked on a physical capture (NFR-22, AD-38).

**Given** Epic 20's webcam
**Then** the harness leaves a screen↔camera one-frame-alignment assertion hook to be populated when `camera-####` files exist (Story 20.1) (NFR-22).

### Story 17.5: Segment-Size & Duration-Cap Settings

As a user,
I want to choose how large each segment gets,
So that recordings match my disk and cleanup habits.

**Requirements:** FR-72 (settings leg), AD-25
**Dependencies:** 17.1, 17.2

**Acceptance Criteria:**

**Given** recording settings
**When** they are read or written
**Then** segment size (default 500 MB) and duration-cap fallback (default 30 min) persist in `keeper.db` behind `keeper-core::settings` — no `tauri-plugin-store`/sql (AD-25) — and are passed to the sidecar on `start{segmentMB, …}` (FR-72).

**Given** a configured segment size
**When** recording runs
**Then** the value is respected within one keyframe interval of file growth (FR-72).

**Given** the two surfaces
**Then** Settings → Recording and the setup card mirror the same values, and changing either affects future sessions only (FR-72, UX-DR29).

## Epic 18: Tray & Loud Failures — The Menu Bar Tells the Truth

Make the menu bar the always-truthful recording surface and every fault loud: tray recording/warning/error states with a live elapsed·segment·size line, Stop, and Open Recordings Folder; forced tray presence with exact restore; honest quit-while-recording; the in-app active-recording banner; the loud-failure triad (tray + notification + banner); and the disk-space guard. This is research R.3 plus the NFR-20 guard, extending Story 10.3 / AD-18.

### Story 18.1: Tray Recording State — Elapsed·Segment·Size, Stop, Open Folder

As a user,
I want the menu bar to show that keeper is recording, for how long, and let me stop with one click,
So that a live recording is never something I can forget about.

**Requirements:** FR-74, AD-39; UX-DR32
**Dependencies:** Epic 16, Epic 17 (segment info)

**Acceptance Criteria:**

**Given** the opt-in tray (`crates/keeper/src/tray.rs`, single mutex-guarded `TrayIcon` slot)
**When** a recording starts
**Then** within 1 s the tray reflects `recording` via `TrayIcon::set_icon` (record-dot badge asset), a ~1 Hz tick updates a disabled menu line (`"Recording — 12:34 · segment 3, 412 MB"`), and the menu adds **Stop Recording** and **Open Recordings Folder** (FR-74, AD-39).

**Given** the tray menu
**When** the user chooses Stop Recording
**Then** the current segment finalizes and the session reaches `finalized`; Open Recordings Folder reveals the session folder (FR-74).

**Given** macOS's own screen-recording indicator
**Then** the system purple pill is left untouched — keeper's tray adds what the pill lacks (elapsed, segment, Stop, error states) (FR-74, AD-39).

### Story 18.2: Forced Tray Presence & Honest Quit-While-Recording

As a user,
I want the recording indicator to always be visible and quitting to never orphan a recorder,
So that recording is never silently running and quitting never loses the tail.

**Requirements:** FR-74, AD-39; FR-53 (quit-honesty extension)
**Dependencies:** 18.1

**Acceptance Criteria:**

**Given** the FR-53 opt-in tray toggle is **off**
**When** a recording starts
**Then** recording **temporarily forces the tray visible**, and Stop restores the exact prior tray configuration — a recording indicator that isn't visible is a bug (FR-74, AD-39).

**Given** a quit (`⌘Q`) while recording
**When** the user confirms
**Then** keeper warns first, then runs the `stop` RPC → flush → **kill-timeout guard**, finalizing the current segment before exit and never orphaning `keeper-rec` — extending FR-53's quit honesty; even a hung sidecar is force-terminated after the timeout (FR-74, AD-39).

### Story 18.3: In-App Active-Recording Banner & Segment Meter

As a user,
I want an in-app twin of the tray while I'm in the Recording view,
So that the recording state is honest whether I'm looking at the menu bar or the app.

**Requirements:** FR-74/FR-75 (in-app surface); UX-DR31, UX-DR30
**Dependencies:** 18.1

**Acceptance Criteria:**

**Given** an active recording
**When** the Recording view is open
**Then** the `active-recording-banner` pins to the top — record dot + `mono` "Recording — 12:34 · segment 3 · 412 MB" + Stop (destructive-outline) — **persistent, never toast-only**, and the `segment-meter` fills toward the segment size and resets at each gapless rotation (UX-DR31).

**Given** the recording-red token
**Then** it appears **only** on the record dot, the banner edge, and the error banner — never on buttons, text, hovers, or decoration — and reduced-motion keeps the dot steady, never pulsing (UX-DR30).

**Given** a fault
**Then** the banner renders warning and error variants (the loud-failure surface wired in Story 18.4) with a "Restart recording" affordance; Pause is absent this phase (UX-DR31).

### Story 18.4: Loud-Failure Triad — Tray Error + Notification + Banner

As a user,
I want every recording fault to be impossible to miss,
So that no recording ever fails silently.

**Requirements:** FR-75, AD-39, AD-18, NFR-5
**Dependencies:** 18.1, 18.3

**Acceptance Criteria:**

**Given** a recorder crash/exit, writer stall, or device loss
**When** the fault occurs
**Then** the tray flips to `error`, a native notification posts within 5 s through the **AD-18 pipeline** (FR-51/NFR-7) offering one-click restart of the recording, and the banner shows the error variant naming the reason; the session manifest records the true terminal status `failed` (FR-75, AD-39).

**Given** any started Recording Session
**Then** it reaches a user-visible terminal state — `finalized | recovered | failed` — extending NFR-5's no-silent-loss to recordings; non-fatal warnings persist until resolved or acknowledged, never a dismissed-and-gone toast (FR-75, NFR-5).

**Given** the induced-failure coverage
**Then** recorder-kill, writer-stall, and device-loss legs are induced in automated tests here; the **live permission-revoke-mid-record** leg (with already-written segments intact) is validated on hardware at SM-10 acceptance (Story 20.6) (FR-75).

### Story 18.5: Disk-Space Guard — Warn & Graceful Stop-and-Finalize

As a user,
I want keeper to protect me from filling the disk mid-meeting,
So that a long recording degrades gracefully instead of dying mid-write.

**Requirements:** NFR-20, AD-39, AD-33
**Dependencies:** 18.3, 18.4, Epic 17 (finalize path)

**Acceptance Criteria:**

**Given** a start request
**When** the target folder is validated
**Then** pre-start free-space validation runs (alongside exists/writable) with an actionable error before any capture begins (NFR-20, FR-71).

**Given** a running recording
**When** free space on the target volume crosses a threshold
**Then** the disk-guard **policy in `keeper-core::recording`** — driven by free-space on the sidecar's `state` events (AD-39 ownership split) — raises a **persistent warning** below the warn threshold (authored default 10 GB) and **gracefully stops-and-finalizes** below the hard floor (authored default 2 GB), saying so, rather than running the volume to exhaustion or dying mid-write (NFR-20).

**Given** the authored thresholds
**Then** they are owner-sign-off items at phase release (PRD §14.7) and are testable via a simulated low-free-space signal without physically filling a disk (NFR-20).

## Epic 19: Sources & Devices — Choose What and Whom to Capture

Give the user real control over what and whom to capture: a live application/window picker with app-scoped audio, a system-audio toggle, a microphone written as its own track with hot-unplug resilience, a destination-folder chooser, and an advanced fps control. This is research R.4 + R.5.

### Story 19.1: Application/Window Picker — SCShareableContent Live List

As a user,
I want to record a single application instead of my whole screen,
So that only the app I'm demoing is in the file — not my notifications or other windows.

**Requirements:** FR-68 (app-picker leg), AD-34; UX-DR29
**Dependencies:** Epic 16, Epic 17

**Acceptance Criteria:**

**Given** `listSources` (AD-34)
**When** the source picker renders
**Then** it lists Displays then Applications with names and icons (single-select), and **re-enumerates as applications launch and quit** (FR-68).

**Given** an app-scoped selection
**When** recording runs
**Then** only that application's windows are captured — keeper itself, other apps, and incoming notification banners never appear in the file — and the setup discloses this inline ("only {App}'s windows and audio — keeper, other apps, and notification banners are excluded") (FR-68, UX-DR34).

**Given** a source that disappeared before Start
**When** the user presses Start
**Then** a clear inline error appears, never a hung recording; on multi-display setups each display is individually selectable (FR-68).

### Story 19.2: System-Audio Toggle & Per-App Audio Scoping

As a user,
I want to choose whether the recorded content's audio is captured,
So that I control what sound lands in the file — and my own notification sounds never do.

**Requirements:** FR-69 (system-audio leg), FR-68
**Dependencies:** 19.1

**Acceptance Criteria:**

**Given** the Audio card
**When** it renders
**Then** a System audio `Switch` (default **on**) is labelled as "the audio the recorded content plays" — not a device pick — and system audio is captured via `capturesAudio` scoped by the same `SCContentFilter` with `excludeCurrentProcessAudio = true` (FR-69).

**Given** an app-scoped recording with system audio on
**Then** only that application's audio is captured, keeper's own notification sounds are excluded, and system audio is written as its **own AAC track**, never premixed (FR-69).

### Story 19.3: Microphone Picker & Separate Track

As a user,
I want to add my microphone as a separate audio track,
So that my voice is captured alongside the content and can be edited independently later.

**Requirements:** FR-69 (mic leg), AD-36 (mic probe)
**Dependencies:** 19.2

**Acceptance Criteria:**

**Given** the Audio card
**When** the microphone `device-picker` renders
**Then** it defaults to "System default input", and Microphone permission is requested **only when this source is enabled**, never preemptively (FR-69, AD-36).

**Given** an enabled microphone
**When** recording runs
**Then** the mic is written as its **own AAC track** (never premixed with system audio) so stock players (QuickTime, browsers, VLC) play the two together and editors can separate them, using in-stream `captureMicrophone` on macOS 15+ and a parallel `AVCaptureSession` on 13–14 (same writer either way, invisible to the user) (FR-69).

### Story 19.4: Microphone Hot-Unplug Resilience

As a user,
I want unplugging my mic mid-meeting to never kill the recording,
So that a bumped cable costs a warning, not the whole session.

**Requirements:** FR-69 (hot-unplug leg); FR-74/FR-75 (warning surface)
**Dependencies:** 19.3, Epic 18 (warning surface)

**Acceptance Criteria:**

**Given** a running recording with a microphone
**When** the mic is unplugged
**Then** the recording **never aborts** — video and system audio keep rolling, the mic track continues **silence-filled**, keeper attempts fallback to the system default input, and a **persistent warning state** is raised on the tray and banner (FR-69, FR-74/FR-75).

**Given** device churn
**When** microphones connect/disconnect
**Then** keeper re-enumerates on device notifications; the never-abort behavior is validated via a simulated device-removal signal (real Continuity-Camera/mic churn on hardware is folded into SM-10 acceptance, Story 20.6) (FR-69).

### Story 19.5: Destination-Folder Chooser & fps Advanced Control

As a user,
I want to pick where recordings are saved and, if I care, the frame rate,
So that files land where I expect and the defaults stay out of my way.

**Requirements:** FR-71 (folder-chooser leg), FR-72 (fps leg), AD-25
**Dependencies:** Epic 17 (settings), 19.1

**Acceptance Criteria:**

**Given** the Destination card
**When** it renders
**Then** a folder chooser shows the remembered default `~/Movies/keeper`, and a **validate-on-Start** check (exists, writable, free space per NFR-20) blocks start with actionable errors (FR-71).

**Given** the collapsed Advanced group
**When** the user opens it
**Then** fps is selectable (30 default, 60 selectable) and passed to the sidecar on `start{fps}` (FR-72).

**Given** the two surfaces
**Then** folder and fps persist in `keeper.db` behind `keeper-core::settings` (AD-25), mirror Settings → Recording, and changing either affects the next session only (FR-71/FR-72).

## Epic 20: Webcam & Polish — Ship the Phase

Close the phase: optional webcam as a separate synchronized file, the Microphone/Camera pre-flight rows, the completion/recovery card, palette actions + optional global hotkey + the capability-gating and zero-egress audits, docs/recording.md, the reliability envelope (4 h soak + CPU/memory), and the SM-9/SM-10 phase acceptance with retrospective inputs. This is research R.6 + R.7.

### Story 20.1: Optional Webcam as a Separate Synchronized File

As a user,
I want my webcam recorded alongside the screen as its own file,
So that I have a talking-head track without burning it into the screen recording.

**Requirements:** FR-70, AD-37; NFR-22 (alignment)
**Dependencies:** Epic 17 (rotation/manifest), Epic 19 (device-picker pattern)

**Acceptance Criteria:**

**Given** the Webcam card
**When** it renders
**Then** a `Switch` (default **off**) reveals a camera `device-picker` (built-in / external / Continuity Camera); Camera permission is requested only when enabled, and webcam off produces no camera files and touches no Camera permission (FR-70, AD-36).

**Given** an enabled webcam
**When** recording runs
**Then** the camera records `camera-####.mp4` in the same session folder from a **second in-sidecar AVAssetWriter**, host-clock anchored and **rotated at the same segment boundaries** as `screen-####`, so played side by side from any segment index the two stay aligned within one video frame — populating the Story 17.4 screen↔camera alignment assertion (FR-70, AD-37, NFR-22).

**Given** camera loss mid-recording
**Then** the screen recording continues (never aborts) with a warning raised, and there is **no PiP burn-in and no self-view bubble** this phase; UX copy may note that macOS 14+ can composite the camera via the system presenter overlay — an OS behavior, not a keeper feature (FR-70, UX-DR34).

### Story 20.2: Microphone & Camera TCC Pre-flight Rows

As a user,
I want the permission pre-flight to cover my mic and camera when I enable them,
So that every source I turn on has an honest, fixable permission state before I start.

**Requirements:** FR-67 (Mic/Camera legs), AD-36; UX-DR33
**Dependencies:** 16.5 (pre-flight mechanism), 19.3 (mic), 20.1 (camera)

**Acceptance Criteria:**

**Given** the pre-flight (from Story 16.5)
**When** microphone or webcam is enabled
**Then** it adds a Microphone row and/or a Camera row, each live-detected at render (never cached), requested via the system prompt on enable (never preemptively), with a deep-link fix-path when only manual granting remains; the camera row is absent when webcam is off (FR-67, AD-36, UX-DR33).

**Given** a blocking source permission
**Then** Start is disabled naming it, and `NSMicrophoneUsageDescription` / `NSCameraUsageDescription` are present in keeper's bundle `Info.plist` via the Tauri `bundle.macOS.infoPlist` merge (FR-67, AD-36).

### Story 20.3: Completion / Reveal-in-Finder & Startup Recovery Notice

As a user,
I want a clear end-of-recording summary and an honest notice when a session was interrupted,
So that I always know where my files are and that nothing was silently lost.

**Requirements:** FR-71 (completion leg), FR-73 (recovery-notice leg); UX-DR34
**Dependencies:** Epic 17 (recovery/ledger), 16.6 (stop path), 18.1 (tray restore)

**Acceptance Criteria:**

**Given** a Stop
**When** the session finalizes
**Then** the Recording view shows a completion `Card` — "Saved N segments · {size}" + the session-folder path in `mono` + a primary **Reveal in Finder** — with no preview, trim, or share affordance, and the tray returns to its exact prior configuration (FR-71, FR-76, UX-DR34).

**Given** an interrupted session (marked `recovered` by Story 17.3)
**When** keeper starts or is about to begin a new recording
**Then** it surfaces **once** as "A recording was interrupted; N segments were saved" — the same card shape with a `bridge-degraded`-tinted edge, linking the folder — and recovered files play as-is with no remux (FR-73, UX-DR34).

### Story 20.4: Palette Actions, Global Hotkey, Capability-Gating & Zero-Egress Audit

As a user,
I want to start/stop recording from the palette or a global hotkey, and I want proof recording never leaks,
So that recording is fast to reach and provably local-only.

**Requirements:** FR-66 (gating audit), FR-48 (palette), FR-50 (hotkey), FR-76 (egress); AD-35
**Dependencies:** 16.3, Epic 18, Epic 19

**Acceptance Criteria:**

**Given** the `recording` flag on
**When** the Command Palette and Shortcuts render
**Then** "Start recording", "Stop recording", and "Open recordings folder" actions are registered **only behind the flag** (FR-66/FR-48), and an optional configurable global Start/Stop Recording hotkey (unset by default) is assignable in Settings → Shortcuts with conflict detection (FR-50) — Stop remains one click from the tray regardless (FR-74).

**Given** the destructive-by-omission guard
**Then** there are no single-key verbs on this surface and `Esc` does **not** stop a recording — stopping is always explicit (UX-DR29).

**Given** the gating audit
**Then** a test confirms no recording surface (sidebar, Settings, palette, tray) renders on macOS < 13.0 or iOS — absent, not disabled (FR-66, AD-35).

**Given** the zero-egress audit
**Then** a full record → stop → recover cycle contacts no new hosts, and the NFR-11 per-release egress inventory diff for the phase is **empty** — verifiable at review and at runtime; no upload/share/transcription/cloud affordance exists anywhere in the recording UI (FR-76).

### Story 20.5: Reliability Envelope — 4 h Soak & CPU/Memory Verification

As the owner,
I want the long-run and performance bars measured on real hardware,
So that "records for hours without falling over" is evidence, not a hope.

**Requirements:** NFR-19, NFR-21, AD-39
**Dependencies:** Epic 17, Epic 18, Epic 19, 20.1
**Human-in-the-loop:** **yes** — a 4 h continuous capture on reference Apple Silicon with an **Apple Development-signed build**; the automation loop defers it to the coordinator instead of escalating.

**Acceptance Criteria:**

**Given** a 4 h continuous recording (1080p-class display, 30 fps, system audio + microphone)
**When** the soak runs on reference hardware
**Then** it completes with **zero** recorder crashes, writer stalls, or A/V desync and no unbounded memory growth; sample-buffer queues stay bounded with a **drop-oldest-video** policy (audio never dropped), and sustained dropping raises a warning (FR-75) (NFR-19).

**Given** the performance envelope
**When** measured during recording
**Then** it adds **< 100% of one core** average CPU and **< 400 MB** combined RSS (sidecar + keeper), and keeper's messaging bars NFR-1–NFR-4 still hold while recording (NFR-21).

**Given** the authored bars (NFR-19 duration, NFR-21 numbers)
**Then** the measurements are recorded and the owner confirms or adjusts them (PRD §14.7 open #1) before they become release gates — mirroring the AD-22/NFR-3 posture (NFR-19, NFR-21).

### Story 20.6: SM-9 / SM-10 Phase Acceptance, docs/recording.md & Retrospective Inputs

As the owner,
I want the phase demonstrated end-to-end on hardware, documented, and accepted,
So that recording ships on evidence and the retrospective has its inputs.

**Requirements:** SM-9, SM-10; FR-75 (induced-failure matrix), FR-76 (egress); FR-67/AD-38 (docs disclosures); retrospective inputs
**Dependencies:** 20.1, 20.2, 20.3, 20.4, 20.5 (+ Epics 16–19 complete)
**Human-in-the-loop:** **yes** — a physical Mac with a Development-signed build, real TCC grants (including a live permission-revoke mid-record) and real device churn. This is the phase's final device step; the automation loop defers it to the coordinator instead of escalating.

**Acceptance Criteria:**

**Given** SM-9 on a Development-signed build on macOS 13+ hardware
**When** the end-to-end gate runs
**Then** permission pre-flight → full-screen **and** app-scoped recording with system audio + microphone (+ webcam as a separate file) → segments rotate at the configured size into the chosen folder with a valid manifest → an induced crash recovers per FR-73 — binary, demo-able, release-gating (SM-9, validates FR-66–FR-76).

**Given** SM-10
**When** reliability is checked
**Then** the NFR-19 soak (Story 20.5) is green, the induced-failure matrix (recorder kill, mic unplug, disk floor, **permission revoke**) surfaces loudly in **100%** of tests with already-written segments intact (FR-75), zero silent recording-loss incidents occur during dogfooding, and the NFR-11 egress diff for the phase is empty (FR-76) (SM-10).

**Given** `docs/recording.md`
**When** written
**Then** it covers the **dev-signing requirement** (Apple Development certificate; macOS 15+ ad-hoc SCK rejection, Cap #1722), the **monthly re-auth nag** for non-picker SCK, the untouched macOS purple capture pill, and the disk-guard / segment-size / folder defaults — **one-to-one with the in-app disclosures** so app and docs never diverge; English, honest per the voice rules, and free of credentials or signing material (FR-67, AD-38).

**Given** the phase retrospective
**Then** its inputs are recorded: outcomes against the §14.6 risk register (TCC/ad-hoc signing, sidecar notarization, monthly nag, disk exhaustion, long-run stability, gapless-rotation correctness, API drift, device churn) and the deferred-work ledger opened this phase (pause/resume, webcam PiP burn-in + self-view, `SCContentSharingPicker` path, HEVC/HDR, DND-while-recording, orphan-segment remux, in-app recordings browsing, Windows/Linux recording).

**Epic 20 exit / phase acceptance:** SM-9 green on dev-signed hardware; SM-10 reliability bars met; docs/recording.md current; authored bars owner-confirmed; retrospective inputs on file.


## Epic 21: Recording Ergonomics — Codec, Scale, Audio-Only, Template Tray, Session Metadata

Owner-requested increment after the v0.2.0 release (2026-07-21): quality/size
control (HEVC + resolution scaling), audio-only capture for calls, macOS-native
template tray icons, and pre-session naming/metadata with wall-clock times in
the manifest. Builds strictly on the shipped Epics 16-20 chain; the sidecar
protocol stays v1 (all wire changes additive, per the 16.5/16.6/17.4 precedent).

### Story 21.1: Codec Choice — H.264 / HEVC (Hardware-Accelerated)

As a user,
I want to pick the video codec, including HEVC,
So that long recordings are markedly smaller with Apple Silicon hardware encoding.

**Requirements:** extends FR-68/AD-37 (format), NFR-21 (envelope)
**Dependencies:** Epics 16-20 shipped

**Acceptance Criteria:**

**Given** the Settings -> Recording section and the Recording view's Advanced card
**When** the user picks the codec (H.264 default | HEVC)
**Then** the choice persists in the Rust settings registry, applies to the next
Recording Session (same "Applies to the next Recording Session." idiom as the
existing knobs), and the sidecar's `startRecording` gains an additive `codec`
param (`"h264"` default when absent — older wire preserved); segments and the
camera file encode with `AVVideoCodecType.hevc` when selected, via
VideoToolbox hardware encode on Apple Silicon (no software-encode fallback
configuration — the OS picks the encoder), staying fragmented QuickTime .mov,
gapless-rotation semantics unchanged (the 17.4 CI gate runs for BOTH codecs).

**Given** an HEVC recording
**Then** the files play in QuickTime on the recording Mac, and docs/recording.md
gains an honest compatibility note (HEVC needs macOS 10.13+/modern players;
H.264 stays the maximum-compatibility default).

### Story 21.2: Resolution Scaling — 100% / 75% / 50%

As a user,
I want to record at a fraction of the native resolution,
So that file size and encode cost drop when full pixel density is not needed.

**Requirements:** extends FR-68, NFR-21
**Dependencies:** 21.1 (shared Advanced-card layout)

**Acceptance Criteria:**

**Given** the Advanced card and Settings -> Recording
**When** the user picks the capture scale (Full 100% default | 75% | 50%)
**Then** the choice persists, applies to the next Session, and the sidecar
scales `SCStreamConfiguration.width/height` (and the display-scoped filter
math) by the factor, rounding to even pixel dimensions (encoder requirement);
the manifest records the effective pixel dimensions per session; app-scoped
capture scales identically.

**Given** any scale
**Then** rotation, the NFR-22 gapless gate, and the idle heartbeat behave
identically (the heartbeat re-appends the scaled frame — no full-res leak).

### Story 21.3: Audio-Only Recording — No Video Track

As a user,
I want an audio-only session (system audio and/or microphone, no video),
So that recording a call does not cost video encode, screen pixels, or size.

**Requirements:** extends FR-69; AD-33/AD-37 (session/manifest shapes hold)
**Dependencies:** Epics 16-20 shipped

**Acceptance Criteria:**

**Given** a new "Audio only" toggle in the Source card (exclusive with display/
app targets; Screen Recording permission NOT required when on — the pre-flight
gates on the mic/system-audio needs only)
**When** the user starts an audio-only session
**Then** the sidecar records `audio-####.m4a` segments (AAC; system audio
and/or mic as separate tracks exactly like the video path) with NO SCStream
video output and no video track; rotation triggers on the same byte/duration
budgets; the manifest marks `captureTarget: {kind: "audioOnly"}` and the
segment ledger tracks the audio files; the banner/tray show the session
without a segment-pixel meter (elapsed + size only).

**Given** system audio in an audio-only session
**Then** SCStream runs audio-only capture (video output not attached) OR, when
only the microphone is enabled, no SCStream at all (AVCapture mic path alone)
— whichever the implementation needs, the Screen Recording TCC is only
demanded when system audio is actually on (SCK requires it for system audio;
the pre-flight row says so honestly).

**Given** a finished audio-only session
**Then** the files play in Music/QuickTime, and the recovery path (17.3)
salvages interrupted audio sessions identically.

### Story 21.4: Template (White) Tray Icons — Native Menu-Bar Look

As a user,
I want keeper's menu-bar icons to render like every other macOS icon,
So that the tray looks native (white/adaptive), not a colored sticker.

**Requirements:** refines FR-53/Epic 18 tray surfaces; UX-DR (tray)
**Dependencies:** Epic 18 shipped

**Acceptance Criteria:**

**Given** every tray state (idle presence, recording, error)
**When** rendered in the menu bar
**Then** the icons are macOS TEMPLATE images (monochrome with alpha,
`set_icon_as_template(true)`), so macOS colors them white/black per menu-bar
appearance and highlights them natively; the recording state stays visually
distinct via glyph shape (e.g. filled record dot vs outline), NOT via color;
the error state uses an exclamation-badged glyph. Dark menu bar, light menu
bar, and reduced-transparency all render correctly (manual device check).

**Given** the loud-failure triad (18.4)
**Then** its behavior is unchanged — only the icon rendering becomes template.

### Story 21.5: Session Naming & Metadata — Title, Participants, Notes, Times

As a user,
I want to name the next recording and attach meta information,
So that sessions are identifiable later (who the call was with, which program).

**Requirements:** extends FR-71 (manifest), AD-33
**Dependencies:** Epics 16-20 shipped

**Acceptance Criteria:**

**Given** a new "Next session" card in the Recording view (above Source)
**When** the user fills optional fields — Title, Participants (free text),
Program/Session note — and starts recording
**Then** the session folder is named `<sanitized title> <local ts>/` (title
absent -> the existing `keeper-rec <local ts>/`; sanitization strips
path-hostile characters, keeps Unicode), and `manifest.json` gains a `meta`
object `{title, participants, note}` (absent fields omitted) — the fields
clear after Start (they described THAT session) but the last values are
offered as quick re-fill.

**Given** every session (with or without meta)
**Then** the manifest records wall-clock times: `startedAt` and `endedAt` as
ISO-8601 local timestamps with offset, alongside the existing host-clock PTS
bounds (which stay authoritative for continuity); the completion card and
recovery notice render the title when present.

**Given** the meta fields
**Then** nothing uploads anywhere (zero egress unchanged); values live only in
the local manifest.

**Epic 21 exit:** all five stories demo-able on hardware; check:all green;
docs/recording.md updated for codec/scale/audio-only/meta; NFR-21 spot-check
(HEVC + 50% scale should not exceed the H.264 baseline envelope).

## Post-MVP — Not Storied (Flagged Only)

Per PRD §5/§6.2 these are explicitly out of MVP; no stories exist for them and none may be smuggled in:

- Snooze/reminders; scheduled send (local-only framing when it comes); low-priority view; message-request filtering; labels/filtered views; note-to-self
- Bridge health dashboard + alert center (aggregate); bbctl full lifecycle supervision (auto-restart, log viewer)
- iMessage via user's own Mac; voice-note recording; notification quick-reply; typing-only privacy toggle; per-Chat stay-archived override; Beeper-style custom views ("Spacebar")
- Agent-proposed Drafts API/MCP (propose-only, behind a flag, after design-partner validation) — the Approval Pane's reserved proposer column is the only MVP concession
- Voice/video calls (Element Call embed); Windows/Linux/Android/iPad (iOS ships as Phase 2, Epics 12–15); Beeper Desktop API companion mode
- Archive-at-rest encryption spike (AD-22); universal binaries

iOS phase (PRD §13.4 + spine Deferred) — explicitly out of this phase, no stories:

- APNs push + Notification Service Extension — behind the paid-program decision gate (§13.5, recorded in Story 15.5); App Store/TestFlight and every paid-program-dependent capability (App Groups, `https://` universal links, AltStore PAL notarization)
- Share extension, home-screen widgets, Siri intents, biometric app lock; full Dynamic Type adoption (rem-scaling is the phase bar per FR-60)
- Disk-backed streaming of large media (capped in-memory buffer is the phase posture, AD-28); micro Swift lifecycle plugin (only if `visibilitychange` proves unreliable, AD-30); Android's `convertMediaSrc` media-URL helper (introduced only when Android starts)
- iPad layout; `NWPathMonitor`-driven fast retry; share-sheet media save

Screen Recording phase (PRD §14.4 + spine Deferred) — explicitly out of this phase, no stories:

- **Video editing — never** (§5): keeper records; it does not trim, annotate, or compose. **Any cloud upload, share service, transcription, or remote processing — never** (§5, FR-76): the recording UI ends at Reveal in Finder.
- Pause/resume, webcam **PiP burn-in**, and a camera self-view preview bubble — deliberately after the capture core is trustworthy (AD-34's contract + AD-37's format are the carry-over seams)
- `SCContentSharingPicker` system-picker path (macOS 14+, also silences the monthly re-auth nag), HEVC/HDR capture, DND-while-recording, and an orphan-segment "tidy" remux pass — later
- The `persistent-content-capture` entitlement (would remove the monthly re-auth nag) — requires the paid Apple Developer Program and Apple approval; sits behind the §13.5-class paid-program gate (recorded in Story 15.5), accepted and disclosed instead (FR-67)
- In-app recordings browsing (a list of past sessions inside keeper) — MVP is folder + Finder + the tray's Open Recordings Folder (PRD §14.7 open #2); the inbox/settings projection patterns (AD-20/AD-25) are the extension point
- Windows/Linux recording — follows those platforms (§6.2); `CapabilitiesVm.recording` (AD-35) and the platform-free `recording` module (AD-33) are the platform-neutral seams (a non-macOS `Recorder` impl replaces `keeper-rec`)

## Validation Summary

- **FR coverage:** FR-1–FR-54 all mapped (see FR Coverage Map); split FRs (FR-6, FR-17, FR-18, FR-28, FR-44) have both legs explicitly assigned to stories.
- **NFR coverage:** NFR-1–NFR-14 either designed into specific stories (NFR-5/8/9/10) or gated in Epic 11 (NFR-1–4, 11–13); NFR-14 is distributed (Stories 1.2, 3.2, 9.2, 9.3) per the UX accessibility floor.
- **UX-DR coverage:** UX-DR1–20 each referenced by at least one story's ACs.
- **Architecture compliance:** AD-6/7/8 land in Story 1.1 (keeper-core split in Epic 1 as required); AD-13 gate seeded in 1.6, completed 7.4/8.3; AD-14 seeded in 3.9, completed 8.1/8.2; databases/tables are created only by the first story needing them (keeper.db registry in 1.3, drafts in 7.1, outbox in 8.3, archive.db in 5.1).
- **Dependencies:** every story depends only on earlier stories; each epic functions without any later epic (Epic 6's FR-28 notification leg is an explicit, documented enhancement in Epic 10, with in-app surfacing complete inside Epic 6).
- **Sizing:** 63 stories across 11 epics, each scoped to a single dev session on the existing scaffold.

**Phase 2 (iOS, appended 2026-07-09):**

- **FR coverage:** FR-55–FR-65 all mapped (see FR Coverage Map); split FRs have both legs assigned — FR-57 (mechanism 12.2 / surfaces 13.7), FR-61 (mechanics 14.1 / copy 14.2), FR-55 (init 12.1 / CI 12.5+15.4 / on-device 12.6 / assets+docs 15.1–15.3).
- **NFR coverage:** NFR-16–NFR-18 engineered and validated in Epic 14 (14.5, 14.6/14.4, 14.4); NFR-15 measured on-device in Story 15.6 and explicitly **not** release-gating until the owner confirms the 3 s bar (PRD §13.8).
- **UX-DR coverage:** UX-DR21–UX-DR28 each referenced by at least one story's ACs.
- **Architecture compliance:** AD-26/AD-27 land in 12.2 (one shell crate, CapabilitiesVm); AD-29 spike-first in 12.3 with on-device confirmation in 12.6; AD-28 cap in 12.4; AD-30's single lifecycle entry in 14.1 with the reload guard in 14.4 (first exercised at the 12.6 gate per the "walking skeleton onward" mandate); AD-31 projection in 13.1 with no router; AD-32 discipline in 12.1/12.5/15.1/15.4.
- **Gates:** SM-7 = Epic 12 exit (Story 12.6); SM-8 opens at Story 15.6; Epic 13 and Epic 14 are parallelizable after SM-7; Epic 15 closes the phase.
- **Human-in-the-loop:** exactly two stories require a physical device and the owner — 12.6 (on-device skeleton validation) and 15.6 (final device install) — both explicitly marked so the automation loop defers them to the coordinator rather than escalating; every other iOS story is implementable with simulator/compile gates alone.
- **Sizing:** 26 stories across Epics 12–15 (6 + 7 + 7 + 6), each scoped to a single dev session; total 89 stories across 15 epics.

**Phase 3 (Screen Recording, appended 2026-07-16):**

- **FR coverage:** FR-66–FR-76 all mapped (see FR Coverage Map); split FRs have every leg assigned — FR-67 (Screen leg 16.5 / Mic+Camera rows 20.2), FR-68 (full-screen 16.6 / app-picker 19.1), FR-69 (system-audio 16.6 / toggle 19.2 / mic 19.3 / hot-unplug 19.4), FR-71 (single-file 16.6 / session-folder+manifest 17.2 / folder-chooser 19.5 / completion 20.3), FR-72 (rotation 17.1 / settings 17.5 / fps 19.5), FR-73 (recovery 17.3 / notice 20.3).
- **NFR coverage:** NFR-22 engineered in Story 17.1 and gated by the automated concat-assert test 17.4; NFR-20 disk guard in 18.5; NFR-19 soak and NFR-21 CPU/memory envelope measured on reference hardware in Story 20.5 and explicitly **not** release-gating until the owner confirms the authored bars (PRD §14.7 open #1), mirroring the AD-22/NFR-3 posture.
- **UX-DR coverage:** UX-DR29–UX-DR34 each referenced by at least one story's ACs (recording view/gating, recording-red token, active banner + segment meter, tray states, permission rows, completion/recovery + voice).
- **Architecture compliance:** AD-38 (sidecar layout/codesign/externalBin) lands in 16.1; AD-33 (recording split + Recorder port) in 16.2; AD-35 (capability flag) in 16.3; AD-34 (NDJSON-RPC contract) in 16.4/16.6; AD-36 (three TCC classes) in 16.5/20.2; AD-37 (format/segmentation/recovery) across 16.6/17.1/17.2/17.3/20.1; AD-39 (tray + honest quit + loud failure) across 18.1–18.5; the platform-free `recording` core never holds an Apple API or process handle (16.2), and every recording surface renders only behind `CapabilitiesVm.recording` (16.3, audited in 20.4).
- **Gates:** R.1 / SM-9-seed = Epic 16 exit (Story 16.6, real recording plays back); the NFR-22 concat-assert gate lands in 17.4; SM-9 (end-to-end) and SM-10 (reliability + induced-failure matrix + empty egress diff) are accepted in Story 20.6; Epics 17 and 18 build on Epic 16 and Epic 19 follows, with Epic 20 closing the phase.
- **Human-in-the-loop:** exactly three stories require a physical Mac, real TCC grants, and an Apple Development-signed build (macOS 15+ rejects ad-hoc ScreenCaptureKit, Cap #1722) — 16.6 (first real capture / walking-skeleton exit), 20.5 (4 h soak + CPU/memory envelope), and 20.6 (SM-9/SM-10 phase acceptance) — all explicitly marked so the automation loop defers them to the coordinator; every other recording story is implementable with compile gates, unit tests, stub sidecars, fixture segments, and simulated fault/low-disk/device-removal signals.
- **Sizing:** 27 stories across Epics 16–20 (6 + 5 + 5 + 5 + 6), each scoped to a single dev session on the existing macOS app; total 116 stories across 20 epics.





## Epic 22: Recording Ergonomics II — Precision, Metadata Depth & Debuggability

User-requested follow-ups (2026-07-21): quarter-scale capture with a live
effective-resolution hint, flicker-free source refresh, richer session
metadata (tags + custom fields), the mic as a separate track in the webcam
file, a debug mode with on-disk event logs, and file-based config overrides.
Out of scope with honest verdicts: AV1 encode (no VideoToolbox encoder on
Apple Silicon through M4; AVFoundation exposes no AV1 writer codec),
per-output-device audio capture (needs Core Audio taps — deferred-work), and
hiding the macOS capture indicator (system-owned by design — documented).

### Story 22.1: Quarter Scale & Live Effective-Resolution Hint
Scale set becomes {100, 75, 50, 25}; `listSources` displays gain additive
`pixelWidth`/`pixelHeight`; the Advanced scale row shows the selected
target's effective output resolution live (e.g. "2880×1800 → 720×450").

### Story 22.2: Flicker-Free Source Refresh Indicator
The "Refreshing…" text line (layout shift on every ~3 s poll) becomes a small
spinner beside the Source heading — no reflow, no flicker.

### Story 22.3: Session Tags & Custom Metadata Fields
The Next-session card gains a Tags input (comma-separated → `meta.tags[]`)
and repeatable custom Name/Value rows (`meta.custom[]`); manifest-local only,
rendered nowhere else yet (browsing is future work).

### Story 22.4: Microphone Track in the Webcam File
When both the webcam and the mic are on, `camera-####.mov` carries the mic as
its own separate AAC track (same never-premixed rule), split at the same
rotation boundaries; the screen file's mic track is unchanged.

### Story 22.5: Debug Mode — Event & Error Logs on Disk
A Settings → Advanced "Debug logging" toggle: every sidecar NDJSON event,
state transition, warning and error of a session appends to
`<session folder>/events.log`, and app-level tracing mirrors to
`~/Library/Logs/keeper/keeper.log` while enabled. Off by default; logs are
local, secret-free, and named in docs.

### Story 22.6: File-Based Config Overrides
`config.json` beside `keeper.db` (data dir): read at startup, its keys are
imported over the settings table (file wins), enabling hand-edited /
version-controlled setups; the path and key list are documented in
docs/recording.md. Malformed files are reported loudly and skipped.
