---
title: "Product Brief: keeper"
status: draft
created: 2026-07-03
updated: 2026-07-03
---

# Product Brief: keeper

## Executive Summary

keeper is an open-source (Apache-2.0), Beeper-style universal messenger client built on Matrix: one fast, native-feeling macOS app for every chat network the user bridges — Telegram, WhatsApp, Signal, Slack, Discord, and more — with a permanent, searchable, exportable local archive of every message. It is a **client only**: no servers, no hosted bridges, no message ever passing through project infrastructure. Users bring their own Matrix homeserver and bridges (or a Beeper account), and keeper makes that stack feel like a polished product instead of a terminal hobby.

The market has split into two halves that don't meet. Beeper proved the unified-inbox category ($125M acquisition, three paid tiers) but keeps its clients closed and paywalls exactly what power users want most — multi-account, incognito, scheduled send. Open-source Matrix clients (Element X, Cinny) have world-class protocol tech but zero bridge UX and no unified-inbox product thinking. Nobody ships an open-source, native desktop client with first-class bridge management and Beeper-grade inbox polish. keeper sits precisely in that gap, at a moment when Matrix 2.0 is stable, the mautrix bridge ecosystem is healthy and Beeper-funded, and Beeper's July 2025 paywall created a visible cohort of annoyed power users.

Technically, keeper embeds matrix-rust-sdk — the engine behind Element X — directly in a Tauri 2 Rust backend, with a thin React/TypeScript UI rendering Rust-owned view models. No Electron, no crypto in JavaScript, no FFI layer. No shipping client uses this architecture yet; keeper would be first.

## The Problem

People live in 5–12 chat networks. Every option for unifying them extracts a tax:

- **Beeper** charges $9.99–$49.99/month for more than five accounts, incognito mode, reminders, and scheduled send; its clients are closed source; and since its 2025 on-device pivot, its architecture keeps drifting away from third-party access. Users also report silent bridge disconnects and notification gaps.
- **Element + self-hosted bridges** works but is hostile: logging into WhatsApp means typing `!wa login` at a bot and squinting at a QR code in a chat room. Bridge sessions die silently. Nothing surfaces health. This is keeper's target user *today* — proof of demand, terrible UX.
- **Webview wrappers** (Ferdium, Rambox) are browsers, not messengers: no unified inbox, no cross-service search, no shared archive, 2GB+ of RAM.

Underneath sits a quieter, deeper problem: **message history belongs to platforms, not people.** Slack free tiers truncate it, Telegram edits rewrite it, disappearing messages erase it, and a SaaS shutdown (Texts.com) can orphan it overnight. Power users fear this loss but have no tool that treats the archive as a first-class product.

## The Solution

A macOS-native messenger that makes a user-owned Matrix + bridges stack feel better than Beeper:

- **Unified inbox** across all networks and accounts — chronological, with archive, favorites/pins, unread states, and reliable native notifications with per-chat and per-network mute.
- **Unlimited multi-account, free forever.** Several Matrix accounts at once (e.g., beeper.com + self-hosted), unlimited networks. The headline wedge against Beeper's paywall.
- **Native bridge management** — keeper's core differentiator. Detect bridges on the connected homeserver, drive logins through native UI (QR rendering, code entry via the bridgev2 provisioning API), surface connection health, and prompt re-login before messages silently drop.
- **Local-first archive.** Every synced message persisted in SQLite with offline full-text search and JSON/Markdown export. History survives network retention limits and, where feasible, even logout. The trust pillar.
- **Privacy and control for free:** incognito mode (suppress read receipts via `m.read.private` and typing indicators, global or per-room) and undo-send (configurable delay window, default 10 s, before dispatch; redaction after).
- **Keyboard-first:** ⌘K command palette, quick-switcher, global hotkeys — the Texts/Beeper heritage this segment expects.

Flagship networks at MVP: **Telegram, WhatsApp, Signal** — highest usage, proven bridges, lowest risk — with the rest of the mautrix ecosystem (Slack, Discord, Instagram, Messenger, X, LinkedIn, Google Voice) supported through the same bridge UX with honest per-network risk labeling.

## What Makes This Different

