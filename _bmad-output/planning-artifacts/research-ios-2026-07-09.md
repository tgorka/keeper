# Technical Research: keeper — iOS/iPhone Phase (Tauri 2 Mobile)

- **Date:** 2026-07-09
- **Author:** BMAD technical-research (Claude)
- **Project:** keeper — open-source Matrix messenger client. Desktop macOS MVP is DONE (Tauri 2 workspace: `keeper-core` platform-free Rust core with matrix-sdk 0.18, `keeper` Tauri shell; React 19 + TS + Tailwind v4 + shadcn/ui; zustand mirror stores over IPC channels; ts-rs bindings).
- **Scope:** iOS phase per AD-24 (Plan A = Tauri mobile reusing keeper-core + the same IPC contract, validated by a walking-skeleton iOS build before major UI investment).
- **Method:** Live web research (2026-07-09) — Tauri v2 docs, GitHub issues/discussions, Apple docs/forums, Element X iOS sources, sideloading-community sources — plus direct inspection of the keeper repo.

---

## 1. Tauri 2 on iOS in mid-2026

### 1.1 Maturity

Tauri 2 has shipped stable iOS/Android targets since v2.0 (Oct 2024). By mid-2026 the mobile API is stable and the toolchain (`tauri ios init/dev/build`) is solid for "webview + Rust backend" apps, but the team's own framing still holds: 2.0 was *not* the "mobile as first-class citizen" release — the foundation works, the ecosystem is younger than desktop. Practical read for keeper:

