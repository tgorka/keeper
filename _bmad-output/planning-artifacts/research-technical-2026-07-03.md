# Technical Research: keeper — Matrix Messenger Client (Beeper-like, client-only)

- **Date:** 2026-07-03
- **Author:** BMAD technical-research (Claude)
- **Project:** keeper — open-source Matrix messenger client, Tauri 2 + React + TypeScript + shadcn/ui, macOS-first, later iOS/Windows/iPad/Android/Linux. Client-only: no server-side components operated by the project.
- **Method:** Live web research (July 2026) — crates.io API, GitHub API, matrix.org TWIM, vendor docs/blogs. All version numbers verified against registries on 2026-07-03.

---

## 1. matrix-rust-sdk — Current State

### 1.1 Versions (verified on crates.io, 2026-07-03)

| Crate | Latest | Released | License |
|---|---|---|---|
| `matrix-sdk` | **0.18.0** | 2026-06-02 | Apache-2.0 |
| `matrix-sdk-ui` | **0.18.0** | 2026-06-02 | Apache-2.0 |
| `matrix-sdk-crypto` | **0.18.0** | 2026-06-02 | Apache-2.0 |
| `matrix-sdk-sqlite` | **0.18.0** | 2026-06-02 | Apache-2.0 |
| `matrix-sdk-base` | 0.18.0 (workspace lockstep) | 2026-06-02 | Apache-2.0 |

Release cadence has accelerated markedly: 0.14.0 (2025-09-04) → 0.16.0 (2025-12-04) → 0.17.0 + 0.16.1 backport (2026-05-08) → 0.18.0 (2026-06-02). Roughly a release every 1–3 months, with patch backports for security issues. The workspace uses **Rust edition 2024** since 0.16.0.

Recent changelog highlights:
- **0.18.0** — tile-server support, fix for a cyclic `Client` reference preventing cleanup, deterministic pinned-event sorting.
- **0.17.0** — new DM room definitions, OAuth caching, **encrypted history sharing on invites**, security fixes (reject invalid edits as latest-event candidates, sender-spoofing hardening).
- **0.16.0** — Rust 2024 edition, expanded OAuth/OIDC incl. **QR-code login (MSC4108)**.
- **0.13/0.14 era** — event-cache SQL-injection fix, thread subscriptions, sliding-sync refinements.

### 1.2 Maturity

The project self-describes as **production ready** and is the foundation of **Element X (iOS + Android), Fractal, and iamb**. The `matrix-sdk-ui` crate is the "batteries included" layer that Element X ships on. API churn is still real (0.x semver; breaking changes every minor), so pin exact versions and budget for upgrade work each release.

### 1.3 Sliding Sync / Simplified Sliding Sync (MSC4186)

- The SDK's `SyncService`/`RoomListService` are built on **native Simplified Sliding Sync (MSC4186)** — the old MSC3575 proxy design is dead (proxy sunset by the Foundation in 2024/2025).
- **MSC4186 entered Final Comment Period in June 2026** (TWIM 2026-06-26) — it is about to be merged into the spec as part of Matrix 2.0.
- Synapse ships SSS **enabled by default**. In Jan 2026 the API removed "sticky parameters" entirely, simplifying requests.
- Offline/cold-cache support exists (sliding-sync state persisted and restored across restarts; event cache persistence is on by default — see below).
- Sliding-sync extensions relevant to keeper: to-device (E2EE), e2ee, account-data, receipts, typing, **thread subscriptions (MSC4308, experimental)**, profile updates (MSC4262, proposed), threads extension (MSC4360, proposed).

### 1.4 E2EE

`matrix-sdk-crypto` is a standalone encryption state machine (same code that powers Element X) on top of **vodozemac** (Apache-2.0, audited olm/megolm implementation). Feature set: Olm/Megolm, cross-signing, interactive verification (SAS + QR), key backup, secret storage/recovery, **dehydrated devices (MSC3814)**, room-key sharing strategies, and (0.17+) **encrypted history sharing when inviting** — a Matrix 2.0 flagship feature. Data is at-rest-encryptable in the SQLite stores with a passphrase. This is the most battle-tested non-Element E2EE path in the ecosystem; do not attempt E2EE in JS.

### 1.5 High-level UI machinery (`matrix-sdk-ui`)