1. **Open source where Beeper is closed, free where Beeper charges.** Every wedge feature — unlimited accounts, incognito, undo-send, local archive — attacks a documented Beeper complaint or paywall line.
2. **The only client with real bridge UX.** Element treats bridge bots as weird chat rooms; bbctl is a CLI. keeper wraps login, health, and re-auth in native product surface. Nothing on the market does this.
3. **Architecture as advantage.** Rust core (matrix-rust-sdk + SQLite + crypto) with a webview that only renders view models — Element X's proven design minus the FFI layer, against Beeper's Electron. This is the difference between scaling to 100k+ events and dying like state-in-JS clients.
4. **Client-only is a trust posture, not a limitation.** keeper never operates bridges, so ToS exposure stays with the user's own homeserver — the same liability posture as Element. Honest per-network risk labels (Telegram green → Meta/X amber → iMessage "advanced, Mac-only") are a feature, not fine print.
5. **A credible path to safe agentic messaging.** Approval-gated drafts — where an AI or external tool can *propose* replies but only the human can send — exists in no competitor (Beeper's MCP API sends unsupervised). Post-MVP experiment, but a genuinely novel bet.

Honest caveat: keeper has no brand, one platform at launch, and depends on users having a homeserver and bridges. The moat is product focus in an empty niche plus a structural gift — Beeper funds the bridges, Element funds the SDK, and keeper rides both upstream for free.

## Who This Serves

**The self-hosting power communicator.** A developer, ops engineer, or indie professional on a Mac daily driver who lives in 5–12 networks, runs (or is willing to run) a Matrix homeserver with mautrix bridges — or holds a Beeper account and wants a client they control. They value keyboard speed, data ownership, and privacy; today they cobble together Element + bbctl or pay Beeper reluctantly. Success for them: one fast app, every conversation, nothing lost, nothing leaking, no subscription.

Secondary: the post-paywall Beeper cohort — users who hit the 5-account cap or resent paying $120/year for incognito and reminders — for whom keeper plus a managed Matrix host (e.g., etke.cc-style) is the escape hatch.

## Success Criteria

- **Daily driver test:** the maintainer and early adopters retire Beeper/Element for daily messaging within 3 months of first beta.
- **MVP demo bar:** Telegram, WhatsApp, and Signal work flawlessly end-to-end — native bridge login, send/receive with E2EE, media, reactions, search — on a self-hosted homeserver and on a Beeper account (cloud + bbctl bridges).
- **Reliability:** no silent message loss; bridge health surfaced within 60 s of a session drop; notifications delivered while the app runs in background.
- **Performance:** cold start to interactive inbox < 2 s; local full-text search across 100k+ events < 200 ms; idle RAM a fraction of Electron peers.
- **Archive trust:** export produces complete, readable JSON/Markdown; archive survives re-login.
- **OSS traction (12 months):** 1,000+ GitHub stars, listing on awesome-selfhosted, external contributors landing merged PRs, and an HN/r/selfhosted launch that validates the "open-source Beeper" framing.

## Scope

**MVP (macOS desktop, text-first):**
Matrix core (login: password + OIDC/MAS + Beeper email-code JWT; E2EE with cross-signing and verification; Simplified Sliding Sync MSC4186); unified inbox with archive, favorites, pins; unlimited multi-account; bridge management UI (bot commands + bridgev2 provisioning, bbctl integration for Beeper self-hosted bridges); local archive with FTS and export; text, replies, edits, reactions, media, files; persistent per-chat drafts; incognito mode; undo-send delay window; ⌘K palette, hotkeys, native notifications; Spaces as room-group views.

**Fast-follow (v1.x):** snooze/reminders (local-only), scheduled send (honest "app must be running"), message-request filtering, bridge health dashboard, note-to-self, iMessage via the user's own Mac (beeper/platform-imessage, MIT — "advanced, may break on macOS updates"), draft-approval workflow prototype behind a flag.

**Explicitly out (this cycle):** voice/video calls (post-MVP via embedded Element Call widget — MatrixRTC is still pre-spec and rough on self-hosted setups); any server-side component; mobile apps (iPhone is next, after macOS proves the core); bridges running inside the client; WhatsApp automation/broadcast of any kind; hosted services.

## Risks — Stated Plainly

- **Beeper's on-device pivot shrinks the third-party surface.** Chats using Beeper's on-device connections (WhatsApp/Signal in official apps since July 2025) never touch matrix.beeper.com and are invisible to keeper. keeper covers Matrix-native + cloud-bridge + bbctl-bridge rooms; parity for on-device networks means running your own bridges. This is documented loudly, not buried.
- **Beeper auth is a private, unversioned API** (email-code JWT, ported from Apache-2.0 bbctl). It can change without notice; keeper isolates it behind a provider interface and treats standard Matrix as the foundation.
- **Network ToS risk is real, mostly for the user.** WhatsApp bans for personal-use bridging are rare but possible; Meta/X login flows are hostile and volatile. keeper ships prominent disclosures and never adds automation features that trigger ban regimes.
- **iMessage has no sanctioned path.** Only automation of the user's own Mac; fragile across macOS updates; never promised for MVP.
- **matrix-rust-sdk is 0.x.** Breaking changes every minor release — a permanent upgrade tax, mitigated by a thin wrapper layer and tracking Element X.
- **The setup cliff bounds the initial market.** MVP addressable users = people with a homeserver + bridges. Mitigations: first-run bridge-detection wizard (this *is* the product), a documented one-command companion stack (docs only), and pointers to managed Matrix hosts.

## Vision

Year one: the obvious open-source answer to "Beeper, but mine" on macOS. Then iPhone, then Windows/Linux/Android/iPad, all on the same Rust core. Calls arrive via Element Call embedding once MatrixRTC stabilizes. The archive grows into the durable, searchable record of a person's entire messaging life — independent of any network's retention policy or any company's fate. And as AI agents enter messaging, keeper becomes the trustworthy surface: agents may read and propose, but a human always approves the send. If Beeper is the polished product you rent, keeper is the one you own.
