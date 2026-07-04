---
stepsCompleted: [1, 2, 3, 4]
inputDocuments:
  - _bmad-output/planning-artifacts/prds/prd-keeper-2026-07-03/prd.md
  - _bmad-output/planning-artifacts/prds/prd-keeper-2026-07-03/addendum.md
  - _bmad-output/planning-artifacts/architecture/architecture-keeper-2026-07-03/ARCHITECTURE-SPINE.md
  - _bmad-output/planning-artifacts/ux-designs/ux-keeper-2026-07-03/DESIGN.md
  - _bmad-output/planning-artifacts/ux-designs/ux-keeper-2026-07-03/EXPERIENCE.md
  - docs/project-context.md
generated: 2026-07-03
mode: headless
storyCount: 63
epicCount: 11
---

# keeper - Epic Breakdown

## Overview

This document provides the complete epic and story breakdown for keeper, decomposing the PRD (FR-1–FR-54, NFR-1–NFR-14), the Architecture Spine (AD-1–AD-25), and the UX design contract (DESIGN.md + EXPERIENCE.md) into implementable stories. Epics are ordered for incremental delivery on the existing Tauri + React scaffold (matrix-sdk 0.18 already wired in). Epic 1 produces a usable walking skeleton and doubles as the PRD OQ-1 exit gate (SSS + timeline channel + send/receive in a release build). Every story is sized for one dev session, carries acceptance criteria mapped to FR/NFR ids, and lists explicit dependencies on previous stories only.

Post-MVP items (PRD §6.2) are flagged in the "Post-MVP — Not Storied" section at the end and deliberately have no stories.

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

## Post-MVP — Not Storied (Flagged Only)

Per PRD §5/§6.2 these are explicitly out of MVP; no stories exist for them and none may be smuggled in:

- Snooze/reminders; scheduled send (local-only framing when it comes); low-priority view; message-request filtering; labels/filtered views; note-to-self
- Bridge health dashboard + alert center (aggregate); bbctl full lifecycle supervision (auto-restart, log viewer)
- iMessage via user's own Mac; voice-note recording; notification quick-reply; typing-only privacy toggle; per-Chat stay-archived override; Beeper-style custom views ("Spacebar")
- Agent-proposed Drafts API/MCP (propose-only, behind a flag, after design-partner validation) — the Approval Pane's reserved proposer column is the only MVP concession
- Voice/video calls (Element Call embed); mobile/Windows/Linux; Beeper Desktop API companion mode
- Archive-at-rest encryption spike (AD-22); universal binaries

## Validation Summary

- **FR coverage:** FR-1–FR-54 all mapped (see FR Coverage Map); split FRs (FR-6, FR-17, FR-18, FR-28, FR-44) have both legs explicitly assigned to stories.
- **NFR coverage:** NFR-1–NFR-14 either designed into specific stories (NFR-5/8/9/10) or gated in Epic 11 (NFR-1–4, 11–13); NFR-14 is distributed (Stories 1.2, 3.2, 9.2, 9.3) per the UX accessibility floor.
- **UX-DR coverage:** UX-DR1–20 each referenced by at least one story's ACs.
- **Architecture compliance:** AD-6/7/8 land in Story 1.1 (keeper-core split in Epic 1 as required); AD-13 gate seeded in 1.6, completed 7.4/8.3; AD-14 seeded in 3.9, completed 8.1/8.2; databases/tables are created only by the first story needing them (keeper.db registry in 1.3, drafts in 7.1, outbox in 8.3, archive.db in 5.1).
- **Dependencies:** every story depends only on earlier stories; each epic functions without any later epic (Epic 6's FR-28 notification leg is an explicit, documented enhancement in Epic 10, with in-app surfacing complete inside Epic 6).
- **Sizing:** 63 stories across 11 epics, each scoped to a single dev session on the existing scaffold.




