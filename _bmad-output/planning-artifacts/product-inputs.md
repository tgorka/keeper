# Keeper — Product Inputs (stakeholder requirements)

Source: project owner (Tomasz Gorka), 2026-07-03, consolidated and translated to English.
This document seeds the BMAD planning chain (brief → PRD → UX → architecture → epics).

## Vision

An open-source, Beeper-style universal messenger **client** built on Matrix. One app for all
chat networks, owned by the user: local message archive, self-hosted or local bridges, no
vendor lock-in. macOS app first; iPhone next; Windows/iPad/Android/Linux later.

## Hard constraints

- **Client only.** No server-side components in this repo. Bridges/homeservers are external.
- **Stack:** Tauri 2 + Rust core (matrix-rust-sdk) + React + TypeScript + shadcn/ui.
- **Open source (Apache-2.0).** No credentials in the repo; dev creds via 1Password (`op`).
- Repository language: English. All project files live in the repo (except build artifacts).
- Modeled on the Beeper client (UX benchmark), reusing lessons from existing OSS clients.

## Required features (owner's list)

1. Full communication: text, audio/video (calls), emoji/reactions, files, media.
2. Local copies of messages (pulled from bridges) — a persistent on-device archive.
3. Connecting to bridges: remote (self-hosted), Beeper cloud bridges, and local bridges
   (bbctl-style), as in Beeper.
4. Drafts that are sent only after explicit approval. If Matrix has no native support, a
   dedicated bridge/server-side app MAY exist someday, but NOT in this repo — for now drafts
   are client-side (local + Matrix account data).
5. UI/UX like Beeper — Spaces should display different room groups; unified inbox.
6. Multi-account (several Matrix accounts at once, e.g. beeper.com + self-hosted).
7. Favorite contacts to talk to (pinned/favorites).
8. Hotkeys and notifications (native macOS).
9. Incognito mode and undo-send with a time window.
10. Networks (via bridges, current & future): Discord, Slack, Instagram, X, WhatsApp,
    Telegram, Signal, Messenger, Google Voice, iMessage, LinkedIn, e-mail (future),
    Beeper/Matrix native, AI bot client (future), terminal client (future).

## Owner decisions (made autonomously by coordinating agent, as delegated)

- **MVP = macOS desktop, text-first.** Voice/video calls deferred post-MVP; delivered via
  Element Call widget (MatrixRTC) rather than native VoIP implementation.
- **Matrix core:** matrix-rust-sdk 0.18+ with matrix-sdk-ui (SyncService, RoomListService,
  Timeline, SendQueue), embedded directly in the Tauri Rust backend (no FFI layer).
- **Sync:** Simplified Sliding Sync (MSC4186). **Auth:** password + OIDC (MAS/MSC3861) +
  Beeper's email-code JWT flow (ported from Apache-2.0 bbctl).
- **Undo send:** outgoing messages held in a configurable delay window (default 10 s) before
  dispatch via SendQueue; after dispatch, "delete for everyone" = redaction.
- **Incognito:** per-account/per-room toggle that suppresses read receipts (private read
  receipts `m.read.private`) and typing indicators.
- **Drafts with approval:** drafts stored locally (SQLite) and mirrored to per-room Matrix
  account data; a Drafts review pane allows send/discard. Designed so an external agent
  (AI/terminal, future) can propose drafts that the user approves in-app.
- **Local archive:** all synced events persisted in SQLite (matrix-sdk store + event cache),
  with full-text search and export; archive survives logout by design where feasible.
- **Bridge management:** in-app UI for connecting bridge bots (login/QR/commands +
  bridgev2 provisioning API where available); guided setup for self-hosted bridges; optional
  bbctl integration for Beeper self-hosted bridges. Bridges themselves are external processes.
- **Beeper caveat (accepted):** since Beeper's 2025 on-device pivot, chats using Beeper's
  on-device connections (e.g. WhatsApp/Signal in the official apps) are NOT visible to
  third-party Matrix clients. Keeper covers Matrix-native + cloud-bridge + self-hosted-bridge
  chats; parity for on-device networks comes from running one's own bridges.
- **iMessage:** only via user's own Mac (beeper/platform-imessage, MIT) — no relay service.
- **Licensing firewall:** Apache-2.0/MIT dependencies only (cargo-deny enforced); AGPL
  projects (Element Web, mautrix bridges, Cinny, gomuks) are study-only references.
- **Quality:** Biome, rustfmt/clippy (-D warnings), vitest, cargo-nextest, lefthook hooks,
  GitHub Actions CI on macOS. Rust-based/fast tooling preferred throughout.

## Reference research

- Technical: `_bmad-output/planning-artifacts/research-technical-2026-07-03.md`
- Market/product: `_bmad-output/planning-artifacts/research-market-2026-07-03.md`
