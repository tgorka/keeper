# Epic 10 Context: Notifications & Background Operation

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Make reliability the feature. keeper must post native macOS notifications straight from its own local, decrypting sync loop — never through any third-party or project-operated push infrastructure — so a backgrounded app can still be trusted. This epic delivers notifications within seconds of sync receipt, granular quieting (per-Chat and per-Network mute, mention-only mode, global do-not-disturb) that holds without suppressing unread accumulation, honest background operation (syncing with the window closed, opt-in launch-at-login and menu-bar presence, and a quit that truthfully stops sync), and click-through that lands the user in the exact Chat/Account/message. It also completes the bridge-health story from Epic 6 by wiring its state machine into the same notification pipeline. This is where the "reliability, not features" promise — the top cluster of competitor complaints — is either kept or broken.

## Stories

- Story 10.1: Native Notifications from the Sync Loop
- Story 10.2: Mutes, Mention-Only, and Do-Not-Disturb
- Story 10.3: Background Operation and Honest Quit
- Story 10.4: Click-Through and Bridge-Health Alerts

## Requirements & Constraints

- Notifications originate exclusively from the local decrypting sync loop. E2EE content is rendered only from that loop; no notification is ever routed through push infrastructure of any kind (project-operated or third-party). This is an egress-honesty invariant, not just a default.
- Notification latency bar: a native notification within 5 s of the event reaching the local sync loop, foreground or background (reliability bar, must hold in ≥99% of backgrounded test events).
- Preview toggle: with previews disabled, notifications show sender and Chat but no message content.
- Notifications group per Chat so a burst does not flood Notification Center.
- Mute (per Chat / per Network), mention-only mode (per Chat), and global DND all suppress notifications while the affected Chats keep updating in the inbox and keep accumulating unread state. Mute never touches unread. Mention-only notifies solely on mentions and replies-to-user.
- Muted rows carry a mute glyph; global DND toggle lives in the sidebar footer menu.
- Background operation: with the window closed the app syncs and notifies identically to foreground. Optional menu-bar presence keeps keeper reachable windowless. Dock badge shows unread count per its Setting (all unreads / mentions only / off).
- Launch-at-login is opt-in and off by default.
- Honest quit: ⌘Q fully stops sync, and Settings copy must say exactly that — no "push while quit" promise anywhere in UI or copy. Window close (⌘W) keeps syncing; only quit stops it.
- Click-through payload is `(account_id, room_id, event_id)`; clicking restores or summons the window and switches to the exact Chat and Account with the message in view, within the interaction-latency bar (Chat switch target ~150 ms).
- Bridge-health leg (completes FR-28 end to end): a Bridge Session drop must be surfaced and notified within 60 s. The notification copy is "Signal disconnected — re-link to keep receiving messages." (Network-named), and clicking it lands directly in that Bridge's re-login flow.
- MVP is click-through only — inline notification quick-reply is explicitly out of scope (v1.x).

## Technical Decisions

- All notification logic lives in `keeper-core::notify`, which consumes post-decryption events, applies mute/mention-only/DND rules, and posts via `tauri-plugin-notification`. Mute logic must not be duplicated in JS.
- Notification rules are persisted in settings and mapped to Matrix push rules where representable, evaluated locally otherwise. Rules must survive restarts. Settings live in `keeper.db` behind `keeper-core::settings` (no `tauri-plugin-store`, no `tauri-plugin-sql`); the autostart plugin backs launch-at-login.
- `notify` is a cross-account aggregator consuming per-account streams under `AccountManager`/`AccountHandle` supervision — no global mutable state; the only globally reachable handle is Tauri's `AppState`.
- Platform capabilities (notifier sink, dock/menu-bar glue) are reached through the `Platform` port so `keeper-core` stays platform-free; shell-side glue lives in the `keeper` Tauri crate.
- Bridge-health alerts ride the same `notify` pipeline rather than a separate path. The health state machine (healthy/degraded/disconnected) is owned by the bridges layer from Epic 6, Story 6.5; Epic 10 consumes its drop events.
- Voice/tone: state and Settings copy follow sentence case, no exclamation marks, honest state narration, Glossary-capitalized nouns; the quit-semantics copy is a specific honesty surface.

## UX & Interaction Patterns

- Notification content: sender + Chat + preview (preview omissible), grouped per Chat; click lands in the exact Chat and Account with the message in view.
- Bridge degraded/disconnected experience is persistent, never toast-only: within 60 s the Bridge card state flips, the sidebar Bridges dot rolls up the worst state, affected Chat rows get the health dot on the network badge, and the conversation shows a non-dismissible inline banner while unhealthy — plus one native notification. Ignoring the notification leaves all persistent states standing until the session is healthy again.
- Global DND toggle sits in the sidebar footer menu; mute/mention-only controls are reachable from the chat context menu, the detail panel per-chat controls, and the network chip menu. Single-key `m` opens the mute menu (mute / mention-only / unmute) when the list is focused.
- Settings surface a Notifications section (previews, mute defaults, DND) and dock-badge mode; the app-quit-vs-background honesty copy lives in Settings.

## Cross-Story Dependencies

- Stories 10.2–10.4 depend on 10.1 (the notification pipeline and rules engine).
- 10.1 depends on Epic 3 (the decrypting sync loop must exist so E2EE content can be rendered locally).
- 10.4's bridge-health leg depends on Epic 6 Story 6.5 (the Bridge health state machine); this split is deliberate — Epic 6 builds detection and in-app surfacing, Epic 10 adds the native-notification leg that completes FR-28.