Everything keeper needs is already implemented as reactive Rust services:

- **`SyncService`** — orchestrates sliding sync + to-device sync, app-state aware (foreground/background).
- **`RoomListService`** — sorted/filtered room list as a stream of `VectorDiff` updates ("all rooms" list + dynamic filters). Designed exactly for a Beeper-style unified inbox.
- **`Timeline`** — per-room virtual timeline: aggregates edits/reactions/redactions, read receipts, local echo, pagination, day dividers, media. Emits `VectorDiff<TimelineItem>` streams.
- **`SendQueue`** — persistent outgoing queue with retry, offline resilience, media upload queueing, dependent-event handling (e.g. edit-before-send). This is the primitive to build a Beeper-style **undo-send window** (delay dispatch N seconds; abort = local delete).
- **`EventCache`** — persistent event storage (SQLite-backed, **enabled by default** after PR #4308), backing instant cold-start timelines.
- **`NotificationClient`** — resolves push/notification content (used by Element X NSE); useful later for iOS.
- **Media** — upload/download with progress, thumbnails, a media cache with retention policies in the SQLite store.
- **Widget driver** (`matrix_sdk::widget`) — implements the widget postMessage API, purpose-built to **embed Element Call** (there is dedicated `element_call.rs` settings code in-tree).

### 1.6 VoIP

No native WebRTC in the SDK (webrtc-rs support is an open issue, #3295, not started). The supported path — the one Element X ships — is **Element Call embedded as a widget**: the Rust SDK handles the Matrix side (MatrixRTC membership, widget transport), and a webview renders Element Call (LiveKit SFU underneath). For keeper (already webview-based) this is a natural fit; Element publishes an embeddable Element Call package. Caveat: Element Call app is AGPL — embedding it as a **separate process/iframe widget** (not linking) keeps license boundaries clean.

### 1.7 How Element X uses it, and Tauri embedding

Element X uses a dedicated `matrix-sdk-ffi` crate generating **UniFFI** bindings (Swift/Kotlin) in a three-layer architecture: pure Kotlin/Swift interfaces → thin Rust-wrapper implementations → UniFFI-exposed Rust SDK. All Matrix logic, crypto, storage and even timeline building live in Rust; the native layer is a renderer.

**For Tauri, no FFI is needed at all** — the Tauri backend *is* a Rust process. keeper can depend on `matrix-sdk`/`matrix-sdk-ui` directly as crates, which is strictly simpler than what Element X has to do. This is the single biggest architectural advantage of the Tauri choice.

Known prior art (GitHub, July 2026):
- **`IT-ess/tauri-plugin-matrix-svelte`** — a Tauri plugin wrapping matrix-rust-sdk (via a `matrix-ui-serializable` adapter) exposing Matrix state to Svelte stores. Small (8 stars) but active (pushed 2026-06-29); proves the pattern works, incl. on mobile. License unclear (NOASSERTION) — study, don't copy.
- **`element-hq/aurora`** — Element's experiment compiling matrix-rust-sdk to **WASM** for a next-gen Element Web/Desktop; originally started against Tauri. AGPL-3.0. Signal that even Element is converging on "rust-sdk everywhere".
- **`cinny-desktop`** — Cinny's desktop app is literally **Tauri** (with matrix-js-sdk in the webview) — useful as a Tauri packaging/config reference for a Matrix client, not for the data layer.
- No mature "Tauri + rust-sdk" client exists yet — **keeper would be first-mover** in this exact slot.

---

## 2. Existing Open-Source Clients Worth Studying

| Client | Stack | License | Status (July 2026) | What to take |
|---|---|---|---|---|
| **Element Web/Desktop** | React + matrix-js-sdk; Electron shell | **AGPL-3.0** (dual commercial); matrix-js-sdk itself is **Apache-2.0** | Very active (pushed daily) | Reference UX for full feature surface (spaces, threads, calls). AGPL → patterns only, no code. matrix-js-sdk is Apache but architecturally wrong for keeper (JS-side state/crypto). |
| **Element X iOS/Android** | SwiftUI / Jetpack Compose over matrix-sdk-ffi (UniFFI) | **AGPL-3.0** (dual) | Active; flagship rust-sdk consumers | The architecture blueprint: Rust owns state, UI renders view-models; Timeline/RoomList streams; NSE push design for later iOS work. |
| **Fractal** | GTK4/libadwaita, Rust, **direct matrix-rust-sdk embedding** | **GPL-3.0** | Active (GNOME) | Closest existing analog of "native Rust app embedding the SDK without FFI". Study session management, store setup, multi-account. GPL → no code copying. |
| **gomuks (2024+ rewrite)** | **Go backend (mautrix-go) + React web frontend + TUI frontend**, websocket API between them | **AGPL-3.0** | Active; v26.03 (March 2026) | The best *architectural* analog for keeper: headless messaging core + thin web UI over a socket. Tulir (Beeper's bridge author) built it — its patterns reflect Beeper realities. AGPL → patterns only. |
| **Cinny** | React web client; replacing matrix-js-sdk with an in-house SDK (PRs paused); desktop shell is **Tauri** | **AGPL-3.0** | Active (pushed 2026-06-28) | Cleanest chat UI in the ecosystem; `cinny-desktop` is a working Tauri Matrix-client config to study. |
| **Nheko** | Qt6/C++20 (mtxclient) | **GPL-3.0** | Active | Feature-density reference for power users; little to reuse technically. |
| **iamb** | Rust TUI on **matrix-rust-sdk** | **Apache-2.0** | Active (pushed 2026-04) | **License-compatible working code** using matrix-rust-sdk directly (login, verification, sync, rooms). The one client you can legally copy patterns *and* code from. |

### Beeper open-source repos (github.com/beeper, July 2026)

| Repo | License | Notes |
|---|---|---|
| `bridge-manager` (**bbctl**) | **Apache-2.0** | Self-hosted-bridge tooling against Beeper's homeserver; active (2026-06). Contains the (private) Beeper API client code — legally reusable. |
| `self-host` | Apache-2.0 | Full Beeper stack self-hosting docs (dormant since 2024-02). |
| `imessage` | AGPL-3.0 | Legacy Mac/phone iMessage bridge; dormant since 2024-04. |
| `platform-imessage` | **MIT** | **Standalone iMessage automation library for macOS** — active (pushed 2026-07-04). Highly relevant to a macOS-first client. |
| `linkedin` | Apache-2.0 | LinkedIn bridge (bridgev2). |
| `line` | MIT | LINE bridgev2 bridge, built for bbctl usage; active. |
| `desktop-api-js` / `desktop-api-python` | MIT | Official SDKs for the **Beeper Desktop API** (localhost REST/WS/MCP). |
| `babbleserv` | AGPL-3.0 | Beeper's next-gen Matrix homeserver on FoundationDB — being developed in the open. |
| Beeper Desktop / mobile apps | closed source | The clients themselves are not open source. |

**Key takeaway:** everything UI-shaped in the ecosystem is AGPL/GPL; the only permissive full-stack references are **iamb (Apache)**, **matrix-js-sdk (Apache)**, **bridge-manager (Apache)**, and Beeper's MIT SDK repos.

---

## 3. Beeper Specifics

### 3.1 How Beeper clients connect

- Beeper accounts are Matrix accounts on **`beeper.com`** (client API at `https://matrix.beeper.com`), served by **hungryserv** — Beeper's closed-source Go homeserver (per-user sharding; "megahungry"). It deliberately implements only a **subset of the Matrix C-S API** (bridge-manager README warns some spec-compliant appservices won't work). Its successor, **babbleserv** (FoundationDB), is developed openly under AGPL.
- Beeper's own docs state the API "is a standard Matrix homeserver interface" and historically supported using any Matrix client against your Beeper account.

### 3.2 Auth (what bbctl does — private API)

The login flow is passwordless email-code, via `https://api.beeper.com` (headers include the self-deprecating `Authorization: BEEPER-PRIVATE-API-PLEASE-DONT-USE`):
1. `POST /user/login` → returns `request` id + supported types,
2. `POST /user/login/email` (request id + email) → sends a code,
3. `POST /user/login/response` (request id + code) → returns a **login token (JWT)**,
4. that token is exchanged for a Matrix access token on `matrix.beeper.com` via the JWT login type (`org.matrix.login.jwt`) — this is exactly what `bbctl login` implements in Go (`api/beeperapi`, Apache-2.0, so the flow can be ported legally).

Risks: it's a **private, unversioned API**; Beeper can change it. Beeper does not run MAS/MSC3861 OIDC. keeper should isolate "Beeper auth" behind a provider interface next to standard Matrix password/OIDC login.

### 3.3 Bridges: cloud, self-hosted, on-device

- **Beeper Cloud bridges** — run in Beeper's cloud; chats exist as rooms on the Beeper homeserver (zero-access encrypted at rest). These are visible to any Matrix client logged into the account.
- **Self-hosted bridges via bbctl** — `bbctl register <name>` provisions an appservice registration against your Beeper account's hungryserv namespace; `bbctl run <name>` runs official bridges locally. Transport is **appservice-over-websocket** (bridgev2 native; `bbctl proxy` adapts plain-HTTP appservices), so no port-forwarding/TLS needed. ~15 official bridges supported. Rooms appear on the Beeper homeserver like cloud-bridge rooms → **fully compatible with a third-party client like keeper**.
- **On-device bridges (2025 relaunch)** — since **July 2025**, Beeper's flagship mode runs bridges *inside the apps* ("On-Device Connections": WhatsApp + Signal first, Telegram/Discord/more rolling out through 2026). Messages go device↔network directly, preserving native E2EE, and **do not touch Beeper's servers**.

**Critical implication for keeper:** on-device-connection chats are **not on matrix.beeper.com**, so a third-party Matrix client cannot see them via the Matrix C-S API. A Beeper-account integration therefore covers: Matrix-native chats + cloud bridges + bbctl self-hosted bridges — but *not* the user's on-device chats. Options:
1. Position keeper's Beeper mode around **bbctl-style self-hosted/local bridges owned by keeper** (the "client-only, bring-your-own-bridges" story — closest to Beeper's own architecture),
2. Optionally integrate the **Beeper Desktop API** (below) to read/write on-device chats *if Beeper Desktop is installed* — a pragmatic companion mode, not a foundation.

### 3.4 Beeper Desktop API (2025/2026)

Beeper Desktop embeds a local API + MCP server: `http://localhost:23373` (REST, OpenAPI), `ws://localhost:23373/v1/ws` (realtime), `/v0/mcp` (MCP). Auth is **OAuth 2.0 + PKCE** or manual access tokens (`BEEPER_ACCESS_TOKEN`). Official MIT-licensed TS/Python SDKs and a CLI (`beeper/cli`) exist. Marked experimental. Covers all networks incl. on-device ones — the sanctioned third-party surface for the modern Beeper stack.

### 3.5 Bridge bounty program

Since Oct 2025 Beeper pays **up to $50k** for new **bridgev2 (Go) bridges under permissive licenses** (targets: WeChat, Viber, Snapchat, Teams, LINE, dating apps). Signals: bridgev2 is the stable interface to build against, and permissively-licensed bridges are becoming more common (see `beeper/line`, MIT).

---

## 4. mautrix Bridges Ecosystem (2026)

All mautrix bridges are **AGPL-3.0** (bridge processes — fine to *run*, since keeper only talks Matrix to them; no linking). The shared framework `mautrix-go` is **MPL-2.0**. Monthly tagged releases (`vYY.MM`, e.g. v26.04). The **bridgev2 "megabridge"** architecture is the current generation; legacy bridges are being rewritten onto it.

| Bridge | Status (mid-2026) |
|---|---|
| **whatsapp** | bridgev2, flagship, very active (pushed 2026-07-02); unofficial multi-device API. |
| **telegram** | **Go/bridgev2 rewrite released v26.04** (topic groups, sticker/emoji import); replaces the old Python bridge. |
| **signal** | bridgev2, active. |
| **discord** | Legacy Go; **rewrite in progress**, ETA "months" as of Apr 2026. |
| **slack** | bridgev2, active. |
| **meta** (Messenger + Instagram) | bridgev2, active; marketplace-chat support added 2026. |
| **twitter** | bridgev2, active. |
| **gvoice**, **gmessages** | bridgev2, active. |
| **bluesky** | bridgev2, active. |
| **linkedin** | maintained at `beeper/linkedin` (Apache-2.0, bridgev2). |
| **imessage** | `mautrix/imessage` legacy (Mac-relay based, low activity); Beeper's practical path is `beeper/platform-imessage` (MIT, macOS automation) + on-device logic. For keeper on macOS this repo is gold. |
| **email** | No first-class mautrix email bridge; ecosystem alternatives (e.g. postmoogle) exist outside mautrix. |
| Relay mode | bridgev2 relay significantly improved in v26.04 (implicit relay defaults, bridging existing chats). |

### Client-side requirements to manage bridges

- **Bridge bot commands** — each bridge exposes a bot DM (`login`, `list-logins`, `logout`, `resolve-identifier`, `start-chat`, `set-relay`…). A client can simply render these chats; a *great* client (keeper goal) wraps them in native UI by sending the same commands programmatically.
- **bridgev2 provisioning API** — bridgev2 exposes a standardized HTTP provisioning API (login flows as JSON state machines: display QR, enter code, etc.). This is what Beeper's own clients use and the right target for keeper's native "connect WhatsApp" UX.
- **bbctl** — for the Beeper-hosted case: keeper can shell out to `bbctl` (Apache-2.0, single Go binary) or port its logic to Rust to register/run local bridges against a Beeper account. Bundling `bbctl` as a Tauri sidecar binary is a legitimate v1 strategy.

---

## 5. Matrix Protocol Features Needed by keeper

| Feature | Status July 2026 | keeper notes |
|---|---|---|
| **Spaces** | Stable in spec | RoomListService filters; low priority for a DM-centric Beeper-like UX. |
| **Threads** | Stable; **MSC4306 thread subscriptions** + **MSC4308 (SSS extension)** experimental in Synapse & rust-sdk (`Room::subscribe_thread()` etc.); MSC4360 sliding-sync threads extension proposed | SDK support already usable behind flags. |
| **Reactions / edits / redactions** | Stable; Timeline API aggregates them automatically | Free with `matrix-sdk-ui`. |
| **Media** | Stable; SDK handles upload/download, thumbnails, cache retention | Encrypt/decrypt handled in Rust; stream to webview via custom protocol handler. |
| **VoIP** | Legacy 1:1 calls (m.call.*) stable; **MatrixRTC (MSC4143) not yet merged** (no FCP; experimental `/rtc/transports` endpoint in Synapse, auth/namespace bugs actively being fixed through 2026); Element Call (LiveKit, MSC4195) is the production implementation; multi-SFU federation landed June 2026 | Embed Element Call widget via rust-sdk widget driver — do not build native VoIP. |
| **Local echo** | Timeline API built-in | Free. |
| **Drafts** | No merged MSC; rust-sdk persists **composer drafts** per-room in the state store (`Room::save_composer_draft`, used by Element X); cross-device drafts = custom `account_data` | Local drafts free via SDK; cross-device optional later. |
| **Undo-send** | Not a protocol feature; implement client-side by delaying `SendQueue` dispatch (plus redaction fallback after send) | SendQueue makes this clean. |
| **Multi-account** | SDK: one `Client` per account, separate store dirs; Element X does this on mobile | Run N clients in the Tauri backend; unified inbox = merge RoomList streams in Rust. |
| **Sliding sync** | **MSC4186 in Final Comment Period (June 2026)**, default-on in Synapse; supported by Beeper's hungryserv lineage (Beeper was an original sliding-sync pioneer) | Non-SSS servers (Conduit forks vary; tuwunel/continuwuity implementing) need the SDK's fallback story — verify per-homeserver support at login. |
| **Push (macOS)** | No third-party APNs without shipping your own push infra. Desktop reality: keep a background process + local notifications | Tauri: `tauri-plugin-notification` + tray + login-item autostart; the Rust sync loop generates notifications (`NotificationClient` for content). APNs/sygnal only becomes necessary for the iOS phase. |
| **Auth / OIDC** | **MSC3861 suite done**: matrix.org migrated to **MAS** on 2025-04-07; OIDC MSCs passed FCP and are in the spec; rust-sdk has full OAuth support incl. QR login (MSC4108). **Beeper does NOT use MAS** — custom JWT email-code flow (§3.2) | keeper needs three login paths: password (legacy), OAuth/MAS (matrix.org & modern servers — SDK-provided), Beeper JWT (custom provider). |

---

## 6. Tauri 2 (July 2026)

### 6.1 Versions & maturity

- Core **tauri 2.11.5**; CLI 2.11.4; bundler 2.9.4; wry 0.55.x, tao 0.35.x. Stable since Oct 2024; mature and widely shipped on desktop.
- **Mobile (iOS/Android) shipped in v2** and matured through 2025–2026: same Rust core across desktop+mobile, capability-based permission model. Still: not all plugins are mobile-ported; treat iOS as "supported but expect rough edges" — fine for keeper's phased roadmap (macOS first).

### 6.2 Plugins keeper needs (all official, plugins-workspace v2)

`notification` (macOS/iOS/Android), `deep-link` (matrix.to / keeper:// links), `global-shortcut` (quick-switcher), `sql` **or none** — prefer letting matrix-sdk-sqlite own the DB and keep app settings in `store`; `updater` (signed updates; desktop), `single-instance` (Windows/Linux; macOS is single-instance by default), plus `autostart`, `window-state`, `clipboard-manager`, `opener`. For iOS later: `notification` + APNs entitlements handled via Xcode project Tauri generates.

### 6.3 IPC patterns for streaming sync updates

- **Commands** (request/response, async, typed via serde) for actions: send message, join room, login.
- **Channels** (`tauri::ipc::Channel<T>`) for **streaming**: the idiomatic v2 mechanism for high-frequency ordered data — one channel per subscription (room list stream, per-open-timeline stream, sync status). Map rust-sdk `VectorDiff` batches → serialized diff ops → apply to a frontend store (Zustand/Jotai). This mirrors exactly how Element X consumes the SDK, but over Tauri IPC instead of UniFFI callbacks.
- **Events** (`emit`/`listen`) for low-frequency broadcasts (notifications, account state).
- IPC payloads are JSON by default (v2 supports raw binary responses for commands); for media, bypass IPC entirely with a **custom URI scheme protocol handler** (`keeper-media://<mxc>`) streaming decrypted bytes from the Rust media cache — no base64 through the bridge.
- Performance: keep the entire message DB and all state in Rust; the webview receives only *view models* for visible ranges (Timeline API already does windowing/pagination). This is the difference between keeper scaling to Beeper-size accounts (100k+ events) and dying like Electron clients that hold state in JS.

### 6.4 macOS packaging

Developer ID signing + **notarization** (App Store Connect API key recommended; adds 2–5 min per CI build), hardened runtime; universal binary or aarch64-only initially. `tauri-action` GitHub Action handles build+sign+notarize+release. Ad-hoc signing acceptable only for local dev on Apple Silicon. Updater plugin requires signing update artifacts with the Tauri updater key.

---

## 7. Rust-based Fast Dev Tooling (2026)

| Concern | Recommendation | State of the art (mid-2026) |
|---|---|---|
| JS lint+format | **Biome 2.x** (single tool, Rust, type-aware lints via its own inference engine — no tsc; ~8.8M weekly downloads) | Alternative: **oxlint** (type-aware via tsgo since 2026, 2× faster) + **oxfmt** (3× faster) — two tools, fewer rules, best raw speed. Either is fine; Biome wins on one-tool DX for a small team. |
| Package manager | **pnpm 10** (monorepo-safest, strict, ecosystem-aligned with Turborepo/moon) | Bun install is 3–5× faster but monorepo/workspace support still trails; revisit if CI install time hurts. Don't adopt Bun runtime just for the package manager. |
| TS type check | `tsc --noEmit` in CI (or tsgo as it stabilizes) | Biome/oxlint don't replace full type-checking. |
| JS tests | **Vitest 4** | Standard for Vite/React in 2026. |
| Git hooks | **lefthook** (Go, fast, parallel, YAML) | Preferred over husky (Node startup cost) per "rust/go fast tools" principle. |
| Rust | `cargo clippy -D warnings`, `cargo fmt --check`, **cargo-nextest** (fast test runner), **cargo-deny** (licenses + advisories — enforce "no GPL/AGPL crates" policy mechanically) | cargo-deny is the license-contamination firewall. |
| CI | GitHub Actions: `macos-15`/`macos-14` (arm64) runners; `tauri-action` for bundle+notarize; `Swatinem/rust-cache`; matrix later for Windows/Linux | Keep notarization creds in encrypted secrets (App Store Connect API key). |

---

## 8. Recommendations

### 8.1 Recommended architecture: Rust core + thin TS UI

```
┌─────────────────────────────────────────────────────────────┐
│ Tauri app (single process + webview)                        │
│                                                             │
│  Rust backend ("keeper-core")                               │
│   ├─ matrix-sdk 0.18 + matrix-sdk-ui (SyncService,          │
│   │   RoomListService, Timeline, SendQueue, EventCache)     │
│   ├─ matrix-sdk-sqlite (state + crypto + events + media)    │
│   ├─ N Clients = N accounts → unified inbox merge in Rust   │
│   ├─ auth providers: password | OAuth/MAS | Beeper-JWT      │
│   ├─ bridge manager: bridgev2 provisioning client,          │
│   │   optional bbctl sidecar for Beeper self-hosted bridges │
│   ├─ notification engine → tauri-plugin-notification        │
│   └─ keeper-media:// custom protocol (decrypted media)      │
│                                                             │
│  IPC: commands (actions) + Channels (VectorDiff streams)    │
│                                                             │
│  React + TS + shadcn/ui frontend ("keeper-ui")              │
│   └─ pure renderer of Rust view-models; zero Matrix logic,  │
│      zero crypto, zero message storage in JS                │
└─────────────────────────────────────────────────────────────┘
```

This is Element X's proven three-layer design with UniFFI deleted (Tauri backend is already Rust) — simpler than any shipping rust-sdk client. It carries to iOS/Android via Tauri mobile with the same core.

### 8.2 Recommended versions (as of 2026-07-03)

- `matrix-sdk = "0.18.0"`, `matrix-sdk-ui = "0.18.0"`, `matrix-sdk-sqlite = "0.18.0"` (crypto comes transitively; pin exact, expect breaking minors; track upstream monthly).
- `tauri = "2.11"`, plugins from plugins-workspace v2 (notification, deep-link, global-shortcut, store, updater, single-instance, autostart, window-state, clipboard-manager, opener).
- React 19 + Vite 7 + TypeScript 5.x, shadcn/ui + Tailwind 4, Zustand/Jotai for diff-applied stores.
- Tooling: pnpm 10, Biome 2.x, Vitest 4, lefthook, cargo-nextest, cargo-deny, tauri-action CI on macos arm64 runners.

### 8.3 Beeper strategy

1. **v1:** first-class *standard Matrix* client (matrix.org/MAS OIDC + password servers) — the safe foundation.
2. **v1.x:** Beeper account support: port bbctl's Apache-2.0 login flow (email code → JWT → `org.matrix.login.jwt` on matrix.beeper.com); clearly flag it as an unofficial private API. Covers Matrix chats + cloud-bridge + bbctl-bridge rooms. **On-device-connection chats will not appear** — document this loudly.
3. **v2 options:** native bridgev2 provisioning UI for self-hosted/local bridges (keeper as "bring-your-own-bridges Beeper"); optional Beeper Desktop API companion mode (localhost:23373, OAuth+PKCE, MIT SDKs) for on-device chats; `beeper/platform-imessage` (MIT) for a macOS iMessage story.

### 8.4 What to avoid

- **matrix-js-sdk in the frontend** — duplicates state, drags crypto into the webview, kills the perf story. One source of truth: Rust.
- **MSC3575 / sliding-sync proxy** — dead; MSC4186 only.
- **Native VoIP implementation** — years of work; embed Element Call widget instead.
- **Copying code from AGPL/GPL projects** — element-web/X, Cinny, gomuks, Fractal, Nheko, mautrix bridges, beeper/imessage. Study architecture, re-implement clean-room. Safe-to-copy references: **iamb (Apache-2.0)**, **bridge-manager (Apache-2.0)**, **matrix-js-sdk (Apache-2.0)**, Beeper's MIT repos.
- **Betting the core on Beeper private APIs** — isolate behind provider traits; Beeper can (and does) change them.
- **Electron-style state-in-JS** — the whole point of this stack is Rust-side data.
- **Holding 0.x SDK upgrades back** — falling multiple versions behind matrix-rust-sdk makes catch-up brutal; upgrade every release.

### 8.5 Licensing notes

- **matrix-rust-sdk, ruma, vodozemac: Apache-2.0/MIT** — keeper's core can itself be Apache-2.0/MIT (recommended: **Apache-2.0** for patent grant).
- **AGPL contamination:** element-hq code (element-web, element-x-*, element-call, aurora, element-desktop) is AGPL-3.0 (dual-licensed commercially). Any copied code would force keeper to AGPL and bar future dual-licensing. Same for Cinny, gomuks, mautrix bridges. *Running* AGPL bridges as separate processes, or embedding Element Call as an out-of-process widget/iframe, does **not** contaminate keeper.
- **MPL-2.0 (mautrix-go)** — file-level copyleft, only relevant if porting its files; avoid porting, re-implement.
- Enforce with **cargo-deny** (deny GPL/AGPL crates) + a PR checklist item for provenance of ported code.
- Beeper repos are mixed: bridge-manager/self-host/linkedin **Apache-2.0**; desktop SDKs/line/platform-imessage **MIT**; imessage/babbleserv/registration providers **AGPL-3.0** — check per-repo before reuse.

### 8.6 Key risks

1. **Beeper on-device pivot** (2025→2026) moves ever more chats *off* their Matrix homeserver — the "third-party Beeper client via Matrix" surface shrinks over time; the durable play is keeper + self-managed bridges (and optionally the Desktop API).
2. **matrix-rust-sdk 0.x churn** — continuous upgrade tax; mitigated by thin wrapper layer around SDK types.
3. **hungryserv partial C-S API** — test keeper against a real Beeper account early; don't assume spec compliance.
4. **Tauri iOS maturity** — validate a walking-skeleton iOS build early in the roadmap, before UI investment locks in desktop-only assumptions.
5. **MatrixRTC still pre-spec** — calls remain the most volatile feature area; keep VoIP behind the Element Call widget boundary.

---

## Sources

- crates.io API: matrix-sdk / matrix-sdk-ui / matrix-sdk-crypto / matrix-sdk-sqlite (versions & dates, 2026-07-03); https://github.com/matrix-org/matrix-rust-sdk (+ /releases, ARCHITECTURE.md)
- https://matrix.org/blog/2026/06/26/this-week-in-matrix-2026-06-26/ (MSC4186 FCP, Element Call multi-SFU); https://github.com/matrix-org/matrix-spec-proposals/pull/4186, /4143, /4306, /4308, /4360
- https://matrix.org/blog/2024/11/14/moving-to-native-sliding-sync/ ; https://matrix.org/blog/2025/04/morg-now-running-mas/ ; https://areweoidcyet.com/
- matrix-rust-sdk issues/PRs: #3280/#4308 (event cache persistence), #3295 (webrtc-rs), #4793 (Element Call widget), #5848 (thread subscriptions); https://element.io/blog/exploring-matrixrtc-real-time-communication-in-rooms/
- GitHub API license/activity checks (2026-07-03): element-hq/element-web, element-x-ios/android, element-desktop, aurora; cinnyapp/cinny; gomuks/gomuks; Nheko-Reborn/nheko; ulyssa/iamb; matrix-org/matrix-js-sdk; mautrix/* ; beeper/* (org listing)
- https://github.com/beeper/bridge-manager (README + api/beeperapi/login.go); https://developers.beeper.com/bridges, /bridges/self-hosting, /desktop-api, /desktop-api/auth; https://www.beeper.com/faq; https://blog.beeper.com/2025/10/28/build-a-beeper-bridge/ ; Beeper July-2025 relaunch coverage (blog.beeper.com, tmcnet)
- https://mau.fi/blog/2026-04-mautrix-release/ ; https://mau.fi/blog/2025-12-mautrix-release/ ; https://docs.mau.fi/bridges/
- https://v2.tauri.app/release/ (tauri 2.11.5 etc.), /distribute/sign/macos/, /distribute/pipelines/github/; github.com/tauri-apps/tauri-action; github.com/IT-ess/tauri-plugin-matrix-svelte; github.com/element-hq/aurora; github.com/cinnyapp/cinny-desktop
- Tooling comparisons (2026): pkgpulse.com Biome-vs-OXC & pnpm-vs-Bun guides; jsmanifest.com Biome/Oxlint 2026; solberg.is fast type-aware linting; dev.to Tauri v2 signing/notarization guides
