---
title: "PRD: keeper"
status: final
created: 2026-07-03
updated: 2026-07-03
---

# PRD: keeper

## 0. Document Purpose

This PRD defines the macOS text-first MVP of keeper, an open-source (Apache-2.0), client-only universal messenger built on Matrix. It is written for the downstream BMAD chain — UX design, architecture, and epic/story creation — and for contributors who need a single authoritative statement of what MVP includes, excludes, and must prove. It builds on, and does not duplicate, four upstream inputs: the product brief and its addendum (`_bmad-output/planning-artifacts/briefs/brief-keeper-2026-07-03/`), the stakeholder requirements (`_bmad-output/planning-artifacts/product-inputs.md`), and the technical and market research reports (`_bmad-output/planning-artifacts/research-technical-2026-07-03.md`, `research-market-2026-07-03.md`). Vocabulary is anchored in §3 Glossary; functional requirements are numbered FR-1 through FR-54 with testable consequences; cross-cutting NFRs are numbered NFR-1 through NFR-14. Inline `[ASSUMPTION]` tags mark inferences made without stakeholder confirmation and are indexed in §12. Technical constraints already locked by the owner (stack, SDK versions, IPC patterns, licensing firewall) live in the brief addendum and this PRD's `addendum.md`; this document states *what* keeper does, not *how*.

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

*FRs are numbered globally (FR-1 … FR-54). Every FR uses Glossary terms verbatim and carries testable consequences. "User" means the single macOS operator of the app.*

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
- **No mobile, no Windows/Linux in MVP.** macOS first; iPhone next after macOS proves the core.
- **No WhatsApp (or any Network) automation, broadcast, or bulk messaging — ever.** These trigger ban regimes and betray the user-safety posture.
- **No agent/AI send path in MVP.** The Approval Pane ships; the propose-only agent API/MCP is a post-MVP experiment behind a flag, gated on design-partner validation. Nothing in MVP may send without explicit user approval (FR-41).
- **No iMessage in MVP.** v1.x at earliest, only via the user's own Mac, labeled "advanced, may break on macOS updates."
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
- Mobile (iOS first), then Windows/Linux/Android/iPad on the same Rust core.
- Beeper Desktop API companion mode (reach On-Device Connection chats when Beeper Desktop is installed) — pragmatic add-on, never a foundation.
- Email network, AI-bot client, terminal client (owner's long-term network list).

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
