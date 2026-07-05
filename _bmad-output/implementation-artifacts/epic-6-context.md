# Epic 6 Context: Bridge Management & First-Run Wizard

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

This epic is the reason keeper exists: it turns a raw user-owned Matrix + bridges stack into a product a normal person can operate. It delivers zero-config bridge discovery on the connected homeserver, fully native bridge login (QR and codes rendered in keeper's own UI, never in a bot chat), honest data-driven risk labeling so users know a network's ToS/ban exposure before connecting, session-health monitoring that surfaces a dying bridge within 60 seconds with one-click re-login, bbctl-driven self-hosted bridges for Beeper accounts, originating new chats via identifier resolution, and a First-Run Wizard that ties all of it together so first launch walks a user from zero to a bridged inbox. It directly answers the market's top complaint (silent bridge disconnects) and the setup-cliff barrier.

## Stories

- Story 6.1: Bridges Surface with Data-Driven Risk Tiers
- Story 6.2: Bridge Discovery
- Story 6.3: Native Bridge Login via Provisioning API
- Story 6.4: Bridge Bot Fallback Driver
- Story 6.5: Bridge Session Health and Re-Login Prompts
- Story 6.6: Start New Chats via Bridge
- Story 6.7: bbctl Integration for Beeper Self-Hosted Bridges
- Story 6.8: First-Run Wizard

## Requirements & Constraints

- **Discovery is zero-config.** Bridges on a connected homeserver must be found and listed with per-bridge status (configured / logged in / not logged in) without the user ever naming a bot's Matrix ID. Discovery runs per Account; cards are keyed Network × Account. An empty homeserver shows a "No bridges found on {homeserver}" state with a companion-stack docs link.
- **Login is native and path-agnostic.** Every login state (choosing method, waiting, QR, code entry, success, failure) renders as a distinct native state. The user must not be able to tell whether the provisioning API or programmatic bot-command driving powered a given login — the flow looks identical either way. Failures show the bridge's own error message verbatim; keeper never guesses at unparseable output.
- **The raw Bridge Bot chat is never hidden.** It stays reachable (card menu, detail panel) and is the manual escape hatch on failure.
- **Health surfaces within 60 seconds.** A bridge session state change reaching the homeserver must be reflected in keeper's UI within 60 s. Unhealthy state is persistent until resolved — never a dismissible toast that can be lost. The native-notification leg of this completes later (Story 10.4); this epic owns detection plus in-app surfacing.
- **Risk labeling is honest and data-driven.** Every network carries its risk tier at setup time and in the bridge list. Volatile-tier networks require explicit "I understand the risk" acknowledgment with plain-language ToS/ban copy; low-risk networks show only the label. Tier copy and caveats live in versioned data, never hardcoded, so guidance updates without UI rework.
- **bbctl is optional.** Self-hosted-bridge support for Beeper accounts is launch-on-demand register/run with status surfacing only. When bbctl is absent, offer guided install instructions and keep everything else fully functional. Auto-restart supervision and a log viewer are explicitly out of scope (v1.x).
- **New-chat resolves before opening.** Identifier resolution (phone / username / Matrix ID) runs through the bridge with a visible resolving state; failures keep the input for correction rather than dismissing; networks lacking resolve support say so upfront rather than failing late.
- **The Wizard is a path, not a gate.** Every step is skippable (Skip for now; Esc asks once), and the Wizard is re-enterable from Settings. A prepared-homeserver user must reach an inbox with ≥ 1 bridged network logged in without leaving the Wizard or reading external docs. Users without a homeserver get an honest fork (companion-stack docs → managed-host pointers → Beeper path) with no fake sign-up.

## Technical Decisions

- **Two transports behind one trait.** A `BridgeTransport` trait has exactly two impls: `Provisioning` (drives the bridgev2 HTTP provisioning JSON state machine into native login states) and `BotDriver` (sends and parses Bridge Bot commands programmatically, with timeouts). All bridge operations (login, list-logins, logout, set-relay) go through this trait so the two paths are behaviorally interchangeable.
- **Three-source discovery, merged.** Discovery combines (a) `GET /_matrix/client/v3/thirdparty/protocols`, (b) a data-driven known-bot MXID probe registry, and (c) a scan of existing bot DMs / portal rooms. Provisioning API base-URL resolution per deployment (config key + probe order) is an implementation detail inside this transport layer.
- **Health is a per-session state machine.** Three states (healthy / degraded / disconnected), fed by bridgev2 state events with a bot-ping fallback.
- **Risk tiers live in repo data.** `crates/keeper-core/data/` holds versioned JSON: risk tiers, coupling caveats, and the known-bot registry, consumed by the core. The tiers are: **Low risk** (Matrix native, Telegram, Google Messages/Chat/Voice — recommend by default), **Maintenance-heavy** (Signal, WhatsApp personal, Discord, Slack — default-on with disclosure), **Volatile / opt-in** (Instagram, Messenger, LinkedIn, X Chat — explicit ToS/ban warning, expect login friction), **Conditional** (iMessage on the user's own Mac only), and **Out of scope** (iMessage without a Mac, official X DM API, WeChat — do not promise).
- **Bridge code lives in `crates/keeper-core/src/bridges/`** (discovery.rs, transport/{provisioning,bot}.rs, health.rs, bbctl.rs), with wizard UI in the frontend `features/` layer.
- **bbctl is a Tauri sidecar.** An Apache-2.0 Go binary shipped per-arch, launched on demand via `exec` with parsed output.
- **Beeper account surface caveat.** A Beeper account sees Matrix-native chats + Beeper Cloud bridge rooms + bbctl self-hosted bridge rooms on matrix.beeper.com; it does *not* see On-Device Connection chats. Relevant when discovery/health runs against a Beeper account (Story 6.7 depends on the Epic 2 Beeper account work).

## UX & Interaction Patterns

- **Bridge card** (one per Network × Account): network glyph, name, risk-tier badge, health dot + state word (Connected / Action needed / Disconnected / Not set up), last-checked time, and a primary action (Connect / Re-link / Manage). Unhealthy cards get a left 3px disconnected-red edge. A Manage menu exposes Re-link, Log out, Open Bridge Bot chat, View sessions.
- **Risk-tier badges** are semantic: secondary badge for low, degraded-tinted outline for maintenance-heavy, filled disconnected-red for volatile. Volatile connect uses an `AlertDialog` gate with the tier copy and an explicit "I understand the risk — connect".
- **Bridge login stepper** is a `Sheet` over Bridges/Wizard rendering the state machine: choosing method → waiting → QR panel *or* code-entry `InputGroup` → success (dot turns healthy-green, auto-advance ~1.5 s) → failure (bridge's error verbatim + Retry + "Open Bridge Bot chat" escape). QR sits on a mandatory white card ≥ 240px with quiet zone in *both* themes (white is required for scannability), a per-network instruction line, and a live state word; QR expiry regenerates in place with a subtle "QR refreshed" note.
- **Health surfacing (unhealthy)** is multi-surface and persistent: card state flips (dot pulses twice then steady), the sidebar Bridges entry rolls up the worst state, affected chat rows get a health dot, and affected conversations show a non-dismissible inline banner ("Signal disconnected — messages may not arrive. Re-link") linking straight into the re-login flow for that exact bridge.
- **New chat dialog** (`⌘N`): pick Network + Account (default last used), enter identifier, resolve with a visible resolving state, then open the chat with composer focused. Inline "Not found on {Network}" on failure with input retained.
- **bbctl panel** ("Run your own bridge", Beeper accounts only): pick Network → log-free progress stepper drives register/run → bridge joins the list.
- **Wizard stepper**: Welcome → Add Account (three tabs: Homeserver login / OIDC / Beeper, reusing Epic 1–2 flows; honest no-homeserver fork) → Bridge discovery (found list with tier badges) → per-Bridge login (reuses the login stepper) → Done (lands in Inbox). Progress dots, no lock-in, Skip on every step, Esc asks once. On first run with no accounts the Wizard replaces the whole frame; skipping everything lands in an empty Inbox with an "Add an account to start" card.
- **Sidebar** Bridges entry lives at `⌘4` and carries the worst-state health roll-up dot.

## Cross-Story Dependencies

- 6.1 depends on Epic 2 (accounts). 6.2 depends on 6.1. 6.3 depends on 6.2. 6.4, 6.5, and 6.6 each depend on 6.3.
- 6.4 layers the `BotDriver` transport onto the same stepper states 6.3 established (they must be indistinguishable).
- 6.7 depends on 6.3 and on Story 2.3 (Beeper Account).
- 6.8 (Wizard) reuses the login flows/stepper from 6.2, 6.3, and 6.4, plus the Add-Account flows from Epics 1–2 — it should compose existing components, not reimplement them.
- 6.5 completes only its detection + in-app surfacing here; the native-notification leg rides the notification pipeline delivered in Story 10.4.