- The core loop (WKWebView frontend, Rust staticlib backend, IPC commands + channels) works on iOS today — this is exactly keeper's architecture, so the risk is concentrated in *plugins and platform glue*, not the app model.
- Known active bug class to watch: webview can go blank/unresponsive when resuming from background on iOS ([tauri#14371](https://github.com/tauri-apps/tauri/issues/14371)) — must be part of walking-skeleton acceptance testing.
- Community feedback (e.g. [discussion #10197](https://github.com/orgs/tauri-apps/discussions/10197)) consistently flags: docs lag, not all plugins available on mobile, and per-plugin stability varies.

### 1.2 Plugin support matrix (as used by keeper today)

Keeper's desktop plugin set (AD-25) vs iOS availability:

| Plugin | iOS | Consequence for keeper |
|---|---|---|
| notification | **Yes** (local notifications; mobile even adds Actions API) | Reusable — but only fires while the app process is alive (see §1.8/§3) |
| deep-link | **Yes** (custom schemes + universal links) | `keeper://` OIDC callback path can be kept; config moves from `desktop.schemes` to mobile scheme registration in the plugin config + Info.plist |
| global-shortcut | **No — desktop only** | cfg-gate out of the iOS build (Epic 9 hotkeys become desktop-only) |
| updater | **No — desktop only** | iOS updates = reinstall/re-sign; cfg-gate out |
| autostart | **No — desktop only** | cfg-gate out |
| window-state | **No — desktop only** | cfg-gate out (single fullscreen scene on iOS) |
| clipboard-manager | Desktop-focused (mobile support partial/unreliable) | Use web Clipboard API in the webview as fallback |
| opener | Desktop-only in official matrix | Replace with a tiny Swift plugin call or `UIApplication.open` via mobile plugin; needed for "open in browser" during OIDC fallback |
| tray-icon (tauri feature) | **No** | cfg-gate `tray` module + `tray-icon` cargo feature |

Mobile-only plugins that become *available* (not currently needed but relevant later): biometric, haptics, barcode-scanner, NFC, geolocation.

**Implication:** `crates/keeper/src/lib.rs` needs `#[cfg(desktop)]` / `#[cfg(mobile)]` seams around plugin registration (tray, global-shortcut, autostart, updater, window-state, deep-link desktop config). Tauri's generated mobile entry point (`tauri::mobile_entry_point`) already exists in the standard template; keeper's `keeper_lib` crate-type is already `["staticlib", "cdylib", "rlib"]` — the staticlib is exactly what the iOS build links (verified in repo: `src-tauri/crates/keeper/Cargo.toml:18`).

### 1.3 `tauri ios init / dev / build` workflow

**Prerequisites checklist (one-time, on the existing dev Mac):**

```sh
# Full Xcode from the App Store (not just CLT), then:
xcode-select -s /Applications/Xcode.app
xcodebuild -runFirstLaunch
# Rust targets (device + Apple-Silicon simulator):
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
# CocoaPods (Tauri's generated project uses it):
brew install cocoapods
```

- Xcode 16.x line (iOS 18 SDK) is the mid-2026 norm; App Store submission requires it since April 2026 — irrelevant for sideloading but sets the toolchain baseline. The existing bun/vite toolchain is unchanged.
- `rust-toolchain.toml` note: ensure the pinned toolchain includes the iOS targets, or CI/`tauri ios build` will fail on a clean machine.
- `tauri ios init` generates `src-tauri/gen/apple/`: a `project.yml` (XcodeGen spec) from which the `.xcodeproj` is regenerated — **manual edits inside the `.xcodeproj` are overwritten**; persistent changes go in `project.yml`, `Info.plist`, or the `*_iOS/` sources. The generated project contains a build phase that shells out to the Tauri CLI to compile the Rust workspace as a staticlib; after compiling, the CLI validates the Mach-O archive contains the `start_app` symbol (i.e. `tauri::mobile_entry_point` was used).
- `tauri ios dev --open` is the recommended loop (keeps the CLI process alive, Xcode handles device deploy + logs); simulator builds compile host-arch only for speed. Frontend hot-reload works via `devUrl` pointed at the dev server on the LAN (Tauri rewrites it for the device; ensure vite listens on `0.0.0.0`).
- `tauri ios build --export-method <method>` archives + exports an IPA to `src-tauri/gen/apple/build/arm64/keeper.ipa`. Valid methods track Xcode's list: `debugging` (a.k.a. development), `release-testing` (ad-hoc), `app-store-connect`, `enterprise`. (Xcode renamed the values; older `app-store-connect` vs `app-store` mismatches caused [tauri#13818](https://github.com/tauri-apps/tauri/issues/13818) — pin CLI + Xcode versions together.)
- **Workspace note:** keeper is a Cargo workspace with the shell crate at `src-tauri/crates/keeper`. `gen/apple` is generated relative to the crate with `tauri.conf.json` — i.e. `src-tauri/crates/keeper/gen/apple/`. The `beforeDevCommand`/`beforeBuildCommand` cwd indirection already in `tauri.conf.json` carries over unchanged. Add `gen/apple/build/` to `.gitignore`; commit the rest of `gen/apple` (Tauri's recommendation) so Info.plist/entitlement edits are versioned.

### 1.4 Custom protocol `keeper-media://` on iOS

Good news — this is the *strong* part of the port:

- On macOS **and iOS**, wry implements `register_uri_scheme_protocol` via **WKURLSchemeHandler** (iOS 11+). The scheme stays native (`keeper-media://...`) — unlike Android/Windows where it is remapped to `http(s)://keeper-media.localhost`. Keeper's frontend already builds `keeper-media://` URLs on macOS, so **URL formats are identical on iOS**; no frontend change for the happy path. (If Android comes later, introduce a `convertMediaSrc`-style helper then.)
- Keeper's handler is already async-safe: it spawns the SDK fetch off-thread and replies through `UriSchemeResponder` (`src-tauri/crates/keeper/src/media_protocol.rs`). This matches WKURLSchemeHandler's async task model. Range slicing (200/206/416) is done in-memory from the SDK cache — works identically on iOS; WKWebView's `<video>` uses Range requests aggressively, which the handler already supports.
- Caveats: (a) WKURLSchemeHandler tasks can be *stopped* by WebKit (task invalidation) — wry handles the `webView(_:stop:)` callback; responses to dead tasks are dropped, which keeper's fire-and-forget responder pattern tolerates. (b) Memory: full-media-in-RAM slicing means a 200 MB video costs 200 MB of RAM inside a jetsam-limited process (§3.3) — acceptable for MVP, flag as a deferred-work item (streaming from disk cache).

### 1.5 SQLite / matrix-sdk-sqlite on iOS

- `matrix-sdk-sqlite` (bundled rusqlite → compiles SQLite from source) builds fine for `aarch64-apple-ios`; this is precisely the stack Element X iOS ships on (matrix-rust-sdk FFI + SQLite stores). No system-SQLite dependency issues because rusqlite bundles.
- **Data protection:** files created in the app container get `NSFileProtectionCompleteUntilFirstUserAuthentication` by default since iOS 7 — encrypted at rest, readable after first unlock post-boot. That is the correct class for a sync database (background/quick-resume friendly). Do **not** raise to `NSFileProtectionComplete`: SQLite WAL access after lock would break a suspended-but-alive sync loop. Optionally set the entitlement `com.apple.developer.default-data-protection` explicitly to document intent — but note data-protection entitlement tweaks are among those that *are* allowed on free personal teams (unlike push/App Groups).
- `Platform::data_dir()` on iOS must resolve to the app sandbox (`Application Support` inside the container) via Tauri's path resolver — the existing `Platform` port (AD-24) is the seam; no core change, just an iOS-aware `MobilePlatform`/adjusted `DesktopPlatform` impl in the shell.
- Exclude the DB from iCloud backup (`isExcludedFromBackup`) — it is re-syncable state and can be multi-GB; a small Swift/objc2 call at first launch.

### 1.6 Keychain on iOS

- The repo currently uses `keyring = "3"` with `apple-native` (macOS Keychain) behind the `Platform::keychain_*` port. On iOS, keyring v3's apple-native backend targets the iOS keychain via `security-framework`, and the keyring project's newer architecture (keyring-core + `apple-native-keyring-store`) makes it explicit: **iOS apps use the "Protected Store" (iOS keychain / kSecClassGenericPassword)**; the macOS-file-based Keychain API is unavailable on iOS. Expect the `keyring` crate to work with the caveat that iOS requires the app to be signed with a keychain-access-group (automatic with any valid signing, including personal team).
- **Verify in the walking skeleton**; fallback options if keyring v3 misbehaves on iOS: (a) direct `security-framework` calls in an `IosPlatform` impl (~40 lines for get/set/delete of generic passwords), (b) community `tauri-plugin-keyring`/`tauri-plugin-keychain`. Because keeper isolated keychain behind the `Platform` port, this is a contained swap.
- Keychain items default to `kSecAttrAccessibleWhenUnlocked`; for tokens the sync loop needs after backgrounding, set `AfterFirstUnlockThisDeviceOnly` (Element X does the same for NSE access). ThisDeviceOnly also prevents iCloud keychain sync of Matrix device keys (desired: a Matrix device must not be cloned).

### 1.7 WKWebView constraints (vs desktop WKWebView)

Same engine family as macOS keeper, so CSS/JS parity is high. iOS-specific deltas:

- **Safe areas:** iOS auto-adjusts `WKWebView.scrollView.contentInsets`, which fights CSS layout. Standard 2026 fix (documented for Tauri 2 specifically): `viewport-fit=cover` in the viewport meta + set `contentInsetAdjustmentBehavior = .never` (community plugin `tauri-plugin-ios-webview-insets` or 5 lines of Swift in the generated project) + own all padding in CSS via `env(safe-area-inset-*)` + match `backgroundColor` in tauri.conf to the theme.
- **Keyboard:** `window.innerHeight` does not shrink when the keyboard shows; `position: fixed` bottom bars (the composer!) get covered. Use the `visualViewport` API to compute the keyboard inset and drive a CSS var; avoid nested fixed/overflow-hidden wrappers around the composer or WKWebView's scroll-into-view will overshoot. (iOS 17+/modern WebKit also supports `interactive-widget=resizes-content` in the viewport meta — test both.)
- No downloads/file-save UX like desktop; media save goes through share sheet (later story).
- Memory: WKWebView content process is separate from the app process but counts toward overall pressure; heavy image grids should stay virtualized (already the norm in the codebase).

### 1.8 App lifecycle: suspension, background, sync

- iOS suspends the process shortly (~seconds, up to ~30 s with `beginBackgroundTask`) after backgrounding. **There is no long-lived background socket**: sliding-sync long-polls die on suspend. True background delivery requires APNs push (paid account) — out of scope for the free-signing phase; accept "sync runs foreground-only".
- Tauri core exposes only a generic `RunEvent::Resumed`; mobile pause/resume is not first-class in core. Two practical hooks: (a) **webview side** — `document.visibilitychange` fires reliably in WKWebView on background/foreground; forward to Rust via a command (`lifecycle_set_foreground(bool)`); (b) **native side** — a micro Swift plugin observing `UIApplication.didEnterBackgroundNotification`/`willEnterForegroundNotification` (community precedent: `tauri-plugin-app-events`). Prefer (b) for correctness, (a) as the zero-native stopgap.
- On background: gracefully pause the `SyncService` (matrix-sdk-ui supports stop/offline-mode transitions) so the HTTP long-poll isn't killed mid-flight; on foreground: resume + trigger an immediate sync. matrix-sdk-ui's SyncService exits offline mode and immediately re-syncs on demand — the same machinery keeper's shell can call.
- Must-test: the "blank webview after resume" bug (§1.1); mitigation patterns exist (reload on visibilitychange if the webview process was jettisoned).

### 1.9 Verdict on AD-24: Plan A vs Plan B

Everything found in this research supports staying on **Plan A (Tauri mobile reusing keeper-core + the same IPC contract)**:

- The two hard dependencies of keeper's architecture — custom URI-scheme media transport and Rust-side sqlite/E2EE stores — are *native strengths* on iOS (WKURLSchemeHandler; Element X ships the same SDK/store stack).
- The gaps are all peripheral (desktop-only plugins, lifecycle hooks, safe-area glue) and each has a documented pattern or a sub-100-line Swift/cfg fix.
- Plan B (UniFFI bindings + SwiftUI shell) would forfeit the entire React UI, the ts-rs contract, and the zustand mirror layer for no capability gain in this phase — push/NSE, the one thing a native shell would eventually do better, is blocked by licensing (free signing) rather than by Tauri.
- **Trigger conditions to revisit Plan B** (record in the decisions ledger): (a) the blank-webview-on-resume class of bugs proves unfixable/recurring across Tauri releases; (b) NSE work begins and the 24 MB extension needs matrix-sdk in a second process — note the NSE would be a Rust+Swift target *regardless* of Plan A/B, so even that is not a shell rewrite.

---

## 2. Distribution without a paid Apple Developer Program

### 2.1 Free Apple ID "Personal Team" signing (recommended baseline)

Xcode with any Apple ID grants a **Personal Team**:

- **Limits:** provisioning profiles expire after **7 days**; max **10 App IDs** per 7-day window; ~**3 devices** registered; no TestFlight/App Store.
- **Blocked entitlements:** push notifications (APNs), App Groups, iCloud/CloudKit, associated domains (universal links), Sign in with Apple, Apple Pay, most background modes. Enabling any of these fails the build with signing errors. Consequences for keeper: **no APNs push, no NSE (needs App Group to share the store), no `https://` universal links** — `keeper://` custom-scheme deep links still work (custom URL schemes need no entitlement).
- **Works fine for a Tauri app:** set `bundle.iOS.developmentTeam` in `tauri.conf.json` (or `TAURI_APPLE_DEVELOPMENT_TEAM` env var) to the personal team ID; Xcode "Automatically manage signing" handles the rest. On-device: trust the developer cert under Settings → General → VPN & Device Management, and enable **Developer Mode** (iOS 16+: Settings → Privacy & Security → Developer Mode; appears after first Xcode connect/deploy attempt; requires a reboot).
- **Re-arm cadence:** after 7 days the app stops launching; re-run `tauri ios dev` (or hit Run in Xcode) to refresh — data persists across re-signs as long as the bundle ID is unchanged. For a dev testing on their own phone this is a 30-second weekly chore.
- Gotchas: keep the bundle identifier stable (`dev.tgorka.keeper` is fine — alphanumerics + dots; avoid hyphens/underscores in the iOS bundle ID) and machine-specific profiles can conflict if the same Apple ID signs from multiple Macs.

### 2.2 Sideloading landscape, mid-2026

| Tool | State (2026-07) | Fit for keeper |
|---|---|---|
| **Xcode / personal team** | Fully supported, 7-day profiles | **Best for the owner-developer** (already has the Mac + repo) |
| **AltStore Classic** | Works worldwide, iOS 12–18+; AltServer on the Mac auto-refreshes the 7-day signatures over Wi-Fi | Nice QoL upgrade: same free-Apple-ID signing but **auto-refresh** removes the weekly chore |
| **SideStore** | AltStore fork; on-device refresh via WireGuard loopback + minimuxer, no computer needed after setup | Best "no Mac nearby" option; slightly fiddlier setup |
| **Sideloadly** | Actively maintained PC/Mac signer, iOS 18+ supported; drag-IPA-and-sign with free Apple ID (same 7-day expiry) | Good for installing a CI-built IPA without opening Xcode |
| **TrollStore** | **Dead for modern devices** — CoreTrust exploit patched in iOS 17.0.1; only useful ≤ iOS 17.0 | Ignore unless the test iPhone is frozen on ≤17.0 (then: permanent installs, no expiry) |
| **AltStore PAL** | EU (+ Japan, Brazil) alternative marketplace; now free (Epic MegaGrant covers Apple's CTF); apps must be **Apple-notarized**, which requires a **paid** developer account | Not usable without the paid program; relevant later as an EU distribution channel for keeper releases |

- **Unsigned IPA → sign later:** works. `tauri ios build` produces a normal IPA; community practice is to export unsigned (or dev-signed) and re-sign with **zsign** (cross-platform, no Xcode) or let **Sideloadly** re-sign on install with the user's own Apple ID. This is the right shape for sharing test builds with other free-account testers. Note Tauri had issues building IPAs with *manual* signing configs ([tauri#10668](https://github.com/tauri-apps/tauri/issues/10668)) — keep "automatic signing + re-sign afterwards" as the flow.

### 2.3 Recommendation

**Primary:** free personal team + `tauri ios dev` from the existing Mac; accept 7-day re-arm; enable Developer Mode once. **Secondary (QoL):** install AltServer so the signature auto-refreshes over Wi-Fi. **For distributing test IPAs later:** `tauri ios build --export-method debugging` → re-sign via Sideloadly/zsign per-tester. Budget decision for the *push* epic: APNs is impossible without the $99/yr program — defer, and revisit paid enrollment only when push/NSE becomes a goal (that also unlocks TestFlight, App Groups, and AltStore PAL notarization).

---

## 3. Matrix client on iOS — specifics

### 3.1 Push (what Element X does; what keeper skips)

Element X iOS: APNs → **Sygnal** push gateway (run by the homeserver operator, configured with the app's APNs key) → app receives `{room_id, event_id}` → a **Notification Service Extension** wakes, restores the session from the shared **App Group** container + keychain, decrypts the event via matrix-rust-sdk's NotificationClient (multiprocess crypto-store locking), and rewrites the notification content. Every ingredient — APNs entitlement, App Group, NSE target, published app identity in a Sygnal deployment — requires the paid program. **Keeper iOS phase-1 stance: no push; local notifications only while foregrounded (the existing tauri-plugin-notification path), badge count on app icon updated on foreground sync.** This matches AD-24's "iOS needs NSE + push decisions out of MVP scope".

### 3.2 E2EE store

Same `matrix-sdk-sqlite` crypto store as desktop; iOS specifics are (a) file protection class (§1.5), (b) keychain accessibility class for the store cipher/session secrets (§1.6), (c) *later, with NSE*: the crypto store must move into an App Group container with the SDK's cross-process lock — a directory-layout decision worth keeping in mind now (keep all account state under one `data_dir()` root so a future container move is a path change in `Platform`, not a migration of scattered files).

### 3.3 Memory (jetsam)

- Foreground app limit on modern iPhones is generous (~2 GB+); the real killers are (a) NSE's hard **24 MB** limit (future problem, paid tier only) and (b) background-suspended footprint — iOS evicts big suspended apps first, so keeper should drop caches (image memory cache, media byte buffers) on `didEnterBackground`/memory-warning.
- matrix-rust-sdk is proven in this envelope (Element X runs it on iOS with tens of accounts' worth of rooms); keeper's single-process model (no separate node/sidecar) is jetsam-friendly. The in-memory Range slicing of large media (§1.4) is the one flagged hotspot.
- **Sidecar port:** iOS forbids spawning child processes — `Platform::sidecar_path`/bbctl bridge management is **desktop-only**; the iOS shell must return a clean "unsupported on this platform" error and the UI must hide bridge-management affordances (capability flag over the existing IPC contract).

### 3.4 Sliding sync on flaky mobile networks

- keeper uses matrix-sdk-ui 0.18's SyncService (Simplified Sliding Sync, MSC4186, native in Synapse ≥ 1.114 — already keeper's documented default). SSS is designed for mobile: server-side sorting/filtering, small initial payloads, fast resume from a `pos` token.
- SDK behavior on bad networks: SyncService distinguishes running/terminated/error states and (0.16+) has an **offline mode** that backs off and exits immediately when the app asks to sync again (e.g. on foreground). Known rough edge tracked upstream: after being offline "a while" the SDK may need a recovering/expired-session restart of the sync loop ([matrix-rust-sdk#3935](https://github.com/matrix-org/matrix-rust-sdk/issues/3935)) — keeper's shell should treat "foreground + last sync > N min ago" as "show cached UI instantly, kick sync, surface a subtle 'connecting' state", which the snapshot-then-diff store architecture (AD-8) already supports for free: the UI always renders from the local mirror.
- Cellular↔Wi-Fi transitions: rely on the OS killing the socket + SyncService retry rather than custom reachability code in phase 1; add `NWPathMonitor`-driven fast-retry later if observed sluggish.

---

## 4. iPhone UX for this codebase

### 4.1 From 3-pane shell to single-pane stack

Current shell already has responsive seams (`src/hooks/use-shell-layout.ts`): sidebar → 48px rail below **1080px**, detail panel → Sheet below **1280px**, plus a `use-mobile` hook. iPhone (~390–430 pt logical width) needs one more tier rather than a rewrite:

- Add a third breakpoint (`phone`, e.g. < 768px) to `useShellLayout`. At phone width: render a **navigation stack** — Inbox list (full screen) → Room timeline (full screen, push) → Room/detail info (push or sheet). Same React components (InboxList, ChatView, DetailPanel), new *arrangement* container; zustand selection state (`selectedRoomId` etc.) already models "which pane is active", so the stack is a projection of existing state, not new state.
- Back navigation: top-left back chevron + **edge-swipe back**. WKWebView does not give native `UINavigationController` swipe for an in-page stack; implement swipe-back as a touch/pointer gesture on the stack container (or enable WKWebView's `allowsBackForwardNavigationGestures` only if adopting history-based routing — keeper has no router today, so prefer the in-page gesture + `history.pushState` integration so the hardware/system back gesture maps sensibly).
- Sidebar (account/space rail) becomes a leading drawer or a row inside the inbox header on phones; command palette (Epic 9) maps to pull-down search.

### 4.2 iOS mechanics

- **Safe areas:** `viewport-fit=cover` + `contentInsetAdjustmentBehavior = .never` + `env(safe-area-inset-*)` padding on the header, composer, and sheets (§1.7). Tailwind v4: expose as CSS vars in the theme (`--safe-top` etc.).
- **Keyboard avoidance:** composer bar bottom-anchored with `bottom: calc(var(--kb-inset, 0px) + env(safe-area-inset-bottom))`, `--kb-inset` driven by `visualViewport` resize/scroll listeners; pin the timeline scroll to bottom on keyboard-open when already at bottom. Test `interactive-widget=resizes-content` as the simpler path on current WebKit.
- **Touch targets & HIG essentials for a chat app:** ≥ 44×44 pt tappables; swipe actions on inbox rows (archive/mute — components exist as context-menu actions today, add swipe bindings); pull-to-refresh on the inbox (maps to "kick sync now"); long-press on message bubble → context menu (already exists for right-click; unify via `pointer`/`contextmenu` events which WKWebView synthesizes on long-press); disable iOS text-selection callout/tap-highlight where it fights custom menus (`-webkit-touch-callout`, `-webkit-tap-highlight-color`); momentum scrolling is default in modern WKWebView; respect Dynamic Type at least via `rem`-based sizing; dark mode already handled by the theme system.
- **Overscroll/bounce:** contain the timeline scroller (`overscroll-behavior: contain`) so rubber-banding doesn't tug the whole shell.
- Window config: the desktop `minWidth: 940` and `titleBarStyle: Overlay` in `tauri.conf.json` are ignored/invalid on iOS (single fullscreen scene) — keep the desktop window block, iOS needs no window config.

---

## 5. Recommendations — epic/story breakdown for the iOS phase

Signing/distribution decision up front: **free personal team via Xcode, 7-day re-arm, AltServer optional for auto-refresh; no push/NSE this phase** (revisit paid program as its own decision gate later).

### Epic 12: iOS Walking Skeleton — Build, Sign, Run (validates AD-24 Plan A before UI investment)

- **12.1 — Project init & toolchain.** `tauri ios init` in the workspace: generate `gen/apple` under `crates/keeper`, commit it (minus `build/` artifacts, gitignored); document Xcode/CocoaPods/rust-target prereqs.
  - AC: `tauri ios dev` opens the app in the iOS Simulator showing the existing login screen; `project.yml` diffs reviewed and committed; repo builds for desktop unchanged (`bun run check`, `check:rust`, `test:rust` green).
- **12.2 — Desktop/mobile compile seam.** cfg-gate the desktop-only surface: `tray` module + `tray-icon` cargo feature, global-shortcut, autostart, updater, window-state plugins behind `#[cfg(desktop)]`; iOS shell registers notification + deep-link only. `Platform::sidecar_path` returns a clean Unsupported error on iOS, and a capability flag in the IPC handshake lets the UI hide bridge-management affordances.
  - AC: `cargo check --target aarch64-apple-ios` passes for the whole workspace; desktop build behavior byte-identical; bridge-management UI hidden when the capability flag is off.
- **12.3 — iOS `Platform` wiring.** `data_dir()` → app container (Application Support); keychain via the existing `keyring` port on iOS (spike: verify set/get/delete on-device; contained fallback = direct `security-framework` generic-password calls, `AfterFirstUnlockThisDeviceOnly`); exclude the DB directory from iCloud backup.
  - AC: token survives app relaunch; keychain item invisible to other apps; DB files carry `NSFileProtectionCompleteUntilFirstUserAuthentication`; backup-exclusion flag verified.
- **12.4 — First run on the owner's iPhone (free signing).** `bundle.iOS.developmentTeam` in conf (or `TAURI_APPLE_DEVELOPMENT_TEAM` env), enable Developer Mode, trust the personal-team cert on device.
  - AC (the AD-24 gate): OIDC login completes via `keeper://` deep-link callback on-device; room list loads; send/receive text in one E2EE room; app relaunch restores the session without re-login.
- **12.5 — Media protocol on iOS.** Verify `keeper-media://` through WKURLSchemeHandler.
  - AC: encrypted image renders in the timeline; video plays and *seeks* (Range/206 path exercised); retry-on-cache-miss path works after force-quit.

### Epic 13: iPhone Shell — Single-Pane Navigation

- **13.1 — `phone` layout tier + stack container.** Third breakpoint in `useShellLayout` (< 768px); stack navigation Inbox → Room → Detail reusing the existing InboxList/ChatView/DetailPanel components, driven by existing selection state; back chevron in the room header.
  - AC: desktop/tablet layouts unchanged at ≥ 768px; deep component tree renders identically (no forked chat components); back returns to inbox preserving scroll position.
- **13.2 — Safe areas.** `viewport-fit=cover` viewport meta, `contentInsetAdjustmentBehavior = .never` (inset plugin or Swift patch in `gen/apple`), `env(safe-area-inset-*)` padding on header/composer/sheets/overlays, theme-matched window background.
  - AC: no white/black bands at notch or home-indicator; sheets and overlays respect insets in portrait and landscape.
- **13.3 — Keyboard avoidance.** visualViewport-driven `--kb-inset` CSS var lifting the composer; timeline stays pinned to bottom when it was at bottom; evaluate `interactive-widget=resizes-content` as the simpler alternative.
  - AC: composer never hidden by the keyboard; no overshoot/fly-away on focus; dismissing the keyboard restores layout without stranded offsets.
- **13.4 — Touch idioms.** Long-press → existing context menus; edge-swipe back on the stack; swipe actions on inbox rows (archive/mute reusing existing actions); pull-to-refresh on inbox → kick sync; `-webkit-touch-callout`/tap-highlight suppression where custom menus exist.
  - AC: every context-menu action reachable by touch; all tappables ≥ 44 pt.

### Epic 14: iOS Platform Behavior

- **14.1 — Lifecycle.** Background/foreground detection (webview `visibilitychange` first; micro Swift plugin on `UIApplication` notifications if visibility proves unreliable) → pause/resume SyncService, immediate sync on foreground; reload-guard mitigation for the known blank-webview-on-resume bug.
  - AC: backgrounding stops the long-poll within seconds (verified in Console.app/proxy); foregrounding shows new messages < 2 s on Wi-Fi; overnight-suspended app resumes without a blank screen.
- **14.2 — Foreground notifications + badge.** tauri-plugin-notification local notifications while active; app icon badge = unread count updated on each sync; suppress notification for the currently visible room (reuse desktop logic).
- **14.3 — Memory hygiene.** Drop image/media caches on `didEnterBackground` and memory warnings; cap the in-memory media Range-slicing buffer; deferred-work ledger entry for disk-backed streaming of large video.
  - AC: memory graph in Xcode Instruments returns near-baseline after backgrounding; no jetsam kill while suspended during a 24 h soak with a large account.
- **14.4 — Flaky-network behavior.** SyncService offline-mode handling; "connecting" affordance rendered over the cached mirror (AD-8 snapshot-then-diff makes stale UI free); sync-loop restart on stale resume (cf. matrix-rust-sdk#3935).
  - AC: airplane-mode toggle recovers without restart; Wi-Fi↔cellular handover resumes sync unaided.

### Epic 15: iOS Fit & Finish + Release Hygiene

- **15.1** iOS icon set/launch screen/branding in `gen/apple`; launch background matches theme (no flash).
- **15.2** CI: `cargo check --target aarch64-apple-ios` (compile-only, no signing, runs on the existing macOS runner) as a required PR gate so desktop work can't silently break the port.
- **15.3** Docs: `docs/ios.md` — the 7-day re-arm ritual, AltServer auto-refresh option, Sideloadly/zsign re-sign flow for sharing test IPAs, known limitations (no push, no background sync, no bridge management on iOS).
- **15.4** Decision-gate story: paid Apple Developer Program go/no-go — unlocks APNs push + NSE + App Groups + TestFlight + AltStore PAL notarization; pulls in the Sygnal question (keeper is client-only, so push would ride Beeper's/homeserver's gateway or a user-run Sygnal — a PRD-level decision, not a story).

### Sequencing rationale

Epic 12 is deliberately UI-free: it retires the three existential risks (toolchain, signing, core-on-iOS) for roughly a week of work before any UX investment, exactly as AD-24 prescribes. Epic 13 is pure frontend and can proceed in parallel with Epic 14 once 12.4 passes. Epic 15 hardens what exists; nothing in it blocks daily dogfooding, which can start the day 13.3 lands.

### Risk register (top 5)

| # | Risk | Likelihood | Mitigation |
|---|---|---|---|
| 1 | Blank webview on resume ([tauri#14371](https://github.com/tauri-apps/tauri/issues/14371)) | Medium | Reload guard in 14.1; test first thing in 12.4; track upstream fix |
| 2 | `keyring` v3 misbehaves on iOS in this stack | Medium | Spike inside 12.3; contained fallback to direct `security-framework` behind the existing `Platform` port |
| 3 | Keyboard/scroll quirks in the composer (WKWebView) | High (quirks), Low (blocker) | Time-boxed 13.3; patterns well documented; `interactive-widget` fallback |
| 4 | 7-day expiry friction erodes dogfooding | Medium | AltServer auto-refresh (15.3); weekly `tauri ios dev` ritual documented |
| 5 | Large-media RAM slicing trips memory pressure | Low–Medium | Cap in 14.3; deferred-work item for disk-backed streaming |

---

## 6. Open questions (pre-answer candidates for the decisions ledger)

1. **Minimum iOS version?** Recommend iOS 16.0 (Developer Mode era, modern WebKit with `visualViewport` maturity; Element X requires 17+ — keeper has no need to chase that). Tauri's default deployment target is well below this; set it explicitly in `project.yml`.
2. **Same bundle ID (`dev.tgorka.keeper`) on iOS as macOS?** Yes — no App Group sharing exists to conflict, and it keeps deep-link registration coherent. Revisit only if the paid program + shared containers arrive.
3. **Router adoption?** The phone stack can be selection-state-driven without adding a router; adding `history.pushState` integration is a cheap enhancer for system back-gesture semantics. Recommend: no router dependency this phase.
4. **Android next?** Out of scope here, but two iOS-phase choices keep it cheap: the `phone` layout tier is platform-neutral, and a `convertMediaSrc`-style URL helper (needed for Android's `http://keeper-media.localhost` remapping) should be introduced only when Android starts — not speculatively.
5. **Paid program timing?** Only when push becomes a product goal; it is the single gate for APNs/NSE/App Groups/TestFlight and AltStore PAL notarization ($99/yr).

---

## Appendix A — Concrete config touchpoints in this repo

| File | Change for iOS phase |
|---|---|
| `src-tauri/crates/keeper/tauri.conf.json` | Add `bundle.iOS.developmentTeam` (or use `TAURI_APPLE_DEVELOPMENT_TEAM` env to keep the team ID out of git); mobile deep-link scheme config alongside the existing `deep-link.desktop.schemes` |
| `src-tauri/crates/keeper/Cargo.toml` | Move desktop-only plugin deps (`tauri-plugin-global-shortcut`, `-autostart`, `-updater`, `-window-state`) under `[target.'cfg(not(any(target_os = "ios", target_os = "android")))'.dependencies]`; same for `tray-icon` feature on `tauri` |
| `src-tauri/crates/keeper/src/lib.rs` | `#[cfg(desktop)]` around tray/global-shortcut/autostart/updater/window-state registration and the deep-link desktop registration path; keep notification + media protocol + IPC unconditional |
| `src-tauri/crates/keeper/src/platform*.rs` | iOS branch of the `Platform` impl: container `data_dir`, keychain accessibility class, `sidecar_path` → Unsupported |
| `src-tauri/crates/keeper/gen/apple/` (new, generated) | `project.yml` (deployment target, background color), `Info.plist` (`CFBundleURLTypes` for `keeper://`), safe-area Swift patch |
| `index.html` | Viewport meta: `viewport-fit=cover` (+ evaluate `interactive-widget=resizes-content`) |
| `src/hooks/use-shell-layout.ts` | Third `phone` tier (< 768px) |
| `src/` shell components | Stack-navigation container for the phone tier; safe-area/keyboard CSS vars in the Tailwind theme |
| `.github/workflows/` | `cargo check --target aarch64-apple-ios` PR gate |
| `docs/ios.md` (new) | Signing re-arm ritual, sideloading flows, platform limitations |

---

## Sources (selected)

- Tauri v2: [plugins list](https://v2.tauri.app/plugin/), [iOS code signing](https://tauri.app/distribute/sign/ios/), [App Store distribution](https://v2.tauri.app/distribute/app-store/), [mobile plugin development](https://v2.tauri.app/develop/plugins/develop-mobile/), [Tauri 2.0 release](https://v2.tauri.app/blog/tauri-20/), [iOS feedback discussion #10197](https://github.com/orgs/tauri-apps/discussions/10197), [issue #14371 blank webview on resume](https://github.com/tauri-apps/tauri/issues/14371), [issue #13818 export methods](https://github.com/tauri-apps/tauri/issues/13818), [issue #10668 manual signing](https://github.com/tauri-apps/tauri/issues/10668)
- Tauri-on-iOS field notes: [zudo-tauri wisdom — iOS project structure & free-team signing](https://takazudomodular.com/pj/zudo-tauri/docs/mobile/), [Mobalab — Tauri 2 iOS safe-area fix (2026-05)](https://engineering.mobalab.net/2026/05/13/tauri-2-on-ios-a-simple-fix-for-wkwebview-safe-area-inset-issues/), [tauri-plugin-app-events](https://github.com/wtto00/tauri-plugin-app-events)
- Apple: [free vs paid membership](https://developer.apple.com/support/compare-memberships/), [provisioning profile updates](https://developer.apple.com/help/account/provisioning-profiles/provisioning-profile-updates/), [WKURLSchemeHandler](https://developer.apple.com/documentation/webkit/wkurlschemehandler)
- Sideloading 2026: [AltStore PAL FAQ](https://faq.altstore.io/altstore-pal/what-is-altstore-pal), [AltStore PAL now free (Epic MegaGrant)](https://www.howtogeek.com/altstore-pal-is-now-free-for-eu-users/), [SideStore](https://sidestore.io/), [TrollStore status / iOS 17.0.1 CoreTrust patch](https://ios18apps.com/trollstore-ios-26/), [Sideloadly IPA signing](https://hackpuntes.com/posts/ios-ipa-signing-sideloadly/), [zsign](https://github.com/zhlynn/zsign), [unsigned IPA via Sideloadly](https://dev.to/oivoodoo/build-unsigned-ios-ipa-to-install-via-sideloadly-236f)
- Matrix/Element X: [Element X iOS notifications architecture (DeepWiki)](https://deepwiki.com/element-hq/element-x-ios/3.5-notifications), [Element push docs (Sygnal/APNs)](https://docs.element.io/latest/element-support/element-androidios-client-settings/understanding-push-notifications/), [Element X push requirements #1644](https://github.com/element-hq/element-x-ios/issues/1644), [matrix-rust-sdk SyncService](https://matrix-org.github.io/matrix-rust-sdk/matrix_sdk_ui/sync_service/struct.SyncService.html), [SSS recovering mode #3935](https://github.com/matrix-org/matrix-rust-sdk/issues/3935), [NSE 24 MB limit](https://blog.kulman.sk/dealing-with-memory-limits-in-app-extensions/)
- Rust/iOS platform: [keyring-rs Apple platforms (DeepWiki)](https://deepwiki.com/open-source-cooperative/keyring-rs/5.1-apple-platforms-(macos-and-ios)), [apple-native-keyring-store](https://crates.io/crates/apple-native-keyring-store), [security-framework](https://crates.io/crates/security-framework), [iOS data protection classes (OWASP MSTG)](https://github.com/MobSF/owasp-mstg/blob/master/Document/0x06d-Testing-Data-Storage.md)
- WKWebView UX: [WebKit bug 191872 safe-area timing](https://bugs.webkit.org/show_bug.cgi?id=191872), [fullscreen webview & notch guide](https://ruoyusun.com/2020/10/21/webview-fullscreen-notch.html), [iOS 11 viewport behavior](https://dpogue.ca/articles/ios11-viewport.html)
