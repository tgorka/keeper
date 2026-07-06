---
title: 'Click-Through and Bridge-Health Alerts'
type: 'feature'
created: '2026-07-06'
status: 'blocked'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-10-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** keeper posts native notifications (Story 10.1) but a click does nothing useful — it cannot land the user in the exact Chat/Account/message, and a Bridge Session drop is never natively notified (only the in-app surfaces from Story 6.5 exist). Story 10.4 must (a) make a notification click restore/summon the window and switch to the exact Chat + Account with the message in view via the `(account_id, room_id, event_id)` payload, and (b) complete FR-28 by notifying "‹Network› disconnected — re-link to keep receiving messages." within 60 s of a Bridge Session drop, with the click landing directly in that Bridge's re-login flow.

**Approach:** Carry a typed click-through target with every notification, route a click through the shell to the frontend's existing deep-link infra (`primaryViewStore.setView` + `roomsStore.requestFocus`) for message targets and to the bridge re-login flow for bridge targets, and feed the existing `HealthAggregator` disconnect transition into the same `notify` pipeline. **This is BLOCKED (see Block If): the notification backend pinned by the epic (`tauri-plugin-notification` 2.3.3) delivers NO desktop notification-click/action callback, so click-through cannot be built on it — resolving this requires a human architecture + scope decision.**

## Boundaries & Constraints

