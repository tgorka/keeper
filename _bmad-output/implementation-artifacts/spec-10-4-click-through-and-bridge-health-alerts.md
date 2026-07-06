---
title: 'Click-Through and Bridge-Health Alerts'
type: 'feature'
created: '2026-07-06'
status: 'draft'
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

**Approach:** Carry a typed click-through target with every notification, and feed the existing `HealthAggregator` disconnect transition into the same `notify` pipeline. **RESOLVED (coordinator decision 2026-07-06, Option B):** keep `tauri-plugin-notification` as the backend. MVP click behavior is **summon + focus the app window** (macOS default activation — no per-notification click callback exists on desktop) plus **coarse view landing** driven by app-side "last notification target" state recorded at dispatch time: Message targets land on the Inbox, Bridge targets land on the Bridges view. Exact-message / exact-re-login deep landing via a click-capable backend (mac-notification-sys or UNUserNotificationCenter) is deferred to Epic 11 (signed .app exists there to validate it) — record that deferral in deferred-work.md.

## Boundaries & Constraints

**Always:**
- Notifications originate only from the local decrypting sync loop / local health machine; never any push infrastructure (egress-honesty). Reuse the `Platform::notify` port; keep mute/DND rules in `keeper-core`, never duplicated in JS.
- Message click-through payload is exactly `(account_id, room_id, event_id)` and is attached to every posted notification (`NotifyTarget::Message`) — the payload ships now even though MVP click handling is coarse. [AMENDED-B] A click restores/summons + focuses the main window (`show_main_window`, `tray.rs:39`, macOS default activation) and lands on the **Inbox view** (`primaryViewStore.setView("inbox")`). Exact Chat+Account+message landing (`roomsStore.requestFocus`) is deferred to Epic 11.
- [AMENDED-B] Per-notification exact click routing is deferred to Epic 11 (no desktop click callback in the kept backend). Coarse landing MAY use a "last notification target" recorded at dispatch to choose the view (Inbox vs Bridges); it must NEVER be presented or tested as exact-message routing.
- Bridge-health: post exactly ONE native notification on the transition **into** `Disconnected` per session; body copy is exactly `"{network_name} disconnected — re-link to keep receiving messages."` (Network-named). [AMENDED-B] A click summons+focuses the window and lands on the **Bridges view** (`primaryViewStore.setView("bridges")`); the persistent Story 6.5 surfaces route the user into the exact re-login. Exact re-login deep-landing deferred to Epic 11.
- The 60 s bar is satisfied by the existing health machine (`run_liveness_tick` ≤60 s + real-time mgmt-room notices, `bridges/health.rs:586,531`); the notify leg only reacts to transitions — do not add new polling.
- Bridge-health notifications respect global DND (consistent with Story 10.2's `NotifyConfig.dnd_enabled`); per-Chat/per-Network mute does NOT apply (bridge integrity ≠ chat noise). The persistent in-app surfaces from Story 6.5 (banner, dots, card state) stand regardless of the native notification.
- `keeper-core` stays platform-free: the OS notification, its click callback, window show/focus, and the Rust→frontend navigate event are shell (`keeper` crate) concerns reached through the `Platform` port. Commit on the current branch only.

**Block If:**
- ~~[RESOLVED 2026-07-06 — Option B]~~ The two prior blockers (no desktop click callback in the pinned backend; signed-bundle coupling to Epic 11) are resolved by scope decision: MVP ships summon+focus + coarse view landing on the kept backend, exact landing deferred to Epic 11. Do NOT re-raise these two conditions; they are settled.
- A genuinely new contradiction outside this decision (e.g. the kept backend cannot even post the bridge notification, or coarse landing conflicts with another frozen spec) still HALTs as usual.

**Never:**
- Never route notifications or health/badge state through any push service (egress + honest-quit invariant). No inline notification quick-reply (v1.x, MVP is click-through only).
- [AMENDED-B] Never present coarse view landing as exact-message routing (no fake "lands on the exact message" claims in UI/docs/tests). Never emit a native toast for the `Degraded` state — only the `Disconnected` transition notifies; Degraded keeps its persistent in-app surfaces only.
- Never re-notify while a session stays `Disconnected` (one alert per drop). Never create branches, push, or rewrite history.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Message click [AMENDED-B] | Notification clicked, window hidden | Window shown+focused (macOS activation); view→inbox (coarse landing) | — |
| Old notification click [AMENDED-B] | An earlier notification (not the newest) clicked | Same coarse behavior (summon+focus, view by last-target kind); exact per-notification routing deferred to Epic 11 | — |
| Bridge drop | Session transitions Healthy/Degraded → `Disconnected` | One native notification, body `"{network_name} disconnected — re-link to keep receiving messages."`; in-app surfaces already updated by 6.5 | notify port unset (headless) → honest no-op |
| Bridge alert click [AMENDED-B] | Disconnected notification clicked | Window shown+focused; view→bridges; Story 6.5 surfaces route into re-login | — |
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
- [ ] `src-tauri/crates/keeper/src/ipc.rs` + `lib.rs` -- [AMENDED-B] on app activation following a notification (kept backend, no click callback): `show_main_window` + emit a coarse navigate event from the "last notification target" kind recorded at dispatch -- the coarse click seam.
- [ ] `src/lib/ipc/gen/NotifyTarget.ts` + `client.ts` + `bridge-relink.ts` + `App.tsx` -- regenerated binding + navigate listener routing Message→inbox view, Bridge→bridges view (coarse; no `requestFocus` deep landing in MVP) -- frontend landing.
- [ ] Unit-test the I/O matrix edges: transition-only bridge dedup, DND suppression, Degraded→no-toast, target attach mapping at dispatch.

**Acceptance Criteria:**
- [AMENDED-B] Given a hidden window and a message notification, when the user clicks it, then the window is summoned+focused (macOS default activation) and the app shows the Inbox; exact Chat/Account/message landing is deferred to Epic 11 (deferred-work entry required).
- [AMENDED-B — dropped for MVP] Per-notification exact landing (older-notification targeting) requires a click-callback backend; deferred to Epic 11 with the backend decision.
- Given a Bridge Session transitions into `Disconnected`, when detection fires (≤60 s), then exactly one native notification with body `"{network_name} disconnected — re-link to keep receiving messages."` is posted (FR-28).
- [AMENDED-B] Given that bridge notification, when clicked, then the window is summoned+focused and the app shows the Bridges view (persistent Story 6.5 surfaces route the user to the exact re-login); exact re-login deep-landing deferred to Epic 11.
- Given global DND is on, when a bridge drops, then no native notification is posted while the in-app health surfaces still update.

## Spec Change Log

- 2026-07-06 (dev-auto re-arm): Escalation resolution (Option B) was recorded in the
  spec body + committed (`9377cba`), but the frozen-spec resolution flow intentionally
  leaves `status:` untouched (the orchestrator re-arms it on resume). This run was
  invoked standalone, so no orchestrator re-arm happened and `status:` was stale at
  `blocked`. Re-armed `status: blocked → draft` to route the now-unblocked plan back
  through step-02 for a coherence/readiness pass — the Option-B descope left a residual
  contradiction (the click-seam execution item still tagged `[blocked]`) and the
  `oversized` warning unresolved. No scope change; the Option B decision stands.
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

Status: resolved-pending-redrive

Prior blocked result superseded by the coordinator's Option B resolution (2026-07-06):
backend kept, summon+focus + coarse view landing in MVP, exact landing deferred to
Epic 11. The frozen intent-contract above now encodes the decision; re-drive per the
amended Tasks & Acceptance.
