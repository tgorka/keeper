---
title: "PRD: keeper"
status: final
created: 2026-07-03
updated: 2026-09-02
---

# PRD: keeper

## 0. Document Purpose

This PRD defines the macOS text-first MVP of keeper, an open-source (Apache-2.0), client-only universal messenger built on Matrix. It is written for the downstream BMAD chain — UX design, architecture, and epic/story creation — and for contributors who need a single authoritative statement of what MVP includes, excludes, and must prove. It builds on, and does not duplicate, four upstream inputs: the product brief and its addendum (`_bmad-output/planning-artifacts/briefs/brief-keeper-2026-07-03/`), the stakeholder requirements (`_bmad-output/planning-artifacts/product-inputs.md`), and the technical and market research reports (`_bmad-output/planning-artifacts/research-technical-2026-07-03.md`, `research-market-2026-07-03.md`). Vocabulary is anchored in §3 Glossary; functional requirements are numbered FR-1 through FR-54 for the macOS MVP with testable consequences; cross-cutting NFRs are numbered NFR-1 through NFR-14. **Phase 2 increment (2026-07-09):** with the macOS MVP implemented in full, §13 extends this PRD with the iOS/iPhone client phase — FR-55 through FR-65 and NFR-15 through NFR-18 — built on the authoritative iOS technical research (`_bmad-output/planning-artifacts/research-ios-2026-07-09.md`); MVP sections §1–§12 are unchanged and remain the authority for all shared behavior. **Phase 3 increment (2026-07-16):** §14 adds the macOS Screen Recording phase — FR-66 through FR-76 and NFR-19 through NFR-22 — built on the authoritative recording research (`_bmad-output/planning-artifacts/research-recording-2026-07-16.md`), whose recommendations and risk register it adopts rather than relitigates. **Phase 8 increment (2026-09-02):** §15 adds the Bots phase — FR-369 through FR-393 and NFR-46 through NFR-49 — bound by Epic 61 (`_bmad-output/planning-artifacts/epic-61-a-model-you-can-talk-to-in-the-app-that-holds-your-drive.md`) and its evidence base (`research-ai-chat-2026-09-02.md`), whose verdicts it adopts rather than relitigates; Phases 4–7 (FR-77 through FR-368, NFR-23 through NFR-45) were specified in `epics.md` and the per-epic files and are not restated here. Inline `[ASSUMPTION]` tags mark inferences made without stakeholder confirmation and are indexed in §12. Technical constraints already locked by the owner (stack, SDK versions, IPC patterns, licensing firewall) live in the brief addendum and this PRD's `addendum.md`; this document states *what* keeper does, not *how*.

## 1. Vision

keeper is the messenger that keeps your messages. One fast, native-feeling macOS app for every chat network the user bridges through Matrix — Telegram, WhatsApp, Signal, Slack, Discord, and the rest of the mautrix ecosystem — with a permanent, searchable, exportable Local Archive of every message. It is a client only: no servers, no hosted bridges, no message ever passing through project infrastructure. Users bring their own Homeserver and Bridges (or a Beeper Account), and keeper makes that stack feel like a polished product instead of a terminal hobby.

The market has split into two halves that don't meet. Beeper proved the unified-inbox category, then paywalled exactly what power users want most — multi-account, incognito, scheduled send — and kept its clients closed. Open-source Matrix clients (Element X, Cinny) have world-class protocol tech but zero bridge UX and no unified-inbox product thinking. keeper sits precisely in that gap: the only open-source, native desktop client with first-class Bridge management and Beeper-grade inbox polish. Every wedge feature — unlimited multi-account, free Incognito Mode, Undo-Send, the Local Archive — attacks a documented Beeper complaint or paywall line, and all of them are free forever.

The MVP must prove one thing: that a user-owned Matrix + Bridges stack, wrapped in keeper, beats Beeper as a daily driver for the self-hosting power communicator. Success is the maintainer and early adopters retiring Beeper/Element within three months of first beta, with Telegram, WhatsApp, and Signal working flawlessly end-to-end. Beyond MVP, keeper grows into the durable record of a person's entire messaging life — and, as AI agents enter messaging, into the trustworthy surface where agents may read and propose but a human always approves the send.

## 2. Target User

### 2.1 Jobs To Be Done

