---
title: "Addendum: keeper Product Brief"
status: final
created: 2026-07-03
updated: 2026-07-03
---

# Addendum — keeper Product Brief

Depth that belongs in downstream documents (PRD, UX spec, architecture) or that earned a place but does not fit the 2-page brief. Sources: `product-inputs.md` (stakeholder requirements, including delegated decisions), `research-technical-2026-07-03.md`, `research-market-2026-07-03.md` — all in `_bmad-output/planning-artifacts/`.

## 1. Locked technical constraints (owner decisions — carry into architecture)

- **Stack:** Tauri 2 (2.11.x) + Rust core + React 19 + TypeScript + shadcn/ui (Tailwind 4). Apache-2.0 license; no credentials in repo (1Password `op` for dev creds); repo language English.
- **Matrix core:** matrix-sdk / matrix-sdk-ui / matrix-sdk-sqlite **0.18+** embedded directly in the Tauri Rust backend (no FFI). Use SyncService, RoomListService, Timeline, SendQueue, EventCache. One `Client` per account; unified inbox = merged RoomList streams in Rust.
- **Sync:** Simplified Sliding Sync (MSC4186) only; MSC3575 proxy is dead. Verify per-homeserver SSS support at login (Conduit-family servers vary).
- **Auth providers (three, behind one interface):** password (legacy), OAuth/OIDC via MAS (MSC3861, SDK-provided), Beeper email-code → JWT → `org.matrix.login.jwt` on matrix.beeper.com (port from Apache-2.0 bbctl `api/beeperapi`; flag as unofficial private API).
- **IPC pattern:** Tauri commands for actions; `tauri::ipc::Channel<T>` streaming `VectorDiff` batches for room list/timeline/sync status; events for low-frequency broadcasts; `keeper-media://` custom protocol streaming decrypted media (no base64 over IPC). All state lives in Rust; webview renders view models only.
- **Undo-send:** hold outgoing messages in a configurable delay window (default 10 s) before SendQueue dispatch; post-dispatch "delete for everyone" = redaction.
- **Incognito:** per-account/per-room toggle → `m.read.private` read receipts + suppressed typing indicators (+ presence where applicable). Note WhatsApp couples sending/seeing read receipts — surface this in UX copy.
- **Drafts:** local SQLite + mirrored per-room Matrix account data (`Room::save_composer_draft` covers local; cross-device via custom account_data). Designed so a future external agent can *propose* drafts approved in-app. No server-side draft component in this repo, ever.
- **Local archive:** matrix-sdk store + event cache in SQLite, plus FTS index and JSON/Markdown export; archive survives logout where feasible.
- **Bridge management:** render bridge-bot chats; drive them programmatically (login/list-logins/logout/resolve-identifier/start-chat/set-relay); prefer bridgev2 provisioning API (JSON login state machines) where available; optional bbctl as Tauri sidecar for Beeper self-hosted bridges.
- **Quality gates:** Biome 2.x, rustfmt + clippy `-D warnings`, Vitest 4, cargo-nextest, cargo-deny (license firewall: deny GPL/AGPL crates), lefthook, pnpm 10, GitHub Actions on macOS arm64 with tauri-action (sign + notarize).

## 2. Licensing firewall (provenance rules)

- **Safe to copy (license-compatible):** iamb (Apache-2.0, working matrix-rust-sdk client code), beeper/bridge-manager (Apache-2.0, incl. Beeper API client), matrix-js-sdk (Apache-2.0, reference only), Beeper MIT repos (platform-imessage, desktop-api SDKs, line).
- **Study only, never copy (AGPL/GPL):** Element Web/X/Desktop, Element Call, aurora, Cinny, gomuks, Fractal, Nheko, mautrix bridges, beeper/imessage, babbleserv. Running AGPL bridges as separate processes or embedding Element Call as out-of-process widget/iframe does **not** contaminate.
- **MPL-2.0 (mautrix-go):** file-level copyleft — re-implement, don't port files.
- Enforcement: cargo-deny in CI + PR checklist item for provenance of ported code.

## 3. Network risk tiers (ship as in-product labeling)