**Always:**
- Notifications originate only from the local decrypting sync loop / local health machine; never any push infrastructure (egress-honesty). Reuse the `Platform::notify` port; keep mute/DND rules in `keeper-core`, never duplicated in JS.
- Message click-through payload is exactly `(account_id, room_id, event_id)`. A click restores/summons + focuses the main window (`show_main_window`, `tray.rs:39`) and lands on the exact Chat + Account with the message in view — routed via `primaryViewStore.setView("inbox")` + `roomsStore.requestFocus({accountId, roomId, eventId})` (the Story 5.4 focus pattern, `rooms.ts:34-38,98`). Chat-switch target ~150 ms.
- Each posted notification must map back to *its own* target (not "the most recent notification"); clicking an old notification lands on that old notification's message.
- Bridge-health: post exactly ONE native notification on the transition **into** `Disconnected` per session; body copy is exactly `"{network_name} disconnected — re-link to keep receiving messages."` (Network-named). A click opens the re-login flow for that `(account_id, network_id)` (`primaryViewStore.setView("bridges")` + open `BridgeLoginSheet`/`bridge_login_start`).
- The 60 s bar is satisfied by the existing health machine (`run_liveness_tick` ≤60 s + real-time mgmt-room notices, `bridges/health.rs:586,531`); the notify leg only reacts to transitions — do not add new polling.
- Bridge-health notifications respect global DND (consistent with Story 10.2's `NotifyConfig.dnd_enabled`); per-Chat/per-Network mute does NOT apply (bridge integrity ≠ chat noise). The persistent in-app surfaces from Story 6.5 (banner, dots, card state) stand regardless of the native notification.
- `keeper-core` stays platform-free: the OS notification, its click callback, window show/focus, and the Rust→frontend navigate event are shell (`keeper` crate) concerns reached through the `Platform` port. Commit on the current branch only.

**Block If:**
- **[FIRED — see Design Notes/Auto Run Result]** The pinned notification backend cannot deliver a per-notification desktop click/action callback that lets the app route to *that* notification's target. `tauri-plugin-notification` 2.3.3 desktop `show()` is fire-and-forget (`notify-rust`); `action_type_id`/`register_action_types`/`onAction` are mobile-only. The only local mechanism is `mac-notification-sys` `wait_for_click(true)`, which (1) blocks the calling thread per notification, (2) shares the global ObjC delegate + `set_application` identity with `notify-rust` so mixing it with the plugin is fragile → implies replacing the shipped Story 10.1 notification backend, (3) carries no structured payload, and (4) needs a signed bundle identity for reliable click delivery. HALT — blocking condition `notification click-through backend decision required`.
- Enabling reliable click delivery requires code-signing / bundle-identity / entitlement changes that couple to and collide with the Epic 11 signing & notarization pipeline, and the ≥99% click-delivery reliability bar cannot be validated in the unsigned `tauri dev` build (notifications are attributed to `com.apple.Terminal` in dev). HALT — blocking condition `notification click delivery needs Epic 11 signing decision`.

**Never:**
- Never route notifications or health/badge state through any push service (egress + honest-quit invariant). No inline notification quick-reply (v1.x, MVP is click-through only).
- Never remember only the last notification's target and misroute older clicks. Never emit a native toast for the `Degraded` state — only the `Disconnected` transition notifies; Degraded keeps its persistent in-app surfaces only.
- Never re-notify while a session stays `Disconnected` (one alert per drop). Never create branches, push, or rewrite history.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Message click | Notification for `(acct, room, event)` clicked, window hidden | Window shown+focused; view→inbox; Chat+Account selected; message scrolled into view within ~150 ms | Room/event missing from view models → select room, best-effort scroll; log warn, no panic |
| Old notification click | An earlier notification (not the newest) clicked | Lands on *that* notification's `(acct, room, event)`, not the newest | as above |
| Bridge drop | Session transitions Healthy/Degraded → `Disconnected` | One native notification, body `"{network_name} disconnected — re-link to keep receiving messages."`; in-app surfaces already updated by 6.5 | notify port unset (headless) → honest no-op |
| Bridge alert click | Disconnected notification clicked | Window shown+focused; view→bridges; re-login flow for `(acct, network_id)` opens | login start error → surfaced in the sheet, no panic |
| Still disconnected | Session stays `Disconnected` across further observations | No additional native notification (dedup on transition) | — |
| Degraded | Session → `Degraded` | No native toast; persistent in-app surfaces only | — |
| DND on | Global DND enabled, bridge drops | Native bridge notification suppressed; in-app surfaces still update | — |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/platform.rs` -- `Platform::notify` (l.45) currently `(title, body)`. Extend to carry a typed click-through target: `NotifyTarget::{ Message { account_id, room_id, event_id }, Bridge { account_id, network_id }, None }` (new type in `vm.rs`, ts-rs exported). All `Platform` impls + test mocks update (mirrors the Story 10.3 `set_badge_count` port addition).
- `src-tauri/crates/keeper-core/src/notify.rs` -- `register_notify_handler` closure (l.356-455) already has `account_id`, `room_id`, `ev.event_id`. Attach `NotifyTarget::Message{..}` at the `platform.notify(...)` dispatch (l.313). Add a bridge-health notification entry point (see below) that posts `NotifyTarget::Bridge{..}`.
- `src-tauri/crates/keeper-core/src/bridges/health.rs` -- `HealthAggregator::observe` (l.362) already fires only on real changes. Add a second, notify-oriented consumer that, on a per-session transition **into** `Disconnected`, invokes `Platform::notify` with the exact copy + `NotifyTarget::Bridge`. Uses `MonitoredSession.network_name` (l.209) for the copy. Respect `NotifyConfig.dnd_enabled`. Dedup so only the transition notifies.
- `src-tauri/crates/keeper-core/src/account.rs` -- wire the notify-oriented health consumer under `subscribe_bridge_health` (l.1935) alongside the existing channel sink; thread the shared `Platform` + `NotifyConfig` in (both already available to `notify`).
- `src-tauri/crates/keeper/src/ipc.rs` -- `DesktopPlatform::notify` (l.322-337) currently posts via the tauri plugin (fire-and-forget). **BLOCKED:** to deliver a click callback carrying/routing the target it must move to `mac-notification-sys` (`wait_for_click(true)`, per-notification blocking thread, `set_application`) — the backend decision. On click → `show_main_window` + emit the navigate event.
- `src-tauri/crates/keeper/src/lib.rs` -- register the notification-click handler; emit a typed Tauri event (e.g. `notify://navigate`) carrying `NotifyTarget` to the `main` window after `show_main_window` (`tray.rs:39`).
- `src/lib/ipc/gen/NotifyTarget.ts` (new) + `src/lib/ipc/client.ts` -- regenerated ts-rs binding + a `listen`-based subscription for the navigate event.
- `src/lib/stores/rooms.ts` -- reuse `requestFocus` (l.98) / `FocusEvent` (l.34-38); `src/lib/stores/primary-view.ts` -- reuse `setView` (l.24).
- `src/lib/stores/bridge-relink.ts` (new, small) + `src/components/layout/conversation-pane.tsx` (`ConversationHealthBanner`/`BridgeLoginSheet`, l.422) and the bridges primary view -- a signal store so a navigate event can open the re-link sheet for `(account_id, network_id)` from outside the conversation pane.
- `src/App.tsx` (or a top-level hook) -- subscribe to the navigate event once; dispatch Message→inbox+focus, Bridge→bridges+relink.

## Tasks & Acceptance

**UNBLOCKED — coordinator decision recorded (2026-07-06): Option B.** Keep
`tauri-plugin-notification` as the backend. MVP click-through = **summon + focus the app
window** (the free macOS default click behavior) and land on the **Inbox** (message
notifications) or the **Bridges view** (bridge alerts) — best-effort coarse landing driven
by app-side state (e.g. a "last notification target" set at dispatch time), NOT by a
per-notification click callback. Exact-message / exact-re-login landing via a click-capable
backend (mac-notification-sys or UNUserNotificationCenter) is DEFERRED to Epic 11, where the
signed .app bundle exists to validate it; record that deferral in deferred-work.md. The
`NotifyTarget` payload shape, DND/mute policy, transition-only dedup, and copy stay exactly
as specified below. AC amendments per this decision are marked [AMENDED-B].

**Execution:**
- [ ] `src-tauri/crates/keeper-core/src/vm.rs` + `platform.rs` -- add `NotifyTarget` (ts-rs) and extend the `Platform::notify` port to carry it; update all impls/mocks -- typed click-through payload across the port.
- [ ] `src-tauri/crates/keeper-core/src/notify.rs` -- attach `NotifyTarget::Message` at dispatch; add a bridge-health notify entry that posts the exact copy + `NotifyTarget::Bridge`, gated on global DND -- both notification kinds carry a target.
- [ ] `src-tauri/crates/keeper-core/src/bridges/health.rs` + `account.rs` -- notify once on the transition into `Disconnected` using `network_name`; wire the consumer under `subscribe_bridge_health` -- FR-28 native leg within the existing 60 s machine.
- [ ] `src-tauri/crates/keeper/src/ipc.rs` + `lib.rs` -- **[blocked]** click-capable notification backend; on click `show_main_window` + emit the typed navigate event -- the click seam.
- [ ] `src/lib/ipc/gen/NotifyTarget.ts` + `client.ts` + `bridge-relink.ts` + `App.tsx` -- regenerated binding + navigate listener routing Message→inbox+`requestFocus`, Bridge→bridges+re-login -- frontend landing.
- [ ] Unit-test the I/O matrix edges: transition-only bridge dedup, DND suppression, Degraded→no-toast, per-notification target mapping.

**Acceptance Criteria:**
- [AMENDED-B] Given a hidden window and a message notification, when the user clicks it, then the window is summoned+focused (macOS default activation) and the app shows the Inbox; exact Chat/Account/message landing is deferred to Epic 11 (deferred-work entry required).
- [AMENDED-B — dropped for MVP] Per-notification exact landing (older-notification targeting) requires a click-callback backend; deferred to Epic 11 with the backend decision.
- Given a Bridge Session transitions into `Disconnected`, when detection fires (≤60 s), then exactly one native notification with body `"{network_name} disconnected — re-link to keep receiving messages."` is posted (FR-28).
- [AMENDED-B] Given that bridge notification, when clicked, then the window is summoned+focused and the app shows the Bridges view (persistent Story 6.5 surfaces route the user to the exact re-login); exact re-login deep-landing deferred to Epic 11.
- Given global DND is on, when a bridge drops, then no native notification is posted while the in-app health surfaces still update.

## Spec Change Log

- 2026-07-06 (coordinator, escalation resolution): **Option B chosen** from the three
  escalation options. Backend stays `tauri-plugin-notification`; MVP click behavior is
  summon+focus with coarse landing (Inbox / Bridges view); exact-target landing and the
  click-callback backend decision deferred to Epic 11. ACs amended and tagged [AMENDED-B].
  The `[blocked]` execution item (click seam in keeper/src/ipc.rs) is reduced to
  summon+focus + coarse-landing wiring; everything else in Execution stands.

## Review Triage Log

_No review pass — blocked at planning (step-02) before implementation._

## Design Notes

**Why this is blocked (evidence gathered during planning):**

1. `tauri-plugin-notification` 2.3.3 desktop path (`.cargo/.../tauri-plugin-notification-2.3.3/src/desktop.rs:26,150-210`) implements `show()` as a fire-and-forget wrapper over `notify_rust::Notification` — it forwards only title/body/icon/sound and returns `()`. There is **no click/action callback on desktop**.
2. `action_type_id` (`src/models.rs:159`), `register_action_types`, and JS `onAction`/`onNotificationReceived` are **mobile-only** (`src/mobile.rs`); the desktop `show()` silently drops action data. Confirmed the app posts via this exact path today: `DesktopPlatform::notify` → `app.notification().builder().title().body().show()` (`ipc.rs:322-337`).
3. The only local way to obtain a desktop notification-click on macOS is `mac-notification-sys` 0.6.15 `send_notification(..., Notification::wait_for_click(true))` → returns `NotificationResponse::Click`. But: it **blocks the calling thread** until the user interacts (or the notification is cleared — ignored notifications can block indefinitely → thread-lifetime concern), carries **no structured payload** (the target must be routed via a captured closure / shell-side id→target map), and `ensure_application_set()` + the shared ObjC delegate mean mixing it with `notify-rust` (the plugin's backend) is fragile — so click-through effectively requires **replacing the Story 10.1 notification backend** in the shell.
4. Reliable click delivery depends on a signed bundle identity (`set_application`); in `tauri dev` notifications are attributed to `com.apple.Terminal`, so click routing and the epic's **≥99% delivery reliability bar cannot be validated unattended** — this couples to the Epic 11 signing/notarization pipeline.

This contradicts the epic's explicit Technical Decision ("posts via `tauri-plugin-notification`") and is an architecture + scope + security-posture fork, so it is a **Block If**, not an unattended implementation detail.

**Decision options for the human (pick one, then re-run):**
- **A. Replace the desktop notification backend** with `mac-notification-sys` (`wait_for_click`, one bounded worker thread or a small pool, shell-side id→target routing). Accept the thread-lifetime + signed-bundle caveats; validates only in a real `.app` build. Both legs become buildable as specified.
- **B. Descope click-through for MVP** to "click summons+focuses the app" (the free macOS default) with exact-message/-bridge landing deferred until a click-capable backend/signed build lands in Epic 11. Amend the AC accordingly.
- **C. Split the story:** ship the bridge-health *notification generation* leg now (no click-through — the persistent in-app surfaces already let the user act), and defer BOTH click-through legs to a backend-decision story. (Note: even the bridge alert's "click lands in re-login" clause needs the same backend, so this still descopes both click clauses.)

The rest of the design (payload shape, DND/mute policy, transition-only bridge dedup, frontend `requestFocus`/`setView` routing, `network_name` copy) is settled and survives whichever option is chosen.

## Verification

**Commands (once unblocked):**
- `bun run check:rust` -- rustfmt clean + `clippy --all-targets -- -D warnings` passes (no `.unwrap()` in new prod paths).
- `bun run test:rust` -- new bridge transition-dedup / DND-suppression / target-mapping unit tests pass under cargo-nextest.
- `bun run check` -- biome + tsc + vitest pass, including the regenerated `NotifyTarget` binding.

**Manual checks (once unblocked, signed build):**
- Click a message notification while the window is hidden → app summons and lands on the exact Chat/Account/message.
- Drop a bridge session → one native "‹Network› disconnected — re-link…" notification; clicking opens that bridge's re-login.

## Auto Run Result

Status: blocked

**Blocking condition:** `notification click-through backend decision required`.

Story 10.4's two legs both require a per-notification **desktop notification-click callback** (message click → exact Chat/Account/message; bridge-disconnect click → that Bridge's re-login flow). The notification backend pinned by the epic and shipped in Story 10.1 — `tauri-plugin-notification` 2.3.3 — provides **no desktop click/action callback** (desktop `show()` is fire-and-forget via `notify-rust`; `action_type_id`/`onAction` are mobile-only). The only local alternative, `mac-notification-sys` `wait_for_click(true)`, blocks per notification, carries no payload, shares the global ObjC delegate/`set_application` identity with `notify-rust` (so it means replacing the shipped notification backend), and needs a signed bundle identity for reliable click delivery — which also makes the epic's ≥99% reliability bar unverifiable in the unsigned dev build and couples to the Epic 11 signing pipeline.

This is an architecture + scope decision that cannot be made unattended and contradicts the epic's explicit "posts via `tauri-plugin-notification`" Technical Decision. Halting rather than silently replacing a shipped foundation or silently under-delivering the acceptance criteria.

**What is settled (survives the decision):** the `NotifyTarget::{Message,Bridge}` payload shape and `Platform::notify` extension; the frontend routing via the existing `primaryViewStore.setView` + `roomsStore.requestFocus` deep-link infra (Story 5.4) and a small bridge-relink signal store; the bridge-health leg feeding the existing `HealthAggregator` disconnect transition into the same `notify` pipeline with copy `"{network_name} disconnected — re-link to keep receiving messages."`, notifying once per drop, respecting global DND, leaving Degraded and the persistent Story 6.5 surfaces untouched, and satisfying the 60 s bar via the existing health machine (no new polling).

**Decision required — one of:** (A) replace the desktop notification backend with `mac-notification-sys` (accept thread-lifetime + signed-bundle caveats); (B) descope MVP click-through to app-summon-only, deferring exact landing to Epic 11; or (C) split the story to ship bridge-health notification generation now and defer both click-through clauses. See Design Notes for details. Re-run `/bmad-dev-auto 10-4-click-through-and-bridge-health-alerts` after the decision (and any AC amendment) is recorded.

**Evidence:** `tauri-plugin-notification-2.3.3/src/desktop.rs:26,150-210` (fire-and-forget), `src/mobile.rs`/`src/models.rs:159` (actions mobile-only), `ipc.rs:322-337` (current post path), `mac-notification-sys-0.6.15/src/lib.rs:66-91` + `notification.rs:232-334` (blocking `wait_for_click`, `NotificationResponse`), `bridges/health.rs:209,362,586` (health machine + 60 s cadence), `rooms.ts:34-38,98` + `primary-view.ts:24` (frontend deep-link infra).