- **Functional:** see every conversation from every Network and Account in one place, fast — without paying a subscription for the privilege.
- **Functional:** keep Bridges alive without babysitting a terminal: discover, log in, monitor, and re-authenticate Bridges from native UI.
- **Functional:** never lose a message — to platform retention limits, remote edits, disappearing messages, or a SaaS shutdown. Search all of it offline; export all of it.
- **Emotional:** read messages without social pressure to respond (no read receipts, no typing indicators — on the user's terms, for free).
- **Emotional:** un-embarrass yourself after a mis-send, on every network, not just the ones with native unsend.
- **Social/professional:** keep work and personal identities separate with multiple Accounts on the same Network, unlimited and free.
- **Contextual:** fly through 100+ Chats with the keyboard — Command Palette, Quick-Switcher, global hotkey.
- **Trust:** own the stack. Client-only, open source, local data, no telemetry — success is "one fast app, every conversation, nothing lost, nothing leaking, no subscription."

### 2.2 Non-Users (v1)

- People without a Matrix Homeserver, a managed Matrix host, or a Beeper Account — the setup cliff bounds the MVP market by design (mitigations in §4.4 First-Run Wizard and docs).
- Mainstream messenger switchers looking for a zero-setup Beeper clone; iPhone/Android/Windows/Linux users (macOS only in MVP).
- Teams needing voice/video calls (post-MVP), or businesses wanting broadcast/automation on WhatsApp or any network (never — see §5).
- Users whose primary need is iMessage (deferred to v1.x, "advanced, may break on macOS updates," and only via the user's own Mac).

### 2.3 Key User Journeys

- **UJ-1. Marek connects his homeserver and sees WhatsApp go green.**
  Marek, an ops engineer who runs Synapse with mautrix-whatsapp and mautrix-telegram in Docker, installs keeper. On first launch the First-Run Wizard asks for his Homeserver; he signs in with OIDC (his server runs MAS). keeper verifies the Homeserver supports Simplified Sliding Sync, syncs, and the Wizard lists the Bridges it detected on his server — WhatsApp and Telegram, each with a Network Risk Tier label. He clicks WhatsApp; keeper renders a QR code natively (no bot chat, no terminal), he scans it with his phone, and the Bridge Session goes healthy. Within a minute his WhatsApp and Telegram Chats stream into the Unified Inbox. **Climax:** the moment bridged Chats appear in one inbox with no `!wa login` ever typed. **Edge case:** his Homeserver lacks a provisioning endpoint for one Bridge — keeper falls back to driving the Bridge Bot conversation programmatically and shows the same native flow.

- **UJ-2. Sofia escapes the Beeper paywall without losing her chats.**
  Sofia hit Beeper's 5-account cap and resents paying $120/year for incognito. She adds her Beeper Account in keeper: enters her email, gets a code, and is signed in (Beeper email-code JWT). keeper shows her Matrix-native Chats, Beeper cloud-Bridge Chats, and her bbctl self-hosted Bridge Chats — and, prominently, a disclosure: Chats on Beeper's On-Device Connections (her WhatsApp in the official app) are not visible to any third-party client; running her own Bridge is the path to parity. She adds her self-hosted Matrix Account alongside — two Accounts, one Unified Inbox, zero dollars. **Climax:** both Accounts merged in one inbox, with honest labeling of what Beeper does and doesn't expose. **Edge case:** Beeper's private login API changes — keeper surfaces a clear "Beeper login unavailable, this is an unofficial API" error rather than a silent failure.

- **UJ-3. Devon triages 40 overnight chats before his first meeting.**
  Devon, an indie consultant in 9 networks across 3 Accounts, opens keeper (cold start under 2 seconds to an interactive inbox). Pinned Chats sit at the top; Favorites are one keystroke away. He walks unread Chats with keyboard navigation, archives 25 to the Archive view, replies to 6. A gossip-heavy group he must monitor but never answer sits in per-room Incognito Mode — he reads it all and no read receipt or typing indicator leaks. A client Space filters the inbox to just that client's rooms during the meeting. **Climax:** inbox zero in four minutes without touching the mouse. **Edge case:** a reply fails because his hotel Wi-Fi dropped — the message shows a visible failed state with retry; nothing is silently lost.

- **UJ-4. Ingrid catches a dead Signal session before it eats a day of messages.**
  Ingrid's Signal Bridge Session expires overnight (linked-device timeout). Within 60 seconds of keeper observing the drop, the Signal Network row shows an unhealthy Bridge Session state and keeper posts a native notification: "Signal disconnected — re-link to keep receiving messages." She clicks it, keeper opens the re-login flow, renders the QR, she re-links. **Climax:** what silently ate messages for days in Element is a one-minute, guided fix. **Edge case:** she ignores the prompt — the Network row stays visibly unhealthy; the state is persistent, not a dismissed toast.

- **UJ-5. Ada proves to herself the archive is real.**
  Ada has 140k events across two years of bridged history. A colleague edits a Telegram message to rewrite what was agreed; a vendor's Slack free tier truncated the original thread months ago. Ada searches offline — results across all Accounts and Networks return in under 200 ms — finds the original message content preserved in her Local Archive with its edit history, and exports the Chat to Markdown for the dispute and JSON for her records. Later she signs the Account out; the Local Archive survives. **Climax:** the moment the platform's rewrite loses to her local copy. **Edge case:** she wants the archive gone — sign-out offers an explicit "delete Local Archive" choice; nothing is deleted by default.

- **UJ-6. Noor stages replies at midnight, sends them at 9am — deliberately.**
  Noor drafts replies to sensitive threads late at night but has learned not to trust midnight-Noor's judgment. She writes Drafts in five Chats; they persist across restart and mirror to her Matrix account data. Next morning she opens the Approval Pane, sees all pending Drafts in one list, edits two, approves four (send), discards one. One approved message she regrets within seconds — the Undo-Send window (10 s default) lets her pull it back before it ever left the machine. **Climax:** the Approval Pane as a deliberate airlock between writing and sending. **Edge case:** she deletes an already-delivered message — keeper falls back to Matrix Redaction and says plainly that remote copies on bridged networks may persist.

## 3. Glossary

- **Account** — one authenticated Matrix user on one Homeserver (including a Beeper Account). keeper supports unlimited concurrent Accounts; each maps to one SDK client with its own store.
- **Homeserver** — the Matrix server an Account lives on (self-hosted Synapse/conduwuit, managed host, or matrix.beeper.com). Always user-provided; never operated by the project.
- **Beeper Account** — an Account on matrix.beeper.com, authenticated via Beeper's email-code JWT flow. Exposes Matrix-native Chats, Beeper cloud-Bridge Chats, and bbctl self-hosted Bridge Chats — but not On-Device Connection chats.
- **On-Device Connection** — Beeper's since-2025 mode where bridges run inside Beeper's own apps; those chats never reach matrix.beeper.com and are invisible to keeper. Disclosed, not worked around.
- **Network** — an external chat service reached through a Bridge (Telegram, WhatsApp, Signal, Slack, Discord, …) or Matrix itself ("Matrix-native").
- **Bridge** — an external mautrix-style process (on the user's Homeserver or Beeper's infrastructure) that connects a Network to Matrix. keeper manages Bridges; it never runs them in-process.
- **Bridge Bot** — the Matrix user a Bridge exposes for control commands (`login`, `list-logins`, `logout`, `resolve-identifier`, `start-chat`).
- **bridgev2 Provisioning API** — the standardized HTTP API modern mautrix Bridges expose for login flows as JSON state machines (QR display, code entry). keeper's preferred Bridge-login mechanism; Bridge Bot commands are the fallback.
- **bbctl** — Beeper's Apache-2.0 CLI (`bridge-manager`) for registering and running self-hosted Bridges against a Beeper Account. keeper can drive it as an optional sidecar.
- **Bridge Session** — a Bridge's authenticated link to one Network account (e.g., a WhatsApp linked device). Has observable health: healthy, degraded/action-needed, disconnected.
- **Network Risk Tier** — keeper's in-product honesty label per Network: low-risk, maintenance-heavy, volatile/opt-in, conditional (full table in `addendum.md`).
- **Chat** — one conversation as the user sees it (DM or group). Backed by exactly one Matrix Room on one Account.
- **Room** — the underlying Matrix object backing a Chat. UI copy says Chat; protocol-level requirements say Room.
- **Unified Inbox** — the single chronological list of Chats merged across all Accounts and Networks. The app's home surface.
- **Archive view** — the list of Chats the user has archived out of the Unified Inbox. A view, not storage — distinct from Local Archive.
- **Local Archive** — keeper's persistent on-device store (SQLite) of every synced event across all Accounts, powering FTS and Export. Independent of any Network's retention. The trust pillar.
- **FTS** — offline full-text search over the Local Archive.
- **Export** — user-initiated dump of a Chat or Account from the Local Archive to JSON or Markdown files.
- **Space** — a Matrix Space surfaced in keeper as a room-group view: a named filter over the Unified Inbox. MVP displays and filters by Spaces; it does not create or manage them.
- **Favorites** — a user-curated, always-visible section of key Chats. Distinct from Pins.
- **Pins** — Chats pinned to the top of the Unified Inbox, removed from the main scroll flow.
- **Draft** — unsent per-Chat message text, persisted locally and mirrored to per-Room Matrix account data. Never sent without explicit approval.
- **Approval Pane** — the surface listing all pending Drafts across Chats and Accounts, with approve (send) and discard actions. The designed insertion point for future agent-proposed Drafts (post-MVP).
- **Incognito Mode** — outbound-signal suppression: private read receipts (`m.read.private`), suppressed typing indicators, suppressed presence where applicable. Toggleable globally, per-Account, and per-Chat.
- **Undo-Send Window** — the configurable delay (default 10 s) during which an approved outgoing message is held locally, before dispatch, and can be cancelled.
- **Redaction** — Matrix's "delete for everyone." keeper's post-dispatch deletion fallback; propagation to bridged Networks is best-effort and disclosed as such.
- **E2EE** — Matrix end-to-end encryption (Olm/Megolm) with Cross-Signing, Device Verification, and key backup. Implemented exclusively in the Rust core.
- **Cross-Signing / Device Verification** — Matrix identity and device trust: users verify their own devices and other users (emoji/SAS or QR).
- **Simplified Sliding Sync (SSS)** — MSC4186, keeper's only sync mechanism. Homeserver support is verified at login.
- **Command Palette** — the ⌘K surface for fuzzy-finding Chats, contacts, and actions.
- **Quick-Switcher** — keyboard-first Chat switching (part of the Command Palette family, tuned for jump-to-Chat).
- **First-Run Wizard** — the guided setup flow: add first Account → detect Bridges → walk through Bridge logins. Treated as core product, not chrome.

## 4. Features

*FRs are numbered globally (FR-1 … FR-54 in this section; the iOS phase continues the sequence with FR-55 … FR-65 in §13, the Screen Recording phase with FR-66 … FR-76 in §14, and the Bots phase with FR-369 … FR-393 in §15 — FR-77 … FR-368 belong to Phases 4–7, specified in `epics.md` and the per-epic files rather than here). Every FR uses Glossary terms verbatim and carries testable consequences. "User" means the single macOS operator of the app.*

### 4.1 Accounts & Authentication

**Description:** keeper supports unlimited concurrent Accounts across any mix of Homeservers — the headline wedge against Beeper's paywall (realizes UJ-2). Three login paths sit behind one provider interface: password (legacy), OIDC via MAS (MSC3861), and Beeper's email-code JWT flow (ported from Apache-2.0 bbctl; an unofficial private API, flagged as such in the UI). At login keeper verifies the Homeserver's Simplified Sliding Sync support and fails with a clear, actionable message when absent. Signing out never silently destroys the Local Archive.

#### FR-1: Password login
User can add an Account by entering a Homeserver address, username, and password (m.login.password). Realizes UJ-1.
**Consequences (testable):**
- Given a reachable Homeserver with password login enabled, valid credentials produce a syncing Account within one flow; invalid credentials produce an inline error naming the cause (bad credentials vs. unreachable server vs. unsupported login type).
- Well-known discovery (`/.well-known/matrix/client`) resolves the Homeserver from a bare domain when present.

#### FR-2: OIDC login (MAS / MSC3861)
User can add an Account on a Homeserver using OIDC-native auth (e.g., matrix.org): keeper opens the system browser for the auth flow and completes login on redirect. Realizes UJ-1.
**Consequences (testable):**
- Against a MAS-enabled Homeserver, completing the browser flow yields a logged-in, syncing Account without the user handling tokens manually.
- Cancelling the browser flow returns keeper to the login screen with no partial Account created.

#### FR-3: Beeper email-code login
User can add a Beeper Account by entering their Beeper email and the emailed code; keeper exchanges the resulting JWT for a Matrix session on matrix.beeper.com. Realizes UJ-2.
**Consequences (testable):**
- Valid email + code produces a syncing Beeper Account showing Matrix-native, cloud-Bridge, and bbctl-Bridge Chats.
- The login UI labels the flow as using an unofficial Beeper API that may break without notice.
- If the Beeper API rejects or changes shape, keeper shows a distinct "Beeper login unavailable" error state — never a generic crash or hang.

#### FR-4: Unlimited multi-account
User can add, and run concurrently, an unlimited number of Accounts (multiple Accounts on the same Homeserver included), with no feature gated by Account count. Realizes UJ-2, UJ-3.
**Consequences (testable):**
- With ≥ 2 Accounts (e.g., beeper.com + self-hosted) signed in simultaneously, all Chats from all Accounts appear in the Unified Inbox and send/receive works on each.
- No code path enforces an Account-count limit; adding a 6th Account behaves identically to adding a 2nd.

#### FR-5: Homeserver capability verification
System verifies at login that the Homeserver supports Simplified Sliding Sync and reports actionable errors when it does not. Realizes UJ-1.
**Consequences (testable):**
- Login against an SSS-capable Homeserver (Synapse ≥ 1.114 defaults) proceeds; login against a non-SSS server fails before Account creation with a message naming SSS as the missing capability and linking to docs.
- The check result is logged per Account for support/diagnostics.

#### FR-6: Account management
User can list Accounts, see per-Account state (Homeserver, user ID, sync status), and sign out any Account — with an explicit choice to keep or delete that Account's slice of the Local Archive. Realizes UJ-5.
**Consequences (testable):**
- Sign-out defaults to keeping the Local Archive; a separate destructive action ("delete Local Archive for this Account") requires confirmation.
- After sign-out with retention, FTS still returns results from that Account's history; after sign-out with deletion, it returns none.

#### FR-7: Beeper coverage disclosure
System discloses, at Beeper Account login and in Account settings, that On-Device Connection chats are not visible to keeper, and points to self-hosted Bridges as the parity path. Realizes UJ-2.
**Consequences (testable):**
- The disclosure appears in the Beeper login flow before completion (not buried post-login) and remains accessible in settings.
- Copy names which of the user's expectations will break (e.g., "WhatsApp connected in the official Beeper app will not appear here").

### 4.2 Core Messaging & E2EE

**Description:** Table-stakes Matrix messaging on the matrix-rust-sdk service layer: text with replies, edits, reactions; media and files; E2EE with Cross-Signing, Device Verification, and key backup; visible send states with no silent loss (realizes UJ-3). All crypto, state, and storage live in the Rust core; the UI renders view models only (NFR-9).

#### FR-8: Sync via Simplified Sliding Sync
System syncs each Account via Simplified Sliding Sync only, resuming cleanly across restarts and offline periods. Realizes UJ-3.
**Consequences (testable):**
- After force-quit and relaunch, previously synced Chats render from local cache before network round-trips complete (cold-start bar: NFR-1).
- After 24 h offline, reconnect converges the Unified Inbox to server state without duplicate or missing Chats.

#### FR-9: Send and receive text
User can send and receive text messages in any Chat, with local echo, an offline-resilient outgoing queue, and visible per-message states (sending / sent / failed with retry). Realizes UJ-3.
**Consequences (testable):**
- A message composed offline shows a queued state and dispatches automatically on reconnect (subject to the Undo-Send Window, FR-46).
- A permanently failed send shows a failed state with a retry affordance; it never disappears silently (NFR-5).

#### FR-10: Replies
User can reply to a specific message; keeper renders the reply relationship inline for both sent and received replies, including replies arriving over Bridges.
**Consequences (testable):**
- Replying to a message in a bridged Telegram Chat produces a reply visible as such on the remote Network (given Bridge support).
- A received reply renders the quoted original; clicking it jumps to the original message in the timeline.

#### FR-11: Edits
User can edit their sent messages; keeper renders received edits as the latest content with an edited marker.
**Consequences (testable):**
- Editing a sent message updates it in-place in the timeline and (given Bridge support) on the remote Network.
- The Local Archive retains the pre-edit content per FR-36.

#### FR-12: Reactions
User can add and remove emoji reactions; received reactions render aggregated on the message.
**Consequences (testable):**
- Adding then removing a reaction round-trips correctly in a Matrix-native Chat and a bridged Chat.
- Reaction counts aggregate multiple reactors on one message.

#### FR-13: Media and files
User can send and receive images, video, audio, and arbitrary files, with thumbnails, upload/download progress, and inline preview for common types; decrypted media streams to the UI without passing through IPC as base64. Realizes UJ-3.
**Consequences (testable):**
- Sending a 25 MB video shows upload progress and produces a playable message on the receiving side; receiving one shows a thumbnail before full download.
- Received encrypted media renders decrypted in the timeline; the decrypted bytes are served via the custom media protocol, never embedded in IPC JSON payloads.
- [ASSUMPTION] Recording voice notes in-app is v1.x; MVP plays back received audio messages but only sends audio as file attachments.

#### FR-14: E2EE with Cross-Signing and Device Verification
User can participate in E2EE Chats: keeper encrypts/decrypts transparently, supports Cross-Signing setup and Device Verification (emoji/SAS and QR), and key backup with recovery-key restore. Realizes UJ-1.
**Consequences (testable):**
- A new keeper login can be verified from an existing session (e.g., Element) and vice versa; after verification, the device shows as trusted on both ends.
- With key backup restored, historical encrypted messages decrypt after a fresh login.
- Unverifiable/undecryptable events render an explicit "unable to decrypt" state with a recovery hint, never a blank.

#### FR-15: Delete for everyone (Redaction)
User can redact their own messages; received Redactions remove content from the timeline view (Local Archive behavior governed by FR-36).
**Consequences (testable):**
- Redacting a message replaces its timeline rendering with a redaction stub for all Matrix clients in the Room.
- In bridged Chats, keeper surfaces that propagation to the remote Network is best-effort (per-Network capability note).

#### FR-16: Read receipts and typing indicators
System displays others' read receipts and typing indicators, and sends the user's own — subject to Incognito Mode (FR-42/43).
**Consequences (testable):**
- With Incognito Mode off, reading a Chat emits a public read receipt (`m.read`); typing in the composer emits typing notifications.
- Received typing indicators and read states render in the Chat within 2 s of the event under normal sync.

#### FR-17: History pagination
User can scroll back through Chat history; keeper back-paginates from the Local Archive first, then the Homeserver, seamlessly.
**Consequences (testable):**
- Scrolling back through ≥ 10k events in one Chat proceeds without UI freeze (interaction bar: NFR-4).
- Events already in the Local Archive render while offline; a visible boundary indicates when older history requires network.

### 4.3 Unified Inbox & Organization

**Description:** The category-defining surface (realizes UJ-3): one chronological Unified Inbox across every Account and Network, with unread management, an Archive view for inbox-zero flow, the Beeper-proven Favorites/Pins two-tier pattern, Space-based room-group filtering, and unambiguous Network/Account attribution on every Chat.

#### FR-18: Unified Inbox
User can see all Chats from all Accounts and Networks in a single list ordered by most recent activity. Realizes UJ-3.
**Consequences (testable):**
- With 3 Accounts across 5+ Networks connected, a new incoming message on any of them moves that Chat to the top of the Unified Inbox within 2 s of sync delivery.
- The Unified Inbox remains a single scroll surface — no per-Network tab switching is required to see any Chat.

#### FR-19: Unread management
User can see unread states (per-Chat unread and mention badges) and mark any Chat read or unread manually. Realizes UJ-3.
**Consequences (testable):**
- Unread and mention counts match server-side read-marker state after sync convergence.
- Mark-as-read while Incognito Mode is on follows FR-45 (private receipt semantics).

#### FR-20: Archive view
User can archive a Chat out of the Unified Inbox into the Archive view and unarchive it back; archived Chats resurface on new activity. Realizes UJ-3.
**Consequences (testable):**
- Archiving removes the Chat from the Unified Inbox and shows it in the Archive view; a new incoming message returns it to the Unified Inbox. [ASSUMPTION] Auto-return on new activity is the default (Beeper's inbox-zero convention); a per-Chat "stay archived" override is v1.x.
- Archive state persists across restarts and syncs across the user's Matrix clients where representable (low-priority tag semantics).

#### FR-21: Favorites
User can mark Chats as Favorites — a DM Chat standing in for a favorite contact — and Favorites render as an always-visible section, distinct from Pins.
**Consequences (testable):**
- A Favorite Chat is reachable in one interaction from the Unified Inbox regardless of scroll position.
- Favorite state persists across restarts and re-login.

#### FR-22: Pins
User can pin Chats; Pins render at the top of the Unified Inbox, removed from the chronological flow.
**Consequences (testable):**
- Pinned Chats stay at top irrespective of newer activity in unpinned Chats; unpinning returns the Chat to chronological position.
- Pin order is user-controllable (drag or move actions).

#### FR-23: Spaces as room-group views
User can see the Spaces their Accounts belong to and filter the Unified Inbox to any Space's Rooms. Realizes UJ-3.
**Consequences (testable):**
- Selecting a Space shows only that Space's Chats; clearing the filter restores the full Unified Inbox.
- Space membership changes on the Homeserver reflect in keeper after sync.
**Out of Scope:** creating, editing, or managing Spaces (join/leave, hierarchy) — view and filter only in MVP.

#### FR-24: Network and Account attribution
System shows, on every Chat row and Chat header, which Network and which Account it belongs to.
**Consequences (testable):**
- Every Chat row and Chat header renders a Network icon and an Account marker; two Chats with the same remote contact via different Accounts always differ in at least the Account marker.
- A filter or grouping by Network is available from the Unified Inbox (e.g., via Command Palette or sidebar). [ASSUMPTION] Per-Network filtering ships as a simple filter, not Beeper's full "Spacebar" custom-views system (v1.x).

### 4.4 Bridge Management

**Description:** keeper's core differentiator and the reason it exists (realizes UJ-1, UJ-4): the unsolved problem no shipping client addresses. keeper detects Bridges on each connected Homeserver, drives logins through native UI — bridgev2 Provisioning API preferred, Bridge Bot command driving as fallback — surfaces Bridge Session health continuously, prompts re-login before messages silently drop, and labels every Network with its honest Network Risk Tier. The First-Run Wizard makes this the first thing a new user touches. For Beeper Accounts, optional bbctl integration registers and runs self-hosted Bridges.

#### FR-25: Bridge discovery
System detects the Bridges available on each connected Homeserver and lists them with status (configured / logged in / not logged in). Realizes UJ-1.
**Consequences (testable):**
- On a Homeserver with mautrix-whatsapp and mautrix-telegram registered, both appear in the Bridge list without manual configuration; a Homeserver with none shows an empty state linking to setup docs.
- [ASSUMPTION] Discovery mechanism (bot-user presence, provisioning endpoints, room heuristics) is an architecture decision; the requirement is that user-visible detection works on standard mautrix deployments without the user naming Bridge bot IDs.

#### FR-26: Native Bridge login via provisioning API
User can log a Bridge into a Network through native keeper UI — QR codes rendered in-app, verification codes entered in native fields — driven by the bridgev2 Provisioning API where available. Realizes UJ-1.
**Consequences (testable):**
- WhatsApp login completes end-to-end in native UI: keeper renders the QR, the phone scans it, the Bridge Session becomes healthy — without the user ever opening the Bridge Bot chat.
- Each provisioning state (waiting, QR, code entry, success, failure) has a distinct rendered state; failures include the Bridge's error message.

#### FR-27: Bridge Bot command driving (fallback)
User can perform Bridge operations (login, list-logins, logout, set-relay) through the same native UI on Bridges without a provisioning API — keeper sends and parses Bridge Bot commands programmatically. Realizes UJ-1.
**Consequences (testable):**
- On a legacy Bridge, native login produces the same user-visible flow (QR/code rendered natively) with the Bridge Bot conversation driven behind the scenes.
- The raw Bridge Bot Chat remains accessible for manual use; keeper never hides it.

#### FR-28: Bridge Session health and re-login prompts
System monitors Bridge Session health per Network and Account, surfaces state changes within 60 seconds, and prompts re-login with a one-click path into the login flow. Realizes UJ-4.
**Consequences (testable):**
- Killing a Bridge Session (e.g., unlinking the device from the phone) produces a visible unhealthy state in keeper and a native notification within 60 s (NFR-6).
- The unhealthy state is persistent until resolved — visible in the Bridge list and on affected Chats — not a dismissible-and-gone toast.
- Clicking the prompt lands directly in the re-login flow for that Bridge (FR-26/27).

#### FR-29: bbctl integration for Beeper self-hosted Bridges
User with a Beeper Account can register and run self-hosted Bridges via keeper's bbctl integration (optional sidecar): pick a Network, keeper drives `bbctl` register/run and the resulting Bridge appears in the Bridge list. Realizes UJ-2.
**Consequences (testable):**
- With bbctl available, a user can go from "no Signal bridge" to a logged-in self-hosted Signal Bridge against their Beeper Account without leaving keeper.
- If bbctl is absent, the UI offers guided install instructions; keeper functions fully without it for non-Beeper flows.
- [ASSUMPTION] MVP manages bbctl-run Bridges as launch-on-demand sidecar processes with status surfaced in the Bridge list; full lifecycle supervision (auto-restart policies, log viewer) is v1.x.

#### FR-30: Network Risk Tier labeling
System labels every Network with its Network Risk Tier at Bridge setup time and in the Bridge list, with plain-language ToS/ban guidance for volatile Networks.
**Consequences (testable):**
- Connecting a volatile-tier Network (e.g., Instagram) requires acknowledging an explicit risk notice; low-risk Networks (Telegram) show none beyond the label.
- Tier copy matches the risk-tier table in `addendum.md`; tiers are data-driven so guidance can update without UI rework.

#### FR-31: First-Run Wizard
User is guided on first launch through: add first Account (any of FR-1/2/3) → Bridge discovery → per-Bridge login — with a skippable path straight to the Unified Inbox. Realizes UJ-1.
**Consequences (testable):**
- A user with a prepared Homeserver reaches a Unified Inbox with ≥ 1 bridged Network logged in without leaving the Wizard or reading external docs.
- Every Wizard step is skippable and re-enterable later from settings (the Wizard is a path, not a gate).
- Users without a Homeserver see the honest fork: docs for the companion stack, managed-host pointers, or the Beeper Account path.

#### FR-32: Start new Chats via Bridge
User can start a new Chat with a Network contact from keeper: resolve an identifier (phone number, username) through the Bridge and open the resulting Chat.
**Consequences (testable):**
- Entering a phone number for a WhatsApp contact resolves (when the Bridge supports resolve-identifier) and opens a functioning Chat.
- Unresolvable identifiers produce a clear "not found / not on this Network" message.

### 4.5 Local Archive, Search & Export

**Description:** The trust pillar (realizes UJ-5): every synced event across every Account persists in the Local Archive on the user's disk, searchable offline in under 200 ms at 100k+ events, exportable to JSON and Markdown, and durable across sign-out, remote edits, and remote deletions. History belongs to the person, not the platform.

#### FR-33: Persist all synced events
System persists every synced event (messages, edits, Redactions, reactions, media metadata) for every Account in the Local Archive, including decrypted content of E2EE messages. Realizes UJ-5.
**Consequences (testable):**
- Events visible in any timeline are queryable from the Local Archive after app restart with network disabled.
- Media files cached locally remain openable offline; cache retention for large media is configurable without breaking message-text durability. [ASSUMPTION] Message text/metadata are retained indefinitely by default; media blobs follow a configurable cache policy (default: keep).

#### FR-34: Offline full-text search
User can run FTS across all Accounts, Networks, and Chats — fully offline — with filters for sender, Chat, Network, and date. Realizes UJ-5.
**Consequences (testable):**
- Search over a 100k+-event Local Archive returns first results in < 200 ms (NFR-2), with the network disabled.
- Results deep-link into the containing Chat at the matched message.
- Search-in-Chat (scoped to the open Chat) is available from the same affordance.

#### FR-35: Export to JSON and Markdown
User can Export any Chat, any Account, or the full Local Archive to JSON (lossless: events with metadata) and Markdown (readable transcript), including referenced media files. Realizes UJ-5.
**Consequences (testable):**
- Exporting a 10k-message Chat produces a complete, well-formed JSON file and a chronologically ordered Markdown transcript; message count matches the Local Archive.
- Export runs in the background with progress and does not block messaging.
- Exported Markdown renders sender, timestamp, edits (final text), and media as file links relative to the export folder.

#### FR-36: Archive durability against remote rewrites
System retains original content in the Local Archive when messages are remotely edited or deleted: edits keep prior versions; Redactions and network-side deletions mark, but do not erase, the local copy. Retention behavior is user-configurable. Realizes UJ-5.
**Consequences (testable):**
- After a remote edit, the Local Archive holds both versions; the timeline shows the latest with edit history inspectable.
- After a remote Redaction, the timeline shows the redaction stub, and the pre-Redaction content remains retrievable via the Local Archive (search/export) — unless the user has enabled "honor remote deletions locally."
- [ASSUMPTION] Default is preserve-locally (the product's core promise); a settings toggle honors remote deletions for users who want norm-following behavior. This applies to the user's own local store only and is disclosed in settings copy.

#### FR-37: Archive survives sign-out
System retains the Local Archive (including FTS and Export availability) after an Account signs out, unless the user explicitly deletes it (FR-6). Realizes UJ-5.
**Consequences (testable):**
- After sign-out with retention, FTS and Export over that Account's history still work with no active session.
- [ASSUMPTION] Already-decrypted content remains readable after sign-out; encrypted events never synced-and-decrypted before sign-out are not recoverable — "survives logout where feasible" per the brief, stated honestly in UI copy.

### 4.6 Drafts & Approval Pane

**Description:** Persistent per-Chat Drafts with a deliberate airlock (realizes UJ-6): the Approval Pane lists every pending Draft across all Chats and Accounts, and nothing sends without an explicit approval action. This is an owner-required MVP feature and the designed foundation for post-MVP agent-proposed Drafts — the pane ships now; the agent API does not (see §5).

#### FR-38: Persistent per-Chat Drafts
User's composer text persists per Chat as a Draft — across Chat switches, app restarts, and crashes.
**Consequences (testable):**
- Text typed in a composer survives force-quit and relaunch, restored in the same Chat.
- Chats with pending Drafts are visibly marked in the Unified Inbox.

#### FR-39: Cross-device Draft mirroring
System mirrors Drafts to per-Room Matrix account data so Drafts follow the Account across devices/clients where supported.
**Consequences (testable):**
- A Draft written in keeper appears (as data) in the Account's per-Room account data; editing the Draft updates it.
- Conflicts (Draft changed elsewhere) resolve last-write-wins with the local unsent text never silently destroyed — [ASSUMPTION] on conflict, keeper keeps the local version and surfaces the remote one for one-tap adoption.

#### FR-40: Approval Pane
User can open the Approval Pane listing all pending Drafts across all Chats and Accounts, and per Draft: edit, approve (send), or discard. Realizes UJ-6.
**Consequences (testable):**
- With Drafts in ≥ 3 Chats across ≥ 2 Accounts, the Approval Pane lists all of them with Chat, Account, and Network attribution.
- Approve dispatches through the normal send pipeline (including the Undo-Send Window); discard removes the Draft locally and from mirrored account data.
- The Approval Pane is reachable via the Command Palette and a dedicated shortcut.

#### FR-41: Explicit-approval invariant
System never sends a Draft without an explicit user approval action (composer send or Approval Pane approve). No background, scheduled, or automated dispatch path exists in MVP.
**Consequences (testable):**
- Code inspection and tests confirm exactly two dispatch triggers, both user-initiated; there is no API surface through which a Draft can be sent programmatically.
- This invariant is documented as the contract future agent-proposal features must honor (agents may propose; only the user approves).

### 4.7 Privacy Controls: Incognito & Undo-Send

**Description:** Beeper charges $9.99/month for incognito; keeper ships it free (realizes UJ-3, UJ-6). Incognito Mode suppresses outbound signals — read receipts via `m.read.private`, typing indicators, presence where applicable — globally, per-Account, or per-Chat. Undo-Send holds every approved outgoing message in a local delay window before dispatch; after dispatch, deletion falls back to Redaction with honest cross-Network caveats.

#### FR-42: Incognito Mode — read receipts
User can enable Incognito Mode globally, per-Account, or per-Chat; while on, reading Chats emits private read receipts (`m.read.private`) instead of public ones. Realizes UJ-3.
**Consequences (testable):**
- With Incognito Mode on for a Chat, the remote party's client shows the message as unread after the user reads it; the user's own read position still syncs across their devices.
- Scope precedence is deterministic: per-Chat overrides per-Account overrides global; effective state is visible in the Chat header.

#### FR-43: Incognito Mode — typing and presence
While Incognito Mode applies, system suppresses typing indicators, and presence where the protocol allows.
**Consequences (testable):**
- Typing a long message in an Incognito Chat emits zero typing events (verifiable at the Homeserver).
- Typing suppression is bundled with Incognito Mode; [ASSUMPTION] no separate typing-only toggle in MVP (Beeper offers one; keeper defers it to v1.x to keep the model simple).

#### FR-44: Coupled-behavior disclosure
System discloses per-Network coupling caveats where suppression has side effects — e.g., WhatsApp couples sending read receipts with seeing others'.
**Consequences (testable):**
- Enabling Incognito Mode on a WhatsApp Chat surfaces the coupling note ("you may also stop seeing others' read receipts") at toggle time.
- Caveats are per-Network data, consistent with the Network Risk Tier copy system (FR-30).

#### FR-45: Manual read release
User can manually mark an Incognito Chat as read publicly ("release the receipt") when they choose to.
**Consequences (testable):**
- The explicit action emits a public `m.read` receipt for the selected Chat at the current read position; without it, only private receipts are ever sent while Incognito applies.

#### FR-46: Undo-Send Window
User's approved outgoing messages are held locally for a configurable Undo-Send Window (default 10 s; configurable 0–60 s) before dispatch; during the window the user can cancel, returning the text to the composer as a Draft. Realizes UJ-6.
**Consequences (testable):**
- Cancelling within the window results in zero network dispatch (verifiable at the Homeserver) and the full text restored as a Draft.
- The pending state is visible (countdown affordance); setting the window to 0 disables holding entirely.
- Queued-offline messages respect the window from the moment of approval, not the moment of reconnect. [ASSUMPTION] Window runs at approval time; a message that survived its window while offline dispatches immediately on reconnect.

#### FR-47: Post-dispatch delete for everyone
User can delete an already-dispatched message for everyone via Redaction, with per-Network best-effort framing.
**Consequences (testable):**
- The action issues a Matrix Redaction; in bridged Chats the UI states that removal on the remote Network depends on the Bridge and Network ("best effort").
- The Local Archive treats the user's own deletions per FR-36 semantics.

### 4.8 Command Palette, Hotkeys & Keyboard Navigation

**Description:** The Texts/Beeper heritage this segment expects (realizes UJ-3): a ⌘K Command Palette over Chats, contacts, and actions; a Quick-Switcher tuned for jump-to-Chat; full keyboard traversal of the Unified Inbox and timeline; and a global hotkey that summons keeper from anywhere in macOS.

#### FR-48: Command Palette
User can open the Command Palette (⌘K) and fuzzy-find Chats, contacts, and app actions (archive, toggle Incognito Mode, open Approval Pane, start Export, Bridge operations), executing any result from the keyboard. Realizes UJ-3.
**Consequences (testable):**
- Typing ≥ 2 characters filters across Chats (all Accounts), contacts, and a registered action list; Enter executes; results render within 100 ms per keystroke at 10k Chats.
- Every MVP feature with a UI surface is reachable through at least one Command Palette action (parity audit is a release gate).

#### FR-49: Keyboard navigation and Quick-Switcher
User can traverse the Unified Inbox and Chats entirely from the keyboard: next/previous Chat, jump into/out of the timeline and composer, archive, mark read/unread, and Quick-Switch to any Chat by name. Realizes UJ-3.
**Consequences (testable):**
- The UJ-3 triage loop (walk unreads → archive → reply → next) completes with zero pointer use.
- A published shortcut reference exists in-app (cheat-sheet overlay); shortcuts follow macOS conventions (⌘-based, standard text editing).

#### FR-50: Global hotkey
User can summon/hide keeper with a system-wide global hotkey, configurable in settings.
**Consequences (testable):**
- The hotkey works while keeper is backgrounded or hidden (given macOS permissions), raising the main window with focus in the Unified Inbox.
- Conflicts with existing system shortcuts are detected at assignment time with a warning.

### 4.9 Notifications

**Description:** Reliability is the bar, not features — competitor complaints cluster here (realizes UJ-3, UJ-4). keeper posts native macOS notifications from its local sync loop (no third-party push infrastructure), honors per-Chat and per-Network mute and mention-only modes, and keeps notifying while backgrounded. Bridge health alerts (FR-28) ride the same pipeline.

#### FR-51: Native notifications
System posts native macOS notifications for new messages, with sender, Chat, and message preview; previews can be disabled (privacy) and E2EE content is only rendered from the local decrypting sync loop.
**Consequences (testable):**
- A message arriving while keeper is backgrounded produces a native notification within 5 s of sync receipt (NFR-7).
- With previews off, notifications show sender/Chat but no content.
- No notification is ever routed through project-operated infrastructure (NFR-11).

#### FR-52: Mute controls and mention-only mode
User can mute notifications per Chat and per Network, set mention-only mode per Chat, and set a global do-not-disturb; muted Chats still accumulate unread state. Realizes UJ-3.
**Consequences (testable):**
- A muted Network produces zero notifications while its Chats continue updating in the Unified Inbox.
- Mention-only Chats notify on mentions/replies-to-user only; the matrix push-rule mapping (or local equivalent) is consistent across restarts.

#### FR-53: Background operation
System continues syncing and notifying while the app runs in the background or is hidden; optional launch-at-login and menu-bar presence keep the sync loop alive without a visible window.
**Consequences (testable):**
- With the window closed (app running), messages sync and notify identically to foreground operation.
- Launch-at-login is opt-in; quitting the app fully stops sync (and the UI says so — no fake "push while quit" promise).

#### FR-54: Notification interaction
User can click a notification to land in the exact Chat (correct Account) with the relevant message in view.
**Consequences (testable):**
- Clicking a notification for Account B's Chat while Account A's Chat is open switches context correctly within the interaction-latency bar (NFR-4).
- [ASSUMPTION] Inline quick-reply from the notification is v1.x; MVP is click-through only.

## 5. Non-Goals (Explicit)

- **No server-side components, ever, in this repo.** No hosted homeservers, no hosted bridges, no relay, no cloud "assist" for any feature (contrast: Beeper's Send Later). If a feature needs a server, it is out or it is honest about being local-only.
- **No bridges running inside the client** (Beeper on-device style). keeper manages external Bridges; it never becomes one. Reassess post-v1, explicitly not now.
- **No voice/video calls in MVP.** Post-MVP via embedded Element Call widget once MatrixRTC stabilizes; no native VoIP implementation on any timeline.
- **No mobile, no Windows/Linux in MVP.** macOS first; iPhone next after macOS proves the core. *(The core is proven: the iPhone phase is now specified as Phase 2 in §13. Windows/Linux remain out.)*
- **No WhatsApp (or any Network) automation, broadcast, or bulk messaging — ever.** These trigger ban regimes and betray the user-safety posture.
- **No agent/AI send path in MVP.** The Approval Pane ships; the propose-only agent API/MCP is a post-MVP experiment behind a flag, gated on design-partner validation. Nothing in MVP may send without explicit user approval (FR-41).
- **No iMessage in MVP.** v1.x at earliest, only via the user's own Mac, labeled "advanced, may break on macOS updates."
- **No video editing, and no recording upload — ever.** The Screen Recording phase (§14) writes local files to a user-chosen folder, full stop: no editor, no share-link or cloud-processing service, zero new network destinations (FR-76).
- **No tool that sends, executes or fetches — ever.** The Bots phase (§15) gives a model the drive through seven filesystem verbs inside a user-granted, revocable scope, full stop: no shell, no network fetch, no Matrix send (FR-389); FR-41 stands untouched, and keeper ships no endpoint, no hosted model and no default Provider — the endpoint is always the user's (D-4).
- **No monetization surface.** No accounts-with-us, no license keys, no telemetry-driven upsell. keeper is free OSS; sustainability questions live outside this PRD.
- **Not a Matrix admin tool.** keeper manages Bridges from a user's perspective; homeserver administration (user management, federation config) is out.
- **Not chasing Beeper feature-for-feature.** Reminders/snooze, scheduled send, message-request filtering, labels, note-to-self are deliberate v1.x fast-follows (§6.2), not silent MVP creep.

## 6. MVP Scope

### 6.1 In Scope (macOS desktop, text-first)

- Matrix core: password + OIDC/MAS + Beeper email-code JWT login; Simplified Sliding Sync (only); E2EE with Cross-Signing, Device Verification, key backup (FR-1–FR-17).
- Unified Inbox with Archive view, unread management, Favorites, Pins, Spaces as room-group views, Network/Account attribution (FR-18–FR-24).
- Unlimited multi-account, free (FR-4).
- Bridge management: discovery, native login (provisioning API + Bridge Bot fallback), Bridge Session health + re-login prompts, bbctl integration, Network Risk Tier labeling, First-Run Wizard, start-new-Chat (FR-25–FR-32).
- Local Archive with offline FTS and JSON/Markdown Export; durability against remote rewrites and sign-out (FR-33–FR-37).
- Messaging surface: text, replies, edits, reactions, media, files (FR-9–FR-13).
- Drafts with Approval Pane and explicit-approval invariant (FR-38–FR-41).
- Incognito Mode and Undo-Send Window with Redaction fallback (FR-42–FR-47).
- Command Palette, keyboard navigation, Quick-Switcher, global hotkey (FR-48–FR-50).
- Native notifications with mute/mention-only controls and background operation (FR-51–FR-54).
- Flagship Networks for the MVP quality bar: **Telegram, WhatsApp, Signal** — flawless end-to-end on both a self-hosted Homeserver and a Beeper Account (cloud + bbctl Bridges). Other mautrix Networks work through the same Bridge UX with Network Risk Tier labeling but sit outside the "flawless" gate.

### 6.2 Out of Scope for MVP

**v1.x fast-follows (committed direction, not MVP):**
- Snooze/reminders — local-only scheduler (Beeper charges for this; keeper's is honest-local). 
- Scheduled send — local-only with explicit "app must be running" framing.
- Low-priority view (hide chats, keep mention notifications), message-request filtering (unknown senders), labels/filtered views, note-to-self.
- Bridge health dashboard + alerting center (MVP has per-Bridge health + prompts, FR-28; the aggregate dashboard is v1.x).
- iMessage via the user's own Mac (beeper/platform-imessage, MIT) — advanced flag, fragility warning. `[NOTE FOR PM]` Emotionally load-bearing for the macOS audience; revisit priority once MVP reliability bars are green.
- Voice-note recording; notification quick-reply; typing-only privacy toggle; per-Chat stay-archived override; full custom filtered views.
- Agent-proposed Drafts: propose-only local API/MCP feeding the Approval Pane, behind a flag — gated on validation with ~10 design partners.

**Post-MVP / explicitly deferred:**
- Voice/video calls (Element Call widget embed, once MatrixRTC stabilizes on self-hosted setups).
- Mobile — **iOS now active as Phase 2, specified in §13**; Windows/Linux/Android/iPad remain later phases on the same Rust core.
- Beeper Desktop API companion mode (reach On-Device Connection chats when Beeper Desktop is installed) — pragmatic add-on, never a foundation.
- Email network, AI-bot client, terminal client (owner's long-term network list).
- Screen recording — **now active as Phase 3, specified in §14** (macOS desktop only; recording on Windows/Linux follows those platforms if and when they exist).
- AI chat — **now active as Phase 8, specified in §15** (the "AI-bot client" of the list above, built as a fifth keeper surface over the OpenAI-compatible wire to the user's own Hermes Agent or Ollama endpoints, not as a Network; talk mode and the wake word are Epic 62).

### 6.3 Why Now

Three clocks aligned in 2025–2026, and none of them stays open forever: (1) **Matrix 2.0 is real** — Simplified Sliding Sync entered Final Comment Period and ships default-on in Synapse, and matrix-rust-sdk (the engine behind Element X) is production-grade and Apache-licensed; (2) **the bridge ecosystem is healthy and funded by someone else** — Beeper employs the mautrix maintainer and pays bounties for new permissively-licensed bridges, all upstream and open; (3) **Beeper's July 2025 paywall created the customer** — a visible cohort of power users hit the 5-account cap or resent $120/year for incognito, exactly the features keeper ships free. Meanwhile the niche is empty: no open-source, native desktop client with real bridge UX exists, and the first credible entrant absorbs the awesome-selfhosted/HN attention cycle.

## 7. Cross-Cutting NFRs

**Performance** *(measured on Apple Silicon (M1 or later), release build, Local Archive ≥ 100k events, 3 Accounts unless stated)*

- **NFR-1 Cold start:** launch → interactive Unified Inbox (cached Chats rendered, input accepted) in **< 2 s**. Cold = process start with warm OS disk cache; sync convergence may continue after interactivity.
- **NFR-2 Search latency:** FTS first results in **< 200 ms** across 100k+ events, offline (p95 across a standard query set).
- **NFR-3 Memory:** idle RSS **≤ 500 MB** with 5 Accounts connected and sync running; **≤ 300 MB** with 1 Account. [ASSUMPTION] Numeric budgets inferred from "a fraction of Electron peers" (brief) and Beeper's ~200 MB reference; confirm before release gating.
- **NFR-4 Interaction latency:** switching Chats renders the cached timeline in **< 150 ms**; composer input latency **< 16 ms/frame**; Unified Inbox scroll at 60 fps with 10k Chats. [ASSUMPTION] Thresholds authored for testability; not in the brief.

**Reliability**

- **NFR-5 No silent message loss:** every outgoing message reaches a terminal user-visible state (sent / failed-with-retry); every incoming event that reaches the sync loop lands in the Local Archive. Failure modes always surface in UI.
- **NFR-6 Bridge health latency:** a dropped Bridge Session is reflected in UI and notified within **60 s** of the underlying state change reaching the Homeserver (per FR-28).
- **NFR-7 Notification latency:** native notification within **5 s** of event receipt by the local sync loop, foreground or background (per FR-51).
- **NFR-8 Crash safety:** an app crash or force-quit at any moment must not corrupt the Local Archive or crypto stores (WAL/atomic writes); next launch recovers to a consistent state with zero lost previously-persisted events.

**Security & Privacy**

- **NFR-9 Rust-core confinement:** all E2EE key material, message plaintext storage, and protocol state live exclusively in the Rust core. The webview holds only rendered view models; no crypto, no message DB, no tokens in JavaScript-accessible storage.
- **NFR-10 At-rest protection:** local stores (state, crypto, Local Archive) support passphrase-based at-rest encryption; enabling it is a first-run choice. [ASSUMPTION] Off by default (single-user Mac with FileVault typical); confirm default posture.
- **NFR-11 Network egress honesty:** keeper contacts only user-configured Homeservers/Bridges, Beeper's API when a Beeper Account is added, and the signed-update endpoint. No telemetry, no analytics, no crash reporting without explicit opt-in. Egress surface is documented and diffable per release.

**Distribution & Compliance**

- **NFR-12 Packaging:** signed + notarized macOS builds (Developer ID, hardened runtime), Apple Silicon native; auto-updates signed with the updater key; reproducible CI builds via GitHub Actions.
- **NFR-13 Licensing firewall:** keeper is Apache-2.0; no GPL/AGPL code or crates (cargo-deny in CI); AGPL ecosystem projects are study-only; MPL files are never ported. Provenance checklist on every PR that ports code.

**Accessibility**

- **NFR-14 Baseline accessibility:** all MVP flows operable via keyboard alone (a superset of FR-48–50); interactive controls carry accessibility labels for VoiceOver; contrast meets WCAG 2.1 AA for text in both light and dark themes. [ASSUMPTION] Full VoiceOver timeline-navigation polish is v1.x; the MVP bar is "operable and labeled."

*Phase 2 (iOS) adds NFR-15 – NFR-18 in §13.3, measured on-device. Phase 3 (Screen Recording) adds NFR-19 – NFR-22 in §14.3. Phase 8 (Bots) adds NFR-46 – NFR-49 in §15.3; NFR-23 – NFR-45 belong to Phases 4–7, specified in `epics.md` and the per-epic files.*

## 8. Constraints & Guardrails

- **Client-only is a trust posture.** keeper never operates infrastructure; ToS exposure for bridging stays with the user on their own Homeserver — the same liability posture as Element. Every surface that touches a gray-zone Network carries the Network Risk Tier disclosure (FR-30). Marketing and docs state this explicitly.
- **Safety of the send path.** The explicit-approval invariant (FR-41) is a product-level guardrail, not an implementation detail: no feature, flag, or future API may introduce an unattended send path without a new PRD-level decision.
- **Beeper private API containment.** Beeper auth (FR-3) is isolated behind a provider interface, labeled unofficial in the UI, and its failure degrades only Beeper Accounts — never core Matrix operation.
- **User data sovereignty.** The Local Archive is the user's property: no cloud sync of the archive, deletion is always explicit and user-initiated (FR-6, FR-36, FR-37), Export is always available and complete (FR-35).
- **Honest-local rule for deferred features.** Any v1.x feature that competitors implement with cloud assistance (scheduled send, reminders) ships local-only with explicit "app must be running" framing — the honesty is the differentiator.
- **Upstream posture.** keeper tracks matrix-rust-sdk releases continuously (0.x churn is a permanent tax; falling behind is the failure mode) and never forks protocol behavior away from Element X-compatible semantics.

## 9. Success Metrics

**Primary**

- **SM-1 Daily-driver conversion:** the maintainer plus ≥ 5 early adopters use keeper as their primary messenger (Beeper/Element retired) within 3 months of first beta. Validates the whole FR set; the product thesis in one metric. [ASSUMPTION] The brief says "the maintainer and early adopters"; the ≥ 5 target is authored for measurability.
- **SM-2 MVP demo bar:** Telegram, WhatsApp, and Signal each pass the end-to-end gate — native Bridge login, send/receive with E2EE, media, reactions, FTS over their history — on both a self-hosted Homeserver and a Beeper Account (cloud + bbctl). Validates FR-8–FR-17, FR-25–FR-32, FR-34. Binary, demo-able, release-gating.
- **SM-3 Reliability bars:** zero known silent-message-loss incidents in beta (NFR-5); Bridge Session drops surfaced within 60 s in 100% of induced-failure tests (FR-28/NFR-6); notifications delivered while backgrounded in ≥ 99% of test events (FR-51/NFR-7).

**Secondary**

- **SM-4 Performance bars:** NFR-1 (< 2 s cold start), NFR-2 (< 200 ms FTS at 100k+ events), NFR-3 (memory budgets) measured in CI on reference hardware and green at release.
- **SM-5 Archive trust:** Export of a 10k+-message Chat is complete and well-formed (count-verified vs. Local Archive, FR-35); Local Archive survives sign-out/re-login in upgrade tests (FR-37).
- **SM-6 OSS traction (12 months):** 1,000+ GitHub stars, listed on awesome-selfhosted, ≥ 3 external contributors with merged PRs, and an HN/r/selfhosted launch where the "open-source Beeper" framing demonstrably lands (front-page thread or equivalent).

**Counter-metrics (do not optimize)**

- **SM-C1 Network count:** number of supported Networks must not grow at the expense of the three flagship Networks' reliability — a 4th network added while SM-3 is red is a regression, not progress. Counterbalances SM-2/SM-6.
- **SM-C2 Launch hype vs. retention:** stars and launch-day traffic (SM-6) must not be pursued with promises the MVP can't keep (calls, iMessage, zero-setup onboarding); the daily-driver metric (SM-1) outranks traction optics.
- **SM-C3 Onboarding conversion:** do not chase setup-cliff conversion by adding hosted convenience services — the client-only constraint (§8) is load-bearing; conversion improves through the Wizard and docs only.

## 10. Open Questions

1. **Technical spike confirmation** — the pre-PRD spike (matrix-rust-sdk 0.18 in a Tauri 2 shell: SSS, E2EE, FTS-over-SQLite on macOS) was recommended by market research §6.4; if not yet green, it gates architecture sign-off, not this PRD. Owner: architecture phase.
2. **Homeserver recommendation** for the companion-stack docs (Synapse vs. conduwuit for single-user deployments). Owner: architecture phase.
3. **hungryserv C-S API surface** — which MVP features degrade on matrix.beeper.com's partial implementation (test against a real Beeper Account early; affects FR-3, FR-8, FR-39). Owner: architecture/first implementation epic.
4. **Agent-proposed Drafts demand** — validate with ~10 design partners before promoting the propose-only API beyond a v1.x flag. Owner: PM, post-MVP.
5. **Problem-interview ranking** — 5–8 interviews with self-hosted-bridge users to rank bridge UX vs. archive vs. incognito vs. approval-Drafts; may reorder v1.x fast-follows (not MVP composition). Owner: PM, during MVP build.
6. **FTS architecture for non-Latin scripts** — tokenization/CJK behavior of SQLite FTS for a global user base; requirement is FR-34, approach is architecture. Owner: architecture phase.
7. **At-rest encryption default** (NFR-10) and **memory budget confirmation** (NFR-3) — assumption-tagged thresholds need owner sign-off before they become release gates.

## 11. Risks (Register)

- **Beeper's on-device pivot shrinks the third-party surface** — more Networks migrate off matrix.beeper.com through 2026; keeper's durable play is self-managed Bridges. Mitigation: FR-7 disclosure, FR-29 bbctl path, Desktop-API companion mode deferred but scoped.
- **Beeper private API breakage** — FR-3 can break without notice. Mitigation: provider isolation (§8), distinct failure states, standard Matrix as the foundation.
- **matrix-rust-sdk 0.x churn** — breaking changes every minor. Mitigation: thin wrapper layer, upgrade every release, track Element X (§8 upstream posture).
- **Network ToS enforcement (Meta/X)** — login friction and rare bans are the user's risk, disclosed honestly. Mitigation: FR-30 risk tiers, no automation features ever (§5).
- **Setup cliff bounds the market** — MVP addressable users = homeserver owners + Beeper Account holders. Mitigation: First-Run Wizard as core product (FR-31), companion-stack docs, managed-host pointers. Accepted, not solved.
- **Solo/small-team velocity vs. a funded competitor** — Beeper ships monthly. Mitigation: ride upstream (Beeper funds bridges, Element funds the SDK), scope discipline via §5/§6, public release rhythm targeted at SM-6.

## 12. Assumptions Index

- §4.2 FR-13 — Voice-note recording deferred to v1.x; MVP plays received audio, sends audio as files.
- §4.3 FR-20 — Archived Chats auto-return on new activity (Beeper convention); "stay archived" override is v1.x.
- §4.3 FR-24 — Per-Network filtering ships as a simple filter; Beeper-style custom views are v1.x.
- §4.4 FR-25 — Bridge-discovery mechanism left to architecture; requirement is zero-config detection on standard mautrix deployments.
- §4.4 FR-29 — bbctl sidecar scope: launch-on-demand + status in MVP; full lifecycle supervision v1.x.
- §4.5 FR-33 — Message text retained indefinitely by default; media blobs on configurable cache policy (default keep).
- §4.5 FR-36 — Local Archive preserves remotely edited/deleted content by default, with a settings toggle to honor remote deletions; local-store-only, disclosed in settings.
- §4.5 FR-37 — "Survives logout where feasible": decrypted-before-sign-out content persists; never-synced encrypted history is not recoverable.
- §4.6 FR-39 — Draft conflict resolution: local version wins, remote surfaced for one-tap adoption.
- §4.7 FR-43 — No separate typing-only toggle in MVP; bundled with Incognito Mode.
- §4.7 FR-46 — Undo-Send Window runs at approval time; offline-queued messages that outlived their window dispatch on reconnect.
- §4.9 FR-54 — Notification quick-reply is v1.x; MVP is click-through.
- §7 NFR-3 — Memory budgets (500 MB / 300 MB) are authored numbers pending owner confirmation.
- §7 NFR-4 — Interaction-latency thresholds (150 ms switch, 16 ms input, 60 fps) authored for testability.
- §7 NFR-10 — At-rest encryption off by default (FileVault-typical Macs); confirm.
- §7 NFR-14 — MVP accessibility bar is "operable + labeled"; full VoiceOver polish v1.x.
- §9 SM-1 — "≥ 5 early adopters" target authored for measurability; brief left the count open.

**Phase 2 (§13):**

- §13 FR-60 — Full Dynamic Type adoption is fit-and-finish; the phase bar is graceful rem-based scaling.
- §13 FR-62 — App badge counts total unreads across all Accounts (same aggregate as the Unified Inbox).
- §13 FR-65 — The iOS Local Archive slice is excluded from device backup; the desktop Local Archive remains the durable, exportable copy this phase (disclosed in docs).
- §13 NFR-15 — 3 s on-device cold-start bar is an authored number pending owner confirmation before release-gating.

**Phase 3 (§14):**

- §14 FR-68 — One capture target per Recording Session; simultaneous multi-target capture is out of this phase.
- §14 FR-70 / NFR-22 — Screen↔camera alignment bound (one video frame at the configured frame rate) is an authored number.
- §14 FR-71/FR-72 — Defaults authored from the research's product synthesis: `~/Movies/keeper` folder, 500 MB segments, 30-minute duration-cap fallback, 30 fps.
- §14 NFR-19 — The 4 h continuous-soak bar is an authored number ("e.g. 4 h" in the owner ask) pending confirmation before release-gating.
- §14 NFR-20 — Disk-guard thresholds (warn below 10 GB free, stop below 2 GB) are authored defaults.
- §14 NFR-21 — CPU/memory envelope numbers are authored pending measurement on reference hardware.

**Phase 8 (§15):**

- §15.1 FR-378 — The phase is specified and accepted on macOS desktop; whether the `bots` capability flag is ever true on iOS is left to the capability handshake.
- §15 FR-379 — Removing a Provider keeps its conversations readable from keeper's store as orphaned rows; nothing is deleted with the Provider.
- §15 FR-392 — Image size and count caps are authored at story time and disclosed in the UI; no number is fixed in the PRD.
- §15 NFR-46 — The silence bound's value is a `keeper-core` constant authored at story time, restated from `keeper-sync`'s policy, and named in the stopped-stream message.

## 13. Phase 2: iOS/iPhone Client

*Added 2026-07-09, after the macOS MVP shipped complete. This section is the Phase 2 increment: it specifies only what iOS adds or constrains, continues the global numbering (FR-55–FR-65, NFR-15–NFR-18), and adopts — not relitigates — the recommendations and risk register of the iOS technical research (`_bmad-output/planning-artifacts/research-ios-2026-07-09.md`). Sections §1–§12 remain authoritative for all shared behavior.*

### 13.1 Phase Goal

keeper runs on the owner's iPhone as a first-class client: the same Rust core and the same React frontend as macOS — one codebase, one IPC contract, no forked chat components. Every MVP capability that iOS permits behaves identically to desktop; every one it forbids (background sync/push, bbctl sidecar processes, global hotkeys, in-app updates, menu-bar tray) is hidden by capability flags or disclosed honestly — never silently broken.

Distribution this phase is free Apple ID Personal Team signing: 7-day provisioning profiles re-armed from the owner's Mac, ~3 registered devices, no TestFlight or App Store. The audience is deliberately the owner-developer (plus hand-provisioned testers) dogfooding daily — SM-1's daily-driver bar extended to the phone. Nothing in this phase requires the paid Apple Developer Program; it is an explicit deferred decision gate (§13.5), not an omission.

The phase opens with a UI-free walking skeleton that retires the three existential risks — toolchain, signing, core-on-iOS — before any UX investment (per AD-24 Plan A: Tauri mobile reusing keeper-core and the existing IPC contract).

### 13.2 Features & Requirements

#### 13.2.1 Platform Target & Build Seam

##### FR-55: iOS app target
System builds and runs as a native iOS app (`tauri ios`) from the existing workspace: keeper-core linked as a static library, the React frontend in WKWebView, free Personal Team signing.
**Consequences (testable):**
- `tauri ios dev` runs the app in the iOS Simulator and on the owner's iPhone (Personal Team signing via development-team config, Developer Mode enabled, certificate trusted on device); desktop build behavior is unchanged.
- Walking-skeleton gate (phase-gating, before major UI work): on-device OIDC login completes via the `keeper://` deep link, the room list loads, text send/receive works in one E2EE Room, and app relaunch restores the session without re-login.
- After the 7-day profile expiry, re-signing restores launch with all local data intact (stable bundle identifier).
- CI runs an iOS compile check (`cargo check --target aarch64-apple-ios`) as a required PR gate so desktop work cannot silently break the port.

##### FR-56: Desktop-only code excluded from the iOS build
System compile-gates desktop-only surfaces out of the iOS target — tray/menu-bar, global-shortcut, autostart, updater, window-state, desktop deep-link registration — while the iOS shell registers only the notification and mobile deep-link plugins plus the shared IPC and media protocol.
**Consequences (testable):**
- The full workspace compiles for the iOS target with desktop-only plugins absent from the binary; desktop builds remain byte-identical in behavior.
- iOS updates arrive by reinstall/re-sign; no in-app updater code path exists on iOS (surfaced per FR-57).

##### FR-57: Platform capability flags
System exposes platform capability flags over the IPC handshake; the UI hides surfaces unsupported on iOS: bbctl sidecar integration (FR-29), global hotkey settings (FR-50), updater controls, and tray/menu-bar + launch-at-login settings (FR-53's background-presence options).
**Consequences (testable):**
- With a capability off, its affordances do not render at all — no dead buttons, no error-on-tap; if reached programmatically, the sidecar path returns a clean "unsupported on this platform" error.
- Bridge management beyond bbctl remains fully functional on iOS: discovery, native provisioning login, Bridge Bot fallback, Bridge Session health + re-login prompts, Network Risk Tier labels, start-new-Chat (FR-25–FR-28, FR-30–FR-32).
- Capability flags are data-driven per platform so later targets (Android, iPad) reuse the same mechanism.

#### 13.2.2 Phone UX

##### FR-58: Phone layout tier
User on a phone-width viewport (< 768 px) gets a single-pane navigation stack — Inbox → Room → Detail — reusing the existing components and selection state; desktop and tablet tiers are unchanged at ≥ 768 px. Realizes UJ-3 on the phone.
**Consequences (testable):**
- Unified Inbox, timeline, and detail render full-screen as pushed stack levels with a back affordance; back returns to the inbox preserving scroll position.
- No forked chat components: the same component trees render in a new arrangement container driven by existing selection state.
- The account/space rail becomes a drawer or inbox-header affordance; Command Palette functionality maps to pull-down search on phone.

##### FR-59: Safe areas and keyboard avoidance
System renders edge-to-edge respecting iOS safe areas, and the composer is never covered by the on-screen keyboard.
**Consequences (testable):**
- No unstyled bands at the notch or home indicator; header, composer, sheets, and overlays respect safe-area insets in portrait and landscape; the window background matches the theme (no launch or rotation flash).
- Opening the keyboard lifts the composer above it; a timeline already at bottom stays pinned to bottom; dismissing the keyboard restores layout with no stranded offsets or overshoot.

##### FR-60: Touch idioms
User can operate every MVP interaction by touch: long-press opens the same context menus as desktop right-click; edge-swipe navigates back in the stack; swipe actions on inbox rows expose archive/mute; pull-to-refresh on the inbox triggers an immediate sync. Realizes UJ-3 on the phone.
**Consequences (testable):**
- Every context-menu action is reachable by touch; all tappables are ≥ 44 pt; system text-selection callouts and tap highlights are suppressed where they fight custom menus.
- Pull-to-refresh visibly kicks the sync loop (the same action as foreground resume, FR-61).
- Text sizing is rem-based so system Dynamic Type scaling degrades gracefully. [ASSUMPTION] Full Dynamic Type adoption is fit-and-finish, not phase-gating.

#### 13.2.3 iOS Platform Behavior

##### FR-61: Lifecycle-aware sync with honest disclosure
System pauses the sync loop gracefully when the app backgrounds and resumes it with an immediate sync on foreground. keeper claims no background delivery: without push (paid-program gate, §13.5) there is no sync and no notification while backgrounded or suspended — and the UI and docs say so plainly.
**Consequences (testable):**
- Backgrounding stops the sliding-sync long-poll within seconds rather than letting it die mid-flight; foregrounding renders cached state instantly and shows new messages within 2 s on Wi-Fi.
- A first-run/settings disclosure states that on iPhone keeper syncs and notifies only while open, and that background notifications await an explicit future decision — no fake "push while closed" promise anywhere (extends FR-53's honesty rule).

##### FR-62: Foreground notifications and app badge
System posts local notifications for new messages while the app is active — same content, preview, and mute/mention-only semantics as FR-51/FR-52 — and keeps the app icon badge equal to the unread count, updated on each sync.
**Consequences (testable):**
- Notifications for the currently visible Chat are suppressed (reusing desktop logic); previews-off and mute settings behave identically to macOS.
- The badge reflects unread state as of the last sync and refreshes on foreground resume; it does not pretend to be live while suspended. [ASSUMPTION] Badge counts total unreads across all Accounts (the Unified Inbox aggregate).

##### FR-63: iOS keychain sessions
System stores session tokens and secrets in the iOS keychain through the existing platform seam, available after first unlock and never synced off-device.
**Consequences (testable):**
- Keychain items use after-first-unlock, this-device-only accessibility: readable by a resumed sync loop, invisible to other apps, excluded from iCloud Keychain — a Matrix device identity must never be cloned to another device.
- Sessions survive app relaunch and 7-day re-sign cycles without re-login.

##### FR-64: Media protocol on WKURLSchemeHandler
System serves decrypted media on iOS through the same `keeper-media://` custom protocol with an identical URL format to macOS, including Range (200/206/416) support for seeking.
**Consequences (testable):**
- Encrypted images render in the timeline; video plays and seeks on-device (Range/206 path exercised); decrypted bytes never pass through IPC JSON — NFR-9 holds unchanged on iOS.
- The retry-on-cache-miss path works after force-quit.

##### FR-65: Backup exclusion and file protection for local stores
System excludes keeper's databases (sync stores, crypto stores, Local Archive) from iCloud/device backup and applies a file-protection class that keeps a resumed sync loop working.
**Consequences (testable):**
- DB directories carry the backup-exclusion flag — multi-gigabyte, re-syncable state does not bloat user backups; files use the complete-until-first-user-authentication protection class (encrypted at rest without breaking database access after screen lock).
- All account state lives under one data-directory root so a future App Group container move (NSE era) is a path change, not a migration of scattered files.
- [ASSUMPTION] Backup exclusion covers the iOS Local Archive slice; the desktop Local Archive remains the durable, exportable copy this phase (FR-33–FR-37 promises stay anchored there), disclosed in the iOS docs.

### 13.3 Phase NFRs

*Continues §7's numbering. Measured on the owner's device (iPhone 12-class or newer), release build, real accounts.*

- **NFR-15 Cold start on device:** launch → interactive Unified Inbox (cached Chats rendered, input accepted) in **< 3 s**. [ASSUMPTION] Authored bar (desktop NFR-1 is 2 s on Apple Silicon); confirm before release-gating.
- **NFR-16 Memory hygiene under jetsam:** with multi-account sync running, keeper drops droppable caches (image memory cache, media byte buffers) on backgrounding and memory warnings; the in-memory media Range-slicing buffer is capped; a 24 h suspended soak with a large account survives without a jetsam kill; memory returns near baseline after backgrounding (Instruments-verified).
- **NFR-17 Flaky-network resilience:** the UI always renders instantly from the local mirror; the sync loop uses Simplified Sliding Sync offline mode with backoff and exits it immediately on demand; airplane-mode toggles and Wi-Fi↔cellular handovers recover unaided; a stale resume (foreground with last sync minutes old) shows cached UI at once, kicks sync, and surfaces a subtle "connecting" state — including a sync-loop restart guard for the known stale-session edge (matrix-rust-sdk#3935).
- **NFR-18 Resume integrity:** resuming from background — including overnight suspension — never leaves a blank or unresponsive webview (Tauri #14371); a reload guard detects a jettisoned webview process and restores the UI; this scenario is acceptance-tested from the walking skeleton onward.

### 13.4 Out of Scope (this phase)

- **APNs push and the Notification Service Extension** — the paid-program decision gate (§13.5). Impossible on free signing (blocked entitlements); deferred by explicit decision, not omission.
- **App Store / TestFlight distribution**, and every other paid-program-dependent capability: App Groups, `https://` universal links, AltStore PAL notarization.
- **iPad layout and Android** — later phases. The phone tier (FR-58) and capability-flag mechanism (FR-57) are deliberately platform-neutral so they carry over; Android's media-URL remapping helper is introduced only when Android starts, not speculatively.
- **Calls** — unchanged (§5).
- **iOS extras with no phase justification:** share extension, home-screen widgets, Siri intents, biometric app lock (mobile plugins exist; nothing this phase depends on them).

### 13.5 Paid Apple Developer Program — the decision gate

The single deliberate deferral of this phase. The $99/yr program is the sole unlock for APNs push, the NSE (background notification decryption, with its 24 MB memory ceiling and App-Group store-layout implications — kept cheap now by the single data-dir root, FR-65), TestFlight, App Groups, and AltStore PAL notarization for EU distribution. The gate opens only when push becomes a product goal — and it then forces a PRD-level question that keeper's client-only constraint makes hard: push must ride a homeserver operator's gateway, Beeper's, or a user-run Sygnal — never project infrastructure (NFR-11). Until the gate: the 7-day re-arm ritual is documented in the iOS docs, AltServer auto-refresh is the optional quality-of-life path, and test IPAs are shared via per-tester re-signing.

### 13.6 Phase Success Metrics

- **SM-7 Walking-skeleton gate:** the FR-55 on-device gate (OIDC login via deep link, room list, E2EE text send/receive, relaunch-restore) passes before phone-UX work begins — the AD-24 Plan A validation, binary and demo-able.
- **SM-8 Phone daily-driver:** the owner uses keeper on iPhone as the primary phone messenger for ≥ 2 consecutive weeks — triage and replies happen on the phone, zero silent-loss incidents (NFR-5 extended to iOS), NFR-15–NFR-18 bars green, and the 7-day re-arm costs minutes per week, not hours.

### 13.7 Phase Risks (Register)

Adopted from the research risk register (research §5):

- **Blank webview on resume (Tauri #14371)** — medium likelihood. Mitigation: reload guard (NFR-18), tested first thing in the walking skeleton, upstream fix tracked.
- **`keyring` crate misbehaves on the iOS keychain** — medium. Mitigation: spike inside the walking skeleton; contained fallback to direct Security-framework calls behind the existing platform seam (FR-63).
- **Keyboard/scroll quirks in the composer (WKWebView)** — high likelihood of quirks, low of blockers. Mitigation: time-boxed keyboard-avoidance work with documented patterns and a simpler viewport fallback (FR-59).
- **7-day expiry friction erodes dogfooding** — medium. Mitigation: AltServer auto-refresh, documented weekly re-arm ritual; SM-8 explicitly tracks the cost.
- **Large-media RAM slicing trips memory pressure** — low–medium. Mitigation: buffer cap (NFR-16); disk-backed streaming recorded as deferred work.

### 13.8 Phase Decisions & Open Questions

**Pre-answered (adopted from the research; revisit only on evidence):**

- Minimum iOS version: **16.0**, set explicitly in the generated project.
- Bundle identifier: **same as macOS** — no shared-container conflicts exist on free signing, and it keeps deep-link registration coherent.
- **No routing library** this phase — the phone stack is a projection of existing selection state; history integration is an optional enhancer for system back-gesture semantics.
- **Plan B (UniFFI + native SwiftUI shell) stays shelved.** Revisit triggers, recorded here as the research directs: (a) the blank-webview bug class proves unfixable across Tauri releases; (b) NSE work begins — noting the NSE is a Rust+Swift target under Plan A regardless, so even that is not a shell rewrite.

**Open:**

1. NFR-15 cold-start number needs owner confirmation before it becomes a release gate. Owner: product owner, at phase release.
2. Paid-program timing — the §13.5 gate itself. Owner: PM/owner, when push demand is real.

## 14. Phase 3: Screen Recording (macOS)

*Added 2026-07-16. This section is the Phase 3 increment: it specifies only what screen recording adds, continues the global numbering (FR-66–FR-76, NFR-19–NFR-22), and adopts — not relitigates — the recommendations and risk register of the recording technical research (`_bmad-output/planning-artifacts/research-recording-2026-07-16.md`). Sections §1–§13 remain authoritative for all existing behavior.*

### 14.1 Phase Goal

keeper records the user's on-screen activity — meetings, presentations, demos — to ordinary local video files in a folder the user chose. The user picks what to capture (a full display or a single application), which audio rides along (system audio on by default, a microphone from a device picker), and optionally a webcam recorded as a separate synchronized file. Recording runs continuously for hours, saving size-bounded segments as it goes, so a crash — keeper's, the recorder's, or the machine's — costs at most the last few seconds, never the meeting. The menu bar always tells the truth: recording state, elapsed time, one-click Stop; every fault is loud, never silent.

The capability is macOS-desktop-only, gated at macOS 13.0 through the existing per-platform capability-flag mechanism (FR-57's `CapabilitiesVm`); the app-wide minimum stays 11.0 and iOS never records. The capture pipeline lives in a small first-party Swift sidecar (`keeper-rec`) spawned on demand through the existing sidecar mechanism — keeper itself owns the UI, settings, tray, and session manifest; a recorder crash can never take the messaging app down (route, format, and floor locked by the research; recorded in §14.7 and the addendum). Like every keeper feature, recording is local-only: files stay on the machine and the feature adds zero new network destinations.

The phase opens with a walking skeleton that retires the existential risks — TCC permissions, sidecar signing, and the capture-to-file pipeline — before feature breadth (research epic sketch R.1).

### 14.2 Features & Requirements

#### 14.2.1 Capability Gating & Permissions

##### FR-66: Recording capability gating
System exposes screen recording as a `recording` capability flag over the IPC handshake, present only on desktop macOS ≥ 13.0; every recording surface — Settings section, tray affordances, Command Palette actions — renders only when the flag is on. Reuses FR-57's capability-flag mechanism.
**Consequences (testable):**
- On macOS < 13.0 and on iOS, no recording affordance renders anywhere — no dead buttons, no error-on-tap; the app-wide `minimumSystemVersion` stays 11.0.
- The flag is data-driven per platform, so recording on a future Windows/Linux target reuses the same mechanism without UI rework.
- Internal version branches (e.g., in-stream microphone capture on macOS 15+) never change the user-visible feature set across 13.0+.

##### FR-67: Permission pre-flight with honest states
User sees an explicit permission pre-flight before recording can start: keeper live-detects and displays the true state of Screen Recording — plus Microphone and Camera when those sources are enabled — requests each via the system prompt where the OS allows, and deep-links to the exact System Settings pane when only manual granting remains.
**Consequences (testable):**
- Each permission renders one of granted / not yet requested / denied-with-fix-path; the displayed state is detected at render time, never cached optimistically; Start is disabled until every required permission is granted, with the blocking permission named.
- The Screen Recording flow states macOS's quirks plainly: a relaunch may be needed after granting, and macOS 15+ re-confirms the grant monthly — disclosed, not hidden.
- Microphone and Camera permissions are requested only when the user enables those sources — never preemptively.
- Permission revocation mid-recording surfaces as a loud failure per FR-75, with already-written segments intact.

#### 14.2.2 Capture Sources

##### FR-68: Source selection — full screen or a selected application
User chooses what to record: a full display (with its audio) or a single running application; the picker lists live displays and running applications with names and icons.
**Consequences (testable):**
- Recording a selected application captures only that application's windows — and, with system audio on, only that application's audio; other windows, keeper itself, and incoming notification banners from other apps never appear in the file.
- On multi-display setups each display is individually selectable. [ASSUMPTION] One capture target per Recording Session; simultaneous multi-target capture is out of this phase.
- The source list refreshes as applications launch and quit; picking a source that has since disappeared yields a clear error at start, never a hung recording.

##### FR-69: Audio sources — system audio toggle and microphone picker
User can toggle system audio (default: on) and select a microphone from a device picker (default: system default input); each enabled audio source is written as its own track in the screen file — never premixed.
**Consequences (testable):**
- The screen file carries up to two AAC tracks (system audio, microphone) that stock players (QuickTime, browsers, VLC) play together and editors can separate; muting or removing one side later is always possible.
- keeper's own notification sounds are excluded from system-audio capture — a message arriving mid-meeting never lands in the recording's audio.
- Microphone hot-unplug mid-recording never aborts: video and system audio keep rolling, the mic track continues (silence-filled), keeper attempts fallback to the system default input, and a persistent warning state is raised (FR-74/FR-75).

##### FR-70: Optional webcam as a separate synchronized file
User can enable a webcam from a device picker (built-in, external, Continuity Camera; default: off); the camera records to separate files in the same session folder, time-anchored to the screen recording and rotated at the same segment boundaries.
**Consequences (testable):**
- With webcam on, the session folder contains `camera-####` files whose segment boundaries match `screen-####`; played side by side from any segment index, the two stay aligned within one video frame at the configured frame rate. [ASSUMPTION] The one-frame bound is authored; confirm on reference hardware.
- Webcam off produces no camera files and touches no Camera permission; camera loss mid-recording follows FR-69's never-abort rule (screen recording continues, warning raised).
- No picture-in-picture burn-in this phase (§14.4); UX copy may note that macOS 14+ can composite the camera via the system presenter overlay — a free OS behavior, not a keeper feature.

#### 14.2.3 Output, Segmentation & Recovery

##### FR-71: Recording Session output — chosen folder, session folder, manifest
User picks — and keeper remembers — a recordings folder (default `~/Movies/keeper`); each recording creates one timestamped session folder containing the segment files and a `manifest.json` describing the session (capture target, devices, segment list, status).
**Consequences (testable):**
- Files land exactly where the user chose; changing the folder in Settings affects future sessions only; folder validation (exists, writable, free space per NFR-20) runs before start with actionable errors.
- Segment names are local-time-stamped, filesystem-safe, and lexicographically ordered; the manifest updates atomically at every segment close and status change — an external tool can always read a consistent manifest.
- Cleanly finalized segments are ordinary `.mp4` files (H.264 + AAC) playable everywhere with no keeper-specific tooling.

##### FR-72: Continuous segmented recording with size-based rotation
System records continuously, rotating to a new segment when the current file reaches the user-configured segment size (default 500 MB), with a duration-cap fallback (default 30 min) so low-motion recordings still rotate; rotation is gapless.
**Consequences (testable):**
- A recording spanning N segments concatenates into playback with no missing or duplicated frames and continuous timestamps (bar: NFR-22); rotation causes no pause, no dropped audio, no user-visible hiccup.
- Segment size is user-configurable in Settings; the configured value is respected within one keyframe interval of file growth.
- [ASSUMPTION] 500 MB / 30 min defaults authored from the research's product synthesis; adjust on dogfooding evidence without PRD change.

##### FR-73: Crash safety and startup recovery
System writes segments in a crash-safe fragmented format so that any interruption — recorder crash, keeper crash, power loss — loses at most the last fragment (~4 s); on startup and before each new recording, keeper scans for interrupted sessions, marks them recovered in their manifests, and surfaces a notice.
**Consequences (testable):**
- Force-killing the recorder mid-segment leaves the partial segment playable up to the last complete fragment; every earlier segment of the session is untouched.
- The recovery notice ("A recording was interrupted; N segments were saved") appears once per interrupted session and links to the session folder; recovered files play as-is, with no remux step required.
- An interruption during recording additionally surfaces live as an error per FR-75 — recovery is the safety net, not the notification.

#### 14.2.4 Control Surface & Honesty

##### FR-74: Tray/menu-bar recording state with elapsed time and Stop
System shows recording state in the menu bar — idle / recording / warning-error — with live elapsed time and current-segment info while recording, and one-click Stop Recording and Open Recordings Folder actions; recording forces the tray visible even when the user's opt-in tray toggle (FR-53) is off, restoring the prior tray state at stop.
**Consequences (testable):**
- Within 1 s of start the tray reflects recording; a ~1 Hz tick updates an elapsed/segment line ("Recording — 12:34 · segment 3, 412 MB"); Stop finalizes the current segment and returns the tray to its prior configuration exactly.
- Quitting keeper while recording warns first, then stops and finalizes cleanly before exit (kill-timeout guarded) — extending FR-53's quit honesty; it never orphans a running recorder.
- macOS's own screen-recording indicator (the menu-bar pill) remains untouched; keeper's tray adds what the system pill lacks — elapsed time, segment info, Stop, and error states.

##### FR-75: Loud failure surfacing — no silent recording loss
System surfaces every recording fault loudly — recorder crash or unexpected exit, writer stall, permission revocation, device loss, disk-guard triggers — via the tray error state plus a native notification; no recording failure mode is silent.
**Consequences (testable):**
- Killing the recorder process flips the tray to error and posts a notification within 5 s (the FR-51/NFR-7 pipeline), offering one-click restart of the recording; the session manifest records the true terminal status.
- Non-fatal warnings (mic unplug, low disk) show a persistent warning state until resolved or acknowledged — never a dismissed-and-gone toast (FR-28's persistence rule applied to recording).
- NFR-5's no-silent-loss rule extends to recordings: every started Recording Session reaches a user-visible terminal state — finalized, recovered, or failed-with-reason.

##### FR-76: Local-only recording — zero new egress
System keeps recording entirely local: recordings, manifests, and recording settings never leave the machine, and the feature adds zero new network destinations to keeper's documented egress surface (NFR-11).
**Consequences (testable):**
- The per-release egress inventory diff (NFR-11) is empty for this phase — verifiable at review and at runtime (no new hosts contacted during a full record/stop/recover cycle).
- No upload, share-link, transcription, or cloud-processing affordance exists anywhere in the recording UI; sharing a recording is the user's act with ordinary files, outside keeper.

### 14.3 Phase NFRs

*Continues §7's numbering. Measured on Apple Silicon (M1 or later), release build, signed per §14.7, unless stated.*

- **NFR-19 Long-run capture stability:** a **4 h** continuous recording (1080p-class display, 30 fps, system audio + microphone) completes with zero recorder crashes, writer stalls, or A/V desync and no unbounded memory growth; sample-buffer queues are bounded with a drop-oldest-video policy — audio is never dropped — and sustained dropping raises a warning (FR-75). [ASSUMPTION] The 4 h bar is authored; confirm before release-gating.
- **NFR-20 Disk-space guard:** recording warns when free space on the target volume falls below a warning threshold and gracefully stops-and-finalizes below a hard floor — it never runs the disk to exhaustion or dies mid-write. [ASSUMPTION] Defaults: warn below 10 GB free, stop below 2 GB; authored pending confirmation.
- **NFR-21 Recording performance envelope:** recording 1080p-class content at 30 fps with both audio tracks adds **< 100% of one core** average CPU and **< 400 MB** combined RSS (sidecar + keeper overhead), and keeper's messaging bars (NFR-1–NFR-4) still hold while recording — a meeting is exactly when the messenger must stay responsive. [ASSUMPTION] Numbers authored; measure on reference hardware before gating.
- **NFR-22 Segment handover gaplessness:** rotation cuts on keyframes with continuous host-clock-anchored timestamps; concatenating a session's segments yields monotonic timestamps with no gap or overlap exceeding one frame duration, and screen↔camera alignment holds within one frame across the full session; an automated concatenate-and-assert test gates release.

### 14.4 Out of Scope (this phase)

- **Video editing — never** (§5). keeper records; it does not trim, annotate, or compose.
- **Any cloud upload, share service, or remote processing — never** (§5, FR-76).
- **Pause/resume**, **webcam PiP burn-in**, and a camera self-view preview bubble — later stories, deliberately after the capture core is trustworthy.
- **`SCContentSharingPicker` system-picker path** (macOS 14+, also silences the monthly re-auth nag), **HEVC/HDR capture**, **DND-while-recording**, and an orphan-segment "tidy" remux pass — later.
- **Windows/Linux recording** — follows those platforms (§6.2); the capability flag (FR-66) and the platform-free recording module are built to carry over.
- **The `persistent-content-capture` entitlement** (would remove the monthly re-auth nag) — requires the paid Apple Developer Program and an Apple approval process; sits behind the §13.5-class paid-program gate, accepted and documented instead (§14.7).

### 14.5 Phase Success Metrics

- **SM-9 Recording end-to-end gate:** on a Development-signed build on macOS 13+ hardware: permission pre-flight → full-screen *and* app-scoped recording with system audio + microphone (+ webcam as a separate file) → segments rotate at the configured size into the chosen folder with a valid manifest → an induced crash recovers per FR-73. Binary, demo-able, release-gating. Validates FR-66–FR-76.
- **SM-10 Recording reliability bars:** the NFR-19 soak green; the induced-failure matrix (recorder kill, mic unplug, disk floor, permission revoke) surfaces loudly in 100% of tests (FR-75); zero silent recording-loss incidents during dogfooding; the NFR-11 egress diff for the phase is empty (FR-76).

### 14.6 Phase Risks (Register)

Adopted from the research risk register (research §8):

- **TCC vs ad-hoc dev builds** — macOS 15+ silently rejects ScreenCaptureKit access for ad-hoc-signed binaries, and identity churn resets grants (Cap #1722). High (DevEx), not a product blocker: local development of this feature requires Apple Development-certificate signing (free account suffices; the iOS phase already established free-team signing); release builds are already Developer-ID signed + notarized. Recorded as a dev-signing requirement in the release docs.
- **Sidecar signing/notarization rough edge** (Tauri `externalBin`, #11992) — medium. Mitigation: explicitly codesign `keeper-rec` (hardened runtime + entitlements) in CI before bundling; aarch64-only shipping avoids the universal-binary step.
- **Monthly re-authorization nag on macOS 15+** for non-picker capture — low/medium. Mitigation: accept + disclose in MVP (FR-67); adopt the system-picker path later; the entitlement escape is paid-program-gated (§14.4).
- **Disk exhaustion during long recordings** — medium. Mitigation: NFR-20 guard; segment sizing keeps cleanup easy.
- **Long-run stability** (backpressure, writer stalls, thermal) — medium. Mitigation: bounded queues with drop-oldest-video (NFR-19), fragment-bounded data loss (FR-73), soak-test story, restart recovery via manifest.
- **Gapless-rotation correctness** (A/V sync across segments) — medium. Mitigation: keyframe-cut dual-writer handover, host-clock PTS, the NFR-22 automated concatenation test.
- **macOS API drift** (Tahoe+ permission UX changes) — low. Mitigation: all Apple API churn is isolated in the small Swift sidecar; the capability handshake lets keeper degrade gracefully.
- **Webcam/mic device churn** (Continuity Camera appearing/disappearing) — low. Mitigation: re-enumerate on device notifications; never hard-fail a running recording on device loss (FR-69/FR-70).

### 14.7 Phase Decisions & Open Questions

**Pre-answered (adopted from the research; revisit only on evidence):**

- **Architecture route locked:** a first-party Swift sidecar `keeper-rec` (ScreenCaptureKit + AVAssetWriter, ~1–2 kLOC, Apache-2.0, in-repo SwiftPM) controlled over NDJSON-RPC on stdio, spawned launch-on-demand through the existing sidecar mechanism (the bbctl precedent); keeper-core owns a platform-free recording module (state machine, manifest schema, settings), the shell owns spawn/stdio/tray glue. Keeps the workspace `unsafe_code = "deny"` posture intact and isolates capture crashes from messaging. In-process Rust bindings and an ffmpeg sidecar were evaluated and rejected (rationale in the addendum §8).
- **Capability floor 13.0** (system-audio capture requires it), runtime-gated via FR-66; internal macOS 15+ branch for in-stream microphone capture; app minimum stays 11.0; iOS never.
- **Format locked:** fragmented MP4 `.mp4`, H.264 video + up to two unmixed AAC tracks, ~4 s fragment interval, 30 fps default at source resolution (60 selectable), defragmented to ordinary MP4 on clean finalize.
- **Defaults:** system audio on, microphone = system default input, webcam off, 500 MB segments, `~/Movies/keeper`.
- **Dev-signing requirement:** local builds exercising recording must be signed with an Apple Development certificate (macOS 15+ ad-hoc rejection); a DevEx requirement documented in release/dev docs — explicitly not a product blocker.
- **TCC attribution:** the sidecar is spawned (never a LaunchAgent) so all permission prompts and System Settings entries attribute to keeper, using keeper's usage strings.

**Open:**

1. Authored bars need owner confirmation before they become release gates: NFR-19 soak duration, NFR-20 thresholds, NFR-21 CPU/memory envelope, the FR-70/NFR-22 one-frame alignment bound. Owner: product owner, at phase release.
2. In-app recordings browsing (a list of past sessions inside keeper) is deliberately unspecified — MVP is folder-and-Finder plus the tray's Open Recordings Folder. Revisit on dogfooding evidence. Owner: PM.

## 15. Phase 8: Bots — A Model You Can Talk To

*Added 2026-09-02, after v0.8.24. This section is the Phase 8 increment: it specifies only what talking to a model adds, continues the global numbering (FR-369–FR-393, NFR-46–NFR-49), and adopts — not relitigates — the verdicts of the AI-chat research (`_bmad-output/planning-artifacts/research-ai-chat-2026-09-02.md`) as bound by Epic 61 (`_bmad-output/planning-artifacts/epic-61-a-model-you-can-talk-to-in-the-app-that-holds-your-drive.md`), which is the authority for the stories. Phases 4–7 (folder sync, notes, recording × sync, sessions — FR-77–FR-368, NFR-23–NFR-45) were specified in `epics.md` and the per-epic files rather than here, which is why this section's numbers do not continue §14's; the sequence is global and unbroken. Sections §1–§14 remain authoritative for all existing behavior.*

### 15.1 Phase Goal

keeper already holds the drive, the notes, the sessions and the tasks; what it does not have is a way to *ask something about them*. This phase adds a fifth keeper surface — Bots, at ⌘9 — where the user talks to a model of their own: a Hermes Agent bot or an Ollama model, local or remote, over the OpenAI-compatible wire both already speak. The user configures any number of Providers side by side, each with its own credential, pins the bots they talk to with a shape, a colour and a mark, keeps every conversation in keeper's own store where it can be listed, searched, renamed, archived and resumed, switches per-message metadata on when they want it, and types slash commands the composer refuses honestly when it does not know them. A model may be given the drive — read or write, the whole drive, one profile or one subtree — through a grant the user can see and revoke in one act, and every byte it touches goes through the containment rule keeper already enforces for itself, is bounded, and is audited before it happens. An image pasted into the conversation reaches a model that can see; a path a model names is opened only inside a grant.

Like every keeper surface, this one is built the way the other four were: one vocabulary (Provider → bot → conversation → message), every decision in `keeper-core` with the shell as a call site, an egress row that is derived from the configured Providers and never hand-written, and no affordance that lies — a capability keeper could not read is `unknown`, never `false`. The surface is gated by a `bots` capability flag computed per platform in the shell (FR-57's `CapabilitiesVm`); chat needs neither `git` nor `sync.db`, so the flag is a genuinely new condition, while the drive-tool half needs the `sync` capability exactly and is gated on it. [ASSUMPTION] The phase is specified and accepted on macOS desktop; whether the `bots` flag is ever true on iOS is left to the capability handshake, not decided here.

**This phase requires no new server-side component, because keeper is a client only (§5, §8).** keeper ships no default endpoint, no hosted model, no relay and no telemetry: a base URL is required, never defaulted, and the endpoint is the user's — recorded as decision D-4 in `docs/decisions.md` by this phase. Two decisions are made before any story starts and are not re-argued in one: **one wire protocol** — `POST /v1/chat/completions` with `stream: true` is the only chat transport, and the per-kind divergences are rows in a quirk table, not a second client; and **keeper owns the conversation** — a Hermes `session_id` is a reference persisted beside the row, never the truth, because Hermes compresses sessions into renamed successors, caches only 100 stored responses, and Ollama has no session concept at all. The stack opens with the two strictly serial stories (a Provider record, then the streaming client) and places the grant **before** the first tool can read a byte; a story order that inverts that ships an unguarded filesystem for one PR's duration.

### 15.2 Features & Requirements

#### 15.2.1 Providers & Egress

##### FR-369: Providers side by side
User can configure any number of AI Providers side by side, each a record with a kind (`hermes` | `ollama`), a display name, a base URL, an optional bot/profile prefix, a timeout override and a health snapshot — the `SyncProfile` multi-tenant precedent applied to model endpoints. Story 61.1; AD-146.
**Consequences (testable):**
- Two Providers of the same kind with different base URLs coexist and are addressed independently; removing one leaves the other untouched.
- The base-URL grammar is a `keeper-core` decision with its own tests: scheme `http` or `https`, no userinfo, no path beyond a profile prefix; a loopback host and a private-network host are both legitimate **and both are disclosed** at save time — the SSRF question is answered by disclosure and an explicit user act, not by a blocklist.
- A Provider row holds no JSON blob (AD-139); a base URL is required and never defaulted, because keeper ships no endpoint of its own (§15.7, D-4).

##### FR-370: Credentials live behind the secret port
System stores a Provider's credential behind the existing secret port only — never in a `keeper.db` row, a log line, an error string or on a screen. Story 61.1; AD-147.
**Consequences (testable):**
- A Provider row that has lost its secret says so in the Providers list and in the pane, instead of failing at send time.
- The `Authorization` header is marked sensitive so it is redacted from error text; no log line and no error carries a token or a URL with userinfo.
- A credential is required for the `hermes` kind (bearer `API_SERVER_KEY`) and optional for `ollama`, which accepts and discards one; the Settings form says which.

##### FR-371: Derived egress
System lists every configured Provider in the egress inventory (Settings → About, `docs/egress.md`) as a derived row — host only, never a full URL — computed by `compute_egress` from the live Provider set, reusing the same host extraction that strips userinfo from git remotes. Story 61.1; AD-148.
**Consequences (testable):**
- Adding a Provider changes the disclosed egress set; removing it removes the row; no hand-maintained list exists anywhere (AD-53 extended to Providers).
- The NFR-11 per-release egress diff for this phase is **non-empty by design** and names exactly the Provider chapter of `docs/egress.md` (Story 61.13); the release note firing is the visibility working, not a regression.
- A loopback or private-network Provider appears in the inventory like any other host.

#### 15.2.2 The Wire

##### FR-372: Streaming chat over the OpenAI-compatible wire, with Stop
System sends every chat as `POST /v1/chat/completions` with `stream: true` — the only chat transport keeper implements, for both kinds — renders the reply progressively, and stops it on the user's act. Story 61.2; AD-149.
**Consequences (testable):**
- Stop aborts the request and leaves a partial assistant row persisted and marked partial — never a silently discarded reply.
- Per-kind divergences are rows in a quirk table, not code paths: Ollama's `/v1` layer ignores `tool_choice`, accepts images as base64 data URIs only, and cannot set the context window (`num_ctx` needs the native dialect, §15.4) — each recorded as data and disclosed where it bites.
- Acceptance is a local test server, not a mock of the client: a valid stream; a frame split across two chunks; a socket that dies mid-message; `[DONE]` with no usage; 401, 404 and 500; a socket held open sending nothing.

##### FR-373: Reassembly from fragmented deltas
System reassembles one message from fragmented server-sent events — `data:` lines, keep-alive comments, `[DONE]`, a frame split across two byte chunks — including index-keyed `tool_calls` fragments whose `arguments` arrive as partial JSON, and the `usage` block where the endpoint sends one. Story 61.2; AD-149.
**Consequences (testable):**
- Content, tool calls (by index) and usage survive any chunk boundary; the framer and the state machine are tested against a socket that misbehaves, not a pre-assembled fixture.
- A `[DONE]` with no `usage` yields a message whose token counts are absent, not zero (FR-384).
- No response body is read unbounded into memory; bytes are consumed in bounded chunks.

##### FR-374: A stream ends by silence, not by a deadline
System bounds a stream by silence — a read timeout between bytes — and never by a whole-request deadline: a long answer is not a fault; a silent socket is. Story 61.2; NFR-46.
**Consequences (testable):**
- A socket held open sending nothing is abandoned after the silence bound with a stated reason; a slow but flowing stream of any total length completes.
- The policy is restated in `keeper-core` with a comment naming `keeper-sync/src/http.rs` as its source — restated, because `keeper-core` may not depend on `keeper-sync` (AD-40), and not copied by accident.

#### 15.2.3 Discovery

##### FR-375: What an endpoint is and what it can do
System probes a Provider and reports what it is (kind, reachable, authenticated) and what it can do (models, per-model capabilities, health) — or says, in the house voice, why it cannot tell. Story 61.3; AD-150.
**Consequences (testable):**
- Hermes is read from `/v1/models` plus `/api/model/options` (providers, curated model lists, `supports_tools` / `supports_vision` / `supports_reasoning` / `context_window`); Ollama from `/api/tags`, because it carries `capabilities` for every local model in one round trip. Discovery is the only place the native endpoints are used.
- A capability keeper could not read is `unknown`, never `false`: an `unknown` vision flag offers the paste affordance with a warning; a `false` one hides it (AD-27).
- A real 401 and a real 404 from a wrong base URL each produce a distinct, actionable state, in the same vocabulary the app already uses for an unreachable remote.

##### FR-376: A bot is verified, never invented
User names a Hermes bot (a profile) and keeper verifies it exists by probing `/p/<name>/v1/models`; keeper never enumerates bots it cannot see, and the empty state says why. Story 61.3; AD-150.
**Consequences (testable):**
- The bearer-authenticated route table has no profile roster (the two doors that list profiles need a dashboard session token or a websocket RPC keeper does not speak), so no "discover bots" affordance exists — a named profile either probes true or false.
- A bot is (Provider, model-or-profile, identity): a Hermes bot is a profile addressed by the `/p/{profile}` URL prefix; an Ollama bot is a model tag and needs no probe beyond `/api/tags`.
- The empty state states, in the house voice, that a roster needs a door keeper is not given.

##### FR-377: Capabilities are read, not assumed
System reads each model's vision, tool and context-window support from the endpoint and stores it per model; nothing about a model is assumed from its name. Story 61.3; AD-151.
**Consequences (testable):**
- Ollama's `capabilities` array and Hermes' `supports_*` fields populate the flags; a field the endpoint omits is `unknown`.
- The paste affordance (FR-392) and the tool surface (FR-389) read these flags; a model whose tool flag is `false` is never sent tool definitions.

#### 15.2.4 The Surface

##### FR-378: A pane at ⌘9 that narrates its own state
User opens a Bots pane at ⌘9 — `PrimaryView` member `bots`, sidebar label Bots — gated by a `bots` capability flag, and the pane narrates its own state honestly: which Provider, which bot, streaming, stopped, unreachable, secret missing. Story 61.4; AD-152.
**Consequences (testable):**
- The flag is computed in the shell and mirrored in the frozen store default and the dev mock shell, or typecheck fails; where the flag is off the sidebar entry is absent, not disabled.
- The flag is a new condition, not a synonym for `sessions`: chat needs neither `git` nor `sync.db`; the grant affordance (FR-386) is additionally gated on `sync`, which is the honest split.
- ⌘9 — the only free digit — is bound by a hook cloned from the tasks shortcut with its five guards in order and its no-dead-chord rule; Stop stops; Retry re-sends; an unreachable endpoint uses the same words as an unreachable remote.

##### FR-379: Providers are added, tested and removed from Settings
User adds, tests, edits and removes a Provider from a Settings → Providers section built as one component in two modes (AD-C7). Story 61.4; AD-146.
**Consequences (testable):**
- Test runs the FR-375 probe and shows the result inline; controls are disabled until hydrated; a rejected write reverts to the last confirmed value.
- Removing a Provider deletes its secret through the same port and its egress row disappears (FR-371). [ASSUMPTION] Conversations that belonged to a removed Provider are kept and remain readable from keeper's store as orphaned rows; nothing is deleted with the Provider.

##### FR-380: An answer is prose and code, and nothing else is executed
System renders an answer as a markdown subset over the lezer stack already in the tree — prose and fenced code — with no remote asset fetch and no HTML injection. Story 61.5; AD-153.
**Consequences (testable):**
- No new dependency, no `dangerouslySetInnerHTML`, no URL fetched by rendering — the position the note protocol already takes: a note that auto-fetches a URL an agent wrote is a tracking pixel.
- Fenced code blocks carry a copy control and a language label; an unterminated fence mid-stream renders as code and closes itself; a 200 kB reply does not lock the webview.

#### 15.2.5 Conversations, Pins, Metadata & Slash Commands

##### FR-381: Conversations are listed, searched, renamed, archived and resumed
User sees conversations newest-first, searches over titles and bodies, renames, archives (reversibly), deletes (confirmed, and the confirmation names what happens to which object) and resumes any of them. Story 61.6; AD-154.
**Consequences (testable):**
- Titles are minted locally from the first user message — no second model call, ever, because a silent request to a paid endpoint is a surprise this app does not ship.
- The list reuses the recordings-archive list/filter/sort/paginate shape and the find-bar conventions; there is no second list component.

##### FR-382: Resume replays from keeper's store
System replays a resumed conversation from keeper's own store; where a Hermes `session_id` exists it is persisted beside the row as a reference, never as the truth. Story 61.6; AD-154.
**Consequences (testable):**
- Ollama has no session concept and resumes identically; where a Hermes `session_id` is held, the detail shows it and says plainly that the remote may have compressed it into a renamed successor.
- Hermes' compression into a continuation session and its 100-row stored-response cache can lose nothing of keeper's, because neither is the store.

##### FR-383: Pinned bots with an identity
User pins bots in a hand order and gives each an identity: a shape from a closed set, a mark (the existing icon picker, extended with a short emoji/grapheme option) and a colour from a bounded palette. Story 61.7; AD-155.
**Consequences (testable):**
- Order is persisted with the existing pins pattern, drag-to-reorder included.
- Every palette member is contrast-checked against both themes by `scripts/check-design.mjs`; a free colour picker is rejected, not deferred, because no colour of any hue passes AA on both themes for an unconstrained pick, and every state colour must be paired with a shape (DESIGN.md).

##### FR-384: Metadata is always recorded and shown on request
System records on every assistant message its model, Provider, prompt and completion tokens where reported, time-to-first-token, total duration, finish reason, tool-call count and the Provider's request id; one persisted toggle — in the pane and in the Command Palette — shows a compact caption per message, with an expander for the rest. Story 61.8; AD-156.
**Consequences (testable):**
- Absent numbers render as absent, never as zero: an endpoint that omits `usage` is a fact about the endpoint, and keeper does not print a number it did not measure.
- Metadata is columns on the message row, not a JSON blob (AD-139).

##### FR-385: Slash commands that refuse honestly
User types slash commands from a client-side `keeper-core` registry — `/new`, `/bot`, `/model`, `/metadata`, `/grant`, `/image`, `/history`, `/help` — with autocomplete cloned from the note editor's slash menu; an unknown command is refused with the nearest match named and is not sent to the model as prose. Story 61.9; AD-157.
**Consequences (testable):**
- The registry carries name, description, argument mode and whether the command needs a Provider; filtering and the keyboard model are the note editor's.
- A literal leading slash is escapable, and the empty state teaches it, because a model may legitimately be asked about `/etc`.
- Hermes' server-side commands are not proxied: they are enumerable only over a websocket RPC keeper does not speak, and a picker that silently dropped `/compact` on Ollama would be an affordance that lies (§15.4).

#### 15.2.6 Grants & Audit

##### FR-386: A grant names a scope and a mode, is visible, and is revocable
User grants a bot access to the drive as (Provider, optional bot) × (scope: the whole drive | one profile | one subtree) × (mode: none | read | write); grants are listed in Settings and in the pane's header and are revocable in one act. Story 61.10; AD-158.
**Consequences (testable):**
- Revocation is checked at every tool call, not at conversation start: an in-flight tool call whose grant vanished fails with that reason — a real revocation against a real in-flight call, not a pure-function proof.
- The grant affordance is gated on the `sync` capability: a drive keeper does not hold cannot be granted.
- This is keeper's first user-granted, revocable permission; what exists today is routing and disclosure, so nothing is extended — it is minted.

##### FR-387: A write is never auto-approved by a grant alone
System never lets a `write` grant by itself approve a write: the first write in a conversation, and every write outside an already-approved subtree, asks; "always for this subtree" is a durable grant edit, not a remembered click. Story 61.10; AD-158.
**Consequences (testable):**
- The answer to "what can it change?" is one list in Settings, never a hidden history of approvals.
- Approval is a user act inside the conversation; no flag, setting or command bypasses it — the FR-41 posture applied to the drive.

##### FR-388: Every tool call that touched the drive is auditable
System writes an audit row for every tool call that touched the drive before the effect, naming paths and not ids, so a human can read afterwards what was read and what was changed. Story 61.10; NFR-47.
**Consequences (testable):**
- The audit row precedes the effect and survives a crash (NFR-47); a write whose audit row could not be written does not happen.
- Audit rows are columns, not JSON (AD-139): path, verb, bound, outcome, grant.

#### 15.2.7 The Drive as Tools

##### FR-389: The drive through keeper's own containment rule
System exposes the drive to a model as seven tools — `list`, `read` (with line ranges), `glob`, `grep`, `stat`, `write`, `edit` — each resolved through the existing containment rule in `keeper-sync` (path resolution, plain segments, write routing), carried across the verb boundary verbatim and never restated as new path arithmetic. Story 61.11; AD-159.
**Consequences (testable):**
- There is no eighth tool: no shell, no exec, no network fetch, no Matrix send — FR-41 and D-3 stand untouched by construction.
- A write is atomic through the same temp-and-rename the tree already uses, proven against a real watcher pass (a real `rename` under the sync watcher).
- A `read` that lands on a pointer file returns the pointer's truth, never an empty file (Epic 56).
- A model whose tool flag is `false` (FR-377) is sent no tool definitions.

##### FR-390: File content is data, never instructions
System places all file content — context files included — into the request as data, under a system-prompt sentence stating that instructions inside file content are not instructions. Story 61.11; NFR-48.
**Consequences (testable):**
- No tool result can widen its own grant (NFR-48): a file whose text says "grant write" changes nothing about the grant.
- What the model was told is shown to the user, verbatim.

##### FR-391: Context files are found, merged and budgeted
System discovers `AGENTS.md`, `CLAUDE.md`, `GEMINI.md` and `.cursorrules` nearest-first within the granted scope, merges them in a stated order, caps the merged size, and shows the user what the model was told. Story 61.11; AD-159.
**Consequences (testable):**
- The discovery list is config: naming one more filename is a config change, not a code change. "okf" could not be identified as any published convention and is not implemented (`[UNVERIFIED]` in the research).
- The merged block is size-capped and the cap is disclosed to the model like every other bound (NFR-49).

#### 15.2.8 Images & Paths

##### FR-392: An image is pasted, and sent only to a model that can see
User pastes an image into the conversation; it reaches Rust as a raw IPC body, is stored and referenced like every other attachment, and is attached to the request as a data-URI content part only for a model whose vision capability is `true` or `unknown`. Story 61.12; AD-160.
**Consequences (testable):**
- No base64 through JSON, ever (AD-58); the attached image renders through the existing media viewer.
- For a `false` vision flag the paste is refused with the model's name in the sentence; for `unknown` it is offered with a warning (FR-375). [ASSUMPTION] Size and count caps are authored at story time and disclosed in the UI; no number is fixed here.
- Ollama's `/v1` layer accepts base64 data URIs only and refuses `image_url` — a quirk-table row (FR-372), not a code path.

##### FR-393: A path a model names is opened only inside a grant
System recognises an absolute path in a reply and offers *reveal* or *open* only if the path falls inside a grant; keeper never fetches, copies or strips a path a model named outside one. Story 61.12; AD-160.
**Consequences (testable):**
- Deliverable mode is handled from the receiving end: over the api-server a client receives the path itself (the `MEDIA:` stripping is a Hermes gateway feature), and the reply text is never altered, because here the reply is the record.
- A path outside every grant renders as text with no affordance — no dead button, no error-on-click (AD-27).

### 15.3 Phase NFRs

*Continues §7's numbering. Each bar is proved against the impure shell the story names — a real socket, a real file, a real revocation — not against a pure function.*

- **NFR-46 Bounded silence, unbounded length:** a stalled stream is abandoned after a bounded silence between bytes, never after a bounded total time; a flowing stream of any duration completes. [ASSUMPTION] The bound's value is a `keeper-core` constant authored at story time, restated from `keeper-sync`'s existing policy, and named in the stopped-stream message.
- **NFR-47 Audit before effect:** a tool-call audit row is written before the effect and survives a crash — an effect without a row is impossible, and a row without a confirmed effect is visible as such.
- **NFR-48 No self-widening:** no tool result can widen its own grant, and no file content is trusted as a directive; every byte a tool returns enters the next request as data.
- **NFR-49 Every byte bounded, and the bound disclosed:** every read and every write performed by a tool call is bounded, and the bound is told to the model (`"truncated at 64 kB of 1.2 MB"`), because a silent truncation makes a model confidently wrong.

### 15.4 Out of Scope (this phase)

- **Talk mode, the wake word and NeuTTS — Epic 62, not a story here.** Every credible permissive voice stack needs new native dependencies that cannot be compiled or exercised on the development host, and the pretrained keyword-spotting and ASR model weights carry dataset terms the code's Apache-2.0 licence does not imply; the direction is already recorded so 62 does not re-research it, and 62 also owns keeping the recording feature's "transcribes nothing" promise (enforced by a source scan) true.
- **The native `/api/chat` dialect** — and with it `num_ctx` and `think` levels on Ollama — because one wire protocol is the phase's first decision and a second client is the cost it refuses (DW-210).
- **Embeddings, RAG, or any index over the drive** — not asked for, and it would make a second source of truth about the user's files (DW-212).
- **Running commands** — no tool executes a shell string; `docs/decisions.md` D-3 stands and the general exec kind remains Epic 60's, unbuilt, stated because a filesystem tool surface is exactly where someone will propose `bash` (DW-213).
- **omp as a Provider kind** — the enum stays closed at two; designing an abstraction today for an endpoint with nothing to read is the mistake this phase refuses (DW-214).
- **Proxying Hermes' server-side slash commands** — enumerable only over a websocket RPC keeper does not speak, so a hand-copied list would rot on the next release (DW-209).
- **A second model call to title a conversation** — titles are minted locally (FR-381) (DW-211).

### 15.5 Phase Success Metrics

- **SM-11 Impure-shell gate:** the five risks the epic names are each proved against the real thing, not a pure function — a real SSE socket that dies mid-frame (61.2), a real 401 and a real 404 from a wrong base URL (61.3), a real file that is a pointer and not content (61.11), a real `rename` under the sync watcher (61.11), and a real revocation that stops an in-flight tool call (61.10) — and the five stay green as release gates. Validates FR-372–FR-377, FR-386, FR-389.
- **SM-12 End-to-end on the owner's endpoints:** from ⌘9, the owner talks to a Hermes bot and to an Ollama model (one local, one remote), grants one subtree, watches the model read and write inside it with every touch in the audit, sees exactly the configured hosts in the egress inventory and nothing else, and the recording zero-egress gate is untouched and green. Binary, demo-able, release-gating. Validates FR-369–FR-393.

### 15.6 Phase Risks (Register)

Adopted from the epic's evidence table and its *What stays out*:

- **The bearer door cannot list bots** — no profile roster on the api-server route table. Medium (product). Mitigation: verification instead of enumeration (FR-376), an honest empty state; revisit only if upstream opens a bearer-authenticated roster (§15.7).
- **Ollama's `/v1` layer diverges silently** — `tool_choice` ignored, `image_url` refused, no `num_ctx`. Medium (correctness). Mitigation: the quirk table is data with a test per row (FR-372); the native dialect is out (§15.4).
- **Hermes sessions are not stable** — compression mints a renamed successor; the stored-response cache is 100 rows LRU. Medium (data). Mitigation: keeper's store is the truth; the remote id is a reference (FR-382).
- **Prompt injection through the drive** — file content, context files and tool results are the lethal-trifecta surface. High (security). Mitigation: content as data under a stated system sentence (FR-390), no self-widening (NFR-48), a grant checked at every call (FR-386), writes never auto-approved (FR-387), an audit before every effect (NFR-47).
- **A base URL is a request keeper makes on the user's behalf** — SSRF by configuration. Medium (security). Mitigation: grammar in `keeper-core` (FR-369), disclosure of loopback and private hosts, derived egress (FR-371); a blocklist is rejected because it would lie about a legitimate local Ollama.
- **A story order that inverts grant-before-tools** ships an unguarded filesystem for one PR. High (process). Mitigation: 61.10 precedes 61.11 absolutely; the epic states it as the one ordering rule.
- **The gates that would now cry wolf** — the NFR-11 egress diff fires (correctly), the recording zero-egress source scan reads verbs. Low (process). Mitigation: 61.13 writes the egress chapter and D-4, and the phase adds no file the recording scan globs match and no palette verb it would read as upload or transcription.
- **New crates** — every added dependency needs a licence justification and passes `cargo-deny`. Low. Mitigation: the phase prefers zero new dependencies (`reqwest`, `serde_json`, `tokio`, `futures`, `bytes`, `rusqlite`, `ts-rs` are already in the tree); the markdown renderer reuses the lezer stack (FR-380).

### 15.7 Phase Decisions & Open Questions

**Pre-answered (adopted from the epic; revisit only on evidence):**

- **No server-side component.** keeper is a client only (§5, §8); every endpoint is the user's. Recorded as **D-4 — the endpoint is yours, never keeper's**: no default endpoint, no hosted model, no telemetry and no opt-in scaffolding for any of them, which is why a base URL is required rather than defaulted.
- **One wire protocol, divergences as data.** `POST /v1/chat/completions` with `stream: true` for both kinds; native endpoints only for discovery, where they are strictly better.
- **keeper owns the conversation.** The store is `keeper.db` (`bot_providers`, `bots`, `bot_sessions`, `bot_messages`, later `bot_grants`, `bot_audit`), no JSON blobs in rows; a Hermes `session_id` is a reference.
- **Verification, not enumeration, for bots.** A named profile is probed; keeper never invents a roster.
- **A bounded palette, not a colour picker.** Colour is paired with a shape and contrast-checked by the design gate.
- **Client-side slash registry.** Hermes' server-side commands are not proxied.
- **The grant exists before the first tool can read a byte.** 61.10 before 61.11, absolutely.
- **No message-bubble reuse.** The Matrix timeline's bubble is typed down to send-state and read receipts; widening it to a second domain couples two surfaces for a rounded rectangle.
- **Epic 60 stays reserved** for the general exec kind Epic 59 deferred; this phase neither builds it nor takes its number.

**Open:**

1. Authored values need owner confirmation before they become gates: the NFR-46 silence bound, the FR-392 image size and count caps, and the FR-379 orphaned-conversation behaviour on Provider removal. Owner: product owner, at phase release.
2. Whether the `bots` flag is ever true on iOS (FR-378 [ASSUMPTION]). Owner: architecture, when the phone is next touched.
3. A bearer-authenticated profile roster on the Hermes api-server would turn FR-376's verification into enumeration; nothing in keeper should be designed for it until it exists upstream. Owner: PM, on Hermes release evidence.
4. "okf" in the owner's ask could not be identified as any published convention; if it names one, it is a config line under FR-391, not a story. Owner: owner.