| Tier | Networks | Guidance |
|---|---|---|
| Low risk | Matrix (native), Telegram, Google Messages/Chat/Voice | Recommend by default |
| Maintenance-heavy | Signal, WhatsApp (personal use), Discord, Slack | Default-on with clear disclosure |
| Volatile / opt-in | Instagram, Messenger, LinkedIn, X Chat | Explicit ToS/ban warning; expect login friction |
| Conditional | iMessage (user's own Mac only; beeper/platform-imessage, MIT) | "Advanced, macOS-only, may break on OS updates" |
| Out of scope | iMessage without a Mac, official X DM API, WeChat | Do not promise |

DMA (Digital Markets Act) wildcard: WhatsApp third-party interop has been live in the EU since Nov 2025 (BirdyChat, Haiket). Monitor as upside; do not build against it.

## 4. Beeper account coverage (exact surface)

A keeper-connected Beeper account sees: Matrix-native chats + Beeper Cloud bridge rooms + bbctl self-hosted bridge rooms on matrix.beeper.com (hungryserv — partial C-S API; test against a real account early). It does **not** see On-Device Connection chats (WhatsApp/Signal in official apps since 2025-07; more networks migrating through 2026). Optional future companion mode: Beeper Desktop API (localhost:23373, OAuth+PKCE, MIT SDKs) can reach on-device chats if Beeper Desktop is installed — pragmatic add-on, never a foundation.

## 5. MoSCoW mapping (PRD traceability)

Adopted from market research §5 / Appendix A, adjusted to owner inputs:

- **Must (MVP):** Matrix core (login/E2EE/SSS/text/replies/edits/reactions/media), unified inbox + archive + unread, unlimited multi-account, bridge management UI, local archive + FTS + export, favorites + pins, ⌘K + hotkeys, native notifications (per-chat/network mute, mention-only), incognito (receipts + typing), drafts with explicit-approval review pane (owner requirement — promoted from Could), undo-send (owner requirement — promoted from Should), Spaces as room-group views (owner requirement — promoted from Should).
- **Should (v1.x):** low-priority view, message requests, labels/filtered views, snooze/reminders (local), scheduled send (local, "app must be running"), note-to-self, bridge health dashboard + alerting.
- **Could (validate first):** agent-proposed drafts — a propose-only local API/MCP feeding the approval pane (approval-gated sends), voice-note transcription via local Whisper, iMessage helper (v1.x, advanced flag), themes.
- **Won't (this cycle):** calls (Element Call embed post-MVP), server-side anything, mobile apps, in-client bridges, automation/broadcast features.

## 6. Rejected alternatives (rationale record)

- **Electron / state-in-JS / matrix-js-sdk in frontend** — kills the perf and trust story; one source of truth in Rust.
- **Native VoIP (webrtc-rs)** — years of work; SDK has no support (matrix-rust-sdk issue #3295); Element Call widget is the sanctioned path.
- **Running bridges inside the client (Beeper on-device style)** — massive scope; keeper manages external bridges instead; reassess post-v1.
- **Hosted bridge/homeserver service** — violates client-only positioning and inherits ToS liability.
- **Betting core flows on Beeper private APIs** — isolated behind provider traits only.

## 7. Setup-cliff mitigations (priority order)

1. First-run wizard detecting bridges on the connected homeserver and walking through logins — treated as core product, not extra.
2. Documented one-command companion stack (docker-compose: Synapse or conduwuit + chosen bridges) maintained as docs only. Server recommendation decided in architecture phase.
3. Point non-self-hosters to managed Matrix-with-bridges providers (etke.cc-style) and to the Beeper-account path (bbctl).

## 8. Open items carried forward

- Validate demand for agent-proposed drafts (propose-only API/MCP) with ~10 design partners before promoting beyond a flag (market §6.3 Q2); the human approval pane itself is already MVP scope per the owner's required feature list.
- Technical spike before PRD commitments: matrix-rust-sdk 0.18 in a Tauri 2 shell on macOS — SSS, E2EE, FTS-over-SQLite (market §6.4 step 2).
- 5–8 problem interviews with self-hosted-bridge users to rank bridge UX vs archive vs incognito vs approval-drafts (market §6.4 step 3).
- Homeserver recommendation for the companion-stack docs (Synapse vs conduwuit).
- iOS walking-skeleton build early in the mobile phase to de-risk Tauri mobile before UI investment locks in desktop assumptions.
